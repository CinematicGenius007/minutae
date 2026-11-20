//! sherpa-onnx offline speaker diarization (pyannote segmentation + speaker embeddings
//! + agglomerative clustering), CPU via onnxruntime.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::{DiarizeError, DiarizeModels, Diarizer, Result, SpeakerTurn};

/// Default clustering threshold (cosine distance). Smaller → more speakers.
pub const DEFAULT_THRESHOLD: f32 = 0.5;

pub struct SherpaDiarizer {
    segmentation: PathBuf,
    embedding: PathBuf,
    threads: i32,
    threshold: f32,
    /// Engine is created lazily and cached; the mutex also serialises `set_config`.
    engine: Mutex<Option<OfflineSpeakerDiarization>>,
}

impl SherpaDiarizer {
    /// Uses the downloaded models; fails with `ModelMissing` if they aren't there.
    pub fn new(models: &DiarizeModels) -> Result<Self> {
        if !models.is_ready() {
            return Err(DiarizeError::ModelMissing);
        }
        Ok(Self::with_paths(
            models.segmentation_path(),
            models.embedding_path(),
        ))
    }

    pub fn with_paths(
        segmentation_onnx: impl Into<PathBuf>,
        embedding_onnx: impl Into<PathBuf>,
    ) -> Self {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2) as i32;
        Self {
            segmentation: segmentation_onnx.into(),
            embedding: embedding_onnx.into(),
            threads,
            threshold: DEFAULT_THRESHOLD,
            engine: Mutex::new(None),
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    pub fn with_threads(mut self, threads: u32) -> Self {
        self.threads = threads.max(1) as i32;
        self
    }

    fn config(&self, num_speakers: Option<u32>) -> OfflineSpeakerDiarizationConfig {
        let path = |p: &Path| Some(p.to_string_lossy().into_owned());
        OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: path(&self.segmentation),
                    ..Default::default()
                },
                num_threads: self.threads,
                ..Default::default()
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: path(&self.embedding),
                num_threads: self.threads,
                ..Default::default()
            },
            clustering: FastClusteringConfig {
                num_clusters: num_speakers
                    .filter(|n| *n > 0)
                    .map(|n| n as i32)
                    .unwrap_or(-1),
                threshold: self.threshold,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize_samples(
        &self,
        samples: &[f32],
        num_speakers: Option<u32>,
    ) -> Result<Vec<SpeakerTurn>> {
        // pyannote works on 10 s windows; anything shorter than ~0.5 s has no turns.
        if samples.len() < 8_000 {
            return Ok(Vec::new());
        }
        let config = self.config(num_speakers);
        let mut guard = self
            .engine
            .lock()
            .map_err(|_| DiarizeError::Engine("engine lock poisoned".into()))?;
        match guard.as_ref() {
            Some(engine) => engine.set_config(&config),
            None => {
                for p in [&self.segmentation, &self.embedding] {
                    if !p.exists() {
                        return Err(DiarizeError::ModelMissing);
                    }
                }
                let engine = OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
                    DiarizeError::Engine("could not load the diarization models".into())
                })?;
                if engine.sample_rate() != 16_000 {
                    return Err(DiarizeError::Engine(format!(
                        "unexpected model sample rate {}",
                        engine.sample_rate()
                    )));
                }
                *guard = Some(engine);
            }
        }
        let engine = guard.as_ref().expect("engine initialised above");
        let result = engine
            .process(samples)
            .ok_or_else(|| DiarizeError::Engine("diarization returned no result".into()))?;
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .filter(|s| s.end > s.start && s.speaker >= 0)
            .map(|s| SpeakerTurn {
                start_ms: (s.start.max(0.0) * 1000.0).round() as u64,
                end_ms: (s.end.max(0.0) * 1000.0).round() as u64,
                speaker: s.speaker as u32,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_models() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            SherpaDiarizer::new(&DiarizeModels::with_models_dir(dir.path())),
            Err(DiarizeError::ModelMissing)
        ));
        let d = SherpaDiarizer::with_paths(dir.path().join("a.onnx"), dir.path().join("b.onnx"));
        assert!(matches!(
            d.diarize_samples(&[0.0; 16_000], None),
            Err(DiarizeError::ModelMissing)
        ));
        assert!(d.diarize_samples(&[0.0; 100], None).unwrap().is_empty());
    }

    #[test]
    fn clustering_config() {
        let d = SherpaDiarizer::with_paths("s.onnx", "e.onnx").with_threshold(0.7);
        assert_eq!(d.config(None).clustering.num_clusters, -1);
        assert_eq!(d.config(Some(3)).clustering.num_clusters, 3);
        assert_eq!(d.config(Some(0)).clustering.num_clusters, -1);
        assert_eq!(d.config(None).clustering.threshold, 0.7);
    }
}
