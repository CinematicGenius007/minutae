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
//! Pipeline per track: cpal callback (downmix to mono into a preallocated lock-free
//! ring; never allocates or blocks) -> writer thread (resample to 16 kHz, meter, keep
//! the track aligned with the wall clock by padding real gaps with silence or dropping
//! samples when it runs ahead, write 10 s WAV chunks) -> on stop, chunks are stitched
//! into `mic.wav` / `system.wav`. [`recover`] stitches chunks left behind by a crash.
//!
//! [`Recorder::start_with_taps`] also hands every block the writer writes (16 kHz mono,
//! with its position on the recording timeline) to an optional [`Tap`] per track, on the
//! writer thread. Live captions use it; a tap must copy and return, never block.
//!
//! Problems during a recording (device unplugged, disk full) are reported live on
//! [`RecordingHandle::events`]. A track whose device disappears is reopened on the
//! current default device when possible.
//!
//! System audio and the call app: when [`CaptureConfig::system_app`] names the app on the
//! call, macOS records only that app's audio (a Core Audio process tap on 14.4+,
//! ScreenCaptureKit on 13.0 - 14.3) and keeps following it as helper processes appear.
//! Until the app has an audio process, or if the tap fails, all system audio is recorded.
//! [`RecordingHandle::system_source`] says which is live; changes are sent as
//! [`CaptureEvent::SystemSourceChanged`].

mod capture;
mod level;
#[cfg(target_os = "macos")]
mod macos;
mod resample;
mod target;
mod wav;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use minutae_core::Track;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};

pub use capture::{InputDevice, list_input_devices};
pub use level::rms;
pub use resample::Resampler;
pub use target::{AppTarget, SystemBackend, SystemScope, SystemSource};
pub use wav::{SAMPLE_RATE, read_pcm16_tolerant};

/// How this machine records one app's audio when [`CaptureConfig::system_app`] is set.
/// `Loopback` = it can't (all system audio is recorded).
pub fn app_capture_backend() -> SystemBackend {
    #[cfg(target_os = "macos")]
    {
        macos::app_backend()
    }
    #[cfg(not(target_os = "macos"))]
    {
        SystemBackend::Loopback
    }
}

/// macOS Screen Recording permission, which the ScreenCaptureKit fallback needs.
#[cfg(target_os = "macos")]
pub mod screen_recording {
    pub use crate::macos::sck::{
        request_screen_recording as request, screen_recording_granted as granted,
    };
}

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
    /// The app on the call: record only its audio where the OS allows (macOS).
    /// `None` = all system audio.
    #[serde(default)]
    pub system_app: Option<AppTarget>,
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
            system_app: None,
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

/// Something went wrong (or changed) while recording. Sent on [`RecordingHandle::events`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptureEvent {
    /// The OS reported a stream error for `track`. If the device is gone, a reopen is
    /// attempted and followed by `DeviceSwitched` or `TrackLost`.
    #[serde(rename_all = "camelCase")]
    StreamError { track: Track, message: String },
    /// `track` is now captured from `device` (after the previous device disappeared,
    /// or the OS rerouted the default device; `None` = name unknown).
    #[serde(rename_all = "camelCase")]
    DeviceSwitched {
        track: Track,
        device: Option<String>,
    },
    /// `track`'s device is gone and no replacement could be opened. The track is padded
    /// with silence; reopening is retried every second.
    #[serde(rename_all = "camelCase")]
    TrackLost { track: Track, message: String },
    /// Writing `track` to disk failed (disk full, permissions). That track has stopped
    /// recording; [`RecordingHandle::stop`] still stitches what was written and returns
    /// the error if it was the mic.
    #[serde(rename_all = "camelCase")]
    WriteFailed { track: Track, message: String },
    /// The system track switched between the call app only and all system audio (e.g.
    /// the call app started playing audio after the recording began).
    #[serde(rename_all = "camelCase")]
    SystemSourceChanged { source: SystemSource },
}

/// Receives each block a track's writer writes: `(start_sample, samples)` at 16 kHz mono,
/// `start_sample` on the recording timeline (pauses excluded; gaps the writer pads with
/// silence show up as a jump). Runs on the writer thread: copy and return, never block.
pub type Tap = Box<dyn FnMut(u64, &[f32]) + Send>;

/// Optional per-track taps for [`Recorder::start_with_taps`].
#[derive(Default)]
pub struct Taps {
    pub mic: Option<Tap>,
    pub system: Option<Tap>,
}

/// Starts recordings. Stateless; one [`RecordingHandle`] per meeting.
#[derive(Debug, Default, Clone, Copy)]
pub struct Recorder;

const LEVEL_INTERVAL: Duration = Duration::from_millis(33);
/// Re-align a track with the wall clock once it is this far off (WASAPI loopback
/// delivers nothing while nothing plays; taps start late; device clocks drift).
const ALIGN_SLACK_SAMPLES: u64 = SAMPLE_RATE as u64 / 5; // 200 ms
/// How often the capture thread retries opening a lost device.
const REOPEN_RETRY: Duration = Duration::from_secs(1);
const EVENT_CAPACITY: usize = 64;

/// One capture stream's output as seen by its writer.
struct Source {
    consumer: rtrb::Consumer<f32>,
    rate: u32,
}

enum Cmd {
    Stop,
    Reopen { track: Track, generation: u64 },
}

impl Recorder {
    pub fn new() -> Self {
        Self
    }

    /// Start recording. Fails if the microphone can't be opened. If system
    /// capture fails, recording continues mic-only and
    /// [`RecordingHandle::system_error`] says why.
    pub fn start(&self, config: CaptureConfig) -> Result<RecordingHandle> {
        self.start_with_taps(config, Taps::default())
    }

