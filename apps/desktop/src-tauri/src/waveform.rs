//! Streaming, range-aware peak envelopes. Cache entries are bounded per track.

use std::fs::{File, Metadata};
use std::io::{self, BufReader};
use std::path::Path;

use minutae_core::{MeetingId, Track};
use minutae_store::{Store, TrackFile};

use crate::dto::{MeetingWaveform, TrackWaveform, WaveformRange};

pub const MAX_BUCKETS: usize = 2048;

pub trait WaveformSource {
    fn read(&self, track: &TrackFile, range: WaveformRange, buckets: usize) -> TrackWaveform;
}

pub struct CachedWaveforms<'a>(pub &'a Store);

impl WaveformSource for CachedWaveforms<'_> {
    fn read(&self, track: &TrackFile, range: WaveformRange, buckets: usize) -> TrackWaveform {
        match self.peaks(track, range, buckets) {
            Ok(Some(levels)) => TrackWaveform::Ready { levels },
            Ok(None) => TrackWaveform::Deleted,
            Err(reason) => TrackWaveform::Unreadable { reason },
        }
    }
}

impl CachedWaveforms<'_> {
    fn peaks(
        &self,
        track: &TrackFile,
        range: WaveformRange,
        buckets: usize,
    ) -> Result<Option<Vec<f32>>, String> {
        match std::fs::metadata(&track.path) {
            Ok(meta) if !meta.is_file() => {
                return Err("This audio track is not a regular file.".into());
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
        let file = match File::open(&track.path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            return Err("This audio track is not a regular file.".into());
        }
        let fingerprint = fingerprint_of(&track.path, &metadata);
        let request = format!("{}:{}:{buckets}", range.start_ms, range.end_ms);
        let key = match track.track {
            Track::Mic => "mic",
            Track::System => "system",
        };
        if let Ok(Some(levels)) = self
            .0
            .waveform(track.meeting_id, key, &fingerprint, &request)
        {
            return Ok(Some(levels));
        }

        let wav = hound::WavReader::new(BufReader::with_capacity(64 * 1024, &file))
            .map_err(|e| e.to_string())?;
        let levels = stream_peaks(wav, range, buckets).map_err(|e| e.to_string())?;
        let current = std::fs::metadata(&track.path).map_err(|e| e.to_string())?;
        if fingerprint != fingerprint_of(&track.path, &current) {
            return Err("This audio track changed while Minutae was reading it. Try again.".into());
        }
        if let Err(e) =
            self.0
                .cache_waveform(track.meeting_id, key, &fingerprint, &request, &levels)
        {
            tracing::warn!(error = %e, "saving waveform cache failed");
        }
        Ok(Some(levels))
    }
}

fn fingerprint_of(path: &str, metadata: &Metadata) -> String {
    let key = format!(
        "v1:{path}:{}:{:?}:{:?}",
        metadata.len(),
        metadata.modified().ok(),
        metadata.created().ok()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{key}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    key
}

fn stream_peaks<R: io::Read + io::Seek>(
    mut wav: hound::WavReader<R>,
    range: WaveformRange,
    buckets: usize,
) -> hound::Result<Vec<f32>> {
    let spec = wav.spec();
    let frame = |ms: u64| ms.saturating_mul(u64::from(spec.sample_rate)) / 1000;
    let start = frame(range.start_ms);
    let end = frame(range.end_ms);
    let available = u64::from(wav.duration());
    // Compute frame boundaries once per bucket, and include every channel's peak.
    let sizes = (0..=buckets)
        .map(|i| {
            let offset = u128::from(end - start) * i as u128 / buckets as u128;
            (start + offset as u64).min(available)
        })
        .collect::<Vec<_>>();
    let counts = sizes
        .windows(2)
        .map(|w| (w[1] - w[0]) * u64::from(spec.channels));
    if spec.sample_format == hound::SampleFormat::Int && spec.bits_per_sample == 16 {
        // The recorder's PCM16 format is scanned in blocks. Validate the packing
        // with hound before reading raw little-endian samples.
        if let Some(first) = wav.samples::<i16>().next() {
            first?;
        }
        wav.seek(start.min(available) as u32)?;
        return pcm16_peaks(wav.into_inner(), counts, buckets);
    }
    wav.seek(start.min(available) as u32)?;
    let mut levels = Vec::with_capacity(buckets);
    match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            let mut samples = wav.samples::<i32>();
            for count in counts {
                let mut peak = 0;
                for _ in 0..count {
                    peak = peak.max(
                        samples
                            .next()
                            .ok_or(hound::Error::FormatError("truncated WAV"))??
                            .unsigned_abs(),
                    );
                }
                levels.push((peak as f32 / scale).min(1.0));
            }
        }
        hound::SampleFormat::Float => {
            let mut samples = wav.samples::<f32>();
            for count in counts {
                let mut peak = 0.0_f32;
                for _ in 0..count {
                    peak = peak.max(
                        samples
                            .next()
                            .ok_or(hound::Error::FormatError("truncated WAV"))??
                            .abs(),
                    );
                }
                levels.push(peak.min(1.0));
            }
        }
    }
    Ok(levels)
}

