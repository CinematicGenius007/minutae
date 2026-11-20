//! minutae-asr — local speech-to-text with whisper.cpp (docs/CROSS-PLATFORM.md §3).
//!
//! - [`ModelManager`] downloads / inspects / deletes `ggml-*.bin` models.
//! - [`Transcriber`] is the engine trait; [`WhisperTranscriber`] is the real one and
//!   [`FakeTranscriber`] returns canned segments for tests.
//!
//! All transcription calls are blocking (CPU/GPU bound). Call them from
//! `tokio::task::spawn_blocking` or a dedicated worker thread.

mod audio;
mod fake;
mod models;
mod whisper;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub use audio::{TARGET_SAMPLE_RATE, read_wav_16k_mono};
pub use fake::FakeTranscriber;
use minutae_core::{Failure, FailureKind, Segment, Track};
pub use models::{DownloadProgress, ModelInfo, ModelManager, ModelStatus, model_info};
pub use whisper::WhisperTranscriber;

/// Progress callback: fraction in `0.0..=1.0`.
pub type ProgressFn = Arc<dyn Fn(f32) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum AsrError {
    #[error("the {0} speech model is not downloaded")]
    ModelMissing(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("download failed: {0}")]
    Download(String),
    #[error("audio: {0}")]
    Audio(String),
    #[error("whisper: {0}")]
    Whisper(String),
    #[error("cancelled")]
    Cancelled,
}

impl AsrError {
    /// Plain-language failure for the UI.
    pub fn to_failure(&self) -> Failure {
        let (kind, message, fix) = match self {
            AsrError::ModelMissing(m) => (
                FailureKind::ModelMissing,
                format!("The {m} speech model isn't downloaded."),
                Some("Download model".to_string()),
            ),
            AsrError::Download(_) => (
                FailureKind::Unavailable,
                "The model download didn't finish.".to_string(),
                Some("Try again".to_string()),
            ),
            AsrError::Cancelled => (
                FailureKind::Cancelled,
                "Transcription was cancelled.".to_string(),
                None,
            ),
            AsrError::Io(_) | AsrError::Audio(_) => (
                FailureKind::Io,
                "The recording couldn't be read.".to_string(),
                None,
            ),
            AsrError::Whisper(_) => (
                FailureKind::Internal,
                "Transcription failed.".to_string(),
                None,
            ),
        };
        Failure { kind, message, fix }
    }
}

pub type Result<T, E = AsrError> = std::result::Result<T, E>;

/// Per-call options.
#[derive(Clone, Default)]
pub struct TranscribeOptions {
    /// ISO-639-1 code (`"en"`, `"de"`…). `None` = auto-detect.
    pub language: Option<String>,
    /// Text that primes the decoder; use it for custom vocabulary / names.
    pub initial_prompt: Option<String>,
    /// CPU threads. `None` = min(available cores, 8).
    pub threads: Option<u32>,
    /// Beam search width. `None` = greedy (faster; good for dictation).
    pub beam_size: Option<u32>,
    /// Set to `true` from another thread to abort a running transcription.
    pub cancel: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for TranscribeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscribeOptions")
            .field("language", &self.language)
            .field("initial_prompt", &self.initial_prompt)
            .field("threads", &self.threads)
            .field("beam_size", &self.beam_size)
            .field("cancellable", &self.cancel.is_some())
            .finish()
    }
}

impl TranscribeOptions {
    /// Builds an `initial_prompt` from the user's vocabulary list (settings.vocabulary).
    pub fn with_vocabulary(mut self, words: &[String]) -> Self {
        let words: Vec<&str> = words
            .iter()
            .map(|w| w.trim())
            .filter(|w| !w.is_empty())
            .collect();
        if !words.is_empty() {
            self.initial_prompt = Some(format!("Vocabulary: {}.", words.join(", ")));
        }
        self
    }
}

/// A speech-to-text engine. Implementations must be safe to share across threads.
pub trait Transcriber: Send + Sync {
    /// Human-readable model id stored in `Transcript.model`, e.g. `"whisper-base"`.
    fn model_name(&self) -> String;

    /// Transcribe 16 kHz mono f32 samples. Timestamps are relative to `samples[0]`.
    fn transcribe_samples(
        &self,
        samples: &[f32],
        track: Track,
        opts: &TranscribeOptions,
        progress: Option<ProgressFn>,
    ) -> Result<Vec<Segment>>;

    /// Transcribe a WAV file (any rate/channels; converted to 16 kHz mono).
    fn transcribe(
        &self,
        wav_path: &Path,
        track: Track,
        opts: &TranscribeOptions,
        progress: Option<ProgressFn>,
    ) -> Result<Vec<Segment>> {
        let samples = read_wav_16k_mono(wav_path)?;
        self.transcribe_samples(&samples, track, opts, progress)
    }

    /// Convenience for dictation: plain text of a short clip.
    fn transcribe_text(&self, samples: &[f32], opts: &TranscribeOptions) -> Result<String> {
        Ok(join_text(&self.transcribe_samples(
            samples,
            Track::Mic,
            opts,
            None,
        )?))
    }
}

/// Joins segment texts with single spaces.
pub fn join_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| s.text.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_builds_prompt() {
        let o = TranscribeOptions::default().with_vocabulary(&[
            "Minutae".into(),
            " ".into(),
            "Tauri".into(),
        ]);
        assert_eq!(
            o.initial_prompt.as_deref(),
            Some("Vocabulary: Minutae, Tauri.")
        );
        let o = TranscribeOptions::default().with_vocabulary(&[]);
        assert!(o.initial_prompt.is_none());
    }

    #[test]
    fn join_text_skips_blanks() {
        let seg = |t: &str| Segment {
            start_ms: 0,
            end_ms: 1,
            text: t.into(),
            track: Track::Mic,
            speaker_id: None,
        };
        assert_eq!(
            join_text(&[seg(" Hello"), seg(""), seg("world ")]),
            "Hello world"
        );
    }

    #[test]
    fn failure_mapping() {
        assert_eq!(
            AsrError::ModelMissing("base".into()).to_failure().kind,
            FailureKind::ModelMissing
        );
        assert_eq!(
            AsrError::Cancelled.to_failure().kind,
            FailureKind::Cancelled
        );
    }
}
