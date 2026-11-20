//! Meeting recording: start/stop/pause, level + state events, crash recovery.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use minutae_asr::captions::{Caption, CaptionConfig, CaptionEngine, CaptionSink};
use minutae_asr::{ModelManager, TranscribeOptions, Transcriber};
use minutae_audio::{
    AudioError, CaptureConfig, CaptureEvent, RecordedTracks, Recorder, RecordingHandle, Taps,
};
use minutae_core::{
    AppSettings, Failure, FailureKind, Meeting, MeetingId, MeetingStatus, Moment, Track,
    WhisperModel,
};
use minutae_jobs::JobKind;
use minutae_store::{Store, TrackFile};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::dto::{self, CaptionStatus, RecordingPhase, RecordingState, SystemAudio, ev};
use crate::pipeline::DEFAULT_TITLE;
use crate::state::{AppState, emit_meetings_changed, recordings_root};
use crate::util;

pub struct ActiveRecording {
    pub meeting: Meeting,
    pub handle: Arc<RecordingHandle>,
    pub dir: PathBuf,
    pub phase: RecordingPhase,
    pub include_system: bool,
    pub system_error: Option<String>,
    /// Started from the call prompt (so "stop when the call ends" applies).
    pub from_call: bool,
    /// Resolved when the first `stop()` has fully finished (stitch + database). Later
    /// callers (a second Stop click, quit, delete, erase) wait on it instead of racing.
    pub stopped: StopSignal,
    ticker: tokio::task::AbortHandle,
    /// Live captions (present whenever a Whisper model is, even if turned off, so the
    /// setting can be flipped mid-recording).
    captions: Option<CaptionEngine>,
    pub caption_status: CaptionStatus,
}

/// One-shot completion shared by every caller of [`stop`] for the same recording.
#[derive(Clone)]
pub struct StopSignal(tokio::sync::watch::Sender<Option<Result<MeetingId, String>>>);

impl Default for StopSignal {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(None).0)
    }
}

impl StopSignal {
    pub fn finish(&self, result: Result<MeetingId, String>) {
        self.0.send_replace(Some(result));
    }

    /// Wait until [`StopSignal::finish`] ran and return its result.
    pub async fn wait(&self) -> Result<MeetingId, String> {
        let mut rx = self.0.subscribe();
        match rx.wait_for(Option::is_some).await {
            Ok(v) => v
                .clone()
                .unwrap_or_else(|| Err("The recording stopped.".into())),
            Err(_) => Err("The recording stopped.".into()),
        }
    }
}

impl ActiveRecording {
    pub fn state(&self) -> RecordingState {
        RecordingState {
            phase: self.phase,
            meeting_id: Some(self.meeting.id.to_string()),
            title: Some(self.meeting.title.clone()),
            source_app: self.meeting.source_app.clone(),
            elapsed_ms: self.handle.elapsed().as_millis() as u64,
            system_audio: if !self.include_system {
                SystemAudio::Off
            } else if self.system_error.is_some() {
                SystemAudio::Unavailable
            } else {
                SystemAudio::On
            },
            system_audio_error: self.system_error.clone(),
            captions: self.caption_status,
        }
    }
}

pub async fn current_state(app: &AppHandle) -> RecordingState {
    let st = app.state::<AppState>();
    let guard = st.recording.lock().await;
    match guard.as_ref() {
        Some(r) => r.state(),
        None => RecordingState::idle(st.settings().include_system_audio),
    }
}

async fn emit_state(app: &AppHandle) {
    let s = current_state(app).await;
    let _ = app.emit(ev::RECORDING_STATE, &s);
    crate::shell::update_tray_recording(app, &s);
}

fn audio_error_message(e: &AudioError) -> String {
    match e {
        AudioError::NoInputDevice => "No microphone was found.".into(),
        AudioError::PermissionDenied(_) => {
            "Minutae isn't allowed to use the microphone. Allow it in System Settings → Privacy."
                .into()
        }
        other => format!("The microphone couldn't be started: {other}"),
    }
}

