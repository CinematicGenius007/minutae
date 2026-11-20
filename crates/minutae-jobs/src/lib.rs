//! minutae-jobs — persistent job runner over the `jobs` table (JOB-01).
//!
//! - Jobs survive restarts: state lives in SQLite (`minutae-store`).
//! - Crash recovery: `running` rows go back to `queued` on [`JobRunner::start`].
//! - Per-kind concurrency limits and attempt budgets ([`KindPolicy`]).
//! - Retry with exponential backoff for retryable [`FailureKind`]s; others fail fast.
//! - Cancel (queued or running), progress, and state changes broadcast as [`JobUpdate`].
//! - Handlers can chain follow-up jobs through [`JobContext::enqueue`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use minutae_core::{Failure, FailureKind, JobId, MeetingId, Progress, Stage};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, watch};
use tokio::task::AbortHandle;

pub use minutae_store::{JobKind, JobRecord, JobState, NewJob, Store, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("job runner is shut down")]
    ShutDown,
    /// The job asking to enqueue a follow-up was cancelled.
    #[error("job was cancelled")]
    Cancelled,
}

/// A job handler. Return `Ok(())` on success or a classified [`Failure`].
#[async_trait]
pub trait JobHandler: Send + Sync + 'static {
    async fn run(&self, job: JobRecord, ctx: JobContext) -> Result<(), Failure>;
}

