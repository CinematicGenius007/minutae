use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use serde_json::json;

use super::*;

fn fast_config() -> RunnerConfig {
    let mut c = RunnerConfig {
        poll_interval: Duration::from_millis(50),
        ..Default::default()
    };
    for k in JobKind::ALL {
        let p = KindPolicy {
            base_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(40),
            ..KindPolicy::defaults(k)
        };
        c = c.with_policy(k, p);
    }
    c
}

fn failure(kind: FailureKind) -> Failure {
    Failure {
        kind,
        message: "nope".into(),
        fix: None,
    }
}

async fn wait_state(store: &Store, id: JobId, state: JobState) -> JobRecord {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let j = store.get_job(id).unwrap().unwrap();
        if j.state == state {
            return j;
        }
        assert!(
            Instant::now() < deadline,
            "job stuck in {:?}, wanted {state:?}",
            j.state
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Fails the first `fail_times` runs with `kind`, then succeeds.
struct Flaky {
    calls: AtomicU32,
    fail_times: u32,
    kind: FailureKind,
}

#[async_trait]
impl JobHandler for Flaky {
    async fn run(&self, _job: JobRecord, _ctx: JobContext) -> Result<(), Failure> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_times {
            Err(failure(self.kind))
        } else {
            Ok(())
        }
    }
}

fn flaky(fail_times: u32, kind: FailureKind) -> Arc<Flaky> {
    Arc::new(Flaky {
        calls: AtomicU32::new(0),
        fail_times,
        kind,
    })
}

#[tokio::test]
async fn retries_with_backoff_then_succeeds() {
    let store = Store::open_in_memory().unwrap();
    let h = flaky(2, FailureKind::Timeout);
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Summarize, h.clone() as Arc<dyn JobHandler>)],
        fast_config(),
    );
    let mut events = runner.subscribe();
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Summarize, json!({"x": 1}), None)
        .await
        .unwrap();
    let j = wait_state(&store, id, JobState::Succeeded).await;
    assert_eq!(j.attempts, 3);
    assert_eq!(h.calls.load(Ordering::SeqCst), 3);
    let mut retries = 0;
    while let Ok(u) = events.try_recv() {
        if u.state == JobState::Queued && u.retry_at.is_some() {
            retries += 1;
            assert_eq!(u.failure.as_ref().unwrap().kind, FailureKind::Timeout);
        }
    }
    assert_eq!(retries, 2);
}

#[tokio::test]
async fn exhausts_attempts() {
    let store = Store::open_in_memory().unwrap();
    let h = flaky(99, FailureKind::RateLimited);
    let cfg = fast_config();
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Transcribe, h.clone() as Arc<dyn JobHandler>)],
        cfg,
    );
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Transcribe, json!(null), None)
        .await
        .unwrap();
    let j = wait_state(&store, id, JobState::Failed).await;
    assert_eq!(j.attempts, 2); // transcribe default budget
    assert_eq!(j.last_error.unwrap().kind, FailureKind::RateLimited);
    // "Retry" button gives a fresh budget.
    assert!(runner.retry(id).await.unwrap());
    wait_state(&store, id, JobState::Failed).await;
    assert_eq!(h.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn non_retryable_fails_fast() {
    let store = Store::open_in_memory().unwrap();
    let h = flaky(99, FailureKind::NotLoggedIn);
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Summarize, h.clone() as Arc<dyn JobHandler>)],
        fast_config(),
    );
    let mut events = runner.subscribe();
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Summarize, json!(null), None)
        .await
        .unwrap();
    let j = wait_state(&store, id, JobState::Failed).await;
    assert_eq!(j.attempts, 1);
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
    let mut saw_failed = false;
    while let Ok(u) = events.try_recv() {
        saw_failed |= u.state == JobState::Failed
            && u.failure.as_ref().unwrap().kind == FailureKind::NotLoggedIn;
    }
    assert!(saw_failed);
}

