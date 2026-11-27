//! minutae-dictation: hold a hotkey, speak, release, and the cleaned-up text
//! is typed wherever the cursor is.
//!
//! - [`DictationController`]: the hotkey state machine (hold / toggle /
//!   double-tap hands-free), driven by `key_down` / `key_up` from the app's
//!   global shortcut, emitting [`HudState`] and finished [`minutae_core::Dictation`]s.
//! - [`shortcuts`]: press/release global shortcuts from the XDG GlobalShortcuts
//!   portal (Wayland), with hold → toggle fallback when releases never arrive.
//! - [`postprocess`]: pure text cleanup (fillers, repeats, spoken punctuation,
//!   vocabulary, capitalisation).
//! - [`Inserter`] / [`SystemInserter`]: clipboard + simulated paste per OS,
//!   with copy-only fallbacks.
//! - [`AudioSource`] / [`SpeechToText`]: seams the app fills with the real
//!   capture and whisper crates.
//!
//! See docs/CROSS-PLATFORM.md §3 for the per-OS limits.

pub mod controller;
pub mod fakes;
pub mod insert;
pub mod platform;
pub mod postprocess;
pub mod shapes;
pub mod shortcuts;
pub mod source;

pub use controller::{
    DictationConfig, DictationController, DictationDeps, DictationEvent, DictationState,
    FinishedDictation, HudState,
};
pub use insert::{CopyReason, InsertOutcome, Inserter, InsertionControl, SystemInserter};
pub use platform::{
    InsertionSupport, SessionKind, accessibility_trusted, frontmost_app_name, insertion_support,
    prompt_accessibility, refresh_paste_keycode, session_kind,
};
pub use shortcuts::{
    BoundShortcut, KeyAction, KeySink, PortalBinding, PortalError, ReleaseSupport, ShortcutId,
    ShortcutPortal, ShortcutRequest, ShortcutSignal, SignalTranslator, run_router, xdg_trigger,
};
pub use source::{AudioSource, SAMPLE_RATE, SpeechToText};
