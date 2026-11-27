//! Post-recording pipeline as persistent jobs:
//! transcribe (echo-cancel the mic track, then both tracks) → diarize (system track)
//! + merge → summarize → index.
//!
//! Each step stores its result before enqueueing the next, so a crash or a
//! "Retry" resumes from the failed step. Audio is never deleted on failure.
//! The Tauri-free [`Host`] trait keeps this testable with fake engines.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use chrono::{Local, Utc};
use minutae_aec::EchoCanceller;
use minutae_asr::{ProgressFn, TranscribeOptions, Transcriber};
use minutae_core::{
    AppSettings, EchoCancelMode, EchoCancelReport, EchoSkip, Failure, FailureKind, Meeting,
    MeetingId, MeetingStatus, Segment, Speaker, Stage, Summary, Track, Transcript, WhisperModel,
};
use minutae_diarize::Diarizer;
use minutae_jobs::{
    JobContext, JobHandler, JobKind, JobRecord, JobRunner, JobState, JobUpdate, Store,
};
use minutae_llm::{Router, SummaryMeta, SummaryOptions};
use minutae_store::UsageEntry;
use serde_json::json;

use crate::util;

pub const DEFAULT_TITLE: &str = "Untitled meeting";

/// What the pipeline needs from the app.
pub trait Host: Send + Sync + 'static {
    fn meeting_operations(&self) -> Option<Arc<crate::state::MeetingOperations>> {
        None
    }
    fn settings(&self) -> AppSettings;
    /// A loaded (or lazily loading) transcriber; `modelMissing` failure when not downloaded.
    fn transcriber(&self, model: WhisperModel) -> Result<Arc<dyn Transcriber>, Failure>;
    /// `None` when the speaker models aren't downloaded.
    fn diarizer(&self) -> Option<Arc<dyn Diarizer>>;
    /// Offline echo canceller for the mic track (system track as reference).
    fn echo_canceller(&self) -> Arc<dyn EchoCanceller>;
    fn router(&self) -> Arc<Router>;
    /// Where summary markdown files go.
    fn summaries_dir(&self) -> PathBuf;
    fn meetings_changed(&self, id: MeetingId);
    fn summary_ready(&self, meeting: &Meeting);
}

pub struct PipelineHandler {
    pub host: Arc<dyn Host>,
}

#[async_trait]
impl JobHandler for PipelineHandler {
    async fn run(&self, job: JobRecord, ctx: JobContext) -> Result<(), Failure> {
        let id = job
            .meeting_id
            .ok_or_else(|| internal("job has no meeting"))?;
        let _operation = match self.host.meeting_operations() {
            Some(operations) => Some(operations.lock(id).await),
            None => None,
        };
        let store = ctx.store().clone();
        let job_id = job.id;
        if !store
            .call(move |s| {
                Ok(s.get_job(job_id)?
                    .is_some_and(|job| job.state == JobState::Running))
            })
            .await
            .map_err(store_failure)?
        {
            // Deletion may have retired a job after it was claimed.
            return Ok(());
        }
        let r = match job.kind {
            JobKind::Transcribe => transcribe(&*self.host, &ctx, id).await,
            JobKind::Diarize => diarize(&*self.host, &ctx, id).await,
            JobKind::Summarize => {
                summarize_step(&*self.host, &ctx, id, job.payload["refresh"] == true).await
            }
            JobKind::Index => index(&*self.host, &ctx, id).await,
            other => Err(internal(&format!("unexpected job kind {other:?}"))),
        };
        self.host.meetings_changed(id);
        r
    }
}

