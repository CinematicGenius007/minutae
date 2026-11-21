//! Global shortcuts that arrive as separate press / release signals: the XDG
//! Desktop Portal `GlobalShortcuts` interface used on Wayland, where X11 key
//! grabs (tauri-plugin-global-shortcut) do not work.
//!
//! - [`ShortcutPortal`]: the backend seam. [`crate::platform::portal::XdgShortcutPortal`]
//!   talks D-Bus on Linux; [`FakePortal`] is driven by tests.
//! - [`SignalTranslator`]: turns `Activated` / `Deactivated` into dictation key
//!   presses and releases, and notices when the compositor never reports a
//!   release, so hold-to-talk falls back to press-to-toggle.
//! - [`run_router`]: pumps a portal's signals through the translator into a [`KeySink`].
//!
//! Assumption: a compositor sends one `Activated` per physical press (no
//! auto-repeat). Some send `Deactivated` immediately after `Activated`, and some
//! never send it; both mean "no release information".

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use minutae_core::HotkeyMode;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// The shortcuts Minutae binds through the portal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutId {
    Dictate,
    Record,
}

impl ShortcutId {
    /// The id sent to the portal.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictate => "dictate",
            Self::Record => "record",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "dictate" => Some(Self::Dictate),
            "record" => Some(Self::Record),
            _ => None,
        }
    }
}

/// One shortcut to bind. `preferred_trigger` is only a hint: the portal shows its
/// own dialog and the user picks the keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutRequest {
    pub id: ShortcutId,
    pub description: String,
    /// In the XDG "shortcuts" format, e.g. `ALT+space` (see [`xdg_trigger`]).
    pub preferred_trigger: Option<String>,
}

/// What the portal bound. `trigger` is its human-readable description
/// (e.g. "Alt+Space"); empty when the user left it unassigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundShortcut {
    pub id: ShortcutId,
    pub trigger: String,
}

/// A portal signal. `at_ms` is the portal's timestamp in milliseconds (or the
/// local monotonic time when the portal sends 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutSignal {
    Activated { id: ShortcutId, at_ms: u64 },
    Deactivated { id: ShortcutId, at_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortalError {
    /// No portal, or the portal frontend has no GlobalShortcuts backend.
    #[error("global shortcuts portal unavailable: {0}")]
    Unavailable(String),
    /// The user closed the portal's dialog.
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(String),
}

/// A live binding: what the portal assigned, and its signals.
pub struct PortalBinding {
    pub shortcuts: Vec<BoundShortcut>,
    /// The portal can open its own shortcut settings (interface version ≥ 2).
    pub can_configure: bool,
    pub signals: mpsc::UnboundedReceiver<ShortcutSignal>,
}

/// A press/release global shortcut backend.
#[async_trait]
pub trait ShortcutPortal: Send + Sync {
    /// Creates a session and binds `requests`. Call once; the session lives as long
    /// as the backend.
    async fn bind(&self, requests: &[ShortcutRequest]) -> Result<PortalBinding, PortalError>;
    /// Opens the portal's own UI for changing the keys (`ConfigureShortcuts`).
    async fn configure(&self) -> Result<(), PortalError>;
}

/// Whether the compositor reports key releases (`Deactivated`) we can trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseSupport {
    /// No complete press seen yet.
    Unknown,
    Reported,
    /// Only presses: hold-to-talk becomes press-to-toggle.
    Missing,
}

impl ReleaseSupport {
    /// The dictation mode to run with: the user's choice, unless releases are missing.
    pub fn effective_mode(self, chosen: HotkeyMode) -> HotkeyMode {
        if self == Self::Missing {
            HotkeyMode::Toggle
        } else {
            chosen
        }
    }
}

/// What the app should do for a signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    DictateDown,
    DictateUp,
    ToggleRecording,
}

/// Signal → key action, with release detection. Pure; no clock of its own.
#[derive(Debug, Clone)]
pub struct SignalTranslator {
    release: ReleaseSupport,
    dictate_down_at: Option<u64>,
}

impl Default for SignalTranslator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalTranslator {
    /// A release this soon after the press is synthetic (no human taps that fast).
    pub const INSTANT_RELEASE_MS: u64 = 20;

    pub fn new() -> Self {
        Self {
            release: ReleaseSupport::Unknown,
            dictate_down_at: None,
        }
    }

    pub fn release_support(&self) -> ReleaseSupport {
        self.release
    }

