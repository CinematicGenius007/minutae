//! Bulk delay between the system track (what the speakers played) and the mic track,
//! estimated per window with GCC-PHAT, cleaned of outliers, and turned into a
//! time-varying delay curve. Following the curve with a fractional-delay interpolator
//! removes both the fixed offset between the two recordings and slow clock drift, so
//! the adaptive filter only has to model the room.

use std::sync::OnceLock;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::SAMPLE_RATE;

/// Spectral whitening exponent for GCC (1 = pure PHAT).
const PHAT_BETA: f32 = 0.7;

/// Settings for [`estimate`].
#[derive(Debug, Clone)]
pub struct AlignConfig {
    /// Analysis window, samples (power of two).
    pub window: usize,
    /// Hop between windows, samples.
    pub hop: usize,
    /// Largest |delay| searched, samples.
    pub max_lag: usize,
    /// A window counts only if the reference RMS is above this (linear, full scale = 1).
    pub min_ref_rms: f32,
    /// GCC-PHAT peak-to-RMS ratio above which a window's delay is trusted.
    pub min_score: f32,
}

impl Default for AlignConfig {
    fn default() -> Self {
        Self {
            window: 1 << 15,               // 2.05 s
            hop: 1 << 14,                  // 1.02 s
            max_lag: SAMPLE_RATE as usize, // ±1 s
            min_ref_rms: 0.003,            // ≈ -50 dBFS
            min_score: 12.0,
        }
    }
}

/// Delay of the echo in the mic relative to the reference at one point in time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayPoint {
    /// Window centre, samples.
    pub at: usize,
    /// `mic[n] ≈ reference[n - delay]`; fractional samples, may be negative.
    pub delay: f64,
    /// GCC-PHAT peak-to-RMS ratio.
    pub score: f32,
}

/// Result of [`estimate`].
#[derive(Debug, Clone, Default)]
pub struct DelayEstimate {
    /// Windows where the reference was active.
    pub active_windows: usize,
    /// Active windows with a confident echo peak (before outlier removal: a peak on a
    /// reflection still proves the speakers are audible).
    pub confident_windows: usize,
    /// Windows with a confident peak, outliers removed, in time order.
    pub points: Vec<DelayPoint>,
    /// Smoothed delay curve `(sample, delay)` the reference is re-timed along.
    curve: Vec<(usize, f64)>,
    slope: f64,
}

/// Offsets from the drift line smaller than this are not followed, samples. GCC peaks
/// hop between the direct path and strong early reflections (up to ~20 ms later) for
/// long stretches; following those hops would make the adaptive filter re-converge each
/// time, while its taps already cover such offsets (see `AecConfig::margin`). Larger
/// persistent jumps (a capture glitch) are followed.
const STEP_TOL: f64 = 0.025 * SAMPLE_RATE as f64;
/// A jump must hold for most of ±this many windows (≈ ±8 s) to be followed.
const STEP_SPAN: usize = 8;