pub fn default_title(source_app: Option<&str>) -> String {
    source_app
        .map(|a| format!("{a} call"))
        .unwrap_or_else(|| DEFAULT_TITLE.to_string())
}

pub async fn start(
    app: &AppHandle,
    title: Option<String>,
    source_app: Option<String>,
    from_call: bool,
) -> Result<dto::Meeting, String> {
    let st = app.state::<AppState>();
    let mut guard = st.recording.lock().await;
    if let Some(r) = guard.as_ref() {
        if r.phase == RecordingPhase::Stopping {
            return Err("The last recording is still being saved. Try again in a moment.".into());
        }
        return Ok(dto::Meeting::from(&r.meeting));
    }
    let settings = st.settings();
    let id = Uuid::new_v4();
    let dir = recordings_root(&settings).join(id.to_string());
    let meeting = Meeting {
        id,
        title: title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| default_title(source_app.as_deref())),
        started_at: Utc::now(),
        ended_at: None,
        status: MeetingStatus::Recording,
        source_app,
        failure: None,
    };
    let cfg = CaptureConfig {
        mic_device: settings.microphone.clone(),
        include_system: settings.include_system_audio,
        out_dir: dir.clone(),
        chunk_secs: 10,
    };
    let (captions, taps, caption_status) = start_captions(app, &settings, id);
    let handle = tokio::task::spawn_blocking(move || Recorder::new().start_with_taps(cfg, taps))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| {
            tracing::warn!(error = %e, "recording failed to start");
            audio_error_message(&e)
        })?;
    // Subscribe before anything else awaits: capture events are not replayed.
    let capture_events = handle.events();
    // The meeting row exists only once audio is flowing.
    let m = meeting.clone();
    st.store
        .call(move |s| s.create_meeting(&m))
        .await
        .map_err(|e| e.to_string())?;
    allow_asset_dir(app, &dir);

    let handle = Arc::new(handle);
    let system_error = handle.system_error().map(str::to_string);
    if let Some(e) = &system_error {
        tracing::warn!(error = %e, "system audio not captured");
    }
    let ticker = tauri::async_runtime::spawn(ticker(app.clone(), handle.clone()))
        .inner()
        .abort_handle();
    *guard = Some(ActiveRecording {
        meeting: meeting.clone(),
        handle,
        dir,
        phase: RecordingPhase::Recording,
        include_system: settings.include_system_audio,
        system_error,
        from_call,
        stopped: StopSignal::default(),
        ticker,
        captions,
        caption_status,
    });
    tauri::async_runtime::spawn(watch_capture(app.clone(), id, capture_events));
    drop(guard);
    crate::calls::hide_prompt(app);
    emit_state(app).await;
    emit_meetings_changed(app, Some(id));
    Ok(dto::Meeting::from(&meeting))
}

/// ~30 Hz levels; state + tray once a second.
async fn ticker(app: AppHandle, handle: Arc<RecordingHandle>) {
    let levels = handle.levels();
    let mut interval = tokio::time::interval(Duration::from_millis(33));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut n: u64 = 0;
    loop {
        interval.tick().await;
        let f = *levels.borrow();
        let frame = dto::LevelFrame {
            elapsed_ms: f.elapsed_ms,
            mic: util::meter_level(f.mic_rms),
            system: util::meter_level(f.system_rms),
        };
        let _ = app.emit(ev::LEVELS, frame);
        n += 1;
        if n.is_multiple_of(30) {
            emit_state(&app).await;
        }
    }
}

