//! Seams to the audio and speech crates. The app wires the real
//! implementations (minutae-audio, minutae-asr); tests use `crate::fakes`.

use std::sync::{Arc, atomic::AtomicBool};

use async_trait::async_trait;
use tokio::sync::watch;

/// Sample rate the controller expects from [`AudioSource::stop`].
pub const SAMPLE_RATE: u32 = 16_000;

/// Microphone capture for one dictation at a time.
pub trait AudioSource: Send + Sync {
    /// Opens the microphone. Called on key-down; must return quickly.
    fn start(&self) -> anyhow::Result<()>;
    /// Closes the microphone and returns everything captured since `start`,
    /// as 16 kHz mono f32 samples.
    fn stop(&self) -> anyhow::Result<Vec<f32>>;
    /// Input level in 0.0..=1.0 for the HUD meter. The controller reads it
    /// only while listening, so the source may publish all the time.
    fn levels(&self) -> watch::Receiver<f32>;
}

/// Speech-to-text for a short clip. Heavy work should run off the async
/// executor (e.g. `spawn_blocking`) inside the implementation.
#[async_trait]
pub trait SpeechToText: Send + Sync {
    /// `vocabulary` is the user's word list, for use as a prompt/bias.
    /// Stop the transcription when `cancel` is set.
    async fn transcribe(
        &self,
        samples: &[f32],
        vocabulary: &[String],
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<String>;
}
