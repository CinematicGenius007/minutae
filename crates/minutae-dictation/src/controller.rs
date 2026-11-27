//! The dictation state machine.
//!
//! ```text
//! Idle/Error ──key_down──▶ Listening ──release / 2nd press / max length──▶ Processing
//!     ▲                       │ short tap, no 2nd tap; cancel()                 │ transcribe + clean
//!     └──────── HudState::Idle ◀┘                                               ▼
//!     ◀───────────── Inserted / Copied / Error ◀──────────────────────────── Inserting
//! ```
//!
//! Hold mode: press starts, release stops. A release within `min_press`
//! (default 250 ms) is a tap: if a second press follows within
//! `double_tap_window` the dictation becomes hands-free (the next press
//! stops it), otherwise it is cancelled with nothing inserted.
//! Toggle mode: each press toggles, releases are ignored; a session shorter
//! than `min_press` is cancelled.
//!
//! `key_down`/`key_up` are cheap and non-blocking: call them straight from
//! the global-shortcut callback. All async work is spawned on the runtime
//! handle given to [`DictationController::new`].

use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use chrono::{DateTime, Utc};
use minutae_core::{
    AppSettings, Dictation, HotkeyMode,
    dictation::{AppIdentity, AppRule, Shape, ShapePreset},
};
use serde::{Deserialize, Serialize};
use tokio::{
    runtime::Handle,
    sync::{mpsc, watch},
    time::Instant,
};

use crate::{
    insert::{CopyReason, InsertOutcome, Inserter, InsertionControl, UndoTarget},
    platform::identity::{AppIdentityLookup, app_changed},
    postprocess,
    shapes::resolve_shape,
    source::{AudioSource, SAMPLE_RATE, SpeechToText},
};

#[derive(Debug, Clone, PartialEq)]
pub struct DictationConfig {
    pub mode: HotkeyMode,
    /// Shorter presses (hold) or sessions (toggle) are taps, not dictations.
    pub min_press: Duration,
    /// After a tap in hold mode, a second press within this window goes hands-free.
    pub double_tap_window: Duration,
    /// Listening stops by itself (and the text is inserted) after this long.
    pub max_duration: Duration,
    pub default_shape: Shape,
    pub app_rules: Vec<AppRule>,
    pub start_sound: bool,
    pub quick_shapes: [Shape; 5],
    pub vocabulary: Vec<String>,
    /// Attach the captured samples to [`DictationEvent::Finished`].
    pub keep_audio: bool,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            mode: HotkeyMode::Hold,
            min_press: Duration::from_millis(250),
            double_tap_window: Duration::from_millis(350),
            max_duration: Duration::from_secs(120),
            default_shape: Shape::default(),
            app_rules: vec![],
            start_sound: false,
            quick_shapes: QUICK_PRESETS.map(ShapePreset::shape),
            vocabulary: Vec::new(),
            keep_audio: false,
        }
    }
}

impl DictationConfig {
    pub fn from_settings(settings: &AppSettings) -> Self {
        Self {
            mode: settings.dictation_mode,
            default_shape: settings.default_dictation_shape(),
            app_rules: settings.dictation_rules(),
            start_sound: settings.dictation_start_sound,
            quick_shapes: QUICK_PRESETS.map(|preset| settings.dictation_shape(preset)),
            vocabulary: settings.vocabulary.clone(),
            keep_audio: settings.keep_dictation_audio,
            ..Self::default()
        }
    }
}

pub const QUICK_PRESETS: [ShapePreset; 5] = [
    ShapePreset::Verbatim,
    ShapePreset::ChatMessage,
    ShapePreset::Email,
    ShapePreset::NotesAndLists,
    ShapePreset::CodeAndCommit,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HudFixAction {
    MicrophoneSettings,
    TranscriptionSettings,
    Retry,
    DictationSettings,
}

/// Coarse state, for the UI and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DictationState {
    Idle,
    Listening,
    Processing,
    Inserting,
    Error,
}

/// What the HUD window shows. Serialized as `{"state": "listening", "level": 0.4, "handsFree": false}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HudState {
    /// Hide the HUD (cancelled).
    Idle,
    Listening {
        level: f32,
        hands_free: bool,
        /// Internal listening posture used to report the currently usable shortcuts.
        #[serde(skip)]
        hotkey_held: bool,
        app: Option<String>,
        shape: ShapePreset,
    },
    Processing,
    /// "Inserted · 23 words"
    Inserted {
        words: u32,
        undo_available: bool,
        #[serde(skip)]
        result_id: Option<uuid::Uuid>,
    },
    /// "No text field · copied instead" / "Copied — press Ctrl+V"
    Copied {
        words: u32,
        reason: CopyReason,
    },
    Error {
        message: String,
        fix_action: HudFixAction,
    },
}

