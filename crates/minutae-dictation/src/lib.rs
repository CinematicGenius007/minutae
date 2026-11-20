//! minutae-dictation: hold a hotkey, speak, release, and the cleaned-up text
//! is typed wherever the cursor is.
//!
//! - [`DictationController`]: the hotkey state machine (hold / toggle /
//!   double-tap hands-free), driven by `key_down` / `key_up` from the app's
//!   global shortcut, emitting [`HudState`] and finished [`minutae_core::Dictation`]s.
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
pub mod source;

pub use controller::{
    DictationConfig, DictationController, DictationDeps, DictationEvent, DictationState,
    FinishedDictation, HudState,
};
pub use insert::{CopyReason, InsertOutcome, Inserter, SystemInserter};
pub use platform::{
    InsertionSupport, SessionKind, accessibility_trusted, frontmost_app_name, insertion_support,
    prompt_accessibility, session_kind,
};
pub use source::{AudioSource, SAMPLE_RATE, SpeechToText};