/// Sets the flag when dropped (step finished, or its future was aborted).
struct SetOnDrop(Arc<AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub fn internal(msg: &str) -> Failure {
    Failure {
        kind: FailureKind::Internal,
        message: format!("Something went wrong: {msg}"),
        fix: Some("Retry".into()),
    }
}

pub fn store_failure(e: minutae_store::StoreError) -> Failure {
    tracing::error!(error = %e, "store error in pipeline");
    Failure {
        kind: FailureKind::Io,
        message: "Minutae couldn't read or write its database.".into(),
        fix: Some("Retry".into()),
    }
}

fn cancelled() -> Failure {
    Failure {
        kind: FailureKind::Cancelled,
        message: "Processing was cancelled.".into(),
        fix: Some("Retry".into()),
    }
}

async fn transcribe(host: &dyn Host, ctx: &JobContext, id: MeetingId) -> Result<(), Failure> {
    let store = ctx.store().clone();
    let settings = host.settings();
    if let Some(m) = store
        .call(move |s| s.get_meeting(id))
        .await
        .map_err(store_failure)?
        && m.status != MeetingStatus::Processing
    {
        store
            .call(move |s| s.set_meeting_status(id, MeetingStatus::Processing, None))
            .await
            .map_err(store_failure)?;
    }
    ctx.progress(Stage::Transcribing, 0.0, None);
    let tracks = store
        .call(move |s| s.list_tracks(id))
        .await
        .map_err(store_failure)?;
    let files: Vec<(Track, PathBuf, u64)> = tracks
        .iter()
        .filter(|t| Path::new(&t.path).exists())
        .map(|t| {
            (
                t.track,
                PathBuf::from(&t.path),
                t.duration_ms.unwrap_or(0).max(1),
            )
        })
        .collect();
    if files.is_empty() {
        return Err(Failure {
            kind: FailureKind::Io,
            message: "The recording file is missing.".into(),
            fix: None,
        });
    }
    let transcriber = host.transcriber(settings.whisper_model)?;
    // Without headphones the mic also hears the others through the speakers: clean it
    // first so they aren't transcribed twice (as "You" and as "Them").
    let mut files = files;
    if let Some(mic) = files.iter_mut().find(|f| f.0 == Track::Mic) {
        let system = tracks
            .iter()
            .find(|t| t.track == Track::System)
            .map(|t| PathBuf::from(&t.path));
        if let Some(clean) = echo_cancel(host, &store, id, &settings, &mic.1, system).await? {
            mic.1 = clean;
        }
    }
    // The job runner cancels (delete meeting) or shuts down (app quit) by aborting this
    // future; the guard then raises whisper's abort flag so the blocking run stops too
    // instead of burning CPU/GPU on a result nobody will read.
    let cancel = Arc::new(AtomicBool::new(ctx.is_cancelled()));
    let _abort_whisper = SetOnDrop(cancel.clone());
    let opts = TranscribeOptions {
        language: settings
            .language
            .clone()
            .filter(|l| !l.trim().is_empty() && l != "auto"),
        cancel: Some(cancel.clone()),
        ..Default::default()
    }
    .with_vocabulary(&settings.vocabulary);

    let total: u64 = files.iter().map(|f| f.2).sum();
    let mut done = 0u64;
    let mut segments: Vec<Segment> = Vec::new();
    let started = std::time::Instant::now();
    for (track, path, dur) in files {
        let base = done as f32 / total as f32;
        let weight = dur as f32 / total as f32;
        let c = ctx.clone();
        let progress: ProgressFn = Arc::new(move |f: f32| {
            c.progress(Stage::Transcribing, (base + f * weight).min(0.99), None);
        });
        let run = |t: Arc<dyn Transcriber>, path: PathBuf, progress: ProgressFn| {
            let o = opts.clone();
            tokio::task::spawn_blocking(move || t.transcribe(&path, track, &o, Some(progress)))
        };
        let r = run(transcriber.clone(), path, progress)
            .await
            .map_err(|e| internal(&e.to_string()))?;
        match r {
            Ok(mut segs) => {
                for s in &mut segs {
                    s.track = track;
                }
                segments.extend(segs);
            }
            Err(minutae_asr::AsrError::Cancelled) => return Err(cancelled()),
            Err(e) => {
                tracing::warn!(error = %e, ?track, "transcription failed");
                return Err(e.to_failure());
            }
        }
        done += dur;
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
    }
    if ctx.is_cancelled() {
        return Err(cancelled());
    }
    segments.sort_by_key(|s| (s.start_ms, s.track == Track::System));
    let transcript = Transcript {
        meeting_id: id,
        language: opts.language.clone(),
        model: transcriber.model_name(),
        segments,
    };
    let model = transcriber.model_name();
    let elapsed = started.elapsed().as_millis() as u64;
    store
        .call(move |s| {
            s.replace_transcript(&transcript, &[])?;
            s.log_usage(&UsageEntry {
                at: Utc::now(),
                provider: "whisper".into(),
                transport: Some("local".into()),
                model,
                purpose: "transcribe".into(),
                meeting_id: Some(id),
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
                duration_ms: elapsed,
                ok: true,
            })
        })
        .await
        .map_err(store_failure)?;
    ctx.progress(Stage::Transcribing, 1.0, None);
    ctx.enqueue(minutae_jobs::NewJob::new(
        JobKind::Diarize,
        json!({}),
        Some(id),
    ))
    .await
    .map_err(|e| internal(&e.to_string()))?;
    Ok(())
}

/// Runs echo cancellation on `mic` (unless the setting is off), records the report on
/// the meeting and returns the cleaned file when it should be transcribed instead.
/// Never fails the meeting: any problem means "transcribe the original mic track".
async fn echo_cancel(
    host: &dyn Host,
    store: &Store,
    id: MeetingId,
    settings: &AppSettings,
    mic: &Path,
    system: Option<PathBuf>,
) -> Result<Option<PathBuf>, Failure> {
    let clean = mic.with_file_name(minutae_aec::CLEAN_MIC_FILE);
    let report = if settings.echo_cancellation == EchoCancelMode::Off {
        if clean.exists() {
            let _ = std::fs::remove_file(&clean);
        }
        EchoCancelReport::skipped(EchoSkip::Off)
    } else {
        let aec = host.echo_canceller();
        let (mic, out) = (mic.to_path_buf(), clean.clone());
        let r = tokio::task::spawn_blocking(move || {
            minutae_aec::cancel_files(&*aec, &mic, system.as_deref(), &out)
        })
        .await
        .map_err(|e| internal(&e.to_string()))?;
        match r {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "echo cancellation failed; using the original mic track");
                if clean.exists() {
                    let _ = std::fs::remove_file(&clean);
                }
                EchoCancelReport::skipped(EchoSkip::NoSystemTrack)
            }
        }
    };
    tracing::info!(
        ran = report.ran,
        skipped = ?report.skipped,
        erle_db = ?report.erle_db,
        delay_ms = ?report.delay_ms,
        drift_ppm = ?report.drift_ppm,
        elapsed_ms = report.elapsed_ms,
        "echo cancellation"
    );
    let ran = report.ran;
    store
        .call(move |s| s.set_echo_cancel(id, &report))
        .await
        .map_err(store_failure)?;
    Ok(ran.then_some(clean))
}