    /// [`Recorder::start`], also feeding each track's written audio to its [`Tap`].
    pub fn start_with_taps(&self, config: CaptureConfig, taps: Taps) -> Result<RecordingHandle> {
        std::fs::create_dir_all(&config.out_dir)?;
        let shared = Arc::new(Shared::new(level::Clock::new()));

        let (mic_src_tx, mic_src_rx) = mpsc::channel::<Source>();
        let (sys_src_tx, sys_src_rx) = mpsc::channel::<Source>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();

        // cpal streams are not `Send` on every backend, so one thread owns them.
        let cfg = config.clone();
        let sh = shared.clone();
        let reopen_tx = cmd_tx.clone();
        let capture_thread = std::thread::Builder::new()
            .name("minutae-capture".into())
            .spawn(move || {
                let mut cap = CaptureLoop {
                    cfg,
                    shared: sh.clone(),
                    on_error: stream_error_handler(sh, reopen_tx),
                    mic: TrackStream::new(mic_src_tx),
                    system: TrackStream::new(sys_src_tx),
                };
                let mic_rate = match cap.open(Track::Mic, Why::Start) {
                    Ok(rate) => rate,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let sys_info = if cap.cfg.include_system {
                    cap.open(Track::System, Why::Start)
                        .map_err(|e| e.to_string())
                } else {
                    Err("disabled".to_string())
                };
                if sys_info.is_err() {
                    cap.system.sources = None; // no system writer will run
                }
                let _ = ready_tx.send(Ok((mic_rate, sys_info)));
                cap.run(cmd_rx);
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

        let mic_writer = spawn_writer(Track::Mic, mic_src_rx, &config, shared.clone(), taps.mic)?;
        let sys_writer = match sys_info {
            Ok(_) => Some(spawn_writer(
                Track::System,
                sys_src_rx,
                &config,
                shared.clone(),
                taps.system,
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
                cmd_tx,
                capture_thread,
                level_thread,
                mic_writer,
                sys_writer,
            })),
        })
    }
}

/// The cpal error callback: log, remember, broadcast, and ask the capture thread to
/// reopen the track when the stream is dead.
fn stream_error_handler(
    shared: Arc<Shared>,
    reopen: mpsc::Sender<Cmd>,
) -> Arc<dyn Fn(Track, u64, cpal::Error) + Send + Sync> {
    Arc::new(move |track, generation, e: cpal::Error| {
        let kind = e.kind();
        if matches!(
            kind,
            cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied
        ) {
            tracing::debug!(track = wav::track_name(track), error = %e, "audio stream glitch");
            return;
        }
        tracing::warn!(track = wav::track_name(track), error = %e, "audio stream error");
        let message = format!("{}: {e}", wav::track_name(track));
        *shared.last_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(message.clone());
        if kind == cpal::ErrorKind::DeviceChanged {
            shared.emit(CaptureEvent::DeviceSwitched {
                track,
                device: None,
            });
            return;
        }
        shared.emit(CaptureEvent::StreamError { track, message });
        if capture::needs_reopen(kind) {
            let _ = reopen.send(Cmd::Reopen { track, generation });
        }
    })
}

/// One track's live stream, owned by the capture thread.
struct TrackStream {
    stream: Option<capture::LiveStream>,
    generation: u64,
    /// Hands each newly opened stream's ring to the writer. `None` = no writer.
    sources: Option<mpsc::Sender<Source>>,
    /// Device lost and not yet reopened.
    lost: bool,
    /// System track recording only the call app: the OS handles of its processes.
    /// `None` = all system audio.
    app_handles: Option<Vec<u32>>,
    /// Per-app capture that failed to start for this set (not retried every second).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    failed: Option<Vec<u32>>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ticks: u32,
}

impl TrackStream {
    fn new(sources: mpsc::Sender<Source>) -> Self {
        Self {
            stream: None,
            generation: 0,
            sources: Some(sources),
            lost: false,
            app_handles: None,
            failed: None,
            ticks: 0,
        }
    }
}

/// Why a track is being opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    Start,
    /// The previous stream died.
    Reopen,
    /// The system track follows the call app (new process, output device changed).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Retarget,
}

/// A freshly started stream, ready to hand to the writer.
struct Opened {
    stream: capture::LiveStream,
    rate: u32,
    consumer: rtrb::Consumer<f32>,
    device: String,
    /// System track only.
    source: Option<SystemSource>,
    app_handles: Option<Vec<u32>>,
}

struct CaptureLoop {
    cfg: CaptureConfig,
    shared: Arc<Shared>,
    on_error: Arc<dyn Fn(Track, u64, cpal::Error) + Send + Sync>,
    mic: TrackStream,
    system: TrackStream,
}

impl CaptureLoop {
    fn slot(&mut self, track: Track) -> &mut TrackStream {
        match track {
            Track::Mic => &mut self.mic,
            Track::System => &mut self.system,
        }
    }

    /// Open (or reopen) `track` and hand its ring to the writer. Returns the device rate.
    fn open(&mut self, track: Track, why: Why) -> Result<u32> {
        let opened = match track {
            Track::Mic => {
                let (device, config) = match capture::open_mic(self.cfg.mic_device.as_deref()) {
                    // The chosen mic vanished: fall back to the default input.
                    Err(_) if why == Why::Reopen && self.cfg.mic_device.is_some() => {
                        capture::open_mic(None)?
                    }
                    other => other?,
                };
                self.open_cpal(track, &device, &config)?
            }
            Track::System => self.open_system()?,
        };
        let rate = opened.rate;
        self.install(track, opened, why);
        Ok(rate)
    }

    /// A ring for a new stream on `track` (bumps the track's generation).
    fn sink(&mut self, track: Track, rate: u32) -> (capture::StreamSink, rtrb::Consumer<f32>) {
        let (producer, consumer) = rtrb::RingBuffer::new(capture::ring_capacity(rate));
        let on_error = self.on_error.clone();
        let paused = self.shared.paused.clone();
        let slot = self.slot(track);
        slot.generation += 1;
        let sink = capture::StreamSink {
            producer,
            paused,
            track,
            generation: slot.generation,
            on_error,
        };
        (sink, consumer)
    }

    fn open_cpal(
        &mut self,
        track: Track,
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
    ) -> Result<Opened> {
        use cpal::traits::StreamTrait;
        let rate = config.sample_rate();
        let (sink, consumer) = self.sink(track, rate);
        let stream = capture::build_stream(device, config, sink)?;
        stream.play().map_err(capture::dev_err)?;
        Ok(Opened {
            stream: capture::LiveStream::Cpal(stream),
            rate,
            consumer,
            device: capture::device_name(device),
            source: (track == Track::System).then_some(SystemSource {
                scope: SystemScope::All,
                backend: SystemBackend::Loopback,
            }),
            app_handles: None,
        })
    }

