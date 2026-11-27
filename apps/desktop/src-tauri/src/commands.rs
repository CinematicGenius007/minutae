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
use crate::meeting_content::{self, LocalMeetingFiles, MeetingFiles};
use crate::state::{AppState, emit_meetings_changed, recordings_root, summaries_root};
use crate::updater::Updates;
use crate::{calls, models, platform, recording, shell, util};

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub(crate) fn uuid(s: &str) -> R<Uuid> {
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
            // A recording or dictation may have started during the download: the new
            // version is already in place, so wait for them to finish before restarting.
            let state = app.state::<AppState>();
            while busy_for_restart(&state).await {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            shell::restart(&app);
        }
    });
    Ok(s)
}

/// A meeting is recording or a dictation is in progress (restarting would cut it off).
async fn busy_for_restart(state: &AppState) -> bool {
    if state.recording.lock().await.is_some() {
        return true;
    }
    state.dictation.get().is_some_and(|d| {
        !matches!(
            d.state(),
            minutae_dictation::DictationState::Idle | minutae_dictation::DictationState::Error
        )
    })
}

#[tauri::command]
pub fn shortcut_status(state: State<'_, AppState>) -> dto::ShortcutStatus {
    state.shortcut_status()
}

#[tauri::command]
pub async fn configure_shortcuts(app: AppHandle) -> R<()> {
    crate::portal::configure(&app).await
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
    if p.contains_key("tidyDictation") && !p.contains_key("dictationDefaultShape") {
        obj.insert("dictationDefaultShape".into(), serde_json::Value::Null);
    }
    for (k, v) in p {
        if obj.contains_key(&k) {
            obj.insert(k, v);
        }
    }
    let mut merged: AppSettings =
        serde_json::from_value(base).map_err(|e| format!("Invalid setting: {e}"))?;
    merged
        .dictation_shape_edits
        .retain(|preset, _| !preset.preserves_identifiers());
    Ok(merged)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: serde_json::Value) -> R<AppSettings> {
    change_settings(&app, |current| merge_settings(current, patch)).await
}

