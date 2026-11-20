//! WAV → 16 kHz mono f32 (kept local so this crate doesn't pull in whisper.cpp).

use std::path::Path;

use crate::{DiarizeError, Result};

pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| DiarizeError::Audio(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| DiarizeError::Audio(e.to_string()))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| DiarizeError::Audio(e.to_string()))?
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
    if spec.sample_rate == 16_000 || mono.is_empty() {
        return Ok(mono);
    }
    let ratio = spec.sample_rate as f64 / 16_000.0;
    let out_len = (mono.len() as f64 / ratio).floor() as usize;
    Ok((0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono[idx.min(mono.len() - 1)];
            let b = mono[(idx + 1).min(mono.len() - 1)];
            a + (b - a) * frac
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_resamples() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 32_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p, spec).unwrap();
        for _ in 0..3200 {
            w.write_sample(8192i16).unwrap();
        }
        w.finalize().unwrap();
        let s = read_wav_16k_mono(&p).unwrap();
        assert_eq!(s.len(), 1600);
        assert!((s[0] - 0.25).abs() < 1e-4);
    }
}
