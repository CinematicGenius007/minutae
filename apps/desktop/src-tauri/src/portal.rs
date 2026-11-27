//! Global shortcuts on Wayland through the XDG GlobalShortcuts portal, plus the
//! `--toggle-dictation` / `--toggle-recording` command-line entry points that work
//! everywhere (the workaround when no portal exists).
//!
//! The backend is chosen at startup: portal on a Wayland session (or with
//! `MINUTAE_SHORTCUTS=portal`), tauri-plugin-global-shortcut otherwise
//! (`MINUTAE_SHORTCUTS=native` forces it).

use std::sync::Arc;

use minutae_core::AppSettings;
use minutae_dictation::{
    KeySink, PortalBinding, PortalError, ReleaseSupport, SessionKind, ShortcutId, ShortcutPortal,
    ShortcutRequest, run_router, xdg_trigger,
};
use tauri::{AppHandle, Emitter, Manager};

use crate::dictation::DictationKey;
use crate::dto::{ShortcutBackend, ShortcutStatus, ev};
use crate::state::AppState;

/// Whether to use the portal instead of the shortcut plugin.
pub fn wants_portal(session: SessionKind, env_override: Option<&str>) -> bool {
    match env_override.map(str::to_ascii_lowercase).as_deref() {
        Some("portal") => true,
        Some("native") => false,
        _ => session == SessionKind::Wayland,
    }
}

pub fn use_portal() -> bool {
    cfg!(target_os = "linux")
        && wants_portal(
            minutae_dictation::session_kind(),
            std::env::var("MINUTAE_SHORTCUTS").ok().as_deref(),
        )
}

/// The two shortcuts, with the user's accelerators as the portal's preferred triggers.
pub fn requests(settings: &AppSettings) -> Vec<ShortcutRequest> {
    vec![
        ShortcutRequest {
            id: ShortcutId::Dictate,
            description: "Dictate (hold to talk)".into(),
            preferred_trigger: xdg_trigger(&settings.dictation_hotkey),
        },
        ShortcutRequest {
            id: ShortcutId::Record,
            description: "Start or stop recording".into(),
            preferred_trigger: xdg_trigger(&settings.record_shortcut),
        },
    ]
}

/// Status after a bind attempt.
pub fn status_after_bind(result: &Result<PortalBinding, PortalError>) -> ShortcutStatus {
    let trigger = |b: &PortalBinding, id: ShortcutId| {
        b.shortcuts
            .iter()
            .find(|s| s.id == id && !s.trigger.trim().is_empty())
            .map(|s| s.trigger.clone())
    };
    match result {
        Ok(b) => ShortcutStatus {
            backend: ShortcutBackend::Portal,
            dictate_trigger: trigger(b, ShortcutId::Dictate),
            record_trigger: trigger(b, ShortcutId::Record),
            can_configure: b.can_configure,
            hold_supported: None,
            cancel_detail: Some(crate::dto::PORTAL_CANCEL_DETAIL.into()),
            detail: None,
        },
        Err(PortalError::Cancelled) => ShortcutStatus {
            backend: ShortcutBackend::Portal,
            dictate_trigger: None,
            record_trigger: None,
            can_configure: false,
            hold_supported: None,
            cancel_detail: Some(crate::dto::PORTAL_CANCEL_DETAIL.into()),
            detail: Some("The shortcut dialog was closed, so no keys are set. Restart Minutae to choose them.".into()),
        },
        Err(PortalError::Unavailable(e)) | Err(PortalError::Failed(e)) => ShortcutStatus {
            backend: ShortcutBackend::Unavailable,
            dictate_trigger: None,
            record_trigger: None,
            can_configure: false,
            hold_supported: None,
            cancel_detail: Some(crate::dto::PORTAL_CANCEL_DETAIL.into()),
            detail: Some(e.clone()),
        },
    }
}