/// A finished dictation, for persistence (done by the app).
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedDictation {
    pub dictation: Dictation,
    pub outcome: InsertOutcome,
    /// 16 kHz mono samples when `keep_audio` is on.
    pub audio: Option<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DictationEvent {
    Hud(HudState),
    Finished(Box<FinishedDictation>),
    StartSound,
}

/// The pluggable parts.
#[derive(Clone)]
pub struct DictationDeps {
    pub audio: Arc<dyn AudioSource>,
    pub stt: Arc<dyn SpeechToText>,
    pub inserter: Arc<dyn Inserter>,
    pub apps: Arc<dyn AppIdentityLookup>,
}

#[derive(Clone)]
pub struct DictationController {
    shared: Arc<Shared>,
}

struct Shared {
    inner: Mutex<Inner>,
    // Escape can signal this even while the key worker is stopping the microphone.
    active_insert: Mutex<Option<Arc<InsertionControl>>>,
    deps: DictationDeps,
    events: mpsc::UnboundedSender<DictationEvent>,
    handle: Handle,
}

struct Inner {
    phase: Phase,
    config: DictationConfig,
    session: u64,
    level: f32,
    undo_target: Option<(uuid::Uuid, UndoTarget)>,
}

enum Phase {
    Idle,
    Error,
    Listening(Listening),
    Processing,
    Inserting,
}

struct Listening {
    started: Instant,
    pressed_at: Instant,
    created_at: DateTime<Utc>,
    app: Option<AppIdentity>,
    shape: Shape,
    quick_shapes: [Shape; 5],
    hands_free: bool,
    hotkey_held: bool,
    awaiting_second_tap: bool,
    /// The release that belongs to the press that switched to hands-free.
    swallow_release: bool,
    /// Dropped when listening ends; background tasks watch it to exit.
    _alive: watch::Sender<()>,
}

struct Job {
    session: u64,
    control: Arc<InsertionControl>,
    samples: Vec<f32>,
    created_at: DateTime<Utc>,
    app: Option<AppIdentity>,
    shape: Shape,
    duration_ms: u64,
    vocabulary: Vec<String>,
    keep_audio: bool,
}

impl DictationController {
    /// Returns the controller and the event stream for the HUD / persistence.
    pub fn new(
        deps: DictationDeps,
        config: DictationConfig,
        handle: Handle,
    ) -> (Self, mpsc::UnboundedReceiver<DictationEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        let inner = Inner {
            phase: Phase::Idle,
            config,
            session: 0,
            level: 0.0,
            undo_target: None,
        };
        let shared = Arc::new(Shared {
            inner: Mutex::new(inner),
            active_insert: Mutex::new(None),
            deps,
            events,
            handle,
        });
        (Self { shared }, rx)
    }

    pub fn state(&self) -> DictationState {
        match self.shared.lock().phase {
            Phase::Idle => DictationState::Idle,
            Phase::Error => DictationState::Error,
            Phase::Listening(_) => DictationState::Listening,
            Phase::Processing => DictationState::Processing,
            Phase::Inserting => DictationState::Inserting,
        }
    }

    /// Applies new settings. A change takes effect at the next key event;
    /// a dictation in progress is not interrupted.
    pub fn set_config(&self, config: DictationConfig) {
        self.shared.lock().config = config;
    }

    pub fn config(&self) -> DictationConfig {
        self.shared.lock().config.clone()
    }

    /// The dictation hotkey went down (repeats while held are ignored).
    pub fn key_down(&self) {
        self.press(None);
    }

    /// Starts or stops a dictation whatever the hotkey mode, like a press in toggle
    /// mode (`--toggle-dictation` from a desktop shortcut).
    pub fn toggle(&self) {
        self.press(Some(HotkeyMode::Toggle));
    }

    pub fn retry(&self) {
        let mut inner = self.shared.lock();
        if matches!(inner.phase, Phase::Error) {
            self.shared.begin(&mut inner, Instant::now(), false);
            // Retry is issued by a click, so there will be no matching key release.
            let level = inner.level;
            if let Phase::Listening(listening) = &mut inner.phase {
                listening.hands_free = true;
                self.shared.emit(listening.hud(level));
            }
        }
    }

    fn press(&self, mode: Option<HotkeyMode>) {
        let shared = &self.shared;
        let now = Instant::now();
        let mut inner = shared.lock();
        let hotkey_held = mode.is_none();
        let mode = mode.unwrap_or(inner.config.mode);
        let min_press = inner.config.min_press;
        let level = inner.level;
        match &mut inner.phase {
            Phase::Idle | Phase::Error => shared.begin(&mut inner, now, hotkey_held),
            Phase::Listening(l) if l.awaiting_second_tap => {
                l.awaiting_second_tap = false;
                l.hands_free = true;
                l.hotkey_held = hotkey_held;
                l.swallow_release = true;
                shared.emit(l.hud(level));
            }
            Phase::Listening(l) if l.hands_free || mode == HotkeyMode::Toggle => {
                if now.duration_since(l.started) < min_press {
                    shared.cancel_listening(&mut inner);
                } else {
                    shared.stop_and_process(&mut inner, now);
                }
            }
            Phase::Listening(_) => {} // auto-repeat while held
            Phase::Processing | Phase::Inserting => {
                tracing::debug!("dictation hotkey ignored while busy");
            }
        }
    }

    /// Applies only to the current listening session.
    pub fn switch_shape(&self, preset: ShapePreset) -> bool {
        let mut inner = self.shared.lock();
        let level = inner.level;
        let Phase::Listening(listening) = &mut inner.phase else {
            return false;
        };
        let Some(shape) = listening.quick_shapes.iter().find(|s| s.preset == preset) else {
            return false;
        };
        listening.shape = *shape;
        self.shared.emit(listening.hud(level));
        true
    }

    /// The HUD calls this only while showing the insertion result. The platform
    /// verifies the foreground process again before dispatching to that process.
    pub fn undo_visible_insertion(&self, result_id: uuid::Uuid) -> bool {
        let mut inner = self.shared.lock();
        let Some((id, process)) = inner.undo_target else {
            return false;
        };
        if id != result_id {
            return false;
        }
        inner.undo_target = None;
        self.shared.deps.inserter.undo(process)
    }

    /// The dictation hotkey was released.
    pub fn key_up(&self) {
        let shared = &self.shared;
        let now = Instant::now();
        let mut inner = shared.lock();
        let mode = inner.config.mode;
        let min_press = inner.config.min_press;
        let window = inner.config.double_tap_window;
        let session = inner.session;
        let level = inner.level;
        let Phase::Listening(l) = &mut inner.phase else {
            return;
        };
        let was_held = std::mem::take(&mut l.hotkey_held);
        if l.swallow_release || mode == HotkeyMode::Toggle || l.hands_free {
            l.swallow_release = false;
            if was_held {
                shared.emit(l.hud(level));
            }
            return;
        }
        if l.awaiting_second_tap {
            return;
        }
        if now.duration_since(l.pressed_at) >= min_press {
            shared.stop_and_process(&mut inner, now);
            return;
        }
        // A tap: wait briefly for a second press before cancelling.
        l.awaiting_second_tap = true;
        if was_held {
            shared.emit(l.hud(level));
        }
        let mut alive = l._alive.subscribe();
        let this = Arc::clone(shared);
        shared.handle.spawn(async move {
            tokio::select! {
                () = tokio::time::sleep(window) => this.on_double_tap_timeout(session),
                _ = alive.changed() => {}
            }
        });
    }

    /// Signal Escape immediately. Follow with `finish_cancel` on the key worker.
    pub fn request_cancel(&self) -> bool {
        self.shared
            .active_insert()
            .as_ref()
            .is_some_and(|control| control.cancel())
    }

    /// Stop listening or abandon transcription off the main thread. Insertion, if
    /// already staging the clipboard, rolls it back before emitting Idle itself.
    pub fn finish_cancel(&self) {
        let shared = &self.shared;
        let mut inner = shared.lock();
        if !shared.is_cancelled() {
            return;
        }
        match inner.phase {
            Phase::Listening(_) => shared.cancel_listening(&mut inner),
            Phase::Processing => shared.cancelled(&mut inner),
            _ => {}
        }
    }

    /// Cancel from a thread where stopping the microphone is safe.
    pub fn cancel(&self) {
        if self.request_cancel() {
            self.finish_cancel();
        }
    }
}

impl Listening {
    fn hud(&self, level: f32) -> HudState {
        HudState::Listening {
            level,
            hands_free: self.hands_free,
            hotkey_held: self.hotkey_held,
            app: self.app.as_ref().and_then(|a| a.display_name.clone()),
            shape: self.shape.preset,
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn active_insert(&self) -> MutexGuard<'_, Option<Arc<InsertionControl>>> {
        self.active_insert.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn is_cancelled(&self) -> bool {
        self.active_insert()
            .as_ref()
            .is_some_and(|control| control.is_cancelled())
    }

    fn emit(&self, state: HudState) {
        let _ = self.events.send(DictationEvent::Hud(state));
    }

    fn begin(self: &Arc<Self>, inner: &mut Inner, now: Instant, hotkey_held: bool) {
        inner.undo_target = None;
        let (app, target_id) = self.deps.apps.foreground_target();
        let shape = resolve_shape(
            app.as_ref(),
            &inner.config.app_rules,
            &inner.config.default_shape,
        );
        if let Err(e) = self.deps.audio.start() {
            tracing::warn!("microphone failed to start: {e:#}");
            inner.phase = Phase::Error;
            self.emit(HudState::Error {
                message: format!("Microphone unavailable: {e}"),
                fix_action: HudFixAction::MicrophoneSettings,
            });
            return;
        }
        *self.active_insert() = Some(Arc::new(InsertionControl::for_target(
            app.clone(),
            target_id,
        )));
        inner.session += 1;
        inner.level = 0.0;
        let session = inner.session;
        let (alive, _) = watch::channel(());
        let levels_alive = alive.subscribe();
        let mut max_alive = alive.subscribe();
        let listening = Listening {
            started: now,
            pressed_at: now,
            created_at: Utc::now(),
            app,
            shape,
            quick_shapes: inner.config.quick_shapes,
            hands_free: false,
            hotkey_held,
            awaiting_second_tap: false,
            swallow_release: false,
            _alive: alive,
        };
        self.emit(listening.hud(0.0));
        inner.phase = Phase::Listening(listening);
        if inner.config.start_sound {
            let _ = self.events.send(DictationEvent::StartSound);
        }

        let this = Arc::clone(self);
        let levels = self.deps.audio.levels();
        self.handle
            .spawn(async move { this.forward_levels(session, levels, levels_alive).await });

        let this = Arc::clone(self);
        let max = inner.config.max_duration;
        self.handle.spawn(async move {
            tokio::select! {
                () = tokio::time::sleep(max) => this.on_max_duration(session),
                _ = max_alive.changed() => {}
            }
        });
    }

    async fn forward_levels(
        &self,
        session: u64,
        mut levels: watch::Receiver<f32>,
        mut alive: watch::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                changed = levels.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    let level = levels.borrow_and_update().clamp(0.0, 1.0);
                    let mut inner = self.lock();
                    if inner.session != session {
                        return;
                    }
                    if let Phase::Listening(l) = &inner.phase {
                        self.emit(l.hud(level));
                        inner.level = level;
                    }
                }
                _ = alive.changed() => return,
            }
        }
    }

    fn on_double_tap_timeout(&self, session: u64) {
        let mut inner = self.lock();
        if inner.session == session
            && matches!(&inner.phase, Phase::Listening(l) if l.awaiting_second_tap)
        {
            self.cancel_listening(&mut inner);
        }
    }

    fn on_max_duration(self: &Arc<Self>, session: u64) {
        let mut inner = self.lock();
        if inner.session == session && matches!(inner.phase, Phase::Listening(_)) {
            tracing::info!("dictation reached the maximum length");
            self.stop_and_process(&mut inner, Instant::now());
        }
    }

    fn cancel_listening(&self, inner: &mut Inner) {
        if let Err(e) = self.deps.audio.stop() {
            tracing::debug!("microphone stop after cancel: {e:#}");
        }
        self.cancelled(inner);
    }

    fn cancelled(&self, inner: &mut Inner) {
        self.active_insert().take();
        inner.phase = Phase::Idle;
        inner.session += 1;
        self.emit(HudState::Idle);
    }

    fn stop_and_process(self: &Arc<Self>, inner: &mut Inner, now: Instant) {
        let Phase::Listening(listening) = std::mem::replace(&mut inner.phase, Phase::Processing)
        else {
            return;
        };
        self.emit(HudState::Processing);
        let samples = match self.deps.audio.stop() {
            Ok(samples) => samples,
            Err(e) => {
                self.fail(
                    inner,
                    format!("Recording failed: {e}"),
                    HudFixAction::MicrophoneSettings,
                );
                return;
            }
        };
        let control = self
            .active_insert()
            .as_ref()
            .expect("active dictation")
            .clone();
        if control.is_cancelled() {
            self.cancelled(inner);
            return;
        }
        let elapsed = now.duration_since(listening.started);
        let audio_ms = samples.len() as u64 * 1000 / u64::from(SAMPLE_RATE);
        let job = Job {
            session: inner.session,
            control,
            samples,
            created_at: listening.created_at,
            app: listening.app,
            shape: listening.shape,
            duration_ms: if audio_ms > 0 {
                audio_ms
            } else {
                elapsed.as_millis() as u64
            },
            vocabulary: inner.config.vocabulary.clone(),
            keep_audio: inner.config.keep_audio,
        };
        let this = Arc::clone(self);
        self.handle
            .spawn(async move { this.run_pipeline(job).await });
    }

    fn fail(&self, inner: &mut Inner, message: String, fix_action: HudFixAction) {
        // Take the slot before deciding the error so an Escape that won is honored.
        if self
            .active_insert()
            .take()
            .is_some_and(|control| control.is_cancelled())
        {
            self.cancelled(inner);
            return;
        }
        tracing::warn!("dictation failed: {message}");
        inner.phase = Phase::Error;
        self.emit(HudState::Error {
            message,
            fix_action,
        });
    }

    /// Locks and checks the pipeline for `session` is still the live one.
    fn current(&self, session: u64) -> Option<MutexGuard<'_, Inner>> {
        let inner = self.lock();
        let live =
            inner.session == session && matches!(inner.phase, Phase::Processing | Phase::Inserting);
        live.then_some(inner)
    }

    async fn run_pipeline(&self, job: Job) {
        if job.control.is_cancelled() {
            return;
        }
        if job.samples.is_empty() {
            if let Some(mut inner) = self.current(job.session) {
                self.fail(
                    &mut inner,
                    "No audio was captured".into(),
                    HudFixAction::MicrophoneSettings,
                );
            }
            return;
        }
        let transcript = self
            .deps
            .stt
            .transcribe(&job.samples, &job.vocabulary, job.control.cancel_flag())
            .await;
        let raw = match transcript {
            Ok(raw) => raw,
            Err(e) => {
                if let Some(mut inner) = self.current(job.session) {
                    self.fail(
                        &mut inner,
                        format!("Couldn't transcribe: {e}"),
                        HudFixAction::TranscriptionSettings,
                    );
                }
                return;
            }
        };
        let text = postprocess::process_with_shape(&raw, &job.shape, &job.vocabulary);
        {
            let Some(mut inner) = self.current(job.session) else {
                return;
            };
            if job.control.is_cancelled() {
                return;
            }
            if text.is_empty() {
                self.fail(&mut inner, "No speech detected".into(), HudFixAction::Retry);
                return;
            }
            inner.phase = Phase::Inserting;
        }

        // Paste only into the app the user started dictating in. If they switched apps while
        // it was transcribing (to a terminal, a chat…), the text goes to the clipboard.
        let now_app = self.deps.apps.frontmost_app();
        let outcome = if app_changed(job.app.as_ref(), now_app.as_ref()) {
            self.deps
                .inserter
                .copy(&text, CopyReason::FocusChanged, job.control.clone())
                .await
        } else {
            self.deps.inserter.insert(&text, job.control.clone()).await
        };
        let words = postprocess::word_count(&text);
        let mut inner = self.lock();
        if inner.session != job.session {
            return;
        }
        let control = self.active_insert().take();
        if outcome == InsertOutcome::Cancelled || control.is_some_and(|c| c.is_cancelled()) {
            self.cancelled(&mut inner);
            return;
        }
        let id = uuid::Uuid::new_v4();
        match &outcome {
            InsertOutcome::Inserted => {
                inner.phase = Phase::Idle;
                inner.undo_target = job.control.undo_target().map(|process| (id, process));
                self.emit(HudState::Inserted {
                    words,
                    undo_available: inner.undo_target.is_some(),
                    result_id: inner.undo_target.map(|(id, _)| id),
                });
            }
            InsertOutcome::Copied(reason) => {
                inner.phase = Phase::Idle;
                self.emit(HudState::Copied {
                    words,
                    reason: *reason,
                });
            }
            InsertOutcome::Failed(reason) => {
                self.fail(&mut inner, reason.clone(), HudFixAction::DictationSettings)
            }
            InsertOutcome::Cancelled => unreachable!(),
        }

        let dictation = Dictation {
            id,
            created_at: job.created_at,
            text,
            raw_text: raw,
            target_app: job.app.as_ref().and_then(|app| app.display_name.clone()),
            app_identity: job.app,
            shape: Some(job.shape),
            word_count: words,
            duration_ms: job.duration_ms,
            inserted: outcome == InsertOutcome::Inserted,
        };
        let audio = job.keep_audio.then_some(job.samples);
        let _ = self
            .events
            .send(DictationEvent::Finished(Box::new(FinishedDictation {
                dictation,
                outcome,
                audio,
            })));
    }
}
