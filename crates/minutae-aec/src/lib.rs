//! minutae-aec — offline acoustic echo cancellation for people without headphones.
//!
//! The mic track also hears the other people coming out of the speakers. We recorded
//! what the speakers played (the system track), so after the meeting we can subtract
//! it from the mic before transcription:
//!
//! 1. **Skip checks**: no / silent system track → skip.
//! 2. **Alignment** ([`align`]): GCC-PHAT per 2 s window (1 s hop) gives the delay of
//!    the speaker sound in the mic. Too few confident windows → the speakers aren't
//!    audible (headphones) → skip. The per-window delays give a drift line (re-estimated
//!    from every window, robust to the peak hopping between reflections) plus large
//!    persistent jumps; the reference is re-timed onto the mic clock along it with a
//!    windowed-sinc fractional delay.
//! 3. **Adaptive filter** (`fdaf`): a 256 ms partitioned-block frequency-domain NLMS
//!    filter with a foreground/background pair for double-talk. Offline, so it runs a
//!    warm-up pass over the first minute first and the start is cancelled too.
//! 4. **ERLE**: median per-frame mic/output energy ratio while the reference plays.
//!    Below [`AecConfig::min_erle_db`] the result is discarded.
//!
//! Pure Rust (rustfft only): builds the same on macOS, Windows (static CRT) and Linux.
//! Blocking and CPU-bound (≈ 0.2 s per minute of audio on an Apple M-series core); call
//! from `spawn_blocking`. Only linear echo is removed; loudspeaker distortion and
//! residual echo are left for the transcript merge's bleed filter.

pub mod align;
mod fdaf;
pub mod synth;
mod wav;

use std::path::Path;
use std::time::Instant;

use minutae_core::{EchoCancelReport, EchoSkip};

pub use align::{AlignConfig, DelayEstimate, DelayPoint};
pub use fdaf::FdafConfig;
pub use wav::{read_wav_16k_mono, write_wav_16k_mono};

/// All processing runs at the recorder's rate (16 kHz mono).
pub const SAMPLE_RATE: u32 = 16_000;

/// File name of the cleaned mic track, next to `mic.wav`.
pub const CLEAN_MIC_FILE: &str = "mic.clean.wav";

