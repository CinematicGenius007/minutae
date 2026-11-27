//! Stored shape rules and deterministic selection for a foreground app.

pub use minutae_core::dictation::{AppMatcher, AppRule, Capitals, LineBreaks, Shape, ShapePreset};

use crate::platform::identity::AppIdentity;
use minutae_core::dictation::executable_stem;

/// A suggestion for a known app. Terminals use this automatically; other
/// suggestions are available to the UI when creating an app rule.
pub fn suggested_preset(app: &AppIdentity) -> Option<ShapePreset> {
    use ShapePreset::*;

    // macOS bundle ids, Windows image stems, and X11 WM_CLASS (class component).
    type Aliases = &'static [&'static str];
    let candidates: &[(Aliases, Aliases, Aliases, ShapePreset)] = &[
        (
            &[
                "com.apple.Terminal",
                "com.googlecode.iterm2",
                "dev.warp.Warp-Stable",
                "com.mitchellh.ghostty",
                "net.kovidgoyal.kitty",
                "org.alacritty",
                "com.github.wez.wezterm",
                "co.zeit.hyper",
            ],
            &[
                "WindowsTerminal",
                "OpenConsole",
                "conhost",
                "cmd",
                "powershell",
                "pwsh",
                "mintty",
                "alacritty",
                "wezterm-gui",
            ],
            &[
                "Gnome-terminal",
                "org.gnome.Terminal",
                "konsole",
                "XTerm",
                "kitty",
                "Alacritty",
                "org.wezfurlong.wezterm",
                "com.mitchellh.ghostty",
                "xfce4-terminal",
                "tilix",
                "mate-terminal",
                "lxterminal",
            ],
            Verbatim,
        ),
        (
            &["com.tinyspeck.slackmacgap"],
            &["slack"],
            &["Slack"],
            ChatMessage,
        ),
        (
            &["com.apple.mail", "com.microsoft.Outlook"],
            &["OUTLOOK", "olk"],
            &["Outlook"],
            Email,
        ),
        (
            &["com.microsoft.VSCode", "com.microsoft.VSCodeInsiders"],
            &["Code", "Code - Insiders"],
            &["Code", "Code - Insiders"],
            CodeAndCommit,
        ),
        (
            &["com.apple.Notes", "md.obsidian"],
            &["Obsidian"],
            &["obsidian"],
            NotesAndLists,
        ),
    ];
    for &(mac, windows, x11, preset) in candidates {
        if app
            .macos_bundle_id
            .as_deref()
            .is_some_and(|id| contains(mac, id))
            || app
                .windows_executable
                .as_deref()
                .is_some_and(|exe| contains(windows, executable_stem(exe)))
            || app
                .x11_window_class
                .as_deref()
                .is_some_and(|class| contains(x11, class))
        {
            return Some(preset);
        }
    }

    // Some macOS apps have a localized name but no bundle id.
    if app.macos_bundle_id.is_some()
        || app.windows_executable.is_some()
        || app.x11_window_class.is_some()
    {
        return None;
    }
    match app.display_name.as_deref()?.to_ascii_lowercase().as_str() {
        "terminal" | "iterm" | "iterm2" | "windows terminal" | "konsole" | "xterm" | "ghostty"
        | "alacritty" | "kitty" | "wezterm" => Some(Verbatim),
        "slack" => Some(ChatMessage),
        "mail" | "outlook" | "microsoft outlook" => Some(Email),
        "visual studio code" | "visual studio code - insiders" => Some(CodeAndCommit),
        "notes" | "obsidian" => Some(NotesAndLists),
        _ => None,
    }
}

/// First matching user rule wins. Known terminals default to Verbatim, and
/// other apps (including an unknown identity) use the user's default.
pub fn resolve_shape(app: Option<&AppIdentity>, rules: &[AppRule], default: &Shape) -> Shape {
    let Some(app) = app else {
        return *default;
    };
    if let Some(rule) = rules.iter().find(|rule| rule.app.matches(app)) {
        return rule.shape;
    }
    if suggested_preset(app) == Some(ShapePreset::Verbatim) {
        return ShapePreset::Verbatim.shape();
    }
    *default
}

fn contains(values: &[&str], value: &str) -> bool {
    values.iter().any(|entry| entry.eq_ignore_ascii_case(value))
}
