//! Crash-safe chunked WAV writer, tolerant reader, and stitching.
//!
//! Layout inside a recording's `out_dir`:
//! ```text
//! chunks/mic-000000.wav      10 s each, 16 kHz mono i16
//! chunks/mic-000001.wav      (the open chunk; header refreshed every second)
//! chunks/system-000000.wav
//! mic.wav / system.wav       written on stop (or by `recover`)
//! ```

use std::fs::{self, File};
use std::io::{BufWriter, Read};
use std::path::{Path, PathBuf};

use minutae_core::Track;

use crate::{AudioError, Result};

pub const SAMPLE_RATE: u32 = 16_000;
pub const CHUNKS_DIR: &str = "chunks";

pub fn spec() -> hound::WavSpec {
    hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    }
}

pub fn track_name(track: Track) -> &'static str {
    match track {
        Track::Mic => "mic",
        Track::System => "system",
    }
}

pub fn final_path(out_dir: &Path, track: Track) -> PathBuf {
    out_dir.join(format!("{}.wav", track_name(track)))
}

pub fn f32_to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Writes one track as a sequence of fixed-length WAV chunks.
pub struct ChunkWriter {
    dir: PathBuf,
    track: Track,
    chunk_samples: u64,
    /// Refresh the WAV header after this many samples (limits loss in the open chunk).
    flush_every: u64,
    seq: u32,
    in_chunk: u64,
    since_flush: u64,
    total: u64,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
}

impl ChunkWriter {
    pub fn new(out_dir: &Path, track: Track, chunk_secs: u32) -> Result<Self> {
        let dir = out_dir.join(CHUNKS_DIR);
        fs::create_dir_all(&dir)?;
        // Continue after any existing chunks so nothing is overwritten.
        let seq = list_chunks(&dir, track)?
            .last()
            .map(|(s, _)| s + 1)
            .unwrap_or(0);
        Ok(Self {
            dir,
            track,
            chunk_samples: chunk_secs.max(1) as u64 * SAMPLE_RATE as u64,
            flush_every: SAMPLE_RATE as u64,
            seq,
            in_chunk: 0,
            since_flush: 0,
            total: 0,
            writer: None,
        })
    }

    pub fn total_samples(&self) -> u64 {
        self.total
    }

    pub fn write(&mut self, samples: &[f32]) -> Result<()> {
        for &s in samples {
            if self.writer.is_none() {
                let path = chunk_path(&self.dir, self.track, self.seq);
                self.writer = Some(hound::WavWriter::create(path, spec())?);
            }
            let w = self.writer.as_mut().expect("writer open");
            w.write_sample(f32_to_i16(s))?;
            self.in_chunk += 1;
            self.since_flush += 1;
            self.total += 1;
            if self.in_chunk >= self.chunk_samples {
                self.close_chunk()?;
            } else if self.since_flush >= self.flush_every {
                w.flush()?;
                self.since_flush = 0;
            }
        }
        Ok(())
    }

    pub fn write_silence(&mut self, n: u64) -> Result<()> {
        const BLOCK: usize = 1600;
        let zeros = [0.0f32; BLOCK];
        let mut left = n;
        while left > 0 {
            let k = left.min(BLOCK as u64) as usize;
            self.write(&zeros[..k])?;
            left -= k as u64;
        }
        Ok(())
    }

    fn close_chunk(&mut self) -> Result<()> {
        if let Some(w) = self.writer.take() {
            w.finalize()?;
            self.seq += 1;
        }
        self.in_chunk = 0;
        self.since_flush = 0;
        Ok(())
    }

    /// Close the open chunk. Chunks stay on disk until [`stitch`] runs.
    pub fn finish(mut self) -> Result<u64> {
        self.close_chunk()?;
        Ok(self.total)
    }
}

fn chunk_path(dir: &Path, track: Track, seq: u32) -> PathBuf {
    dir.join(format!("{}-{seq:06}.wav", track_name(track)))
}

/// Chunks for `track` in `dir`, sorted by sequence number.
pub fn list_chunks(dir: &Path, track: Track) -> Result<Vec<(u32, PathBuf)>> {
    let prefix = format!("{}-", track_name(track));
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(seq) = name
            .strip_prefix(&prefix)
            .and_then(|r| r.strip_suffix(".wav"))
            .and_then(|r| r.parse::<u32>().ok())
        {
            out.push((seq, path));
        }
    }
    out.sort_by_key(|(s, _)| *s);
    Ok(out)
}

/// Read 16-bit PCM samples from a WAV file, tolerating a header whose sizes
/// were never finalised (the chunk that was open during a crash).
pub fn read_pcm16_tolerant(path: &Path) -> Result<Vec<i16>> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioError::Corrupt(path.to_path_buf()));
    }
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"data" {
            // Trust the declared size only if it fits; otherwise take the rest.
            let end = if size > 0 && body + size <= bytes.len() {
                body + size
            } else {
                bytes.len()
            };
            let data = &bytes[body..end];
            return Ok(data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect());
        }
        pos = body + size + (size & 1);
    }
    // Header written but no data chunk yet: an empty chunk.
    Ok(Vec::new())
}

