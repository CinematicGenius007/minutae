//! Streaming windowed-sinc resampler (any rate -> any rate, mono f32).
//!
//! Uses exact rational positions (`n * L / M`) so there is no drift over long
//! recordings. When the reduced ratio has at most [`MAX_PHASES`] phases (all
//! common rates: 44.1k, 48k, 96k, 22.05k, 32k...), the filter is precomputed as
//! a polyphase table; otherwise coefficients are computed on the fly.

const ZERO_CROSSINGS: f64 = 16.0;
const MAX_PHASES: u64 = 4096;
/// Pass band edge as a fraction of the lower Nyquist frequency.
const ROLLOFF: f64 = 0.92;

pub struct Resampler {
    passthrough: bool,
    /// Input step numerator / denominator (reduced): out[n] sits at input position n*l/m.
    l: u64,
    m: u64,
    /// Taps on each side of the centre (in input samples).
    half: i64,
    /// Normalised cutoff in cycles per input sample.
    fc: f64,
    /// `m * taps` coefficients when precomputed.
    table: Option<Vec<f32>>,
    /// Input history; `buf[0]` is absolute input index `buf_start`.
    buf: Vec<f32>,
    buf_start: i64,
    /// Next output index.
    n: u64,
    flushed: bool,
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn kernel(x: f64, fc: f64, half: f64) -> f64 {
    if x.abs() >= half {
        return 0.0;
    }
    let s = if x == 0.0 {
        2.0 * fc
    } else {
        (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x)
    };
    // Blackman window over [-half, half].
    let r = x / half;
    let w = 0.42
        + 0.5 * (std::f64::consts::PI * r).cos()
        + 0.08 * (2.0 * std::f64::consts::PI * r).cos();
    s * w
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        assert!(in_rate > 0 && out_rate > 0, "sample rates must be non-zero");
        let g = gcd(in_rate as u64, out_rate as u64);
        let (l, m) = (in_rate as u64 / g, out_rate as u64 / g);
        let fc = 0.5 * (out_rate as f64 / in_rate as f64).min(1.0) * ROLLOFF;
        let half = (ZERO_CROSSINGS / (2.0 * fc)).ceil() as i64;
        let mut r = Self {
            passthrough: in_rate == out_rate,
            l,
            m,
            half,
            fc,
            table: None,
            // Pre-roll of zeros so the first outputs have full history.
            buf: vec![0.0; half as usize],
            buf_start: -half,
            n: 0,
            flushed: false,
        };
        if !r.passthrough && m <= MAX_PHASES {
            let taps = r.taps();
            let mut t = Vec::with_capacity(m as usize * taps);
            for p in 0..m {
                t.extend(r.coeffs(p));
            }
            r.table = Some(t);
        }
        r
    }

    fn taps(&self) -> usize {
        (2 * self.half) as usize
    }

    /// Normalised (unity DC gain) coefficients for phase `p` (fraction p/m),
    /// applied to input offsets k = -half+1 ..= half around the centre.
    fn coeffs(&self, p: u64) -> Vec<f32> {
        let frac = p as f64 / self.m as f64;
        let h = self.half as f64;
        let raw: Vec<f64> = (-self.half + 1..=self.half)
            .map(|k| kernel(k as f64 - frac, self.fc, h))
            .collect();
        let sum: f64 = raw.iter().sum();
        raw.iter().map(|v| (v / sum) as f32).collect()
    }

    /// Feed input samples; appends resampled output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        self.drain(out);
    }

    /// Flush the tail (pads with zeros). Call once at end of stream.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if self.passthrough || self.flushed {
            return;
        }
        self.flushed = true;
        // Total real input = buf_start + buf.len(); emit outputs whose centre lies inside it.
        let end = self.buf_start + self.buf.len() as i64;
        self.buf
            .extend(std::iter::repeat_n(0.0, self.half as usize + 1));
        let before = out.len();
        self.drain(out);
        // Drop outputs that fall beyond the real input end.
        let max_n = ((end.max(0) as u64) * self.m).div_ceil(self.l);
        let produced_total = self.n;
        if produced_total > max_n {
            let extra = (produced_total - max_n) as usize;
            let keep = (out.len() - before).saturating_sub(extra);
            out.truncate(before + keep);
            self.n = max_n;
        }
    }

    fn drain(&mut self, out: &mut Vec<f32>) {
        let taps = self.taps();
        let avail_end = self.buf_start + self.buf.len() as i64; // exclusive
        loop {
            let pos = self.n * self.l;
            let c = (pos / self.m) as i64;
            let p = pos % self.m;
            if c + self.half >= avail_end {
                break;
            }
            let start = (c - self.half + 1 - self.buf_start) as usize;
            let window = &self.buf[start..start + taps];
            let y = match &self.table {
                Some(t) => {
                    let row = &t[p as usize * taps..(p as usize + 1) * taps];
                    dot(window, row)
                }
                None => dot(window, &self.coeffs(p)),
            };
            out.push(y);
            self.n += 1;
        }
        // Discard history no longer needed by the next output.
        let next_c = ((self.n * self.l) / self.m) as i64;
        let keep_from = next_c - self.half + 1;
        let drop = (keep_from - self.buf_start).clamp(0, self.buf.len() as i64) as usize;
        if drop > 0 {
            self.buf.drain(..drop);
            self.buf_start += drop as i64;
        }
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, secs: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn run(in_rate: u32, input: &[f32], block: usize) -> Vec<f32> {
        let mut r = Resampler::new(in_rate, 16_000);
        let mut out = Vec::new();
        for c in input.chunks(block) {
            r.process(c, &mut out);
        }
        r.flush(&mut out);
        out
    }

    #[test]
    fn length_matches_ratio() {
        for rate in [48_000, 44_100, 96_000, 22_050, 8_000, 16_000, 47_999] {
            let input = sine(rate, 440.0, 1.0, 0.5);
            let out = run(rate, &input, 480);
            let expected = 16_000i64;
            assert!(
                (out.len() as i64 - expected).abs() <= 1,
                "{rate}: got {} samples",
                out.len()
            );
        }
    }

    #[test]
    fn passband_preserved() {
        for rate in [48_000, 44_100] {
            let out = run(rate, &sine(rate, 1000.0, 1.0, 0.5), 333);
            // Skip edges (filter warm-up).
            let mid = &out[1000..15_000];
            let r = rms(mid);
            assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.01, "{rate}: rms {r}");
        }
    }

    #[test]
    fn frequency_preserved() {
        let out = run(48_000, &sine(48_000, 1000.0, 1.0, 0.5), 512);
        let mid = &out[1000..15_000];
        let crossings = mid.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        // 14_000 samples at 16 kHz = 0.875 s -> ~875 cycles of 1 kHz.
        assert!((crossings as i32 - 875).abs() <= 2, "crossings {crossings}");
    }

    #[test]
    fn aliasing_rejected() {
        // 12 kHz is above the 8 kHz output Nyquist; it must not fold back.
        let out = run(48_000, &sine(48_000, 12_000.0, 1.0, 0.5), 480);
        let r = rms(&out[1000..15_000]);
        assert!(r < 0.005, "alias rms {r}");
    }

    #[test]
    fn block_size_independent() {
        let input = sine(44_100, 700.0, 0.5, 0.3);
        let a = run(44_100, &input, 1);
        let b = run(44_100, &input, 4096);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-6);
        }
    }

    #[test]
    fn passthrough_is_identity() {
        let input = sine(16_000, 300.0, 0.1, 0.3);
        assert_eq!(run(16_000, &input, 100), input);
    }
}