async fn diarize(host: &dyn Host, ctx: &JobContext, id: MeetingId) -> Result<(), Failure> {
    let store = ctx.store().clone();
    let settings = host.settings();
    ctx.progress(Stage::Diarizing, 0.0, None);
    let transcript = store
        .call(move |s| s.get_transcript(id))
        .await
        .map_err(store_failure)?
        .ok_or_else(|| internal("transcript missing"))?;
    let tracks = store
        .call(move |s| s.list_tracks(id))
        .await
        .map_err(store_failure)?;
    let (mic, system): (Vec<Segment>, Vec<Segment>) = transcript
        .segments
        .iter()
        .cloned()
        .partition(|s| s.track == Track::Mic);
    let system_path = tracks
        .iter()
        .find(|t| t.track == Track::System)
        .map(|t| PathBuf::from(&t.path))
        .filter(|p| p.exists());

    let mut turns = Vec::new();
    if settings.label_speakers
        && !system.is_empty()
        && let (Some(path), Some(d)) = (system_path, host.diarizer())
    {
        let r = tokio::task::spawn_blocking(move || d.diarize(&path, None))
            .await
            .map_err(|e| internal(&e.to_string()))?;
        match r {
            Ok(t) => turns = t,
            // Speaker labels are a nicety: fall back to one "Speaker 1" instead of failing.
            Err(e) => tracing::warn!(error = %e, "diarization failed; using a single speaker"),
        }
    }
    let (segments, speakers): (Vec<Segment>, Vec<Speaker>) =
        minutae_diarize::merge(&mic, &system, &turns, id);
    let merged = Transcript {
        segments,
        ..transcript
    };
    store
        .call(move |s| s.replace_transcript(&merged, &speakers))
        .await
        .map_err(store_failure)?;
    ctx.progress(Stage::Diarizing, 1.0, None);
    ctx.enqueue(minutae_jobs::NewJob::new(
        JobKind::Summarize,
        json!({}),
        Some(id),
    ))
    .await
    .map_err(|e| internal(&e.to_string()))?;
    Ok(())
}