fn pcm16_peaks(
    mut reader: impl io::Read,
    counts: impl Iterator<Item = u64>,
    buckets: usize,
) -> hound::Result<Vec<f32>> {
    let mut levels = Vec::with_capacity(buckets);
    let mut buffer = [0; 64 * 1024];
    for mut count in counts {
        let mut peak = 0;
        while count > 0 {
            let samples = count.min((buffer.len() / 2) as u64) as usize;
            let bytes = &mut buffer[..samples * 2];
            reader.read_exact(bytes)?;
            for &sample in bytes.as_chunks::<2>().0 {
                peak = peak.max(i16::from_le_bytes(sample).unsigned_abs());
            }
            count -= samples as u64;
        }
        levels.push(f32::from(peak) / 32768.0);
    }
    Ok(levels)
}

pub fn meeting_waveform(
    source: &dyn WaveformSource,
    tracks: &[TrackFile],
    fallback_ms: u64,
    buckets: usize,
    range: Option<WaveformRange>,
) -> Result<MeetingWaveform, String> {
    if !(1..=MAX_BUCKETS).contains(&buckets) {
        return Err(format!(
            "Request between 1 and {MAX_BUCKETS} waveform buckets."
        ));
    }
    if range.is_some_and(|r| r.end_ms <= r.start_ms || r.end_ms > 9_007_199_254_740_991) {
        return Err("The waveform time range is invalid.".into());
    }
    let range = range.unwrap_or_else(|| WaveformRange {
        start_ms: 0,
        end_ms: tracks
            .iter()
            .map(|t| {
                t.duration_ms
                    .or_else(|| {
                        let path = Path::new(&t.path);
                        if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
                            return None;
                        }
                        crate::recording::wav_duration_ms(path)
                    })
                    .unwrap_or(0)
            })
            .max()
            .filter(|ms| *ms > 0)
            .unwrap_or(fallback_ms),
    });
    let track = |track| {
        tracks
            .iter()
            .find(|t| t.track == track)
            .map_or(TrackWaveform::Missing, |t| source.read(t, range, buckets))
    };
    Ok(MeetingWaveform {
        range,
        mic: track(Track::Mic),
        system: track(Track::System),
    })
}

