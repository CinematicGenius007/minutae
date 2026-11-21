//! WAV in/out at 16 kHz mono (the recorder's format: 16-bit PCM, see minutae-audio).

use std::path::Path;

use crate::{AecError, Result, SAMPLE_RATE};

/// Reads any PCM/float WAV as 16 kHz mono f32 (downmixed, linearly resampled).
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| AecError::Audio(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()
            .map_err(|e| AecError::Audio(e.to_string()))?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()
                .map_err(|e| AecError::Audio(e.to_string()))?
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
    if spec.sample_rate == SAMPLE_RATE || mono.is_empty() {
        return Ok(mono);
    }
    let ratio = spec.sample_rate as f64 / SAMPLE_RATE as f64;
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

/// Writes 16 kHz mono 16-bit PCM (same format as `mic.wav`), via a temp file + rename
/// so a crash never leaves a half-written track behind.
pub fn write_wav_16k_mono(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tmp = path.with_extension("wav.part");
    {
        let mut w = hound::WavWriter::create(&tmp, spec)
            .map_err(|e| AecError::Audio(format!("{}: {e}", tmp.display())))?;
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            w.write_sample(v)
                .map_err(|e| AecError::Audio(e.to_string()))?;
        }
        w.finalize().map_err(|e| AecError::Audio(e.to_string()))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Streaming 16 kHz mono 16-bit writer to `<path>.part`, renamed into place by
/// [`Writer::finish`] so readers never see a half-written track.
pub(crate) struct Writer {
    inner: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
    tmp: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl Writer {
    pub(crate) fn create(path: &Path) -> Result<Self> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let tmp = path.with_extension("wav.part");
        let inner = hound::WavWriter::create(&tmp, spec)
            .map_err(|e| AecError::Audio(format!("{}: {e}", tmp.display())))?;
        Ok(Self {
            inner: Some(inner),
            tmp,
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn write(&mut self, samples: &[f32]) -> Result<()> {
        let w = self.inner.as_mut().expect("writer open");
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            w.write_sample(v)
                .map_err(|e| AecError::Audio(e.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        if let Some(w) = self.inner.take() {
            w.finalize().map_err(|e| AecError::Audio(e.to_string()))?;
        }
        std::fs::rename(&self.tmp, &self.path)?;
        Ok(())
    }

    pub(crate) fn discard(mut self) {
        drop(self.inner.take());
        let _ = std::fs::remove_file(&self.tmp);
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if self.inner.take().is_some() {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}
