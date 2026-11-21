//! Typing text into whatever app has focus.
//!
//! Strategy on every OS: save the clipboard, put the dictation on the
//! clipboard, simulate paste (⌘V / Ctrl+V), then restore the old contents after a
//! delay (longer for apps known to read the clipboard late) unless something else
//! changed the clipboard meanwhile. When paste cannot work (no Accessibility
//! permission, no focused text field, Wayland, secure input) the text is left on
//! the clipboard and the outcome is [`InsertOutcome::Copied`] so the HUD can say
//! "copied — press Ctrl+V".
//!
//! Preserved: plain text, HTML (with its text alternative), file lists and images.
//! Other formats (RTF-only, app-private types) cannot be read back; when nothing
//! readable was there, the dictation is simply left on the clipboard.

use std::{borrow::Cow, path::PathBuf, thread, time::Duration};

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
    /// Put `text` on the clipboard only, without pasting it anywhere.
    async fn copy(&self, text: &str, reason: CopyReason) -> InsertOutcome;
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
            Snapshot::Unreadable => Ok(()),
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
        let saved = Snapshot::take(&mut clipboard);
        let restore_after = self.restore_delay_for(platform::frontmost_app_name().as_deref());
        if let Err(e) = clipboard.set_text(text) {
            return InsertOutcome::Failed(format!("couldn't write the clipboard: {e}"));
        }
        thread::sleep(self.settle);

        if let Err(e) = send_paste() {
            tracing::warn!("paste keystroke failed: {e}");
            platform::keep_clipboard_alive(clipboard);
            return InsertOutcome::Copied(CopyReason::PasteFailed);
        }

        thread::sleep(restore_after);
        // Restore only if the clipboard still holds our text; if the user or
        // another app copied something in the meantime, leave theirs alone.
        if should_restore(&saved, clipboard.get_text().ok().as_deref(), text)
            && let Err(e) = saved.restore(&mut clipboard)
        {
            tracing::warn!("couldn't restore the clipboard: {e}");
        }
        platform::keep_clipboard_alive(clipboard);
        InsertOutcome::Inserted
    }
}

/// Restore when we have something to put back and the clipboard still holds our text.
fn should_restore(saved: &Snapshot, current: Option<&str>, ours: &str) -> bool {
    !matches!(saved, Snapshot::Unreadable) && current == Some(ours)
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

    async fn copy(&self, text: &str, reason: CopyReason) -> InsertOutcome {
        let text = text.to_owned();
        tokio::task::spawn_blocking(move || copy_only(&text, reason))
            .await
            .unwrap_or_else(|e| InsertOutcome::Failed(format!("clipboard thread failed: {e}")))
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
    // A raw keycode avoids enigo's keyboard-layout lookup, which must run on the
    // main thread on recent macOS. The keycode for "v" in the current layout
    // (47 on Dvorak) is cached by `platform::refresh_paste_keycode`; QWERTY's 9 otherwise.
    #[cfg(target_os = "macos")]
    return enigo.raw(platform::paste_keycode(), Direction::Click);
    #[cfg(target_os = "windows")]
    return enigo.key(enigo::Key::V, Direction::Click);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    enigo.key(enigo::Key::Unicode('v'), Direction::Click)
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

    #[test]
    fn restores_only_our_text_and_only_readable_snapshots() {
        let text = Snapshot::Text("old".into());
        assert!(should_restore(&text, Some("dictated"), "dictated"));
        // Someone copied something else meanwhile.
        assert!(!should_restore(&text, Some("theirs"), "dictated"));
        assert!(!should_restore(&text, None, "dictated"));
        // Nothing we could read back: leave the dictation rather than clear the clipboard.
        assert!(!should_restore(
            &Snapshot::Unreadable,
            Some("dictated"),
            "dictated"
        ));
        let files = Snapshot::Files(vec![PathBuf::from("/tmp/a")]);
        assert!(should_restore(&files, Some("dictated"), "dictated"));
    }
}
