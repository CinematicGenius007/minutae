//! Live captions while recording: rolling windows from both tracks, transcribed on a
//! low-priority worker, turned into partial/final caption lines.
//!
//! ```text
//! writer thread ──CaptionFeed (lock-free ring, no alloc)──▶ pump thread (100 ms)
//!     pump: CaptionCore::ingest → fixed windows (4 s, 0.5 s overlap) → bounded queue
//!     worker (low priority): CaptionCore::next_job → Transcriber → CaptionCore::complete
//!                            → Caption events (sink)
//! ```
//!
//! Best effort by design:
//! - The recorder's writer only copies samples into a preallocated ring; when the ring is
//!   full the samples are dropped for captions (never for the recording).
//! - Silent windows are skipped (energy gate), so idle tracks cost nothing.
//! - The final-window queue is bounded; when the machine falls behind the **oldest**
//!   window is dropped, so captions stay current instead of lagging further.
//! - Partials (the line still being spoken) are only transcribed when the queue is empty.
//! - Pausing discards pending audio and queued work.
//!
//! Lines: every window has an `id`. Partials for the line being spoken carry the id that
//! line's window will get, so the final replaces the partial in place. A final with empty
//! text retracts a partial that turned out to be nothing (silence, a duplicate).
//!
//! [`CaptionCore`] is synchronous and deterministic (tests drive it directly);
//! [`CaptionEngine`] wraps it in the pump + worker threads.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use minutae_core::Track;

use crate::{TARGET_SAMPLE_RATE, TranscribeOptions, Transcriber, join_text};

const RATE: u64 = TARGET_SAMPLE_RATE as u64;

/// Tuning for [`CaptionCore`]. All lengths in ms of 16 kHz audio.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptionConfig {
    /// Length of one final window.
    pub window_ms: u32,
    /// Audio shared by consecutive windows (words cut at a boundary appear in both;
    /// the repeat is removed from the later line).
    pub overlap_ms: u32,
    /// A partial needs at least this much audio in the line being spoken…
    pub partial_min_ms: u32,
    /// …and this much more than the previous partial of the same line.
    pub partial_step_ms: u32,
    /// Final windows waiting for the worker. Full = drop the oldest.
    pub queue_capacity: usize,
    /// Windows whose loudest 100 ms frame is below this RMS are skipped as silence.
    pub speech_rms: f32,
    /// Capacity of each track's feed ring (absorbs pump stalls).
    pub ring_ms: u32,
    /// Transcribe with [`TranscribeOptions::short_clip`].
    pub short_clip: bool,
}

impl Default for CaptionConfig {
    fn default() -> Self {
        Self {
            window_ms: 4_000,
            overlap_ms: 500,
            partial_min_ms: 1_200,
            partial_step_ms: 1_000,
            queue_capacity: 3,
            speech_rms: 0.008,
            ring_ms: 30_000,
            short_clip: true,
        }
    }
}

impl CaptionConfig {
    fn samples(ms: u32) -> usize {
        (ms as u64 * RATE / 1000) as usize
    }
    fn window(&self) -> usize {
        Self::samples(self.window_ms).max(1)
    }
    fn hop(&self) -> usize {
        Self::samples(self.window_ms.saturating_sub(self.overlap_ms)).max(1)
    }
}

/// One caption event. Times are on the recording timeline (pauses excluded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caption {
    /// Line id; a later caption with the same id replaces it.
    pub id: u64,
    pub track: Track,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Empty + `is_final` = remove the line.
    pub text: String,
    pub is_final: bool,
}

/// Counters for logs and cost measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptionStats {
    /// Final windows cut from the audio (speech or not).
    pub windows: u64,
    /// Windows skipped as silence.
    pub silent: u64,
    /// Windows dropped because the worker fell behind.
    pub dropped: u64,
    pub finals: u64,
    pub partials: u64,
    pub errors: u64,
    /// Time the worker spent transcribing, and how much audio that covered.
    pub busy_ms: u64,
    pub audio_ms: u64,
    /// Samples lost because a feed ring was full (pump stalled).
    pub overflow_samples: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Final,
    Partial,
}

/// Audio for the worker to transcribe.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: u64,
    pub track: Track,
    pub kind: JobKind,
    /// Sample index (16 kHz, recording timeline) of `samples[0]`.
    pub start: u64,
    pub samples: Vec<f32>,
    /// Partial text currently on screen for this line (finals only).
    shown: Option<String>,
    /// The audio before this window wasn't transcribed (silence, a drop, a gap, a pause),
    /// so there is no overlap to remove.
    fresh: bool,
}

struct Line {
    track: Track,
    /// Audio of the line being spoken; `buf[0]` is at sample `buf_start`.
    buf: Vec<f32>,
    buf_start: u64,
    /// Id the next window of this track will carry (and its partials).
    id: u64,
    shown: Option<String>,
    partial_len: usize,
    /// Raw text of the previous final window, for overlap de-duplication.
    last_final: String,
    /// The next window follows audio that wasn't transcribed (see [`Job::fresh`]).
    fresh_next: bool,
}