    /// All system audio through the OS loopback (cpal).
    fn open_loopback(&mut self) -> Result<Opened> {
        let (device, config) = capture::open_system()?;
        self.open_cpal(Track::System, &device, &config)
    }

    /// The system track: only the call app where possible, else all system audio.
    fn open_system(&mut self) -> Result<Opened> {
        #[cfg(target_os = "macos")]
        {
            let target = self.cfg.system_app.clone().filter(|t| !t.is_empty());
            if macos::app_backend() == SystemBackend::ScreenCapture {
                // No process taps on this macOS: ScreenCaptureKit does both cases.
                return self.open_sck(target.as_ref());
            }
            if let Some(t) = &target {
                let objects = resolve_tap_target(t);
                if objects.is_empty() {
                    tracing::info!(
                        "the call app has no audio process yet; recording all system audio until it has"
                    );
                } else {
                    match self.open_tap(&objects) {
                        Ok(o) => return Ok(o),
                        Err(e) => {
                            tracing::warn!(error = %e, "per-app audio tap failed; recording all system audio");
                            self.system.failed = Some(objects);
                        }
                    }
                }
            }
        }
        self.open_loopback()
    }

    #[cfg(target_os = "macos")]
    fn open_tap(&mut self, objects: &[u32]) -> Result<Opened> {
        let mut tap = macos::tap::ProcessTap::create(objects)?;
        let rate = tap.rate;
        let (sink, consumer) = self.sink(Track::System, rate);
        tap.start(sink)?;
        let handles = tap.objects.clone();
        Ok(Opened {
            stream: capture::LiveStream::Tap(tap),
            rate,
            consumer,
            device: "call app audio (process tap)".into(),
            source: Some(SystemSource {
                scope: SystemScope::App,
                backend: SystemBackend::ProcessTap,
            }),
            app_handles: Some(handles),
        })
    }

    #[cfg(target_os = "macos")]
    fn open_sck(&mut self, target: Option<&AppTarget>) -> Result<Opened> {
        let rate = macos::sck::RATE;
        let (sink, consumer) = self.sink(Track::System, rate);
        let on_error = self.on_error.clone();
        let generation = sink.generation;
        let on_stop = Box::new(move |message: String| {
            on_error(
                Track::System,
                generation,
                cpal::Error::with_message(cpal::ErrorKind::StreamInvalidated, message),
            )
        });
        let stream = macos::sck::SckStream::start(target, sink, on_stop)?;
        let app = !stream.pids.is_empty();
        let handles = app.then(|| stream.pids.clone());
        Ok(Opened {
            stream: capture::LiveStream::Sck(stream),
            rate,
            consumer,
            device: "ScreenCaptureKit".into(),
            source: Some(SystemSource {
                scope: if app {
                    SystemScope::App
                } else {
                    SystemScope::All
                },
                backend: SystemBackend::ScreenCapture,
            }),
            app_handles: handles,
        })
    }

    /// Make `opened` the live stream of `track` (the previous one, if any, stops after
    /// the new one runs) and tell the writer and listeners.
    fn install(&mut self, track: Track, opened: Opened, why: Why) {
        let Opened {
            stream,
            rate,
            consumer,
            device,
            source,
            app_handles,
        } = opened;
        let slot = self.slot(track);
        if let Some(tx) = &slot.sources {
            let _ = tx.send(Source { consumer, rate });
        }
        slot.stream = Some(stream);
        slot.lost = false;
        slot.app_handles = app_handles;
        if let Some(source) = source {
            let prev = self
                .shared
                .system_source
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .replace(source);
            tracing::info!(?source, "system audio source");
            if prev.is_some_and(|p| p != source) {
                self.shared
                    .emit(CaptureEvent::SystemSourceChanged { source });
            }
        }
        if why == Why::Reopen {
            tracing::info!(track = wav::track_name(track), %device, "audio device reopened");
            self.shared.emit(CaptureEvent::DeviceSwitched {
                track,
                device: Some(device),
            });
        }
    }

    /// Once a second: follow the call app (macOS). Switch from all audio to the app once
    /// it has an audio process, add helper processes that appear, and rebuild the tap
    /// when the output device changes.
    #[cfg(target_os = "macos")]
    fn retarget_system(&mut self) {
        let Some(target) = self.cfg.system_app.clone().filter(|t| !t.is_empty()) else {
            return;
        };
        let slot = &mut self.system;
        if slot.sources.is_none() || slot.lost || slot.stream.is_none() {
            return;
        }
        slot.ticks = slot.ticks.wrapping_add(1);
        let ticks = slot.ticks;
        match macos::app_backend() {
            SystemBackend::ProcessTap => {
                let stale = matches!(&slot.stream, Some(capture::LiveStream::Tap(t)) if t.stale());
                let resolved = resolve_tap_target(&target);
                let next = match target::retarget(slot.app_handles.as_deref(), &resolved) {
                    target::Retarget::To(h) => Some(h),
                    target::Retarget::Keep if stale && !resolved.is_empty() => Some(resolved),
                    target::Retarget::Keep => None,
                };
                if stale && next.is_none() {
                    // The tap's device is gone and the app has no process left to tap.
                    match self.open_loopback() {
                        Ok(o) => self.install(Track::System, o, Why::Retarget),
                        Err(e) => tracing::warn!(error = %e, "system audio fallback failed"),
                    }
                    return;
                }
                let Some(objects) = next else { return };
                if self.system.failed.as_ref() == Some(&objects) && !stale {
                    return;
                }
                match self.open_tap(&objects) {
                    Ok(o) => {
                        self.system.failed = None;
                        self.install(Track::System, o, Why::Retarget);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "per-app audio tap failed; keeping the current source");
                        self.system.failed = Some(objects);
                    }
                }
            }
            SystemBackend::ScreenCapture => {
                // Upgrade "all audio" to the app once it shows up (every 5 s: listing
                // shareable content is heavier than a Core Audio query).
                if slot.app_handles.is_some() || !ticks.is_multiple_of(5) {
                    return;
                }
                if let Ok(pids) = macos::sck::resolve(&target)
                    && !pids.is_empty()
                {
                    match self.open_sck(Some(&target)) {
                        Ok(o) => self.install(Track::System, o, Why::Retarget),
                        Err(e) => tracing::warn!(error = %e, "ScreenCaptureKit retarget failed"),
                    }
                }
            }
            SystemBackend::Loopback => {}
        }
    }

    fn reopen(&mut self, track: Track) {
        let first_failure = {
            let slot = self.slot(track);
            if slot.sources.is_none() {
                return; // track not being recorded
            }
            slot.stream = None; // stop the dead stream before opening a new one
            !slot.lost
        };
        if let Err(e) = self.open(track, Why::Reopen) {
            self.slot(track).lost = true;
            if first_failure {
                tracing::warn!(track = wav::track_name(track), error = %e, "audio device lost");
                self.shared.emit(CaptureEvent::TrackLost {
                    track,
                    message: e.to_string(),
                });
            }
        }
    }

    fn run(mut self, cmds: Receiver<Cmd>) {
        loop {
            match cmds.recv_timeout(REOPEN_RETRY) {
                Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Cmd::Reopen { track, generation }) => {
                    if self.slot(track).generation == generation {
                        self.reopen(track);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    for track in [Track::Mic, Track::System] {
                        if self.slot(track).lost {
                            self.reopen(track);
                        }
                    }
                    #[cfg(target_os = "macos")]
                    self.retarget_system();
                }
            }
        }
        // Dropping the streams abandons the rings; dropping the senders tells the
        // writers no further sources will come, so they drain and finish.
    }
}

