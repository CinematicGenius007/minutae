//! Minutae desktop backend: wires the `minutae-*` crates into Tauri commands,
//! events, windows, tray and global shortcuts. Contract: docs/IPC.md.

pub mod calls;
pub mod commands;
pub mod dictation;
pub mod dto;
pub mod models;
pub mod pipeline;
pub mod platform;
pub mod portal;
pub mod recording;
pub mod shell;
pub mod state;
pub mod updater;
pub mod util;

use std::sync::Arc;
use std::time::Duration;

use minutae_core::{AppSettings, paths};
use minutae_jobs::{JobKind, JobRunner};
use minutae_store::{Store, UsageEntry};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

use crate::dto::ev;
use crate::state::{AppHost, AppState};

/// React to a settings change (router, shortcuts, call detection, autostart, dictation).
pub fn apply_settings(app: &AppHandle, old: &AppSettings, new: &AppSettings) {
    let st = app.state::<AppState>();
    if old.transport_order != new.transport_order
        || old.api_fallback_enabled != new.api_fallback_enabled
        || old.codex_model != new.codex_model
        || old.ollama_model != new.ollama_model
    {
        st.rebuild_router(new);
    }
    if old.record_shortcut != new.record_shortcut || old.dictation_hotkey != new.dictation_hotkey {
        shell::apply_shortcuts(app);
        shell::refresh_tray(app);
    } else if old.dictation_mode != new.dictation_mode {
        shell::refresh_tray(app); // "Hold …" / "Press …" hint
    }
    if old.auto_detect_meetings != new.auto_detect_meetings {
        calls::sync_monitor(app);
    }
    if old.launch_at_login != new.launch_at_login {
        sync_autostart(app, new.launch_at_login);
    }
    if old.whisper_model != new.whisper_model {
        st.transcribers.clear();
    }
    if old.live_captions != new.live_captions {
        let app = app.clone();
        let enabled = new.live_captions;
        tauri::async_runtime::spawn(async move {
            recording::apply_caption_setting(&app, enabled).await;
        });
    }
    if let Some(dir) = new
        .recordings_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        recording::allow_asset_dir(app, std::path::Path::new(dir));
    }
    dictation::apply_settings(app, new);
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    // Debug builds would register the dev binary as a login item.
    if cfg!(debug_assertions) {
        tracing::debug!(enabled, "autostart not changed in debug builds");
        return;
    }
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    if let Err(e) = r {
        tracing::warn!(error = %e, "autostart");
    }
}

/// Audio may only be deleted once the meeting is fully processed (`Done`, so the
/// transcript is stored) and it started before `cutoff`. Never for recording,
/// processing or failed meetings: their audio is still needed to (re)transcribe.
pub fn retention_eligible(
    meeting: &minutae_core::Meeting,
    cutoff: chrono::DateTime<chrono::Utc>,
) -> bool {
    meeting.status == minutae_core::MeetingStatus::Done && meeting.started_at < cutoff
}

