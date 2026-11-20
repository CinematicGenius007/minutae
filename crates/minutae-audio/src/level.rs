//! RMS level metering and the recording clock.

use std::sync::{Arc, Mutex};
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

/// Source of "now"; injectable so tests don't depend on scheduler timing.
pub type TimeSource = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Wall clock that excludes paused time.
pub struct Clock {
    now: TimeSource,
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
        Self::with_source(Arc::new(Instant::now))
    }

    pub fn with_source(now: TimeSource) -> Self {
        let start = now();
        Self {
            now,
            inner: Mutex::new(ClockInner {
                start,
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
        let now = (self.now)();
        let paused_now = c
            .paused_since
            .map(|p| now.saturating_duration_since(p))
            .unwrap_or_default();
        now.saturating_duration_since(c.start)
            .saturating_sub(c.paused_total + paused_now)
    }

    pub fn active_samples(&self, rate: u32) -> u64 {
        (self.active().as_secs_f64() * rate as f64) as u64
    }

    pub fn pause(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if c.paused_since.is_none() {
            c.paused_since = Some((self.now)());
        }
    }

    pub fn resume(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = c.paused_since.take() {
            c.paused_total += (self.now)().saturating_duration_since(p);
        }
    }

    /// Reset time zero to now (clears pauses).
    pub fn restart(&self) {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        c.start = (self.now)();
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
pub(crate) mod tests {
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

    /// Manually advanced time source.
    pub(crate) fn manual_time() -> (TimeSource, Arc<Mutex<Instant>>) {
        let t = Arc::new(Mutex::new(Instant::now()));
        let t2 = t.clone();
        (Arc::new(move || *t2.lock().unwrap()), t)
    }

    pub(crate) fn advance(t: &Mutex<Instant>, ms: u64) {
        *t.lock().unwrap() += Duration::from_millis(ms);
    }

    #[test]
    fn clock_excludes_pause() {
        let (src, t) = manual_time();
        let c = Clock::with_source(src);
        advance(&t, 30);
        c.pause();
        let at_pause = c.active();
        assert_eq!(at_pause, Duration::from_millis(30));
        advance(&t, 50);
        assert_eq!(c.active(), at_pause);
        c.pause(); // idempotent
        c.resume();
        advance(&t, 20);
        assert_eq!(c.active(), Duration::from_millis(50));
        assert_eq!(c.active_samples(16_000), 800);
        c.freeze();
        advance(&t, 10);
        assert_eq!(c.active(), Duration::from_millis(50));
        c.restart();
        assert_eq!(c.active(), Duration::ZERO);
    }
}