/// Stop the active recording. Concurrent callers all wait for the same stop to finish
/// (stitch + database update) and get its result, so nothing races the stitch.
pub async fn stop(app: &AppHandle) -> Result<MeetingId, String> {
    let st = app.state::<AppState>();
    let (handle, meeting, ticker, signal, captions) = {
        let mut guard = st.recording.lock().await;
        let Some(r) = guard.as_mut() else {
            return Err("Nothing is recording.".into());
        };
        if r.phase == RecordingPhase::Stopping {
            let signal = r.stopped.clone();
            drop(guard);
            return signal.wait().await;
        }
        r.phase = RecordingPhase::Stopping;
        (
            r.handle.clone(),
            r.meeting.clone(),
            r.ticker.clone(),
            r.stopped.clone(),
            r.captions.take(),
        )
    };
    emit_state(app).await;
    ticker.abort();
    if let Some(engine) = captions {
        // Aborts a caption in progress; the full files are transcribed after the stop.
        if let Ok(stats) = tokio::task::spawn_blocking(move || engine.stop()).await {
            tracing::info!(
                windows = stats.windows,
                silent = stats.silent,
                dropped = stats.dropped,
                finals = stats.finals,
                partials = stats.partials,
                errors = stats.errors,
                busy_ms = stats.busy_ms,
                audio_ms = stats.audio_ms,
                "live captions stopped"
            );
        }
    }
    let id = meeting.id;
    let result = tokio::task::spawn_blocking(move || handle.stop()).await;
    let outcome = match result {
        Ok(Ok(tracks)) => {
            let store = st.store.clone();
            match finish_recorded(&store, id, &tracks, Utc::now()).await {
                Ok(()) => {
                    enqueue_pipeline(app, id).await;
                    Ok(id)
                }
                Err(e) => {
                    // Audio is on disk; startup recovery re-attaches it to the meeting.
                    tracing::error!(error = %e, "saving the recorded tracks failed");
                    Err("The recording was saved, but Minutae couldn't update its database. It will retry on the next launch.".to_string())
                }
            }
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "stopping the recording failed");
            let f = Failure {
                kind: FailureKind::Io,
                message: "The recording couldn't be saved.".into(),
                fix: None,
            };
            let _ = st
                .store
                .call(move |s| s.set_meeting_status(id, MeetingStatus::Failed, Some(&f)))
                .await;
            Ok(id)
        }
        Err(e) => {
            tracing::error!(error = %e, "recorder stop task panicked");
            let f = Failure {
                kind: FailureKind::Internal,
                message: "The recording couldn't be saved.".into(),
                fix: None,
            };
            let _ = st
                .store
                .call(move |s| s.set_meeting_status(id, MeetingStatus::Failed, Some(&f)))
                .await;
            Ok(id)
        }
    };
    // Always leave the `Stopping` phase, whatever happened above, so a new recording can
    // start; only now (files and rows written) are waiting callers released.
    *st.recording.lock().await = None;
    signal.finish(outcome.clone());
    emit_state(app).await;
    emit_meetings_changed(app, Some(id));
    outcome
}

/// Stop any recording and wait until the recorder is idle (used by quit, delete, erase).
pub async fn stop_and_wait(app: &AppHandle) {
    if app.state::<AppState>().recording.lock().await.is_some() {
        let _ = stop(app).await;
    }
}

/// Save the track files on the meeting and move it to `processing`.
pub async fn finish_recorded(
    store: &Store,
    id: MeetingId,
    tracks: &RecordedTracks,
    ended_at: chrono::DateTime<Utc>,
) -> minutae_store::Result<()> {
    let mut rows = vec![];
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if tracks.mic.exists() {
        rows.push(TrackFile {
            meeting_id: id,
            track: Track::Mic,
            path: tracks.mic.to_string_lossy().into(),
            duration_ms: Some(tracks.duration_ms),
            bytes: size(&tracks.mic),
        });
    }
    if let Some(sys) = &tracks.system {
        rows.push(TrackFile {
            meeting_id: id,
            track: Track::System,
            path: sys.to_string_lossy().into(),
            duration_ms: Some(tracks.duration_ms),
            bytes: size(sys),
        });
    }
    store
        .call(move |s| {
            for r in &rows {
                s.upsert_track(r)?;
            }
            s.end_meeting(id, ended_at)
        })
        .await
}

