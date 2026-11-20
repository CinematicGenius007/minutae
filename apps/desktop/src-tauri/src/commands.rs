//! Every `#[tauri::command]` in `apps/desktop/src/lib/api.ts`, same names and args.
//! Errors reach the UI as one plain sentence.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Datelike, Local, NaiveDate, TimeZone, Utc};
use minutae_core::{AppSettings, MeetingStatus, Stage, Transport, WhisperModel, paths};
use minutae_store::{SearchFilters, SearchKind};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use crate::dto::{self, ev};
use crate::pipeline::find_summary_file;
use crate::state::{AppState, emit_meetings_changed, recordings_root, summaries_root};
use crate::updater::Updates;
use crate::{calls, models, platform, recording, shell, util};

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn uuid(s: &str) -> R<Uuid> {
    Uuid::parse_str(s).map_err(|_| format!("Not a valid id: {s}"))
}

// ---------------------------------------------------------------- app

#[tauri::command]
pub fn platform(app: AppHandle) -> dto::PlatformInfo {
    platform::info(app.package_info().version.to_string())
}

// ---------------------------------------------------------------- updates

#[tauri::command]
pub fn update_status(updates: State<'_, Arc<Updates>>) -> dto::UpdateStatus {
    updates.status()
}

/// Starts a check and returns at once (`checking`, or the current status when updates are
/// off or already busy). The result arrives on `update://status`.
#[tauri::command]
pub fn check_for_updates(updates: State<'_, Arc<Updates>>) -> dto::UpdateStatus {
    let Some(s) = updates.begin_check() else {
        return updates.status();
    };
    let u = updates.inner().clone();
    tauri::async_runtime::spawn(async move {
        u.run_check().await;
    });
    s
}

/// Downloads and installs the update found by the last check, then restarts the app
/// (graceful quit first). Returns at once; progress and errors arrive on `update://status`.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, Arc<Updates>>,
) -> R<dto::UpdateStatus> {
    if state.recording.lock().await.is_some() {
        return Err("Stop the recording before installing the update.".into());
    }
    let s = updates.begin_install()?;
    let u = updates.inner().clone();
    tauri::async_runtime::spawn(async move {
        if u.run_install().await.is_ok() {
            shell::restart(&app);
        }
    });
    Ok(s)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings()
}

/// Merge a partial `AppSettings` JSON object into the current settings.
pub fn merge_settings(current: &AppSettings, patch: serde_json::Value) -> R<AppSettings> {
    let mut base = serde_json::to_value(current).map_err(err)?;
    let (Some(obj), serde_json::Value::Object(p)) = (base.as_object_mut(), patch) else {
        return Err("Settings patch must be an object.".into());
    };
    for (k, v) in p {
        if obj.contains_key(&k) {
            obj.insert(k, v);
        }
    }
    serde_json::from_value(base).map_err(|e| format!("Invalid setting: {e}"))
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: serde_json::Value) -> R<AppSettings> {
    // Serialise read-merge-write so two quick toggles can't overwrite each other.
    static UPDATING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = UPDATING.lock().await;
    let st = app.state::<AppState>();
    let old = st.settings();
    let new = merge_settings(&old, patch)?;
    {
        let picked = st.picked_folders.lock().unwrap_or_else(|e| e.into_inner());
        check_folder_setting(
            old.recordings_path.as_deref(),
            new.recordings_path.as_deref(),
            &picked,
        )?;
        check_folder_setting(
            old.summaries_path.as_deref(),
            new.summaries_path.as_deref(),
            &picked,
        )?;
    }
    let shortcuts_changed =
        old.record_shortcut != new.record_shortcut || old.dictation_hotkey != new.dictation_hotkey;
    if shortcuts_changed {
        check_shortcuts(&old, &new)?;
        // Register the new pair now; if the changed one is refused, the old pair stays.
        let failed = shell::register_shortcuts(&app, &new);
        let changed_failed = failed.iter().any(|f| {
            (f == &new.record_shortcut && new.record_shortcut != old.record_shortcut)
                || (f == &new.dictation_hotkey && new.dictation_hotkey != old.dictation_hotkey)
        });
        if changed_failed {
            shell::register_shortcuts(&app, &old);
            return Err("That shortcut is already used by another app".into());
        }
    }
    let s = new.clone();
    if let Err(e) = st.store.call(move |store| store.set_settings(&s)).await {
        if shortcuts_changed {
            shell::register_shortcuts(&app, &old);
        }
        return Err(err(e));
    }
    *st.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
    crate::apply_settings(&app, &old, &new);
    let _ = app.emit(ev::SETTINGS_CHANGED, &new);
    Ok(new)
}

#[tauri::command]
pub fn permissions() -> dto::Permissions {
    platform::permissions()
}

#[tauri::command]
pub async fn request_permission(kind: dto::PermissionKind) -> R<dto::Permissions> {
    tokio::task::spawn_blocking(move || platform::request(kind))
        .await
        .map_err(err)
}

#[tauri::command]
pub fn open_system_settings(app: AppHandle, kind: dto::PermissionKind) -> R<()> {
    let url = platform::settings_url(kind).ok_or("Open your system settings to change this.")?;
    app.opener().open_url(url, None::<&str>).map_err(err)
}

