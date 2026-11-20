//! minutae-audio: two-track meeting capture (mic = "You", system = everyone else).
//!
//! ```no_run
//! use minutae_audio::{CaptureConfig, Recorder};
//! let handle = Recorder::new().start(CaptureConfig::new("/tmp/meeting".into()))?;
//! let levels = handle.levels(); // tokio watch receiver, ~30 Hz
//! std::thread::sleep(std::time::Duration::from_secs(5));
//! println!("{:?}", *levels.borrow());
//! let tracks = handle.stop()?;
//! # Ok::<(), minutae_audio::AudioError>(())
//! ```
//!
//! Pipeline per track: cpal callback (downmix to mono, never blocks) ->
//! bounded queue -> writer thread (resample to 16 kHz, meter, gap-fill with
//! silence against the wall clock, write 10 s WAV chunks) -> on stop, chunks
//! are stitched into `mic.wav` / `system.wav`. [`recover`] stitches chunks left
//! behind by a crash.

mod capture;
mod level;
mod resample;
mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use minutae_core::Track;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

pub use capture::{InputDevice, list_input_devices};
pub use level::rms;
pub use resample::Resampler;
pub use wav::{SAMPLE_RATE, read_pcm16_tolerant};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no microphone found")]
    NoInputDevice,
    #[error("audio device error: {0}")]
    Device(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("system audio capture unavailable: {0}")]
    Unsupported(String),
    #[error("corrupt audio file: {0}")]
    Corrupt(PathBuf),
    #[error("recording already stopped")]
    Stopped,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Wav(#[from] hound::Error),
}

pub type Result<T, E = AudioError> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    /// cpal device id from [`list_input_devices`]; `None` = system default.
    pub mic_device: Option<String>,
    /// Also record the computer's output audio (the other participants).
    pub include_system: bool,
    pub out_dir: PathBuf,
    /// Chunk length; a crash loses at most this much audio (usually ~1 s,
    /// since the open chunk's header is refreshed every second).
    pub chunk_secs: u32,
}