    pub fn on_signal(&mut self, signal: ShortcutSignal) -> Option<KeyAction> {
        match signal {
            ShortcutSignal::Activated {
                id: ShortcutId::Record,
                ..
            } => Some(KeyAction::ToggleRecording),
            ShortcutSignal::Deactivated {
                id: ShortcutId::Record,
                ..
            } => None,
            ShortcutSignal::Activated { at_ms, .. } => match self.release {
                ReleaseSupport::Missing => Some(KeyAction::DictateDown),
                ReleaseSupport::Reported if self.dictate_down_at.is_some() => None, // repeat
                ReleaseSupport::Unknown if self.dictate_down_at.is_some() => {
                    // A second press with no release in between: releases never come.
                    self.release = ReleaseSupport::Missing;
                    self.dictate_down_at = None;
                    Some(KeyAction::DictateDown)
                }
                _ => {
                    self.dictate_down_at = Some(at_ms);
                    Some(KeyAction::DictateDown)
                }
            },
            ShortcutSignal::Deactivated { at_ms, .. } => {
                if self.release == ReleaseSupport::Missing {
                    return None;
                }
                let down = self.dictate_down_at.take()?;
                if self.release == ReleaseSupport::Unknown
                    && at_ms.saturating_sub(down) < Self::INSTANT_RELEASE_MS
                {
                    self.release = ReleaseSupport::Missing;
                    return None;
                }
                self.release = ReleaseSupport::Reported;
                Some(KeyAction::DictateUp)
            }
        }
    }
}

/// Where translated actions go. The app sends them to the dictation worker and
/// the recorder; tests call the controller directly.
pub trait KeySink: Send + Sync {
    /// Called once when release support becomes known, *before* the action that
    /// revealed it, so the controller can switch to toggle mode first.
    fn release_support(&self, support: ReleaseSupport);
    fn dictate(&self, down: bool);
    fn toggle_recording(&self);
}

/// Forwards `signals` to `sink` until the stream ends.
pub async fn run_router(
    mut signals: mpsc::UnboundedReceiver<ShortcutSignal>,
    sink: Arc<dyn KeySink>,
) {
    let mut t = SignalTranslator::new();
    while let Some(signal) = signals.recv().await {
        let before = t.release_support();
        let action = t.on_signal(signal);
        let after = t.release_support();
        if after != before {
            tracing::info!(?after, "global shortcut portal release support");
            sink.release_support(after);
        }
        match action {
            Some(KeyAction::DictateDown) => sink.dictate(true),
            Some(KeyAction::DictateUp) => sink.dictate(false),
            Some(KeyAction::ToggleRecording) => sink.toggle_recording(),
            None => {}
        }
    }
}

/// Tauri accelerator (`CommandOrControl+Alt+R`, `Alt+Space`) → XDG shortcuts
/// trigger (`CTRL+ALT+r`, `ALT+space`). `None` if it has no key.
pub fn xdg_trigger(accelerator: &str) -> Option<String> {
    let mut mods = vec![];
    let mut key = None;
    for part in accelerator
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        match part.to_ascii_lowercase().as_str() {
            "commandorcontrol" | "cmdorctrl" | "control" | "ctrl" => mods.push("CTRL"),
            "alt" | "option" => mods.push("ALT"),
            "shift" => mods.push("SHIFT"),
            "super" | "meta" | "command" | "cmd" => mods.push("LOGO"),
            k => {
                key = Some(match k {
                    "space" => "space".to_string(),
                    "enter" | "return" => "Return".to_string(),
                    "tab" => "Tab".to_string(),
                    "escape" | "esc" => "Escape".to_string(),
                    "backspace" => "BackSpace".to_string(),
                    k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => {
                        k.to_ascii_uppercase()
                    }
                    k if k.starts_with("key") && k.len() == 4 => k[3..].to_string(),
                    k if k.starts_with("digit") && k.len() == 6 => k[5..].to_string(),
                    k => k.to_string(),
                })
            }
        }
    }
    let key = key?;
    mods.dedup();
    mods.push(&key);
    Some(mods.join("+"))
}

/// Test backend: returns preset bindings; tests push signals with [`FakePortal::send`].
pub struct FakePortal {
    pub result: Mutex<Result<Vec<BoundShortcut>, PortalError>>,
    pub can_configure: bool,
    pub requests: Mutex<Vec<ShortcutRequest>>,
    pub configured: Mutex<u32>,
    tx: Mutex<Option<mpsc::UnboundedSender<ShortcutSignal>>>,
}

impl FakePortal {
    /// Binds every request to its preferred trigger.
    pub fn new(can_configure: bool) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(vec![])),
            can_configure,
            requests: Mutex::new(vec![]),
            configured: Mutex::new(0),
            tx: Mutex::new(None),
        })
    }

    pub fn unavailable() -> Arc<Self> {
        let p = Self::new(false);
        *p.result.lock().unwrap() = Err(PortalError::Unavailable("no portal".into()));
        p
    }

    pub fn send(&self, signal: ShortcutSignal) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(signal);
        }
    }

    /// Ends the signal stream (the router returns).
    pub fn close(&self) {
        self.tx.lock().unwrap().take();
    }
}