#[derive(Debug, thiserror::Error)]
pub enum AecError {
    #[error("audio: {0}")]
    Audio(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = AecError> = std::result::Result<T, E>;

/// Result of [`cancel_to_vec`]: the cleaned mic samples when it ran.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub report: EchoCancelReport,
    pub cleaned: Option<Vec<f32>>,
}

/// Offline echo canceller: `mic` and `reference` (the system track) at 16 kHz mono.
pub trait EchoCanceller: Send + Sync {
    /// Streams the cleaned mic to `out` (in order, `mic.len()` samples in total) and
    /// returns what happened. Streaming keeps a long meeting's memory at the two input
    /// tracks. When the report says it did not run (`ran == false`), discard whatever
    /// `out` received: the decision to keep the result is made at the end.
    fn cancel(
        &self,
        mic: &[f32],
        reference: &[f32],
        out: &mut dyn FnMut(&[f32]),
    ) -> EchoCancelReport;
}

/// [`EchoCanceller::cancel`] into memory (tests, small inputs).
pub fn cancel_to_vec(aec: &dyn EchoCanceller, mic: &[f32], reference: &[f32]) -> Outcome {
    let mut buf = Vec::new();
    let report = aec.cancel(mic, reference, &mut |s| buf.extend_from_slice(s));
    let cleaned = report.ran.then_some(buf);
    Outcome { report, cleaned }
}

/// Tunables for [`OfflineAec`].
#[derive(Debug, Clone)]
pub struct AecConfig {
    pub align: AlignConfig,
    pub filter: FdafConfig,
    /// Share of 100 ms reference frames that must be above `align.min_ref_rms`.
    pub min_active_share: f32,
    /// Share of active windows with a confident echo peak needed to run (headphones ≈ 0).
    pub min_coupling: f32,
    /// Below this the cleaned track is not used.
    pub min_erle_db: f32,
    /// Samples the direct path is placed into the filter (room for alignment error and
    /// GCC locking onto an early reflection instead of the direct path).
    pub margin: usize,
    /// Length of the warm-up pass, seconds.
    pub warmup_secs: f32,
}

impl Default for AecConfig {
    fn default() -> Self {
        Self {
            align: AlignConfig::default(),
            filter: FdafConfig::default(),
            min_active_share: 0.01,
            min_coupling: 0.15,
            min_erle_db: 3.0,
            margin: 320, // 20 ms
            warmup_secs: 60.0,
        }
    }
}

/// The real canceller (alignment + PBFDAF).
#[derive(Debug, Clone, Default)]
pub struct OfflineAec {
    pub config: AecConfig,
}

impl EchoCanceller for OfflineAec {
    fn cancel(
        &self,
        mic: &[f32],
        reference: &[f32],
        out: &mut dyn FnMut(&[f32]),
    ) -> EchoCancelReport {
        let started = Instant::now();
        let skipped = |reason| {
            let mut r = EchoCancelReport::skipped(reason);
            r.elapsed_ms = started.elapsed().as_millis() as u64;
            r
        };
        let cfg = &self.config;
        if reference.is_empty() || mic.is_empty() {
            return skipped(EchoSkip::NoSystemTrack);
        }
        if active_share(reference, cfg.align.min_ref_rms) < cfg.min_active_share {
            return skipped(EchoSkip::SystemSilent);
        }
        let est = align::estimate(mic, reference, &cfg.align);
        let coupling = est.coupling();
        tracing::debug!(
            active = est.active_windows,
            confident = est.confident_windows,
            kept = est.points.len(),
            coupling,
            "echo alignment"
        );
        if est.points.len() < 2 || coupling < cfg.min_coupling {
            return skipped(EchoSkip::NoEcho);
        }
        // The reference re-timed onto the mic clock, generated block by block.
        let margin = cfg.margin as f64;
        let aligned = |n: usize| align::sample_at(reference, n as f64 - est.delay_at(n) + margin);
        let erle = run_filter(cfg, mic, &aligned, out);
        let mut report = EchoCancelReport {
            ran: erle >= cfg.min_erle_db,
            skipped: (erle < cfg.min_erle_db).then_some(EchoSkip::Ineffective),
            erle_db: Some(erle),
            delay_ms: est
                .median_delay()
                .map(|d| (d / SAMPLE_RATE as f64 * 1000.0) as f32),
            drift_ppm: Some(est.drift_ppm() as f32),
            elapsed_ms: 0,
        };
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        report
    }
}

/// Runs the filter over the whole signal — after a warm-up pass over the first
/// `warmup_secs`, so the opening seconds are cancelled with a converged filter — and
/// streams the output to `out`. Returns the ERLE estimate (see [`ErleMeter`]).
fn run_filter(
    cfg: &AecConfig,
    mic: &[f32],
    aligned: &dyn Fn(usize) -> f32,
    out: &mut dyn FnMut(&[f32]),
) -> f32 {
    let mut f = fdaf::Fdaf::new(cfg.filter.clone());
    let warm = ((cfg.warmup_secs * SAMPLE_RATE as f32) as usize).min(mic.len());
    if warm >= 10 * f.block() {
        filter_pass(&mut f, &mic[..warm], aligned, |_, _, _| {});
        f.reset_history();
    }
    let mut meter = ErleMeter::new(cfg.align.min_ref_rms);
    filter_pass(&mut f, mic, aligned, |d, e, x| {
        out(e);
        meter.add(d, e, x);
    });
    meter.erle_db()
}

/// One pass of `f` over `mic`; `on_block(mic, output, reference)` per block.
fn filter_pass(
    f: &mut fdaf::Fdaf,
    mic: &[f32],
    aligned: &dyn Fn(usize) -> f32,
    mut on_block: impl FnMut(&[f32], &[f32], &[f32]),
) {
    let b = f.block();
    let mut out = vec![0.0f32; b];
    let mut xb = vec![0.0f32; b];
    let mut db = vec![0.0f32; b];
    for (k, chunk) in mic.chunks(b).enumerate() {
        let n = chunk.len();
        let pos = k * b;
        for (i, x) in xb.iter_mut().enumerate() {
            *x = if i < n { aligned(pos + i) } else { 0.0 };
        }
        db.fill(0.0);
        db[..n].copy_from_slice(chunk);
        f.process(&xb, &db, &mut out);
        on_block(&db[..n], &out[..n], &xb[..n]);
    }
}

/// Share of 100 ms frames whose RMS exceeds `min_rms`.
fn active_share(x: &[f32], min_rms: f32) -> f32 {
    let frame = SAMPLE_RATE as usize / 10;
    let (mut active, mut total) = (0usize, 0usize);
    for f in x.chunks(frame) {
        total += 1;
        let e = f.iter().map(|v| v * v).sum::<f32>() / f.len() as f32;
        if e.sqrt() > min_rms {
            active += 1;
        }
    }
    active as f32 / total.max(1) as f32
}

/// Echo removed while the far end plays: the median over 20 ms frames (with an active
/// reference) of mic energy / output energy, in dB. Frames where the user talks over
/// the far end pull single frames down but barely move the median. Fed incrementally.
struct ErleMeter {
    min_ref_rms: f32,
    /// Sums for the current frame: mic, output, reference energy and sample count.
    acc: (f32, f32, f32, usize),
    ratios: Vec<f32>,
}

impl ErleMeter {
    const FRAME: usize = SAMPLE_RATE as usize / 50;