/// Duration of a WAV file in ms (`None` if unreadable or empty).
pub fn wav_duration_ms(path: &Path) -> Option<u64> {
    let r = hound::WavReader::open(path).ok()?;
    let spec = r.spec();
    let frames = r.duration() as u64;
    (frames > 0 && spec.sample_rate > 0).then(|| frames * 1000 / spec.sample_rate as u64)
}

/// Already-stitched `mic.wav` / `system.wav` in a recording folder, if any are readable.
pub fn final_tracks(dir: &Path) -> Option<RecordedTracks> {
    let mic = dir.join("mic.wav");
    let system = dir.join("system.wav");
    let mic_ms = wav_duration_ms(&mic);
    let sys_ms = wav_duration_ms(&system);
    let duration_ms = mic_ms.or(sys_ms)?.max(sys_ms.unwrap_or(0));
    Some(RecordedTracks {
        mic,
        system: sys_ms.is_some().then_some(system),
        duration_ms,
    })
}

pub async fn enqueue_pipeline(app: &AppHandle, id: MeetingId) {
    let st = app.state::<AppState>();
    match st.runner() {
        Ok(r) => {
            if let Err(e) = r.enqueue(JobKind::Transcribe, json!({}), Some(id)).await {
                tracing::error!(error = %e, "enqueue transcribe");
            }
        }
        Err(e) => tracing::error!("{e}"),
    }
}

pub async fn set_paused(app: &AppHandle, paused: bool) -> Result<(), String> {
    let st = app.state::<AppState>();
    {
        let mut guard = st.recording.lock().await;
        let Some(r) = guard.as_mut() else {
            return Err("Nothing is recording.".into());
        };
        if r.phase == RecordingPhase::Stopping {
            return Ok(());
        }
        if paused {
            r.handle.pause();
            r.phase = RecordingPhase::Paused;
        } else {
            r.handle.resume();
            r.phase = RecordingPhase::Recording;
        }
        if let Some(c) = &r.captions
            && c.is_enabled()
        {
            c.set_paused(paused);
        }
    }
    emit_state(app).await;
    Ok(())
}

/// The model live captions use: a small fast one when downloaded, else the selected one,
/// else any downloaded model. `None` = no Whisper model at all.
pub fn caption_model(
    downloaded: impl Fn(WhisperModel) -> bool,
    selected: WhisperModel,
) -> Option<WhisperModel> {
    [
        WhisperModel::Base,
        WhisperModel::Tiny,
        selected,
        WhisperModel::Small,
        WhisperModel::Medium,
    ]
    .into_iter()
    .find(|m| downloaded(*m))
}

pub fn caption_status(enabled: bool, model: Option<WhisperModel>) -> CaptionStatus {
    match (enabled, model) {
        (_, None) => CaptionStatus::NoModel,
        (false, Some(_)) => CaptionStatus::Off,
        (true, Some(_)) => CaptionStatus::On,
    }
}

/// Live captions use a few cores at most and run at low priority (see `minutae_asr::captions`).
fn caption_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(1, 4))
        .unwrap_or(2) as u32
}

pub fn caption_line(meeting_id: &str, c: Caption) -> dto::CaptionLine {
    dto::CaptionLine {
        id: c.id,
        meeting_id: meeting_id.to_string(),
        track: c.track,
        speaker: match c.track {
            Track::Mic => "You",
            Track::System => "Them",
        }
        .to_string(),
        at_ms: c.start_ms,
        end_ms: c.end_ms,
        text: c.text,
        is_final: c.is_final,
    }
}

