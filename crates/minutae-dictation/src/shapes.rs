//! Stored shape rules and deterministic selection for a foreground app.

use serde::{Deserialize, Serialize};

use crate::platform::identity::AppIdentity;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapePreset {
    Verbatim,
    #[default]
    Tidy,
    ChatMessage,
    Email,
    NotesAndLists,
    CodeAndCommit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capitals {
    /// Capitalize sentences and the pronoun I.
    Sentence,
    /// Keep the transcript's capitalization.
    AsSaid,
    /// Capitalize the pronoun I and keep other words as recognized.
    Relaxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineBreaks {
    /// Keep spoken break phrases as words and flatten actual line breaks.
    SingleLine,
    /// Preserve line breaks already present in the transcript.
    Preserve,
    /// Honor spoken "new line" and "new paragraph". Tidy folds raw line
    /// breaks in this mode for compatibility with its legacy behavior.
    Spoken,
    /// Honor spoken breaks and lay out clearly delimited greetings and sign-offs.
    Email,
    /// Honor spoken breaks and explicit "bullet point" cues.
    NotesAndLists,
}

impl LineBreaks {
    pub(crate) const fn honors_spoken_breaks(self) -> bool {
        matches!(self, Self::Spoken | Self::Email | Self::NotesAndLists)
    }
}

/// A preset with editable rules. Verbatim and Code & commit preserve wording
/// and case by bypassing prose cleanup and vocabulary substitution. They
/// interpret spoken punctuation and line breaks only when enabled and prefixed
/// with "insert", e.g. "insert colon" or "insert new line".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub preset: ShapePreset,
    pub capitals: Capitals,
    pub drop_final_period: bool,
    pub tidy: bool,
    pub spoken_punctuation: bool,
    pub line_breaks: LineBreaks,
    /// Reserved for a future local rewrite. The rules engine ignores it.
    pub rewrite_with_local_model: bool,
}

impl Default for Shape {
    fn default() -> Self {
        ShapePreset::Tidy.shape()
    }
}

impl ShapePreset {
    /// The initial rules for this preset and the values used by Reset shape.
    pub const fn shape(self) -> Shape {
        let mut shape = Shape {
            preset: self,
            capitals: Capitals::Sentence,
            drop_final_period: false,
            tidy: true,
            spoken_punctuation: true,
            line_breaks: LineBreaks::Spoken,
            rewrite_with_local_model: false,
        };
        match self {
            Self::Tidy => {}
            Self::Verbatim | Self::CodeAndCommit => {
                shape.capitals = Capitals::AsSaid;
                shape.tidy = false;
                shape.spoken_punctuation = false;
                shape.line_breaks = if matches!(self, Self::Verbatim) {
                    LineBreaks::SingleLine
                } else {
                    LineBreaks::Preserve
                };
            }
            Self::ChatMessage => {
                shape.capitals = Capitals::Relaxed;
                shape.drop_final_period = true;
            }
            Self::Email => shape.line_breaks = LineBreaks::Email,
            Self::NotesAndLists => shape.line_breaks = LineBreaks::NotesAndLists,
        }
        shape
    }

    pub(crate) const fn preserves_identifiers(self) -> bool {
        matches!(self, Self::Verbatim | Self::CodeAndCommit)
    }
}

/// Exact, ASCII-case-insensitive match against one field of an app identity.
/// Windows executable names may be stored with or without the .exe suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AppMatcher {
    MacosBundleId(String),
    WindowsExecutable(String),
    X11WindowClass(String),
    DisplayName(String),
}

impl AppMatcher {
    pub fn matches(&self, app: &AppIdentity) -> bool {
        let (actual, expected) = match self {
            Self::MacosBundleId(value) => (app.macos_bundle_id.as_deref(), value),
            Self::WindowsExecutable(value) => {
                return app
                    .windows_executable
                    .as_deref()
                    .map(executable_stem)
                    .is_some_and(|actual| {
                        !actual.is_empty() && actual.eq_ignore_ascii_case(executable_stem(value))
                    });
            }
            Self::X11WindowClass(value) => (app.x11_window_class.as_deref(), value),
            Self::DisplayName(value) => (app.display_name.as_deref(), value),
        };
        actual.is_some_and(|actual| !actual.is_empty() && actual.eq_ignore_ascii_case(expected))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRule {
    pub app: AppMatcher,
    pub shape: Shape,
}

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

fn executable_stem(name: &str) -> &str {
    let suffix_start = name.len().saturating_sub(4);
    match name.get(suffix_start..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".exe") => &name[..suffix_start],
        _ => name,
    }
}
