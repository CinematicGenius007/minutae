//! Typed app settings. Mirrors the Settings screens (Paper 06a–06f).
//! Secrets (API keys) are never stored here; they live in the OS keychain.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TranscribeWhere {
    #[default]
    Local,
    Cloud,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum WhisperModel {
    Tiny,
    Base,
    #[default]
    Small,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    CodexCli,
    GrokCli,
    Ollama,
    Api,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SummaryLength {
    #[default]
    Brief,
    Detailed,
}

/// Remove the other people's voices from the mic track when they came out of the
/// speakers (no headphones). `Auto` runs it only when the speakers are heard in the mic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EchoCancelMode {
    #[default]
    Auto,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyMode {
    #[default]
    Hold,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    // General
    pub appearance: Appearance,
    pub launch_at_login: bool,
    pub auto_detect_meetings: bool,
    pub prompt_delay_seconds: u32,
    pub stop_when_call_ends: bool,
    pub include_system_audio: bool,
    /// None = follow the system default input.
    pub microphone: Option<String>,
    /// e.g. "Ctrl+Alt+R"
    pub record_shortcut: String,
    // Transcription
    pub transcribe_where: TranscribeWhere,
    pub whisper_model: WhisperModel,
    /// None = detect automatically.
    pub language: Option<String>,
    pub label_speakers: bool,
    /// Live captions while recording (needs a downloaded Whisper model; best effort).
    pub live_captions: bool,
    pub echo_cancellation: EchoCancelMode,
    // Summaries
    pub transport_order: Vec<Transport>,
    pub api_fallback_enabled: bool,
    pub summary_length: SummaryLength,
    pub summaries_path: Option<String>,
    pub codex_model: Option<String>,
    pub ollama_model: String,
    // Dictation
    /// e.g. "Alt+Space" (macOS ⌥Space) / "Ctrl+Space"
    pub dictation_hotkey: String,
    pub dictation_mode: HotkeyMode,
    pub tidy_dictation: bool,
    pub keep_dictation_audio: bool,
    /// None = keep dictations and their audio forever.
    pub dictation_retention_days: Option<u32>,
    pub vocabulary: Vec<String>,
    // Storage
    /// None = never delete audio.
    pub delete_audio_after_days: Option<u32>,
    pub recordings_path: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            launch_at_login: true,
            auto_detect_meetings: true,
            prompt_delay_seconds: 5,
            stop_when_call_ends: true,
            include_system_audio: true,
            microphone: None,
            record_shortcut: "CommandOrControl+Alt+R".into(),
            transcribe_where: TranscribeWhere::Local,
            whisper_model: WhisperModel::Small,
            language: None,
            label_speakers: true,
            live_captions: true,
            echo_cancellation: EchoCancelMode::Auto,
            transport_order: vec![Transport::CodexCli, Transport::Ollama, Transport::GrokCli],
            api_fallback_enabled: false,
            summary_length: SummaryLength::Brief,
            summaries_path: None,
            codex_model: None,
            ollama_model: "llama3.1:8b".into(),
            dictation_hotkey: if cfg!(target_os = "macos") {
                "Alt+Space".into()
            } else {
                "Control+Space".into()
            },
            dictation_mode: HotkeyMode::Hold,
            tidy_dictation: true,
            keep_dictation_audio: false,
            dictation_retention_days: None,
            vocabulary: vec![],
            delete_audio_after_days: Some(30),
            recordings_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let s: AppSettings = serde_json::from_str(r#"{"appearance":"dark"}"#).unwrap();
        assert_eq!(s.appearance, Appearance::Dark);
        assert_eq!(s.dictation_retention_days, None);
        assert_eq!(s.prompt_delay_seconds, 5);
        assert_eq!(s.echo_cancellation, EchoCancelMode::Auto);
    }

    #[test]
    fn echo_cancellation_wire_values() {
        let s: AppSettings = serde_json::from_str(r#"{"echoCancellation":"off"}"#).unwrap();
        assert_eq!(s.echo_cancellation, EchoCancelMode::Off);
        let v = serde_json::to_value(AppSettings::default()).unwrap();
        assert_eq!(v["echoCancellation"], "auto");
    }
}
