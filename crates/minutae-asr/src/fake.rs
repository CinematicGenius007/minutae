//! Deterministic transcriber for tests and UI work without a model.

use std::sync::Mutex;

use minutae_core::{Segment, Track};

use crate::{ProgressFn, Result, TranscribeOptions, Transcriber};

/// Returns canned `(start_ms, end_ms, text)` segments (clipped to the input length)
/// and records how many times it was called.
pub struct FakeTranscriber {
    segments: Vec<(u64, u64, String)>,
    calls: Mutex<usize>,
}

impl FakeTranscriber {
    pub fn new(segments: Vec<(u64, u64, String)>) -> Self {
        Self {
            segments,
            calls: Mutex::new(0),
        }
    }

    pub fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl Default for FakeTranscriber {
    fn default() -> Self {
        Self::new(vec![(0, 1_000, "Hello from the fake transcriber.".into())])
    }
}

impl Transcriber for FakeTranscriber {
    fn model_name(&self) -> String {
        "fake".into()
    }

    fn transcribe_samples(
        &self,
        samples: &[f32],
        track: Track,
        _opts: &TranscribeOptions,
        progress: Option<ProgressFn>,
    ) -> Result<Vec<Segment>> {
        *self.calls.lock().unwrap() += 1;
        let len_ms = samples.len() as u64 * 1000 / crate::TARGET_SAMPLE_RATE as u64;
        if let Some(p) = &progress {
            p(1.0);
        }
        Ok(self
            .segments
            .iter()
            .filter(|(s, _, _)| *s < len_ms.max(1))
            .map(|(s, e, t)| Segment {
                start_ms: *s,
                end_ms: (*e).min(len_ms.max(*s)),
                text: t.clone(),
                track,
                speaker_id: None,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clips_to_length_and_counts_calls() {
        let f = FakeTranscriber::new(vec![
            (0, 500, "a".into()),
            (900, 1500, "b".into()),
            (5000, 6000, "c".into()),
        ]);
        let out = f
            .transcribe_samples(
                &vec![0.0; 16_000],
                Track::System,
                &TranscribeOptions::default(),
                None,
            )
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].end_ms, 1000);
        assert_eq!(out[0].track, Track::System);
        assert_eq!(
            f.transcribe_text(&vec![0.0; 16_000], &TranscribeOptions::default())
                .unwrap(),
            "a b"
        );
        assert_eq!(f.calls(), 2);
    }
}
