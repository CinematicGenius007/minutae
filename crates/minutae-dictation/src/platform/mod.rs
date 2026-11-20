//! Per-OS pieces of insertion and permissions.

use serde::{Deserialize, Serialize};

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::insert::CopyReason;
use crate::insert::Preflight;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// The desktop session the app runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    MacOs,
    Windows,
    X11,
    Wayland,
    Unknown,
}

/// How well dictation can reach other apps in this session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InsertionSupport {
    /// Paste into the focused app.
    Paste,
    /// Copy only; the user pastes (Wayland).
    CopyOnly,
}

/// Classifies a Linux session from `XDG_SESSION_TYPE` and `WAYLAND_DISPLAY`.
pub fn classify_linux_session(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
) -> SessionKind {
    match xdg_session_type.map(str::to_ascii_lowercase).as_deref() {
        Some("wayland") => SessionKind::Wayland,
        Some("x11") => SessionKind::X11,
        _ if wayland_display.is_some_and(|d| !d.is_empty()) => SessionKind::Wayland,
        _ => SessionKind::X11,
    }
}

pub fn session_kind() -> SessionKind {
    if cfg!(target_os = "macos") {
        SessionKind::MacOs
    } else if cfg!(target_os = "windows") {
        SessionKind::Windows
    } else if cfg!(target_os = "linux") {
        let xdg = std::env::var("XDG_SESSION_TYPE").ok();
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        classify_linux_session(xdg.as_deref(), wayland.as_deref())
    } else {
        SessionKind::Unknown
    }
}

pub fn insertion_support() -> InsertionSupport {
    match session_kind() {
        SessionKind::Wayland => InsertionSupport::CopyOnly,
        _ => InsertionSupport::Paste,
    }
}

/// macOS: whether Accessibility is granted (needed to paste into other
/// apps). Always true on other platforms.
pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    return macos::accessibility_trusted();
    #[cfg(not(target_os = "macos"))]
    true
}

/// macOS: opens the system Accessibility prompt when not yet trusted and
/// returns the current state. No-op returning true elsewhere.
pub fn prompt_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    return macos::prompt_accessibility();
    #[cfg(not(target_os = "macos"))]
    true
}

/// Display name of the app with keyboard focus. `None` on Linux for now.
pub fn frontmost_app_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    return macos::frontmost_app_name();
    #[cfg(target_os = "windows")]
    return windows::frontmost_app_name();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    None
}

pub(crate) fn preflight() -> Preflight {
    #[cfg(target_os = "macos")]
    return macos::preflight();
    #[cfg(target_os = "windows")]
    return windows::preflight();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    match session_kind() {
        SessionKind::Wayland => Preflight::CopyOnly(CopyReason::Wayland),
        _ => Preflight::Paste,
    }
}

/// On X11/Wayland the clipboard is served by the owning process, so the
/// `Clipboard` must outlive the paste. Elsewhere dropping it is fine.
pub(crate) fn keep_clipboard_alive(clipboard: arboard::Clipboard) {
    #[cfg(target_os = "linux")]
    {
        static HELD: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);
        if let Ok(mut held) = HELD.lock() {
            *held = Some(clipboard);
        }
    }
    #[cfg(not(target_os = "linux"))]
    drop(clipboard);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_session_detection() {
        let cases = [
            (Some("wayland"), None, SessionKind::Wayland),
            (Some("Wayland"), Some("wayland-0"), SessionKind::Wayland),
            (Some("x11"), Some("wayland-0"), SessionKind::X11),
            (None, Some("wayland-0"), SessionKind::Wayland),
            (None, Some(""), SessionKind::X11),
            (Some("tty"), None, SessionKind::X11),
            (None, None, SessionKind::X11),
        ];
        for (xdg, display, expected) in cases {
            assert_eq!(
                classify_linux_session(xdg, display),
                expected,
                "{xdg:?} {display:?}"
            );
        }
    }
}
