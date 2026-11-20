//! Call detection: mic-in-use by another app → (after the prompt delay) the
//! "Start recording?" prompt; the call ending → optional auto-stop after 10 s.

use std::time::Duration;

use minutae_detect::MicEvent;
use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{self, ev};
use crate::state::AppState;

const STOP_AFTER_CALL_ENDS: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct CallState {
    /// App named by the last `Busy` event (prompt shown or pending).
    pub app: Option<String>,
    pub busy: bool,
    /// Bumped on every mic event; delayed actions check it is unchanged.
    pub generation: u64,
    pub prompt_visible: bool,
}

/// Start or stop the monitor to match `settings.auto_detect_meetings`.
pub fn sync_monitor(app: &AppHandle) {
    let st = app.state::<AppState>();
    let want = st.settings().auto_detect_meetings;
    let mut slot = st.monitor.lock().unwrap_or_else(|e| e.into_inner());
    if !want {
        if let Some(h) = slot.take() {
            h.stop();
        }
        return;
    }
    if slot.is_some() {
        return;
    }
    match minutae_detect::start_monitor(Duration::from_secs(1)) {
        Ok(mut handle) => {
            let (_, dummy) = tokio::sync::mpsc::unbounded_channel();
            let mut rx = std::mem::replace(&mut handle.events, dummy);
            *slot = Some(handle);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    on_event(&app, ev).await;
                }
            });
            tracing::info!("call detection on");
        }
        Err(e) => tracing::warn!(error = %e, "call detection unavailable"),
    }
}

async fn on_event(app: &AppHandle, event: MicEvent) {
    let st = app.state::<AppState>();
    let settings = st.settings();
    match event {
        MicEvent::Busy { app: name } => {
            let generation = {
                let mut c = st.calls.lock().unwrap_or_else(|e| e.into_inner());
                c.busy = true;
                c.app = name.clone();
                c.generation += 1;
                c.generation
            };
            tracing::info!(app = ?name, "call detected");
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(settings.prompt_delay_seconds as u64)).await;
                let st = app.state::<AppState>();
                if st.recording.lock().await.is_some() {
                    return;
                }
                let never = name.as_ref().is_some_and(|n| {
                    st.flags()
                        .never_prompt_apps
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(n))
                });
                {
                    let mut c = st.calls.lock().unwrap_or_else(|e| e.into_inner());
                    if c.generation != generation || !c.busy || never {
                        return;
                    }
                    c.prompt_visible = true;
                }
                let _ = app.emit(ev::CALL_DETECTED, dto::CallDetected { app: name });
                crate::shell::show_aux(&app, crate::shell::PROMPT);
            });
        }
        MicEvent::Free => {
            let generation = {
                let mut c = st.calls.lock().unwrap_or_else(|e| e.into_inner());
                c.busy = false;
                c.generation += 1;
                c.generation
            };
            tracing::info!("call ended");
            let _ = app.emit(ev::CALL_ENDED, ());
            hide_prompt(app);
            let from_call = st
                .recording
                .lock()
                .await
                .as_ref()
                .is_some_and(|r| r.from_call);
            if from_call && settings.stop_when_call_ends {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(STOP_AFTER_CALL_ENDS).await;
                    let st = app.state::<AppState>();
                    let still_free = {
                        let c = st.calls.lock().unwrap_or_else(|e| e.into_inner());
                        c.generation == generation && !c.busy
                    };
                    if still_free {
                        tracing::info!("call ended 10 s ago; stopping the recording");
                        let _ = crate::recording::stop(&app).await;
                    }
                });
            }
        }
    }
}

pub fn hide_prompt(app: &AppHandle) {
    let st = app.state::<AppState>();
    let was = std::mem::replace(
        &mut st
            .calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prompt_visible,
        false,
    );
    if was {
        crate::shell::hide_aux(app, crate::shell::PROMPT);
    }
}

pub async fn accept(app: &AppHandle) -> Result<dto::Meeting, String> {
    let name = app
        .state::<AppState>()
        .calls
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .app
        .clone();
    hide_prompt(app);
    crate::recording::start(app, None, name, true).await
}

pub fn dismiss(app: &AppHandle, always: bool) {
    let st = app.state::<AppState>();
    let name = st
        .calls
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .app
        .clone();
    hide_prompt(app);
    if always && let Some(n) = name {
        let mut flags = st.flags();
        if !flags
            .never_prompt_apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&n))
        {
            flags.never_prompt_apps.push(n);
            flags.save();
        }
    }
}
