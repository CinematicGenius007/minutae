//! Call detection: mic-in-use by another app → (after the prompt delay) the
//! "Start recording?" prompt; the call ending → optional auto-stop after 10 s.

use std::time::Duration;

use minutae_detect::MicEvent;
use tauri::{AppHandle, Emitter, Manager};

use minutae_core::MeetingId;

use crate::dto::{self, RecordingPhase, ev};
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
            // Remember which meeting the timer is for: only that meeting, still recording
            // and started from the call prompt, may be auto-stopped.
            let started_for = st
                .recording
                .lock()
                .await
                .as_ref()
                .filter(|r| r.from_call)
                .map(|r| r.meeting.id);
            if let Some(meeting_id) = started_for
                && settings.stop_when_call_ends
            {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(STOP_AFTER_CALL_ENDS).await;
                    let st = app.state::<AppState>();
                    let still_free = {
                        let c = st.calls.lock().unwrap_or_else(|e| e.into_inner());
                        c.generation == generation && !c.busy
                    };
                    let current = st
                        .recording
                        .lock()
                        .await
                        .as_ref()
                        .map(|r| (r.meeting.id, r.from_call, r.phase));
                    if should_auto_stop(meeting_id, current, still_free) {
                        tracing::info!("call ended 10 s ago; stopping the recording");
                        let _ = crate::recording::stop(&app).await;
                    }
                });
            }
        }
    }
}

/// Auto-stop only the meeting the timer was started for, if it is still recording
/// (and not already stopping), was started from the call prompt, and the call stayed over.
pub fn should_auto_stop(
    started_for: MeetingId,
    current: Option<(MeetingId, bool, RecordingPhase)>,
    still_free: bool,
) -> bool {
    still_free
        && current.is_some_and(|(id, from_call, phase)| {
            id == started_for && from_call && phase != RecordingPhase::Stopping
        })
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
    // The prompt stays up until recording really started; on failure say why.
    match crate::recording::start(app, None, name, true).await {
        Ok(m) => {
            hide_prompt(app);
            Ok(m)
        }
        Err(e) => {
            tracing::warn!("call prompt start: {e}");
            use tauri_plugin_notification::NotificationExt;
            let _ = app
                .notification()
                .builder()
                .title("Minutae")
                .body(&e)
                .show();
            Err(e)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_stop_only_for_the_same_call_meeting() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let rec = RecordingPhase::Recording;
        assert!(should_auto_stop(a, Some((a, true, rec)), true));
        assert!(should_auto_stop(
            a,
            Some((a, true, RecordingPhase::Paused)),
            true
        ));
        // A new call started in the meantime.
        assert!(!should_auto_stop(a, Some((a, true, rec)), false));
        // The user stopped and started another recording.
        assert!(!should_auto_stop(a, Some((b, true, rec)), true));
        assert!(!should_auto_stop(a, Some((b, false, rec)), true));
        assert!(!should_auto_stop(a, None, true));
        assert!(!should_auto_stop(
            a,
            Some((a, true, RecordingPhase::Stopping)),
            true
        ));
    }
}
