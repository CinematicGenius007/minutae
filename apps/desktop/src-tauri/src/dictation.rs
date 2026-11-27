//! Dictation wiring: mic capture (minutae-audio) + whisper (minutae-asr) + system paste,
//! driven by `DictationController`; HUD events go to the `hud` window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use minutae_asr::{ModelManager, TranscribeOptions, Transcriber};
use minutae_audio::{CaptureConfig, Recorder, RecordingHandle};
use minutae_core::{AppSettings, WhisperModel, dictation::ShapePreset};
use minutae_dictation::{
    AudioSource, DictationConfig, DictationController, DictationDeps, DictationEvent,
    FinishedDictation, HudFixAction, HudState, QUICK_PRESETS, SpeechToText, SystemInserter,
    platform::identity::SystemAppIdentityLookup,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

use crate::dto::{self, ev};
use crate::state::{AppState, TranscriberCache};
use crate::util;

/// Mic-only capture into a temp folder; returns 16 kHz mono f32 on stop.
pub struct MicSource {
    settings: Arc<RwLock<AppSettings>>,
    active: Mutex<Option<(RecordingHandle, PathBuf, Arc<AtomicBool>)>>,
    level_tx: watch::Sender<f32>,
}

impl MicSource {
    pub fn new(settings: Arc<RwLock<AppSettings>>) -> Self {
        Self {
            settings,
            active: Mutex::new(None),
            level_tx: watch::channel(0.0).0,
        }
    }
}

impl AudioSource for MicSource {
    fn start(&self) -> anyhow::Result<()> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((h, dir, stop)) = active.take() {
            stop.store(true, Ordering::SeqCst);
            let _ = h.stop();
            let _ = std::fs::remove_dir_all(dir);
        }
        let device = self
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .microphone
            .clone();
        let dir = std::env::temp_dir().join(format!(
            "minutae-dictation-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let handle = Recorder::new().start(CaptureConfig {
            mic_device: device,
            include_system: false,
            system_app: None,
            out_dir: dir.clone(),
            chunk_secs: 30,
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let (levels, tx, s2) = (handle.levels(), self.level_tx.clone(), stop.clone());
        std::thread::Builder::new()
            .name("minutae-dictation-levels".into())
            .spawn(move || {
                while !s2.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                    tx.send_replace(util::meter_level(levels.borrow().mic_rms));
                }
                tx.send_replace(0.0);
            })?;
        *active = Some((handle, dir, stop));
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<Vec<f32>> {
        let Some((handle, dir, stop)) =
            self.active.lock().unwrap_or_else(|e| e.into_inner()).take()
        else {
            anyhow::bail!("not recording");
        };
        stop.store(true, Ordering::SeqCst);
        let tracks = handle.stop();
        let samples = tracks
            .map_err(anyhow::Error::from)
            .and_then(|t| Ok(minutae_asr::read_wav_16k_mono(&t.mic)?));
        let _ = std::fs::remove_dir_all(&dir);
        samples
    }

    fn levels(&self) -> watch::Receiver<f32> {
        self.level_tx.subscribe()
    }
}

/// Whisper for short clips. Prefers a small model for speed.
pub struct WhisperStt {
    pub settings: Arc<RwLock<AppSettings>>,
    pub models: ModelManager,
    pub cache: Arc<TranscriberCache>,
}

/// The configured model when it is tiny/base and installed, else the smallest installed one.
pub fn dictation_model(models: &ModelManager, preferred: WhisperModel) -> Option<WhisperModel> {
    let installed = |m: WhisperModel| models.status(m).downloaded;
    if matches!(preferred, WhisperModel::Tiny | WhisperModel::Base) && installed(preferred) {
        return Some(preferred);
    }
    [
        WhisperModel::Tiny,
        WhisperModel::Base,
        WhisperModel::Small,
        WhisperModel::Medium,
    ]
    .into_iter()
    .find(|m| installed(*m))
}

#[async_trait]
impl SpeechToText for WhisperStt {
    async fn transcribe(
        &self,
        samples: &[f32],
        vocabulary: &[String],
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<String> {
        let settings = self
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let model = dictation_model(&self.models, settings.whisper_model).ok_or_else(|| {
            anyhow::anyhow!("Download a speech model in Settings → Transcription")
        })?;
        let t = self
            .cache
            .get(&self.models, model)
            .map_err(|f| anyhow::anyhow!(f.message))?;
        let opts = TranscribeOptions {
            cancel: Some(cancel),
            language: settings
                .language
                .clone()
                .filter(|l| !l.trim().is_empty() && l != "auto"),
            ..Default::default()
        }
        .with_vocabulary(vocabulary);
        let samples = samples.to_vec();
        let text =
            tokio::task::spawn_blocking(move || t.transcribe_text(&samples, &opts)).await??;
        Ok(text)
    }
}

pub fn init(app: &AppHandle, rt: tokio::runtime::Handle) {
    let st = app.state::<AppState>();
    let deps = DictationDeps {
        audio: Arc::new(MicSource::new(st.settings.clone())),
        stt: Arc::new(WhisperStt {
            settings: st.settings.clone(),
            models: st.models.clone(),
            cache: st.transcribers.clone(),
        }),
        inserter: Arc::new(SystemInserter::default()),
        apps: Arc::new(SystemAppIdentityLookup),
    };
    let (controller, mut rx) =
        DictationController::new(deps, DictationConfig::from_settings(&st.settings()), rt);
    let _ = st.dictation.set(controller);
    spawn_key_worker(app);
    let (hud_tx, mut requests) = mpsc::channel(32);
    let _ = st.hud_commands.set(hud_tx);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut hud = HudState::Idle;
        let mut clock = HudClock::default();
        loop {
            let next = tokio::select! {
                biased;
                event = rx.recv() => match event {
                    Some(DictationEvent::Hud(h)) => h,
                    Some(DictationEvent::StartSound) => {
                        tauri::async_runtime::spawn_blocking(|| {
                            if let Err(e) = crate::dictation_sound::play() {
                                tracing::debug!(error = %e, "dictation cue unavailable");
                            }
                        });
                        continue;
                    }
                    Some(DictationEvent::Finished(f)) => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move { persist(&app, *f).await });
                        continue;
                    }
                    None => break,
                },
                () = wait_for_expiry(clock.deadline) => HudState::Idle,
                Some(request) = requests.recv() => {
                    if handle_hud_request(&app, request, &mut hud, &mut clock) {
                        HudState::Idle
                    } else {
                        continue;
                    }
                }
            };
            sync_escape(&app, hud_is_active(&next));
            sync_shape_shortcuts(&app, matches!(next, HudState::Listening { .. }));
            // Meter updates should never cause another window show/position call.
            if std::mem::discriminant(&hud) != std::mem::discriminant(&next) {
                clock.set_state(&next);
                crate::shell::present_hud(&app, &next);
            }
            hud = next;
            emit_hud(&app, &hud);
        }
        release_escape(&app);
    });
}