fn set_status(app: &AppHandle, status: ShortcutStatus) {
    *app.state::<AppState>()
        .shortcut_status
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = status.clone();
    let _ = app.emit(ev::SHORTCUTS_CHANGED, status);
    crate::shell::refresh_tray(app);
}

/// Bind the portal shortcuts once, in the background (the portal may show a dialog).
pub fn start(app: &AppHandle) {
    {
        let st = app.state::<AppState>();
        let mut status = st.shortcut_status.lock().unwrap_or_else(|e| e.into_inner());
        status.backend = ShortcutBackend::Portal;
        status.hold_supported = None;
        status.cancel_detail = Some(crate::dto::PORTAL_CANCEL_DETAIL.into());
    }
    #[cfg(target_os = "linux")]
    let backend: Arc<dyn ShortcutPortal> =
        Arc::new(minutae_dictation::platform::portal::XdgShortcutPortal::default());
    #[cfg(not(target_os = "linux"))]
    let backend: Arc<dyn ShortcutPortal> = {
        // Only reachable with MINUTAE_SHORTCUTS=portal off Linux: no portal here.
        Arc::new(NoPortal)
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move { bind(&app, backend).await });
}

pub async fn bind(app: &AppHandle, backend: Arc<dyn ShortcutPortal>) {
    let st = app.state::<AppState>();
    let result = backend.bind(&requests(&st.settings())).await;
    match &result {
        Ok(b) => tracing::info!(shortcuts = ?b.shortcuts, "global shortcuts portal bound"),
        Err(e) => tracing::warn!(error = %e, "global shortcuts portal"),
    }
    set_status(app, status_after_bind(&result));
    let _ = st.shortcut_portal.set(backend);
    if let Ok(b) = result {
        let sink = Arc::new(AppSink { app: app.clone() });
        tauri::async_runtime::spawn(run_router(b.signals, sink));
    }
}

/// Open the portal's own shortcut settings.
pub async fn configure(app: &AppHandle) -> Result<(), String> {
    let Some(p) = app.state::<AppState>().shortcut_portal.get().cloned() else {
        return Err("Shortcuts are set in your system settings.".into());
    };
    p.configure().await.map_err(|e| match e {
        PortalError::Cancelled => "Closed without changes.".to_string(),
        e => {
            tracing::warn!(error = %e, "configure shortcuts");
            "Your desktop couldn't open its shortcut settings. Change them in system settings."
                .into()
        }
    })
}

struct AppSink {
    app: AppHandle,
}

impl KeySink for AppSink {
    fn release_support(&self, support: ReleaseSupport) {
        let st = self.app.state::<AppState>();
        let status = {
            let mut s = st.shortcut_status.lock().unwrap_or_else(|e| e.into_inner());
            s.hold_supported = match support {
                ReleaseSupport::Unknown => None,
                ReleaseSupport::Reported => Some(true),
                ReleaseSupport::Missing => Some(false),
            };
            s.clone()
        };
        // Switch the controller to toggle before the press that revealed it is handled.
        crate::dictation::apply_settings(&self.app, &st.settings());
        let _ = self.app.emit(ev::SHORTCUTS_CHANGED, status);
        crate::shell::refresh_tray(&self.app);
    }

    fn dictate(&self, down: bool) {
        crate::dictation::send_key(
            &self.app,
            if down {
                DictationKey::Down
            } else {
                DictationKey::Up
            },
        );
    }

    fn toggle_recording(&self) {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move { crate::recording::toggle(&app).await });
    }
}

#[cfg(not(target_os = "linux"))]
struct NoPortal;

#[cfg(not(target_os = "linux"))]
#[async_trait::async_trait]
impl ShortcutPortal for NoPortal {
    async fn bind(&self, _: &[ShortcutRequest]) -> Result<PortalBinding, PortalError> {
        Err(PortalError::Unavailable(
            "The GlobalShortcuts portal only exists on Linux.".into(),
        ))
    }
    async fn configure(&self) -> Result<(), PortalError> {
        Err(PortalError::Unavailable("no portal".into()))
    }
}

