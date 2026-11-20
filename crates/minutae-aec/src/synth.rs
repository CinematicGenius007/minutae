//! Deterministic synthetic signals for tests and benchmarks: speech-like audio, room
//! impulse responses, delay and clock drift. No microphone, no files, no randomness
//! from the OS — every signal is a pure function of its seed.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::SAMPLE_RATE;

/// Small, fast, seedable PRNG (SplitMix64).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
    /// Approximately standard normal (sum of 4 uniforms).
    pub fn gauss(&mut self) -> f32 {
        (self.unit() + self.unit() + self.unit() + self.unit() - 2.0) * 1.732
    }
}

/// Vowel formants (F1, F2, F3) in Hz.
const VOWELS: [[f32; 3]; 6] = [
    [730.0, 1090.0, 2440.0],
    [530.0, 1840.0, 2480.0],
    [270.0, 2290.0, 3010.0],
    [570.0, 840.0, 2410.0],
    [300.0, 870.0, 2240.0],
    [660.0, 1720.0, 2410.0],
];

/// Two-pole resonator.
struct Resonator {
    a1: f32,
    a2: f32,
    gain: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    fn new() -> Self {
        Self {
            a1: 0.0,
            a2: 0.0,
            gain: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    fn tune(&mut self, freq: f32, bw: f32) {
        let r = (-std::f32::consts::PI * bw / SAMPLE_RATE as f32).exp();
        let w = 2.0 * std::f32::consts::PI * freq / SAMPLE_RATE as f32;
        self.a1 = 2.0 * r * w.cos();
        self.a2 = -r * r;
        self.gain = 1.0 - r;
    }
    fn step(&mut self, x: f32) -> f32 {
        let y = self.gain * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Speech-like audio: words of 1–4 voiced/unvoiced syllables with pitch movement and
/// formant changes, separated by pauses. `f0` sets the voice (≈110 male, ≈210 female).
/// Output RMS during speech is about 0.1 (-20 dBFS).
pub fn speech_like(seconds: f32, seed: u64, f0: f32) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    let sr = SAMPLE_RATE as f32;
    let mut rng = Rng::new(seed);
    let mut out = vec![0.0f32; n];
    let mut formants = [Resonator::new(), Resonator::new(), Resonator::new()];
    let mut pos = 0usize;
    let mut phase = 0.0f32;
    while pos < n {
        // A word.
        let syllables = 1 + (rng.next_u64() % 4) as usize;
        for _ in 0..syllables {
            let len = (rng.range(0.12, 0.28) * sr) as usize;
            let vowel = VOWELS[(rng.next_u64() % VOWELS.len() as u64) as usize];
            for (i, r) in formants.iter_mut().enumerate() {
                r.tune(vowel[i] * rng.range(0.9, 1.1), 60.0 + 40.0 * i as f32);
            }
            let voiced = rng.unit() < 0.8;
            let pitch0 = f0 * rng.range(0.85, 1.2);
            let pitch1 = pitch0 * rng.range(0.85, 1.15);
            for i in 0..len {
                if pos >= n {
                    break;
                }
                let t = i as f32 / len as f32;
                let env = (std::f32::consts::PI * t).sin().powf(0.6);
                let pitch = pitch0 + (pitch1 - pitch0) * t;
                phase += pitch / sr;
                let excitation = if voiced {
                    // Glottal-ish pulse train plus a little aspiration noise.
                    let pulse = if phase >= 1.0 {
                        phase -= 1.0;
                        1.0
                    } else {
                        0.0
                    };
                    pulse * 6.0 + rng.gauss() * 0.05
                } else {
                    rng.gauss() * 0.6
                };
                let mut y = 0.0;
                for r in formants.iter_mut() {
                    y += r.step(excitation);
                }
                out[pos] = y * env;
                pos += 1;
            }
        }
        // Pause between words.
        pos += (rng.range(0.08, 0.5) * sr) as usize;
    }
    normalize_active(&mut out, 0.1);
    out
}

/// Scale so the RMS over samples louder than 1% of peak equals `target`.
pub fn normalize_active(x: &mut [f32], target: f32) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak == 0.0 {
        return;
    }
    let (mut e, mut c) = (0.0f64, 0usize);
    for v in x.iter() {
        if v.abs() > 0.01 * peak {
            e += (*v as f64).powi(2);
            c += 1;
        }
    }
    let rms = (e / c.max(1) as f64).sqrt() as f32;
    if rms > 0.0 {
        let g = target / rms;
        x.iter_mut().for_each(|v| *v *= g);
    }
}

/// Multiplies `x` by 1 inside the given `[start, end)` second ranges and 0 elsewhere,
/// with 10 ms fades so the gates don't click.
pub fn gate(x: &mut [f32], ranges: &[(f32, f32)]) {
    let sr = SAMPLE_RATE as f32;
    let fade = (0.01 * sr) as usize;
    let mut g = vec![0.0f32; x.len()];
    for &(a, b) in ranges {
        let (a, b) = ((a * sr) as usize, ((b * sr) as usize).min(x.len()));
        for (i, v) in g.iter_mut().enumerate().take(b).skip(a) {
            let edge = (i - a).min(b - 1 - i);
            *v = v.max((edge as f32 / fade as f32).min(1.0));
        }
    }
    x.iter_mut().zip(g).for_each(|(v, g)| *v *= g);
}

/// Room impulse response: direct path at `pre_delay_ms`, then an exponentially decaying
/// diffuse tail (`rt60_s`) of `len_ms`. Peak gain `gain`.
pub fn room_ir(len_ms: f32, rt60_s: f32, gain: f32, seed: u64) -> Vec<f32> {
    let sr = SAMPLE_RATE as f32;
    let n = (len_ms / 1000.0 * sr) as usize;
    let mut rng = Rng::new(seed);
    let mut h = vec![0.0f32; n.max(1)];
    h[0] = 1.0;
    // A few early reflections.
    for _ in 0..6 {
        let at = (rng.range(0.002, 0.02) * sr) as usize;
        if at < n {
            h[at] += rng.range(-0.5, 0.5);
        }
    }
    let start = (0.003 * sr) as usize;
    for (i, v) in h.iter_mut().enumerate().skip(start) {
        let t = i as f32 / sr;
        *v += 0.25 * rng.gauss() * (-6.9 * t / rt60_s).exp();
    }
    h.iter_mut().for_each(|v| *v *= gain);
    h
}

/// Linear convolution via FFT overlap-add; output has `x.len()` samples.
pub fn convolve(x: &[f32], h: &[f32]) -> Vec<f32> {
    let m = h.len();
    let fft_len = (2 * m).next_power_of_two().max(1024);
    let step = fft_len - m + 1;
    let mut planner = FftPlanner::<f32>::new();
    let fwd = planner.plan_fft_forward(fft_len);
    let inv = planner.plan_fft_inverse(fft_len);
    let mut hf: Vec<Complex32> = (0..fft_len)
        .map(|i| Complex32::new(h.get(i).copied().unwrap_or(0.0), 0.0))
        .collect();
    fwd.process(&mut hf);
    let mut out = vec![0.0f32; x.len() + fft_len];
    let mut buf = vec![Complex32::new(0.0, 0.0); fft_len];
    let scale = 1.0 / fft_len as f32;
    let mut pos = 0;
    while pos < x.len() {
        for (i, b) in buf.iter_mut().enumerate() {
            let v = if i < step {
                x.get(pos + i).copied()
            } else {
                None
            };
            *b = Complex32::new(v.unwrap_or(0.0), 0.0);
        }
        fwd.process(&mut buf);
        buf.iter_mut().zip(&hf).for_each(|(b, h)| *b *= h);
        inv.process(&mut buf);
        for (i, b) in buf.iter().enumerate() {
            out[pos + i] += b.re * scale;
        }
        pos += step;
    }
    out.truncate(x.len());
    out
}

/// Delays `x` by `ms` (zeros in front), keeping the length.
pub fn delay(x: &[f32], ms: f32) -> Vec<f32> {
    let d = (ms / 1000.0 * SAMPLE_RATE as f32).round() as usize;
    let mut out = vec![0.0f32; x.len()];
    if d < x.len() {
        out[d..].copy_from_slice(&x[..x.len() - d]);
    }
    out
}

/// Simulates a capture clock that runs `ppm` parts-per-million slower than the
/// playback clock: output sample `n` is input at time `n · (1 + ppm·1e-6)`.
pub fn drift(x: &[f32], ppm: f32) -> Vec<f32> {
    let ratio = 1.0 + ppm as f64 * 1e-6;
    (0..x.len())
        .map(|n| crate::align::sample_at(x, n as f64 * ratio))
        .collect()
}

/// White noise at `rms`.
pub fn noise(len: usize, rms: f32, seed: u64) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    (0..len).map(|_| rng.gauss() * rms).collect()
}

/// Energy (sum of squares) of `x[a..b]` in seconds.
pub fn energy(x: &[f32], a_s: f32, b_s: f32) -> f64 {
    let sr = SAMPLE_RATE as f32;
    let (a, b) = ((a_s * sr) as usize, ((b_s * sr) as usize).min(x.len()));
    x[a.min(b)..b].iter().map(|v| (*v as f64).powi(2)).sum()
}

/// A synthetic one-minute laptop call (speakers, no headphones). Timeline (s):
/// far end talks 0–30 and 40–60; the user talks 20–40 (double-talk 20–30).
pub struct Scene {
    pub mic: Vec<f32>,
    pub reference: Vec<f32>,
    pub echo: Vec<f32>,
    pub near: Vec<f32>,
}

pub fn laptop_call(delay_ms: f32, drift_ppm: f32, seed: u64) -> Scene {
    let secs = 60.0;
    let mut far = speech_like(secs, seed, 200.0);
    gate(&mut far, &[(0.0, 30.0), (40.0, 60.0)]);
    let mut near = speech_like(secs, seed + 100, 115.0);
    gate(&mut near, &[(20.0, 40.0)]);
    // Laptop speaker → mic: a reverberant path, echo about 6 dB below the user's voice.
    let h = room_ir(200.0, 0.3, 0.45, seed + 7);
    let echo = drift(&delay(&convolve(&far, &h), delay_ms), drift_ppm);
    let floor = noise(far.len(), 3e-4, seed + 9);
    let mic = echo
        .iter()
        .zip(&near)
        .zip(&floor)
        .map(|((e, n), f)| e + n + f)
        .collect();
    Scene {
        mic,
        reference: far,
        echo,
        near,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_is_deterministic_and_bursty() {
        let a = speech_like(3.0, 7, 120.0);
        let b = speech_like(3.0, 7, 120.0);
        assert_eq!(a, b);
        assert_ne!(a, speech_like(3.0, 8, 120.0));
        // Has pauses: some 20 ms frames are (near) silent.
        let silent = a
            .chunks(320)
            .filter(|f| f.iter().all(|v| v.abs() < 1e-3))
            .count();
        assert!(silent > 3, "{silent}");
    }

    #[test]
    fn convolve_matches_direct_form() {
        let x = noise(3000, 0.1, 1);
        let h = room_ir(20.0, 0.2, 0.5, 2);
        let fast = convolve(&x, &h);
        for n in [0usize, 10, 400, 2999] {
            let direct: f32 = (0..h.len())
                .filter(|k| *k <= n)
                .map(|k| h[k] * x[n - k])
                .sum();
            assert!(
                (fast[n] - direct).abs() < 1e-4,
                "{n}: {} vs {direct}",
                fast[n]
            );
        }
    }

    #[test]
    fn delay_and_drift() {
        let x: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(delay(&x, 1.0)[16], 0.0);
        assert_eq!(delay(&x, 1.0)[17], 1.0);
        let d = drift(&x, 1000.0); // 0.1%
        assert!((d[500] - 500.5).abs() < 1e-3, "{}", d[500]);
    }
}