/// Synchronous caption state machine. See the module docs.
pub struct CaptionCore {
    cfg: CaptionConfig,
    lines: [Line; 2],
    queue: VecDeque<Job>,
    next_id: u64,
    paused: bool,
    stats: CaptionStats,
}

fn idx(track: Track) -> usize {
    match track {
        Track::Mic => 0,
        Track::System => 1,
    }
}

impl CaptionCore {
    pub fn new(cfg: CaptionConfig) -> Self {
        let line = |track, id| Line {
            track,
            buf: Vec::new(),
            buf_start: 0,
            id,
            shown: None,
            partial_len: 0,
            last_final: String::new(),
            fresh_next: true,
        };
        Self {
            cfg,
            lines: [line(Track::Mic, 1), line(Track::System, 2)],
            queue: VecDeque::new(),
            next_id: 3,
            paused: false,
            stats: CaptionStats::default(),
        }
    }

    pub fn config(&self) -> &CaptionConfig {
        &self.cfg
    }

    pub fn stats(&self) -> CaptionStats {
        self.stats
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    fn new_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add `samples` of `track` starting at sample `start` (16 kHz recording timeline).
    /// Returns captions to emit right away (lines closed by a drop or a gap).
    pub fn ingest(&mut self, track: Track, start: u64, samples: &[f32]) -> Vec<Caption> {
        let mut out = Vec::new();
        if self.paused || samples.is_empty() {
            return out;
        }
        let window = self.cfg.window();
        let hop = self.cfg.hop();
        let i = idx(track);
        {
            let line = &mut self.lines[i];
            if line.buf.is_empty() {
                line.buf_start = start;
            }
        }
        let end = self.lines[i].buf_start + self.lines[i].buf.len() as u64;
        let (start, samples) = if start < end {
            // Already have part of this (shouldn't happen): keep only the new tail.
            let skip = ((end - start) as usize).min(samples.len());
            (end, &samples[skip..])
        } else {
            (start, samples)
        };
        if start > end {
            let gap = (start - end) as usize;
            if gap < window {
                let line = &mut self.lines[i];
                line.buf.resize(line.buf.len() + gap, 0.0);
            } else {
                // A long gap: close what we have and start over at `start`.
                self.close_line(i, &mut out);
                self.lines[i].buf_start = start;
            }
        }
        self.lines[i].buf.extend_from_slice(samples);
        while self.lines[i].buf.len() >= window {
            let samples = self.lines[i].buf[..window].to_vec();
            self.cut_window(i, samples, &mut out);
            let line = &mut self.lines[i];
            line.buf.drain(..hop);
            line.buf_start += hop as u64;
            line.partial_len = line.partial_len.saturating_sub(hop);
        }
        out
    }

    /// Turn the line's buffer into a final job (if it holds speech) and start a new line.
    fn close_line(&mut self, i: usize, out: &mut Vec<Caption>) {
        let samples = std::mem::take(&mut self.lines[i].buf);
        if samples.len() >= CaptionConfig::samples(self.cfg.partial_min_ms) {
            self.cut_window(i, samples, out);
        } else if let Some(c) = self.retire_shown(i) {
            out.push(c);
        }
        let line = &mut self.lines[i];
        line.buf.clear();
        line.partial_len = 0;
        line.fresh_next = true;
    }

    /// The current line ends without a window: keep its partial text as final.
    fn retire_shown(&mut self, i: usize) -> Option<Caption> {
        let id = self.new_id();
        let line = &mut self.lines[i];
        let old = std::mem::replace(&mut line.id, id);
        line.partial_len = 0;
        line.shown.take().map(|text| Caption {
            id: old,
            track: line.track,
            start_ms: line.buf_start * 1000 / RATE,
            end_ms: (line.buf_start + line.buf.len() as u64) * 1000 / RATE,
            text,
            is_final: true,
        })
    }

    fn cut_window(&mut self, i: usize, samples: Vec<f32>, out: &mut Vec<Caption>) {
        self.stats.windows += 1;
        let next = self.new_id();
        let speech = has_speech(&samples, self.cfg.speech_rms);
        let line = &mut self.lines[i];
        let id = std::mem::replace(&mut line.id, next);
        let shown = line.shown.take();
        line.partial_len = 0;
        let job = Job {
            id,
            track: line.track,
            kind: JobKind::Final,
            start: line.buf_start,
            samples,
            shown,
            fresh: std::mem::take(&mut line.fresh_next),
        };
        if !speech {
            self.stats.silent += 1;
            // The next window starts fresh. Not clearing `last_final` here: windows still
            // queued need it, and they complete in order.
            line.fresh_next = true;
            if let Some(c) = finalize_shown(&job) {
                out.push(c);
            }
            return;
        }
        if self.queue.len() >= self.cfg.queue_capacity.max(1)
            && let Some(old) = self.queue.pop_front()
        {
            self.stats.dropped += 1;
            tracing::debug!(target: "minutae_asr::captions", id = old.id, "caption window dropped (behind)");
            if let Some(c) = finalize_shown(&old) {
                out.push(c);
            }
            // Its successor has no transcribed predecessor to de-duplicate against.
            match self.queue.iter_mut().find(|q| q.track == old.track) {
                Some(next) => next.fresh = true,
                None if old.track == job.track => {
                    self.queue.push_back(Job { fresh: true, ..job });
                    return;
                }
                None => self.lines[idx(old.track)].fresh_next = true,
            }
        }
        self.queue.push_back(job);
    }

    /// Next piece of work: queued final windows first (oldest first); when there are
    /// none, a partial of the line being spoken on the track with the most new audio.
    pub fn next_job(&mut self) -> Option<Job> {
        if let Some(j) = self.queue.pop_front() {
            return Some(j);
        }
        if self.paused {
            return None;
        }
        let min = CaptionConfig::samples(self.cfg.partial_min_ms);
        let step = CaptionConfig::samples(self.cfg.partial_step_ms);
        let speech_rms = self.cfg.speech_rms;
        let line = self
            .lines
            .iter_mut()
            .filter(|l| l.buf.len() >= min && l.buf.len() >= l.partial_len + step)
            .filter(|l| has_speech(&l.buf[l.partial_len.min(l.buf.len())..], speech_rms))
            .max_by_key(|l| l.buf.len() - l.partial_len)?;
        line.partial_len = line.buf.len();
        Some(Job {
            id: line.id,
            track: line.track,
            kind: JobKind::Partial,
            start: line.buf_start,
            samples: line.buf.clone(),
            shown: None,
            fresh: line.fresh_next,
        })
    }

    /// Record the worker's result for `job`. Returns the caption to emit, if any.
    pub fn complete(&mut self, job: Job, text: &str, busy: Duration) -> Option<Caption> {
        self.stats.busy_ms += busy.as_millis() as u64;
        self.stats.audio_ms += job.samples.len() as u64 * 1000 / RATE;
        let start_ms = job.start * 1000 / RATE;
        let end_ms = (job.start + job.samples.len() as u64) * 1000 / RATE;
        let line = &mut self.lines[idx(job.track)];
        match job.kind {
            JobKind::Final => {
                self.stats.finals += 1;
                let text = text.trim();
                let prev = if job.fresh {
                    ""
                } else {
                    line.last_final.as_str()
                };
                let new = dedupe_overlap(prev, text);
                line.last_final = text.to_string();
                if new.is_empty() && job.shown.is_none() {
                    return None;
                }
                Some(Caption {
                    id: job.id,
                    track: job.track,
                    start_ms,
                    end_ms,
                    text: new,
                    is_final: true,
                })
            }
            JobKind::Partial => {
                let prev = if job.fresh {
                    ""
                } else {
                    line.last_final.as_str()
                };
                let new = dedupe_overlap(prev, text.trim());
                if new.is_empty() {
                    return None;
                }
                if line.id == job.id {
                    line.shown = Some(new.clone());
                } else {
                    // The window was cut while this partial ran; it still shows until then.
                    // No queued window = the line is closed (pause, drop): don't resurrect it.
                    let q = self.queue.iter_mut().find(|q| q.id == job.id)?;
                    q.shown = Some(new.clone());
                }
                self.stats.partials += 1;
                Some(Caption {
                    id: job.id,
                    track: job.track,
                    start_ms,
                    end_ms,
                    text: new,
                    is_final: false,
                })
            }
        }
    }

    /// The worker failed on `job`: count it and settle any partial on screen.
    pub fn fail(&mut self, job: Job) -> Option<Caption> {
        self.stats.errors += 1;
        match job.kind {
            JobKind::Final => finalize_shown(&job),
            JobKind::Partial => None,
        }
    }

    /// Pausing drops queued windows and pending audio; partials on screen become final.
    /// Resuming starts fresh lines.
    pub fn set_paused(&mut self, paused: bool) -> Vec<Caption> {
        let mut out = Vec::new();
        if paused == self.paused {
            return out;
        }
        self.paused = paused;
        if paused {
            for job in std::mem::take(&mut self.queue) {
                out.extend(finalize_shown(&job));
            }
            for i in 0..self.lines.len() {
                out.extend(self.retire_shown(i));
                let line = &mut self.lines[i];
                line.buf.clear();
                line.last_final.clear();
                line.fresh_next = true;
            }
        }
        out
    }

    pub fn note_overflow(&mut self, samples: u64) {
        self.stats.overflow_samples += samples;
    }
}

fn finalize_shown(job: &Job) -> Option<Caption> {
    job.shown.as_ref().map(|text| Caption {
        id: job.id,
        track: job.track,
        start_ms: job.start * 1000 / RATE,
        end_ms: (job.start + job.samples.len() as u64) * 1000 / RATE,
        text: text.clone(),
        is_final: true,
    })
}

/// True if any 100 ms frame of `samples` is louder than `threshold` RMS.
pub fn has_speech(samples: &[f32], threshold: f32) -> bool {
    let frame = (RATE / 10) as usize;
    samples.chunks(frame).any(|f| {
        let sum: f32 = f.iter().map(|x| x * x).sum();
        (sum / f.len().max(1) as f32).sqrt() >= threshold
    })
}

/// Words too common to treat a single repeat as window overlap.
const FILLER: &[&str] = &[
    "the", "and", "but", "for", "you", "are", "was", "not", "its", "our", "has", "had", "can",
    "she", "him", "her", "his", "all", "any", "yes", "now", "too", "out", "who", "how", "why",
];

fn norm(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Remove from the start of `next` the words that repeat the end of `prev` (the audio
/// overlap between consecutive windows). Matching ignores case and punctuation; a
/// single-word match doesn't count for very common short words (avoids eating "the").
pub fn dedupe_overlap(prev: &str, next: &str) -> String {
    let p: Vec<String> = prev.split_whitespace().map(norm).collect();
    let words: Vec<&str> = next.split_whitespace().collect();
    let n: Vec<String> = words.iter().map(|w| norm(w)).collect();
    let max = p.len().min(n.len()).min(8);
    let mut cut = 0;
    for k in (1..=max).rev() {
        if p[p.len() - k..] == n[..k] && n[..k].iter().all(|w| !w.is_empty()) {
            if k == 1 && (n[0].chars().count() < 3 || FILLER.contains(&n[0].as_str())) {
                continue;
            }
            cut = k;
            break;
        }
    }
    words[cut..].join(" ")
}

// ---------------------------------------------------------------------------------------
// Threads

/// Called by the engine for every caption (from its threads; must not block for long).
pub type CaptionSink = Arc<dyn Fn(Caption) + Send + Sync>;

/// Writer-thread end of one track's caption ring. [`CaptionFeed::push`] never blocks or
/// allocates: it copies into a preallocated lock-free ring and drops what doesn't fit.
pub struct CaptionFeed {
    producer: rtrb::Producer<f32>,
    /// Sample index the next push is expected at.
    next: Option<u64>,
    overflow: Arc<AtomicU64>,
}

impl CaptionFeed {
    /// `start` is the sample index (16 kHz recording timeline) of `samples[0]`.
    /// Gaps (padded silence in the recording) are filled with zeros.
    pub fn push(&mut self, start: u64, samples: &[f32]) {
        let next = self.next.unwrap_or(start);
        let samples = if start < next {
            &samples[((next - start) as usize).min(samples.len())..]
        } else {
            if start > next {
                let gap = (start - next) as usize;
                let wrote = self.fill(gap, std::iter::repeat(0.0));
                if wrote < gap {
                    self.overflow
                        .fetch_add((gap - wrote) as u64, Ordering::Relaxed);
                }
            }
            samples
        };
        let wrote = self.fill(samples.len(), samples.iter().copied());
        if wrote < samples.len() {
            self.overflow
                .fetch_add((samples.len() - wrote) as u64, Ordering::Relaxed);
        }
        self.next = Some(start.max(next) + samples.len() as u64);
    }

    fn fill(&mut self, n: usize, it: impl Iterator<Item = f32>) -> usize {
        let n = n.min(self.producer.slots());
        if n == 0 {
            return 0;
        }
        match self.producer.write_chunk_uninit(n) {
            Ok(chunk) => chunk.fill_from_iter(it),
            Err(_) => 0,
        }
    }
}

struct FeedReader {
    track: Track,
    consumer: rtrb::Consumer<f32>,
    overflow: Arc<AtomicU64>,
    seen_overflow: u64,
    /// Timeline position of the next sample in the ring (`None` until the first one).
    pos: u64,
}

struct Shared {
    core: Mutex<CaptionCore>,
    wake: Condvar,
    stop: Arc<AtomicBool>,
    enabled: AtomicBool,
}

/// Owns the pump and worker threads. Stop with [`CaptionEngine::stop`] (or drop).
pub struct CaptionEngine {
    shared: Arc<Shared>,
    sink: CaptionSink,
    threads: Vec<JoinHandle<()>>,
}

const PUMP_EVERY: Duration = Duration::from_millis(100);

impl CaptionEngine {
    /// Start the threads. Returns the engine and one feed per track (mic, system) for
    /// the recorder's writers. `opts` is used for every transcription (its `cancel`
    /// is replaced by the engine's stop flag).
    pub fn start(
        transcriber: Arc<dyn Transcriber>,
        mut opts: TranscribeOptions,
        cfg: CaptionConfig,
        sink: CaptionSink,
    ) -> std::io::Result<(Self, CaptionFeed, CaptionFeed)> {
        let ring = CaptionConfig::samples(cfg.ring_ms).max(RATE as usize);
        let stop = Arc::new(AtomicBool::new(false));
        opts.cancel = Some(stop.clone());
        opts.short_clip = cfg.short_clip;
        let shared = Arc::new(Shared {
            core: Mutex::new(CaptionCore::new(cfg)),
            wake: Condvar::new(),
            stop,
            enabled: AtomicBool::new(true),
        });
        let mut feeds = Vec::new();
        let mut readers = Vec::new();
        for track in [Track::Mic, Track::System] {
            let (producer, consumer) = rtrb::RingBuffer::new(ring);
            let overflow = Arc::new(AtomicU64::new(0));
            feeds.push(CaptionFeed {
                producer,
                next: None,
                overflow: overflow.clone(),
            });
            readers.push(FeedReader {
                track,
                consumer,
                overflow,
                seen_overflow: 0,
                pos: 0,
            });
        }
        let pump = {
            let shared = shared.clone();
            let sink = sink.clone();
            std::thread::Builder::new()
                .name("minutae-captions-pump".into())
                .spawn(move || pump_loop(&shared, readers, &sink))?
        };
        let worker = {
            let shared = shared.clone();
            let sink = sink.clone();
            std::thread::Builder::new()
                .name("minutae-captions".into())
                .spawn(move || {
                    lower_thread_priority();
                    worker_loop(&shared, transcriber.as_ref(), &opts, &sink)
                })?
        };
        let system = feeds.pop().expect("two feeds");
        let mic = feeds.pop().expect("two feeds");
        Ok((
            Self {
                shared,
                sink,
                threads: vec![pump, worker],
            },
            mic,
            system,
        ))
    }

    fn with_core<T>(&self, f: impl FnOnce(&mut CaptionCore) -> T) -> T {
        let mut core = self.shared.core.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut core)
    }

    pub fn set_paused(&self, paused: bool) {
        let out = self.with_core(|c| c.set_paused(paused));
        for c in out {
            (self.sink)(c);
        }
        self.shared.wake.notify_all();
    }

    /// Turn captions off/on without stopping the threads (off = like paused, and audio
    /// is discarded at the pump).
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.enabled.store(enabled, Ordering::SeqCst);
        self.set_paused(!enabled);
    }

    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> CaptionStats {
        self.with_core(|c| c.stats())
    }

    /// Stop both threads (aborting a transcription in progress) and return the counters.
    pub fn stop(mut self) -> CaptionStats {
        self.shutdown();
        self.stats()
    }

    fn shutdown(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for CaptionEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn pump_loop(shared: &Shared, mut readers: Vec<FeedReader>, sink: &CaptionSink) {
    let mut scratch: Vec<f32> = Vec::with_capacity(RATE as usize);
    while !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(PUMP_EVERY);
        let enabled = shared.enabled.load(Ordering::Relaxed);
        let mut out = Vec::new();
        let mut fed = false;
        for r in &mut readers {
            scratch.clear();
            let n = r.consumer.slots();
            if let Ok(chunk) = r.consumer.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                scratch.extend_from_slice(a);
                scratch.extend_from_slice(b);
                chunk.commit_all();
            }
            let lost = r.overflow.load(Ordering::Relaxed);
            let new_lost = lost - r.seen_overflow;
            r.seen_overflow = lost;
            let start = r.pos;
            r.pos += scratch.len() as u64 + new_lost;
            if !enabled || scratch.is_empty() {
                continue;
            }
            let mut core = shared.core.lock().unwrap_or_else(|e| e.into_inner());
            if new_lost > 0 {
                core.note_overflow(new_lost);
            }
            out.extend(core.ingest(r.track, start, &scratch));
            fed = true;
        }
        if fed {
            shared.wake.notify_all();
        }
        for c in out {
            sink(c);
        }
    }
}

fn worker_loop(shared: &Shared, t: &dyn Transcriber, opts: &TranscribeOptions, sink: &CaptionSink) {
    loop {
        let job = {
            let mut core = shared.core.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(j) = core.next_job() {
                    break j;
                }
                core = shared
                    .wake
                    .wait_timeout(core, Duration::from_millis(250))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        };
        let started = Instant::now();
        let result = t.transcribe_samples(&job.samples, job.track, opts, None);
        let busy = started.elapsed();
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let caption = {
            let mut core = shared.core.lock().unwrap_or_else(|e| e.into_inner());
            match result {
                Ok(segments) => core.complete(job, &join_text(&segments), busy),
                Err(e) => {
                    tracing::debug!(target: "minutae_asr::captions", error = %e, "caption transcription failed");
                    core.fail(job)
                }
            }
        };
        if let Some(c) = caption {
            sink(c);
        }
    }
}

/// Run captions below normal priority so recording, the UI and the final pipeline win.
/// macOS: the default QoS class at its lowest relative priority. (`UTILITY` was tried: on a
/// busy machine it starved the worker until no caption came out at all.)
fn lower_thread_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: sets the calling thread's QoS class; no pointers involved.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_DEFAULT, -15);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: on Linux, PRIO_PROCESS with who = 0 changes the calling thread's nice value.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS as _, 0, 10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FakeTranscriber, Result};
    use minutae_core::Segment;

    const S: usize = RATE as usize;

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * S as f32) as usize)
            .map(|i| (i as f32 * 0.07).sin() * 0.2)
            .collect()
    }

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * S as f32) as usize]
    }

    fn core() -> CaptionCore {
        CaptionCore::new(CaptionConfig::default())
    }

    fn feed(core: &mut CaptionCore, track: Track, at: &mut u64, samples: &[f32]) -> Vec<Caption> {
        let out = core.ingest(track, *at, samples);
        *at += samples.len() as u64;
        out
    }

    #[test]
    fn windows_are_cut_with_overlap() {
        let mut c = core();
        let mut at = 0;
        // 11 s of speech → 4 s windows at 0, 3.5 and 7 s; the line being spoken then
        // starts at 10.5 s (the overlap) and holds 0.5 s.
        for _ in 0..110 {
            feed(&mut c, Track::Mic, &mut at, &tone(0.1));
        }
        assert_eq!(c.stats().windows, 3);
        let starts: Vec<u64> = c.queue.iter().map(|j| j.start).collect();
        assert_eq!(starts, vec![0, 56_000, 112_000]);
        assert!(c.queue.iter().all(|j| j.samples.len() == 64_000));
        assert!(c.queue.iter().all(|j| j.kind == JobKind::Final));
        // Unique ids, all different from the line now being spoken.
        let mut ids: Vec<u64> = c.queue.iter().map(|j| j.id).collect();
        ids.push(c.lines[0].id);
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(c.lines[0].buf_start, 168_000);
    }

    #[test]
    fn silent_windows_are_skipped() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::System, &mut at, &silence(8.0));
        assert_eq!(c.stats().windows, 2);
        assert_eq!(c.stats().silent, 2);
        assert_eq!(c.queued(), 0);
        assert!(c.next_job().is_none(), "no partial for silence");
    }

    #[test]
    fn overlap_dedupe() {
        assert_eq!(
            dedupe_overlap("we should ship in May.", "May, then review it"),
            "then review it"
        );
        assert_eq!(dedupe_overlap("so the plan is", "plan is fine"), "fine");
        // Short single words are not treated as overlap.
        assert_eq!(dedupe_overlap("look at the", "the end"), "the end");
        assert_eq!(dedupe_overlap("", "hello there"), "hello there");
        assert_eq!(dedupe_overlap("hello there", "Hello there."), "");
        assert_eq!(dedupe_overlap("one two", "three four"), "three four");
    }

    #[test]
    fn finals_dedupe_against_previous_window() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::System, &mut at, &tone(7.5));
        let j1 = c.next_job().unwrap();
        let j2 = c.next_job().unwrap();
        let a = c
            .complete(j1, "Let's move the launch to May", Duration::ZERO)
            .unwrap();
        let b = c
            .complete(j2, "to May. Alice owns the mockups.", Duration::ZERO)
            .unwrap();
        assert_eq!(a.text, "Let's move the launch to May");
        assert_eq!(b.text, "Alice owns the mockups.");
        assert!(a.is_final && b.is_final);
        assert_eq!((a.start_ms, a.end_ms), (0, 4_000));
        assert_eq!((b.start_ms, b.end_ms), (3_500, 7_500));
        assert_eq!(b.track, Track::System);
    }

    #[test]
    fn dedupe_uses_window_order_not_cut_order() {
        let mut c = core();
        let mut at = 0;
        // Two speech windows queue up, then a silent one is cut before either completes.
        feed(&mut c, Track::System, &mut at, &tone(7.5));
        feed(&mut c, Track::System, &mut at, &silence(8.0));
        assert_eq!((c.stats().windows, c.stats().silent), (4, 1));
        let w1 = c.next_job().unwrap();
        let w2 = c.next_job().unwrap();
        let w3 = c.next_job().unwrap(); // 7–11 s: the last 0.5 s of speech
        c.complete(w1, "so Bob is looking", Duration::ZERO);
        let b = c
            .complete(w2, "is looking at the budget", Duration::ZERO)
            .unwrap();
        assert_eq!(b.text, "at the budget");
        assert!(c.complete(w3, "", Duration::ZERO).is_none());
        // After the silence the next window is not de-duplicated against old text.
        feed(&mut c, Track::System, &mut at, &tone(4.0));
        let w = c.next_job().unwrap();
        let fresh = c.complete(w, "the budget is fine", Duration::ZERO).unwrap();
        assert_eq!(fresh.text, "the budget is fine");
    }

    #[test]
    fn window_after_a_dropped_one_is_not_deduplicated() {
        let mut c = CaptionCore::new(CaptionConfig {
            queue_capacity: 1,
            ..Default::default()
        });
        let mut at = 0;
        feed(&mut c, Track::Mic, &mut at, &tone(4.0));
        let w1 = c.next_job().unwrap();
        c.complete(w1, "we will ship in May", Duration::ZERO);
        feed(&mut c, Track::Mic, &mut at, &tone(7.0)); // windows 2 and 3; 2 is dropped
        assert_eq!(c.stats().dropped, 1);
        let w3 = c.next_job().unwrap();
        let t = c.complete(w3, "in May we review", Duration::ZERO).unwrap();
        assert_eq!(t.text, "in May we review", "no overlap with window 1");
    }

    #[test]
    fn partial_then_final_share_an_id() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::Mic, &mut at, &tone(2.0));
        let p = c.next_job().unwrap();
        assert_eq!(p.kind, JobKind::Partial);
        let line_id = p.id;
        let shown = c.complete(p, "Hello every", Duration::ZERO).unwrap();
        assert!(!shown.is_final);
        assert_eq!(shown.id, line_id);
        // Not enough new audio for another partial yet.
        feed(&mut c, Track::Mic, &mut at, &tone(0.5));
        assert!(c.next_job().is_none());
        feed(&mut c, Track::Mic, &mut at, &tone(1.5));
        let f = c.next_job().unwrap();
        assert_eq!((f.kind, f.id), (JobKind::Final, line_id));
        let fin = c.complete(f, "Hello everyone.", Duration::ZERO).unwrap();
        assert_eq!((fin.id, fin.is_final), (line_id, true));
    }

    #[test]
    fn partial_that_finishes_after_its_window_was_cut_still_counts() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::Mic, &mut at, &tone(2.0));
        let p = c.next_job().unwrap();
        feed(&mut c, Track::Mic, &mut at, &tone(2.0)); // window cut while p "runs"
        assert_eq!(c.queued(), 1);
        let ev = c.complete(p, "Hi", Duration::ZERO).unwrap();
        assert!(!ev.is_final);
        // If the window is then dropped/fails, the partial is settled as final.
        let f = c.next_job().unwrap();
        let settled = c.fail(f).unwrap();
        assert_eq!((settled.text.as_str(), settled.is_final), ("Hi", true));
    }

    #[test]
    fn empty_final_retracts_a_partial() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::Mic, &mut at, &tone(2.0));
        let p = c.next_job().unwrap();
        c.complete(p, "Uh", Duration::ZERO).unwrap();
        feed(&mut c, Track::Mic, &mut at, &tone(2.0));
        let f = c.next_job().unwrap();
        let ev = c.complete(f, "", Duration::ZERO).unwrap();
        assert!(ev.is_final && ev.text.is_empty());
        // Without a partial on screen, an empty final emits nothing.
        feed(&mut c, Track::Mic, &mut at, &tone(3.5));
        let f = c.next_job().unwrap();
        assert_eq!(f.kind, JobKind::Final);
        assert!(c.complete(f, "", Duration::ZERO).is_none());
    }

    #[test]
    fn backpressure_drops_oldest_window() {
        let mut c = CaptionCore::new(CaptionConfig {
            queue_capacity: 2,
            ..Default::default()
        });
        let mut at = 0;
        // Partial shown for the first line, then the worker stalls while 4 windows queue.
        feed(&mut c, Track::Mic, &mut at, &tone(2.0));
        let p = c.next_job().unwrap();
        c.complete(p, "First words", Duration::ZERO);
        let mut settled = vec![];
        for _ in 0..30 {
            settled.extend(feed(&mut c, Track::Mic, &mut at, &tone(0.5)));
        }
        // 2 + 15 s = 17 s → windows at 0, 3.5, 7, 10.5 → 4 windows, capacity 2.
        assert_eq!(c.stats().windows, 4);
        assert_eq!(c.stats().dropped, 2);
        assert_eq!(c.queued(), 2);
        let starts: Vec<u64> = c.queue.iter().map(|j| j.start * 1000 / RATE).collect();
        assert_eq!(starts, vec![7_000, 10_500], "newest windows are kept");
        // The dropped first line keeps the partial the user saw, now final.
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].text, "First words");
        assert!(settled[0].is_final);
    }

    #[test]
    fn pause_discards_pending_work_and_settles_partials() {
        let mut c = core();
        let mut at = 0;
        feed(&mut c, Track::Mic, &mut at, &tone(5.0)); // 1 window + 1.5 s pending
        let p = c.next_job().unwrap(); // the queued final first
        assert_eq!(p.kind, JobKind::Final);
        c.complete(p, "Before pause", Duration::ZERO);
        let partial = c.next_job().unwrap();
        assert_eq!(partial.kind, JobKind::Partial);
        let in_flight = partial.clone();
        c.complete(partial, "half a sen", Duration::ZERO).unwrap();
        feed(&mut c, Track::Mic, &mut at, &tone(3.0)); // another window queued
        assert_eq!(c.queued(), 1);

        let out = c.set_paused(true);
        assert_eq!(c.queued(), 0);
        assert_eq!(out.len(), 1, "the queued line's partial is settled");
        assert_eq!(out[0].text, "half a sen");
        assert!(out[0].is_final);
        // Nothing is accepted or produced while paused, and late partials are ignored.
        feed(&mut c, Track::Mic, &mut at, &tone(6.0));
        assert!(c.next_job().is_none());
        assert!(c.complete(in_flight, "late", Duration::ZERO).is_none());

        c.set_paused(false);
        let resume_at = at;
        feed(&mut c, Track::Mic, &mut at, &tone(4.0));
        let j = c.next_job().unwrap();
        assert_eq!(j.start, resume_at, "a fresh line starts at resume");
    }

    #[test]
    fn long_gap_closes_the_line() {
        let mut c = core();
        feed(&mut c, Track::Mic, &mut 0, &tone(2.0));
        // Next audio arrives 10 s later on the timeline.
        c.ingest(Track::Mic, 12 * S as u64, &tone(0.5));
        let j = c.next_job().unwrap();
        assert_eq!(
            (j.kind, j.start, j.samples.len()),
            (JobKind::Final, 0, 2 * S)
        );
        assert_eq!(c.lines[0].buf_start, 12 * S as u64);
        // A short gap is filled with silence instead.
        let mut c = core();
        feed(&mut c, Track::Mic, &mut 0, &tone(1.0));
        c.ingest(Track::Mic, S as u64 + 8_000, &tone(3.0));
        // 1 s + 0.5 s of zeros + 3 s = 4.5 s → one window, 1 s left.
        let j = c.next_job().unwrap();
        assert!(j.samples[S..S + 8_000].iter().all(|x| *x == 0.0));
        assert_eq!(c.lines[0].buf.len(), S);
    }

    #[test]
    fn feed_fills_gaps_and_counts_overflow() {
        let (producer, mut consumer) = rtrb::RingBuffer::new(100);
        let overflow = Arc::new(AtomicU64::new(0));
        let mut f = CaptionFeed {
            producer,
            next: None,
            overflow: overflow.clone(),
        };
        f.push(1_000, &[1.0; 10]);
        f.push(1_015, &[2.0; 10]); // 5-sample gap → zeros
        assert_eq!(consumer.slots(), 25);
        let got: Vec<f32> = (0..25).map(|_| consumer.pop().unwrap()).collect();
        assert_eq!(&got[10..15], &[0.0; 5]);
        assert_eq!(got[15], 2.0);
        f.push(1_025, &[3.0; 150]);
        assert_eq!(overflow.load(Ordering::Relaxed), 50);
    }

    /// Transcriber that "hears" text per track, slow enough to build a backlog.
    struct Scripted {
        delay: Duration,
        calls: Mutex<usize>,
    }

    impl Transcriber for Scripted {
        fn model_name(&self) -> String {
            "scripted".into()
        }
        fn transcribe_samples(
            &self,
            samples: &[f32],
            track: Track,
            _opts: &TranscribeOptions,
            _p: Option<crate::ProgressFn>,
        ) -> Result<Vec<Segment>> {
            std::thread::sleep(self.delay);
            *self.calls.lock().unwrap() += 1;
            Ok(vec![Segment {
                start_ms: 0,
                end_ms: samples.len() as u64 / 16,
                text: format!(
                    "{} said {}",
                    if track == Track::Mic { "you" } else { "they" },
                    samples.len() / 1_600
                ),
                track,
                speaker_id: None,
            }])
        }
    }

    #[test]
    fn engine_threads_end_to_end_with_fake_transcriber() {
        let got = Arc::new(Mutex::new(Vec::<Caption>::new()));
        let g = got.clone();
        let sink: CaptionSink = Arc::new(move |c| g.lock().unwrap().push(c));
        let t = Arc::new(FakeTranscriber::new(vec![(0, 4_000, "hello world".into())]));
        let (engine, mut mic, mut sys) = CaptionEngine::start(
            t.clone(),
            TranscribeOptions::default(),
            CaptionConfig::default(),
            sink,
        )
        .unwrap();
        // 4.5 s of speech on the mic, silence on the system track, in 100 ms writes.
        let mut at = 0;
        for _ in 0..45 {
            mic.push(at, &tone(0.1));
            sys.push(at, &silence(0.1));
            at += S as u64 / 10;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !got.lock().unwrap().iter().any(|c| c.is_final) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let stats = engine.stop();
        let got = got.lock().unwrap();
        let fin = got.iter().find(|c| c.is_final).expect("a final caption");
        assert_eq!(fin.track, Track::Mic);
        assert_eq!(fin.text, "hello world");
        assert_eq!((fin.start_ms, fin.end_ms), (0, 4_000));
        assert!(
            got.iter().all(|c| c.track == Track::Mic),
            "silent system track is skipped"
        );
        assert_eq!(stats.silent, 1);
        assert!(t.calls() >= 1);
    }

    #[test]
    fn engine_keeps_up_by_dropping_when_transcriber_is_slow() {
        let got = Arc::new(Mutex::new(Vec::<Caption>::new()));
        let g = got.clone();
        let sink: CaptionSink = Arc::new(move |c| g.lock().unwrap().push(c));
        let t = Arc::new(Scripted {
            delay: Duration::from_millis(300),
            calls: Mutex::new(0),
        });
        let cfg = CaptionConfig {
            queue_capacity: 1,
            ring_ms: 120_000,
            ..Default::default()
        };
        let (engine, mut mic, _sys) =
            CaptionEngine::start(t.clone(), TranscribeOptions::default(), cfg, sink).unwrap();
        // 60 s of speech pushed at once: far more than a 300 ms/window worker can do now.
        let mut at = 0;
        for _ in 0..60 {
            mic.push(at, &tone(1.0));
            at += S as u64;
        }
        std::thread::sleep(Duration::from_millis(800));
        let stats = engine.stop();
        assert!(stats.windows >= 16, "{stats:?}");
        assert!(stats.dropped >= 10, "old windows dropped: {stats:?}");
        assert!(*t.calls.lock().unwrap() <= 4);
        assert_eq!(stats.overflow_samples, 0);
    }

    #[test]
    fn engine_pause_and_disable() {
        let got = Arc::new(Mutex::new(Vec::<Caption>::new()));
        let g = got.clone();
        let sink: CaptionSink = Arc::new(move |c| g.lock().unwrap().push(c));
        let t = Arc::new(FakeTranscriber::new(vec![(0, 1_000, "x".into())]));
        let (engine, mut mic, _sys) = CaptionEngine::start(
            t.clone(),
            TranscribeOptions::default(),
            CaptionConfig::default(),
            sink,
        )
        .unwrap();
        engine.set_enabled(false);
        assert!(!engine.is_enabled());
        mic.push(0, &tone(6.0));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(t.calls(), 0);
        assert!(got.lock().unwrap().is_empty());
        let stats = engine.stop();
        assert_eq!(stats.windows, 0);
    }
}