#[tauri::command]
pub async fn meeting_waveform_levels(
    state: tauri::State<'_, crate::state::AppState>,
    id: String,
    buckets: usize,
    range: Option<WaveformRange>,
) -> Result<MeetingWaveform, String> {
    let id: MeetingId = crate::commands::uuid(&id)?;
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        let m = store
            .get_meeting(id)
            .map_err(|e| e.to_string())?
            .ok_or("This meeting no longer exists.")?;
        let tracks = store.list_tracks(id).map_err(|e| e.to_string())?;
        let fallback = m.ended_at.map_or(0, |end| {
            (end - m.started_at).num_milliseconds().max(0) as u64
        });
        meeting_waveform(&CachedWaveforms(&store), &tracks, fallback, buckets, range)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeWaveform;
    impl WaveformSource for FakeWaveform {
        fn read(&self, _: &TrackFile, range: WaveformRange, buckets: usize) -> TrackWaveform {
            assert_eq!((range.start_ms, range.end_ms), (100, 200));
            TrackWaveform::Ready {
                levels: vec![0.5; buckets],
            }
        }
    }

    #[test]
    fn range_is_on_one_shared_timeline_and_missing_is_not_deleted() {
        let track = TrackFile {
            meeting_id: uuid::Uuid::new_v4(),
            track: Track::Mic,
            path: String::new(),
            duration_ms: Some(900),
            bytes: None,
        };
        let range = Some(WaveformRange {
            start_ms: 100,
            end_ms: 200,
        });
        let tape = meeting_waveform(&FakeWaveform, &[track], 0, 2, range).unwrap();
        assert_eq!(
            tape.mic,
            TrackWaveform::Ready {
                levels: vec![0.5; 2]
            }
        );
        assert_eq!(tape.system, TrackWaveform::Missing);
        assert!(meeting_waveform(&FakeWaveform, &[], 0, MAX_BUCKETS + 1, range).is_err());
    }

    #[test]
    fn two_hour_pcm16_recording_is_scanned_in_blocks() {
        use std::io::{Seek, SeekFrom, Write};

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("two-hours.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        hound::WavWriter::create(&path, spec)
            .unwrap()
            .finalize()
            .unwrap();
        let data_bytes: u32 = 2 * 60 * 60 * spec.sample_rate * 2;
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        // Expand the recorder's canonical header into a sparse WAV. Its sole
        // nonzero sample is in the final bucket.
        file.seek(SeekFrom::Start(4)).unwrap();
        file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(40)).unwrap();
        file.write_all(&data_bytes.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(u64::from(42 + data_bytes)))
            .unwrap();
        file.write_all(&16384_i16.to_le_bytes()).unwrap();
        drop(file);

        let wav = hound::WavReader::open(path).unwrap();
        let levels = stream_peaks(
            wav,
            WaveformRange {
                start_ms: 0,
                end_ms: 7_200_000,
            },
            64,
        )
        .unwrap();
        assert_eq!(levels[63], 0.5);
        assert!(levels[..63].iter().all(|level| *level == 0.0));
    }

    #[test]
    fn stream_cache_is_invalidated_on_replacement_and_distinguishes_failures() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mic.wav");
        let store = Store::open_in_memory().unwrap();
        let now = chrono::Utc::now();
        let meeting = minutae_core::Meeting {
            id: uuid::Uuid::new_v4(),
            title: "Tape".into(),
            started_at: now,
            ended_at: Some(now),
            status: minutae_core::MeetingStatus::Done,
            source_app: None,
            failure: None,
        };
        store.create_meeting(&meeting).unwrap();
        let write = |path: &Path, values: &[i16]| {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: 1000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut wav = hound::WavWriter::create(path, spec).unwrap();
            for value in values {
                wav.write_sample(*value).unwrap();
            }
            wav.finalize().unwrap();
        };
        write(&path, &[i16::MAX, 8192, -16384, 4096, 2048, 0]);
        let track = TrackFile {
            meeting_id: meeting.id,
            track: Track::Mic,
            path: path.to_string_lossy().into_owned(),
            duration_ms: Some(6),
            bytes: None,
        };
        let range = WaveformRange {
            start_ms: 1,
            end_ms: 9,
        };
        let source = CachedWaveforms(&store);
        let expected = TrackWaveform::Ready {
            levels: vec![0.5, 0.125, 0.0, 0.0],
        };
        assert_eq!(source.read(&track, range, 4), expected);
        assert_eq!(source.read(&track, range, 4), expected);
        let replacement = temp.path().join("replacement.wav");
        write(&replacement, &[0; 6]);
        std::fs::remove_file(&path).unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert_eq!(
            source.read(&track, range, 4),
            TrackWaveform::Ready {
                levels: vec![0.0; 4]
            }
        );
        std::fs::write(&path, b"not audio").unwrap();
        assert!(matches!(
            source.read(&track, range, 4),
            TrackWaveform::Unreadable { .. }
        ));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(source.read(&track, range, 4), TrackWaveform::Deleted);
    }
}