async fn change_settings(
    app: &AppHandle,
    change: impl FnOnce(&AppSettings) -> R<AppSettings>,
) -> R<AppSettings> {
    // Serialise read-merge-write so two quick toggles can't overwrite each other.
    let st = app.state::<AppState>();
    let _guard = st.storage_ops.lock().await;
    let old = st.settings();
    let new = change(&old)?;
    if old.dictation_retention_days != new.dictation_retention_days
        && !matches!(new.dictation_retention_days, None | Some(7 | 30 | 90))
    {
        return Err("Choose Forever, 7, 30, or 90 days for dictation history.".into());
    }
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
    if old.summaries_path != new.summaries_path || old.recordings_path != new.recordings_path {
        let folders = meeting_content::ContentFolders::current(&old);
        st.store
            .call(move |store| meeting_content::remember_storage(store, &folders))
            .await
            .map_err(err)?;
    }
    let shortcuts_changed =
        old.record_shortcut != new.record_shortcut || old.dictation_hotkey != new.dictation_hotkey;
    if shortcuts_changed {
        check_shortcuts(&old, &new)?;
        // Register the new pair now; if the changed one is refused, the old pair stays.
        let failed = shell::register_shortcuts(app, &new);
        let changed_failed = failed.iter().any(|f| {
            (f == &new.record_shortcut && new.record_shortcut != old.record_shortcut)
                || (f == &new.dictation_hotkey && new.dictation_hotkey != old.dictation_hotkey)
        });
        if changed_failed {
            shell::register_shortcuts(app, &old);
            return Err("That shortcut is already used by another app".into());
        }
    }
    let s = new.clone();
    if let Err(e) = st.store.call(move |store| store.set_settings(&s)).await {
        if shortcuts_changed {
            shell::register_shortcuts(app, &old);
        }
        return Err(err(e));
    }
    *st.settings.write().unwrap_or_else(|e| e.into_inner()) = new.clone();
    crate::apply_settings(app, &old, &new);
    let _ = app.emit(ev::SETTINGS_CHANGED, &new);
    if old.summaries_path != new.summaries_path || old.recordings_path != new.recordings_path {
        crate::spawn_export_backfill(&app);
    }
    if old.dictation_retention_days != new.dictation_retention_days {
        crate::dictation::prune(app).await;
    }
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
    let operation = state.meeting_ops.try_lock(id);
    let markdown_path = state
        .store
        .call(move |store| {
            // Progress refreshes must remain responsive while a processing job
            // holds this lock. Discovery can wait until the next idle refresh.
            let path = if let Some(_operation) = operation {
                meeting_content::markdown_path(store, &summaries_dir, id)?
            } else {
                store.meeting_markdown_file(id)?.map(PathBuf::from)
            };
            Ok(path.filter(|p| meeting_content::summary_belongs_to(p, id).unwrap_or(false)))
        })
        .await
        .map_err(err)?;
    let duration_ms = d
        .tracks
        .iter()
        .filter_map(|t| t.duration_ms)
        .max()
        .or_else(|| {
            d.meeting
                .ended_at
                .map(|e| (e - d.meeting.started_at).num_milliseconds().max(0) as u64)
        });
    let existing = |t: minutae_core::Track| {
        d.tracks
            .iter()
            .find(|x| x.track == t && !x.deleted)
            .filter(|x| LocalMeetingFiles.track_exists(Path::new(&x.path), id))
            .map(|x| x.path.clone())
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
        echo_cancel: d.echo_cancel,
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
    let _operation = st.meeting_ops.lock(id).await;
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
    crate::meeting_content::delete_entire_meeting(app, id).await
}

#[tauri::command]
pub async fn retry_meeting(app: AppHandle, id: String) -> R<()> {
    let id = uuid(&id)?;
    let st = app.state::<AppState>();
    let _operation = st.meeting_ops.lock(id).await;
    if !st
        .store
        .call(move |s| s.get_meeting(id))
        .await
        .map_err(err)?
        .is_some_and(|m| m.status == minutae_core::MeetingStatus::Failed)
    {
        return Err("Only failed meetings can be retried.".into());
    }
    crate::pipeline::retry_meeting(st.runner()?, id).await?;
    emit_meetings_changed(&app, Some(id));
    Ok(())
}

#[tauri::command]
pub async fn set_action_done(app: AppHandle, id: String, done: bool) -> R<dto::ActionChangeResult> {
    crate::meeting_content::set_action_done(&app, &id, done).await
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
    let operation = state.meeting_ops.lock(id).await;
    let dir = summaries_root(&state.settings());
    state
        .store
        .call(move |s| {
            let _operation = operation;
            if let Some(path) = crate::meeting_content::markdown_path(s, &dir, id)?
                && let Ok(Some(bytes)) = meeting_content::read_export(&path, id)
                && let Ok(markdown) = String::from_utf8(bytes)
            {
                return Ok(markdown);
            }
            Ok(s.get_summary(id)?.map(|s| s.markdown).unwrap_or_default())
        })
        .await
        .map_err(err)
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

/// Menu-bar panel: show the main window (optionally at a route) and close the panel.
#[tauri::command]
pub fn open_main(app: AppHandle, route: Option<String>) {
    shell::hide_tray_panel(&app);
    match route {
        Some(r) => shell::navigate(&app, &r),
        None => shell::show_main(&app),
    }
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    shell::hide_tray_panel(&app);
}

/// The panel page reports its content height (logical px).
#[tauri::command]
pub fn tray_panel_resize(app: AppHandle, height: f64) {
    shell::resize_tray_panel(&app, height);
}

/// Quit for real (stops a recording first, same as the tray menu's Quit).
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    shell::quit(&app);
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

use minutae_core::dictation::{AppMatcher, DictationAppRule, ShapeEdits, ShapePreset};

#[tauri::command]
pub fn dictation_shape_options(app: AppHandle) -> dto::DictationShapeOptions {
    let settings = app.state::<AppState>().settings();
    dto::DictationShapeOptions {
        presets: [
            ShapePreset::Tidy,
            ShapePreset::Verbatim,
            ShapePreset::ChatMessage,
            ShapePreset::Email,
            ShapePreset::NotesAndLists,
            ShapePreset::CodeAndCommit,
        ]
        .map(ShapePreset::shape),
        quick_presets: minutae_dictation::QUICK_PRESETS,
        default_shape: settings.default_dictation_shape(),
        capabilities: dto::DictationCapabilities {
            hud_visible: true,
            hud_interactive: shell::hud_interactive(&app),
            local_rewrite_available: false,
            cloud_rewrite: dto::DictationCloudRewrite::Planned,
        },
    }
}

#[tauri::command]
pub fn list_dictation_app_rules(state: State<'_, AppState>) -> Vec<DictationAppRule> {
    state.settings().dictation_app_rules
}

#[tauri::command]
pub async fn save_dictation_app_rule(app: AppHandle, mut rule: DictationAppRule) -> R<AppSettings> {
    if rule.app.value().trim().is_empty() {
        return Err("Choose an app for the dictation rule.".into());
    }
    if rule.display_name.trim().is_empty() {
        rule.display_name = rule.app.value().into();
    }
    change_settings(&app, |current| {
        let mut next = current.clone();
        if let Some(existing) = next
            .dictation_app_rules
            .iter_mut()
            .find(|r| r.app.same_app(&rule.app))
        {
            *existing = rule;
        } else {
            next.dictation_app_rules.push(rule);
        }
        Ok(next)
    })
    .await
}

#[tauri::command]
pub async fn remove_dictation_app_rule(app: AppHandle, matcher: AppMatcher) -> R<AppSettings> {
    change_settings(&app, |current| {
        let mut next = current.clone();
        next.dictation_app_rules
            .retain(|rule| !rule.app.same_app(&matcher));
        Ok(next)
    })
    .await
}

#[tauri::command]
pub async fn set_dictation_shape_edits(
    app: AppHandle,
    preset: ShapePreset,
    edits: Option<ShapeEdits>,
) -> R<AppSettings> {
    change_settings(&app, |current| {
        let mut next = current.clone();
        match edits.filter(|_| !preset.preserves_identifiers()) {
            Some(edits) => {
                next.dictation_shape_edits.insert(preset, edits);
            }
            None => {
                next.dictation_shape_edits.remove(&preset);
            }
        }
        Ok(next)
    })
    .await
}

#[tauri::command]
pub async fn list_dictation_apps(state: State<'_, AppState>) -> R<Vec<dto::DictationApp>> {
    use minutae_dictation::platform::identity::{AppIdentityLookup, SystemAppIdentityLookup};
    let live = tokio::task::spawn_blocking(|| SystemAppIdentityLookup.available_apps());
    let mut identities = state
        .store
        .call(|s| s.dictation_apps())
        .await
        .map_err(err)?;
    identities.extend(live.await.map_err(err)?);
    // Prefer a platform ID where an old history row only knows the name.
    identities.sort_by_key(|app| matches!(app.matcher(), Some(AppMatcher::DisplayName(_))));
    let rules = state.settings().dictation_app_rules;
    let mut apps: Vec<dto::DictationApp> = Vec::new();
    for identity in identities {
        let Some(app) = identity.matcher() else {
            continue;
        };
        let display_name = identity.display_name.as_deref().unwrap_or(app.value());
        let name_only = matches!(app, AppMatcher::DisplayName(_));
        if rules.iter().any(|r| {
            r.app.matches(&identity)
                || (name_only && r.display_name.eq_ignore_ascii_case(display_name))
        }) || apps.iter().any(|a| {
            a.app.same_app(&app) || (name_only && a.display_name.eq_ignore_ascii_case(display_name))
        }) {
            continue;
        }
        apps.push(dto::DictationApp {
            suggested_preset: minutae_dictation::shapes::suggested_preset(&identity),
            display_name: display_name.to_owned(),
            app,
        });
    }
    apps.sort_by_key(|app| app.display_name.to_lowercase());
    Ok(apps)
}

#[tauri::command]
pub fn preview_dictation_shape(
    state: State<'_, AppState>,
    sample: String,
    preset: ShapePreset,
    edits: Option<ShapeEdits>,
) -> R<String> {
    if sample.len() > 16_384 {
        return Err("Use a shorter dictation sample.".into());
    }
    let settings = state.settings();
    let shape = edits.map_or_else(
        || settings.dictation_shape(preset),
        |edits| edits.apply(preset.shape()),
    );
    Ok(minutae_dictation::postprocess::process_with_shape(
        &sample,
        &shape,
        &settings.vocabulary,
    ))
}

#[tauri::command]
pub async fn get_dictation_hud(app: AppHandle) -> R<dto::HudState> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::Get).await
}