impl CaptureConfig {
    pub fn new(out_dir: PathBuf) -> Self {
        Self {
            mic_device: None,
            include_system: true,
            out_dir,
            chunk_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelFrame {
    /// Recorded time, excluding pauses.
    pub elapsed_ms: u64,
    /// RMS of the 16 kHz mono signal since the previous frame, 0.0..=1.0.
    pub mic_rms: f32,
    pub system_rms: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedTracks {
    pub mic: PathBuf,
    pub system: Option<PathBuf>,
    pub duration_ms: u64,
}

/// Starts recordings. Stateless; one [`RecordingHandle`] per meeting.
#[derive(Debug, Default, Clone, Copy)]
pub struct Recorder;

const LEVEL_INTERVAL: Duration = Duration::from_millis(33);
/// Queue depth between the audio callback and the writer (~10 ms blocks -> ~5 s).
const QUEUE_BLOCKS: usize = 512;
/// Pad with silence when a track falls this far behind the wall clock
/// (WASAPI loopback delivers nothing while nothing plays; taps can start late).
const GAP_FILL_SAMPLES: u64 = SAMPLE_RATE as u64 / 5; // 200 ms

impl Recorder {
    pub fn new() -> Self {
        Self
    }

    /// Start recording. Fails if the microphone can't be opened. If system
    /// capture fails, recording continues mic-only and
    /// [`RecordingHandle::system_error`] says why.
    pub fn start(&self, config: CaptureConfig) -> Result<RecordingHandle> {
        std::fs::create_dir_all(&config.out_dir)?;
        let shared = Arc::new(Shared {
            clock: level::Clock::new(),
            paused: Arc::new(AtomicBool::new(false)),
            stopped: AtomicBool::new(false),
            mic_meter: level::Meter::default(),
            sys_meter: level::Meter::default(),
            last_error: Arc::new(Mutex::new(None)),
        });

        let (mic_tx, mic_rx) = mpsc::sync_channel(QUEUE_BLOCKS);
        let (sys_tx, sys_rx) = mpsc::sync_channel(QUEUE_BLOCKS);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        // cpal streams are not `Send` on every backend, so one thread owns them.
        let cfg = config.clone();
        let sh = shared.clone();
        let capture_thread = std::thread::Builder::new()
            .name("minutae-capture".into())
            .spawn(move || {
                let ctx = |tx, label| capture::CallbackCtx {
                    tx,
                    paused: sh.paused.clone(),
                    last_error: sh.last_error.clone(),
                    label,
                };
                let mic = capture::open_mic(cfg.mic_device.as_deref()).and_then(|(d, c)| {
                    let rate = c.sample_rate();
                    capture::build_stream(&d, &c, ctx(mic_tx, "mic")).map(|s| (s, rate))
                });
                let (mic_stream, mic_rate) = match mic {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let system = if cfg.include_system {
                    capture::open_system().and_then(|(d, c)| {
                        let rate = c.sample_rate();
                        capture::build_stream(&d, &c, ctx(sys_tx, "system")).map(|s| (s, rate))
                    })
                } else {
                    drop(sys_tx);
                    Err(AudioError::Unsupported("disabled".into()))
                };
                use cpal::traits::StreamTrait;
                if let Err(e) = mic_stream.play() {
                    let _ = ready_tx.send(Err(capture::dev_err(e)));
                    return;
                }
                let (sys_stream, sys_info) = match system {
                    Ok((s, rate)) => match s.play() {
                        Ok(()) => (Some(s), Ok(rate)),
                        Err(e) => (None, Err(capture::dev_err(e).to_string())),
                    },
                    Err(e) => (None, Err(e.to_string())),
                };
                let _ = ready_tx.send(Ok((mic_rate, sys_info)));
                let _ = stop_rx.recv();
                drop(sys_stream);
                drop(mic_stream);
            })?;

        let (mic_rate, sys_info) = match ready_rx.recv() {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                let _ = capture_thread.join();
                return Err(e);
            }
            Err(_) => {
                let _ = capture_thread.join();
                return Err(AudioError::Device("capture thread exited".into()));
            }
        };
        // Time zero is when both streams are running, not when we began opening devices.
        shared.clock.restart();
        let system_error = if config.include_system {
            sys_info.as_ref().err().cloned()
        } else {
            None
        };

        let mic_writer = spawn_writer(Track::Mic, mic_rx, mic_rate, &config, shared.clone())?;
        let sys_writer = match sys_info {
            Ok(rate) => Some(spawn_writer(
                Track::System,
                sys_rx,
                rate,
                &config,
                shared.clone(),
            )?),
            Err(_) => None,
        };

        let (level_tx, level_rx) = watch::channel(LevelFrame::default());
        let sh = shared.clone();
        let level_thread = std::thread::Builder::new()
            .name("minutae-levels".into())
            .spawn(move || {
                while !sh.stopped.load(Ordering::Relaxed) {
                    std::thread::sleep(LEVEL_INTERVAL);
                    let frame = LevelFrame {
                        elapsed_ms: sh.clock.active().as_millis() as u64,
                        mic_rms: sh.mic_meter.take(),
                        system_rms: sh.sys_meter.take(),
                    };
                    level_tx.send_replace(frame);
                }
            })?;

        tracing::info!(
            ?mic_rate,
            system = system_error.is_none(),
            "recording started"
        );
        Ok(RecordingHandle {
            out_dir: config.out_dir,
            shared,
            levels: level_rx,
            system_error,
            inner: Mutex::new(Some(Running {
                stop_tx,
                capture_thread,
                level_thread,
                mic_writer,
                sys_writer,
            })),
        })
    }
}

struct Shared {
    clock: level::Clock,
    paused: Arc<AtomicBool>,
    stopped: AtomicBool,
    mic_meter: level::Meter,
    sys_meter: level::Meter,
    last_error: Arc<Mutex<Option<String>>>,
}

struct Running {
    stop_tx: mpsc::Sender<()>,
    capture_thread: JoinHandle<()>,
    level_thread: JoinHandle<()>,
    mic_writer: JoinHandle<Result<u64>>,
    sys_writer: Option<JoinHandle<Result<u64>>>,
}

pub struct RecordingHandle {
    out_dir: PathBuf,
    shared: Arc<Shared>,
    levels: watch::Receiver<LevelFrame>,
    system_error: Option<String>,
    inner: Mutex<Option<Running>>,
}

impl RecordingHandle {
    /// Level meter at ~30 Hz. `borrow()` from sync code or `changed().await`.
    pub fn levels(&self) -> watch::Receiver<LevelFrame> {
        self.levels.clone()
    }

    /// Why system audio isn't being recorded (`None` = it is, or it wasn't requested).
    pub fn system_error(&self) -> Option<&str> {
        self.system_error.as_deref()
    }

    /// Most recent stream error reported by the OS, if any.
    pub fn last_stream_error(&self) -> Option<String> {
        self.shared
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn out_dir(&self) -> &Path {
        &self.out_dir
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::SeqCst);
        self.shared.clock.pause();
    }

    pub fn resume(&self) {
        self.shared.clock.resume();
        self.shared.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    pub fn elapsed(&self) -> Duration {
        self.shared.clock.active()
    }

    /// Stop capture, close chunks and stitch them into `mic.wav` / `system.wav`.
    pub fn stop(&self) -> Result<RecordedTracks> {
        let running = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or(AudioError::Stopped)?;
        self.shared.clock.freeze();
        self.shared.stopped.store(true, Ordering::SeqCst);
        let _ = running.stop_tx.send(());
        let _ = running.capture_thread.join(); // drops streams -> writers see disconnect
        let _ = running.level_thread.join();
        let join = |h: JoinHandle<Result<u64>>| {
            h.join()
                .unwrap_or_else(|_| Err(AudioError::Device("writer thread panicked".into())))
        };
        let mic_res = join(running.mic_writer);
        let sys_res = running.sys_writer.map(join);
        mic_res?;
        if let Some(Err(e)) = &sys_res {
            tracing::warn!(error = %e, "system track writer failed");
        }
        let (mic, mic_samples) = wav::stitch(&self.out_dir, Track::Mic)?
            .ok_or_else(|| AudioError::Device("no microphone audio was captured".into()))?;
        let system = match sys_res {
            Some(_) => wav::stitch(&self.out_dir, Track::System)?.map(|(p, _)| p),
            None => None,
        };
        Ok(RecordedTracks {
            mic,
            system,
            duration_ms: mic_samples * 1000 / SAMPLE_RATE as u64,
        })
    }
}

impl Drop for RecordingHandle {
    fn drop(&mut self) {
        if self.inner.lock().map(|g| g.is_some()).unwrap_or(false)
            && let Err(e) = self.stop()
        {
            tracing::warn!(error = %e, "stopping recording on drop failed");
        }
    }
}

fn spawn_writer(
    track: Track,
    rx: Receiver<capture::Block>,
    in_rate: u32,
    config: &CaptureConfig,
    shared: Arc<Shared>,
) -> Result<JoinHandle<Result<u64>>> {
    let mut writer = wav::ChunkWriter::new(&config.out_dir, track, config.chunk_secs)?;
    let name = format!("minutae-writer-{}", wav::track_name(track));
    Ok(std::thread::Builder::new().name(name).spawn(move || {
        let meter = match track {
            Track::Mic => &shared.mic_meter,
            Track::System => &shared.sys_meter,
        };
        let mut resampler = Resampler::new(in_rate, SAMPLE_RATE);
        let mut out = Vec::with_capacity(4096);
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(block) => {
                    out.clear();
                    resampler.process(&block, &mut out);
                    meter.add(&out);
                    writer.write(&out)?;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            fill_gap(&mut writer, &shared, GAP_FILL_SAMPLES)?;
        }
        out.clear();
        resampler.flush(&mut out);
        writer.write(&out)?;
        // Align the track's end with the wall clock (frozen at stop).
        fill_gap(&mut writer, &shared, 0)?;
        writer.finish()
    })?)
}

/// If the track is more than `slack` samples behind the recording clock, pad
/// with silence so the two tracks stay time-aligned.
fn fill_gap(writer: &mut wav::ChunkWriter, shared: &Shared, slack: u64) -> Result<()> {
    if shared.paused.load(Ordering::Relaxed) {
        return Ok(());
    }
    let expected = shared.clock.active_samples(SAMPLE_RATE);
    let written = writer.total_samples();
    if written + slack < expected {
        // Leave half the slack so normal jitter doesn't trigger padding.
        let target = expected - slack / 2;
        writer.write_silence(target - written)?;
    }
    Ok(())
}

/// Stitch chunks left in `out_dir` by a crash into `mic.wav` / `system.wav`.
/// Returns `None` if there was nothing to recover.
pub fn recover(out_dir: &Path) -> Result<Option<RecordedTracks>> {
    let mic = wav::stitch(out_dir, Track::Mic)?;
    let system = wav::stitch(out_dir, Track::System)?;
    match (mic, system) {
        (None, None) => Ok(None),
        (mic, system) => {
            let (mic_path, samples) = match mic {
                Some(v) => v,
                // System-only leftovers: still report, with mic.wav if it exists.
                None => (wav::final_path(out_dir, Track::Mic), 0),
            };
            let samples = if samples == 0 {
                system.as_ref().map(|(_, n)| *n).unwrap_or(0)
            } else {
                samples
            };
            Ok(Some(RecordedTracks {
                mic: mic_path,
                system: system.map(|(p, _)| p),
                duration_ms: samples * 1000 / SAMPLE_RATE as u64,
            }))
        }
    }
}

/// True if `out_dir` holds chunks from an unfinished recording.
pub fn needs_recovery(out_dir: &Path) -> bool {
    let dir = out_dir.join(wav::CHUNKS_DIR);
    [Track::Mic, Track::System].iter().any(|&t| {
        wav::list_chunks(&dir, t)
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_is_send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<RecordingHandle>();
        assert::<Recorder>();
    }

    #[test]
    fn gap_fill_pads_to_clock() {
        let d = std::env::temp_dir().join(format!("minutae-gap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let shared = Shared {
            clock: level::Clock::new(),
            paused: Arc::new(AtomicBool::new(false)),
            stopped: AtomicBool::new(false),
            mic_meter: Default::default(),
            sys_meter: Default::default(),
            last_error: Default::default(),
        };
        let mut w = wav::ChunkWriter::new(&d, Track::System, 10).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        fill_gap(&mut w, &shared, GAP_FILL_SAMPLES).unwrap();
        let n = w.total_samples();
        // ~400 ms elapsed minus 100 ms half-slack => ~300 ms of silence.
        assert!((4_000..=6_000).contains(&n), "padded {n}");
        // Within slack: nothing added.
        fill_gap(&mut w, &shared, GAP_FILL_SAMPLES).unwrap();
        assert_eq!(w.total_samples(), n);
        shared.clock.freeze();
        fill_gap(&mut w, &shared, 0).unwrap();
        assert!(w.total_samples() >= shared.clock.active_samples(SAMPLE_RATE));
        w.finish().unwrap();
        assert!(needs_recovery(&d));
        let rec = recover(&d).unwrap().unwrap();
        assert!(rec.system.is_some());
        assert!(!needs_recovery(&d));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