/// Core Audio process objects of the call app; falls back to translating its pid
/// (`kAudioHardwarePropertyTranslatePIDToProcessObject`) when the list has no match.
#[cfg(target_os = "macos")]
fn resolve_tap_target(t: &AppTarget) -> Vec<u32> {
    let mut objects = target::resolve(t, &macos::CoreAudioProcesses, std::process::id() as i32);
    if objects.is_empty()
        && let Some(o) = t.pid.and_then(macos::process_object)
    {
        objects.push(o);
    }
    objects
}

struct Shared {
    clock: level::Clock,
    paused: Arc<AtomicBool>,
    stopped: AtomicBool,
    mic_meter: level::Meter,
    sys_meter: level::Meter,
    last_error: Mutex<Option<String>>,
    events: broadcast::Sender<CaptureEvent>,
    /// What the system track records right now (`None` = no system track).
    system_source: Mutex<Option<SystemSource>>,
}

impl Shared {
    fn new(clock: level::Clock) -> Self {
        Self {
            clock,
            paused: Arc::new(AtomicBool::new(false)),
            stopped: AtomicBool::new(false),
            mic_meter: level::Meter::default(),
            sys_meter: level::Meter::default(),
            last_error: Mutex::new(None),
            events: broadcast::channel(EVENT_CAPACITY).0,
            system_source: Mutex::new(None),
        }
    }

    fn emit(&self, event: CaptureEvent) {
        let _ = self.events.send(event); // no receivers is fine
    }
}

struct Running {
    cmd_tx: mpsc::Sender<Cmd>,
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

    /// Live problems during the recording (device lost/switched, disk full).
    /// Subscribe right after [`Recorder::start`]; events sent before subscribing are not
    /// replayed ([`Self::last_stream_error`] keeps the latest stream error).
    pub fn events(&self) -> broadcast::Receiver<CaptureEvent> {
        self.shared.events.subscribe()
    }

    /// Why system audio isn't being recorded (`None` = it is, or it wasn't requested).
    pub fn system_error(&self) -> Option<&str> {
        self.system_error.as_deref()
    }