/// Only Minutae's own folders (and the last export) may be opened or revealed.
fn allowed_path(app: &AppHandle, path: &str, open: bool) -> R<PathBuf> {
    let st = app.state::<AppState>();
    let roots = crate::state::user_path_roots(&st.settings());
    let exact: Vec<PathBuf> = st
        .last_export
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    util::checked_user_path(path, &roots, &exact, open)
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> R<()> {
    let p = allowed_path(&app, &path, false)?;
    app.opener().reveal_item_in_dir(&p).map_err(err)
}

#[tauri::command]
pub fn open_path(app: AppHandle, path: String) -> R<()> {
    let p = allowed_path(&app, &path, true)?;
    app.opener()
        .open_path(p.to_string_lossy(), None::<&str>)
        .map_err(err)
}

/// A custom recordings/summaries folder is accepted only if it is unchanged, cleared
/// (back to the default), or one the user picked with `pick_folder` this session.
pub fn check_folder_setting(
    old: Option<&str>,
    new: Option<&str>,
    picked: &std::collections::HashSet<PathBuf>,
) -> R<()> {
    let norm = |v: Option<&str>| {
        v.map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
    };
    let Some(new) = norm(new) else { return Ok(()) };
    if norm(old).as_ref() == Some(&new) {
        return Ok(());
    }
    if !util::acceptable_folder(&new) {
        return Err("Choose a different folder.".into());
    }
    if !picked.contains(&new) {
        return Err("Choose that folder again from Settings.".into());
    }
    Ok(())
}

/// Validate a new record/dictation shortcut against the other one (see
/// [`util::validate_accelerator`]).
fn check_shortcuts(old: &AppSettings, new: &AppSettings) -> R<()> {
    if new.record_shortcut != old.record_shortcut {
        util::validate_accelerator(&new.record_shortcut, &new.dictation_hotkey)?;
    }
    if new.dictation_hotkey != old.dictation_hotkey {
        util::validate_accelerator(&new.dictation_hotkey, &new.record_shortcut)?;
    }
    Ok(())
}

/// Kept alive for the whole run: on Linux (X11/Wayland) the clipboard owner must outlive
/// the copy or the text disappears as soon as the `Clipboard` is dropped.
static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);

#[tauri::command]
pub fn copy_text(text: String) -> R<()> {
    let mut slot = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(arboard::Clipboard::new().map_err(|e| {
            tracing::warn!(error = %e, "clipboard unavailable");
            "The clipboard isn't available.".to_string()
        })?);
    }
    let r = slot.as_mut().map(|c| c.set_text(text));
    match r {
        Some(Ok(())) => Ok(()),
        other => {
            if let Some(Err(e)) = other {
                tracing::warn!(error = %e, "copy failed");
            }
            *slot = None;
            Err("Couldn't copy to the clipboard.".into())
        }
    }
}

/// True once finished, or right away when this OS has no permission to grant
/// (Windows/Linux), so the UI never shows an empty onboarding.
#[tauri::command]
pub fn onboarding_done(state: State<'_, AppState>) -> bool {
    state.flags().onboarding_done || platform::nothing_to_grant(&platform::permissions())
}

#[tauri::command]
pub fn finish_onboarding(state: State<'_, AppState>) {
    let mut f = state.flags();
    f.onboarding_done = true;
    f.save();
}

// ---------------------------------------------------------------- meetings

fn status_line(item: &minutae_store::MeetingListItem) -> Option<String> {
    let local = if cfg!(target_os = "macos") {
        "this Mac"
    } else if cfg!(target_os = "windows") {
        "this PC"
    } else {
        "this computer"
    };
    match item.meeting.status {
        MeetingStatus::Recording => Some("Recording…".into()),
        MeetingStatus::Processing => Some(match item.progress.as_ref().map(|p| p.stage) {
            Some(Stage::Transcribing) | None => format!("Transcribing on {local}…"),
            Some(Stage::Diarizing) => "Labelling speakers…".into(),
            Some(Stage::Summarizing) => "Writing the summary…".into(),
            Some(Stage::Indexing) => "Almost done…".into(),
            Some(Stage::DownloadingModel) => "Downloading the speech model…".into(),
        }),
        _ => item.takeaway.clone(),
    }
}