pub(crate) enum HudCommand {
    Get,
    Switch(ShapePreset),
    Interaction { hovered: bool, focused: bool },
    Dismiss,
    DismissError,
    Undo(uuid::Uuid),
    Fix,
}

pub(crate) struct HudRequest {
    command: HudCommand,
    reply: oneshot::Sender<Result<dto::HudState, String>>,
}

pub(crate) async fn hud_request(
    app: &AppHandle,
    command: HudCommand,
) -> Result<dto::HudState, String> {
    let st = app.state::<AppState>();
    let tx = st.hud_commands.get().ok_or("Dictation is unavailable.")?;
    let (reply, result) = oneshot::channel();
    tx.send(HudRequest { command, reply })
        .await
        .map_err(|_| "Dictation is unavailable.")?;
    result.await.map_err(|_| "Dictation is unavailable.")?
}

fn handle_hud_request(
    app: &AppHandle,
    request: HudRequest,
    hud: &mut HudState,
    clock: &mut HudClock,
) -> bool {
    let st = app.state::<AppState>();
    let mut hide = false;
    let result = (|| {
        if crate::shell::is_quitting() {
            return Err("Minutae is quitting.".into());
        }
        let controller = st.dictation.get().ok_or("Dictation is unavailable.")?;
        match request.command {
            HudCommand::Get => {}
            HudCommand::Switch(preset) => {
                if !controller.switch_shape(preset) {
                    return Err("Choose a shape while listening.".into());
                }
            }
            HudCommand::Interaction { hovered, focused } => clock.interact(hovered || focused),
            HudCommand::Dismiss if hud_is_active(hud) => {
                if controller.request_cancel() {
                    send_key(app, DictationKey::Cancel);
                }
            }
            HudCommand::Dismiss => hide = true,
            // An error button or tray click queued during an error must never dismiss a
            // newer session.
            HudCommand::DismissError => hide = matches!(hud, HudState::Error { .. }),
            HudCommand::Undo(result_id) => {
                let HudState::Inserted {
                    result_id: Some(id),
                    undo_available,
                    ..
                } = hud
                else {
                    return Err("Nothing to undo.".into());
                };
                if result_id != *id
                    || !*undo_available
                    || !app
                        .get_webview_window(crate::shell::HUD)
                        .is_some_and(|w| w.is_visible().unwrap_or(false))
                {
                    return Err("Undo is unavailable.".into());
                }
                if !controller.undo_visible_insertion(*id) {
                    *undo_available = false;
                    emit_hud(app, hud);
                    return Err("Undo is unavailable for the current text field.".into());
                }
                hide = true;
            }
            HudCommand::Fix => {
                let HudState::Error { fix_action, .. } = hud else {
                    return Err("There is no dictation error to fix.".into());
                };
                match fix_action {
                    HudFixAction::MicrophoneSettings => {
                        if crate::platform::settings_url(dto::PermissionKind::Microphone).is_some()
                        {
                            crate::commands::open_system_settings(
                                app.clone(),
                                dto::PermissionKind::Microphone,
                            )?;
                        } else {
                            crate::shell::navigate(app, "/settings/general");
                        }
                    }
                    HudFixAction::TranscriptionSettings => {
                        crate::shell::navigate(app, "/settings/transcription")
                    }
                    HudFixAction::DictationSettings => {
                        crate::shell::navigate(app, "/settings/dictation")
                    }
                    HudFixAction::Retry => send_key(app, DictationKey::Retry),
                }
                // Keep reporting the error until dismissed or a new dictation starts.
            }
        }
        Ok(if hide {
            dto::HudState::Idle
        } else {
            hud_dto(app, hud)
        })
    })();
    let _ = request.reply.send(result);
    hide
}

