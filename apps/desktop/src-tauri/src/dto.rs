//! Wire types for the UI. Each struct serializes to exactly the shape in
//! `apps/desktop/src/lib/types.ts` (camelCase; missing data is `null`, never renamed).
//! Shared enums (`MeetingStatus`, `Track`, `Stage`, `FailureKind`, `AppSettings`…) come
//! from `minutae-core`, which already uses the same wire names.

use minutae_core::{Failure, MeetingStatus, Progress, Track, WhisperModel};
use serde::{Deserialize, Serialize};

pub use minutae_core::{AppSettings, Moment};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: MeetingStatus,
    pub source_app: Option<String>,
    pub failure: Option<Failure>,
}

impl From<&minutae_core::Meeting> for Meeting {
    fn from(m: &minutae_core::Meeting) -> Self {
        Self {
            id: m.id.to_string(),
            title: m.title.clone(),
            started_at: m.started_at.to_rfc3339(),
            ended_at: m.ended_at.map(|t| t.to_rfc3339()),
            status: m.status,
            source_app: m.source_app.clone(),
            failure: m.failure.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingListItem {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub duration_ms: Option<u64>,
    pub status: MeetingStatus,
    pub takeaway: Option<String>,
    pub people: Vec<String>,
    pub open_actions: u32,
    pub total_actions: u32,
    pub progress: Option<Progress>,
    pub failure: Option<Failure>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayGroup {
    pub label: String,
    pub items: Vec<MeetingListItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekStats {
    pub recorded_ms: u64,
    pub open_actions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speaker {
    pub id: String,
    pub meeting_id: String,
    pub label: String,
    pub display_name: Option<String>,
    pub track: Track,
}

impl From<&minutae_core::Speaker> for Speaker {
    fn from(s: &minutae_core::Speaker) -> Self {
        Self {
            id: s.id.to_string(),
            meeting_id: s.meeting_id.to_string(),
            label: s.label.clone(),
            display_name: s.display_name.clone(),
            track: s.track,
        }
    }
}

/// `minutae_core::Segment` already has the TS shape; re-exported for the key test.
pub type Segment = minutae_core::Segment;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    /// The store's row id as a string (the UI treats ids as opaque strings).
    pub id: String,
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub done: bool,
}

impl From<&minutae_store::ActionItemRow> for ActionItem {
    fn from(r: &minutae_store::ActionItemRow) -> Self {
        Self {
            id: r.id.to_string(),
            text: r.item.text.clone(),
            owner: r.item.owner.clone(),
            due: r.item.due.clone(),
            done: r.item.done,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MomentDto {
    pub meeting_id: String,
    pub at_ms: u64,
    pub label: String,
}

impl From<&Moment> for MomentDto {
    fn from(m: &Moment) -> Self {
        Self {
            meeting_id: m.meeting_id.to_string(),
            at_ms: m.at_ms,
            label: m.label.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub overview: String,
    pub points: Vec<String>,
    pub decisions: Vec<String>,
    pub takeaway: String,
    pub markdown: String,
    pub markdown_path: Option<String>,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPaths {
    pub mic: Option<String>,
    pub system: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub duration_ms: Option<u64>,
    pub speakers: Vec<Speaker>,
    pub segments: Vec<Segment>,
    pub summary: Option<Summary>,
    pub action_items: Vec<ActionItem>,
    pub moments: Vec<MomentDto>,
    pub audio: Option<AudioPaths>,
    pub progress: Option<Progress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingPhase {
    Idle,
    Recording,
    Paused,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemAudio {
    On,
    Off,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingState {
    pub phase: RecordingPhase,
    pub meeting_id: Option<String>,
    pub title: Option<String>,
    pub source_app: Option<String>,
    pub elapsed_ms: u64,
    pub system_audio: SystemAudio,
    pub system_audio_error: Option<String>,
}

impl RecordingState {
    pub fn idle(include_system: bool) -> Self {
        Self {
            phase: RecordingPhase::Idle,
            meeting_id: None,
            title: None,
            source_app: None,
            elapsed_ms: 0,
            system_audio: if include_system {
                SystemAudio::On
            } else {
                SystemAudio::Off
            },
            system_audio_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelFrame {
    pub elapsed_ms: u64,
    pub mic: f32,
    pub system: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dictation {
    pub id: String,
    pub created_at: String,
    pub text: String,
    pub raw_text: String,
    pub target_app: Option<String>,
    pub word_count: u32,
    pub duration_ms: u64,
    pub inserted: bool,
}

impl From<&minutae_core::Dictation> for Dictation {
    fn from(d: &minutae_core::Dictation) -> Self {
        Self {
            id: d.id.to_string(),
            created_at: d.created_at.to_rfc3339(),
            text: d.text.clone(),
            raw_text: d.raw_text.clone(),
            target_app: d.target_app.clone(),
            word_count: d.word_count,
            duration_ms: d.duration_ms,
            inserted: d.inserted,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStats {
    pub words_this_week: u64,
    pub minutes_saved: u64,
}

/// `{ state: "copied", words, reason }` etc. Same tagging as the dictation crate's
/// `HudState`, but `reason` is a plain string as in types.ts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HudState {
    Idle,
    Listening { level: f32, hands_free: bool },
    Processing,
    Inserted { words: u32 },
    Copied { words: u32, reason: String },
    Error { message: String },
}

impl From<&minutae_dictation::HudState> for HudState {
    fn from(h: &minutae_dictation::HudState) -> Self {
        use minutae_dictation::HudState as H;
        match h {
            H::Idle => HudState::Idle,
            H::Listening { level, hands_free } => HudState::Listening {
                level: *level,
                hands_free: *hands_free,
            },
            H::Processing => HudState::Processing,
            H::Inserted { words } => HudState::Inserted { words: *words },
            H::Copied { words, reason } => HudState::Copied {
                words: *words,
                reason: copy_reason_text(*reason).into(),
            },
            H::Error { message } => HudState::Error {
                message: message.clone(),
            },
        }
    }
}

pub fn copy_reason_text(r: minutae_dictation::CopyReason) -> &'static str {
    use minutae_dictation::CopyReason as C;
    match r {
        C::AccessibilityNotTrusted => "Allow Accessibility to paste automatically",
        C::NoTextField => "No text field",
        C::Wayland => "Wayland doesn't allow typing into other apps",
        C::SecureInput => "Secure input is on",
        C::PasteFailed => "Paste didn't work",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HitKind {
    Meeting,
    Dictation,
    ActionItem,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: HitKind,
    pub meeting_id: Option<String>,
    pub dictation_id: Option<String>,
    pub title: String,
    pub snippet: String,
    pub highlights: Vec<[u32; 2]>,
    pub at_ms: Option<u64>,
    pub date: String,
}

impl From<&minutae_store::SearchHit> for SearchHit {
    fn from(h: &minutae_store::SearchHit) -> Self {
        use minutae_store::HitKind as K;
        Self {
            kind: match h.kind {
                K::Meeting | K::Transcript => HitKind::Meeting,
                K::Summary => HitKind::Summary,
                K::ActionItem => HitKind::ActionItem,
                K::Dictation => HitKind::Dictation,
            },
            meeting_id: h.meeting_id.map(|u| u.to_string()),
            dictation_id: h.dictation_id.map(|u| u.to_string()),
            title: h.title.clone(),
            snippet: h.snippet.clone(),
            highlights: h.highlights.clone(),
            at_ms: h.at_ms,
            date: h.date.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub text: String,
    pub sources: Vec<SearchHit>,
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
    NotRequired,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub microphone: PermissionState,
    pub system_audio: PermissionState,
    pub accessibility: PermissionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Microphone,
    SystemAudio,
    Accessibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    X11,
    Wayland,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: Os,
    pub session_kind: Option<SessionKind>,
    pub app_version: String,
}

/// Where the in-app updater is. `disabled`: this build has no updater public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    Disabled,
    Idle,
    Checking,
    UpToDate,
    Available,
    Installing,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    /// The newer version on offer (`available`, `installing`).
    pub version: Option<String>,
    /// Release notes of that version, if the release has any.
    pub notes: Option<String>,
    /// Download progress 0..1 while `installing`; `null` when the size is unknown.
    pub fraction: Option<f32>,
    /// One plain sentence when `error`.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelState {
    Missing,
    Downloading,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub model: WhisperModel,
    pub size_bytes: u64,
    pub state: ModelState,
    pub fraction: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizeModelInfo {
    pub state: ModelState,
    pub fraction: Option<f32>,
    pub size_bytes: u64,
}

/// Payload of `model://progress`. `model` is a `WhisperModel` name or `"diarize"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgress {
    pub model: String,
    pub fraction: f32,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Ready,
    NotInstalled,
    NotLoggedIn,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub transport: minutae_core::Transport,
    pub name: String,
    pub availability: Availability,
    pub detail: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiProvider {
    Anthropic,
    Openai,
    Gemini,
    Grok,
}

impl ApiProvider {
    /// Keychain account. The first three match `minutae_llm::ApiProvider::secret_account`.
    pub fn secret_account(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => minutae_llm::ApiProvider::Anthropic.secret_account(),
            ApiProvider::Openai => minutae_llm::ApiProvider::OpenAi.secret_account(),
            ApiProvider::Gemini => minutae_llm::ApiProvider::Gemini.secret_account(),
            ApiProvider::Grok => "grok-api-key",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageByProvider {
    pub provider: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub day: u32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMonth {
    pub year: i32,
    pub month: u32,
    pub total_usd: f64,
    pub summaries: u64,
    pub tokens: u64,
    pub free_share: f64,
    pub by_provider: Vec<UsageByProvider>,
    pub by_day: Vec<UsageDay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub recordings_bytes: u64,
    pub models_bytes: u64,
    pub index_bytes: u64,
    pub data_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallDetected {
    pub app: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsChanged {
    pub meeting_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    pub route: String,
}

/// Event names (mirror `events` in api.ts).
pub mod ev {
    pub const RECORDING_STATE: &str = "recording://state";
    pub const LEVELS: &str = "recording://levels";
    pub const PROGRESS: &str = "job://progress";
    pub const MEETINGS_CHANGED: &str = "meetings://changed";
    pub const DICTATIONS_CHANGED: &str = "dictations://changed";
    pub const CALL_DETECTED: &str = "call://detected";
    pub const CALL_ENDED: &str = "call://ended";
    pub const HUD: &str = "dictation://hud";
    pub const MODEL_PROGRESS: &str = "model://progress";
    pub const SETTINGS_CHANGED: &str = "settings://changed";
    pub const NAVIGATE: &str = "nav://goto";
    pub const UPDATE_STATUS: &str = "update://status";
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::Utc;
    use minutae_core::{FailureKind, Stage};
    use serde_json::Value;
    use uuid::Uuid;

    use super::*;

    /// Keys of a serialized object, compared to the hand-written list from types.ts.
    fn assert_keys<T: Serialize>(v: &T, expected: &[&str]) {
        let json = serde_json::to_value(v).unwrap();
        let got: BTreeSet<String> = json.as_object().expect("object").keys().cloned().collect();
        let want: BTreeSet<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(got, want, "keys of {}", std::any::type_name::<T>());
    }

    fn failure() -> Failure {
        Failure {
            kind: FailureKind::NoProviderReady,
            message: "m".into(),
            fix: Some("f".into()),
        }
    }

    fn progress() -> Progress {
        Progress {
            meeting_id: Some(Uuid::nil()),
            stage: Stage::Transcribing,
            fraction: 0.5,
            eta_seconds: None,
        }
    }

    fn meeting() -> Meeting {
        Meeting::from(&minutae_core::Meeting {
            id: Uuid::nil(),
            title: "t".into(),
            started_at: Utc::now(),
            ended_at: None,
            status: MeetingStatus::Failed,
            source_app: None,
            failure: Some(failure()),
        })
    }

    #[test]
    fn core_shapes() {
        assert_keys(&failure(), &["kind", "message", "fix"]);
        assert_keys(
            &progress(),
            &["meetingId", "stage", "fraction", "etaSeconds"],
        );
        assert_eq!(
            serde_json::to_value(Stage::DownloadingModel).unwrap(),
            "downloadingModel"
        );
        assert_eq!(
            serde_json::to_value(FailureKind::ModelMissing).unwrap(),
            "modelMissing"
        );
        assert_eq!(
            serde_json::to_value(MeetingStatus::Processing).unwrap(),
            "processing"
        );
    }

    #[test]
    fn meeting_shapes() {
        let m = meeting();
        assert_keys(
            &m,
            &[
                "id",
                "title",
                "startedAt",
                "endedAt",
                "status",
                "sourceApp",
                "failure",
            ],
        );
        let item = MeetingListItem {
            id: m.id.clone(),
            title: "t".into(),
            started_at: m.started_at.clone(),
            duration_ms: None,
            status: MeetingStatus::Done,
            takeaway: None,
            people: vec![],
            open_actions: 0,
            total_actions: 0,
            progress: Some(progress()),
            failure: None,
        };
        assert_keys(
            &item,
            &[
                "id",
                "title",
                "startedAt",
                "durationMs",
                "status",
                "takeaway",
                "people",
                "openActions",
                "totalActions",
                "progress",
                "failure",
            ],
        );
        assert_keys(
            &DayGroup {
                label: "Today".into(),
                items: vec![item],
            },
            &["label", "items"],
        );
        assert_keys(&WeekStats::default(), &["recordedMs", "openActions"]);
    }

    #[test]
    fn detail_shapes() {
        let speaker = Speaker {
            id: "s".into(),
            meeting_id: "m".into(),
            label: "You".into(),
            display_name: None,
            track: Track::Mic,
        };
        assert_keys(
            &speaker,
            &["id", "meetingId", "label", "displayName", "track"],
        );
        let seg = Segment {
            start_ms: 0,
            end_ms: 1,
            text: "x".into(),
            track: Track::System,
            speaker_id: None,
        };
        assert_keys(&seg, &["startMs", "endMs", "text", "track", "speakerId"]);
        let action = ActionItem {
            id: "1".into(),
            text: "x".into(),
            owner: None,
            due: None,
            done: false,
        };
        assert_keys(&action, &["id", "text", "owner", "due", "done"]);
        let moment = MomentDto {
            meeting_id: "m".into(),
            at_ms: 1,
            label: "l".into(),
        };
        assert_keys(&moment, &["meetingId", "atMs", "label"]);
        let summary = Summary {
            overview: "o".into(),
            points: vec![],
            decisions: vec![],
            takeaway: "t".into(),
            markdown: "#".into(),
            markdown_path: None,
            provider: "p".into(),
            model: "m".into(),
        };
        assert_keys(
            &summary,
            &[
                "overview",
                "points",
                "decisions",
                "takeaway",
                "markdown",
                "markdownPath",
                "provider",
                "model",
            ],
        );
        let audio = AudioPaths {
            mic: None,
            system: None,
        };
        assert_keys(&audio, &["mic", "system"]);
        let detail = MeetingDetail {
            meeting: meeting(),
            duration_ms: None,
            speakers: vec![speaker],
            segments: vec![seg],
            summary: Some(summary),
            action_items: vec![action],
            moments: vec![moment],
            audio: Some(audio),
            progress: None,
        };
        assert_keys(
            &detail,
            &[
                "meeting",
                "durationMs",
                "speakers",
                "segments",
                "summary",
                "actionItems",
                "moments",
                "audio",
                "progress",
            ],
        );
    }

    #[test]
    fn recording_shapes() {
        let s = RecordingState::idle(true);
        assert_keys(
            &s,
            &[
                "phase",
                "meetingId",
                "title",
                "sourceApp",
                "elapsedMs",
                "systemAudio",
                "systemAudioError",
            ],
        );
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["phase"], "idle");
        assert_eq!(v["systemAudio"], "on");
        assert_keys(
            &LevelFrame {
                elapsed_ms: 0,
                mic: 0.0,
                system: 0.0,
            },
            &["elapsedMs", "mic", "system"],
        );
        assert_keys(&CallDetected { app: None }, &["app"]);
        assert_keys(&MeetingsChanged { meeting_id: None }, &["meetingId"]);
        assert_keys(&Navigate { route: "/".into() }, &["route"]);
    }

    #[test]
    fn dictation_shapes() {
        let d = Dictation {
            id: "d".into(),
            created_at: "x".into(),
            text: "t".into(),
            raw_text: "t".into(),
            target_app: None,
            word_count: 1,
            duration_ms: 1,
            inserted: true,
        };
        assert_keys(
            &d,
            &[
                "id",
                "createdAt",
                "text",
                "rawText",
                "targetApp",
                "wordCount",
                "durationMs",
                "inserted",
            ],
        );
        assert_keys(
            &DictationStats::default(),
            &["wordsThisWeek", "minutesSaved"],
        );

        let cases: Vec<(HudState, Vec<&str>, &str)> = vec![
            (HudState::Idle, vec!["state"], "idle"),
            (
                HudState::Listening {
                    level: 0.1,
                    hands_free: true,
                },
                vec!["state", "level", "handsFree"],
                "listening",
            ),
            (HudState::Processing, vec!["state"], "processing"),
            (
                HudState::Inserted { words: 3 },
                vec!["state", "words"],
                "inserted",
            ),
            (
                HudState::Copied {
                    words: 3,
                    reason: "r".into(),
                },
                vec!["state", "words", "reason"],
                "copied",
            ),
            (
                HudState::Error {
                    message: "m".into(),
                },
                vec!["state", "message"],
                "error",
            ),
        ];
        for (h, keys, tag) in cases {
            assert_keys(&h, &keys);
            assert_eq!(serde_json::to_value(&h).unwrap()["state"], tag);
        }
        let from = HudState::from(&minutae_dictation::HudState::Copied {
            words: 2,
            reason: minutae_dictation::CopyReason::NoTextField,
        });
        assert!(matches!(from, HudState::Copied { words: 2, .. }));
    }

    #[test]
    fn search_shapes() {
        let hit = SearchHit {
            kind: HitKind::ActionItem,
            meeting_id: None,
            dictation_id: None,
            title: "t".into(),
            snippet: "s".into(),
            highlights: vec![[0, 1]],
            at_ms: None,
            date: "d".into(),
        };
        assert_keys(
            &hit,
            &[
                "kind",
                "meetingId",
                "dictationId",
                "title",
                "snippet",
                "highlights",
                "atMs",
                "date",
            ],
        );
        let v = serde_json::to_value(&hit).unwrap();
        assert_eq!(v["kind"], "actionItem");
        assert_eq!(v["highlights"], serde_json::json!([[0, 1]]));
        assert_keys(
            &Answer {
                text: "a".into(),
                sources: vec![hit],
                provider: "p".into(),
            },
            &["text", "sources", "provider"],
        );
    }

    #[test]
    fn settings_shape() {
        assert_keys(
            &AppSettings::default(),
            &[
                "appearance",
                "launchAtLogin",
                "autoDetectMeetings",
                "promptDelaySeconds",
                "stopWhenCallEnds",
                "includeSystemAudio",
                "microphone",
                "recordShortcut",
                "transcribeWhere",
                "whisperModel",
                "language",
                "labelSpeakers",
                "transportOrder",
                "apiFallbackEnabled",
                "summaryLength",
                "summariesPath",
                "codexModel",
                "ollamaModel",
                "dictationHotkey",
                "dictationMode",
                "tidyDictation",
                "keepDictationAudio",
                "vocabulary",
                "deleteAudioAfterDays",
                "recordingsPath",
            ],
        );
        let v = serde_json::to_value(AppSettings::default()).unwrap();
        assert_eq!(
            v["transportOrder"],
            serde_json::json!(["codexCli", "ollama", "grokCli"])
        );
    }

    #[test]
    fn update_status_shape() {
        let s = UpdateStatus {
            phase: UpdatePhase::UpToDate,
            current_version: "0.4.0".into(),
            version: None,
            notes: None,
            fraction: None,
            error: None,
        };
        assert_keys(
            &s,
            &[
                "phase",
                "currentVersion",
                "version",
                "notes",
                "fraction",
                "error",
            ],
        );
        assert_eq!(serde_json::to_value(&s).unwrap()["phase"], "upToDate");
        assert_eq!(ev::UPDATE_STATUS, "update://status");
    }

    #[test]
    fn platform_shapes() {
        assert_keys(
            &InputDevice {
                id: "i".into(),
                name: "n".into(),
                is_default: true,
            },
            &["id", "name", "isDefault"],
        );
        let p = Permissions {
            microphone: PermissionState::NotDetermined,
            system_audio: PermissionState::NotRequired,
            accessibility: PermissionState::Granted,
        };
        assert_keys(&p, &["microphone", "systemAudio", "accessibility"]);
        assert_eq!(
            serde_json::to_value(p).unwrap()["microphone"],
            "notDetermined"
        );
        let kind: PermissionKind = serde_json::from_value(Value::from("systemAudio")).unwrap();
        assert_eq!(kind, PermissionKind::SystemAudio);
        let info = PlatformInfo {
            os: Os::Macos,
            session_kind: Some(SessionKind::X11),
            app_version: "0".into(),
        };
        assert_keys(&info, &["os", "sessionKind", "appVersion"]);
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["os"], "macos");
        assert_eq!(v["sessionKind"], "x11");
    }

    #[test]
    fn model_and_provider_shapes() {
        let m = ModelInfo {
            model: WhisperModel::Base,
            size_bytes: 1,
            state: ModelState::Ready,
            fraction: None,
        };
        assert_keys(&m, &["model", "sizeBytes", "state", "fraction"]);
        assert_eq!(serde_json::to_value(&m).unwrap()["model"], "base");
        assert_keys(
            &DiarizeModelInfo {
                state: ModelState::Missing,
                fraction: None,
                size_bytes: 0,
            },
            &["state", "fraction", "sizeBytes"],
        );
        assert_keys(
            &ModelProgress {
                model: "diarize".into(),
                fraction: 0.0,
                done: false,
                error: None,
            },
            &["model", "fraction", "done", "error"],
        );
        let p = ProviderStatus {
            transport: minutae_core::Transport::CodexCli,
            name: "Codex".into(),
            availability: Availability::NotLoggedIn,
            detail: None,
            model: None,
        };
        assert_keys(
            &p,
            &["transport", "name", "availability", "detail", "model"],
        );
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["transport"], "codexCli");
        assert_eq!(v["availability"], "notLoggedIn");
        let a: ApiProvider = serde_json::from_value(Value::from("openai")).unwrap();
        assert_eq!(a, ApiProvider::Openai);
    }

    #[test]
    fn usage_and_storage_shapes() {
        let u = UsageMonth {
            year: 2026,
            month: 11,
            total_usd: 0.0,
            summaries: 0,
            tokens: 0,
            free_share: 1.0,
            by_provider: vec![UsageByProvider {
                provider: "p".into(),
                calls: 1,
                tokens: 1,
                cost_usd: 0.0,
            }],
            by_day: vec![UsageDay {
                day: 1,
                cost_usd: 0.0,
            }],
        };
        assert_keys(
            &u,
            &[
                "year",
                "month",
                "totalUsd",
                "summaries",
                "tokens",
                "freeShare",
                "byProvider",
                "byDay",
            ],
        );
        assert_keys(
            &u.by_provider[0],
            &["provider", "calls", "tokens", "costUsd"],
        );
        assert_keys(&u.by_day[0], &["day", "costUsd"]);
        assert_keys(
            &StorageUsage {
                recordings_bytes: 0,
                models_bytes: 0,
                index_bytes: 0,
                data_dir: "d".into(),
            },
            &["recordingsBytes", "modelsBytes", "indexBytes", "dataDir"],
        );
    }
}
