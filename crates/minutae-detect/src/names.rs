//! Map OS identifiers (macOS bundle ids, Windows consent-store keys,
//! executable names) to friendly app names.

/// Known call apps by identifier prefix (lower-case). Order matters: more
/// specific prefixes first.
const KNOWN: &[(&str, &str)] = &[
    // macOS bundle ids
    ("us.zoom.", "Zoom"),
    ("com.microsoft.teams", "Teams"),
    ("com.tinyspeck.slackmacgap", "Slack"),
    ("com.hnc.discord", "Discord"),
    ("com.apple.facetime", "FaceTime"),
    ("com.apple.avconferenced", "FaceTime"),
    ("com.cisco.webex", "Webex"),
    ("cisco-systems.spark", "Webex"),
    ("com.webex.", "Webex"),
    ("com.skype.", "Skype"),
    ("net.whatsapp.", "WhatsApp"),
    ("ru.keepcoder.telegram", "Telegram"),
    ("org.telegram.", "Telegram"),
    ("com.loom.", "Loom"),
    ("com.around.", "Around"),
    ("com.google.chrome", "Chrome (browser call)"),
    ("com.apple.safari", "Safari (browser call)"),
    ("com.apple.webkit", "Safari (browser call)"),
    ("company.thebrowser.", "Arc (browser call)"),
    ("com.microsoft.edgemac", "Edge (browser call)"),
    ("org.mozilla.firefox", "Firefox (browser call)"),
    ("com.brave.browser", "Brave (browser call)"),
    ("com.vivaldi.", "Vivaldi (browser call)"),
    ("com.operasoftware.", "Opera (browser call)"),
    // Windows packaged-app family names
    ("msteams_", "Teams"),
    ("microsoftteams_", "Teams"),
    ("microsoft.skypeapp_", "Skype"),
    ("5319275a.whatsappdesktop_", "WhatsApp"),
    ("microsoft.windowscommunicationsapps_", "Mail and Calendar"),
    // Executable stems (Windows NonPackaged, Linux application names)
    ("zoom", "Zoom"),
    ("ms-teams", "Teams"),
    ("teams", "Teams"),
    ("slack", "Slack"),
    ("discord", "Discord"),
    ("webex", "Webex"),
    ("ciscocollabhost", "Webex"),
    ("skype", "Skype"),
    ("whatsapp", "WhatsApp"),
    ("telegram", "Telegram"),
    ("chrome", "Chrome (browser call)"),
    ("google chrome", "Chrome (browser call)"),
    ("chromium", "Chromium (browser call)"),
    ("msedge", "Edge (browser call)"),
    ("microsoft edge", "Edge (browser call)"),
    ("firefox", "Firefox (browser call)"),
    ("brave", "Brave (browser call)"),
    ("arc", "Arc (browser call)"),
    ("opera", "Opera (browser call)"),
    ("vivaldi", "Vivaldi (browser call)"),
];

/// macOS processes that hold the mic open without being a call (Siri,
/// dictation, system audio services). Never reported as Busy.
const MACOS_IGNORED: &[&str] = &[
    "com.apple.corespeechd",
    "com.apple.siri",
    "com.apple.assistant",
    "com.apple.speech",
    "com.apple.dictation",
    "com.apple.coreaudiod",
    "com.apple.audio.",
    "com.apple.accessibility.heard",
    "com.apple.controlcenter",
    "com.apple.systemuiserver",
];

fn known(id_lower: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|(p, _)| id_lower.starts_with(p))
        .map(|(_, n)| *n)
}

pub fn macos_is_ignored(bundle_id: &str) -> bool {
    let b = bundle_id.to_ascii_lowercase();
    MACOS_IGNORED.iter().any(|p| b.starts_with(p))
}

/// `us.zoom.xos` -> "Zoom"; `com.google.Chrome.helper` -> "Chrome (browser call)";
/// unknown `com.acme.Meeter.helper` -> "Meeter".
pub fn from_bundle_id(bundle_id: &str) -> String {
    let lower = bundle_id.to_ascii_lowercase();
    if let Some(n) = known(&lower) {
        return n.to_string();
    }
    let parts: Vec<&str> = bundle_id.split('.').collect();
    // Drop helper-ish suffixes, then use the last meaningful component.
    let meaningful = parts
        .iter()
        .rev()
        .find(|p| {
            let l = p.to_ascii_lowercase();
            !matches!(
                l.as_str(),
                "helper"
                    | "renderer"
                    | "gpu"
                    | "plugin"
                    | "xpc"
                    | "app"
                    | "mac"
                    | "macos"
                    | "desktop"
            ) && !l.is_empty()
        })
        .copied()
        .unwrap_or(bundle_id);
    title_case(meaningful)
}