// ---- command line ----

/// `minutae --toggle-dictation` / `--toggle-recording`, forwarded to the running app by
/// tauri-plugin-single-instance. Bind them to desktop shortcuts where no portal exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    ToggleDictation,
    ToggleRecording,
}

pub fn cli_action<S: AsRef<str>>(args: &[S]) -> Option<CliAction> {
    args.iter().skip(1).find_map(|a| match a.as_ref() {
        "--toggle-dictation" => Some(CliAction::ToggleDictation),
        "--toggle-recording" => Some(CliAction::ToggleRecording),
        _ => None,
    })
}

/// Runs the action in `args`; `false` when there is none (then the caller shows the window).
pub fn handle_cli(app: &AppHandle, args: &[String]) -> bool {
    match cli_action(args) {
        Some(CliAction::ToggleDictation) => {
            crate::dictation::send_key(app, DictationKey::Toggle);
            true
        }
        Some(CliAction::ToggleRecording) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { crate::recording::toggle(&app).await });
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use minutae_dictation::BoundShortcut;
    use tokio::sync::mpsc;

    use super::*;

    #[test]
    fn portal_only_on_wayland_unless_overridden() {
        assert!(wants_portal(SessionKind::Wayland, None));
        assert!(!wants_portal(SessionKind::X11, None));
        assert!(wants_portal(SessionKind::X11, Some("portal")));
        assert!(!wants_portal(SessionKind::Wayland, Some("Native")));
        assert!(wants_portal(SessionKind::Wayland, Some("other")));
    }

    #[test]
    fn requests_carry_preferred_triggers() {
        let s = AppSettings {
            dictation_hotkey: "Alt+Space".into(),
            record_shortcut: "CommandOrControl+Alt+R".into(),
            ..AppSettings::default()
        };
        let r = requests(&s);
        assert_eq!(r[0].preferred_trigger.as_deref(), Some("ALT+space"));
        assert_eq!(r[1].preferred_trigger.as_deref(), Some("CTRL+ALT+r"));
    }

    #[test]
    fn status_from_bind_results() {
        let ok = Ok(PortalBinding {
            shortcuts: vec![
                BoundShortcut {
                    id: ShortcutId::Dictate,
                    trigger: "Alt+Space".into(),
                },
                BoundShortcut {
                    id: ShortcutId::Record,
                    trigger: " ".into(),
                },
            ],
            can_configure: true,
            signals: mpsc::unbounded_channel().1,
        });
        let s = status_after_bind(&ok);
        assert_eq!(s.backend, ShortcutBackend::Portal);
        assert_eq!(s.dictate_trigger.as_deref(), Some("Alt+Space"));
        assert_eq!(s.record_trigger, None);
        assert!(s.can_configure);
        assert_eq!(s.hold_supported, None);

        let s = status_after_bind(&Err(PortalError::Unavailable("gone".into())));
        assert_eq!(s.backend, ShortcutBackend::Unavailable);
        assert_eq!(s.detail.as_deref(), Some("gone"));

        let s = status_after_bind(&Err(PortalError::Cancelled));
        assert_eq!(s.backend, ShortcutBackend::Portal);
        assert!(s.detail.is_some());
    }

    #[test]
    fn cli_flags() {
        assert_eq!(
            cli_action(&["minutae", "--toggle-dictation"]),
            Some(CliAction::ToggleDictation)
        );
        assert_eq!(
            cli_action(&["minutae", "--x", "--toggle-recording"]),
            Some(CliAction::ToggleRecording)
        );
        // argv[0] is the binary, never an action.
        assert_eq!(cli_action(&["--toggle-dictation"]), None);
        assert_eq!(cli_action::<&str>(&[]), None);
    }
}
