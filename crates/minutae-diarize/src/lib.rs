//! minutae-diarize — "who spoke when" with sherpa-onnx (pyannote segmentation 3.0 +
//! 3D-Speaker CAM++ embeddings), plus the pure [`merge`] that attributes transcript
//! segments to speakers (docs/CROSS-PLATFORM.md, "Multi-speaker plan").
//!
//! Diarization is blocking; call it from `spawn_blocking` / a worker thread.

mod audio;
mod merge;
mod models;
mod sherpa;

use std::path::Path;

use minutae_core::{Failure, FailureKind};
use serde::{Deserialize, Serialize};

pub use audio::read_wav_16k_mono;
pub use merge::{MAX_GAP_MS, YOU_LABEL, merge, merge_single_track};
pub use models::{
    DiarizeModelStatus, DiarizeModels, DownloadProgress, EMBEDDING_FILE, EMBEDDING_URL,
    SEGMENTATION_FILE, SEGMENTATION_URL,
};
pub use sherpa::SherpaDiarizer;

/// One stretch of speech by one (anonymous) speaker. `speaker` ids are arbitrary
/// per call; [`merge`] turns them into stable "Speaker N" labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerTurn {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    #[error("speaker models are not downloaded")]
    ModelMissing,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("download failed: {0}")]
    Download(String),
    #[error("audio: {0}")]
    Audio(String),
    #[error("diarization: {0}")]
    Engine(String),
}

impl DiarizeError {
    pub fn to_failure(&self) -> Failure {
        let (kind, message, fix) = match self {
            DiarizeError::ModelMissing => (
                FailureKind::ModelMissing,
                "The speaker-labelling models aren't downloaded.".to_string(),
                Some("Download models".to_string()),
            ),
            DiarizeError::Download(_) => (
                FailureKind::Unavailable,
                "The model download didn't finish.".to_string(),
                Some("Try again".to_string()),
            ),
            DiarizeError::Io(_) | DiarizeError::Audio(_) => (
                FailureKind::Io,
                "The recording couldn't be read.".to_string(),
                None,
            ),
            DiarizeError::Engine(_) => (
                FailureKind::Internal,
                "Speaker labelling failed.".to_string(),
                None,
            ),
        };
        Failure { kind, message, fix }
    }
}

pub type Result<T, E = DiarizeError> = std::result::Result<T, E>;

pub trait Diarizer: Send + Sync {
    /// Diarize 16 kHz mono samples. `num_speakers`: exact count if known, else clustered by threshold.
    fn diarize_samples(
        &self,
        samples: &[f32],
        num_speakers: Option<u32>,
    ) -> Result<Vec<SpeakerTurn>>;

    /// Diarize a WAV file (converted to 16 kHz mono).
    fn diarize(&self, wav_path: &Path, num_speakers: Option<u32>) -> Result<Vec<SpeakerTurn>> {
        let samples = read_wav_16k_mono(wav_path)?;
        self.diarize_samples(&samples, num_speakers)
    }
}

/// Returns fixed turns; for tests of code that consumes a [`Diarizer`].
#[derive(Debug, Clone, Default)]
pub struct FakeDiarizer {
    pub turns: Vec<SpeakerTurn>,
}

impl Diarizer for FakeDiarizer {
    fn diarize_samples(
        &self,
        samples: &[f32],
        _num_speakers: Option<u32>,
    ) -> Result<Vec<SpeakerTurn>> {
        let len_ms = samples.len() as u64 * 1000 / 16_000;
        Ok(self
            .turns
            .iter()
            .filter(|t| t.start_ms < len_ms)
            .copied()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_is_camel_case() {
        let v = serde_json::to_value(SpeakerTurn {
            start_ms: 1,
            end_ms: 2,
            speaker: 0,
        })
        .unwrap();
        assert!(v.get("startMs").is_some());
    }

    #[test]
    fn fake_diarizer_clips() {
        let f = FakeDiarizer {
            turns: vec![
                SpeakerTurn {
                    start_ms: 0,
                    end_ms: 500,
                    speaker: 0,
                },
                SpeakerTurn {
                    start_ms: 2_000,
                    end_ms: 3_000,
                    speaker: 1,
                },
            ],
        };
        assert_eq!(f.diarize_samples(&[0.0; 16_000], None).unwrap().len(), 1);
    }

    #[test]
    fn failures() {
        assert_eq!(
            DiarizeError::ModelMissing.to_failure().kind,
            FailureKind::ModelMissing
        );
    }
}