/// "Name: text" lines, consecutive lines by the same speaker joined.
pub fn transcript_text(segments: &[Segment], speakers: &[Speaker]) -> String {
    let name = |s: &Segment| -> String {
        s.speaker_id
            .and_then(|id| speakers.iter().find(|sp| sp.id == id))
            .map(|sp| sp.display_name.clone().unwrap_or_else(|| sp.label.clone()))
            .unwrap_or_else(|| {
                if s.track == Track::Mic {
                    "You".into()
                } else {
                    "Them".into()
                }
            })
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        let n = name(s);
        match out.last_mut() {
            Some((last, buf)) if *last == n => {
                buf.push(' ');
                buf.push_str(text);
            }
            _ => out.push((n, text.to_string())),
        }
    }
    out.into_iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn is_default_title(m: &Meeting) -> bool {
    m.title == DEFAULT_TITLE
        || m.source_app
            .as_deref()
            .is_some_and(|a| m.title == format!("{a} call"))
}

/// `<dir>/YYYY-MM-DD-<slug>-<id8>.md`
pub fn summary_file_name(m: &Meeting) -> String {
    let id = m.id.simple().to_string();
    format!(
        "{}-{}-{}.md",
        m.started_at.with_timezone(&Local).format("%Y-%m-%d"),
        util::slug(&m.title),
        &id[..8]
    )
}

/// Existing markdown file for a meeting (title may have changed since it was written).
pub fn find_summary_file(dir: &Path, id: MeetingId) -> Option<PathBuf> {
    crate::meeting_content::LocalMeetingFiles(minutae_core::paths::data_dir())
        .find_markdown(dir, id)
}

async fn summarize_step(
    host: &dyn Host,
    ctx: &JobContext,
    id: MeetingId,
    refresh: bool,
) -> Result<(), Failure> {
    let store = ctx.store().clone();
    let settings = host.settings();
    ctx.progress(Stage::Summarizing, 0.05, None);
    let (meeting, transcript, speakers, tracks) = store
        .call(move |s| {
            Ok((
                s.get_meeting(id)?,
                s.get_transcript(id)?,
                s.list_speakers(id)?,
                s.list_tracks(id)?,
            ))
        })
        .await
        .map_err(store_failure)?;
    let meeting = meeting.ok_or_else(|| internal("meeting missing"))?;
    let segments = transcript.map(|t| t.segments).unwrap_or_default();
    let text = transcript_text(&segments, &speakers);
    let duration_ms = meeting
        .ended_at
        .map(|e| (e - meeting.started_at).num_milliseconds().max(0) as u64)
        .or_else(|| tracks.iter().filter_map(|t| t.duration_ms).max());
    let mut participants: Vec<String> = Vec::new();
    for sp in &speakers {
        let n = sp.display_name.clone().unwrap_or_else(|| sp.label.clone());
        if !participants.contains(&n) {
            participants.push(n);
        }
    }
    let meta = SummaryMeta {
        meeting_id: id,
        title: meeting.title.clone(),
        date: meeting.started_at.with_timezone(&Local).naive_local(),
        duration: duration_ms.map(std::time::Duration::from_millis),
        participants: participants.clone(),
        recording: Some(id.to_string()),
    };

    let (summary, suggestion) = if text.trim().is_empty() {
        let none = "No speech was detected in this recording.".to_string();
        let markdown = minutae_llm::summary::render_markdown(
            &meta,
            &participants,
            std::slice::from_ref(&none),
            &[],
            &[],
        );
        (
            Summary {
                meeting_id: id,
                overview: none.clone(),
                decisions: vec![],
                action_items: vec![],
                takeaway: none,
                markdown,
                provider: "Minutae".into(),
                model: "none".into(),
            },
            None,
        )
    } else {
        let router = host.router();
        let cancel = minutae_llm::CancellationToken::new();
        let c2 = cancel.clone();
        let c = ctx.clone();
        let watcher = tokio::spawn(async move {
            c.cancelled().await;
            c2.cancel();
        });
        let opts = SummaryOptions {
            length: settings.summary_length,
            cancel,
            ..Default::default()
        };
        let r = minutae_llm::summarize(&router, &text, &meta, &opts).await;
        watcher.abort();
        match r {
            Ok(o) => (o.summary, o.title_suggestion),
            Err(e) => {
                tracing::warn!(kind = ?e.kind, attempts = ?e.attempts, "summary failed");
                // The transcript is already stored at this point: always say so.
                let mut f = e.failure();
                if !f.message.contains("Transcript is saved") {
                    f.message = format!("{} Transcript is saved.", f.message.trim_end());
                }
                return Err(f);
            }
        }
    };
    ctx.progress(Stage::Summarizing, 0.9, None);

    let new_title = suggestion.filter(|_| is_default_title(&meeting));
    let mut updated = meeting.clone();
    if let Some(title) = &new_title {
        updated.title.clone_from(title);
    }
    let dir = host.summaries_dir();
    store
        .call(move |s| {
            crate::meeting_content::publish_summary(
                s,
                &updated,
                &summary,
                new_title.as_deref(),
                &dir,
            )
        })
        .await
        .map_err(store_failure)?;
    ctx.progress(Stage::Summarizing, 1.0, None);
    if refresh {
        return Ok(());
    }
    ctx.enqueue(minutae_jobs::NewJob::new(
        JobKind::Index,
        json!({}),
        Some(id),
    ))
    .await
    .map_err(|e| internal(&e.to_string()))?;
    Ok(())
}

/// FTS5 triggers already index segments/summaries/action items on write; this
/// step only finishes the meeting.
async fn index(host: &dyn Host, ctx: &JobContext, id: MeetingId) -> Result<(), Failure> {
    let store = ctx.store().clone();
    ctx.progress(Stage::Indexing, 0.5, None);
    store
        .call(move |s| s.set_meeting_status(id, MeetingStatus::Done, None))
        .await
        .map_err(store_failure)?;
    ctx.progress(Stage::Indexing, 1.0, None);
    if let Some(m) = store
        .call(move |s| s.get_meeting(id))
        .await
        .map_err(store_failure)?
    {
        host.summary_ready(&m);
    }
    Ok(())
}

/// Reflect a job's terminal state on its meeting. Returns the meeting to refresh.
pub fn apply_job_update(store: &Store, u: &JobUpdate) -> Option<MeetingId> {
    let id = u.meeting_id?;
    if !matches!(
        u.kind,
        JobKind::Transcribe | JobKind::Diarize | JobKind::Summarize | JobKind::Index
    ) {
        return None;
    }
    match u.state {
        JobState::Failed | JobState::Cancelled => {
            let mut f = u.failure.clone().unwrap_or_else(cancelled);
            if let Err(e) = store.transaction(|tx| {
                use minutae_store::rusqlite::{OptionalExtension, params};
                let payload: Option<String> = tx
                    .query_row(
                        "SELECT payload FROM jobs WHERE id=?1 AND state=?2 AND attempts=?3
                       AND NOT EXISTS (SELECT 1 FROM jobs newer WHERE newer.meeting_id=jobs.meeting_id
                                       AND newer.rowid > jobs.rowid)",
                        params![
                            u.job_id.to_string(),
                            if u.state == JobState::Failed {
                                "failed"
                            } else {
                                "cancelled"
                            },
                            u.attempts
                        ],
                        |row| row.get(0),
                    )
                    .optional()?;
                let Some(payload) = payload else {
                    return Ok(());
                };
                let refresh =
                    serde_json::from_str::<serde_json::Value>(&payload)?["refresh"] == true;
                if refresh {
                    let has_summary: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM summaries WHERE meeting_id=?1)",
                        [id.to_string()], |row| row.get(0),
                    )?;
                    if has_summary {
                        f.message = format!("{} The previous summary is still available.", f.message);
                    }
                    f.fix = Some("Summarize again".into());
                }
                tx.execute(
                    "UPDATE meetings SET status=?2, failure=?3 WHERE id=?1",
                    params![
                        id.to_string(),
                        if refresh { "done" } else { "failed" },
                        serde_json::to_string(&f)?
                    ],
                )?;
                Ok(())
            }) {
                tracing::warn!(error = %e, "mark meeting failed");
            }
            Some(id)
        }
        JobState::Queued | JobState::Succeeded => Some(id),
        JobState::Running => None,
    }
}

