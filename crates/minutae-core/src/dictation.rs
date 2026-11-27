//! Dictation settings and history shared by the engine and store.

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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub const fn honors_spoken_breaks(self) -> bool {
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

    pub const fn preserves_identifiers(self) -> bool {
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

pub fn executable_stem(name: &str) -> &str {
    let suffix_start = name.len().saturating_sub(4);
    match name.get(suffix_start..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".exe") => &name[..suffix_start],
        _ => name,
    }
}

impl AppIdentity {
    /// Prefer stable platform identifiers over a localized name.
    pub fn matcher(&self) -> Option<AppMatcher> {
        [
            self.macos_bundle_id
                .as_ref()
                .map(|s| AppMatcher::MacosBundleId(s.clone())),
            self.windows_executable
                .as_ref()
                .map(|s| AppMatcher::WindowsExecutable(s.clone())),
            self.x11_window_class
                .as_ref()
                .map(|s| AppMatcher::X11WindowClass(s.clone())),
            self.display_name
                .as_ref()
                .map(|s| AppMatcher::DisplayName(s.clone())),
        ]
        .into_iter()
        .flatten()
        .find(|app| !app.value().is_empty())
    }
}

impl AppMatcher {
    pub fn value(&self) -> &str {
        match self {
            Self::MacosBundleId(s)
            | Self::WindowsExecutable(s)
            | Self::X11WindowClass(s)
            | Self::DisplayName(s) => s,
        }
    }

    pub fn same_app(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::WindowsExecutable(a), Self::WindowsExecutable(b)) => {
                executable_stem(a).eq_ignore_ascii_case(executable_stem(b))
            }
            _ => {
                std::mem::discriminant(self) == std::mem::discriminant(other)
                    && self.value().eq_ignore_ascii_case(other.value())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationAppRule {
    pub app: AppMatcher,
    pub display_name: String,
    pub preset: ShapePreset,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShapeEdits {
    pub capitals: Option<Capitals>,
    pub drop_final_period: Option<bool>,
}

impl ShapeEdits {
    pub fn apply(self, mut shape: Shape) -> Shape {
        if shape.preset.preserves_identifiers() {
            return shape;
        }
        if let Some(capitals) = self.capitals {
            shape.capitals = capitals;
        }
        if let Some(drop) = self.drop_final_period {
            shape.drop_final_period = drop;
        }
        shape
    }
}