impl DelayEstimate {
    /// Builds the delay curve: a line through the points — clock drift is linear — plus
    /// persistent jumps away from it (e.g. a capture glitch), found as median-filtered
    /// residuals larger than [`STEP_TOL`].
    ///
    /// GCC peaks form clusters (direct path, strong reflections a few ms later) and the
    /// winning cluster can change for long stretches, which would fool a plain line fit
    /// into a large fake drift. So the slope comes only from nearby pairs in the same
    /// cluster, and the line is anchored on the earliest well-populated cluster.
    pub fn new(active_windows: usize, confident_windows: usize, points: Vec<DelayPoint>) -> Self {
        let mut est = Self {
            active_windows,
            confident_windows,
            points,
            curve: Vec::new(),
            slope: 0.0,
        };
        let p = &est.points;
        if p.is_empty() {
            return est;
        }
        let slope = drift_slope(p);
        let mut base: Vec<f64> = p.iter().map(|x| x.delay - slope * x.at as f64).collect();
        let intercept = anchor(&mut base);
        let resid: Vec<f64> = p
            .iter()
            .map(|x| x.delay - (intercept + slope * x.at as f64))
            .collect();
        est.curve = p
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let lo = i.saturating_sub(STEP_SPAN);
                let hi = (i + STEP_SPAN + 1).min(resid.len());
                let mut w = resid[lo..hi].to_vec();
                let r = median(&mut w).unwrap_or(0.0);
                let r = if r.abs() < STEP_TOL { 0.0 } else { r };
                (x.at, intercept + slope * x.at as f64 + r)
            })
            .collect();
        est.slope = slope;
        est
    }

    /// Share of active windows with a confident echo peak (0 when nothing was active).
    pub fn coupling(&self) -> f32 {
        if self.active_windows == 0 {
            0.0
        } else {
            self.confident_windows as f32 / self.active_windows as f32
        }
    }

    /// Median of the delay curve, samples.
    pub fn median_delay(&self) -> Option<f64> {
        let mut d: Vec<f64> = self.curve.iter().map(|p| p.1).collect();
        median(&mut d)
    }

    /// Clock drift (slope of the delay line), ppm. Positive: the echo arrives later
    /// and later.
    pub fn drift_ppm(&self) -> f64 {
        self.slope * 1e6
    }

    /// Delay at sample `n`: the curve, linear between its points and extended along
    /// the drift line beyond them.
    pub fn delay_at(&self, n: usize) -> f64 {
        let c = &self.curve;
        match c.len() {
            0 => 0.0,
            1 => c[0].1,
            _ => {
                let i = c.partition_point(|x| x.0 <= n);
                if i == 0 {
                    c[0].1 - self.slope * (c[0].0 - n) as f64
                } else if i == c.len() {
                    let last = c[c.len() - 1];
                    last.1 + self.slope * (n - last.0) as f64
                } else {
                    let (a, b) = (c[i - 1], c[i]);
                    let t = (n - a.0) as f64 / (b.0 - a.0) as f64;
                    a.1 + (b.1 - a.1) * t
                }
            }
        }
    }
}

/// Clock drift as samples of delay per sample of time. A coarse slope from pairs 2–5 s
/// apart whose delays differ by under 2 ms (same GCC cluster even at ~400 ppm), then a
/// refined one from pairs 3–60 s apart that stay within 1 ms of the coarse line.
fn drift_slope(p: &[DelayPoint]) -> f64 {
    let sr = SAMPLE_RATE as f64;
    let pairs = |min_dt: f64, max_dt: f64, tol: f64, guess: f64| {
        let mut slopes = Vec::new();
        for i in 0..p.len() {
            for q in &p[i + 1..] {
                let dt = q.at as f64 - p[i].at as f64;
                if dt > max_dt {
                    break;
                }
                let dd = q.delay - p[i].delay;
                if dt >= min_dt && (dd - guess * dt).abs() < tol {
                    slopes.push(dd / dt);
                }
            }
        }
        median(&mut slopes)
    };
    let coarse = pairs(2.0 * sr, 5.0 * sr, 0.002 * sr, 0.0).unwrap_or(0.0);
    pairs(3.0 * sr, 60.0 * sr, 0.001 * sr, coarse).unwrap_or(coarse)
}

