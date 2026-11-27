//! Typing text into whatever app has focus.
//!
//! Strategy on every OS: save the clipboard, put the dictation on the
//! clipboard, simulate paste (⌘V / Ctrl+V), then restore the old contents after a
//! delay (longer for apps known to read the clipboard late) unless something else
//! changed the clipboard meanwhile. When paste cannot work (no Accessibility
//! permission, no focused text field, Wayland, secure input) the text is left on
//! the clipboard and the outcome is [`InsertOutcome::Copied`] so the HUD can say
//! a copy reason and the platform paste shortcut.
//!
//! Preserved: plain text, HTML (with its text alternative), file lists and images.
//! Other formats (RTF-only, app-private types) cannot be read back. An empty or
//! unreadable clipboard is cleared after a paste or cancellation.

use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    platform::{
        self,
        identity::{AppIdentity, AppIdentityLookup, SystemAppIdentityLookup},
    },
    shapes::{ShapePreset, suggested_preset},
};

/// Why the text went to the clipboard instead of being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyReason {
    /// macOS: the app has not been granted Accessibility.
    AccessibilityNotTrusted,
    /// The focused element is not something you can type into.
    NoTextField,
    /// Linux Wayland: other apps cannot be typed into.
    Wayland,
    /// macOS secure keyboard entry is on (e.g. a password prompt or Terminal setting).
    SecureInput,
    UnverifiedField,
    MultilineTerminal,
    /// Simulating the paste keystroke failed.
    PasteFailed,
    /// The user switched to another app while the dictation was transcribed: pasting would
    /// put the text somewhere they didn't dictate it for.
    FocusChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum InsertOutcome {
    /// Paste was sent to the focused app. Delivery is not confirmed by the OS.
    Inserted,
    /// The text is on the clipboard for the user to paste.
    Copied(CopyReason),
    /// Nothing was typed or copied.
    Failed(String),
    /// Cancellation won before a paste or copy was committed.
    Cancelled,
}

/// Orders cancellation against the irreversible paste keystroke or clipboard copy.
#[derive(Default)]
pub struct InsertionControl {
    committed: Mutex<bool>,
    cancelled: Arc<AtomicBool>,
    target: Option<(Option<AppIdentity>, Option<u32>)>,
    undo_target: Mutex<Option<UndoTarget>>,
}

impl InsertionControl {
    pub(crate) fn for_target(app: Option<AppIdentity>, process: Option<u32>) -> Self {
        Self {
            target: Some((app, process)),
            ..Self::default()
        }
    }

    pub(crate) fn undo_target(&self) -> Option<UndoTarget> {
        *self.undo_target.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn record_undo_target(&self, target: UndoTarget) {
        *self.undo_target.lock().unwrap_or_else(|e| e.into_inner()) = Some(target);
    }

    pub(crate) fn cancel(&self) -> bool {
        let committed = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        if *committed {
            return false;
        }
        self.cancelled.store(true, Ordering::Relaxed);
        true
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }

    /// Run the irreversible part if cancellation has not won.
    pub fn commit<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        let mut committed = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        if *committed || self.is_cancelled() {
            return None;
        }
        *committed = true;
        Some(action())
    }
}

/// Identifies the process instance that received the paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoTarget {
    pub process: u32,
    pub launched_at: u64,
}

/// Puts dictated text into the frontmost app.
#[async_trait]
pub trait Inserter: Send + Sync {
    /// Name of the app that has focus now, e.g. "Notes". Called on key-down,
    /// so it names the app the user was in when they started talking.
    fn target_app(&self) -> Option<String> {
        None
    }
    async fn insert(&self, text: &str, control: Arc<InsertionControl>) -> InsertOutcome;

    fn undo(&self, _target: UndoTarget) -> bool {
        false
    }
    /// Put `text` on the clipboard only, without pasting it anywhere.
    async fn copy(
        &self,
        text: &str,
        reason: CopyReason,
        control: Arc<InsertionControl>,
    ) -> InsertOutcome;
}

/// What the platform layer decided before touching the clipboard.
// Not every OS produces every variant.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Preflight {
    Paste,
    CopyOnly(CopyReason),
}