    fn new(min_ref_rms: f32) -> Self {
        Self {
            min_ref_rms,
            acc: (0.0, 0.0, 0.0, 0),
            ratios: Vec::new(),
        }
    }

    fn add(&mut self, mic: &[f32], out: &[f32], reference: &[f32]) {
        for ((m, o), r) in mic.iter().zip(out).zip(reference) {
            self.acc.0 += m * m;
            self.acc.1 += o * o;
            self.acc.2 += r * r;
            self.acc.3 += 1;
            if self.acc.3 == Self::FRAME {
                let (em, eo, er, n) = self.acc;
                if (er / n as f32).sqrt() >= self.min_ref_rms && em > 1e-10 {
                    self.ratios.push(10.0 * (em / eo.max(1e-12)).log10());
                }
                self.acc = (0.0, 0.0, 0.0, 0);
            }
        }
    }

    fn erle_db(mut self) -> f32 {
        if self.ratios.is_empty() {
            return 0.0;
        }
        self.ratios.sort_by(|a, b| a.total_cmp(b));
        self.ratios[self.ratios.len() / 2]
    }
}

/// Cleans `mic` with `system` as the far-end reference and writes `out` when it ran.
/// `system == None` (or missing) → skipped with [`EchoSkip::NoSystemTrack`]. A stale
/// `out` from an earlier run is removed whenever this run doesn't produce one.
pub fn cancel_files(
    aec: &dyn EchoCanceller,
    mic: &Path,
    system: Option<&Path>,
    out: &Path,
) -> Result<EchoCancelReport> {
    let started = Instant::now();
    let remove_stale = || -> Result<()> {
        if out.exists() {
            std::fs::remove_file(out)?;
        }
        Ok(())
    };
    let Some(system) = system.filter(|p| p.exists()) else {
        remove_stale()?;
        return Ok(EchoCancelReport::skipped(EchoSkip::NoSystemTrack));
    };
    let mic_s = read_wav_16k_mono(mic)?;
    let sys_s = read_wav_16k_mono(system)?;
    let mut writer = wav::Writer::create(out)?;
    let mut write_err = None;
    let mut report = aec.cancel(&mic_s, &sys_s, &mut |s| {
        if write_err.is_none()
            && let Err(e) = writer.write(s)
        {
            write_err = Some(e);
        }
    });
    if let Some(e) = write_err {
        writer.discard();
        return Err(e);
    }
    if report.ran {
        writer.finish()?;
    } else {
        writer.discard();
        remove_stale()?;
    }
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

/// Returns a fixed report; for tests of code that consumes an [`EchoCanceller`].
/// When `report.ran`, the "cleaned" track is the mic scaled by `gain`.
#[derive(Debug, Clone)]
pub struct FakeEchoCanceller {
    pub report: EchoCancelReport,
    pub gain: f32,
}

impl FakeEchoCanceller {
    pub fn ran(erle_db: f32) -> Self {
        Self {
            report: EchoCancelReport {
                ran: true,
                skipped: None,
                erle_db: Some(erle_db),
                delay_ms: Some(40.0),
                drift_ppm: Some(0.0),
                elapsed_ms: 1,
            },
            gain: 0.5,
        }
    }
    pub fn skipping(reason: EchoSkip) -> Self {
        Self {
            report: EchoCancelReport::skipped(reason),
            gain: 1.0,
        }
    }
}

impl EchoCanceller for FakeEchoCanceller {
    fn cancel(
        &self,
        mic: &[f32],
        _reference: &[f32],
        out: &mut dyn FnMut(&[f32]),
    ) -> EchoCancelReport {
        if self.report.ran {
            let scaled: Vec<f32> = mic.iter().map(|v| v * self.gain).collect();
            out(&scaled);
        }
        self.report.clone()
    }
}

#[cfg(test)]
mod tests;
