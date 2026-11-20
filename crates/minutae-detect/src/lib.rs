//! minutae-detect: notice when another app starts using the microphone
//! (a call starting), with the app's name when the OS exposes it.
//!
//! - macOS: Core Audio process objects (`kAudioHardwarePropertyProcessObjectList`,
//!   `kAudioProcessPropertyIsRunningInput`, bundle id; macOS 14+), falling back
//!   to `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device.
//! - Windows: the Capability Access Manager consent store in HKCU
//!   (`LastUsedTimeStop == 0` means the app is using the mic right now).
//! - Linux: `pactl list source-outputs` (PulseAudio or PipeWire-Pulse).
//!
//! ```no_run
//! let mut mon = minutae_detect::start_monitor(std::time::Duration::from_secs(1))?;
//! while let Some(ev) = mon.events.blocking_recv() {
//!     println!("{ev:?}");
//! }
//! # Ok::<(), minutae_detect::DetectError>(())
//! ```

pub mod names;
pub mod pactl;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("microphone detection is not supported on this platform")]
    Unsupported,
    #[error("microphone detection failed: {0}")]
    Os(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = DetectError> = std::result::Result<T, E>;

/// Emitted only when the state changes (after debouncing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MicEvent {
    /// Another app is using the microphone. `app` is a display name like "Zoom";
    /// `process` identifies it where the OS says (macOS), so its audio can be recorded.
    Busy {
        app: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        process: Option<MicProcess>,
    },
    /// No other app is using the microphone.
    Free,
}

/// The process behind a [`MicEvent::Busy`] app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MicProcess {
    pub pid: i32,
    pub bundle_id: Option<String>,
}

/// One observation from a [`MicMonitor`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MicActivity {
    pub busy: bool,
    /// Display names of the apps using the mic (ours excluded), best first.
    pub apps: Vec<String>,
    /// The process of `apps[0]` (lowest pid of that app), when known.
    pub process: Option<MicProcess>,
}

impl MicActivity {
    pub fn free() -> Self {
        Self::default()
    }

    pub fn busy(apps: Vec<String>) -> Self {
        Self {
            busy: true,
            apps,
            process: None,
        }
    }

    fn to_event(&self) -> MicEvent {
        if self.busy {
            MicEvent::Busy {
                app: self.apps.first().cloned(),
                process: self.process.clone(),
            }
        } else {
            MicEvent::Free
        }
    }
}

/// A platform probe: "who is using the mic right now?". Must exclude our own process.
pub trait MicMonitor: Send {
    fn poll(&mut self) -> Result<MicActivity>;
}

/// The probe for the current OS.
pub fn platform_monitor() -> Result<Box<dyn MicMonitor>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::CoreAudioMonitor::new()))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::ConsentStoreMonitor::new()))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::PactlMonitor::new()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(DetectError::Unsupported)
    }
}

/// Turns raw observations into change events. A new state must be seen
/// `confirm` polls in a row before it is emitted (filters blips such as a
/// system sound briefly opening the mic).
#[derive(Debug)]
pub struct Debouncer {
    confirm: u32,
    emitted: Option<MicEvent>,
    candidate: Option<(MicEvent, u32)>,
}

impl Debouncer {
    pub fn new(confirm: u32) -> Self {
        Self {
            confirm: confirm.max(1),
            emitted: None,
            candidate: None,
        }
    }

    /// Feed one observation; returns an event if the debounced state changed.
    /// The very first observation is emitted only if it is `Busy` (so starting
    /// the app mid-call prompts immediately, but a quiet start emits nothing).
    pub fn feed(&mut self, ev: MicEvent) -> Option<MicEvent> {
        let current = self.emitted.clone().unwrap_or(MicEvent::Free);
        if ev == current {
            self.candidate = None;
            if self.emitted.is_none() {
                self.emitted = Some(ev);
            }
            return None;
        }
        let count = match &self.candidate {
            Some((c, n)) if *c == ev => n + 1,
            _ => 1,
        };
        if count >= self.confirm {
            self.candidate = None;
            self.emitted = Some(ev.clone());
            Some(ev)
        } else {
            self.candidate = Some((ev, count));
            None
        }
    }
}