/// The real inserter: clipboard + simulated paste.
#[derive(Debug, Clone)]
pub struct SystemInserter {
    /// Wait after writing the clipboard before sending the keystroke.
    pub settle: Duration,
    /// Wait after the keystroke before restoring the previous clipboard.
    /// The target app reads the clipboard asynchronously, so this cannot be 0.
    pub restore_delay: Duration,
    /// Used instead of `restore_delay` when the frontmost app matches `slow_apps`.
    pub slow_restore_delay: Duration,
    /// Lowercase substrings of app names that read the clipboard late (Electron apps,
    /// remote desktop and VM clients). Matched against the frontmost app's name.
    pub slow_apps: Vec<String>,
}

/// Apps known to read the pasteboard well after ⌘V arrives.
pub const DEFAULT_SLOW_APPS: &[&str] = &[
    // Electron / web-view apps
    "slack",
    "discord",
    "visual studio code",
    "cursor",
    "notion",
    "obsidian",
    "microsoft teams",
    "figma",
    "linear",
    "whatsapp",
    "signal",
    // Remote desktop and virtual machines
    "remote desktop",
    "windows app",
    "citrix",
    "vmware",
    "parallels",
    "virtualbox",
    "teamviewer",
    "anydesk",
    "jump desktop",
    "screen sharing",
    "vnc",
    "rustdesk",
    "utm",
];