/// "Retry": re-queue the failed step, or start from the first missing result.
pub async fn retry_meeting(runner: &JobRunner, id: MeetingId) -> Result<(), String> {
    let store = runner.store().clone();
    let jobs = store
        .call(move |s| s.jobs_for_meeting(id))
        .await
        .map_err(|e| e.to_string())?;
    if jobs
        .iter()
        .any(|j| matches!(j.state, JobState::Queued | JobState::Running))
    {
        return Ok(());
    }
    store
        .call(move |s| s.set_meeting_status(id, MeetingStatus::Processing, None))
        .await
        .map_err(|e| e.to_string())?;
    let last = jobs.iter().rev().find(|j| {
        matches!(
            j.kind,
            JobKind::Transcribe | JobKind::Diarize | JobKind::Summarize | JobKind::Index
        )
    });
    if let Some(j) = last
        && matches!(j.state, JobState::Failed | JobState::Cancelled)
        && runner.retry(j.id).await.map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    let kind = first_missing_step(&store, id).await?;
    runner
        .enqueue(kind, json!({}), Some(id))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn first_missing_step(store: &Store, id: MeetingId) -> Result<JobKind, String> {
    let (transcript, speakers, summary) = store
        .call(move |s| {
            Ok((
                s.get_transcript(id)?,
                s.list_speakers(id)?,
                s.get_summary(id)?,
            ))
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(match (transcript, summary) {
        (None, _) => JobKind::Transcribe,
        (Some(t), None) if speakers.is_empty() && !t.segments.is_empty() => JobKind::Diarize,
        (Some(_), None) => JobKind::Summarize,
        (Some(_), Some(_)) => JobKind::Index,
    })
}

pub fn handlers(host: Arc<dyn Host>) -> Vec<(JobKind, Arc<dyn JobHandler>)> {
    let h: Arc<dyn JobHandler> = Arc::new(PipelineHandler { host });
    vec![
        (JobKind::Transcribe, h.clone()),
        (JobKind::Diarize, h.clone()),
        (JobKind::Summarize, h.clone()),
        (JobKind::Index, h),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use minutae_aec::FakeEchoCanceller;
    use minutae_asr::FakeTranscriber;
    use minutae_core::Transport;
    use minutae_diarize::{FakeDiarizer, SpeakerTurn};
    use minutae_llm::{Availability, LlmError, LlmTransport, Request, Response};
    use minutae_store::TrackFile;
    use uuid::Uuid;

    use super::*;

    struct FakeLlm {
        reply: Result<String, FailureKind>,
    }

    #[async_trait]
    impl LlmTransport for FakeLlm {
        fn id(&self) -> Transport {
            Transport::Ollama
        }
        fn name(&self) -> &str {
            "Ollama"
        }
        async fn availability(&self) -> Availability {
            Availability::Ready
        }
        async fn complete(&self, _req: &Request) -> Result<Response, LlmError> {
            match &self.reply {
                Ok(t) => Ok(Response {
                    text: t.clone(),
                    model: "fake-model".into(),
                    tokens_in: Some(10),
                    tokens_out: Some(5),
                    provider: "ollama".into(),
                    cost_usd: None,
                }),
                Err(k) => Err(LlmError::new(*k, "nope")),
            }
        }
    }

    struct TestHost {
        transcriber: Arc<FakeTranscriber>,
        diarizer: Option<Arc<FakeDiarizer>>,
        router: Arc<Router>,
        dir: PathBuf,
        missing_model: bool,
        ready: Mutex<Vec<MeetingId>>,
        /// Replaces `transcriber` when set (e.g. a slow one for cancellation tests).
        override_transcriber: Mutex<Option<Arc<dyn Transcriber>>>,
        echo: Mutex<Arc<dyn EchoCanceller>>,
        echo_mode: Mutex<EchoCancelMode>,
    }

    /// Fake transcriber that remembers which files it was asked to read.
    #[derive(Default)]
    struct PathSpy {
        inner: FakeTranscriber,
        paths: Mutex<Vec<PathBuf>>,
    }

    impl Transcriber for PathSpy {
        fn model_name(&self) -> String {
            "spy".into()
        }
        fn transcribe(
            &self,
            wav_path: &Path,
            track: Track,
            opts: &TranscribeOptions,
            progress: Option<ProgressFn>,
        ) -> minutae_asr::Result<Vec<Segment>> {
            self.paths.lock().unwrap().push(wav_path.to_path_buf());
            let samples = minutae_asr::read_wav_16k_mono(wav_path)?;
            self.transcribe_samples(&samples, track, opts, progress)
        }
        fn transcribe_samples(
            &self,
            samples: &[f32],
            track: Track,
            opts: &TranscribeOptions,
            progress: Option<ProgressFn>,
        ) -> minutae_asr::Result<Vec<Segment>> {
            self.inner
                .transcribe_samples(samples, track, opts, progress)
        }
    }

    impl PathSpy {
        fn file_names(&self) -> Vec<String> {
            let mut v: Vec<String> = self
                .paths
                .lock()
                .unwrap()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }
    }

    /// Spins until the cancel flag is raised (max 10 s), like whisper's abort callback.
    struct BlockingTranscriber {
        started: std::sync::atomic::AtomicBool,
        saw_cancel: std::sync::atomic::AtomicBool,
    }

    impl Transcriber for BlockingTranscriber {
        fn model_name(&self) -> String {
            "blocking".into()
        }
        fn transcribe_samples(
            &self,
            _samples: &[f32],
            _track: Track,
            opts: &TranscribeOptions,
            _progress: Option<ProgressFn>,
        ) -> minutae_asr::Result<Vec<Segment>> {
            self.started.store(true, Ordering::SeqCst);
            let flag = opts.cancel.clone().expect("pipeline passes a cancel flag");
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                if flag.load(Ordering::Relaxed) {
                    self.saw_cancel.store(true, Ordering::SeqCst);
                    return Err(minutae_asr::AsrError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(vec![])
        }
    }

    impl Host for TestHost {
        fn settings(&self) -> AppSettings {
            AppSettings {
                vocabulary: vec!["Minutae".into()],
                echo_cancellation: *self.echo_mode.lock().unwrap(),
                ..Default::default()
            }
        }
        fn transcriber(&self, model: WhisperModel) -> Result<Arc<dyn Transcriber>, Failure> {
            if let Some(t) = self.override_transcriber.lock().unwrap().clone() {
                return Ok(t);
            }
            if self.missing_model {
                return Err(minutae_asr::AsrError::ModelMissing(
                    format!("{model:?}").to_lowercase(),
                )
                .to_failure());
            }
            Ok(self.transcriber.clone())
        }
        fn echo_canceller(&self) -> Arc<dyn EchoCanceller> {
            self.echo.lock().unwrap().clone()
        }
        fn diarizer(&self) -> Option<Arc<dyn Diarizer>> {
            self.diarizer.clone().map(|d| d as Arc<dyn Diarizer>)
        }
        fn router(&self) -> Arc<Router> {
            self.router.clone()
        }
        fn summaries_dir(&self) -> PathBuf {
            self.dir.join("summaries")
        }
        fn meetings_changed(&self, _id: MeetingId) {}
        fn summary_ready(&self, m: &Meeting) {
            self.ready.lock().unwrap().push(m.id);
        }
    }

    fn write_wav(path: &Path, secs: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..(16_000 * secs) {
            w.write_sample(((i as f32 * 0.05).sin() * 3000.0) as i16)
                .unwrap();
        }
        w.finalize().unwrap();
    }

    const SUMMARY_JSON: &str = r#"{"overview":"We agreed to ship in May.","points":["Ship in May"],"takeaway":"Launch moves to May.","decisions":["Ship in May"],"actionItems":[{"text":"Send the plan","owner":"You","due":null}],"titleSuggestion":"Launch planning","participants":[]}"#;

    async fn setup(
        reply: Result<String, FailureKind>,
        missing_model: bool,
    ) -> (
        Store,
        JobRunner,
        Arc<TestHost>,
        MeetingId,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let id = Uuid::new_v4();
        let started = Utc::now() - chrono::Duration::seconds(10);
        store
            .create_meeting(&Meeting {
                id,
                title: DEFAULT_TITLE.into(),
                started_at: started,
                ended_at: None,
                status: MeetingStatus::Recording,
                source_app: None,
                failure: None,
            })
            .unwrap();
        for (track, name) in [(Track::Mic, "mic.wav"), (Track::System, "system.wav")] {
            let p = dir.path().join(name);
            write_wav(&p, 4);
            store
                .upsert_track(&TrackFile {
                    meeting_id: id,
                    track,
                    path: p.to_string_lossy().into(),
                    duration_ms: Some(4000),
                    bytes: None,
                })
                .unwrap();
        }
        store.end_meeting(id, Utc::now()).unwrap();
        let host = Arc::new(TestHost {
            transcriber: Arc::new(FakeTranscriber::new(vec![
                (0, 1500, "Hello there.".into()),
                (2000, 3500, "Let's ship in May.".into()),
            ])),
            diarizer: Some(Arc::new(FakeDiarizer {
                turns: vec![
                    SpeakerTurn {
                        start_ms: 0,
                        end_ms: 1800,
                        speaker: 0,
                    },
                    SpeakerTurn {
                        start_ms: 1900,
                        end_ms: 4000,
                        speaker: 1,
                    },
                ],
            })),
            router: Arc::new(Router::new(
                vec![Transport::Ollama],
                vec![Arc::new(FakeLlm { reply })],
            )),
            dir: dir.path().to_path_buf(),
            missing_model,
            ready: Mutex::new(vec![]),
            override_transcriber: Mutex::new(None),
            echo: Mutex::new(Arc::new(FakeEchoCanceller::ran(24.0))),
            echo_mode: Mutex::new(EchoCancelMode::Auto),
        });
        let runner = JobRunner::new(store.clone(), handlers(host.clone()));
        runner.start().await.unwrap();
        // Mirror the app's job-update listener.
        let mut rx = runner.subscribe();
        let s2 = store.clone();
        tokio::spawn(async move {
            while let Ok(u) = rx.recv().await {
                apply_job_update(&s2, &u);
            }
        });
        (store, runner, host, id, dir)
    }

    async fn wait_status(store: &Store, id: MeetingId, want: MeetingStatus) -> Meeting {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let m = store.get_meeting(id).unwrap().unwrap();
            if m.status == want {
                return m;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "meeting stuck in {:?} ({:?})",
                m.status,
                m.failure
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_pipeline_produces_speakers_summary_and_markdown() {
        let (store, runner, host, id, dir) = setup(Ok(SUMMARY_JSON.into()), false).await;
        runner
            .enqueue(JobKind::Transcribe, json!({}), Some(id))
            .await
            .unwrap();
        let m = wait_status(&store, id, MeetingStatus::Done).await;
        assert_eq!(
            m.title, "Launch planning",
            "default title replaced by suggestion"
        );

        let speakers = store.list_speakers(id).unwrap();
        let labels: Vec<&str> = speakers.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.contains(&"Speaker 1") && labels.contains(&"Speaker 2"),
            "{labels:?}"
        );
        // The fake returns identical text on both tracks — exactly what speaker bleed
        // looks like — so the merge drops the mic duplicates and keeps the system lines.
        let t = store.get_transcript(id).unwrap().unwrap();
        assert_eq!(t.segments.len(), 2);
        assert!(
            t.segments.iter().all(|s| s.track == Track::System),
            "mic duplicates of system speech are dropped"
        );
        assert!(t.segments.iter().all(|s| s.speaker_id.is_some()));

        let summary = store.get_summary(id).unwrap().unwrap();
        assert_eq!(summary.takeaway, "Launch moves to May.");
        assert_eq!(store.list_action_items(id).unwrap().len(), 1);
        let file = find_summary_file(&dir.path().join("summaries"), id).expect("markdown written");
        assert!(
            std::fs::read_to_string(file)
                .unwrap()
                .contains("## Summary")
        );
        assert_eq!(host.ready.lock().unwrap().as_slice(), &[id]);
        assert_eq!(host.transcriber.calls(), 2, "both tracks transcribed");
        // Whisper usage row (free, on-device).
        let usage = store
            .usage_by_month(Utc::now() - chrono::Duration::days(1))
            .unwrap();
        assert!(usage.iter().any(|u| u.provider == "whisper"));
        // The fake canceller "ran": its report is on the meeting, the clean file exists.
        let ec = store.echo_cancel(id).unwrap().expect("echo report stored");
        assert!(ec.ran && ec.erle_db == Some(24.0), "{ec:?}");
        assert!(dir.path().join(minutae_aec::CLEAN_MIC_FILE).exists());
        runner.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn llm_failure_marks_meeting_failed_and_keeps_transcript_then_retry_resumes() {
        let (store, runner, _host, id, _dir) = setup(Err(FailureKind::NotLoggedIn), false).await;
        runner
            .enqueue(JobKind::Transcribe, json!({}), Some(id))
            .await
            .unwrap();
        let m = wait_status(&store, id, MeetingStatus::Failed).await;
        let f = m.failure.unwrap();
        // The router reports the real cause of the last attempt, not a generic "no provider".
        assert_eq!(f.kind, FailureKind::NotLoggedIn);
        assert!(f.message.contains("Transcript is saved"));
        assert!(
            store.get_transcript(id).unwrap().is_some(),
            "transcript kept"
        );
        assert!(store.get_summary(id).unwrap().is_none());
        assert_eq!(
            first_missing_step(&store, id).await.unwrap(),
            JobKind::Summarize
        );

        retry_meeting(&runner, id).await.unwrap();
        // Still failing (same fake), but it went back through the summarize step only.
        let m = wait_status(&store, id, MeetingStatus::Failed).await;
        assert_eq!(m.failure.unwrap().kind, FailureKind::NotLoggedIn);
        let jobs = store.jobs_for_meeting(id).unwrap();
        assert_eq!(
            jobs.iter()
                .filter(|j| j.kind == JobKind::Transcribe)
                .count(),
            1
        );
        runner.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_model_fails_with_download_fix() {
        let (store, runner, _host, id, _dir) = setup(Ok(SUMMARY_JSON.into()), true).await;
        runner
            .enqueue(JobKind::Transcribe, json!({}), Some(id))
            .await
            .unwrap();
        let m = wait_status(&store, id, MeetingStatus::Failed).await;
        let f = m.failure.unwrap();
        assert_eq!(f.kind, FailureKind::ModelMissing);
        assert_eq!(f.fix.as_deref(), Some("Download model"));
        runner.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_the_job_aborts_a_running_transcription() {
        let (store, runner, host, id, _dir) = setup(Ok(SUMMARY_JSON.into()), false).await;
        let slow = Arc::new(BlockingTranscriber {
            started: Default::default(),
            saw_cancel: Default::default(),
        });
        *host.override_transcriber.lock().unwrap() = Some(slow.clone());
        let job = runner
            .enqueue(JobKind::Transcribe, json!({}), Some(id))
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !slow.started.load(Ordering::SeqCst) {
            assert!(
                std::time::Instant::now() < deadline,
                "transcription never started"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let t0 = std::time::Instant::now();
        assert!(runner.cancel(job).await.unwrap());
        while !slow.saw_cancel.load(Ordering::SeqCst) {
            assert!(
                t0.elapsed() < Duration::from_secs(2),
                "whisper kept running after the job was cancelled"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let m = wait_status(&store, id, MeetingStatus::Failed).await;
        assert_eq!(m.failure.unwrap().kind, FailureKind::Cancelled);
        runner.shutdown().await;
    }

    #[test]
    fn transcript_text_joins_runs() {
        let mid = Uuid::new_v4();
        let you = Speaker {
            id: Uuid::new_v4(),
            meeting_id: mid,
            label: "You".into(),
            display_name: None,
            track: Track::Mic,
        };
        let s1 = Speaker {
            id: Uuid::new_v4(),
            meeting_id: mid,
            label: "Speaker 1".into(),
            display_name: Some("Alice".into()),
            track: Track::System,
        };
        let seg = |t: &str, sp: &Speaker| Segment {
            start_ms: 0,
            end_ms: 1,
            text: t.into(),
            track: sp.track,
            speaker_id: Some(sp.id),
        };
        let text = transcript_text(
            &[seg("Hi.", &you), seg("How are", &s1), seg("you?", &s1)],
            &[you.clone(), s1.clone()],
        );
        assert_eq!(text, "You: Hi.\nAlice: How are you?");
    }

    async fn run_with_spy(
        configure: impl FnOnce(&TestHost, &Path),
    ) -> (Store, Arc<PathSpy>, MeetingId, tempfile::TempDir) {
        let (store, runner, host, id, dir) = setup(Ok(SUMMARY_JSON.into()), false).await;
        let spy = Arc::new(PathSpy::default());
        *host.override_transcriber.lock().unwrap() = Some(spy.clone());
        configure(&host, dir.path());
        runner
            .enqueue(JobKind::Transcribe, json!({}), Some(id))
            .await
            .unwrap();
        wait_status(&store, id, MeetingStatus::Done).await;
        runner.shutdown().await;
        (store, spy, id, dir)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cleaned_mic_is_transcribed_when_echo_cancellation_ran() {
        let (store, spy, id, dir) = run_with_spy(|_, _| {}).await;
        assert_eq!(spy.file_names(), ["mic.clean.wav", "system.wav"]);
        assert!(store.echo_cancel(id).unwrap().unwrap().ran);
        // The original recording is kept.
        assert!(dir.path().join("mic.wav").exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn echo_cancellation_off_uses_the_original_mic_and_drops_a_stale_clean_file() {
        let (store, spy, id, dir) = run_with_spy(|host, dir| {
            *host.echo_mode.lock().unwrap() = EchoCancelMode::Off;
            write_wav(&dir.join(minutae_aec::CLEAN_MIC_FILE), 1);
        })
        .await;
        assert_eq!(spy.file_names(), ["mic.wav", "system.wav"]);
        assert!(!dir.path().join(minutae_aec::CLEAN_MIC_FILE).exists());
        assert_eq!(
            store.echo_cancel(id).unwrap(),
            Some(EchoCancelReport::skipped(EchoSkip::Off))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn headphones_skip_keeps_the_original_mic() {
        let (store, spy, id, _dir) = run_with_spy(|host, _| {
            *host.echo.lock().unwrap() = Arc::new(FakeEchoCanceller::skipping(EchoSkip::NoEcho));
        })
        .await;
        assert_eq!(spy.file_names(), ["mic.wav", "system.wav"]);
        let r = store.echo_cancel(id).unwrap().unwrap();
        assert_eq!((r.ran, r.skipped), (false, Some(EchoSkip::NoEcho)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_system_track_skips_echo_cancellation() {
        let (store, spy, id, _dir) = run_with_spy(|_, dir| {
            std::fs::remove_file(dir.join("system.wav")).unwrap();
        })
        .await;
        assert_eq!(spy.file_names(), ["mic.wav"]);
        assert_eq!(
            store.echo_cancel(id).unwrap().unwrap().skipped,
            Some(EchoSkip::NoSystemTrack)
        );
    }

    /// The real canceller on a synthetic laptop call written as the two track files.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn real_echo_canceller_cleans_a_synthetic_laptop_call() {
        let (store, spy, id, dir) = run_with_spy(|host, dir| {
            let scene = minutae_aec::synth::laptop_call(70.0, 25.0, 9);
            minutae_aec::write_wav_16k_mono(&dir.join("mic.wav"), &scene.mic).unwrap();
            minutae_aec::write_wav_16k_mono(&dir.join("system.wav"), &scene.reference).unwrap();
            *host.echo.lock().unwrap() = Arc::new(minutae_aec::OfflineAec::default());
        })
        .await;
        assert_eq!(spy.file_names(), ["mic.clean.wav", "system.wav"]);
        let r = store.echo_cancel(id).unwrap().unwrap();
        assert!(r.ran && r.erle_db.unwrap() > 20.0, "{r:?}");
        assert!((r.delay_ms.unwrap() - 70.0).abs() < 25.0, "{r:?}");
        let clean = minutae_aec::read_wav_16k_mono(&dir.path().join("mic.clean.wav")).unwrap();
        assert_eq!(clean.len(), 60 * 16_000);
    }
}
