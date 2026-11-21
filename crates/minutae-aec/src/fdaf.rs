//! Partitioned-block frequency-domain adaptive filter (PBFDAF / MDF, overlap-save,
//! per-bin NLMS, round-robin gradient constraint) with a foreground/background pair
//! for double-talk:
//!
//! - the **background** filter adapts on every block with a normalised step;
//! - the **foreground** filter produces the output and only takes the background's
//!   coefficients when the background has cancelled more for ~100 ms in a row, shows
//!   real cancellation, and is within 10 dB of the best cancellation seen so far. When
//!   the near-end talker disturbs the background (double-talk), the foreground keeps
//!   the good estimate;
//! - if the background drifts far off, it is reset from the foreground;
//! - no copies right after a long silence of the reference, when only a few partitions
//!   hold signal and the background can look good on the current block while being
//!   wrong everywhere else.
//!
//! The echo path itself is linear here: bulk delay and clock drift are removed
//! beforehand by [`crate::align`], so the filter only models the room (≤ `taps`).

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Filter settings.
#[derive(Debug, Clone)]
pub struct FdafConfig {
    /// Block length, samples (power of two). FFT size is twice this.
    pub block: usize,
    /// Number of partitions; filter length = `block · partitions` taps.
    pub partitions: usize,
    /// Background NLMS step (0 < μ ≤ 1).
    pub step: f32,
    /// Step regulariser as a share of the mean per-bin reference power: bins the
    /// reference barely excites adapt slowly instead of chasing noise.
    pub reg: f32,
}

impl Default for FdafConfig {
    fn default() -> Self {
        Self {
            block: 256,
            partitions: 16, // 4096 taps = 256 ms at 16 kHz
            step: 0.5,
            reg: 0.01,
        }
    }
}

/// Blocks in a row the background must win before the foreground takes it (≈ 100 ms).
/// A single quiet block in double-talk can make a disturbed filter look good.
const BETTER_BLOCKS: u32 = 6;
/// …and its error must be this far below the mic (≈ 9 dB of cancellation shown). In
/// double-talk the error is mostly the user, so a disturbed background can’t get there.
const COPY_MAX_RESIDUAL: f32 = 0.125;
/// …and within 10 dB of the best error/mic ratio seen so far…
const BEST_MARGIN: f32 = 10.0;
/// …which relaxes by 10 dB per ~10 s (625 blocks) so a moved laptop can re-converge.
const BEST_RELAX: f32 = 1.003_69;

const ZERO: Complex32 = Complex32 { re: 0.0, im: 0.0 };

pub struct Fdaf {
    cfg: FdafConfig,
    n: usize,
    bins: usize,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    /// Reference spectra, newest first (ring with `head`).
    x_hist: Vec<Vec<Complex32>>,
    head: usize,
    /// Σ_p |X_p|² per bin.
    x_pow: Vec<f32>,
    w_fg: Vec<Vec<Complex32>>,
    w_bg: Vec<Vec<Complex32>>,
    prev_x: Vec<f32>,
    buf: Vec<Complex32>,
    /// Smoothed error energies (foreground, background) and mic energy.
    e_fg: f32,
    e_bg: f32,
    e_d: f32,
    /// Consecutive blocks where the background beat the foreground.
    better: u32,
    /// Consecutive blocks where the background was far worse than the foreground.
    worse: u32,
    /// Blocks processed (drives the round-robin constraint).
    t: u64,
    /// Blocks of reference since its last long (≥ 1 s) silence, and the current silent run.
    excited: usize,
    silent: usize,
    /// Lowest smoothed error/mic ratio seen (slowly relaxed).
    best: f32,
    /// Step regulariser for this block.
    reg: f32,
}

impl Fdaf {
    pub fn new(cfg: FdafConfig) -> Self {
        let n = cfg.block * 2;
        let bins = n; // full complex spectrum (simple; real-input symmetry unused)
        let mut planner = FftPlanner::<f32>::new();
        let p = cfg.partitions;
        Self {
            fwd: planner.plan_fft_forward(n),
            inv: planner.plan_fft_inverse(n),
            x_hist: vec![vec![ZERO; bins]; p],
            head: 0,
            x_pow: vec![0.0; bins],
            w_fg: vec![vec![ZERO; bins]; p],
            w_bg: vec![vec![ZERO; bins]; p],
            prev_x: vec![0.0; cfg.block],
            buf: vec![ZERO; n],
            e_fg: 0.0,
            e_bg: 0.0,
            e_d: 0.0,
            better: 0,
            worse: 0,
            t: 0,
            excited: 0,
            silent: 0,
            reg: 1.0,
            best: 1.0,
            n,
            bins,
            cfg,
        }
    }

    pub fn block(&self) -> usize {
        self.cfg.block
    }

    /// Clears the signal history, keeping the learned filters (for a second pass).
    pub fn reset_history(&mut self) {
        self.x_hist.iter_mut().for_each(|x| x.fill(ZERO));
        self.x_pow.fill(0.0);
        self.prev_x.fill(0.0);
        self.head = 0;
    }