/// Set up live captions for a new recording: the engine (if a model exists) and the taps
/// that feed it from the recorder's writers.
fn start_captions(
    app: &AppHandle,
    settings: &AppSettings,
    meeting_id: MeetingId,
) -> (Option<CaptionEngine>, Taps, CaptionStatus) {
    let st = app.state::<AppState>();
    let models: &ModelManager = &st.models;
    let model = caption_model(|m| models.status(m).downloaded, settings.whisper_model);
    let status = caption_status(settings.live_captions, model);
    let Some(model) = model else {
        return (None, Taps::default(), status);
    };
    let transcriber: Arc<dyn Transcriber> = match st.transcribers.get(models, model) {
        Ok(t) => t,
        Err(f) => {
            tracing::warn!(message = %f.message, "live captions unavailable");
            return (None, Taps::default(), CaptionStatus::NoModel);
        }
    };
    let opts = TranscribeOptions {
        language: settings.language.clone(),
        threads: Some(caption_threads()),
        ..Default::default()
    }
    .with_vocabulary(&settings.vocabulary);
    let emitter = app.clone();
    let id = meeting_id.to_string();
    let sink: CaptionSink = Arc::new(move |c: Caption| {
        let _ = emitter.emit(ev::CAPTION, caption_line(&id, c));
    });
    match CaptionEngine::start(transcriber, opts, CaptionConfig::default(), sink) {
        Ok((engine, mut mic, mut system)) => {
            engine.set_enabled(settings.live_captions);
            tracing::info!(
                ?model,
                enabled = settings.live_captions,
                "live captions ready"
            );
            let taps = Taps {
                mic: Some(Box::new(move |at, block: &[f32]| mic.push(at, block))),
                system: Some(Box::new(move |at, block: &[f32]| system.push(at, block))),
            };
            (Some(engine), taps, status)
        }
        Err(e) => {
            tracing::warn!(error = %e, "live captions failed to start");
            (None, Taps::default(), CaptionStatus::NoModel)
        }
    }
}

/// The live-captions setting changed: apply it to a recording in progress.
pub async fn apply_caption_setting(app: &AppHandle, enabled: bool) {
    let st = app.state::<AppState>();
    {
        let mut guard = st.recording.lock().await;
        let Some(r) = guard.as_mut() else { return };
        let Some(engine) = &r.captions else { return };
        engine.set_enabled(enabled);
        if enabled && r.phase == RecordingPhase::Paused {
            engine.set_paused(true);
        }
        r.caption_status = if enabled {
            CaptionStatus::On
        } else {
            CaptionStatus::Off
        };
    }
    emit_state(app).await;
}

pub async fn mark_moment(app: &AppHandle, label: Option<String>) -> Result<dto::MomentDto, String> {
    let st = app.state::<AppState>();
    let moment = {
        let guard = st.recording.lock().await;
        let Some(r) = guard.as_ref() else {
            return Err("Nothing is recording.".into());
        };
        Moment {
            meeting_id: r.meeting.id,
            at_ms: r.handle.elapsed().as_millis() as u64,
            label: label
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| "Moment".into()),
        }
    };
    let m = moment.clone();
    st.store
        .call(move |s| s.add_moment(&m))
        .await
        .map_err(|e| e.to_string())?;
    Ok(dto::MomentDto::from(&moment))
}

pub async fn toggle(app: &AppHandle) {
    let recording = app.state::<AppState>().recording.lock().await.is_some();
    let r = if recording {
        stop(app).await.map(|_| ())
    } else {
        start(app, None, None, false).await.map(|_| ())
    };
    if let Err(e) = r {
        tracing::warn!("record toggle: {e}");
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title("Minutae").body(e).show();
    }
}

/// Let `<audio>` play files in this folder through the asset protocol.
/// Only recordings folders are ever added; never the top of a disk.
pub fn allow_asset_dir(app: &AppHandle, dir: &Path) {
    if !util::acceptable_folder(dir) {
        tracing::warn!(dir = %dir.display(), "refusing to expose this folder to the webview");
        return;
    }
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        tracing::debug!(error = %e, "asset scope");
    }
}