#[tauri::command]
pub async fn list_meetings(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> R<Vec<dto::DayGroup>> {
    let limit = limit.unwrap_or(200).clamp(1, 10_000);
    let (items, totals) = state
        .store
        .call(move |s| {
            let items = s.list_meetings(limit, 0)?;
            let totals: HashMap<String, u32> = s.read(|c| {
                let mut stmt = c.prepare_cached(
                    "SELECT meeting_id, count(*) FROM action_items GROUP BY meeting_id",
                )?;
                let rows =
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
                Ok(rows.collect::<Result<_, _>>()?)
            })?;
            Ok((items, totals))
        })
        .await
        .map_err(err)?;
    let today = Local::now().date_naive();
    let mut groups: Vec<dto::DayGroup> = Vec::new();
    for item in items {
        let label = util::day_label(
            item.meeting.started_at.with_timezone(&Local).date_naive(),
            today,
        );
        let id = item.meeting.id.to_string();
        let row = dto::MeetingListItem {
            total_actions: totals.get(&id).copied().unwrap_or(0),
            takeaway: status_line(&item),
            id,
            title: item.meeting.title.clone(),
            started_at: item.meeting.started_at.to_rfc3339(),
            duration_ms: item.duration_ms,
            status: item.meeting.status,
            people: item
                .people
                .iter()
                .filter(|p| p.as_str() != minutae_diarize::YOU_LABEL)
                .cloned()
                .collect(),
            open_actions: item.open_actions,
            progress: item.progress.clone(),
            failure: item.meeting.failure.clone(),
        };
        match groups.last_mut() {
            Some(g) if g.label == label => g.items.push(row),
            _ => groups.push(dto::DayGroup {
                label,
                items: vec![row],
            }),
        }
    }
    Ok(groups)
}

/// Monday 00:00 local time of the current week.
fn week_start() -> chrono::DateTime<Utc> {
    let today = Local::now().date_naive();
    let monday = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    local_midnight(monday)
}

fn local_midnight(d: NaiveDate) -> chrono::DateTime<Utc> {
    Local
        .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

#[tauri::command]
pub async fn week_stats(state: State<'_, AppState>) -> R<dto::WeekStats> {
    let since = week_start();
    let s = state
        .store
        .call(move |s| s.week_stats(since))
        .await
        .map_err(err)?;
    Ok(dto::WeekStats {
        recorded_ms: s.recorded_ms,
        open_actions: s.open_actions,
    })
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, AppState>, id: String) -> R<dto::MeetingDetail> {
    let id = uuid(&id)?;
    let d = state
        .store
        .call(move |s| s.meeting_detail(id))
        .await
        .map_err(err)?
        .ok_or("This meeting no longer exists.")?;
    let summaries_dir = summaries_root(&state.settings());
    let markdown_path = find_summary_file(&summaries_dir, id);
    let duration_ms = d
        .meeting
        .ended_at
        .map(|e| (e - d.meeting.started_at).num_milliseconds().max(0) as u64)
        .or_else(|| d.tracks.iter().filter_map(|t| t.duration_ms).max());
    let existing = |t: minutae_core::Track| {
        d.tracks
            .iter()
            .find(|x| x.track == t)
            .map(|x| x.path.clone())
            .filter(|p| Path::new(p).exists())
    };
    let audio = (!d.tracks.is_empty()).then(|| dto::AudioPaths {
        mic: existing(minutae_core::Track::Mic),
        system: existing(minutae_core::Track::System),
    });
    let summary = d.summary.as_ref().map(|s| dto::Summary {
        overview: s.overview.clone(),
        points: util::markdown_bullets(&s.markdown, "Summary"),
        decisions: s.decisions.clone(),
        takeaway: s.takeaway.clone(),
        markdown: s.markdown.clone(),
        markdown_path: markdown_path.map(|p| p.to_string_lossy().into_owned()),
        provider: provider_label(&s.provider),
        model: s.model.clone(),
    });
    Ok(dto::MeetingDetail {
        meeting: dto::Meeting::from(&d.meeting),
        duration_ms,
        speakers: d.speakers.iter().map(dto::Speaker::from).collect(),
        segments: d.transcript.map(|t| t.segments).unwrap_or_default(),
        summary,
        action_items: d.action_items.iter().map(dto::ActionItem::from).collect(),
        moments: d.moments.iter().map(dto::MomentDto::from).collect(),
        audio,
        progress: d.progress,
    })
}

#[tauri::command]
pub async fn rename_meeting(app: AppHandle, id: String, title: String) -> R<()> {
    let id = uuid(&id)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("The title can't be empty.".into());
    }
    let st = app.state::<AppState>();
    let t = title.clone();
    st.store
        .call(move |s| s.rename_meeting(id, &t))
        .await
        .map_err(err)?;
    if let Some(r) = st.recording.lock().await.as_mut()
        && r.meeting.id == id
    {
        r.meeting.title = title;
    }
    emit_meetings_changed(&app, Some(id));
    Ok(())
}

#[tauri::command]
pub async fn delete_meeting(app: AppHandle, id: String) -> R<()> {
    let id = uuid(&id)?;
    let st = app.state::<AppState>();
    let recording_this = st
        .recording
        .lock()
        .await
        .as_ref()
        .is_some_and(|r| r.meeting.id == id);
    if recording_this {
        // Waits for a stop already in progress, so the stitch can't recreate files
        // after they are deleted below.
        let _ = recording::stop(&app).await;
    }
    let store = st.store.clone();
    if let Ok(runner) = st.runner() {
        let jobs = store
            .call(move |s| s.jobs_for_meeting(id))
            .await
            .map_err(err)?;
        for j in jobs.iter().filter(|j| !j.state.is_terminal()) {
            let _ = runner.cancel(j.id).await;
        }
    }
    let tracks = store.call(move |s| s.list_tracks(id)).await.map_err(err)?;
    let settings = st.settings();
    let summaries = summaries_root(&settings);
    tokio::task::spawn_blocking(move || {
        let mut dirs: Vec<PathBuf> = vec![];
        for t in &tracks {
            let p = PathBuf::from(&t.path);
            let _ = std::fs::remove_file(&p);
            if let Some(parent) = p.parent() {
                dirs.push(parent.to_path_buf());
            }
        }
        dirs.push(recordings_root(&settings).join(id.to_string()));
        for d in dirs {
            // Only the meeting's own folder, and only when it is named after the meeting.
            if d.file_name().and_then(|n| n.to_str()) == Some(&id.to_string()) {
                let _ = std::fs::remove_dir_all(d);
            }
        }
        if let Some(md) = find_summary_file(&summaries, id) {
            let _ = std::fs::remove_file(md);
        }
    })
    .await
    .map_err(err)?;
    store
        .call(move |s| s.delete_meeting(id))
        .await
        .map_err(err)?;
    emit_meetings_changed(&app, Some(id));
    Ok(())
}

#[tauri::command]
pub async fn retry_meeting(app: AppHandle, id: String) -> R<()> {
    let id = uuid(&id)?;
    let st = app.state::<AppState>();
    crate::pipeline::retry_meeting(st.runner()?, id).await?;
    emit_meetings_changed(&app, Some(id));
    Ok(())
}

#[tauri::command]
pub async fn set_action_done(app: AppHandle, id: String, done: bool) -> R<()> {
    let row: i64 = id
        .parse()
        .map_err(|_| format!("Not a valid action item id: {id}"))?;
    let st = app.state::<AppState>();
    st.store
        .call(move |s| s.set_action_item_done(row, done))
        .await
        .map_err(err)?;
    emit_meetings_changed(&app, None);
    Ok(())
}

#[tauri::command]
pub async fn rename_speaker(app: AppHandle, speaker_id: String, name: String) -> R<()> {
    let sid = uuid(&speaker_id)?;
    let st = app.state::<AppState>();
    // An empty name resets the speaker to its label ("Speaker 1").
    let name = Some(name.trim().to_string()).filter(|n| !n.is_empty());
    st.store
        .call(move |s| s.rename_speaker(sid, name.as_deref()))
        .await
        .map_err(err)?;
    emit_meetings_changed(&app, None);
    Ok(())
}

#[tauri::command]
pub async fn summary_markdown(state: State<'_, AppState>, id: String) -> R<String> {
    let id = uuid(&id)?;
    if let Some(p) = find_summary_file(&summaries_root(&state.settings()), id)
        && let Ok(s) = std::fs::read_to_string(p)
    {
        return Ok(s);
    }
    let s = state
        .store
        .call(move |s| s.get_summary(id))
        .await
        .map_err(err)?;
    Ok(s.map(|s| s.markdown).unwrap_or_default())
}

// ---------------------------------------------------------------- recording

#[tauri::command]
pub async fn recording_state(app: AppHandle) -> dto::RecordingState {
    recording::current_state(&app).await
}

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    title: Option<String>,
    source_app: Option<String>,
) -> R<dto::Meeting> {
    recording::start(&app, title, source_app, false).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> R<String> {
    recording::stop(&app).await.map(|id| id.to_string())
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle) -> R<()> {
    recording::set_paused(&app, true).await
}

#[tauri::command]
pub async fn resume_recording(app: AppHandle) -> R<()> {
    recording::set_paused(&app, false).await
}

#[tauri::command]
pub async fn mark_moment(app: AppHandle, label: Option<String>) -> R<dto::MomentDto> {
    recording::mark_moment(&app, label).await
}

#[tauri::command]
pub async fn accept_call_prompt(app: AppHandle) -> R<dto::Meeting> {
    calls::accept(&app).await
}

#[tauri::command]
pub fn dismiss_call_prompt(app: AppHandle, always: Option<bool>) {
    calls::dismiss(&app, always.unwrap_or(false));
}

#[tauri::command]
pub async fn list_input_devices() -> R<Vec<dto::InputDevice>> {
    let list = tokio::task::spawn_blocking(minutae_audio::list_input_devices)
        .await
        .map_err(err)?
        .map_err(err)?;
    Ok(list
        .into_iter()
        .map(|d| dto::InputDevice {
            id: d.id,
            name: d.name,
            is_default: d.is_default,
        })
        .collect())
}

// ---------------------------------------------------------------- dictation

#[tauri::command]
pub async fn list_dictations(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> R<Vec<dto::Dictation>> {
    let limit = limit.unwrap_or(100).clamp(1, 10_000);
    let v = state
        .store
        .call(move |s| s.list_dictations(limit, 0))
        .await
        .map_err(err)?;
    Ok(v.iter().map(dto::Dictation::from).collect())
}

/// Minutes saved = typing time at 40 wpm minus speaking time.
pub fn minutes_saved(words: u64, spoken_ms: u64) -> u64 {
    let typing_min = words as f64 / 40.0;
    let spoken_min = spoken_ms as f64 / 60_000.0;
    (typing_min - spoken_min).max(0.0).round() as u64
}

#[tauri::command]
pub async fn dictation_stats(state: State<'_, AppState>) -> R<dto::DictationStats> {
    let since = week_start();
    let s = state
        .store
        .call(move |s| s.dictation_stats(since))
        .await
        .map_err(err)?;
    Ok(dto::DictationStats {
        words_this_week: s.words,
        minutes_saved: minutes_saved(s.words, s.duration_ms),
    })
}

#[tauri::command]
pub async fn delete_dictation(app: AppHandle, id: String) -> R<()> {
    let id = uuid(&id)?;
    app.state::<AppState>()
        .store
        .call(move |s| s.delete_dictation(id))
        .await
        .map_err(err)?;
    let _ = std::fs::remove_file(crate::dictation::dictation_audio_path(id));
    let _ = app.emit(ev::DICTATIONS_CHANGED, ());
    Ok(())
}

// ---------------------------------------------------------------- library

#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    query: String,
    kind: Option<SearchKind>,
) -> R<Vec<dto::SearchHit>> {
    let filters = SearchFilters {
        kind: kind.unwrap_or_default(),
        ..Default::default()
    };
    let hits = state
        .store
        .call(move |s| s.search(&query, &filters))
        .await
        .map_err(err)?;
    Ok(hits.iter().map(dto::SearchHit::from).collect())
}

#[tauri::command]
pub async fn ask(state: State<'_, AppState>, question: String) -> R<dto::Answer> {
    let store = state.store.clone();
    let q = question.clone();
    let chunks =
        tokio::task::spawn_blocking(move || minutae_search::Retriever::new(store).retrieve(&q, 8))
            .await
            .map_err(err)?
            .map_err(err)?;
    let ctx: Vec<minutae_llm::ContextChunk> = chunks
        .iter()
        .map(|c| minutae_llm::ContextChunk {
            meeting_title: c.title.clone(),
            start_ms: c.start_ms,
            text: c.text.clone(),
        })
        .collect();
    let router = state.router();
    let answer = minutae_llm::ask(&router, &question, &ctx)
        .await
        .map_err(|e| e.failure().message)?;
    let sources = answer
        .sources
        .iter()
        .filter_map(|&i| chunks.get(i))
        .map(|c| dto::SearchHit {
            kind: dto::HitKind::Meeting,
            meeting_id: Some(c.meeting_id.to_string()),
            dictation_id: None,
            title: c.title.clone(),
            snippet: minutae_llm::summary::shorten(&c.text.replace('\n', " "), 200),
            highlights: vec![],
            at_ms: Some(c.start_ms),
            date: c.meeting_started_at.to_rfc3339(),
        })
        .collect();
    Ok(dto::Answer {
        text: answer.text,
        sources,
        provider: answer
            .provider
            .as_deref()
            .map(provider_label)
            .unwrap_or_default(),
    })
}

// ---------------------------------------------------------------- models

fn whisper_model(s: &str) -> R<WhisperModel> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|_| format!("Unknown model: {s}"))
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Vec<dto::ModelInfo> {
    models::list(&state)
}