#[tauri::command]
pub async fn switch_dictation_shape(app: AppHandle, preset: ShapePreset) -> R<()> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::Switch(preset))
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn set_dictation_hud_interaction(app: AppHandle, hovered: bool, focused: bool) -> R<()> {
    crate::dictation::hud_request(
        &app,
        crate::dictation::HudCommand::Interaction { hovered, focused },
    )
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn dismiss_dictation_hud(app: AppHandle) -> R<()> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::Dismiss)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn dismiss_dictation_error(app: AppHandle) -> R<()> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::DismissError)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn undo_dictation_insert(app: AppHandle, result_id: uuid::Uuid) -> R<()> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::Undo(result_id))
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn fix_dictation_error(app: AppHandle) -> R<()> {
    crate::dictation::hud_request(&app, crate::dictation::HudCommand::Fix)
        .await
        .map(|_| ())
}

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
    crate::dictation::delete(&app, crate::dictation::DeleteDictations::One(uuid(&id)?)).await
}

#[tauri::command]
pub async fn delete_all_dictations(app: AppHandle) -> R<()> {
    crate::dictation::delete(&app, crate::dictation::DeleteDictations::All).await
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
    let _storage = st.storage_ops.lock().await;
    if st.importing.load(std::sync::atomic::Ordering::SeqCst) {
        return Err("An import is running. Wait for it to finish, then erase.".into());
    }
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
    let mut cleanup_errors = meeting_content::erase_meetings(&st, &settings).await?;
    let _dictations = st.dictation_storage.lock().await;
    let mut other_errors = st
        .store
        .call(|s| {
            let mut errors = erase_local_files(&paths::data_dir(), &s.dictation_ids_before(None)?);
            if let Err(error) = paths::ensure_all() {
                errors.push(error.to_string());
            }
            s.write(|c| {
                Ok(c.execute_batch(
                    "DELETE FROM jobs; DELETE FROM dictations; DELETE FROM api_usage;
                 DELETE FROM speaker_profiles; DELETE FROM settings; DELETE FROM imports;
                 DELETE FROM legacy_imports;",
                )?)
            })?;
            s.write(|c| Ok(c.execute_batch("VACUUM;")?))?;
            Ok(errors)
        })
        .await
        .map_err(err)?;
    cleanup_errors.append(&mut other_errors);
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
        cleanup_errors.push("Some API keys couldn't be removed from the keychain.".into());
    }
    if cleanup_errors.is_empty() {
        Ok(())
    } else {
        Err(cleanup_errors.join("\n"))
    }
}