/// Startup crash recovery: meetings still marked `recording` lost their recorder.
/// Stitch leftover chunks, keep whatever audio exists and queue the pipeline; meetings in
/// `processing` with no job (crash between stop and enqueue) are re-queued.
pub async fn recover(app: &AppHandle) {
    let st = app.state::<AppState>();
    let store = st.store.clone();
    let settings = st.settings();
    let Ok(meetings) = store.call(|s| s.list_meetings(10_000, 0)).await else {
        return;
    };
    let roots = {
        let mut r = vec![minutae_core::paths::recordings_dir()];
        let custom = recordings_root(&settings);
        if !r.contains(&custom) {
            r.push(custom);
        }
        r
    };
    for root in &roots {
        allow_asset_dir(app, root);
    }
    for item in meetings {
        let m = item.meeting;
        // A recording started right after launch is live, not interrupted.
        if st
            .recording
            .lock()
            .await
            .as_ref()
            .is_some_and(|r| r.meeting.id == m.id)
        {
            continue;
        }
        match m.status {
            MeetingStatus::Recording => {
                let id = m.id;
                let dir = roots
                    .iter()
                    .map(|r| r.join(id.to_string()))
                    .find(|d| d.exists());
                let recovered = match dir.clone() {
                    Some(d) => tokio::task::spawn_blocking(move || minutae_audio::recover(&d))
                        .await
                        .ok()
                        .and_then(|r| r.ok())
                        .flatten(),
                    None => None,
                };
                // No chunks left (e.g. stitched, then the database update failed): use
                // the final mic.wav / system.wav already in the meeting's folder.
                let recovered = match (recovered, dir) {
                    (Some(t), _) => Some(t),
                    (None, Some(d)) => tokio::task::spawn_blocking(move || final_tracks(&d))
                        .await
                        .ok()
                        .flatten(),
                    (None, None) => None,
                };
                match recovered.filter(|t| t.mic.exists() || t.system.is_some()) {
                    Some(tracks) => {
                        let ended = m.started_at
                            + chrono::Duration::milliseconds(tracks.duration_ms as i64);
                        tracing::info!(meeting = %id, ms = tracks.duration_ms, "recovered interrupted recording");
                        if finish_recorded(&store, id, &tracks, ended).await.is_ok() {
                            enqueue_pipeline(app, id).await;
                        }
                    }
                    None => {
                        let f = Failure {
                            kind: FailureKind::Io,
                            message: "The recording was interrupted and no audio was saved.".into(),
                            fix: None,
                        };
                        let _ = store
                            .call(move |s| {
                                s.end_meeting(id, Utc::now())?;
                                s.set_meeting_status(id, MeetingStatus::Failed, Some(&f))
                            })
                            .await;
                    }
                }
            }
            MeetingStatus::Processing if item.progress.is_none() => {
                let id = m.id;
                let jobs = store
                    .call(move |s| s.jobs_for_meeting(id))
                    .await
                    .unwrap_or_default();
                let active = jobs.iter().any(|j| {
                    matches!(
                        j.state,
                        minutae_jobs::JobState::Queued | minutae_jobs::JobState::Running
                    )
                });
                if !active && let Ok(runner) = st.runner() {
                    let kind = crate::pipeline::first_missing_step(&store, id)
                        .await
                        .unwrap_or(JobKind::Transcribe);
                    let _ = runner.enqueue(kind, json!({}), Some(id)).await;
                }
            }
            _ => {}
        }
    }
    emit_meetings_changed(app, None);
}