#[tauri::command]
pub fn download_model(app: AppHandle, model: String) -> R<()> {
    models::download(&app, whisper_model(&model)?);
    Ok(())
}

#[tauri::command]
pub fn cancel_model_download(app: AppHandle, model: String) -> R<()> {
    models::cancel(&app, &models::key(whisper_model(&model)?));
    Ok(())
}

#[tauri::command]
pub fn delete_model(app: AppHandle, model: String) -> R<()> {
    models::delete(&app, whisper_model(&model)?)
}

#[tauri::command]
pub fn diarize_model(state: State<'_, AppState>) -> dto::DiarizeModelInfo {
    models::diarize_info(&state)
}

#[tauri::command]
pub fn download_diarize_model(app: AppHandle) {
    models::download_diarize(&app);
}

// ---------------------------------------------------------------- providers

/// Display name for a usage/summary provider id.
pub fn provider_label(id: &str) -> String {
    let local = if cfg!(target_os = "macos") {
        "on this Mac"
    } else if cfg!(target_os = "windows") {
        "on this PC"
    } else {
        "on this computer"
    };
    match id {
        "codex-cli" | "codexCli" | "codex" => "Codex".into(),
        "grok-cli" | "grokCli" | "grok" => "Grok".into(),
        "ollama" => format!("Ollama · {local}"),
        "whisper" => format!("Whisper · {local}"),
        "anthropic" => "Anthropic · API fallback".into(),
        "openai" => "OpenAI · API fallback".into(),
        "gemini" => "Gemini · API fallback".into(),
        other => other.to_string(),
    }
}

