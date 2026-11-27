//! Foreground application metadata. No window titles or contents are read.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub macos_bundle_id: Option<String>,
    /// The executable stem reported by Win32, e.g. "notepad".
    pub windows_executable: Option<String>,
    pub x11_window_class: Option<String>,
    pub display_name: Option<String>,
}

pub trait AppIdentityLookup: Send + Sync {
    fn frontmost_app(&self) -> Option<AppIdentity>;
}

#[derive(Debug, Default)]
pub struct SystemAppIdentityLookup;

impl AppIdentityLookup for SystemAppIdentityLookup {
    fn frontmost_app(&self) -> Option<AppIdentity> {
        frontmost_app_identity()
    }
}

/// A fixed foreground identity for deterministic tests.
#[derive(Debug, Default)]
pub struct FakeAppIdentityLookup(pub Option<AppIdentity>);

impl AppIdentityLookup for FakeAppIdentityLookup {
    fn frontmost_app(&self) -> Option<AppIdentity> {
        self.0.clone()
    }
}

#[cfg(target_os = "macos")]
pub fn frontmost_app_identity() -> Option<AppIdentity> {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let macos_bundle_id = app.bundleIdentifier().map(|id| id.to_string());
    let display_name = app.localizedName().map(|name| name.to_string());
    (macos_bundle_id.is_some() || display_name.is_some()).then_some(AppIdentity {
        macos_bundle_id,
        display_name,
        ..AppIdentity::default()
    })
}

#[cfg(target_os = "windows")]
pub fn frontmost_app_identity() -> Option<AppIdentity> {
    let executable = super::windows::frontmost_app_name()?;
    Some(AppIdentity {
        display_name: Some(executable.clone()),
        windows_executable: Some(executable),
        ..AppIdentity::default()
    })
}

#[cfg(target_os = "linux")]
pub fn frontmost_app_identity() -> Option<AppIdentity> {
    use std::process::Command;

    if super::session_kind() != super::SessionKind::X11 {
        return None;
    }
    std::env::var_os("DISPLAY").filter(|display| !display.is_empty())?;
    // xprop is optional. Only query EWMH/ICCCM metadata and ignore XWayland
    // windows. Validate the window id before passing it back as an argument.
    let active = Command::new("xprop")
        .args(["-root", "-notype", "_NET_ACTIVE_WINDOW"])
        .output()
        .ok()?;
    let id = active_window_id(&active.status.success().then_some(active.stdout)?)?;
    let property = Command::new("xprop")
        .args(["-id", &format!("0x{id:x}"), "-notype", "WM_CLASS"])
        .output()
        .ok()?;
    let class = window_class(&property.status.success().then_some(property.stdout)?)?;
    Some(AppIdentity {
        display_name: Some(class.clone()),
        x11_window_class: Some(class),
        ..AppIdentity::default()
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn frontmost_app_identity() -> Option<AppIdentity> {
    None
}

#[cfg(any(target_os = "linux", test))]
fn active_window_id(output: &[u8]) -> Option<u32> {
    let output = std::str::from_utf8(output).ok()?;
    let id = output
        .trim()
        .strip_prefix("_NET_ACTIVE_WINDOW: window id # 0x")?;
    let id = u32::from_str_radix(id, 16).ok()?;
    (id != 0).then_some(id)
}

#[cfg(any(target_os = "linux", test))]
fn window_class(output: &[u8]) -> Option<String> {
    let output = std::str::from_utf8(output).ok()?;
    // The class is the last quoted field (the first field is the instance).
    let (_, class) = output
        .trim()
        .strip_prefix("WM_CLASS = \"")?
        .rsplit_once("\", \"")?;
    let class = class.strip_suffix('"')?;
    (!class.is_empty() && !class.contains(['"', '\\'])).then(|| class.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x11_active_window_and_class() {
        for (input, expected) in [
            (
                "_NET_ACTIVE_WINDOW: window id # 0x8a00001\n",
                Some(0x8a00001),
            ),
            ("_NET_ACTIVE_WINDOW: window id # 0x0\n", None),
            ("_NET_ACTIVE_WINDOW:  not found.\n", None),
            ("_NET_ACTIVE_WINDOW: window id # 0x42, 0x43\n", None),
            ("_NET_ACTIVE_WINDOW: window id # -1\n", None),
        ] {
            assert_eq!(active_window_id(input.as_bytes()), expected, "{input}");
        }
        for (input, expected) in [
            ("WM_CLASS = \"code\", \"Code\"\n", Some("Code")),
            ("WM_CLASS = \"kitty\", \"kitty\"\n", Some("kitty")),
            (
                "WM_CLASS = \"instance, other\", \"Class, Variant\"\n",
                Some("Class, Variant"),
            ),
            ("WM_CLASS = \"code\", \"\"\n", None),
            ("WM_CLASS = \"just-one-field\"\n", None),
            ("WM_CLASS:  not found.\n", None),
        ] {
            assert_eq!(
                window_class(input.as_bytes()).as_deref(),
                expected,
                "{input}"
            );
        }
    }
}