/// Prune expired dictations and meeting audio.
pub async fn run_retention(app: &AppHandle) {
    dictation::prune(app).await;
    let st = app.state::<AppState>();
    let Some(days) = st.settings().delete_audio_after_days else {
        return;
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let store = st.store.clone();
    let Ok(meetings) = store.call(|s| s.list_meetings(100_000, 0)).await else {
        return;
    };
    let mut removed = 0;
    for item in meetings
        .into_iter()
        .filter(|m| retention_eligible(&m.meeting, cutoff))
    {
        let id = item.meeting.id;
        let Ok(tracks) = store.call(move |s| s.list_tracks(id)).await else {
            continue;
        };
        for t in tracks {
            if std::fs::remove_file(&t.path).is_ok() {
                removed += 1;
            }
            if t.track == minutae_core::Track::Mic {
                let clean =
                    std::path::Path::new(&t.path).with_file_name(minutae_aec::CLEAN_MIC_FILE);
                let _ = std::fs::remove_file(clean);
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, days, "deleted old audio");
    }
}

fn spawn_background(
    app: &AppHandle,
    mut usage_rx: tokio::sync::mpsc::UnboundedReceiver<minutae_llm::UsageRecord>,
) {
    // LLM usage → store.
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(r) = usage_rx.recv().await {
            let entry = UsageEntry {
                at: r.at,
                provider: r.provider,
                transport: Some(minutae_llm::types::transport_id(r.transport).to_string()),
                model: r.model.unwrap_or_default(),
                purpose: serde_json::to_value(r.purpose)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default(),
                meeting_id: None,
                tokens_in: r.tokens_in.unwrap_or(0) as u64,
                tokens_out: r.tokens_out.unwrap_or(0) as u64,
                cost_usd: r.cost_usd.unwrap_or(0.0),
                duration_ms: r.duration_ms,
                ok: r.error_class.is_none(),
            };
            let st = a.state::<AppState>();
            if let Err(e) = st.store.call(move |s| s.log_usage(&entry)).await {
                tracing::warn!(error = %e, "usage log");
            }
        }
    });

    // Job runner + events, then crash recovery, then the daily retention pass.
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = a.state::<AppState>();
        let host = Arc::new(AppHost { app: a.clone() });
        let runner = JobRunner::new(st.store.clone(), pipeline::handlers(host));
        let mut rx = runner.subscribe();
        let _ = st.runner.set(runner.clone());
        let a2 = a.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(u) => {
                        if let Some(p) = &u.progress {
                            let _ = a2.emit(ev::PROGRESS, p);
                        }
                        let store = a2.state::<AppState>().store.clone();
                        let u2 = u.clone();
                        if let Ok(Some(id)) = tokio::task::spawn_blocking(move || {
                            pipeline::apply_job_update(&store, &u2)
                        })
                        .await
                        {
                            state::emit_meetings_changed(&a2, Some(id));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
        match runner.start().await {
            Ok(n) if n > 0 => tracing::info!(n, "resumed interrupted jobs"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "job runner failed to start"),
        }
        recording::recover(&a).await;
        let _ = runner; // keep alive in state
        loop {
            run_retention(&a).await;
            let _ = a
                .state::<AppState>()
                .store
                .call(|s| s.prune_jobs(chrono::Utc::now() - chrono::Duration::days(30)))
                .await;
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    });
    let _ = JobKind::Retention; // retention runs on a timer, not as a queued job
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    paths::ensure_all()?;
    let db = paths::database_file();
    let store = Store::open(&db)?;
    tracing::info!(db = %db.display(), "database open");
    let (sink, usage_rx) = minutae_llm::usage_channel();
    app.manage(AppState::new(store, sink));
    let handle = app.handle().clone();
    app.manage(updater::init(&handle));

    shell::create_aux_windows(&handle)?;
    shell::create_tray(&handle)?;
    shell::refresh_tray(&handle);
    shell::init_shortcuts(&handle);

    let rt = tauri::async_runtime::block_on(async { tokio::runtime::Handle::current() });
    dictation::init(&handle, rt);
    // `minutae --toggle-recording` when the app was not running yet.
    let args: Vec<String> = std::env::args().collect();
    portal::handle_cli(&handle, &args);
    calls::sync_monitor(&handle);
    let settings = handle.state::<AppState>().settings();
    sync_autostart(&handle, settings.launch_at_login);
    if let Some(dir) = settings
        .recordings_path
        .as_deref()
        .filter(|p| !p.trim().is_empty())
    {
        recording::allow_asset_dir(&handle, std::path::Path::new(dir));
    }
    spawn_background(&handle, usage_rx);
    spawn_signal_handler(&handle);
    Ok(())
}

/// SIGTERM / SIGINT (Ctrl+C on Windows) → graceful quit, so recordings are saved and
/// whisper models are freed instead of the process dying mid-write.
fn spawn_signal_handler(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut int)) = (
                signal(SignalKind::terminate()),
                signal(SignalKind::interrupt()),
            ) else {
                tracing::warn!("signal handlers unavailable");
                return;
            };
            tokio::select! {
                _ = term.recv() => tracing::info!("SIGTERM"),
                _ = int.recv() => tracing::info!("SIGINT"),
            }
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            tracing::info!("Ctrl+C");
        }
        shell::quit(&app);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,whisper_rs=warn,minutae_asr=info".into()),
        )
        .try_init();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A second launch with --toggle-dictation / --toggle-recording acts on the
            // running app (desktop-shortcut workaround); a plain launch shows the window.
            if !portal::handle_cli(app, &args) {
                shell::show_main(app)
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // Always registered (its config must exist); `updater::init` turns the feature off
        // at runtime when no public key is configured.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    shell::on_shortcut(app, shortcut, event.state())
                })
                .build(),
        );
    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }
    builder = builder
        .setup(setup)
        .on_window_event(|window, event| {
            // Closing the main window hides it; Minutae keeps running in the tray.
            if window.label() == shell::MAIN
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::platform,
            commands::shortcut_status,
            commands::configure_shortcuts,
            commands::open_main,
            commands::tray_panel_hide,
            commands::tray_panel_resize,
            commands::quit_app,
            commands::get_settings,
            commands::update_settings,
            commands::permissions,
            commands::request_permission,
            commands::open_system_settings,
            commands::reveal_path,
            commands::open_path,
            commands::copy_text,
            commands::onboarding_done,
            commands::finish_onboarding,
            commands::list_meetings,
            commands::week_stats,
            commands::get_meeting,
            commands::rename_meeting,
            commands::delete_meeting,
            commands::retry_meeting,
            commands::set_action_done,
            commands::rename_speaker,
            commands::summary_markdown,
            commands::recording_state,
            commands::start_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::mark_moment,
            commands::accept_call_prompt,
            commands::dismiss_call_prompt,
            commands::list_input_devices,
            commands::list_dictations,
            commands::dictation_stats,
            commands::delete_dictation,
            commands::delete_all_dictations,
            commands::search,
            commands::ask,
            commands::list_models,
            commands::download_model,
            commands::cancel_model_download,
            commands::delete_model,
            commands::diarize_model,
            commands::download_diarize_model,
            commands::provider_statuses,
            commands::set_api_key,
            commands::clear_api_key,
            commands::api_key_hint,
            commands::usage_month,
            commands::storage_usage,
            commands::pick_folder,
            commands::export_all,
            commands::erase_all,
            commands::import_legacy_scan,
            commands::import_legacy_run,
            commands::update_status,
            commands::check_for_updates,
            commands::install_update,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building Minutae");
    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => shell::show_main(app),
        RunEvent::ExitRequested { api, code, .. } => {
            // `app.exit(code)` (end of `shell::quit`) goes through. Anything else (Cmd+Q
            // from the app menu, last window gone, OS logout) runs the graceful quit first.
            if code.is_none() {
                api.prevent_exit();
                shell::quit(app);
            }
        }
        RunEvent::Exit => {
            // Last chance on every exit path: no whisper (Metal) context may outlive main.
            if let Some(st) = app.try_state::<AppState>() {
                st.transcribers.shutdown(Duration::from_secs(5));
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use minutae_core::{Meeting, MeetingStatus};

    #[test]
    fn retention_only_touches_old_done_meetings() {
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::days(30);
        let meeting = |status, age_days| Meeting {
            id: uuid::Uuid::new_v4(),
            title: "t".into(),
            started_at: now - chrono::Duration::days(age_days),
            ended_at: None,
            status,
            source_app: None,
            failure: None,
        };
        assert!(retention_eligible(
            &meeting(MeetingStatus::Done, 40),
            cutoff
        ));
        assert!(!retention_eligible(
            &meeting(MeetingStatus::Done, 10),
            cutoff
        ));
        for s in [
            MeetingStatus::Failed,
            MeetingStatus::Processing,
            MeetingStatus::Recording,
        ] {
            assert!(!retention_eligible(&meeting(s, 40), cutoff), "{s:?}");
        }
    }
}