/// A running monitor. Dropping it stops the polling thread.
pub struct MonitorHandle {
    pub events: mpsc::UnboundedReceiver<MicEvent>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MonitorHandle {
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for MonitorHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Start polling the current OS every `poll` (1 s is a good default; the
/// probe is cheap and keeps idle CPU well under 1%). Events are debounced
/// over 2 polls.
pub fn start_monitor(poll: Duration) -> Result<MonitorHandle> {
    start_monitor_with(platform_monitor()?, poll, 2)
}

/// Like [`start_monitor`] with a custom probe (tests, fakes) and debounce count.
pub fn start_monitor_with(
    mut monitor: Box<dyn MicMonitor>,
    poll: Duration,
    confirm: u32,
) -> Result<MonitorHandle> {
    let (tx, rx) = mpsc::unbounded_channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let thread = std::thread::Builder::new()
        .name("minutae-mic-monitor".into())
        .spawn(move || {
            let mut deb = Debouncer::new(confirm);
            let mut last_err: Option<String> = None;
            while !stop2.load(Ordering::SeqCst) {
                match monitor.poll() {
                    Ok(activity) => {
                        last_err = None;
                        if let Some(ev) = deb.feed(activity.to_event())
                            && tx.send(ev).is_err()
                        {
                            break; // receiver gone
                        }
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if last_err.as_deref() != Some(&msg) {
                            tracing::warn!(error = %msg, "mic monitor poll failed");
                            last_err = Some(msg);
                        }
                    }
                }
                // Sleep in small steps so stop() is prompt.
                let mut left = poll;
                while !left.is_zero() && !stop2.load(Ordering::SeqCst) {
                    let step = left.min(Duration::from_millis(50));
                    std::thread::sleep(step);
                    left -= step;
                }
            }
        })?;
    Ok(MonitorHandle {
        events: rx,
        stop,
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    fn busy(app: &str) -> MicEvent {
        MicEvent::Busy {
            app: Some(app.into()),
            process: None,
        }
    }

    #[test]
    fn busy_carries_the_process() {
        let a = MicActivity {
            busy: true,
            apps: vec!["Zoom".into()],
            process: Some(MicProcess {
                pid: 42,
                bundle_id: Some("us.zoom.xos".into()),
            }),
        };
        let ev = a.to_event();
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::json!({"kind": "busy", "app": "Zoom", "process": {"pid": 42, "bundleId": "us.zoom.xos"}})
        );
    }

    #[test]
    fn debounce_requires_confirmation() {
        let mut d = Debouncer::new(2);
        assert_eq!(d.feed(MicEvent::Free), None);
        assert_eq!(d.feed(busy("Zoom")), None); // 1st sighting
        assert_eq!(d.feed(MicEvent::Free), None); // blip cancelled
        assert_eq!(d.feed(busy("Zoom")), None);
        assert_eq!(d.feed(busy("Zoom")), Some(busy("Zoom")));
        assert_eq!(d.feed(busy("Zoom")), None); // no repeats
        assert_eq!(d.feed(MicEvent::Free), None);
        assert_eq!(d.feed(MicEvent::Free), Some(MicEvent::Free));
        assert_eq!(d.feed(MicEvent::Free), None);
    }

    #[test]
    fn debounce_app_change_is_an_event() {
        let mut d = Debouncer::new(1);
        assert_eq!(d.feed(busy("Zoom")), Some(busy("Zoom")));
        assert_eq!(d.feed(busy("Teams")), Some(busy("Teams")));
    }

    #[test]
    fn first_free_is_silent() {
        let mut d = Debouncer::new(1);
        assert_eq!(d.feed(MicEvent::Free), None);
        assert_eq!(d.feed(MicEvent::Free), None);
    }

    struct Scripted(VecDeque<MicActivity>);
    impl MicMonitor for Scripted {
        fn poll(&mut self) -> Result<MicActivity> {
            Ok(self.0.pop_front().unwrap_or_else(MicActivity::free))
        }
    }

    #[test]
    fn monitor_thread_emits_changes() {
        let script = Scripted(
            vec![
                MicActivity::free(),
                MicActivity::busy(vec!["Zoom".into()]),
                MicActivity::busy(vec!["Zoom".into()]),
                MicActivity::busy(vec!["Zoom".into()]),
                MicActivity::free(),
                MicActivity::free(),
            ]
            .into(),
        );
        let mut h = start_monitor_with(Box::new(script), Duration::from_millis(1), 2).unwrap();
        assert_eq!(h.events.blocking_recv(), Some(busy("Zoom")));
        assert_eq!(h.events.blocking_recv(), Some(MicEvent::Free));
        h.stop();
    }

    #[test]
    fn event_serializes_for_the_ui() {
        let s = serde_json::to_string(&busy("Zoom")).unwrap();
        assert_eq!(s, r#"{"kind":"busy","app":"Zoom"}"#);
    }

    #[test]
    fn platform_probe_runs() {
        // Real OS call; must not error or panic (result depends on the machine).
        if let Ok(mut m) = platform_monitor() {
            let r = m.poll();
            eprintln!("platform poll: {r:?}");
        }
    }
}