/// Surfaces live capture problems (device unplugged, disk full) instead of silently
/// recording silence. System-audio trouble degrades the meeting to mic-only and shows
/// the reason; losing the mic (or failing to write it) stops and saves what was recorded.
async fn watch_capture(
    app: AppHandle,
    meeting_id: MeetingId,
    mut rx: tokio::sync::broadcast::Receiver<CaptureEvent>,
) {
    use tauri_plugin_notification::NotificationExt;
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        let st = app.state::<AppState>();
        let still_ours = {
            let guard = st.recording.lock().await;
            guard.as_ref().is_some_and(|r| r.meeting.id == meeting_id)
        };
        if !still_ours {
            return;
        }
        match ev {
            CaptureEvent::StreamError { track, message } => {
                tracing::warn!(?track, %message, "capture stream error");
            }
            CaptureEvent::DeviceSwitched { track, device } => {
                tracing::info!(?track, ?device, "capture device switched");
                if track == Track::System {
                    if let Some(r) = st.recording.lock().await.as_mut() {
                        r.system_error = None;
                    }
                    emit_state(&app).await;
                }
            }
            CaptureEvent::TrackLost {
                track: Track::System,
                message,
            }
            | CaptureEvent::WriteFailed {
                track: Track::System,
                message,
            } => {
                tracing::warn!(%message, "system audio lost");
                if let Some(r) = st.recording.lock().await.as_mut() {
                    r.system_error = Some(message);
                }
                emit_state(&app).await;
            }
            CaptureEvent::TrackLost {
                track: Track::Mic,
                message,
            }
            | CaptureEvent::WriteFailed {
                track: Track::Mic,
                message,
            } => {
                tracing::error!(%message, "microphone lost; stopping recording");
                let _ = app
                    .notification()
                    .builder()
                    .title("Recording stopped")
                    .body(format!("{message} What was recorded so far is saved."))
                    .show();
                let _ = stop(&app).await;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn later_stop_callers_wait_for_the_first() {
        let signal = StopSignal::default();
        let id = Uuid::new_v4();
        let waiter = tokio::spawn({
            let s = signal.clone();
            async move { s.wait().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished(), "must wait until the stop finished");
        signal.finish(Ok(id));
        assert_eq!(waiter.await.unwrap(), Ok(id));
        // Callers arriving after completion get the result immediately.
        assert_eq!(signal.wait().await, Ok(id));
    }

    fn write_wav(path: &Path, samples: usize) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for _ in 0..samples {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn caption_model_prefers_small_fast_models() {
        use WhisperModel::*;
        let have = |list: Vec<WhisperModel>| move |m| list.contains(&m);
        assert_eq!(caption_model(have(vec![Small, Base]), Small), Some(Base));
        assert_eq!(caption_model(have(vec![Small, Tiny]), Small), Some(Tiny));
        assert_eq!(
            caption_model(have(vec![Medium, Small]), Medium),
            Some(Medium)
        );
        assert_eq!(caption_model(have(vec![Small]), Medium), Some(Small));
        assert_eq!(caption_model(have(vec![]), Small), None);
        assert_eq!(caption_status(true, None), CaptionStatus::NoModel);
        assert_eq!(caption_status(false, Some(Base)), CaptionStatus::Off);
        assert_eq!(caption_status(true, Some(Base)), CaptionStatus::On);
    }

    #[test]
    fn caption_lines_name_the_track() {
        let c = |track| Caption {
            id: 7,
            track,
            start_ms: 3_500,
            end_ms: 7_500,
            text: "Hello".into(),
            is_final: false,
        };
        let you = caption_line("m", c(Track::Mic));
        assert_eq!(
            (you.speaker.as_str(), you.at_ms, you.end_ms),
            ("You", 3_500, 7_500)
        );
        assert_eq!(caption_line("m", c(Track::System)).speaker, "Them");
        let v = serde_json::to_value(&you).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "atMs",
                "endMs",
                "final",
                "id",
                "meetingId",
                "speaker",
                "text",
                "track"
            ]
        );
        assert_eq!(v["final"], false);
        assert_eq!(v["track"], "mic");
    }

    #[test]
    fn final_tracks_are_reattached_without_chunks() {
        let dir = tempfile::tempdir().unwrap();
        assert!(final_tracks(dir.path()).is_none());
        write_wav(&dir.path().join("mic.wav"), 32_000);
        let t = final_tracks(dir.path()).unwrap();
        assert_eq!(t.duration_ms, 2_000);
        assert!(t.system.is_none());
        write_wav(&dir.path().join("system.wav"), 48_000);
        let t = final_tracks(dir.path()).unwrap();
        assert_eq!(t.duration_ms, 3_000);
        assert!(t.system.is_some());
    }
}
