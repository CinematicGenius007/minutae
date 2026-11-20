//! WAV loading: any PCM/float WAV → 16 kHz mono f32.

use std::path::Path;

use crate::{AsrError, Result};

/// Whisper's (and the diarizer's) expected input rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Reads a WAV file and returns 16 kHz mono samples in `-1.0..=1.0`.
/// Multi-channel input is averaged; other rates are linearly resampled
/// (recordings should already be 16 kHz, this is only a safety net).
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| AsrError::Audio(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| AsrError::Audio(e.to_string()))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| AsrError::Audio(e.to_string()))?
        }
    };
    let mono: Vec<f32> = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks(channels)
            .map(|f| f.iter().sum::<f32>() / f.len() as f32)
            .collect()
    };
    Ok(resample_linear(&mono, spec.sample_rate, TARGET_SAMPLE_RATE))
}

pub(crate) fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx.min(input.len() - 1)];
            let b = input[(idx + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, channels: u16, frames: &[i16]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for s in frames {
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn reads_16k_mono_as_is() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, 16_000, 1, &[0, 16384, -16384, i16::MAX]);
        let s = read_wav_16k_mono(&p).unwrap();
        assert_eq!(s.len(), 4);
        assert!((s[1] - 0.5).abs() < 1e-4 && (s[2] + 0.5).abs() < 1e-4);
    }

    #[test]
    fn downmixes_and_resamples() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.wav");
        // 48 kHz stereo, 0.1 s; L=+0.5, R=-0.5 → mono 0.
        let frames: Vec<i16> = (0..4800).flat_map(|_| [16384i16, -16384]).collect();
        write_wav(&p, 48_000, 2, &frames);
        let s = read_wav_16k_mono(&p).unwrap();
        assert_eq!(s.len(), 1600);
        assert!(s.iter().all(|v| v.abs() < 1e-4));
    }

    #[test]
    fn missing_file_is_audio_error() {
        assert!(matches!(
            read_wav_16k_mono(Path::new("/nonexistent.wav")),
            Err(AsrError::Audio(_))
        ));
    }
}