/// Failure kinds worth retrying. Everything else (not installed, not signed in,
/// permission denied, model missing, cancelled…) needs the user and fails fast.
pub fn is_retryable(kind: FailureKind) -> bool {
    matches!(
        kind,
        FailureKind::RateLimited
            | FailureKind::Timeout
            | FailureKind::Unavailable
            | FailureKind::InvalidOutput
            | FailureKind::Io
            | FailureKind::Internal
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindPolicy {
    pub concurrency: usize,
    pub max_attempts: u32,
    /// Delay before attempt 2; doubles each attempt.
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl KindPolicy {
    pub fn defaults(kind: JobKind) -> Self {
        Self {
            concurrency: match kind {
                JobKind::Summarize | JobKind::Export => 2,
                _ => 1,
            },
            max_attempts: kind.default_max_attempts(),
            base_delay: Duration::from_secs(30),
            max_delay: Duration::from_secs(15 * 60),
        }
    }

    /// Backoff after `attempts` failed attempts (1-based).
    pub fn backoff(&self, attempts: u32) -> Duration {
        let exp = attempts.saturating_sub(1).min(20);
        self.base_delay
            .saturating_mul(1u32 << exp)
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub policies: HashMap<JobKind, KindPolicy>,
    /// Upper bound on how long the scheduler sleeps between scans.
    pub poll_interval: Duration,
    /// Minimum gap between progress writes to the database per job.
    pub progress_persist_interval: Duration,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            policies: JobKind::ALL
                .iter()
                .map(|k| (*k, KindPolicy::defaults(*k)))
                .collect(),
            poll_interval: Duration::from_secs(1),
            progress_persist_interval: Duration::from_millis(500),
        }
    }
}

impl RunnerConfig {
    pub fn policy(&self, kind: JobKind) -> KindPolicy {
        self.policies
            .get(&kind)
            .copied()
            .unwrap_or_else(|| KindPolicy::defaults(kind))
    }

    pub fn with_policy(mut self, kind: JobKind, p: KindPolicy) -> Self {
        self.policies.insert(kind, p);
        self
    }
}

/// Broadcast on every state change and progress tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobUpdate {
    pub job_id: JobId,
    pub kind: JobKind,
    pub meeting_id: Option<MeetingId>,
    pub state: JobState,
    pub attempts: u32,
    pub max_attempts: u32,
    pub progress: Option<Progress>,
    pub failure: Option<Failure>,
    /// Set when the job was re-queued after a retryable failure.
    pub retry_at: Option<DateTime<Utc>>,
}

impl JobUpdate {
    fn of(job: &JobRecord, state: JobState) -> Self {
        Self {
            job_id: job.id,
            kind: job.kind,
            meeting_id: job.meeting_id,
            state,
            attempts: job.attempts,
            max_attempts: job.max_attempts,
            progress: None,
            failure: None,
            retry_at: None,
        }
    }
}

/// Passed to a handler for one run.
#[derive(Clone)]
pub struct JobContext {
    inner: Arc<Inner>,
    job: Arc<JobRecord>,
    cancel: watch::Receiver<bool>,
    last_persist: Arc<Mutex<(Option<Instant>, Option<Stage>)>>,
    /// Follow-ups enqueued by this run; cancelled again if the job ends up cancelled.
    follow_ups: Arc<Mutex<Vec<JobId>>>,
}

impl JobContext {
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn job(&self) -> &JobRecord {
        &self.job
    }

    /// Report progress. Broadcast immediately; persisted (throttled) for the list view.
    pub fn progress(&self, stage: Stage, fraction: f32, eta_seconds: Option<u32>) {
        let p = Progress {
            meeting_id: self.job.meeting_id,
            stage,
            fraction: fraction.clamp(0.0, 1.0),
            eta_seconds,
        };
        let mut u = JobUpdate::of(&self.job, JobState::Running);
        u.progress = Some(p.clone());
        let _ = self.inner.events.send(u);

        let persist = {
            let mut lp = self.last_persist.lock().unwrap_or_else(|e| e.into_inner());
            let due = match lp.0 {
                None => true,
                Some(t) => {
                    t.elapsed() >= self.inner.config.progress_persist_interval
                        || lp.1 != Some(stage)
                        || p.fraction >= 1.0
                }
            };
            if due {
                *lp = (Some(Instant::now()), Some(stage));
            }
            due
        };
        if persist {
            let store = self.inner.store.clone();
            let id = self.job.id;
            tokio::task::spawn_blocking(move || {
                if let Err(e) = store.set_job_progress(id, &p) {
                    tracing::warn!(error = %e, "persist job progress");
                }
            });
        }
    }

    /// Enqueue a follow-up job (e.g. transcribe → diarize → summarize).
    /// Fails with [`JobError::Cancelled`] when this job was cancelled; a follow-up enqueued
    /// just before a cancel lands is cancelled when the job finishes.
    pub async fn enqueue(&self, job: NewJob) -> Result<JobRecord, JobError> {
        if self.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let rec = JobRunner {
            inner: self.inner.clone(),
        }
        .enqueue_job(job)
        .await?;
        self.follow_ups
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(rec.id);
        Ok(rec)
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// Resolves when the job is cancelled. Handlers doing blocking work can poll
    /// [`JobContext::is_cancelled`] instead; async handlers are also aborted.
    pub async fn cancelled(&self) {
        let mut rx = self.cancel.clone();
        let _ = rx.wait_for(|c| *c).await;
    }
}

struct Running {
    kind: JobKind,
    cancel: watch::Sender<bool>,
    /// None while the job is reserved (being claimed) but its task is not spawned yet.
    abort: Option<AbortHandle>,
}

struct Inner {
    store: Store,
    handlers: HashMap<JobKind, Arc<dyn JobHandler>>,
    config: RunnerConfig,
    events: broadcast::Sender<JobUpdate>,
    wake: Notify,
    running: Mutex<HashMap<JobId, Running>>,
    shutdown: watch::Sender<bool>,
    started: Mutex<bool>,
}

/// The job runner. Cheap to clone.
#[derive(Clone)]
pub struct JobRunner {
    inner: Arc<Inner>,
}

impl JobRunner {
    pub fn new(
        store: Store,
        handlers: impl IntoIterator<Item = (JobKind, Arc<dyn JobHandler>)>,
    ) -> Self {
        Self::with_config(store, handlers, RunnerConfig::default())
    }

    pub fn with_config(
        store: Store,
        handlers: impl IntoIterator<Item = (JobKind, Arc<dyn JobHandler>)>,
        config: RunnerConfig,
    ) -> Self {
        let (events, _) = broadcast::channel(512);
        Self {
            inner: Arc::new(Inner {
                store,
                handlers: handlers.into_iter().collect(),
                config,
                events,
                wake: Notify::new(),
                running: Mutex::new(HashMap::new()),
                shutdown: watch::channel(false).0,
                started: Mutex::new(false),
            }),
        }
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub fn subscribe(&self) -> broadcast::Receiver<JobUpdate> {
        self.inner.events.subscribe()
    }

    /// Recover interrupted jobs and start scheduling. Returns how many were recovered.
    /// Calling it twice is a no-op.
    pub async fn start(&self) -> Result<usize, JobError> {
        {
            let mut started = self.inner.started.lock().unwrap_or_else(|e| e.into_inner());
            if *started {
                return Ok(0);
            }
            *started = true;
        }
        let recovered = self.inner.store.call(|s| s.recover_running_jobs()).await?;
        if recovered > 0 {
            tracing::info!(recovered, "re-queued interrupted jobs");
        }
        let inner = self.inner.clone();
        tokio::spawn(scheduler(inner));
        Ok(recovered)
    }

    /// Enqueue with the kind's default policy.
    pub async fn enqueue(
        &self,
        kind: JobKind,
        payload: serde_json::Value,
        meeting_id: Option<MeetingId>,
    ) -> Result<JobId, JobError> {
        Ok(self
            .enqueue_job(NewJob::new(kind, payload, meeting_id))
            .await?
            .id)
    }

    pub async fn enqueue_job(&self, mut job: NewJob) -> Result<JobRecord, JobError> {
        if *self.inner.shutdown.borrow() {
            return Err(JobError::ShutDown);
        }
        if job.max_attempts.is_none() {
            job.max_attempts = Some(self.inner.config.policy(job.kind).max_attempts);
        }
        let rec = self.inner.store.call(move |s| s.insert_job(&job)).await?;
        let _ = self
            .inner
            .events
            .send(JobUpdate::of(&rec, JobState::Queued));
        self.inner.wake.notify_one();
        Ok(rec)
    }

    /// Cancel a queued or running job. Returns false if it had already finished.
    pub async fn cancel(&self, id: JobId) -> Result<bool, JobError> {
        let changed = self.inner.store.call(move |s| s.cancel_job(id)).await?;
        if !changed {
            return Ok(false);
        }
        let was_running = {
            let running = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
            match running.get(&id) {
                Some(r) => {
                    // A reserved job sees the flag in `spawn_job` and never starts.
                    let _ = r.cancel.send(true);
                    if let Some(a) = &r.abort {
                        a.abort();
                    }
                    true
                }
                None => false,
            }
        };
        if !was_running {
            // Running jobs emit their Cancelled update from `finish`.
            if let Some(rec) = self.inner.store.call(move |s| s.get_job(id)).await? {
                let _ = self
                    .inner
                    .events
                    .send(JobUpdate::of(&rec, JobState::Cancelled));
            }
        }
        self.inner.wake.notify_one();
        Ok(true)
    }

    /// Re-queue a failed or cancelled job with a fresh attempt budget ("Retry" button).
    pub async fn retry(&self, id: JobId) -> Result<bool, JobError> {
        let ok = self.inner.store.call(move |s| s.requeue_job(id)).await?;
        if ok {
            if let Some(rec) = self.inner.store.call(move |s| s.get_job(id)).await? {
                let _ = self
                    .inner
                    .events
                    .send(JobUpdate::of(&rec, JobState::Queued));
            }
            self.inner.wake.notify_one();
        }
        Ok(ok)
    }

    /// Number of jobs currently executing.
    pub fn running_count(&self) -> usize {
        self.inner
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Stop scheduling, abort running jobs and put them back in the queue
    /// (their attempt is not counted) so the next start resumes them.
    pub async fn shutdown(&self) {
        let _ = self.inner.shutdown.send(true);
        self.inner.wake.notify_one();
        let ids: Vec<JobId> = {
            let running = self.inner.running.lock().unwrap_or_else(|e| e.into_inner());
            for r in running.values() {
                if let Some(a) = &r.abort {
                    a.abort();
                }
            }
            running.keys().copied().collect()
        };
        for id in ids {
            let _ = self.inner.store.call(move |s| s.release_job(id)).await;
        }
    }
}

async fn scheduler(inner: Arc<Inner>) {
    let mut shutdown = inner.shutdown.subscribe();
    loop {
        if *shutdown.borrow() {
            break;
        }
        let wait = match tick(&inner).await {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(error = %e, "job scheduler tick failed");
                inner.config.poll_interval
            }
        };
        tokio::select! {
            _ = inner.wake.notified() => {}
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown.changed() => {}
        }
    }
}

/// Start every due job that has a free slot. Returns how long to sleep.
async fn tick(inner: &Arc<Inner>) -> Result<Duration, JobError> {
    let now = Utc::now();
    let due = inner.store.call(move |s| s.due_jobs(now, 256)).await?;
    for job in due {
        if *inner.shutdown.borrow() {
            break;
        }
        let Some(handler) = inner.handlers.get(&job.kind).cloned() else {
            continue;
        };
        // Reserve the slot before claiming so a `cancel` racing the claim finds the job
        // in `running` and flags it instead of letting it start.
        let Some(cancel_rx) = reserve(inner, &job) else {
            continue;
        };
        let id = job.id;
        let claimed = match inner.store.call(move |s| s.claim_job(id)).await {
            Ok(c) => c,
            Err(e) => {
                unreserve(inner, &job);
                return Err(e.into());
            }
        };
        let Some(claimed) = claimed else {
            unreserve(inner, &job);
            continue;
        };
        spawn_job(inner, claimed, handler, cancel_rx);
    }
    let next = inner.store.call(|s| s.next_job_due()).await?;
    // Jobs already due but not started (slot busy / no handler) wait for a wake-up
    // (a job finishing or a new enqueue) or the poll interval, never a busy loop.
    let wait = match next.and_then(|t| (t - Utc::now()).to_std().ok()) {
        Some(d) if !d.is_zero() => d,
        _ => inner.config.poll_interval,
    };
    Ok(wait.clamp(Duration::from_millis(1), inner.config.poll_interval))
}

/// Takes a concurrency slot for `job` (registered in `running` without a task yet).
/// None when the job is already running or its kind has no free slot.
fn reserve(inner: &Arc<Inner>, job: &JobRecord) -> Option<watch::Receiver<bool>> {
    let policy = inner.config.policy(job.kind);
    let mut running = inner.running.lock().unwrap_or_else(|e| e.into_inner());
    if running.contains_key(&job.id)
        || running.values().filter(|r| r.kind == job.kind).count() >= policy.concurrency
    {
        return None;
    }
    let (cancel_tx, cancel_rx) = watch::channel(false);
    running.insert(
        job.id,
        Running {
            kind: job.kind,
            cancel: cancel_tx,
            abort: None,
        },
    );
    Some(cancel_rx)
}

/// Drops a reservation whose claim failed. If a cancel flagged it meanwhile, that cancel
/// left the `Cancelled` update to us.
fn unreserve(inner: &Arc<Inner>, job: &JobRecord) {
    let entry = inner
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job.id);
    if entry.is_some_and(|r| *r.cancel.borrow()) {
        let _ = inner.events.send(JobUpdate::of(job, JobState::Cancelled));
    }
}

fn spawn_job(
    inner: &Arc<Inner>,
    job: JobRecord,
    handler: Arc<dyn JobHandler>,
    cancel_rx: watch::Receiver<bool>,
) {
    let job = Arc::new(job);
    let follow_ups = Arc::new(Mutex::new(Vec::new()));
    let ctx = JobContext {
        inner: inner.clone(),
        job: job.clone(),
        cancel: cancel_rx,
        last_persist: Arc::new(Mutex::new((None, None))),
        follow_ups: follow_ups.clone(),
    };
    // Hold the lock while checking and spawning so neither `cancel` nor `finish` can
    // interleave with the registration.
    let mut running = inner.running.lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = running.get_mut(&job.id) else {
        return;
    };
    if *entry.cancel.borrow() {
        // Cancelled between claim and spawn: the store already says cancelled.
        running.remove(&job.id);
        drop(running);
        let _ = inner.events.send(JobUpdate::of(&job, JobState::Cancelled));
        inner.wake.notify_one();
        return;
    }
    if *inner.shutdown.borrow() {
        // Shut down while claiming: hand the job back for the next start.
        running.remove(&job.id);
        drop(running);
        let store = inner.store.clone();
        let id = job.id;
        tokio::spawn(async move {
            let _ = store.call(move |s| s.release_job(id)).await;
        });
        return;
    }
    let _ = inner.events.send(JobUpdate::of(&job, JobState::Running));
    let run_job = (*job).clone();
    let handle = tokio::spawn(async move { handler.run(run_job, ctx).await });
    entry.abort = Some(handle.abort_handle());
    drop(running);
    let inner = inner.clone();
    tokio::spawn(async move {
        let outcome = handle.await;
        finish(&inner, &job, outcome, &follow_ups).await;
        inner.wake.notify_one();
    });
}

/// Cancels follow-up jobs enqueued by a run that ended up cancelled.
async fn cancel_follow_ups(inner: &Arc<Inner>, follow_ups: &Mutex<Vec<JobId>>) {
    let ids = std::mem::take(&mut *follow_ups.lock().unwrap_or_else(|e| e.into_inner()));
    let runner = JobRunner {
        inner: inner.clone(),
    };
    for id in ids {
        if let Err(e) = Box::pin(runner.cancel(id)).await {
            tracing::warn!(error = %e, job = %id, "could not cancel follow-up job");
        }
    }
}

async fn finish(
    inner: &Arc<Inner>,
    job: &JobRecord,
    outcome: Result<Result<(), Failure>, tokio::task::JoinError>,
    follow_ups: &Mutex<Vec<JobId>>,
) {
    let entry = inner
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job.id);
    let cancelled = entry.as_ref().is_some_and(|r| *r.cancel.borrow());
    let shutting_down = *inner.shutdown.borrow();
    let id = job.id;

    let result = match outcome {
        _ if cancelled => {
            cancel_follow_ups(inner, follow_ups).await;
            let _ = inner.events.send(JobUpdate::of(job, JobState::Cancelled));
            return;
        }
        Err(e) if e.is_cancelled() => {
            if shutting_down {
                let _ = inner.store.call(move |s| s.release_job(id)).await;
            }
            return;
        }
        Err(e) => Err(Failure {
            kind: FailureKind::Internal,
            message: format!("The job crashed: {e}"),
            fix: Some("Retry".into()),
        }),
        Ok(r) => r,
    };

    let store_res = match result {
        Ok(()) => match inner.store.call(move |s| s.complete_job(id)).await {
            Ok(true) => {
                let _ = inner.events.send(JobUpdate::of(job, JobState::Succeeded));
                Ok(())
            }
            Ok(false) => {
                // Cancelled after the handler returned but before we recorded success:
                // its follow-ups must not run either. (A shutdown release re-queues instead.)
                let state = inner.store.call(move |s| s.get_job(id)).await;
                if matches!(state, Ok(Some(ref j)) if j.state == JobState::Cancelled) {
                    cancel_follow_ups(inner, follow_ups).await;
                }
                Ok(())
            }
            Err(e) => Err(e),
        },
        Err(failure) => {
            let policy = inner.config.policy(job.kind);
            let give_up = !is_retryable(failure.kind) || job.attempts >= job.max_attempts;
            if give_up {
                tracing::warn!(job = %id, kind = ?job.kind, failure = ?failure.kind, "job failed");
                let f = failure.clone();
                inner
                    .store
                    .call(move |s| s.fail_job(id, &f))
                    .await
                    .map(|ok| {
                        if ok {
                            let mut u = JobUpdate::of(job, JobState::Failed);
                            u.failure = Some(failure);
                            let _ = inner.events.send(u);
                        }
                    })
            } else {
                let delay = policy.backoff(job.attempts);
                let at = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
                tracing::info!(job = %id, kind = ?job.kind, attempt = job.attempts, ?delay, "job will retry");
                let f = failure.clone();
                inner
                    .store
                    .call(move |s| s.retry_job(id, &f, at))
                    .await
                    .map(|ok| {
                        if ok {
                            let mut u = JobUpdate::of(job, JobState::Queued);
                            u.failure = Some(failure);
                            u.retry_at = Some(at);
                            let _ = inner.events.send(u);
                        }
                    })
            }
        }
    };
    if let Err(e) = store_res {
        tracing::error!(error = %e, job = %id, "could not record job result");
    }
}

#[cfg(test)]
mod tests;