fn hud_is_active(hud: &HudState) -> bool {
    matches!(hud, HudState::Listening { .. } | HudState::Processing)
}

#[derive(Default)]
struct HudClock {
    deadline: Option<Instant>,
    remaining: Option<Duration>,
    interacting: bool,
}

impl HudClock {
    fn set_state(&mut self, hud: &HudState) {
        // A new or finished session: hover and focus on the old result are gone.
        if matches!(
            hud,
            HudState::Idle | HudState::Listening { .. } | HudState::Processing
        ) {
            self.interacting = false;
        }
        let duration = matches!(hud, HudState::Inserted { .. } | HudState::Copied { .. })
            .then_some(Duration::from_secs(6));
        if self.interacting {
            self.remaining = duration;
            self.deadline = None;
        } else {
            self.remaining = None;
            self.deadline = duration.map(|d| Instant::now() + d);
        }
    }

    fn interact(&mut self, interacting: bool) {
        if self.interacting == interacting {
            return;
        }
        self.interacting = interacting;
        let now = Instant::now();
        if interacting {
            self.remaining = self
                .deadline
                .take()
                .map(|deadline| deadline.saturating_duration_since(now));
        } else {
            self.deadline = self.remaining.take().map(|duration| now + duration);
        }
    }
}

async fn wait_for_expiry(deadline: Option<Instant>) {
    match deadline {
        Some(time) => tokio::time::sleep_until(time).await,
        None => std::future::pending().await,
    }
}

#[derive(Default)]
pub(crate) struct EscapeRegistration {
    active: bool,
    bindings: Vec<Shortcut>,
}