/// Name for a Windows consent-store subkey.
/// - Packaged: `MSTeams_8wekyb3d8bbwe` -> "Teams"
/// - NonPackaged: `C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe` -> "Zoom"
pub fn from_windows_key(key: &str, non_packaged: bool) -> String {
    if non_packaged {
        let exe = key.rsplit(['#', '\\', '/']).next().unwrap_or(key);
        return from_exe(exe);
    }
    let lower = key.to_ascii_lowercase();
    if let Some(n) = known(&lower) {
        return n.to_string();
    }
    // `Publisher.AppName_hash` -> "AppName"
    let family = key.split('_').next().unwrap_or(key);
    let app = family.rsplit('.').next().unwrap_or(family);
    title_case(app)
}

/// `Zoom.exe` / `zoom` / `Google Chrome` -> friendly name.
pub fn from_exe(exe: &str) -> String {
    let stem = exe
        .strip_suffix(".exe")
        .or_else(|| exe.strip_suffix(".EXE"))
        .unwrap_or(exe);
    let lower = stem.to_ascii_lowercase();
    // Whole-word prefix only, so "Archiver" is not "Arc".
    let hit = KNOWN.iter().find(|(p, _)| {
        lower.starts_with(p)
            && lower[p.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric())
    });
    match hit {
        Some((_, n)) => n.to_string(),
        None => title_case(stem),
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Windows consent-store key name for an executable path (`\` -> `#`).
pub fn windows_key_for_path(path: &str) -> String {
    path.replace('\\', "#")
}

/// A Windows consent-store entry is "in use" when it has started and has no
/// stop time yet (`LastUsedTimeStop == 0`).
pub fn consent_entry_in_use(last_start: Option<u64>, last_stop: Option<u64>) -> bool {
    matches!((last_start, last_stop), (Some(start), Some(0)) if start > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_ids() {
        assert_eq!(from_bundle_id("us.zoom.xos"), "Zoom");
        assert_eq!(from_bundle_id("com.microsoft.teams2"), "Teams");
        assert_eq!(from_bundle_id("com.microsoft.teams2.helper"), "Teams");
        assert_eq!(
            from_bundle_id("com.google.Chrome.helper"),
            "Chrome (browser call)"
        );
        assert_eq!(
            from_bundle_id("com.google.Chrome.app.kjgfgldnnfoeklkmfkjfagphfepbbdan"),
            "Chrome (browser call)"
        );
        assert_eq!(
            from_bundle_id("com.apple.WebKit.GPU"),
            "Safari (browser call)"
        );
        assert_eq!(
            from_bundle_id("company.thebrowser.Browser"),
            "Arc (browser call)"
        );
        assert_eq!(from_bundle_id("com.tinyspeck.slackmacgap.helper"), "Slack");
        assert_eq!(from_bundle_id("com.apple.FaceTime"), "FaceTime");
        assert_eq!(from_bundle_id("com.acme.meeter.helper"), "Meeter");
    }

    #[test]
    fn ignored_system_processes() {
        assert!(macos_is_ignored("com.apple.corespeechd"));
        assert!(macos_is_ignored("com.apple.siri.embeddedspeech"));
        assert!(!macos_is_ignored("us.zoom.xos"));
        assert!(!macos_is_ignored("com.apple.FaceTime"));
    }

    #[test]
    fn windows_keys() {
        assert_eq!(from_windows_key("MSTeams_8wekyb3d8bbwe", false), "Teams");
        assert_eq!(
            from_windows_key("Microsoft.SkypeApp_kzf8qxf38zg5c", false),
            "Skype"
        );
        assert_eq!(from_windows_key("Contoso.CallMe_abc123", false), "CallMe");
        assert_eq!(
            from_windows_key("C:#Users#me#AppData#Roaming#Zoom#bin#Zoom.exe", true),
            "Zoom"
        );
        assert_eq!(
            from_windows_key(
                "C:#Program Files#Google#Chrome#Application#chrome.exe",
                true
            ),
            "Chrome (browser call)"
        );
        assert_eq!(from_windows_key("C:#Tools#recorder.exe", true), "Recorder");
        assert_eq!(
            windows_key_for_path(r"C:\Apps\minutae.exe"),
            "C:#Apps#minutae.exe"
        );
    }

    #[test]
    fn consent_in_use() {
        assert!(consent_entry_in_use(Some(133_000_000_000_000_000), Some(0)));
        assert!(!consent_entry_in_use(
            Some(133_000_000_000_000_000),
            Some(133_000_000_100_000_000)
        ));
        assert!(!consent_entry_in_use(None, Some(0)));
        assert!(!consent_entry_in_use(Some(0), Some(0)));
        assert!(!consent_entry_in_use(Some(5), None));
    }

    #[test]
    fn exe_names() {
        assert_eq!(from_exe("Zoom.exe"), "Zoom");
        assert_eq!(from_exe("Google Chrome"), "Chrome (browser call)");
        assert_eq!(from_exe("Firefox"), "Firefox (browser call)");
        assert_eq!(from_exe("ZOOM VoiceEngine"), "Zoom");
        assert_eq!(from_exe("Archiver"), "Archiver"); // not "arc"
        assert_eq!(from_exe("obs"), "Obs");
    }
}