#[tauri::command]
pub async fn provider_statuses(state: State<'_, AppState>) -> R<Vec<dto::ProviderStatus>> {
    let settings = state.settings();
    let router = state.router();
    let mut order: Vec<Transport> = settings
        .transport_order
        .iter()
        .copied()
        .filter(|t| *t != Transport::Api)
        .collect();
    for t in [Transport::CodexCli, Transport::Ollama, Transport::GrokCli] {
        if !order.contains(&t) {
            order.push(t);
        }
    }
    order.push(Transport::Api);
    let mut out = vec![];
    for t in order {
        let Some(tr) = router.transport(t) else {
            continue;
        };
        let a = tr.availability().await;
        let (availability, mut detail) = match &a {
            minutae_llm::Availability::Ready => (dto::Availability::Ready, None),
            minutae_llm::Availability::NotInstalled => (dto::Availability::NotInstalled, None),
            minutae_llm::Availability::NotLoggedIn => (dto::Availability::NotLoggedIn, None),
            minutae_llm::Availability::Unavailable(r) => {
                (dto::Availability::Unavailable, Some(r.clone()))
            }
        };
        let (name, model) = match t {
            // Configured model, else the one Codex auto-selection picked from `codex debug models`.
            Transport::CodexCli => (
                "Codex".to_string(),
                match settings
                    .codex_model
                    .clone()
                    .filter(|m| !m.trim().is_empty())
                {
                    Some(m) => Some(m),
                    None => tr.model().await,
                },
            ),
            Transport::GrokCli => ("Grok".to_string(), None),
            Transport::Ollama => (
                provider_label("ollama"),
                Some(settings.ollama_model.clone()),
            ),
            Transport::Api => ("API key".to_string(), None),
        };
        if detail.is_none() {
            detail = Some(match (t, availability) {
                (Transport::CodexCli, dto::Availability::Ready) => {
                    "Uses your ChatGPT sign-in. No API key needed.".into()
                }
                (Transport::CodexCli, dto::Availability::NotLoggedIn) => {
                    "Run `codex login` to sign in.".into()
                }
                (Transport::Ollama, dto::Availability::Ready) => {
                    format!(
                        "Fully offline. {} · slower on long meetings.",
                        settings.ollama_model
                    )
                }
                (Transport::GrokCli, dto::Availability::NotLoggedIn) => {
                    "Sign in to the Grok CLI.".into()
                }
                (Transport::Api, dto::Availability::NotLoggedIn) => "Needs an API key.".into(),
                (Transport::Api, dto::Availability::Ready) if !settings.api_fallback_enabled => {
                    "Key saved. Turn on the API fallback to use it.".into()
                }
                (_, dto::Availability::NotInstalled) => format!("{name} isn't installed."),
                _ => String::new(),
            })
            .filter(|d: &String| !d.is_empty());
        }
        out.push(dto::ProviderStatus {
            transport: t,
            name,
            availability,
            detail,
            model,
        });
    }
    Ok(out)
}