#[async_trait]
impl ShortcutPortal for FakePortal {
    async fn bind(&self, requests: &[ShortcutRequest]) -> Result<PortalBinding, PortalError> {
        *self.requests.lock().unwrap() = requests.to_vec();
        let preset = self.result.lock().unwrap().clone()?;
        let shortcuts = if preset.is_empty() {
            requests
                .iter()
                .map(|r| BoundShortcut {
                    id: r.id,
                    trigger: r.preferred_trigger.clone().unwrap_or_default(),
                })
                .collect()
        } else {
            preset
        };
        let (tx, signals) = mpsc::unbounded_channel();
        *self.tx.lock().unwrap() = Some(tx);
        Ok(PortalBinding {
            shortcuts,
            can_configure: self.can_configure,
            signals,
        })
    }

    async fn configure(&self) -> Result<(), PortalError> {
        if !self.can_configure {
            return Err(PortalError::Failed("not supported".into()));
        }
        *self.configured.lock().unwrap() += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(id: ShortcutId, at_ms: u64) -> ShortcutSignal {
        ShortcutSignal::Activated { id, at_ms }
    }
    fn deact(id: ShortcutId, at_ms: u64) -> ShortcutSignal {
        ShortcutSignal::Deactivated { id, at_ms }
    }
    use ShortcutId::{Dictate, Record};

    #[test]
    fn press_and_release_pass_through() {
        let mut t = SignalTranslator::new();
        assert_eq!(t.on_signal(act(Dictate, 0)), Some(KeyAction::DictateDown));
        assert_eq!(t.on_signal(deact(Dictate, 900)), Some(KeyAction::DictateUp));
        assert_eq!(t.release_support(), ReleaseSupport::Reported);
        // Later repeats while held are ignored; a stray release is ignored.
        assert_eq!(
            t.on_signal(act(Dictate, 2000)),
            Some(KeyAction::DictateDown)
        );
        assert_eq!(t.on_signal(act(Dictate, 2030)), None);
        assert_eq!(
            t.on_signal(deact(Dictate, 2500)),
            Some(KeyAction::DictateUp)
        );
        assert_eq!(t.on_signal(deact(Dictate, 2600)), None);
    }

    #[test]
    fn two_presses_without_release_mean_missing() {
        let mut t = SignalTranslator::new();
        assert_eq!(t.on_signal(act(Dictate, 0)), Some(KeyAction::DictateDown));
        assert_eq!(
            t.on_signal(act(Dictate, 3000)),
            Some(KeyAction::DictateDown)
        );
        assert_eq!(t.release_support(), ReleaseSupport::Missing);
        assert_eq!(
            t.on_signal(act(Dictate, 5000)),
            Some(KeyAction::DictateDown)
        );
        assert_eq!(t.on_signal(deact(Dictate, 5001)), None);
    }

    #[test]
    fn instant_release_means_missing() {
        let mut t = SignalTranslator::new();
        assert_eq!(t.on_signal(act(Dictate, 100)), Some(KeyAction::DictateDown));
        assert_eq!(t.on_signal(deact(Dictate, 101)), None);
        assert_eq!(t.release_support(), ReleaseSupport::Missing);
        assert_eq!(
            ReleaseSupport::Missing.effective_mode(HotkeyMode::Hold),
            HotkeyMode::Toggle
        );
        assert_eq!(
            ReleaseSupport::Reported.effective_mode(HotkeyMode::Hold),
            HotkeyMode::Hold
        );
    }

    #[test]
    fn record_toggles_on_press_only() {
        let mut t = SignalTranslator::new();
        assert_eq!(
            t.on_signal(act(Record, 0)),
            Some(KeyAction::ToggleRecording)
        );
        assert_eq!(t.on_signal(deact(Record, 1)), None);
        assert_eq!(t.release_support(), ReleaseSupport::Unknown);
    }

    #[test]
    fn accelerators_become_xdg_triggers() {
        assert_eq!(xdg_trigger("Alt+Space").as_deref(), Some("ALT+space"));
        assert_eq!(
            xdg_trigger("CommandOrControl+Alt+R").as_deref(),
            Some("CTRL+ALT+r")
        );
        assert_eq!(
            xdg_trigger("Ctrl+Shift+KeyD").as_deref(),
            Some("CTRL+SHIFT+d")
        );
        assert_eq!(xdg_trigger("Super+F9").as_deref(), Some("LOGO+F9"));
        assert_eq!(xdg_trigger("Alt+").as_deref(), None);
        assert_eq!(ShortcutId::parse("record"), Some(ShortcutId::Record));
        assert_eq!(ShortcutId::parse("x"), None);
    }
}
