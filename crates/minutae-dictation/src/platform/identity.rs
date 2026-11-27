//! Foreground application metadata. No window titles or contents are read.

pub use minutae_core::dictation::AppIdentity;

pub trait AppIdentityLookup: Send + Sync {
    fn frontmost_app(&self) -> Option<AppIdentity>;
    fn foreground_target(&self) -> (Option<AppIdentity>, Option<u32>) {
        (self.frontmost_app(), None)
    }
    fn available_apps(&self) -> Vec<AppIdentity> {
        self.frontmost_app().into_iter().collect()
    }
}

#[derive(Debug, Default)]
pub struct SystemAppIdentityLookup;

impl AppIdentityLookup for SystemAppIdentityLookup {
    fn frontmost_app(&self) -> Option<AppIdentity> {
        frontmost_app_identity()
    }

    #[cfg(target_os = "macos")]
    fn foreground_target(&self) -> (Option<AppIdentity>, Option<u32>) {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else {
            return (None, None);
        };
        (
            mac_identity(&app),
            u32::try_from(app.processIdentifier()).ok(),
        )
    }

    #[cfg(target_os = "macos")]
    fn available_apps(&self) -> Vec<AppIdentity> {
        use objc2_app_kit::NSApplicationActivationPolicy;
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        workspace
            .runningApplications()
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter(|app| app.processIdentifier() as u32 != std::process::id())
            .filter_map(|app| mac_identity(&app))
            .collect()
    }

    #[cfg(target_os = "windows")]
    fn foreground_target(&self) -> (Option<AppIdentity>, Option<u32>) {
        let Some((exe, pid)) = super::windows::foreground_executable() else {
            return (None, None);
        };
        (Some(windows_identity(exe)), Some(pid))
    }

    #[cfg(target_os = "windows")]
    fn available_apps(&self) -> Vec<AppIdentity> {
        super::windows::visible_executables()
            .into_iter()
            .map(windows_identity)
            .collect()
    }

    #[cfg(target_os = "linux")]
    fn available_apps(&self) -> Vec<AppIdentity> {
        use std::process::Command;
        if super::session_kind() != super::SessionKind::X11 {
            return Vec::new();
        }
        let Ok(output) = Command::new("xprop")
            .args(["-root", "-notype", "_NET_CLIENT_LIST"])
            .output()
        else {
            return Vec::new();
        };
        let Ok(text) = std::str::from_utf8(&output.stdout) else {
            return Vec::new();
        };
        let Some(ids) = text.trim().strip_prefix("_NET_CLIENT_LIST: window id # ") else {
            return Vec::new();
        };
        ids.split(", ")
            .take(100)
            .filter_map(|id| u32::from_str_radix(id.strip_prefix("0x")?, 16).ok())
            .filter_map(x11_identity)
            .collect()
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
    mac_identity(&app)
}

#[cfg(target_os = "macos")]
fn mac_identity(app: &objc2_app_kit::NSRunningApplication) -> Option<AppIdentity> {
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
    Some(windows_identity(super::windows::frontmost_app_name()?))
}

#[cfg(target_os = "windows")]
fn windows_identity(executable: String) -> AppIdentity {
    AppIdentity {
        display_name: Some(executable.clone()),
        windows_executable: Some(executable),
        ..AppIdentity::default()
    }
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
    x11_identity(id)
}

#[cfg(target_os = "linux")]
fn x11_identity(id: u32) -> Option<AppIdentity> {
    let property = std::process::Command::new("xprop")
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