/// Concatenate all chunks of `track` into `out_dir/{track}.wav`, then delete
/// the chunks. Returns the final path and sample count, or `None` if there
/// were no chunks. Unreadable chunks are skipped (logged), not fatal.
pub fn stitch(out_dir: &Path, track: Track) -> Result<Option<(PathBuf, u64)>> {
    let dir = out_dir.join(CHUNKS_DIR);
    let chunks = list_chunks(&dir, track)?;
    if chunks.is_empty() {
        return Ok(None);
    }
    let final_path = final_path(out_dir, track);
    let tmp = final_path.with_extension("wav.tmp");
    let total;
    {
        let mut w = hound::WavWriter::create(&tmp, spec())?;
        // If a previous stitch left a final file (e.g. crash during cleanup), keep it first.
        if final_path.exists() {
            for s in read_pcm16_tolerant(&final_path)? {
                w.write_sample(s)?;
            }
        }
        for (_, path) in &chunks {
            match read_pcm16_tolerant(path) {
                Ok(samples) => {
                    for s in samples {
                        w.write_sample(s)?;
                    }
                }
                Err(e) => tracing::warn!(?path, error = %e, "skipping unreadable chunk"),
            }
        }
        total = w.len() as u64;
        w.finalize()?;
    }
    fs::rename(&tmp, &final_path)?;
    for (_, path) in &chunks {
        let _ = fs::remove_file(path);
    }
    let _ = fs::remove_dir(&dir); // only succeeds when empty
    Ok(Some((final_path, total)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "minutae-audio-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn ramp(n: usize) -> Vec<f32> {
        (0..n).map(|i| ((i % 1000) as f32 / 1000.0) - 0.5).collect()
    }

    #[test]
    fn writes_fixed_length_chunks() {
        let d = tmpdir("chunks");
        let mut w = ChunkWriter::new(&d, Track::Mic, 1).unwrap();
        w.write(&ramp(16_000 * 3 + 500)).unwrap();
        assert_eq!(w.finish().unwrap(), 16_000 * 3 + 500);
        let chunks = list_chunks(&d.join(CHUNKS_DIR), Track::Mic).unwrap();
        assert_eq!(chunks.len(), 4);
        let lens: Vec<_> = chunks
            .iter()
            .map(|(_, p)| hound::WavReader::open(p).unwrap().len())
            .collect();
        assert_eq!(lens, vec![16_000, 16_000, 16_000, 500]);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn stitch_concatenates_in_order() {
        let d = tmpdir("stitch");
        let input = ramp(16_000 * 2 + 123);
        let mut w = ChunkWriter::new(&d, Track::System, 1).unwrap();
        for c in input.chunks(777) {
            w.write(c).unwrap();
        }
        w.finish().unwrap();
        let (path, n) = stitch(&d, Track::System).unwrap().unwrap();
        assert_eq!(path, d.join("system.wav"));
        assert_eq!(n, input.len() as u64);
        let mut r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec(), spec());
        let got: Vec<i16> = r.samples::<i16>().map(|s| s.unwrap()).collect();
        let want: Vec<i16> = input.iter().map(|&x| f32_to_i16(x)).collect();
        assert_eq!(got, want);
        assert!(!d.join(CHUNKS_DIR).exists(), "chunks cleaned up");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn recovers_unfinalised_chunk() {
        let d = tmpdir("recover");
        let input = ramp(16_000 + 8_000);
        let mut w = ChunkWriter::new(&d, Track::Mic, 1).unwrap();
        w.write(&input).unwrap();
        // Simulate a crash: the open chunk's writer is never finalised.
        // (hound refreshed the header once per second; write the last bytes raw.)
        std::mem::forget(w);
        // Also simulate a header with zero sizes (crash before the first flush).
        let mut raw = Vec::new();
        raw.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        raw.extend_from_slice(&16u32.to_le_bytes());
        raw.extend_from_slice(&1u16.to_le_bytes());
        raw.extend_from_slice(&1u16.to_le_bytes());
        raw.extend_from_slice(&16_000u32.to_le_bytes());
        raw.extend_from_slice(&32_000u32.to_le_bytes());
        raw.extend_from_slice(&2u16.to_le_bytes());
        raw.extend_from_slice(&16u16.to_le_bytes());
        raw.extend_from_slice(b"data\0\0\0\0");
        for s in [100i16, 200, 300] {
            raw.extend_from_slice(&s.to_le_bytes());
        }
        fs::write(d.join(CHUNKS_DIR).join("mic-000009.wav"), raw).unwrap();

        let rec = crate::recover(&d).unwrap().expect("something to recover");
        let mut r = hound::WavReader::open(&rec.mic).unwrap();
        let got: Vec<i16> = r.samples::<i16>().map(|s| s.unwrap()).collect();
        // First chunk complete (finalised at 1 s); second chunk: at least what was flushed.
        assert!(got.len() >= 16_000 + 3, "got {}", got.len());
        assert_eq!(&got[got.len() - 3..], &[100, 200, 300]);
        assert_eq!(
            got[..16_000],
            input[..16_000]
                .iter()
                .map(|&x| f32_to_i16(x))
                .collect::<Vec<_>>()[..]
        );
        assert!(rec.system.is_none());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tolerant_reader_rejects_garbage() {
        let d = tmpdir("garbage");
        let p = d.join("x.wav");
        fs::write(&p, b"not a wav").unwrap();
        assert!(read_pcm16_tolerant(&p).is_err());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn i16_conversion_clamps() {
        assert_eq!(f32_to_i16(2.0), i16::MAX);
        assert_eq!(f32_to_i16(-2.0), -i16::MAX);
        assert_eq!(f32_to_i16(0.0), 0);
    }
}