    /// Filters one block: `x` = aligned reference, `d` = mic, `out` = mic minus the
    /// estimated echo. All slices are `block` long.
    pub fn process(&mut self, x: &[f32], d: &[f32], out: &mut [f32]) {
        let b = self.cfg.block;
        let p = self.cfg.partitions;
        debug_assert!(x.len() == b && d.len() == b && out.len() == b);

        // New reference spectrum: FFT([previous block, current block]).
        self.head = (self.head + p - 1) % p;
        {
            let slot = &mut self.x_hist[self.head];
            for (i, v) in slot.iter_mut().enumerate() {
                let s = if i < b { self.prev_x[i] } else { x[i - b] };
                *v = Complex32::new(s, 0.0);
            }
            self.fwd.process(slot);
        }
        self.prev_x.copy_from_slice(x);
        // Track how long the reference has been playing since its last long silence.
        let ex: f32 = x.iter().map(|v| v * v).sum();
        if ex > 1e-7 * b as f32 {
            self.excited += 1;
            self.silent = 0;
        } else {
            self.silent += 1;
            if self.silent > 4 * p {
                self.excited = 0;
            }
        }
        // Per-bin reference power over the filter span (NLMS normalisation), plus a
        // regulariser tied to the average power so near-empty bins don't get huge steps.
        self.x_pow.fill(0.0);
        for xs in &self.x_hist {
            for (pw, v) in self.x_pow.iter_mut().zip(xs) {
                *pw += v.norm_sqr();
            }
        }
        let mean = self.x_pow.iter().sum::<f32>() / self.bins as f32;
        self.reg = self.cfg.reg * mean + 1e-8 * (self.n * p) as f32;

        self.t += 1;
        let e_fg = self.error(false, d);
        let e_bg_time = self.error(true, d);
        let ef: f32 = e_fg.iter().map(|v| v * v).sum();
        let eb: f32 = e_bg_time.iter().map(|v| v * v).sum();
        let ed: f32 = d.iter().map(|v| v * v).sum();
        self.e_fg = 0.7 * self.e_fg + 0.3 * ef;
        self.e_bg = 0.7 * self.e_bg + 0.3 * eb;
        self.e_d = 0.7 * self.e_d + 0.3 * ed;

        // Adapt the background with its own error.
        self.adapt(&e_bg_time);

        // Best cancellation shown so far (error/mic), forgotten slowly so a changed room
        // can set a new, worse best.
        let settled = self.excited >= 2 * p;
        if settled && self.e_d > 1e-9 * b as f32 {
            let ratio = self.e_fg.min(self.e_bg) / self.e_d;
            self.best = ratio.min(self.best * BEST_RELAX).min(1.0);
        }

        // Foreground/background decision. The background must beat the foreground *and*
        // cancel nearly as well as the best seen: in double-talk the error is mostly the
        // user's voice (which a disturbed background can partly "explain" over a few
        // blocks), so it stays far above that and the foreground keeps its estimate.
        let max_residual = COPY_MAX_RESIDUAL.min(BEST_MARGIN * self.best);
        if self.e_bg < 0.85 * self.e_fg && self.e_bg < max_residual * self.e_d && settled {
            self.better += 1;
            if self.better >= BETTER_BLOCKS {
                for (f, g) in self.w_fg.iter_mut().zip(&self.w_bg) {
                    f.copy_from_slice(g);
                }
                self.e_fg = self.e_bg;
            }
        } else {
            self.better = 0;
        }
        // Background knocked off by double-talk: restart it from the foreground.
        if self.e_bg > 2.0 * self.e_fg.max(1e-12) {
            self.worse += 1;
        } else {
            self.worse = 0;
        }
        if self.worse >= 8 || self.e_bg > 4.0 * self.e_fg.max(1e-12) {
            for (g, f) in self.w_bg.iter_mut().zip(&self.w_fg) {
                g.copy_from_slice(f);
            }
            self.e_bg = self.e_fg;
            self.worse = 0;
        }
        out.copy_from_slice(&e_fg);
    }

    /// Time-domain error `d - y` for the foreground (or background) filter.
    fn error(&mut self, background: bool, d: &[f32]) -> Vec<f32> {
        let b = self.cfg.block;
        let p = self.cfg.partitions;
        let weights = if background { &self.w_bg } else { &self.w_fg };
        self.buf.fill(ZERO);
        for (j, ws) in weights.iter().enumerate() {
            let xs = &self.x_hist[(self.head + j) % p];
            for ((acc, w), x) in self.buf.iter_mut().zip(ws).zip(xs) {
                *acc += w * x;
            }
        }
        self.inv.process(&mut self.buf);
        let scale = 1.0 / self.n as f32;
        (0..b).map(|i| d[i] - self.buf[b + i].re * scale).collect()
    }

    /// Constrained NLMS update of the background filter.
    fn adapt(&mut self, e: &[f32]) {
        let b = self.cfg.block;
        let p = self.cfg.partitions;
        // E = FFT([0, e])
        let mut ef = vec![ZERO; self.n];
        for i in 0..b {
            ef[b + i] = Complex32::new(e[i], 0.0);
        }
        self.fwd.process(&mut ef);
        let delta = self.reg;
        let mu = self.cfg.step;
        let scale = 1.0 / self.n as f32;
        // Unconstrained gradient for every partition; the time-domain constraint (taps
        // beyond the block length zeroed) is applied to one partition per block in turn,
        // as in Speex's MDF. Constraining all partitions every block costs 2·P extra FFTs
        // per block for no measurable ERLE gain on the synthetic calls.
        for j in 0..p {
            let xs = &self.x_hist[(self.head + j) % p];
            for ((w, x), (e, pw)) in self.w_bg[j]
                .iter_mut()
                .zip(xs)
                .zip(ef.iter().zip(&self.x_pow))
            {
                *w += x.conj() * e * (mu / (pw + delta));
            }
        }
        let c = (self.t as usize) % p;
        let w = &mut self.w_bg[c];
        self.inv.process(w);
        for (i, v) in w.iter_mut().enumerate() {
            *v = if i < b { *v * scale } else { ZERO };
        }
        self.fwd.process(w);
    }
}
