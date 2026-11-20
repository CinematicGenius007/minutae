//! RMS level metering and the recording clock.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Accumulates sum of squares between reads.
#[derive(Default)]
pub struct Meter {
    acc: Mutex<(f64, u64)>,
}

impl Meter {
    pub fn add(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
        let mut a = self.acc.lock().unwrap_or_else(|e| e.into_inner());
        a.0 += sq;
        a.1 += samples.len() as u64;
    }

    /// RMS since the previous call (0.0 if nothing arrived), then reset.
    pub fn take(&self) -> f32 {
        let mut a = self.acc.lock().unwrap_or_else(|e| e.into_inner());
        let r = if a.1 == 0 {
            0.0
        } else {
            (a.0 / a.1 as f64).sqrt() as f32
        };
        *a = (0.0, 0);
        r
    }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sq / samples.len() as f64).sqrt() as f32
}

/// Wall clock that excludes paused time.
pub struct Clock {
    inner: Mutex<ClockInner>,
}

struct ClockInner {
    start: Instant,
    paused_total: Duration,
    paused_since: Option<Instant>,
    frozen: Option<Duration>,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ClockInner {
                start: Instant::now(),
                paused_total: Duration::ZERO,
                paused_since: None,
                frozen: None,
            }),
        }
    }

    pub fn active(&self) -> Duration {
        let c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = c.frozen {
            return f;
        }
        let now = Instant::now();
        let paused_now = c.paused_since.map(|p| now - p).unwrap_or_default();
        (now - c.start).saturating_sub(c.paused_total + paused_now)
    }

    pub fn active_samples(&self, rate: u32) -> u64 {
        (self.active().as_secs_f64() * rate as f64) as u64
    }

    pub fn pause(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if c.paused_since.is_none() {
            c.paused_since = Some(Instant::now());
        }
    }

    pub fn resume(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = c.paused_since.take() {
            c.paused_total += p.elapsed();
        }
    }

    /// Reset time zero to now (clears pauses).
    pub fn restart(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        c.start = Instant::now();
        c.paused_total = Duration::ZERO;
        c.paused_since = None;
        c.frozen = None;
    }

    /// Stop the clock; `active()` returns this value from now on.
    pub fn freeze(&self) {
        let d = self.active();
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).frozen = Some(d);
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_sine_and_dc() {
        let s: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin())
            .collect();
        assert!((rms(&s) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert_eq!(rms(&[0.5; 100]), 0.5);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn meter_accumulates_and_resets() {
        let m = Meter::default();
        m.add(&[1.0; 10]);
        m.add(&[0.0; 30]);
        assert!((m.take() - 0.5).abs() < 1e-6);
        assert_eq!(m.take(), 0.0);
    }

    #[test]
    fn clock_excludes_pause() {
        let c = Clock::new();
        std::thread::sleep(Duration::from_millis(30));
        c.pause();
        let at_pause = c.active();
        std::thread::sleep(Duration::from_millis(50));
        assert!(c.active() - at_pause < Duration::from_millis(5));
        c.resume();
        std::thread::sleep(Duration::from_millis(20));
        let a = c.active();
        assert!(
            a >= Duration::from_millis(50) && a < Duration::from_millis(90),
            "{a:?}"
        );
        c.freeze();
        let f = c.active();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(c.active(), f);
    }
}
