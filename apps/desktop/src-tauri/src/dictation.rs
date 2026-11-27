//! Dictation wiring: mic capture (minutae-audio) + whisper (minutae-asr) + system paste,
//! driven by `DictationController`; HUD events go to the `hud` window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use minutae_asr::{ModelManager, TranscribeOptions, Transcriber};
use minutae_audio::{CaptureConfig, Recorder, RecordingHandle};
use minutae_core::{AppSettings, WhisperModel};
use minutae_dictation::{
    AudioSource, DictationConfig, DictationController, DictationDeps, DictationEvent,
    FinishedDictation, HudState, SpeechToText, SystemInserter,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::{
    sync::{mpsc, watch},
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
    };
    let (controller, mut rx) =
        DictationController::new(deps, DictationConfig::from_settings(&st.settings()), rt);
    let _ = st.dictation.set(controller);
    spawn_key_worker(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut deadline = None;
        while let Some(event) = next_event(&mut rx, &mut deadline).await {
            match event {
                DictationEvent::Hud(h) => {
                    sync_escape(&app, hud_is_active(&h));
                    on_hud(&app, &h);
                }
                DictationEvent::Finished(f) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { persist(&app, *f).await });
                }
            }
        }
        sync_escape(&app, false);
    });
}

fn hud_is_active(hud: &HudState) -> bool {
    matches!(hud, HudState::Listening { .. } | HudState::Processing)
}

fn hud_duration(hud: &HudState) -> Option<Duration> {
    Some(Duration::from_millis(match hud {
        HudState::Inserted { .. } => 1500,
        HudState::Copied { .. } => 4000,
        HudState::Error { .. } => 6000,
        _ => return None,
    }))
}

/// HUD events and expiry run serially. A newer event takes precedence over a
/// previous HUD's timer, even if they become ready at the same time.
async fn next_event(
    events: &mut mpsc::UnboundedReceiver<DictationEvent>,
    deadline: &mut Option<Instant>,
) -> Option<DictationEvent> {
    let event = tokio::select! {
        biased;
        event = events.recv() => event,
        () = wait_for_expiry(*deadline) => Some(DictationEvent::Hud(HudState::Idle)),
    };
    if let Some(DictationEvent::Hud(hud)) = &event {
        *deadline = hud_duration(hud).map(|duration| Instant::now() + duration);
    }
    event
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

pub fn release_escape(app: &AppHandle) {
    sync_escape(app, false);
}

/// A dictation key event from a global shortcut (plugin or portal) or the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationKey {
    Down,
    Up,
    Cancel,
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

fn on_hud(app: &AppHandle, h: &HudState) {
    let _ = app.emit(ev::HUD, dto::HudState::from(h));
    match h {
        HudState::Idle => crate::shell::hide_aux(app, crate::shell::HUD),
        HudState::Listening { level, .. } if *level > 0.0 => {} // already visible; avoid meter-rate show calls
        _ => crate::shell::show_aux(app, crate::shell::HUD),
    }
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
    fn escape_is_registered_only_during_an_active_dictation() {
        let mut escape = EscapeRegistration::default();
        let changes = std::cell::RefCell::new(Vec::new());
        for hud in [
            HudState::Listening {
                level: 0.0,
                hands_free: false,
            },
            HudState::Listening {
                level: 0.5,
                hands_free: true,
            },
            HudState::Processing,
            HudState::Idle,
            HudState::Listening {
                level: 0.0,
                hands_free: false,
            },
            HudState::Inserted { words: 1 },
            HudState::Listening {
                level: 0.0,
                hands_free: false,
            },
            HudState::Error {
                message: "no speech".into(),
            },
            HudState::Listening {
                level: 0.0,
                hands_free: false,
            },
            HudState::Copied {
                words: 1,
                reason: minutae_dictation::CopyReason::Wayland,
            },
        ] {
            escape.transition(
                hud_is_active(&hud),
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
        assert_eq!(
            changes.into_inner(),
            [true, false, true, false, true, false, true, false]
        );
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

    #[test]
    fn hud_outcomes_have_distinct_timeouts() {
        assert_eq!(
            hud_duration(&HudState::Inserted { words: 1 }),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            hud_duration(&HudState::Copied {
                words: 1,
                reason: minutae_dictation::CopyReason::NoTextField
            }),
            Some(Duration::from_secs(4))
        );
        assert_eq!(
            hud_duration(&HudState::Error {
                message: "no speech".into()
            }),
            Some(Duration::from_secs(6))
        );
        assert_eq!(hud_duration(&HudState::Processing), None);
        assert_eq!(hud_duration(&HudState::Idle), None);
    }

    #[tokio::test(start_paused = true)]
    async fn old_timeout_cannot_hide_a_new_dictation() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut deadline = None;
        let inserted = DictationEvent::Hud(HudState::Inserted { words: 1 });
        tx.send(inserted.clone()).unwrap();
        assert_eq!(next_event(&mut rx, &mut deadline).await, Some(inserted));

        tokio::time::advance(Duration::from_millis(1500)).await;
        let listening = DictationEvent::Hud(HudState::Listening {
            level: 0.0,
            hands_free: false,
        });
        tx.send(listening.clone()).unwrap();
        assert_eq!(next_event(&mut rx, &mut deadline).await, Some(listening));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), next_event(&mut rx, &mut deadline))
                .await
                .is_err()
        );

        let error = DictationEvent::Hud(HudState::Error {
            message: "no speech".into(),
        });
        tx.send(error.clone()).unwrap();
        assert_eq!(next_event(&mut rx, &mut deadline).await, Some(error));
        assert_eq!(
            next_event(&mut rx, &mut deadline).await,
            Some(DictationEvent::Hud(HudState::Idle))
        );
        assert!(deadline.is_none());
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