/// Delay (drift removed) of the earliest cluster holding at least a third as many
/// points as the biggest one: the direct path when it is visible often enough.
fn anchor(base: &mut [f64]) -> f64 {
    base.sort_by(|a, b| a.total_cmp(b));
    let width = 0.001 * SAMPLE_RATE as f64;
    // Clusters: runs of sorted values with gaps under `width`.
    let mut clusters: Vec<&[f64]> = Vec::new();
    let mut start = 0;
    for i in 1..=base.len() {
        if i == base.len() || base[i] - base[i - 1] > width {
            clusters.push(&base[start..i]);
            start = i;
        }
    }
    let biggest = clusters.iter().map(|c| c.len()).max().unwrap_or(0);
    clusters
        .iter()
        .find(|c| c.len() * 3 >= biggest)
        .map(|c| c[c.len() / 2])
        .unwrap_or(0.0)
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Per-window GCC-PHAT delay estimates between `mic` and `reference`.
pub fn estimate(mic: &[f32], reference: &[f32], cfg: &AlignConfig) -> DelayEstimate {
    let w = cfg.window;
    // Generous zero padding: the PHAT score threshold below is calibrated for this size.
    let n_fft = (w + cfg.max_lag).next_power_of_two() * 2;
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(n_fft);
    let inv = planner.plan_fft_inverse(n_fft);
    let len = mic.len().min(reference.len());
    let sr = SAMPLE_RATE as f32;
    let (lo_bin, hi_bin) = (
        (100.0 * n_fft as f32 / sr) as usize,
        (4000.0 * n_fft as f32 / sr) as usize,
    );
    let mut a = vec![Complex32::new(0.0, 0.0); n_fft];
    let mut b = vec![Complex32::new(0.0, 0.0); n_fft];
    let mut active_windows = 0usize;
    let mut raw = Vec::new();
    let mut start = 0usize;
    while start + w <= len {
        let r = &reference[start..start + w];
        let m_lo = start.saturating_sub(cfg.max_lag);
        let m_hi = (start + w + cfg.max_lag).min(len);
        if rms(r) < cfg.min_ref_rms || rms(&mic[start..start + w]) < cfg.min_ref_rms * 0.1 {
            start += cfg.hop;
            continue;
        }
        active_windows += 1;
        // Mic segment covers the window plus the search margin on both sides; the
        // reference window sits at offset `start - m_lo` inside it.
        a.iter_mut().for_each(|v| *v = Complex32::new(0.0, 0.0));
        b.iter_mut().for_each(|v| *v = Complex32::new(0.0, 0.0));
        for (i, v) in mic[m_lo..m_hi].iter().enumerate() {
            a[i].re = *v;
        }
        let off = start - m_lo;
        for (i, v) in r.iter().enumerate() {
            b[off + i].re = *v * hann(i, w);
        }
        fwd.process(&mut a);
        fwd.process(&mut b);
        for k in 0..n_fft {
            let kk = k.min(n_fft - k);
            if kk < lo_bin || kk > hi_bin {
                a[k] = Complex32::new(0.0, 0.0);
                continue;
            }
            let g = a[k] * b[k].conj();
            let mag = g.norm();
            // PHAT-β: mostly whitened, but quiet (noise-only) bins weigh less.
            a[k] = if mag > 1e-12 {
                g / mag.powf(PHAT_BETA)
            } else {
                Complex32::new(0.0, 0.0)
            };
        }
        inv.process(&mut a);
        // Correlation at lag L (mic later than reference by L) is at index L (mod n_fft).
        let at = |lag: i64| a[lag.rem_euclid(n_fft as i64) as usize].re;
        let max_lag = cfg.max_lag as i64;
        let (mut best, mut best_v) = (0i64, f32::MIN);
        for lag in -max_lag..=max_lag {
            let v = at(lag);
            if v > best_v {
                best_v = v;
                best = lag;
            }
        }
        let mut e = 0.0f64;
        let mut c = 0usize;
        for lag in -max_lag..=max_lag {
            if (lag - best).abs() > 32 {
                e += (at(lag) as f64).powi(2);
                c += 1;
            }
        }
        let floor = ((e / c.max(1) as f64).sqrt() as f32).max(1e-12);
        let score = best_v / floor;
        if score >= cfg.min_score {
            // Parabolic peak interpolation for a sub-sample delay.
            let (y0, y1, y2) = (at(best - 1), best_v, at(best + 1));
            let den = y0 - 2.0 * y1 + y2;
            let frac = if den.abs() > 1e-12 {
                (0.5 * (y0 - y2) / den).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            raw.push(DelayPoint {
                at: start + w / 2,
                delay: best as f64 + frac as f64,
                score,
            });
        }
        start += cfg.hop;
    }
    let confident = raw.len();
    DelayEstimate::new(active_windows, confident, reject_outliers(raw))
}

/// Drops points more than 3 ms from the median of their ±3 neighbours (a reflection
/// or the near-end talker briefly winning the correlation).
fn reject_outliers(points: Vec<DelayPoint>) -> Vec<DelayPoint> {
    let tol = 0.003 * SAMPLE_RATE as f64;
    let n = points.len();
    (0..n)
        .filter(|&i| {
            let lo = i.saturating_sub(3);
            let hi = (i + 4).min(n);
            let mut d: Vec<f64> = points[lo..hi].iter().map(|p| p.delay).collect();
            let m = median(&mut d).unwrap_or(points[i].delay);
            (points[i].delay - m).abs() <= tol
        })
        .map(|i| points[i])
        .collect()
}

fn hann(i: usize, n: usize) -> f32 {
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()
}

/// Half-width of the interpolation kernel (taps = 2·HALF).
const HALF: usize = 8;
/// Fractional positions in the kernel table.
const PHASES: usize = 512;

/// Lanczos-windowed sinc table, `PHASES + 1` rows of `2·HALF` taps.
fn kernel() -> &'static [[f32; 2 * HALF]] {
    static K: OnceLock<Vec<[f32; 2 * HALF]>> = OnceLock::new();
    K.get_or_init(|| {
        (0..=PHASES)
            .map(|p| {
                let frac = p as f64 / PHASES as f64;
                let mut row = [0.0f32; 2 * HALF];
                let mut sum = 0.0;
                for (j, v) in row.iter_mut().enumerate() {
                    // Tap j sits at integer offset j - HALF + 1 from floor(t).
                    let x = (j as f64 - HALF as f64 + 1.0) - frac;
                    let s = sinc(x) * sinc(x / HALF as f64);
                    *v = s as f32;
                    sum += s;
                }
                row.iter_mut().for_each(|v| *v /= sum as f32);
                row
            })
            .collect()
    })
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

/// `x` at fractional time `t` (samples), zero outside the signal.
pub fn sample_at(x: &[f32], t: f64) -> f32 {
    if !t.is_finite() || t < -(HALF as f64) || t > (x.len() + HALF) as f64 {
        return 0.0;
    }
    let base = t.floor();
    let frac = t - base;
    let row = &kernel()[(frac * PHASES as f64).round() as usize];
    let base = base as i64;
    let mut acc = 0.0f32;
    for (j, k) in row.iter().enumerate() {
        let i = base + j as i64 - HALF as i64 + 1;
        if i >= 0 && (i as usize) < x.len() {
            acc += k * x[i as usize];
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    #[test]
    fn integer_and_fractional_interpolation() {
        let x: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
        assert!((sample_at(&x, 20.0) - x[20]).abs() < 1e-5);
        let mid = sample_at(&x, 20.5);
        let want = (20.5f32 * 0.3).sin();
        assert!((mid - want).abs() < 2e-3, "{mid} vs {want}");
        assert_eq!(sample_at(&x, -100.0), 0.0);
    }

    #[test]
    fn finds_delay_and_drift_through_a_room() {
        let far = synth::speech_like(40.0, 11, 180.0);
        let h = synth::room_ir(120.0, 0.25, 0.6, 3);
        let echo = synth::drift(&synth::delay(&synth::convolve(&far, &h), 150.0), 100.0);
        let mut mic = echo.clone();
        let n = synth::noise(mic.len(), 0.001, 9);
        mic.iter_mut().zip(n).for_each(|(m, v)| *m += v);
        let est = estimate(&mic, &far, &AlignConfig::default());
        // Very reverberant room: not every window gives a clear peak, but plenty do.
        assert!(est.coupling() > 0.25, "coupling {}", est.coupling());
        let d = est.median_delay().unwrap() / 16.0;
        // 150 ms minus ~2 ms of drift by the middle of the file; the peak may sit on an
        // early reflection a few ms later, which the filter margin absorbs.
        assert!((147.0..156.0).contains(&d), "delay {d} ms");
        let ppm = est.drift_ppm();
        // drift(ppm) shifts the echo earlier by ppm·n: the measured delay falls.
        assert!((ppm + 100.0).abs() < 25.0, "drift {ppm} ppm");
    }

    #[test]
    fn uncorrelated_tracks_have_no_coupling() {
        let far = synth::speech_like(30.0, 21, 180.0);
        let near = synth::speech_like(30.0, 22, 110.0);
        let est = estimate(&near, &far, &AlignConfig::default());
        assert!(est.active_windows > 10);
        assert!(est.coupling() < 0.1, "coupling {}", est.coupling());
    }
}