#[tokio::test]
async fn recovers_running_jobs_after_crash() {
    let store = Store::open_in_memory().unwrap();
    // Simulate a previous process that claimed the job and died.
    let rec = store
        .insert_job(&NewJob::new(JobKind::Index, json!(null), None))
        .unwrap();
    store.claim_job(rec.id).unwrap().unwrap();
    assert_eq!(
        store.get_job(rec.id).unwrap().unwrap().state,
        JobState::Running
    );

    let h = flaky(0, FailureKind::Io);
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Index, h.clone() as Arc<dyn JobHandler>)],
        fast_config(),
    );
    assert_eq!(runner.start().await.unwrap(), 1);
    let j = wait_state(&store, rec.id, JobState::Succeeded).await;
    assert_eq!(j.attempts, 1, "interrupted attempt is not counted");
}

struct Slow {
    active: AtomicUsize,
    peak: AtomicUsize,
    done: AtomicUsize,
}

#[async_trait]
impl JobHandler for Slow {
    async fn run(&self, _job: JobRecord, _ctx: JobContext) -> Result<(), Failure> {
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.done.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn respects_per_kind_concurrency() {
    let store = Store::open_in_memory().unwrap();
    let summarize = Arc::new(Slow {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
    });
    let transcribe = Arc::new(Slow {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
    });
    let runner = JobRunner::with_config(
        store.clone(),
        [
            (JobKind::Summarize, summarize.clone() as Arc<dyn JobHandler>),
            (
                JobKind::Transcribe,
                transcribe.clone() as Arc<dyn JobHandler>,
            ),
        ],
        fast_config(),
    );
    let mut ids = Vec::new();
    for _ in 0..6 {
        ids.push(
            runner
                .enqueue(JobKind::Summarize, json!(null), None)
                .await
                .unwrap(),
        );
        ids.push(
            runner
                .enqueue(JobKind::Transcribe, json!(null), None)
                .await
                .unwrap(),
        );
    }
    runner.start().await.unwrap();
    for id in ids {
        wait_state(&store, id, JobState::Succeeded).await;
    }
    assert_eq!(summarize.peak.load(Ordering::SeqCst), 2);
    assert_eq!(transcribe.peak.load(Ordering::SeqCst), 1);
    assert_eq!(
        summarize.done.load(Ordering::SeqCst) + transcribe.done.load(Ordering::SeqCst),
        12
    );
}

struct Forever;

#[async_trait]
impl JobHandler for Forever {
    async fn run(&self, _job: JobRecord, ctx: JobContext) -> Result<(), Failure> {
        ctx.progress(Stage::Transcribing, 0.25, Some(10));
        std::future::pending::<()>().await;
        Ok(())
    }
}

#[tokio::test]
async fn cancels_running_and_queued_jobs() {
    let store = Store::open_in_memory().unwrap();
    let runner = JobRunner::with_config(
        store.clone(),
        [(
            JobKind::Transcribe,
            Arc::new(Forever) as Arc<dyn JobHandler>,
        )],
        fast_config(),
    );
    let mut events = runner.subscribe();
    runner.start().await.unwrap();
    let meeting = uuid::Uuid::new_v4();
    store
        .create_meeting(&minutae_core::Meeting {
            id: meeting,
            title: "m".into(),
            started_at: Utc::now(),
            ended_at: None,
            status: minutae_core::MeetingStatus::Processing,
            source_app: None,
            failure: None,
        })
        .unwrap();
    let a = runner
        .enqueue(JobKind::Transcribe, json!(null), Some(meeting))
        .await
        .unwrap();
    let b = runner
        .enqueue(JobKind::Transcribe, json!(null), None)
        .await
        .unwrap(); // waits for the slot
    wait_state(&store, a, JobState::Running).await;

    // Progress is broadcast and persisted.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let u = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        if u.job_id == a && u.progress.is_some() {
            assert_eq!(u.progress.unwrap().meeting_id, Some(meeting));
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.get_job(a).unwrap().unwrap().progress.is_none() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        store.list_meetings(10, 0).unwrap()[0]
            .progress
            .as_ref()
            .unwrap()
            .fraction,
        0.25
    );

    assert!(runner.cancel(b).await.unwrap());
    assert_eq!(
        store.get_job(b).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert!(runner.cancel(a).await.unwrap());
    wait_state(&store, a, JobState::Cancelled).await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while runner.running_count() > 0 {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(!runner.cancel(a).await.unwrap());
    // Stays cancelled (not overwritten by a late finish).
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        store.get_job(a).unwrap().unwrap().state,
        JobState::Cancelled
    );
}

struct Chain;

#[async_trait]
impl JobHandler for Chain {
    async fn run(&self, job: JobRecord, ctx: JobContext) -> Result<(), Failure> {
        if job.kind == JobKind::Transcribe {
            let next = NewJob::new(JobKind::Summarize, json!({"from": job.id}), job.meeting_id);
            ctx.enqueue(next).await.map_err(|e| Failure {
                kind: FailureKind::Internal,
                message: e.to_string(),
                fix: None,
            })?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn handlers_chain_follow_up_jobs() {
    let store = Store::open_in_memory().unwrap();
    let h: Arc<dyn JobHandler> = Arc::new(Chain);
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Transcribe, h.clone()), (JobKind::Summarize, h)],
        fast_config(),
    );
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Transcribe, json!(null), None)
        .await
        .unwrap();
    wait_state(&store, id, JobState::Succeeded).await;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let all = store.list_jobs(&[JobState::Succeeded], 10).unwrap();
        if let Some(s) = all.iter().find(|j| j.kind == JobKind::Summarize) {
            assert_eq!(s.payload["from"], json!(id.to_string()));
            break;
        }
        assert!(Instant::now() < deadline, "follow-up job never ran");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

struct Panics;

#[async_trait]
impl JobHandler for Panics {
    async fn run(&self, _job: JobRecord, _ctx: JobContext) -> Result<(), Failure> {
        panic!("boom")
    }
}

#[tokio::test]
async fn panicking_handler_is_classified_internal() {
    let store = Store::open_in_memory().unwrap();
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Export, Arc::new(Panics) as Arc<dyn JobHandler>)],
        fast_config(),
    );
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Export, json!(null), None)
        .await
        .unwrap();
    let j = wait_state(&store, id, JobState::Failed).await;
    assert_eq!(j.attempts, 3);
    assert_eq!(j.last_error.unwrap().kind, FailureKind::Internal);
}