impl EscapeRegistration {
    fn transition(
        &mut self,
        active: bool,
        bind: impl FnOnce(&mut Vec<Shortcut>) -> Option<String>,
        mut release: impl FnMut(Shortcut) -> bool,
    ) -> Option<Option<String>> {
        let was_active = std::mem::replace(&mut self.active, active);
        if !active {
            self.bindings.retain(|&key| !release(key));
        }
        if active && !was_active {
            return Some(bind(&mut self.bindings));
        }
        None
    }
}

fn sync_escape(app: &AppHandle, active: bool) {
    let st = app.state::<AppState>();
    let mut escape = st
        .escape_registration
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(detail) = escape.transition(
        active && !crate::shell::is_quitting(),
        |bindings| register_escape(app, bindings),
        |key| {
            app.global_shortcut()
                .unregister(key)
                .map_err(|e| {
                    tracing::warn!(%key, error = %e, "could not release Escape");
                })
                .is_ok()
        },
    ) {
        let status = {
            let mut status = st.shortcut_status.lock().unwrap_or_else(|e| e.into_inner());
            if status.cancel_detail == detail {
                return;
            }
            status.cancel_detail = detail;
            status.clone()
        };
        let _ = app.emit(ev::SHORTCUTS_CHANGED, status);
    }
}

pub fn escape_shortcut() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

fn register_escape(app: &AppHandle, bindings: &mut Vec<Shortcut>) -> Option<String> {
    if crate::portal::use_portal() {
        return Some(dto::PORTAL_CANCEL_DETAIL.into());
    }
    let st = app.state::<AppState>();
    // In hold mode the modifier can still be down when the user presses Escape.
    let held = st
        .shortcuts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .dictate
        .filter(|key| {
            !key.mods.is_empty() && st.settings().dictation_mode == minutae_core::HotkeyMode::Hold
        })
        .map(|key| Shortcut::new(Some(key.mods), Code::Escape));
    let shortcuts = app.global_shortcut();
    let mut bind = |key: Shortcut| {
        if bindings.contains(&key) {
            return true;
        }
        if shortcuts.is_registered(key) {
            return false;
        }
        match shortcuts.on_shortcut(key, |app, _, event| {
            if event.state == ShortcutState::Pressed
                && app
                    .state::<AppState>()
                    .dictation
                    .get()
                    .is_some_and(|c| c.request_cancel())
            {
                send_key(app, DictationKey::Cancel);
            }
        }) {
            Ok(()) => {
                bindings.push(key);
                true
            }
            Err(e) => {
                tracing::warn!(%key, error = %e, "could not bind Escape");
                false
            }
        }
    };

    let plain_ok = bind(escape_shortcut());
    let held_ok = held.map(&mut bind);
    match (plain_ok, held_ok) {
        (true, Some(false)) => Some("Release the dictation shortcut before pressing Esc.".into()),
        (false, Some(true)) => {
            Some("Esc is available only while holding the dictation shortcut.".into())
        }
        (false, _) => Some("The system couldn't register Esc to cancel dictation.".into()),
        _ => None,
    }
}

const SHAPE_KEYS: [Code; 5] = [
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
];

fn held_shape_modifiers(st: &AppState) -> Modifiers {
    let binding = st
        .shortcuts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .dictate;
    Modifiers::ALT | binding.map_or_else(Modifiers::empty, |key| key.mods)
}

fn sync_shape_shortcuts(app: &AppHandle, listening: bool) {
    let st = app.state::<AppState>();
    let mut registered = st
        .shape_registration
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    registered.transition(
        listening && !crate::shell::is_quitting() && !crate::portal::use_portal(),
        |bindings| {
            let held = held_shape_modifiers(&st);
            let held_modifiers = (held != Modifiers::ALT
                && st.settings().dictation_mode == minutae_core::HotkeyMode::Hold)
                .then_some(held);
            let shortcuts = app.global_shortcut();
            for (code, preset) in SHAPE_KEYS.into_iter().zip(QUICK_PRESETS) {
                for mods in std::iter::once(Modifiers::ALT).chain(held_modifiers) {
                    let key = Shortcut::new(Some(mods), code);
                    if !bindings.contains(&key) && !shortcuts.is_registered(key) {
                        match shortcuts.on_shortcut(key, move |app, _, event| {
                            if event.state == ShortcutState::Pressed {
                                send_key(app, DictationKey::Shape(preset));
                            }
                        }) {
                            Ok(()) => bindings.push(key),
                            Err(error) => tracing::debug!(%key, %error, "dictation shape shortcut unavailable"),
                        }
                    }
                }
            }
            None
        },
        |key| app.global_shortcut().unregister(key).map_err(|error| {
            tracing::warn!(%key, %error, "could not release dictation shape shortcut");
        }).is_ok(),
    );
}