#[tauri::command]
pub async fn set_api_key(app: AppHandle, provider: dto::ApiProvider, key: String) -> R<()> {
    use minutae_llm::SecretStore;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("The key is empty.".into());
    }
    let st = app.state::<AppState>();
    let secrets = st.secrets.clone();
    tokio::task::spawn_blocking(move || secrets.set(provider.secret_account(), &key))
        .await
        .map_err(err)?
        .map_err(|e| e.detail)?;
    st.rebuild_router(&st.settings());
    Ok(())
}

#[tauri::command]
pub async fn clear_api_key(app: AppHandle, provider: dto::ApiProvider) -> R<()> {
    use minutae_llm::SecretStore;
    let st = app.state::<AppState>();
    let secrets = st.secrets.clone();
    tokio::task::spawn_blocking(move || secrets.delete(provider.secret_account()))
        .await
        .map_err(err)?
        .map_err(|e| e.detail)?;
    st.rebuild_router(&st.settings());
    Ok(())
}

#[tauri::command]
pub async fn api_key_hint(
    state: State<'_, AppState>,
    provider: dto::ApiProvider,
) -> R<Option<String>> {
    use minutae_llm::SecretStore;
    let secrets = state.secrets.clone();
    let key = tokio::task::spawn_blocking(move || secrets.get(provider.secret_account()))
        .await
        .map_err(err)?
        .map_err(|e| e.detail)?;
    Ok(key.map(|k| util::mask_key(&k)))
}

// ---------------------------------------------------------------- usage + storage