impl Default for SystemInserter {
    fn default() -> Self {
        // Cache the layout's ⌘V keycode now; this is a no-op unless on the main thread.
        let _ = crate::platform::refresh_paste_keycode();
        Self {
            settle: Duration::from_millis(40),
            restore_delay: Duration::from_millis(600),
            slow_restore_delay: Duration::from_millis(1500),
            slow_apps: DEFAULT_SLOW_APPS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// What was on the clipboard before we replaced it.
#[derive(Debug)]
enum Snapshot {
    Text(String),
    Html {
        html: String,
        alt: Option<String>,
    },
    Files(Vec<PathBuf>),
    Image(arboard::ImageData<'static>),
    /// Empty, or only formats we cannot read back.
    Unreadable,
}

impl Snapshot {
    fn take(clipboard: &mut arboard::Clipboard) -> Self {
        if let Ok(files) = clipboard.get().file_list()
            && !files.is_empty()
        {
            return Snapshot::Files(files);
        }
        let text = clipboard.get_text().ok();
        if let Ok(html) = clipboard.get().html()
            && !html.is_empty()
        {
            return Snapshot::Html { html, alt: text };
        }
        if let Some(text) = text {
            return Snapshot::Text(text);
        }
        if let Ok(image) = clipboard.get_image() {
            return Snapshot::Image(image);
        }
        Snapshot::Unreadable
    }

    fn restore(self, clipboard: &mut arboard::Clipboard) -> Result<(), arboard::Error> {
        match self {
            Snapshot::Text(t) => clipboard.set_text(t),
            Snapshot::Html { html, alt } => {
                clipboard.set().html(Cow::Owned(html), alt.map(Cow::Owned))
            }
            Snapshot::Files(f) => clipboard.set().file_list(&f),
            Snapshot::Image(i) => clipboard.set_image(i),
            Snapshot::Unreadable => clipboard.clear(),
        }
    }
}

impl SystemInserter {
    /// How long to wait before restoring the clipboard when `app` has focus.
    pub fn restore_delay_for(&self, app: Option<&str>) -> Duration {
        let slow = app.is_some_and(|name| {
            let name = name.to_lowercase();
            self.slow_apps
                .iter()
                .any(|s| !s.is_empty() && name.contains(&s.to_lowercase()))
        });
        if slow {
            self.slow_restore_delay.max(self.restore_delay)
        } else {
            self.restore_delay
        }
    }

    /// Blocking insertion. [`Inserter::insert`] runs this on a blocking thread.
    fn insert_blocking(&self, text: &str, control: &InsertionControl) -> InsertOutcome {
        if control.is_cancelled() {
            return InsertOutcome::Cancelled;
        }
        let (app_now, process_now) = SystemAppIdentityLookup.foreground_target();
        let (app, process) = control
            .target
            .as_ref()
            .cloned()
            .unwrap_or((app_now.clone(), process_now));
        if platform::session_kind() == platform::SessionKind::Wayland {
            return copy_only(text, CopyReason::Wayland, control);
        }
        if let Some(matcher) = app.as_ref().and_then(AppIdentity::matcher)
            && !app_now.as_ref().is_some_and(|now| matcher.matches(now))
        {
            return copy_only(text, CopyReason::FocusChanged, control);
        }
        let Some(process) = process else {
            return copy_only(text, CopyReason::UnverifiedField, control);
        };
        if process_now != Some(process) {
            return copy_only(text, CopyReason::FocusChanged, control);
        }
        let terminal = app.as_ref().and_then(suggested_preset) == Some(ShapePreset::Verbatim);
        if terminal && text.chars().any(char::is_control) {
            return copy_only(text, CopyReason::MultilineTerminal, control);
        }
        match platform::preflight(process) {
            Preflight::Paste => {}
            Preflight::CopyOnly(reason) => return copy_only(text, reason, control),
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => return InsertOutcome::Failed(format!("clipboard unavailable: {e}")),
        };
        let restore_after = self.restore_delay_for(platform::frontmost_app_name().as_deref());
        let outcome = paste_with_clipboard(
            &mut clipboard,
            text,
            control,
            || thread::sleep(self.settle),
            || {
                if let Preflight::CopyOnly(reason) = platform::preflight(process) {
                    return Err(reason);
                }
                let undo_target = (!terminal)
                    .then(|| platform::undo_target(process))
                    .flatten();
                platform::send_paste(process)?;
                if let Some(target) = undo_target {
                    control.record_undo_target(target);
                }
                Ok(())
            },
            || thread::sleep(restore_after),
        );
        platform::keep_clipboard_alive(clipboard);
        outcome
    }
}

trait PasteClipboard {
    fn snapshot(&mut self) -> Snapshot;
    fn read_text(&mut self) -> Option<String>;
    fn write_text(&mut self, text: &str) -> Result<(), arboard::Error>;
    fn restore(&mut self, saved: Snapshot) -> Result<(), arboard::Error>;
}

impl PasteClipboard for arboard::Clipboard {
    fn snapshot(&mut self) -> Snapshot {
        Snapshot::take(self)
    }

    fn read_text(&mut self) -> Option<String> {
        self.get_text().ok()
    }

    fn write_text(&mut self, text: &str) -> Result<(), arboard::Error> {
        self.set_text(text)
    }

    fn restore(&mut self, saved: Snapshot) -> Result<(), arboard::Error> {
        saved.restore(self)
    }
}

fn paste_with_clipboard(
    clipboard: &mut impl PasteClipboard,
    text: &str,
    control: &InsertionControl,
    settle: impl FnOnce(),
    paste: impl FnOnce() -> Result<(), CopyReason>,
    wait_for_paste: impl FnOnce(),
) -> InsertOutcome {
    let saved = clipboard.snapshot();
    if let Err(e) = clipboard.write_text(text) {
        return InsertOutcome::Failed(format!("couldn't write the clipboard: {e}"));
    }
    settle();

    let outcome = match control.commit(paste) {
        Some(Ok(())) => {
            wait_for_paste();
            InsertOutcome::Inserted
        }
        Some(Err(e)) => {
            return InsertOutcome::Copied(e);
        }
        None => InsertOutcome::Cancelled,
    };

    // Keep any clipboard change made by the user or another app while we were waiting.
    if clipboard.read_text().as_deref() == Some(text)
        && let Err(e) = clipboard.restore(saved)
    {
        tracing::warn!("couldn't restore the clipboard: {e}");
    }
    outcome
}

#[async_trait]
impl Inserter for SystemInserter {
    fn undo(&self, target: UndoTarget) -> bool {
        platform::undo(target)
    }

    fn target_app(&self) -> Option<String> {
        platform::frontmost_app_name()
    }

    async fn insert(&self, text: &str, control: Arc<InsertionControl>) -> InsertOutcome {
        let this = self.clone();
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || this.insert_blocking(&text, &control))
            .await
            .unwrap_or_else(|e| InsertOutcome::Failed(format!("insertion thread failed: {e}")))
    }

    async fn copy(
        &self,
        text: &str,
        reason: CopyReason,
        control: Arc<InsertionControl>,
    ) -> InsertOutcome {
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || copy_only(&text, reason, &control))
            .await
            .unwrap_or_else(|e| InsertOutcome::Failed(format!("clipboard thread failed: {e}")))
    }
}

fn copy_only(text: &str, reason: CopyReason, control: &InsertionControl) -> InsertOutcome {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(clipboard) => clipboard,
        Err(e) => return InsertOutcome::Failed(format!("clipboard unavailable: {e}")),
    };
    match control.commit(|| clipboard.set_text(text)) {
        Some(Ok(())) => {
            platform::keep_clipboard_alive(clipboard);
            InsertOutcome::Copied(reason)
        }
        Some(Err(e)) => InsertOutcome::Failed(format!("couldn't copy to the clipboard: {e}")),
        None => InsertOutcome::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inserter() -> SystemInserter {
        SystemInserter {
            settle: Duration::from_millis(1),
            restore_delay: Duration::from_millis(600),
            slow_restore_delay: Duration::from_millis(1500),
            slow_apps: DEFAULT_SLOW_APPS.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn slow_apps_get_a_longer_restore_delay() {
        let i = inserter();
        assert_eq!(i.restore_delay_for(None), Duration::from_millis(600));
        assert_eq!(
            i.restore_delay_for(Some("Notes")),
            Duration::from_millis(600)
        );
        assert_eq!(
            i.restore_delay_for(Some("Slack")),
            Duration::from_millis(1500)
        );
        assert_eq!(
            i.restore_delay_for(Some("Microsoft Remote Desktop")),
            Duration::from_millis(1500)
        );
        let custom = SystemInserter {
            slow_apps: vec!["textedit".into()],
            ..inserter()
        };
        assert_eq!(
            custom.restore_delay_for(Some("TextEdit")),
            Duration::from_millis(1500)
        );
        assert_eq!(
            custom.restore_delay_for(Some("Slack")),
            Duration::from_millis(600)
        );
    }

    struct FakeClipboard(Option<String>);

    impl PasteClipboard for FakeClipboard {
        fn snapshot(&mut self) -> Snapshot {
            self.0.clone().map_or(Snapshot::Unreadable, Snapshot::Text)
        }

        fn read_text(&mut self) -> Option<String> {
            self.0.clone()
        }

        fn write_text(&mut self, text: &str) -> Result<(), arboard::Error> {
            self.0 = Some(text.into());
            Ok(())
        }

        fn restore(&mut self, saved: Snapshot) -> Result<(), arboard::Error> {
            self.0 = match saved {
                Snapshot::Text(text) => Some(text),
                Snapshot::Unreadable => None,
                _ => unreachable!(),
            };
            Ok(())
        }
    }

    #[test]
    fn escape_after_staging_restores_the_previous_clipboard() {
        for previous in [Some("previous".into()), None] {
            let mut clipboard = FakeClipboard(previous.clone());
            let control = InsertionControl::default();
            let outcome = paste_with_clipboard(
                &mut clipboard,
                "dictation",
                &control,
                || assert!(control.cancel()),
                || panic!("cancelled dictation must not paste"),
                || panic!("cancelled dictation must not wait for paste"),
            );
            assert_eq!(outcome, InsertOutcome::Cancelled);
            assert_eq!(clipboard.0, previous);
        }
    }

    #[test]
    fn escape_after_paste_still_restores_the_clipboard() {
        let mut clipboard = FakeClipboard(Some("previous".into()));
        let control = InsertionControl::default();
        let outcome = paste_with_clipboard(
            &mut clipboard,
            "dictation",
            &control,
            || (),
            || Ok(()),
            || assert!(!control.cancel()),
        );
        assert_eq!(outcome, InsertOutcome::Inserted);
        assert_eq!(clipboard.0.as_deref(), Some("previous"));
    }
}