pub fn release_escape(app: &AppHandle) {
    sync_escape(app, false);
    sync_shape_shortcuts(app, false);
}

/// A dictation key event from a global shortcut (plugin or portal) or the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationKey {
    Down,
    Up,
    Cancel,
    Shape(ShapePreset),
    Retry,
    /// `--toggle-dictation`: start or stop whatever the hotkey mode.
    Toggle,
}

/// Queue a key event for the dictation worker.
pub fn send_key(app: &AppHandle, key: DictationKey) {
    if let Some(tx) = app.state::<AppState>().dictation_keys.get() {
        let _ = tx.send(key);
    }
}

/// Hotkey presses and releases are applied in order on one worker thread:
/// `key_down`/`key_up` start/stop the mic (and stitch + read the clip), which must not
/// block the main thread that delivers global-shortcut events.
fn spawn_key_worker(app: &AppHandle) {
    let (tx, rx) = std::sync::mpsc::channel::<DictationKey>();
    let a = app.clone();
    let spawned = std::thread::Builder::new()
        .name("minutae-dictation-keys".into())
        .spawn(move || {
            while let Ok(key) = rx.recv() {
                if crate::shell::is_quitting() {
                    break;
                }
                if let Some(c) = a.state::<AppState>().dictation.get() {
                    match key {
                        DictationKey::Down => c.key_down(),
                        DictationKey::Up => c.key_up(),
                        DictationKey::Toggle => c.toggle(),
                        DictationKey::Cancel => c.finish_cancel(),
                        DictationKey::Shape(preset) => {
                            c.switch_shape(preset);
                        }
                        DictationKey::Retry => c.retry(),
                    }
                }
            }
        });
    match spawned {
        Ok(_) => {
            let _ = app.state::<AppState>().dictation_keys.set(tx);
        }
        Err(e) => tracing::error!(error = %e, "dictation key worker failed to start"),
    }
}

fn audio_path(dir: &Path, id: uuid::Uuid) -> PathBuf {
    dir.join(format!("{id}.wav"))
}

/// Only use the app's own dictations directory. In particular, reject a symlink
/// (or Windows junction) to another place, even if it contains matching filenames.
fn checked_audio_dir(data_dir: &Path) -> std::io::Result<PathBuf> {
    let expected = data_dir.canonicalize()?.join("dictations");
    let actual = expected.canonicalize()?;
    if actual != expected {
        return Err(std::io::Error::other(
            "dictations directory points outside its expected path",
        ));
    }
    Ok(actual)
}