fn erase_local_files(data: &Path, dictations: &[Uuid]) -> Vec<String> {
    use cap_fs_ext::DirExt;
    use cap_std::fs::Dir;
    use std::io::ErrorKind::NotFound;

    let mut errors = Vec::new();
    let root = match Dir::open_ambient_dir(data, cap_std::ambient_authority()) {
        Ok(root) => root,
        Err(error) => return vec![format!("{}: {error}", data.display())],
    };
    let mut remove = |dir: &Dir, folder: &str, names: Vec<String>| {
        for name in names {
            let result = dir.symlink_metadata(&name).and_then(|meta| {
                if meta.is_file() {
                    dir.remove_file(&name)
                } else {
                    Err(std::io::Error::other(
                        "The path is not a regular Minutae file.",
                    ))
                }
            });
            if let Err(error) = result
                && error.kind() != NotFound
            {
                errors.push(format!("{folder}/{name}: {error}"));
            }
        }
    };
    remove(&root, "", vec!["app-state.json".into()]);
    let whisper = [
        WhisperModel::Tiny,
        WhisperModel::Base,
        WhisperModel::Small,
        WhisperModel::Medium,
    ]
    .into_iter()
    .flat_map(|model| {
        let name = minutae_asr::model_info(model).file_name;
        [format!("{name}.part"), format!("{name}.part.etag"), name]
    })
    .collect();
    // Only standard download and dictation filenames are eligible. Do not
    // traverse links below the data root or recursively remove user files.
    let mut rejected = Vec::new();
    for (folder, names) in [
        (
            "dictations",
            dictations.iter().map(|id| format!("{id}.wav")).collect(),
        ),
        ("models", whisper),
    ] {
        match root.open_dir_nofollow(folder) {
            Ok(dir) => {
                remove(&dir, folder, names);
                if folder == "models" {
                    match dir.open_dir_nofollow("diarize") {
                        Ok(diarize) => remove(
                            &diarize,
                            "models/diarize",
                            minutae_diarize::download_files(),
                        ),
                        Err(error) if error.kind() == NotFound => {}
                        Err(error) => rejected.push(format!("models/diarize: {error}")),
                    }
                }
            }
            Err(error) if error.kind() == NotFound => {}
            Err(error) => rejected.push(format!("{folder}: {error}")),
        }
    }
    errors.extend(rejected);
    errors
}

