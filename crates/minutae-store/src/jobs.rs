//! Persistence for the job runner (`minutae-jobs`). The runner owns the
//! scheduling logic; these are the atomic state transitions it relies on.

use chrono::{DateTime, Utc};
use minutae_core::{Failure, JobId, MeetingId, Progress};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::meetings::kind_stage;
use crate::{Result, Store, enum_from, enum_str, from_ms, json_col, ms, now_ms, parse_uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Transcribe,
    Diarize,
    Summarize,
    Index,
    Export,
    Retention,
}

impl JobKind {
    pub const ALL: [JobKind; 6] = [
        JobKind::Transcribe,
        JobKind::Diarize,
        JobKind::Summarize,
        JobKind::Index,
        JobKind::Export,
        JobKind::Retention,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Transcribe => "transcribe",
            JobKind::Diarize => "diarize",
            JobKind::Summarize => "summarize",
            JobKind::Index => "index",
            JobKind::Export => "export",
            JobKind::Retention => "retention",
        }
    }

    /// Default attempt budget (matches the Swift runner).
    pub fn default_max_attempts(self) -> u32 {
        match self {
            JobKind::Transcribe | JobKind::Diarize => 2,
            JobKind::Retention => 1,
            _ => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: JobId,
    pub kind: JobKind,
    pub state: JobState,
    pub meeting_id: Option<MeetingId>,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub max_attempts: u32,
    pub run_after: DateTime<Utc>,
    pub last_error: Option<Failure>,
    pub progress: Option<Progress>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewJob {
    pub kind: JobKind,
    pub payload: serde_json::Value,
    pub meeting_id: Option<MeetingId>,
    /// `None` = `kind.default_max_attempts()`.
    pub max_attempts: Option<u32>,
    /// `None` = now.
    pub run_after: Option<DateTime<Utc>>,
}

impl NewJob {
    pub fn new(kind: JobKind, payload: serde_json::Value, meeting_id: Option<MeetingId>) -> Self {
        Self {
            kind,
            payload,
            meeting_id,
            max_attempts: None,
            run_after: None,
        }
    }
}

const COLS: &str = "id, kind, state, meeting_id, payload, attempts, max_attempts, run_after, last_error, \
                    stage, fraction, eta_seconds, created_at, updated_at";

fn job_row(r: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let kind: JobKind = enum_from(&r.get::<_, String>(1)?)?;
    let meeting_id = r
        .get::<_, Option<String>>(3)?
        .as_deref()
        .map(parse_uuid)
        .transpose()?;
    let stage: Option<String> = r.get(9)?;
    let fraction: Option<f64> = r.get(10)?;
    let stage = match stage {
        Some(s) => Some(enum_from(&s)?),
        None if fraction.is_some() => kind_stage(kind.as_str()),
        None => None,
    };
    Ok(JobRecord {
        id: parse_uuid(&r.get::<_, String>(0)?)?,
        kind,
        state: enum_from(&r.get::<_, String>(2)?)?,
        meeting_id,
        payload: json_col(&r.get::<_, String>(4)?)?,
        attempts: r.get(5)?,
        max_attempts: r.get(6)?,
        run_after: from_ms(r.get(7)?),
        last_error: r
            .get::<_, Option<String>>(8)?
            .as_deref()
            .map(json_col)
            .transpose()?,
        progress: stage.map(|stage| Progress {
            meeting_id,
            stage,
            fraction: fraction.unwrap_or(0.0) as f32,
            eta_seconds: r.get(11).ok().flatten(),
        }),
        created_at: from_ms(r.get(12)?),
        updated_at: from_ms(r.get(13)?),
    })
}

impl Store {
    /// Queue `job`. A meeting job identical to one already queued (same kind, meeting and
    /// payload) is not added again; the queued one is returned. That makes "queue the next
    /// stage" safe to repeat: a job that crashes after queueing its successor but before
    /// finishing reruns and queues it again, which would otherwise run the stage (and its
    /// LLM calls) twice.
    pub fn insert_job(&self, job: &NewJob) -> Result<JobRecord> {
        let id = uuid::Uuid::new_v4();
        let now = now_ms();
        let payload = serde_json::to_string(&job.payload)?;
        self.transaction(|c| {
            if let Some(mid) = job.meeting_id {
                let existing = c
                    .query_row(
                        &format!(
                            "SELECT {COLS} FROM jobs WHERE state = 'queued' AND kind = ?1
                             AND meeting_id = ?2 AND payload = ?3 ORDER BY created_at LIMIT 1"
                        ),
                        params![job.kind.as_str(), mid.to_string(), payload],
                        job_row,
                    )
                    .optional()?;
                if let Some(rec) = existing {
                    return Ok(rec);
                }
            }
            Ok(c.query_row(
                &format!(
                    "INSERT INTO jobs (id, kind, state, meeting_id, payload, attempts, max_attempts, run_after, created_at, updated_at)
                     VALUES (?1, ?2, 'queued', ?3, ?4, 0, ?5, ?6, ?7, ?7) RETURNING {COLS}"
                ),
                params![
                    id.to_string(),
                    job.kind.as_str(),
                    job.meeting_id.map(|m| m.to_string()),
                    payload,
                    job.max_attempts.unwrap_or_else(|| job.kind.default_max_attempts()).max(1),
                    job.run_after.map(ms).unwrap_or(now),
                    now
                ],
                job_row,
            )?)
        })
    }

    pub fn get_job(&self, id: JobId) -> Result<Option<JobRecord>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM jobs WHERE id = ?1"),
                [id.to_string()],
                job_row,
            )
            .optional()?)
        })
    }

    /// Jobs in any of `states` (all when empty), oldest first.
    pub fn list_jobs(&self, states: &[JobState], limit: u32) -> Result<Vec<JobRecord>> {
        let filter = if states.is_empty() {
            String::new()
        } else {
            let list: Vec<String> = states
                .iter()
                .map(|s| format!("'{}'", enum_str(s)))
                .collect();
            format!("WHERE state IN ({})", list.join(","))
        };
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM jobs {filter} ORDER BY created_at, rowid LIMIT ?1"
            ))?;
            let rows = stmt.query_map([limit], job_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn jobs_for_meeting(&self, meeting: MeetingId) -> Result<Vec<JobRecord>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {COLS} FROM jobs WHERE meeting_id = ?1 ORDER BY created_at, rowid"
            ))?;
            let rows = stmt.query_map([meeting.to_string()], job_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Queued jobs whose `run_after` has passed, oldest first.
    pub fn due_jobs(&self, now: DateTime<Utc>, limit: u32) -> Result<Vec<JobRecord>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {COLS} FROM jobs WHERE state = 'queued' AND run_after <= ?1 ORDER BY created_at, rowid LIMIT ?2"
            ))?;
            let rows = stmt.query_map(params![ms(now), limit], job_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Earliest `run_after` among queued jobs.
    pub fn next_job_due(&self) -> Result<Option<DateTime<Utc>>> {
        self.read(|c| {
            let v: Option<i64> = c.query_row(
                "SELECT MIN(run_after) FROM jobs WHERE state = 'queued'",
                [],
                |r| r.get(0),
            )?;
            Ok(v.map(from_ms))
        })
    }

    /// Atomically move a queued job to running and count the attempt.
    /// Returns `None` when another worker claimed it or it was cancelled.
    pub fn claim_job(&self, id: JobId) -> Result<Option<JobRecord>> {
        self.write(|c| {
            Ok(c.query_row(
                &format!(
                    "UPDATE jobs SET state = 'running', attempts = attempts + 1, updated_at = ?2,
                       stage = NULL, fraction = NULL, eta_seconds = NULL
                     WHERE id = ?1 AND state = 'queued' RETURNING {COLS}"
                ),
                params![id.to_string(), now_ms()],
                job_row,
            )
            .optional()?)
        })
    }

    /// Running → succeeded. Returns false if the job is no longer running (e.g. cancelled).
    pub fn complete_job(&self, id: JobId) -> Result<bool> {
        self.finish(id, "succeeded", None, None)
    }

    /// Running → failed with the classified failure.
    pub fn fail_job(&self, id: JobId, failure: &Failure) -> Result<bool> {
        self.finish(id, "failed", Some(failure), None)
    }

    /// Running → queued again, not before `run_after`.
    pub fn retry_job(
        &self,
        id: JobId,
        failure: &Failure,
        run_after: DateTime<Utc>,
    ) -> Result<bool> {
        self.finish(id, "queued", Some(failure), Some(ms(run_after)))
    }

    fn finish(
        &self,
        id: JobId,
        state: &str,
        failure: Option<&Failure>,
        run_after: Option<i64>,
    ) -> Result<bool> {
        let failure = failure.map(serde_json::to_string).transpose()?;
        self.write(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = ?2, last_error = COALESCE(?3, last_error), run_after = COALESCE(?4, run_after),
                   updated_at = ?5 WHERE id = ?1 AND state = 'running'",
                params![id.to_string(), state, failure, run_after, now_ms()],
            )? > 0)
        })
    }

    /// Queued/running → cancelled. Returns false if already finished.
    pub fn cancel_job(&self, id: JobId) -> Result<bool> {
        self.write(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = 'cancelled', updated_at = ?2 WHERE id = ?1 AND state IN ('queued','running')",
                params![id.to_string(), now_ms()],
            )? > 0)
        })
    }

    /// Put a failed or cancelled job back in the queue with a fresh attempt budget ("Retry").
    pub fn requeue_job(&self, id: JobId) -> Result<bool> {
        self.write(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = 'queued', attempts = 0, run_after = ?2, updated_at = ?2
                 WHERE id = ?1 AND state IN ('failed','cancelled')",
                params![id.to_string(), now_ms()],
            )? > 0)
        })
    }

    /// Crash recovery: every `running` job was interrupted by the app quitting or crashing.
    /// The interrupted attempt **is counted** (a job that crashes the process must not rerun
    /// forever): jobs with attempts left go back to `queued`, the rest become `failed`.
    /// Returns the number of re-queued jobs.
    pub fn recover_running_jobs(&self) -> Result<usize> {
        let failure = serde_json::to_string(&Failure {
            kind: minutae_core::FailureKind::Internal,
            message: "The app stopped while this job was running.".into(),
            fix: Some("Retry".into()),
        })?;
        self.transaction(|tx| {
            let now = now_ms();
            tx.execute(
                "UPDATE jobs SET state = 'failed', last_error = ?2, updated_at = ?1
                 WHERE state = 'running' AND attempts >= max_attempts",
                params![now, failure],
            )?;
            Ok(tx.execute(
                "UPDATE jobs SET state = 'queued', updated_at = ?1 WHERE state = 'running'",
                [now],
            )?)
        })
    }

    /// Graceful shutdown: one running job goes back to `queued` and the interrupted attempt
    /// is refunded (unlike [`Store::recover_running_jobs`], the job did not crash anything).
    pub fn release_job(&self, id: JobId) -> Result<bool> {
        self.write(|c| {
            Ok(c.execute(
                "UPDATE jobs SET state = 'queued', attempts = MAX(attempts - 1, 0), updated_at = ?2
                 WHERE id = ?1 AND state = 'running'",
                params![id.to_string(), now_ms()],
            )? > 0)
        })
    }

    /// Progress is only recorded while the job is running; a late write after the job
    /// finished (throttled progress persists off-thread) is ignored.
    pub fn set_job_progress(&self, id: JobId, p: &Progress) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE jobs SET stage = ?2, fraction = ?3, eta_seconds = ?4, updated_at = ?5
                 WHERE id = ?1 AND state = 'running'",
                params![
                    id.to_string(),
                    enum_str(&p.stage),
                    p.fraction as f64,
                    p.eta_seconds,
                    now_ms()
                ],
            )?;
            Ok(())
        })
    }

    /// Delete finished jobs last updated before `before`.
    pub fn prune_jobs(&self, before: DateTime<Utc>) -> Result<usize> {
        self.write(|c| {
            Ok(c.execute(
                "DELETE FROM jobs WHERE state IN ('succeeded','failed','cancelled') AND updated_at < ?1",
                [ms(before)],
            )?)
        })
    }
}
