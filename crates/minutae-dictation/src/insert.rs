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
        identity::{AppIdentity, AppIdentityLookup, SystemAppIdentityLookup, app_changed},
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
    /// Password field detected, or macOS secure keyboard entry is on.
    SecureInput,
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
    pub(crate) fn for_target(app: Option<AppIdentity>, target_id: Option<u32>) -> Self {
        Self {
            target: Some((app, target_id)),
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

    // Keystroke delivery alone does not confirm that the target inserted the text.
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
        let session = platform::session_kind();
        if let Preflight::CopyOnly(reason) = platform::preflight(session) {
            return copy_only(text, reason, control);
        }
        let (app_now, id_now) = SystemAppIdentityLookup.foreground_target();
        let (app, target_id) = control.target.as_ref().cloned().unwrap_or_default();
        if let Some(reason) =
            target_copy_reason(text, app.as_ref(), target_id, app_now.as_ref(), id_now)
        {
            return copy_only(text, reason, control);
        }
        // If the start observation was incomplete, use what we can observe
        // before staging the clipboard for the final focus check.
        let app = app.or(app_now);
        let target_id = target_id.or(id_now);

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
                send_paste(|| {
                    if let Preflight::CopyOnly(reason) = platform::preflight(session) {
                        return Err(reason);
                    }
                    let (current_app, current_id) = SystemAppIdentityLookup.foreground_target();
                    target_copy_reason(
                        text,
                        app.as_ref(),
                        target_id,
                        current_app.as_ref(),
                        current_id,
                    )
                    .map_or(Ok(()), Err)
                })
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

fn target_copy_reason(
    text: &str,
    original_app: Option<&AppIdentity>,
    original_id: Option<u32>,
    current_app: Option<&AppIdentity>,
    current_id: Option<u32>,
) -> Option<CopyReason> {
    let changed = match (original_id, current_id) {
        (Some(then), Some(now)) => then != now,
        _ => app_changed(original_app, current_app),
    };
    if changed {
        return Some(CopyReason::FocusChanged);
    }
    let terminal = [current_app, original_app]
        .into_iter()
        .flatten()
        .any(|app| suggested_preset(app) == Some(ShapePreset::Verbatim));
    (terminal
        && text
            .chars()
            .any(|c| c.is_control() || crate::postprocess::is_line_ending(c)))
    .then_some(CopyReason::MultilineTerminal)
}

/// ⌘V on macOS, Ctrl+V elsewhere.
fn send_paste(check_target: impl FnOnce() -> Result<(), CopyReason>) -> Result<(), CopyReason> {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).map_err(paste_error)?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    check_target()?;
    enigo.key(modifier, Direction::Press).map_err(paste_error)?;
    let pressed = press_v(&mut enigo);
    let released = enigo.key(modifier, Direction::Release);
    pressed.and(released).map_err(paste_error)
}

fn paste_error(error: impl std::fmt::Display) -> CopyReason {
    tracing::warn!(%error, "paste keystroke failed");
    CopyReason::PasteFailed
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

    fn assert_dispatch(gate: Preflight, expected: InsertOutcome) {
        let mut clipboard = FakeClipboard(Some("previous".into()));
        let control = InsertionControl::default();
        let outcome = paste_with_clipboard(
            &mut clipboard,
            "dictation",
            &control,
            || (),
            || match gate {
                Preflight::Paste => Ok(()),
                Preflight::CopyOnly(reason) => Err(reason),
            },
            || (),
        );
        assert_eq!(outcome, expected);
        assert_eq!(
            clipboard.0.as_deref(),
            Some(if expected == InsertOutcome::Inserted {
                "previous"
            } else {
                "dictation"
            })
        );
        assert!(control.undo_target().is_none());
    }

    #[test]
    fn macos_unknown_and_custom_fields_insert_and_positive_field_signals_copy() {
        for (role, subrole, expected) in [
            (None, None, InsertOutcome::Inserted),
            (Some("AXWebArea"), None, InsertOutcome::Inserted),
            (Some("AXGroup"), Some("Custom"), InsertOutcome::Inserted),
            (Some("AXTextArea"), None, InsertOutcome::Inserted),
            (
                Some("AXButton"),
                None,
                InsertOutcome::Copied(CopyReason::NoTextField),
            ),
            (
                Some("AXSecureTextField"),
                None,
                InsertOutcome::Copied(CopyReason::SecureInput),
            ),
            (
                None,
                Some("AXSecureTextField"),
                InsertOutcome::Copied(CopyReason::SecureInput),
            ),
        ] {
            let gate = platform::preflight_for(platform::SessionKind::MacOs, || {
                platform::macos_field_copy_reason(role, subrole)
                    .map_or(Preflight::Paste, Preflight::CopyOnly)
            });
            assert_dispatch(gate, expected);
        }
    }

    #[test]
    fn windows_non_edit_and_unknown_controls_insert_and_password_edits_copy() {
        for (class, masked, expected) in [
            (
                "Chrome_RenderWidgetHostHWND",
                false,
                InsertOutcome::Inserted,
            ),
            ("RichEdit50W", false, InsertOutcome::Inserted),
            ("", false, InsertOutcome::Inserted),
            ("Edit", false, InsertOutcome::Inserted),
            ("EDIT", true, InsertOutcome::Copied(CopyReason::SecureInput)),
        ] {
            let gate = platform::preflight_for(platform::SessionKind::Windows, || {
                platform::windows_field_copy_reason(class, || {
                    assert!(class.eq_ignore_ascii_case("Edit"));
                    masked
                })
                .map_or(Preflight::Paste, Preflight::CopyOnly)
            });
            assert_dispatch(gate, expected);
        }
    }

    #[test]
    fn x11_and_unknown_sessions_insert_and_wayland_copies() {
        use platform::SessionKind;
        for (session, expected) in [
            (SessionKind::X11, InsertOutcome::Inserted),
            (SessionKind::Unknown, InsertOutcome::Inserted),
            (
                SessionKind::Wayland,
                InsertOutcome::Copied(CopyReason::Wayland),
            ),
        ] {
            assert_dispatch(
                platform::preflight_for(session, || panic!("no focused-field query")),
                expected,
            );
        }
    }

    #[test]
    fn x11_same_class_different_window_copies() {
        use crate::platform::identity::x11_target;
        let editor = || AppIdentity {
            x11_window_class: Some("Code".into()),
            ..Default::default()
        };
        let (original, original_id) =
            x11_target(b"_NET_ACTIVE_WINDOW: window id # 0x12", |_| Some(editor())).unwrap();
        for (current, current_id, expected) in [
            (Some(editor()), Some(0x12), InsertOutcome::Inserted),
            (
                Some(editor()),
                Some(0x34),
                InsertOutcome::Copied(CopyReason::FocusChanged),
            ),
            (
                None,
                Some(0x34),
                InsertOutcome::Copied(CopyReason::FocusChanged),
            ),
            (None, None, InsertOutcome::Inserted),
        ] {
            let active = format!(
                "_NET_ACTIVE_WINDOW: window id # 0x{:x}",
                current_id.unwrap_or(0)
            );
            let (app, id) = x11_target(active.as_bytes(), |_| current).unwrap_or_default();
            assert_dispatch(
                target_copy_reason(
                    "dictation",
                    original.as_ref(),
                    original_id,
                    app.as_ref(),
                    id,
                )
                .map_or(Preflight::Paste, Preflight::CopyOnly),
                expected,
            );
        }
    }

    #[test]
    fn terminal_guard_copies_code_shaped_with_unicode_line_separators() {
        let terminal = AppIdentity {
            x11_window_class: Some("kitty".into()),
            ..Default::default()
        };
        for raw in ["one\u{2028}two", "one\u{2029}two"] {
            let shape = ShapePreset::CodeAndCommit.shape();
            let text = crate::postprocess::process_with_shape(raw, &shape, &[]);
            assert_eq!(text, raw);
            assert_dispatch(
                target_copy_reason(&text, None, None, Some(&terminal), None)
                    .map_or(Preflight::Paste, Preflight::CopyOnly),
                InsertOutcome::Copied(CopyReason::MultilineTerminal),
            );
        }
    }

    #[test]
    fn target_guard_copies_for_focus_change_or_terminal_control_characters() {
        let terminal = AppIdentity {
            x11_window_class: Some("kitty".into()),
            ..Default::default()
        };
        let editor = AppIdentity {
            x11_window_class: Some("Code".into()),
            ..Default::default()
        };
        for (text, original, pid, current, current_pid, expected) in [
            ("text", None, None, None, None, InsertOutcome::Inserted),
            (
                "text",
                Some(&editor),
                Some(1),
                None,
                None,
                InsertOutcome::Inserted,
            ),
            (
                "text",
                Some(&editor),
                None,
                Some(&terminal),
                None,
                InsertOutcome::Copied(CopyReason::FocusChanged),
            ),
            (
                "text",
                Some(&editor),
                Some(1),
                Some(&editor),
                Some(2),
                InsertOutcome::Copied(CopyReason::FocusChanged),
            ),
            (
                "text",
                Some(&terminal),
                None,
                Some(&terminal),
                None,
                InsertOutcome::Inserted,
            ),
            (
                "text\nmore",
                Some(&terminal),
                None,
                Some(&terminal),
                None,
                InsertOutcome::Copied(CopyReason::MultilineTerminal),
            ),
            (
                "text\tmore",
                None,
                None,
                Some(&terminal),
                None,
                InsertOutcome::Copied(CopyReason::MultilineTerminal),
            ),
        ] {
            assert_dispatch(
                target_copy_reason(text, original, pid, current, current_pid)
                    .map_or(Preflight::Paste, Preflight::CopyOnly),
                expected,
            );
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