#[cfg(test)]
#[test]
fn erase_keeps_user_notes_in_meeting_directory() {
    let temp = tempfile::tempdir().unwrap();
    let recordings = temp.path().join("recordings");
    let settings = AppSettings {
        recordings_path: Some(recordings.to_string_lossy().into_owned()),
        summaries_path: Some(temp.path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    let store = minutae_store::Store::open_in_memory().unwrap();
    let id = Uuid::new_v4();
    let now = Utc::now();
    store
        .create_meeting(&minutae_core::Meeting {
            id,
            title: "Test".into(),
            started_at: now,
            ended_at: Some(now),
            status: MeetingStatus::Done,
            source_app: None,
            failure: None,
        })
        .unwrap();
    let directory = recordings.join(id.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("mic.wav"), b"recording").unwrap();
    std::fs::write(directory.join("notes.txt"), b"personal notes").unwrap();
    let errors = meeting_content::erase_meeting_files(
        &store,
        &meeting_content::ContentFolders::current(&settings),
        id,
    )
    .unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(store.get_meeting(id).unwrap().is_none());
    assert!(!directory.join("mic.wav").exists());
    assert_eq!(
        std::fs::read(directory.join("notes.txt")).unwrap(),
        b"personal notes"
    );
}

// ---------------------------------------------------------------- import (MIG-10)

/// Older Minutae databases found at their default paths, with counts. Reads only.
#[tauri::command]
pub async fn import_legacy_scan(state: State<'_, AppState>) -> R<dto::LegacyScan> {
    let store = state.store.clone();
    let sources = tokio::task::spawn_blocking(move || {
        minutae_import::scan(&store, &minutae_import::detect())
    })
    .await
    .map_err(err)?;
    Ok(dto::LegacyScan {
        sources,
        running: state.importing.load(std::sync::atomic::Ordering::SeqCst),
    })
}

/// Start importing every found source. Returns at once; the work runs on the blocking pool
/// and reports `import://progress` (throttled) and one `import://finished`.
///
/// Not a `minutae-jobs` job: those are per-meeting pipeline steps reported as `job://progress`
/// stages. The import is idempotent (`legacy_imports`), so after a crash the user re-runs it.
#[tauri::command]
pub fn import_legacy_run(app: AppHandle) -> R<()> {
    use std::sync::atomic::Ordering;
    let st = app.state::<AppState>();
    let _storage = st
        .storage_ops
        .try_lock()
        .map_err(|_| "Storage settings are being updated. Try again in a moment.")?;
    if st.importing.swap(true, Ordering::SeqCst) {
        return Err("An import is already running.".into());
    }
    let store = st.store.clone();
    let root = recordings_root(&st.settings());
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let emitter = a.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let sources = minutae_import::detect();
            let mut last = std::time::Instant::now();
            let mut progress = |p: &dto::ImportProgress| {
                if p.done == p.total || last.elapsed() >= std::time::Duration::from_millis(100) {
                    last = std::time::Instant::now();
                    let _ = emitter.emit(ev::IMPORT_PROGRESS, p);
                }
            };
            minutae_import::run(
                &store,
                &sources,
                minutae_import::RunOptions {
                    recordings_root: root,
                    progress: &mut progress,
                },
            )
        })
        .await;
        a.state::<AppState>()
            .importing
            .store(false, Ordering::SeqCst);
        let finished = match outcome {
            Ok(result) => {
                tracing::info!(
                    imported = result.imported,
                    dictations = result.dictations,
                    skipped = result.skipped,
                    failed = result.failed,
                    "legacy import finished"
                );
                dto::ImportFinished {
                    result: Some(result),
                    error: None,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "legacy import stopped");
                dto::ImportFinished {
                    result: None,
                    error: Some("The import stopped unexpectedly. Try again.".into()),
                }
            }
        };
        let _ = a.emit(ev::IMPORT_FINISHED, &finished);
        let _ = a.emit(ev::DICTATIONS_CHANGED, ());
        emit_meetings_changed(&a, None);
    });
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
    fn settings_patch_drops_edits_for_identifier_preserving_shapes() {
        use minutae_core::dictation::ShapePreset;

        let edits = serde_json::json!({ "capitals": "sentence", "dropFinalPeriod": true });
        let patched = merge_settings(
            &AppSettings::default(),
            serde_json::json!({ "dictationShapeEdits": {
                "verbatim": edits, "codeAndCommit": edits, "email": edits
            } }),
        )
        .unwrap();
        assert_eq!(
            patched.dictation_shape_edits.keys().collect::<Vec<_>>(),
            [&ShapePreset::Email]
        );

        // Keys stored by an earlier build go away with the next settings change.
        let mut stored = AppSettings::default();
        stored
            .dictation_shape_edits
            .insert(ShapePreset::Verbatim, Default::default());
        let patched = merge_settings(&stored, serde_json::json!({ "appearance": "dark" })).unwrap();
        assert!(patched.dictation_shape_edits.is_empty());
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