pub fn usage_month_from(
    store: &minutae_store::Store,
    year: i32,
    month: u32,
) -> minutae_store::Result<dto::UsageMonth> {
    let start = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| minutae_store::StoreError::Invalid("bad month".into()))?;
    let end = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .ok_or_else(|| minutae_store::StoreError::Invalid("bad month".into()))?;
    let (from, to) = (
        local_midnight(start).timestamp_millis(),
        local_midnight(end).timestamp_millis(),
    );
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, i64, f64, bool)> = store.read(|c| {
        let mut stmt = c.prepare_cached(
            "SELECT at, provider, purpose, tokens_in + tokens_out, cost_usd, ok FROM api_usage WHERE at >= ?1 AND at < ?2",
        )?;
        let rows = stmt.query_map([from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    })?;
    let mut by_provider: Vec<dto::UsageByProvider> = vec![];
    let days = (end - start).num_days() as u32;
    let mut by_day: Vec<dto::UsageDay> = (1..=days)
        .map(|day| dto::UsageDay { day, cost_usd: 0.0 })
        .collect();
    let (mut total, mut tokens, mut summaries, mut free, mut calls) = (0.0, 0u64, 0u64, 0u64, 0u64);
    for (at, provider, purpose, tok, cost, ok) in rows {
        calls += 1;
        total += cost;
        tokens += tok.max(0) as u64;
        if cost <= 0.0 {
            free += 1;
        }
        if purpose == "summary" && ok {
            summaries += 1;
        }
        let label = provider_label(&provider);
        match by_provider.iter_mut().find(|p| p.provider == label) {
            Some(p) => {
                p.calls += 1;
                p.tokens += tok.max(0) as u64;
                p.cost_usd += cost;
            }
            None => by_provider.push(dto::UsageByProvider {
                provider: label,
                calls: 1,
                tokens: tok.max(0) as u64,
                cost_usd: cost,
            }),
        }
        let day = Utc
            .timestamp_millis_opt(at)
            .single()
            .map(|t| t.with_timezone(&Local).day())
            .unwrap_or(1);
        if let Some(d) = by_day.get_mut(day as usize - 1) {
            d.cost_usd += cost;
        }
    }
    by_provider.sort_by_key(|a| std::cmp::Reverse(a.calls));
    Ok(dto::UsageMonth {
        year,
        month,
        total_usd: total,
        summaries,
        tokens,
        free_share: if calls == 0 {
            0.0
        } else {
            free as f64 / calls as f64
        },
        by_provider,
        by_day,
    })
}

#[tauri::command]
pub async fn usage_month(state: State<'_, AppState>, year: i32, month: u32) -> R<dto::UsageMonth> {
    state
        .store
        .call(move |s| usage_month_from(s, year, month))
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn storage_usage(state: State<'_, AppState>) -> R<dto::StorageUsage> {
    let settings = state.settings();
    tokio::task::spawn_blocking(move || {
        let mut recordings = util::dir_size(&paths::recordings_dir());
        let custom = recordings_root(&settings);
        if custom != paths::recordings_dir() {
            recordings += util::dir_size(&custom);
        }
        let db = paths::database_file();
        let index = ["", "-wal", "-shm"]
            .iter()
            .map(|s| util::dir_size(Path::new(&format!("{}{s}", db.display()))))
            .sum();
        dto::StorageUsage {
            recordings_bytes: recordings,
            models_bytes: util::dir_size(&paths::models_dir()),
            index_bytes: index,
            data_dir: paths::data_dir().to_string_lossy().into_owned(),
        }
    })
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> R<Option<String>> {
    let a = app.clone();
    let picked = tokio::task::spawn_blocking(move || {
        a.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(err)?;
    let Some(p) = picked else { return Ok(None) };
    if !util::acceptable_folder(&p) {
        return Err("Choose a folder inside your disk, not the top of it.".into());
    }
    let s = p.to_string_lossy().into_owned();
    app.state::<AppState>()
        .picked_folders
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(PathBuf::from(s.trim()));
    Ok(Some(s))
}

/// Zip the data folder (database, recordings, summaries; not the downloadable models).
pub fn write_export(
    dest: &Path,
    data_dir: &Path,
    extra: &[(PathBuf, String)],
) -> std::io::Result<()> {
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let models = paths::models_dir();
    let mut add = |path: &Path, name: String| -> std::io::Result<()> {
        if path == dest || path.starts_with(&models) {
            return Ok(());
        }
        zip.start_file(name.replace('\\', "/"), opts)
            .map_err(std::io::Error::other)?;
        let mut f = std::fs::File::open(path)?;
        std::io::copy(&mut f, &mut zip)?;
        Ok(())
    };
    for p in util::walk_files(data_dir) {
        let rel = p
            .strip_prefix(data_dir)
            .unwrap_or(&p)
            .to_string_lossy()
            .into_owned();
        add(&p, rel)?;
    }
    for (root, prefix) in extra {
        for p in util::walk_files(root) {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned();
            add(&p, format!("{prefix}/{rel}"))?;
        }
    }
    zip.finish().map_err(std::io::Error::other)?.flush()
}

#[tauri::command]
pub async fn export_all(app: AppHandle) -> R<Option<String>> {
    let st = app.state::<AppState>();
    let name = format!("minutae-export-{}.zip", Local::now().format("%Y-%m-%d"));
    let a = app.clone();
    let dest = tokio::task::spawn_blocking(move || {
        a.dialog()
            .file()
            .set_file_name(name)
            .add_filter("Zip archive", &["zip"])
            .blocking_save_file()
    })
    .await
    .map_err(err)?
    .and_then(|p| p.into_path().ok());
    let Some(dest) = dest else { return Ok(None) };
    // Fold the WAL into the main file so the copy is consistent.
    let _ = st
        .store
        .call(|s| s.write(|c| Ok(c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?)))
        .await;
    let settings = st.settings();
    let mut extra = vec![];
    let rec = recordings_root(&settings);
    if !rec.starts_with(paths::data_dir()) {
        extra.push((rec, "recordings-custom".to_string()));
    }
    let sum = summaries_root(&settings);
    if !sum.starts_with(paths::data_dir()) {
        extra.push((sum, "summaries-custom".to_string()));
    }
    let d = dest.clone();
    tokio::task::spawn_blocking(move || write_export(&d, &paths::data_dir(), &extra))
        .await
        .map_err(err)?
        .map_err(|e| format!("The export couldn't be written: {e}"))?;
    *st.last_export.lock().unwrap_or_else(|e| e.into_inner()) = Some(dest.clone());
    Ok(Some(dest.to_string_lossy().into_owned()))
}

/// Delete every meeting, dictation (and kept dictation audio), recording, summary file,
/// usage row, flag, setting, saved API key and downloaded model.
#[tauri::command]
pub async fn erase_all(app: AppHandle) -> R<()> {
    let st = app.state::<AppState>();
    recording::stop_and_wait(&app).await;
    if let Ok(runner) = st.runner() {
        let jobs = st
            .store
            .call(|s| {
                s.list_jobs(
                    &[
                        minutae_jobs::JobState::Queued,
                        minutae_jobs::JobState::Running,
                    ],
                    10_000,
                )
            })
            .await
            .map_err(err)?;
        for j in jobs {
            let _ = runner.cancel(j.id).await;
        }
    }
    // Stop model downloads and unload whisper before their files go.
    let downloads: Vec<String> = st
        .downloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    for k in downloads {
        models::cancel(&app, &k);
    }
    let cache = st.transcribers.clone();
    let _ = tokio::task::spawn_blocking(move || cache.shutdown(std::time::Duration::from_secs(10)))
        .await;
    // API keys: remove every provider's key from the keychain.
    let secrets = st.secrets.clone();
    let key_errors = tokio::task::spawn_blocking(move || {
        use minutae_llm::SecretStore;
        [
            dto::ApiProvider::Anthropic,
            dto::ApiProvider::Openai,
            dto::ApiProvider::Gemini,
            dto::ApiProvider::Grok,
        ]
        .into_iter()
        .filter_map(|p| secrets.delete(p.secret_account()).err())
        .map(|e| e.detail)
        .collect::<Vec<_>>()
    })
    .await
    .map_err(err)?;
    for e in &key_errors {
        tracing::warn!(error = %e, "removing an API key failed");
    }
    let settings = st.settings();
    let meeting_ids: Vec<Uuid> = st
        .store
        .call(|s| s.list_meetings(1_000_000, 0))
        .await
        .map_err(err)?
        .into_iter()
        .map(|m| m.meeting.id)
        .collect();
    st.store
        .call(|s| {
            s.write(|c| {
                Ok(c.execute_batch(
                    "DELETE FROM jobs; DELETE FROM meetings; DELETE FROM dictations; DELETE FROM api_usage;
                     DELETE FROM speaker_profiles; DELETE FROM settings; DELETE FROM imports;",
                )?)
            })?;
            s.write(|c| Ok(c.execute_batch("VACUUM;")?))
        })
        .await
        .map_err(err)?;
    let (rec_root, sum_root) = (recordings_root(&settings), summaries_root(&settings));
    tokio::task::spawn_blocking(move || {
        // Minutae's own folders go entirely (models too: "erase all" means all).
        for d in [
            paths::recordings_dir(),
            paths::summaries_dir(),
            paths::models_dir(),
            paths::data_dir().join("dictations"),
        ] {
            let _ = std::fs::remove_dir_all(&d);
        }
        // User-chosen folders may hold other files: only remove what Minutae wrote there.
        for id in &meeting_ids {
            if rec_root != paths::recordings_dir() {
                let _ = std::fs::remove_dir_all(rec_root.join(id.to_string()));
            }
            if sum_root != paths::summaries_dir()
                && let Some(md) = find_summary_file(&sum_root, *id)
            {
                let _ = std::fs::remove_file(md);
            }
        }
        let _ = std::fs::remove_file(crate::state::Flags::path());
        let _ = paths::ensure_all();
    })
    .await
    .map_err(err)?;
    *st.flags() = Default::default();
    let defaults = AppSettings::default();
    *st.settings.write().unwrap_or_else(|e| e.into_inner()) = defaults.clone();
    crate::apply_settings(&app, &settings, &defaults);
    // The router caches which API keys exist; rebuild it now that they are gone.
    st.rebuild_router(&defaults);
    let _ = app.emit(ev::SETTINGS_CHANGED, &defaults);
    let _ = app.emit(ev::DICTATIONS_CHANGED, ());
    emit_meetings_changed(&app, None);
    shell::navigate(&app, "/onboarding");
    if !key_errors.is_empty() {
        return Err(
            "Everything else was erased, but some API keys couldn't be removed from the keychain."
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_merges_and_validates() {
        let base = AppSettings::default();
        let s = merge_settings(
            &base,
            serde_json::json!({"whisperModel": "base", "vocabulary": ["Tauri"], "nope": 1}),
        )
        .unwrap();
        assert_eq!(s.whisper_model, WhisperModel::Base);
        assert_eq!(s.vocabulary, vec!["Tauri".to_string()]);
        assert_eq!(s.prompt_delay_seconds, base.prompt_delay_seconds);
        assert!(merge_settings(&base, serde_json::json!({"whisperModel": "huge"})).is_err());
        assert!(merge_settings(&base, serde_json::json!([1])).is_err());
    }

    #[test]
    fn saved_minutes() {
        assert_eq!(minutes_saved(400, 60_000), 9);
        assert_eq!(minutes_saved(10, 600_000), 0);
    }

    #[test]
    fn usage_month_counts_whisper_as_free() {
        let store = minutae_store::Store::open_in_memory().unwrap();
        let now = Utc::now();
        for (provider, cost, purpose) in [
            ("whisper", 0.0, "transcribe"),
            ("codex-cli", 0.0, "summary"),
            ("anthropic", 0.02, "summary"),
        ] {
            store
                .log_usage(&minutae_store::UsageEntry {
                    at: now,
                    provider: provider.into(),
                    transport: None,
                    model: "m".into(),
                    purpose: purpose.into(),
                    meeting_id: None,
                    tokens_in: 10,
                    tokens_out: 5,
                    cost_usd: cost,
                    duration_ms: 1,
                    ok: true,
                })
                .unwrap();
        }
        let local = now.with_timezone(&Local);
        let u = usage_month_from(&store, local.year(), local.month()).unwrap();
        assert_eq!(u.summaries, 2);
        assert_eq!(u.tokens, 45);
        assert!((u.total_usd - 0.02).abs() < 1e-9);
        assert!((u.free_share - 2.0 / 3.0).abs() < 1e-9);
        assert!(
            u.by_provider
                .iter()
                .any(|p| p.provider.starts_with("Whisper"))
        );
        assert!((u.by_day[local.day() as usize - 1].cost_usd - 0.02).abs() < 1e-9);
    }

    #[test]
    fn export_zips_data_dir_without_models() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("summaries")).unwrap();
        std::fs::write(dir.path().join("summaries/a.md"), "# hi").unwrap();
        std::fs::write(dir.path().join("minutae-v4.db"), "db").unwrap();
        let out = tempfile::tempdir().unwrap();
        let dest = out.path().join("x.zip");
        write_export(&dest, dir.path(), &[]).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"summaries/a.md".to_string()), "{names:?}");
        assert!(names.contains(&"minutae-v4.db".to_string()));
    }
}