#[tokio::test]
async fn shutdown_requeues_running_jobs() {
    let store = Store::open_in_memory().unwrap();
    let runner = JobRunner::with_config(
        store.clone(),
        [(JobKind::Diarize, Arc::new(Forever) as Arc<dyn JobHandler>)],
        fast_config(),
    );
    runner.start().await.unwrap();
    let id = runner
        .enqueue(JobKind::Diarize, json!(null), None)
        .await
        .unwrap();
    wait_state(&store, id, JobState::Running).await;
    runner.shutdown().await;
    let j = wait_state(&store, id, JobState::Queued).await;
    assert_eq!(j.attempts, 0);
    assert!(matches!(
        runner.enqueue(JobKind::Diarize, json!(null), None).await,
        Err(JobError::ShutDown)
    ));
}

#[test]
fn backoff_doubles_and_caps() {
    let p = KindPolicy {
        concurrency: 1,
        max_attempts: 5,
        base_delay: Duration::from_secs(30),
        max_delay: Duration::from_secs(100),
    };
    assert_eq!(p.backoff(1), Duration::from_secs(30));
    assert_eq!(p.backoff(2), Duration::from_secs(60));
    assert_eq!(p.backoff(3), Duration::from_secs(100));
    assert_eq!(p.backoff(40), Duration::from_secs(100));
}
