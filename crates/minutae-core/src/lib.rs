//! Shared types for every Minutae crate and the Tauri app.
//!
//! Rules: plain data only (no I/O except `paths`), serde everywhere,
//! `camelCase` on the wire so the TypeScript side reads it directly.
//! Changing a type here is a cross-crate change: keep additions backward compatible.

pub mod paths;
pub mod settings;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use settings::*;

pub type MeetingId = Uuid;
pub type DictationId = Uuid;
pub type SpeakerId = Uuid;
pub type JobId = Uuid;

/// Lifecycle of a meeting. Stored as the lowercase string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingStatus {
    Recording,
    Processing,
    Done,
    Failed,
}

/// Which capture stream a piece of audio or text came from.
/// `Mic` is always the local user ("You"); `System` is everyone else on the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Track {
    Mic,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: MeetingId,
    pub title: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: MeetingStatus,
    /// App that was using the mic when the call was detected, e.g. "Zoom".
    pub source_app: Option<String>,
    /// Plain-language reason when `status == Failed`.
    pub failure: Option<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    pub id: SpeakerId,
    pub meeting_id: MeetingId,
    /// Stable label from diarization: "You", "Speaker 1", "Speaker 2"...
    pub label: String,
    /// Name the user typed, e.g. "Alice".
    pub display_name: Option<String>,
    pub track: Track,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub track: Track,
    pub speaker_id: Option<SpeakerId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub meeting_id: MeetingId,
    pub language: Option<String>,
    pub model: String,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub meeting_id: MeetingId,
    /// One-paragraph overview (the serif lead on the detail screen).
    pub overview: String,
    pub decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
    /// One line used in the meetings list.
    pub takeaway: String,
    /// Full markdown written to disk.
    pub markdown: String,
    pub provider: String,
    pub model: String,
}

/// A user-marked moment during recording (⌘M).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub meeting_id: MeetingId,
    pub at_ms: u64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dictation {
    pub id: DictationId,
    pub created_at: DateTime<Utc>,
    pub text: String,
    pub raw_text: String,
    pub target_app: Option<String>,
    pub word_count: u32,
    pub duration_ms: u64,
    pub inserted: bool,
}

/// Classified failure shown to the user as one sentence + one fix.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    /// Plain sentence, e.g. "Codex isn't signed in."
    pub message: String,
    /// Button label for the fix, e.g. "Sign in to Codex".
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    NotInstalled,
    NotLoggedIn,
    RateLimited,
    Timeout,
    Cancelled,
    InvalidOutput,
    ModelRejected,
    Unavailable,
    NoProviderReady,
    PermissionDenied,
    ModelMissing,
    Io,
    Internal,
}

/// Progress pushed to the UI for any long job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub meeting_id: Option<MeetingId>,
    pub stage: Stage,
    /// 0.0..=1.0
    pub fraction: f32,
    pub eta_seconds: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Transcribing,
    Diarizing,
    Summarizing,
    Indexing,
    DownloadingModel,
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&MeetingStatus::Processing).unwrap(),
            "\"processing\""
        );
        assert_eq!(serde_json::to_string(&Track::System).unwrap(), "\"system\"");
    }

    #[test]
    fn segment_is_camel_case() {
        let s = Segment {
            start_ms: 1,
            end_ms: 2,
            text: "hi".into(),
            track: Track::Mic,
            speaker_id: None,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("startMs").is_some());
    }
}
