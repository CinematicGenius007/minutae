//! Just enough RIFF/WAVE parsing to know how long a legacy recording is.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Duration of a WAV file in milliseconds, or `None` when the header can't be read.
/// The `data` size is clamped to the bytes really on disk (crashed recordings left
/// stale sizes in their headers).
pub fn duration_ms(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let mut byte_rate: Option<u64> = None;
    let mut pos = 12u64;
    while pos + 8 <= len {
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut ch = [0u8; 8];
        f.read_exact(&mut ch).ok()?;
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;
        match &ch[0..4] {
            b"fmt " => {
                let mut fmt = [0u8; 16];
                f.read_exact(&mut fmt).ok()?;
                byte_rate = Some(u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]) as u64);
            }
            b"data" => {
                let rate = byte_rate.filter(|r| *r > 0)?;
                let available = len.saturating_sub(pos + 8);
                let bytes = if size == 0 || size > available {
                    available
                } else {
                    size
                };
                return Some(bytes * 1000 / rate);
            }
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    None
}

#[cfg(test)]
pub(crate) fn write_test_wav(path: &Path, sample_rate: u32, seconds: u32) {
    // 16-bit mono PCM silence.
    let data = sample_rate * 2 * seconds;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(44 + data as usize, 0);
    std::fs::write(path, b).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_duration_and_rejects_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_test_wav(&p, 16_000, 3);
        assert_eq!(duration_ms(&p), Some(3000));
        let bad = dir.path().join("b.wav");
        std::fs::write(&bad, b"not a wav file at all").unwrap();
        assert_eq!(duration_ms(&bad), None);
        assert_eq!(duration_ms(&dir.path().join("missing.wav")), None);
    }
}
