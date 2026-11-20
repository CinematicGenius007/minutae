//! Typing text into whatever app has focus.
//!
//! Strategy on every OS: save the clipboard text, put the dictation on the
//! clipboard, simulate paste (⌘V / Ctrl+V), then restore the old text after a
//! short delay unless something else changed the clipboard meanwhile. When
//! paste cannot work (no Accessibility permission, no focused text field,
//! Wayland, secure input) the text is left on the clipboard and the outcome is
//! [`InsertOutcome::Copied`] so the HUD can say "copied — press Ctrl+V".
//!
//! Only text is preserved: images or files on the clipboard are replaced.

use std::{thread, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::platform;

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
    /// Simulating the paste keystroke failed.
    PasteFailed,
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
}

/// Puts dictated text into the frontmost app.
#[async_trait]
pub trait Inserter: Send + Sync {
    /// Name of the app that has focus now, e.g. "Notes". Called on key-down,
    /// so it names the app the user was in when they started talking.
    fn target_app(&self) -> Option<String> {
        None
    }
    async fn insert(&self, text: &str) -> InsertOutcome;
}

/// What the platform layer decided before touching the clipboard.
// Not every OS produces every variant.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Preflight {
    Paste,
    CopyOnly(CopyReason),
    Refuse(String),
}

/// The real inserter: clipboard + simulated paste.
#[derive(Debug, Clone)]
pub struct SystemInserter {
    /// Wait after writing the clipboard before sending the keystroke.
    pub settle: Duration,
    /// Wait after the keystroke before restoring the previous clipboard text.
    /// The target app reads the clipboard asynchronously, so this cannot be 0.
    pub restore_delay: Duration,
}

impl Default for SystemInserter {
    fn default() -> Self {
        Self {
            settle: Duration::from_millis(40),
            restore_delay: Duration::from_millis(300),
        }
    }
}

impl SystemInserter {
    /// Blocking insertion. [`Inserter::insert`] runs this on a blocking thread.
    pub fn insert_blocking(&self, text: &str) -> InsertOutcome {
        match platform::preflight() {
            Preflight::Paste => {}
            Preflight::CopyOnly(reason) => return copy_only(text, reason),
            Preflight::Refuse(reason) => return InsertOutcome::Failed(reason),
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => return InsertOutcome::Failed(format!("clipboard unavailable: {e}")),
        };
        let saved = clipboard.get_text().ok();
        if let Err(e) = clipboard.set_text(text) {
            return InsertOutcome::Failed(format!("couldn't write the clipboard: {e}"));
        }
        thread::sleep(self.settle);

        if let Err(e) = send_paste() {
            tracing::warn!("paste keystroke failed: {e}");
            platform::keep_clipboard_alive(clipboard);
            return InsertOutcome::Copied(CopyReason::PasteFailed);
        }

        thread::sleep(self.restore_delay);
        // Restore only if the clipboard still holds our text; if the user or
        // another app copied something in the meantime, leave theirs alone.
        if let Some(previous) = saved
            && clipboard.get_text().ok().as_deref() == Some(text)
            && let Err(e) = clipboard.set_text(previous)
        {
            tracing::warn!("couldn't restore the clipboard: {e}");
        }
        platform::keep_clipboard_alive(clipboard);
        InsertOutcome::Inserted
    }
}

#[async_trait]
impl Inserter for SystemInserter {
    fn target_app(&self) -> Option<String> {
        platform::frontmost_app_name()
    }

    async fn insert(&self, text: &str) -> InsertOutcome {
        let this = self.clone();
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || this.insert_blocking(&text))
            .await
            .unwrap_or_else(|e| InsertOutcome::Failed(format!("insertion thread failed: {e}")))
    }
}

fn copy_only(text: &str, reason: CopyReason) -> InsertOutcome {
    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text).map(|()| c)) {
        Ok(clipboard) => {
            platform::keep_clipboard_alive(clipboard);
            InsertOutcome::Copied(reason)
        }
        Err(e) => InsertOutcome::Failed(format!("couldn't copy to the clipboard: {e}")),
    }
}

/// ⌘V on macOS, Ctrl+V elsewhere.
fn send_paste() -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| e.to_string())?;
    let pressed = press_v(&mut enigo);
    let released = enigo.key(modifier, Direction::Release);
    pressed.and(released).map_err(|e| e.to_string())
}

fn press_v(enigo: &mut enigo::Enigo) -> enigo::InputResult<()> {
    use enigo::{Direction, Keyboard};
    // kVK_ANSI_V. A raw keycode avoids enigo's keyboard-layout lookup,
    // which must run on the main thread on recent macOS.
    #[cfg(target_os = "macos")]
    return enigo.raw(9, Direction::Click);
    #[cfg(target_os = "windows")]
    return enigo.key(enigo::Key::V, Direction::Click);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    enigo.key(enigo::Key::Unicode('v'), Direction::Click)
}