    /// What the system track is recording right now: only the call app or all system
    /// audio, and through which OS API. `None` = no system track.
    pub fn system_source(&self) -> Option<SystemSource> {
        if self.system_error.is_some() {
            return None;
        }
        *self
            .shared
            .system_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
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
    /// If a writer failed (e.g. disk full), whatever was written is still stitched and
    /// returned (the failure was already sent as [`CaptureEvent::WriteFailed`]); it is an
    /// error only when no microphone audio exists at all.
    pub fn stop(&self) -> Result<RecordedTracks> {
        let (mic_res, sys_res) = self.stop_capture()?;
        if let Some(Err(e)) = &sys_res {
            tracing::warn!(error = %e, "system track writer failed");
        }
        let mic_stitched = wav::stitch(&self.out_dir, Track::Mic);
        let sys_stitched = match sys_res {
            Some(_) => wav::stitch(&self.out_dir, Track::System),
            None => Ok(None),
        };
        finish_tracks(mic_res.map(|_| ()), mic_stitched, sys_stitched)
    }

    /// Stop device and writer threads without stitching the captured chunks.
    pub fn discard(&self) -> Result<()> {
        // Only cleanup matters: the caller removes the chunks even if a writer failed.
        self.stop_capture().map(|_| ())
    }

    fn stop_capture(&self) -> Result<(Result<u64>, Option<Result<u64>>)> {
        let running = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .ok_or(AudioError::Stopped)?;
        self.shared.clock.freeze();
        self.shared.stopped.store(true, Ordering::SeqCst);
        let _ = running.cmd_tx.send(Cmd::Stop);
        let _ = running.capture_thread.join(); // drops streams -> writers drain and finish
        let _ = running.level_thread.join();
        let join = |h: JoinHandle<Result<u64>>| {
            h.join()
                .unwrap_or_else(|_| Err(AudioError::Device("writer thread panicked".into())))
        };
        let mic_res = join(running.mic_writer);
        let sys_res = running.sys_writer.map(join);
        Ok((mic_res, sys_res))
    }
}

/// What [`RecordingHandle::stop`] returns, given how the mic writer ended and what could
/// be stitched. A writer that failed partway (disk full 40 minutes in) still leaves the
/// audio written before the failure: keep it, so the meeting is transcribed instead of
/// being marked failed with its audio on disk.
fn finish_tracks(
    mic_writer: Result<()>,
    mic_stitched: Result<Option<(PathBuf, u64)>>,
    sys_stitched: Result<Option<(PathBuf, u64)>>,
) -> Result<RecordedTracks> {
    let mic = match (mic_writer, mic_stitched) {
        (_, Ok(Some((path, samples)))) if samples > 0 => (path, samples),
        (Err(e), _) => return Err(e),
        (Ok(()), Err(e)) => return Err(e),
        (Ok(()), Ok(_)) => {
            return Err(AudioError::Device(
                "no microphone audio was captured".into(),
            ));
        }
    };
    let system = match sys_stitched {
        Ok(s) => s.map(|(p, _)| p),
        Err(e) => {
            // The mic track alone is still a usable recording.
            tracing::warn!(error = %e, "stitching the system track failed");
            None
        }
    };
    Ok(RecordedTracks {
        mic: mic.0,
        system,
        duration_ms: mic.1 * 1000 / SAMPLE_RATE as u64,
    })
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
    sources: Receiver<Source>,
    config: &CaptureConfig,
    shared: Arc<Shared>,
    tap: Option<Tap>,
) -> Result<JoinHandle<Result<u64>>> {
    let mut writer = wav::ChunkWriter::new(&config.out_dir, track, config.chunk_secs)?;
    let name = format!("minutae-writer-{}", wav::track_name(track));
    Ok(std::thread::Builder::new().name(name).spawn(move || {
        let mut tap = tap;
        match run_writer(track, &mut writer, &sources, &shared, &mut tap) {
            Ok(()) => writer.finish(),
            Err(e) => {
                tracing::error!(track = wav::track_name(track), error = %e, "writing audio failed");
                shared.emit(CaptureEvent::WriteFailed {
                    track,
                    message: e.to_string(),
                });
                // Close the open chunk if possible so stitching gets everything written.
                let _ = writer.finish();
                Err(e)
            }
        }
    })?)
}

/// Writer loop: drain the current ring (switching to new ones as devices are reopened),
/// resample to 16 kHz, meter, keep the track aligned with the clock, write chunks.
/// Returns when no further source can arrive and the last ring is drained.
fn run_writer(
    track: Track,
    writer: &mut wav::ChunkWriter,
    sources: &Receiver<Source>,
    shared: &Shared,
    tap: &mut Option<Tap>,
) -> Result<()> {
    let meter = match track {
        Track::Mic => &shared.mic_meter,
        Track::System => &shared.sys_meter,
    };
    let Ok(mut src) = sources.recv() else {
        return Ok(());
    };
    let mut resampler = Resampler::new(src.rate, SAMPLE_RATE);
    let mut out = Vec::with_capacity(8192);
    let mut to_drop = 0u64;
    let mut more_sources = true;
    loop {
        // Decide before writing, counting what is still queued, so a writer that merely
        // fell behind is never padded (which would offset the track for good).
        if !shared.paused.load(Ordering::Relaxed) {
            let queued = src.consumer.slots() as u64 * SAMPLE_RATE as u64 / src.rate.max(1) as u64;
            match align(
                writer.total_samples(),
                queued,
                to_drop,
                shared.clock.active_samples(SAMPLE_RATE),
                ALIGN_SLACK_SAMPLES,
            ) {
                Align::Keep => {}
                Align::Pad(n) => writer.write_silence(n)?,
                Align::Drop(n) => to_drop += n,
            }
        }
        let got = drain(&mut src.consumer, &mut resampler, &mut out);
        write_block(writer, meter, tap, &out, &mut to_drop)?;
        if more_sources {
            match sources.try_recv() {
                Ok(next) => {
                    // The old stream is gone; finish its ring, then continue on the new one.
                    drain(&mut src.consumer, &mut resampler, &mut out);
                    write_block(writer, meter, tap, &out, &mut to_drop)?;
                    out.clear();
                    resampler.flush(&mut out);
                    write_block(writer, meter, tap, &out, &mut to_drop)?;
                    resampler = Resampler::new(next.rate, SAMPLE_RATE);
                    src = next;
                    continue;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => more_sources = false,
            }
        }
        if !more_sources && src.consumer.is_abandoned() && src.consumer.is_empty() {
            break;
        }
        if got == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    out.clear();
    resampler.flush(&mut out);
    write_block(writer, meter, tap, &out, &mut to_drop)?;
    // Align the track's end with the wall clock (frozen at stop).
    let expected = shared.clock.active_samples(SAMPLE_RATE);
    let written = writer.total_samples();
    if written < expected {
        writer.write_silence(expected - written)?;
    }
    Ok(())
}

/// Move everything readable from `ring` through the resampler into `out` (cleared first).
/// Returns the number of input samples consumed.
fn drain(ring: &mut rtrb::Consumer<f32>, resampler: &mut Resampler, out: &mut Vec<f32>) -> usize {
    out.clear();
    let n = ring.slots();
    if n == 0 {
        return 0;
    }
    let Ok(chunk) = ring.read_chunk(n) else {
        return 0;
    };
    let (a, b) = chunk.as_slices();
    resampler.process(a, out);
    resampler.process(b, out);
    chunk.commit_all();
    n
}

/// Meter `block` and write it, skipping up to `to_drop` leading samples.
fn write_block(
    writer: &mut wav::ChunkWriter,
    meter: &level::Meter,
    tap: &mut Option<Tap>,
    block: &[f32],
    to_drop: &mut u64,
) -> Result<()> {
    if block.is_empty() {
        return Ok(());
    }
    meter.add(block);
    let skip = (*to_drop).min(block.len() as u64);
    *to_drop -= skip;
    let block = &block[skip as usize..];
    if let Some(tap) = tap.as_mut()
        && !block.is_empty()
    {
        tap(writer.total_samples(), block);
    }
    writer.write(block)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Keep,
    /// Insert this much silence now (a real gap: nothing was captured).
    Pad(u64),
    /// Discard this many upcoming samples (the track runs ahead of the clock).
    Drop(u64),
}

/// Decide how to keep a track aligned with the recording clock. `written` samples are on
/// disk, `queued` are captured but not yet written, `pending_drop` are already scheduled
/// to be discarded, and `expected` is where the clock says the track should be. Only a
/// difference beyond `slack` is corrected, back to within `slack / 2`, so normal jitter
/// (and samples merely waiting in the queue after a writer stall) cause no change.
fn align(written: u64, queued: u64, pending_drop: u64, expected: u64, slack: u64) -> Align {
    let have = (written + queued).saturating_sub(pending_drop);
    if have + slack < expected {
        Align::Pad(expected - slack / 2 - have)
    } else if have > expected + slack {
        Align::Drop(have - expected - slack / 2)
    } else {
        Align::Keep
    }
}

/// Recover a meeting directory after a crash: stitch leftover chunks into `mic.wav` /
/// `system.wav` (finishing an interrupted stitch), or, if there are no chunks, report the
/// final files that already exist. Duration comes from the WAV data. Returns `None` if
/// there is no audio at all.
pub fn recover(out_dir: &Path) -> Result<Option<RecordedTracks>> {
    let mic = recover_track(out_dir, Track::Mic)?;
    let system = recover_track(out_dir, Track::System)?;
    if mic.is_none() && system.is_none() {
        return Ok(None);
    }
    let mic_samples = mic.as_ref().map(|(_, n)| *n).unwrap_or(0);
    let samples = if mic_samples > 0 {
        mic_samples
    } else {
        system.as_ref().map(|(_, n)| *n).unwrap_or(0)
    };
    Ok(Some(RecordedTracks {
        // System-only leftovers: still report, with the (missing) mic path.
        mic: mic
            .map(|(p, _)| p)
            .unwrap_or_else(|| wav::final_path(out_dir, Track::Mic)),
        system: system.map(|(p, _)| p),
        duration_ms: samples * 1000 / SAMPLE_RATE as u64,
    }))
}

fn recover_track(out_dir: &Path, track: Track) -> Result<Option<(PathBuf, u64)>> {
    if let Some(v) = wav::stitch(out_dir, track)? {
        return Ok(Some(v));
    }
    let path = wav::final_path(out_dir, track);
    if !path.exists() {
        return Ok(None);
    }
    match wav::wav_samples(&path) {
        Ok(n) => Ok(Some((path, n))),
        Err(e) => {
            tracing::warn!(?path, error = %e, "ignoring unreadable recording");
            Ok(None)
        }
    }
}

/// True if `out_dir` holds chunks from an unfinished recording (or an interrupted stitch).
pub fn needs_recovery(out_dir: &Path) -> bool {
    let dir = out_dir.join(wav::CHUNKS_DIR);
    [Track::Mic, Track::System].iter().any(|&t| {
        wav::has_stitch_marker(out_dir, t)
            || wav::list_chunks(&dir, t)
                .map(|c| !c.is_empty())
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use level::tests::{advance, manual_time};

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "minutae-lib-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn handle_is_send_sync() {
        fn assert<T: Send + Sync>() {}
        assert::<RecordingHandle>();
        assert::<Recorder>();
    }

    #[test]
    fn align_pads_only_real_gaps_and_drops_when_ahead() {
        let slack = ALIGN_SLACK_SAMPLES; // 3200
        // Within slack either way: nothing.
        assert_eq!(align(10_000, 0, 0, 12_000, slack), Align::Keep);
        assert_eq!(align(14_000, 0, 0, 12_000, slack), Align::Keep);
        // Real gap: pad back to within slack/2.
        assert_eq!(align(4_000, 0, 0, 12_000, slack), Align::Pad(6_400));
        // Writer stalled but the samples are queued: no padding (this used to
        // offset the track permanently).
        assert_eq!(align(4_000, 8_000, 0, 12_000, slack), Align::Keep);
        // Running ahead (e.g. padded earlier, or a fast device clock): drop.
        assert_eq!(align(12_000, 8_000, 0, 12_000, slack), Align::Drop(6_400));
        // Already-scheduled drops are accounted for.
        assert_eq!(align(12_000, 8_000, 6_400, 12_000, slack), Align::Keep);
    }

    fn feed(samples: &[f32], rate: u32) -> (Source, rtrb::Producer<f32>) {
        let (mut p, c) = rtrb::RingBuffer::new(samples.len().max(1) + 16);
        for &s in samples {
            p.push(s).unwrap();
        }
        (Source { consumer: c, rate }, p)
    }

    #[test]
    fn writer_switches_sources_and_pads_to_frozen_clock() {
        let d = tmpdir("writer");
        let (src, t) = manual_time();
        let shared = Shared::new(level::Clock::with_source(src));
        let mut events = shared.events.subscribe();
        let mut w = wav::ChunkWriter::new(&d, Track::Mic, 10).unwrap();
        // Paused: no mid-stream alignment, only the final pad to the clock.
        shared.paused.store(true, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        // 0.5 s at 16 kHz, then (device reopened) 0.5 s at 48 kHz.
        let (a, pa) = feed(&vec![0.25; 8_000], 16_000);
        let (b, pb) = feed(&vec![0.5; 24_000], 48_000);
        tx.send(a).unwrap();
        tx.send(b).unwrap();
        drop((tx, pa, pb)); // capture finished
        advance(&t, 1_500); // 1.5 s of active time: the last 0.5 s is a real gap
        shared.clock.freeze();
        run_writer(Track::Mic, &mut w, &rx, &shared, &mut None).unwrap();
        let total = w.finish().unwrap();
        assert_eq!(total, 24_000, "padded to the frozen clock");
        let (path, n) = wav::stitch(&d, Track::Mic).unwrap().unwrap();
        assert_eq!(n, 24_000);
        let got: Vec<i16> = hound::WavReader::open(&path)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap())
            .collect();
        // Audio from both sources is present, in order, followed by silence.
        assert!((got[4_000] as f32 / i16::MAX as f32 - 0.25).abs() < 0.01);
        assert!((got[12_000] as f32 / i16::MAX as f32 - 0.5).abs() < 0.02);
        assert_eq!(got[23_000], 0);
        assert!(events.try_recv().is_err(), "no problems reported");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Count sign changes (a sine of `f` Hz has `2 f` per second).
    fn zero_crossings(x: &[i16]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count()
    }

    /// Full capture path without a device: interleaved stereo 48 kHz frames delivered in
    /// realtime-sized callbacks -> downmix into the ring -> writer -> 16 kHz resampler ->
    /// chunks -> stitch. Checks length, level and frequency (zero crossings) per channel
    /// layout and a chirp, so channel handling / rate / dropped-or-duplicated blocks show up.
    #[test]
    fn stereo_48k_through_capture_ring_writer_resampler() {
        let rate = 48_000u32;
        let secs = 2usize;
        // (left Hz, right Hz or None for silence, expected output Hz, expected RMS)
        let cases: [(f32, Option<f32>, f32, f32); 3] = [
            (
                440.0,
                Some(440.0),
                440.0,
                0.5 * std::f32::consts::FRAC_1_SQRT_2,
            ),
            (
                1_000.0,
                None,
                1_000.0,
                0.25 * std::f32::consts::FRAC_1_SQRT_2,
            ),
            (
                3_000.0,
                Some(3_000.0),
                3_000.0,
                0.5 * std::f32::consts::FRAC_1_SQRT_2,
            ),
        ];
        for (fl, fr, want_hz, want_rms) in cases {
            let d = tmpdir("stereo48k");
            let (src, t) = manual_time();
            let shared = Shared::new(level::Clock::with_source(src));
            let n = rate as usize * secs;
            let mut interleaved = Vec::with_capacity(n * 2);
            for i in 0..n {
                let ph = 2.0 * std::f32::consts::PI * i as f32 / rate as f32;
                interleaved.push(0.5 * (fl * ph).sin());
                interleaved.push(fr.map(|f| 0.5 * (f * ph).sin()).unwrap_or(0.0));
            }
            let (mut producer, consumer) = rtrb::RingBuffer::new(capture::ring_capacity(rate));
            let (tx, rx) = mpsc::channel();
            tx.send(Source { consumer, rate }).unwrap();
            drop(tx);
            // Realtime-sized callbacks (512 frames), then the writer drains the ring.
            for block in interleaved.chunks(512 * 2) {
                capture::push_downmixed(&mut producer, block, 2);
            }
            drop(producer);
            advance(&t, (secs * 1000) as u64);
            shared.clock.freeze();
            let mut w = wav::ChunkWriter::new(&d, Track::System, 1).unwrap();
            run_writer(Track::System, &mut w, &rx, &shared, &mut None).unwrap();
            w.finish().unwrap();
            let (path, total) = wav::stitch(&d, Track::System).unwrap().unwrap();
            assert_eq!(total, 16_000 * secs as u64, "{fl} Hz: duration");
            let got: Vec<i16> = hound::WavReader::open(&path)
                .unwrap()
                .samples::<i16>()
                .map(|s| s.unwrap())
                .collect();
            // Skip the resampler's start-up; measure one full second.
            let mid = &got[8_000..24_000];
            let hz = zero_crossings(mid) as f32 / 2.0;
            assert!((hz - want_hz).abs() <= 2.0, "{fl} Hz in, {hz} Hz out");
            let rms = level::rms(
                &mid.iter()
                    .map(|&s| s as f32 / i16::MAX as f32)
                    .collect::<Vec<_>>(),
            );
            assert!(
                (rms - want_rms).abs() < 0.01,
                "{fl} Hz: rms {rms}, want {want_rms}"
            );
            std::fs::remove_dir_all(d).unwrap();
        }

        // A 200 Hz -> 6 kHz chirp keeps its shape: early and late windows differ as expected.
        let d = tmpdir("chirp");
        let (src, t) = manual_time();
        let shared = Shared::new(level::Clock::with_source(src));
        let n = rate as usize * secs;
        let (f0, f1) = (200.0f32, 6_000.0f32);
        let k = (f1 - f0) / secs as f32;
        let mut interleaved = Vec::with_capacity(n * 2);
        for i in 0..n {
            let tt = i as f32 / rate as f32;
            let v = 0.5 * (2.0 * std::f32::consts::PI * (f0 * tt + 0.5 * k * tt * tt)).sin();
            interleaved.extend([v, v]);
        }
        let (mut producer, consumer) = rtrb::RingBuffer::new(capture::ring_capacity(rate));
        let (tx, rx) = mpsc::channel();
        tx.send(Source { consumer, rate }).unwrap();
        drop(tx);
        for block in interleaved.chunks(480 * 2) {
            capture::push_downmixed(&mut producer, block, 2);
        }
        drop(producer);
        advance(&t, (secs * 1000) as u64);
        shared.clock.freeze();
        let mut w = wav::ChunkWriter::new(&d, Track::System, 10).unwrap();
        run_writer(Track::System, &mut w, &rx, &shared, &mut None).unwrap();
        assert_eq!(w.finish().unwrap(), 16_000 * secs as u64);
        let (path, _) = wav::stitch(&d, Track::System).unwrap().unwrap();
        let got: Vec<i16> = hound::WavReader::open(&path)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap())
            .collect();
        // Instantaneous frequency at window centre t: f0 + k t. 100 ms windows.
        for centre_ms in [250u64, 1_000, 1_750] {
            let c = (centre_ms * 16) as usize;
            let win = &got[c - 800..c + 800];
            let hz = zero_crossings(win) as f32 / 2.0 / 0.1;
            let want = f0 + k * centre_ms as f32 / 1000.0;
            assert!(
                (hz - want).abs() < want * 0.03 + 20.0,
                "chirp at {centre_ms} ms: {hz} Hz, want {want}"
            );
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tap_sees_exactly_what_is_written_with_positions() {
        let d = tmpdir("tap");
        let (src, t) = manual_time();
        let shared = Shared::new(level::Clock::with_source(src));
        shared.paused.store(true, Ordering::SeqCst);
        let mut w = wav::ChunkWriter::new(&d, Track::Mic, 10).unwrap();
        let (tx, rx) = mpsc::channel();
        let (a, pa) = feed(&vec![0.25; 8_000], 16_000);
        let (b, pb) = feed(&vec![0.5; 4_000], 16_000);
        tx.send(a).unwrap();
        tx.send(b).unwrap();
        drop((tx, pa, pb));
        advance(&t, 1_000); // final pad of 0.25 s is silence, not tapped
        shared.clock.freeze();
        let seen = Arc::new(Mutex::new(Vec::<(u64, usize, f32)>::new()));
        let s2 = seen.clone();
        let mut tap: Option<Tap> = Some(Box::new(move |start, block: &[f32]| {
            s2.lock().unwrap().push((start, block.len(), block[0]));
        }));
        run_writer(Track::Mic, &mut w, &rx, &shared, &mut tap).unwrap();
        assert_eq!(w.finish().unwrap(), 16_000);
        let seen = seen.lock().unwrap();
        let mut pos = 0;
        for &(start, len, _) in seen.iter() {
            assert_eq!(start, pos, "blocks are contiguous on the timeline");
            pos += len as u64;
        }
        assert_eq!(pos, 12_000);
        assert_eq!(seen.first().unwrap().2, 0.25);
        assert_eq!(seen.last().unwrap().2, 0.5);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn writer_drops_samples_when_ahead_of_clock() {
        let d = tmpdir("ahead");
        let (src, _t) = manual_time();
        let shared = Shared::new(level::Clock::with_source(src));
        shared.clock.freeze(); // the clock says 0 s have elapsed
        let mut w = wav::ChunkWriter::new(&d, Track::System, 10).unwrap();
        // 1 s of audio is waiting: everything beyond slack/2 is dropped instead of
        // shifting the track for the rest of the meeting.
        let (tx, rx) = mpsc::channel();
        let (a, pa) = feed(&vec![0.1; 16_000], 16_000);
        tx.send(a).unwrap();
        drop((tx, pa));
        run_writer(Track::System, &mut w, &rx, &shared, &mut None).unwrap();
        assert_eq!(w.total_samples(), ALIGN_SLACK_SAMPLES / 2);
        w.finish().unwrap();
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn writer_failure_is_reported() {
        let d = tmpdir("diskfull");
        let shared = Arc::new(Shared::new(level::Clock::new()));
        let mut events = shared.events.subscribe();
        let cfg = CaptureConfig::new(d.clone());
        // Make chunk creation fail: replace the chunks dir with a file after the writer
        // was constructed (stands in for disk full / permissions).
        let (tx, rx) = mpsc::channel();
        let h = spawn_writer(Track::Mic, rx, &cfg, shared.clone(), None).unwrap();
        std::fs::remove_dir_all(d.join(wav::CHUNKS_DIR)).unwrap();
        std::fs::write(d.join(wav::CHUNKS_DIR), b"not a dir").unwrap();
        let (a, pa) = feed(&vec![0.1; 1_600], 16_000);
        tx.send(a).unwrap();
        drop((tx, pa));
        assert!(h.join().unwrap().is_err());
        match events.try_recv().unwrap() {
            CaptureEvent::WriteFailed { track, .. } => assert_eq!(track, Track::Mic),
            other => panic!("unexpected {other:?}"),
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn partial_audio_survives_a_writer_failure() {
        let disk_full = || Err(AudioError::Device("disk full".into()));
        let mic = || Ok(Some((PathBuf::from("mic.wav"), 16_000 * 60)));
        // Writer failed after a minute: the minute is kept.
        let t = finish_tracks(disk_full(), mic(), Ok(None)).unwrap();
        assert_eq!((t.mic, t.duration_ms), (PathBuf::from("mic.wav"), 60_000));
        // A system track that couldn't be stitched doesn't sink the mic track.
        let t = finish_tracks(Ok(()), mic(), Err(AudioError::Device("x".into()))).unwrap();
        assert!(t.system.is_none());
        // Nothing written: the writer's error is what the caller sees.
        let e = finish_tracks(disk_full(), Ok(None), Ok(None)).unwrap_err();
        assert!(e.to_string().contains("disk full"), "{e}");
        assert!(finish_tracks(Ok(()), Ok(Some((PathBuf::from("m"), 0))), Ok(None)).is_err());
    }

    #[test]
    fn recover_reports_existing_final_files() {
        let d = tmpdir("finals");
        assert!(recover(&d).unwrap().is_none());
        for (track, n) in [(Track::Mic, 32_000usize), (Track::System, 16_000)] {
            let mut w = hound::WavWriter::create(wav::final_path(&d, track), wav::spec()).unwrap();
            for _ in 0..n {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        }
        let rec = recover(&d).unwrap().expect("final files are reported");
        assert_eq!(rec.mic, d.join("mic.wav"));
        assert_eq!(rec.system, Some(d.join("system.wav")));
        assert_eq!(rec.duration_ms, 2_000);
        assert!(!needs_recovery(&d));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn recover_stitches_leftover_chunks() {
        let d = tmpdir("leftover");
        let mut w = wav::ChunkWriter::new(&d, Track::System, 10).unwrap();
        w.write_silence(4_000).unwrap();
        w.finish().unwrap();
        assert!(needs_recovery(&d));
        let rec = recover(&d).unwrap().unwrap();
        assert!(rec.system.is_some());
        assert_eq!(rec.duration_ms, 250);
        assert!(!needs_recovery(&d));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn capture_event_json_shape() {
        let e = CaptureEvent::DeviceSwitched {
            track: Track::Mic,
            device: Some("USB Mic".into()),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"kind": "deviceSwitched", "track": "mic", "device": "USB Mic"})
        );
    }

    /// Records ~1 s from the real default devices. `cargo test -p minutae-audio -- --ignored`
    #[test]
    #[ignore]
    fn real_device_smoke() {
        let d = tmpdir("real");
        let h = Recorder::new()
            .start(CaptureConfig::new(d.clone()))
            .unwrap();
        let _events = h.events();
        std::thread::sleep(Duration::from_millis(1_200));
        let tracks = h.stop().unwrap();
        assert!(tracks.duration_ms >= 1_000, "{tracks:?}");
        assert!(!needs_recovery(&d));
        std::fs::remove_dir_all(d).unwrap();
    }
}