fn remove_audio(data_dir: &Path, id: uuid::Uuid) -> std::io::Result<()> {
    let result =
        checked_audio_dir(data_dir).and_then(|dir| std::fs::remove_file(audio_path(&dir, id)));
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

async fn persist(app: &AppHandle, finished: FinishedDictation) {
    let st = app.state::<AppState>();
    let _guard = st.dictation_storage.lock().await;
    let r = st
        .store
        .call(move |store| {
            let data_dir = minutae_core::paths::data_dir();
            let id = finished.dictation.id;
            store.insert_dictation(&finished.dictation)?;
            if let Some(samples) = finished.audio.filter(|a| !a.is_empty()) {
                let result = std::fs::create_dir_all(data_dir.join("dictations"))
                    .and_then(|()| checked_audio_dir(&data_dir))
                    .and_then(|dir| save_dictation_audio(&audio_path(&dir, id), &samples));
                if let Err(e) = result {
                    let _ = remove_audio(&data_dir, id);
                    tracing::warn!(error = %e, "saving dictation audio failed");
                }
            }
            Ok(())
        })
        .await;
    match r {
        Ok(()) => {
            let _ = app.emit(ev::DICTATIONS_CHANGED, ());
        }
        Err(e) => tracing::warn!(error = %e, "saving dictation failed"),
    }
}

pub enum DeleteDictations {
    One(uuid::Uuid),
    All,
    Expired,
}

pub async fn delete(app: &AppHandle, selection: DeleteDictations) -> Result<(), String> {
    let st = app.state::<AppState>();
    let _guard = st.dictation_storage.lock().await;
    let cutoff = match selection {
        DeleteDictations::Expired => {
            let Some(days) = st.settings().dictation_retention_days else {
                return Ok(());
            };
            Some(chrono::Utc::now() - chrono::Duration::days(i64::from(days)))
        }
        _ => None,
    };
    let result = st
        .store
        .call(move |store| {
            let ids = match selection {
                DeleteDictations::One(id) => vec![id],
                _ => store.dictation_ids_before(cutoff)?,
            };
            delete_files_and_rows(store, &minutae_core::paths::data_dir(), ids)
        })
        .await;
    if !matches!(result, Ok(false)) {
        let _ = app.emit(ev::DICTATIONS_CHANGED, ());
    }
    result.map(|_| ()).map_err(|e| e.to_string())
}

fn delete_files_and_rows(
    store: &minutae_store::Store,
    data_dir: &Path,
    ids: Vec<uuid::Uuid>,
) -> minutae_store::Result<bool> {
    let mut changed = false;
    let mut failure = None;
    for id in ids {
        match remove_audio(data_dir, id) {
            Ok(()) => changed |= store.delete_dictation(id)?,
            Err(e) => {
                tracing::warn!(%id, error = %e, "deleting dictation audio failed");
                failure = Some(minutae_store::StoreError::Invalid(
                    "Couldn't delete saved dictation audio.".into(),
                ));
            }
        }
    }
    // Leave rows whose WAV couldn't be removed so cleanup can be retried.
    failure.map_or(Ok(changed), Err)
}

pub async fn prune(app: &AppHandle) {
    if let Err(e) = delete(app, DeleteDictations::Expired).await {
        tracing::warn!(error = %e, "pruning dictations failed");
    }
}

/// 16 kHz mono 16-bit PCM.
pub fn save_dictation_audio(path: &std::path::Path, samples: &[f32]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(std::io::Error::other)?;
    for &x in samples {
        let v = (x.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        w.write_sample(v).map_err(std::io::Error::other)?;
    }
    w.finalize().map_err(std::io::Error::other)
}

fn hud_dto(app: &AppHandle, hud: &HudState) -> dto::HudState {
    let mut dto = dto::HudState::from(hud);
    if let (
        HudState::Listening { hotkey_held, .. },
        dto::HudState::Listening {
            shortcut_presets, ..
        },
    ) = (hud, &mut dto)
    {
        let st = app.state::<AppState>();
        let registered = st
            .shape_registration
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let modifiers = if *hotkey_held {
            held_shape_modifiers(&st)
        } else {
            Modifiers::ALT
        };
        *shortcut_presets = registered_shape_presets(&registered.bindings, modifiers);
    }
    dto
}

fn registered_shape_presets(bindings: &[Shortcut], modifiers: Modifiers) -> Vec<ShapePreset> {
    SHAPE_KEYS
        .into_iter()
        .zip(QUICK_PRESETS)
        .filter_map(|(code, preset)| {
            bindings
                .contains(&Shortcut::new(Some(modifiers), code))
                .then_some(preset)
        })
        .collect()
}

fn emit_hud(app: &AppHandle, hud: &HudState) {
    let _ = app.emit(ev::HUD, hud_dto(app, hud));
}

pub fn apply_settings(app: &AppHandle, settings: &AppSettings) {
    let st = app.state::<AppState>();
    if let Some(c) = st.dictation.get() {
        c.set_config(config_for(settings, st.hold_supported()));
    }
}

/// The controller config: the user's settings, except hold becomes toggle when the
/// shortcut backend never reports key releases (some Wayland portals).
pub fn config_for(settings: &AppSettings, hold_supported: Option<bool>) -> DictationConfig {
    let mut c = DictationConfig::from_settings(settings);
    if hold_supported == Some(false) {
        c.mode = minutae_core::HotkeyMode::Toggle;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_shortcuts_require_registration_for_the_current_modifiers() {
        let plain = Modifiers::ALT;
        let held = Modifiers::ALT | Modifiers::CONTROL;
        let keys = [
            Shortcut::new(Some(plain), Code::Digit1),
            Shortcut::new(Some(held), Code::Digit2),
        ];
        assert_eq!(
            registered_shape_presets(&keys, held),
            [ShapePreset::ChatMessage]
        );
        assert_eq!(
            registered_shape_presets(&keys, plain),
            [ShapePreset::Verbatim]
        );
    }

    #[test]
    fn hold_becomes_toggle_without_releases() {
        use minutae_core::HotkeyMode;
        let s = AppSettings {
            dictation_mode: HotkeyMode::Hold,
            ..AppSettings::default()
        };
        assert_eq!(config_for(&s, Some(true)).mode, HotkeyMode::Hold);
        assert_eq!(config_for(&s, None).mode, HotkeyMode::Hold);
        assert_eq!(config_for(&s, Some(false)).mode, HotkeyMode::Toggle);
    }

    #[test]
    fn escape_and_shape_shortcuts_release_on_every_listening_exit() {
        fn exercise(active: impl Fn(&HudState) -> bool, expected: &[bool]) {
            let mut registration = EscapeRegistration::default();
            let changes = std::cell::RefCell::new(Vec::new());
            let listening = HudState::Listening {
                level: 0.0,
                hands_free: false,
                hotkey_held: true,
                app: None,
                shape: ShapePreset::Tidy,
            };
            for hud in [
                listening.clone(),
                listening.clone(),
                HudState::Processing,
                HudState::Idle,
                listening.clone(),
                HudState::Idle,
                listening,
                HudState::Error {
                    message: "No speech".into(),
                    fix_action: HudFixAction::Retry,
                },
            ] {
                registration.transition(
                    active(&hud),
                    |bindings| {
                        changes.borrow_mut().push(true);
                        bindings.push(escape_shortcut());
                        None
                    },
                    |_| {
                        changes.borrow_mut().push(false);
                        true
                    },
                );
            }
            assert_eq!(changes.into_inner(), expected);
            assert!(registration.bindings.is_empty());
        }
        exercise(hud_is_active, &[true, false, true, false, true, false]);
        exercise(
            |hud| matches!(hud, HudState::Listening { .. }),
            &[true, false, true, false, true, false],
        );
        assert!(hud_is_active(&HudState::Processing));
    }

    #[test]
    fn unavailable_escape_is_reported_and_retried_on_the_next_dictation() {
        let mut escape = EscapeRegistration::default();
        assert_eq!(
            escape.transition(
                true,
                |_| Some(dto::PORTAL_CANCEL_DETAIL.into()),
                |_| unreachable!()
            ),
            Some(Some(dto::PORTAL_CANCEL_DETAIL.into()))
        );
        assert_eq!(
            escape.transition(true, |_| panic!("meter update"), |_| unreachable!()),
            None
        );
        assert_eq!(
            escape.transition(false, |_| unreachable!(), |_| panic!("unregistered")),
            None
        );
        assert_eq!(
            escape.transition(true, |_| None, |_| unreachable!()),
            Some(None)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn hud_times_both_results_and_pauses_for_interaction() {
        for result in [
            HudState::Inserted {
                words: 1,
                undo_available: true,
                result_id: None,
            },
            HudState::Copied {
                words: 1,
                reason: minutae_dictation::CopyReason::NoTextField,
            },
        ] {
            let mut clock = HudClock::default();
            clock.set_state(&result);
            assert_eq!(
                clock.deadline,
                Some(Instant::now() + Duration::from_secs(6))
            );
            tokio::time::advance(Duration::from_secs(2)).await;
            clock.interact(true);
            tokio::time::advance(Duration::from_secs(10)).await;
            assert_eq!(clock.deadline, None);
            clock.interact(false);
            assert_eq!(
                clock.deadline,
                Some(Instant::now() + Duration::from_secs(4))
            );
            clock.set_state(&HudState::Error {
                message: "No speech".into(),
                fix_action: HudFixAction::Retry,
            });
            assert_eq!(clock.deadline, None);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn hud_interaction_does_not_carry_into_a_new_dictation() {
        let inserted = HudState::Inserted {
            words: 1,
            undo_available: false,
            result_id: None,
        };
        for next in [
            HudState::Idle,
            HudState::Listening {
                level: 0.0,
                hands_free: false,
                hotkey_held: true,
                app: None,
                shape: ShapePreset::Tidy,
            },
            HudState::Processing,
        ] {
            let mut clock = HudClock::default();
            clock.set_state(&inserted);
            clock.interact(true);
            clock.set_state(&next);
            assert_eq!(clock.deadline, None);
            clock.set_state(&inserted);
            assert_eq!(
                clock.deadline,
                Some(Instant::now() + Duration::from_secs(6)),
                "result after {next:?} must expire"
            );
        }
    }

    fn dictation_with_audio(
        store: &minutae_store::Store,
        root: &Path,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> (uuid::Uuid, PathBuf) {
        let id = uuid::Uuid::new_v4();
        let path = audio_path(&root.join("dictations"), id);
        store
            .insert_dictation(&minutae_core::Dictation {
                app_identity: None,
                shape: None,
                id,
                created_at,
                text: "test".into(),
                raw_text: "test".into(),
                target_app: None,
                word_count: 1,
                duration_ms: 500,
                inserted: false,
            })
            .unwrap();
        save_dictation_audio(&path, &[0.0, 0.5]).unwrap();
        (id, path)
    }

    #[test]
    fn prune_single_delete_and_bulk_delete_remove_audio_with_the_rows() {
        let store = minutae_store::Store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::days(7);
        let (old, old_wav) =
            dictation_with_audio(&store, dir.path(), cutoff - chrono::Duration::days(1));
        let (boundary, boundary_wav) = dictation_with_audio(&store, dir.path(), cutoff);
        let (recent, recent_wav) = dictation_with_audio(&store, dir.path(), now);

        let expired = store.dictation_ids_before(Some(cutoff)).unwrap();
        assert_eq!(expired, [old]);
        assert!(delete_files_and_rows(&store, dir.path(), expired).unwrap());
        assert!(store.get_dictation(old).unwrap().is_none());
        assert!(!old_wav.exists());
        assert!(boundary_wav.exists());
        assert!(recent_wav.exists());

        assert!(delete_files_and_rows(&store, dir.path(), vec![recent]).unwrap());
        assert!(store.get_dictation(recent).unwrap().is_none());
        assert!(!recent_wav.exists());

        assert_eq!(store.dictation_ids_before(None).unwrap(), [boundary]);
        assert!(
            delete_files_and_rows(
                &store,
                dir.path(),
                store.dictation_ids_before(None).unwrap()
            )
            .unwrap()
        );
        assert!(store.dictation_ids_before(None).unwrap().is_empty());
        assert!(!boundary_wav.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_rejects_redirected_dictation_directory_and_preserves_external_files() {
        let store = minutae_store::Store::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let (id, wav) = dictation_with_audio(&store, dir.path(), chrono::Utc::now());
        let external = audio_path(outside.path(), id);
        std::fs::rename(&wav, &external).unwrap();
        std::fs::remove_dir(wav.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path(), wav.parent().unwrap()).unwrap();
        assert!(delete_files_and_rows(&store, dir.path(), vec![id]).is_err());
        assert!(external.exists());
        assert!(store.get_dictation(id).unwrap().is_some());

        std::fs::remove_file(wav.parent().unwrap()).unwrap();
        std::fs::create_dir(wav.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&external, &wav).unwrap();
        assert!(delete_files_and_rows(&store, dir.path(), vec![id]).unwrap());
        assert!(external.exists());
        assert!(!wav.exists());
    }

    #[test]
    fn dictation_audio_is_16k_mono_pcm() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("d/x.wav");
        save_dictation_audio(&p, &[0.0, 0.5, -1.0, 2.0]).unwrap();
        let r = hound::WavReader::open(&p).unwrap();
        assert_eq!(r.spec().sample_rate, 16_000);
        assert_eq!(r.spec().channels, 1);
        let v: Vec<i16> = r.into_samples().map(|s| s.unwrap()).collect();
        assert_eq!(v, vec![0, 16_384, -32_767, 32_767]);
    }
}
