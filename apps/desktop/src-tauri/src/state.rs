//! App-wide state managed by Tauri, plus the pipeline [`Host`] backed by it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use minutae_asr::{ModelManager, Transcriber, WhisperTranscriber};
use minutae_core::{AppSettings, Failure, Meeting, MeetingId, WhisperModel, paths};
use minutae_diarize::{DiarizeModels, Diarizer, SherpaDiarizer};
use minutae_dictation::DictationController;
use minutae_jobs::{JobRunner, Store};
use minutae_llm::{ApiConfig, KeyringStore, Router, UsageSink};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::calls::CallState;
use crate::dto::{self, ev};
use crate::pipeline::Host;
use crate::recording::ActiveRecording;

/// Small app flags that are not part of `AppSettings` (whose shape is fixed by the contract).
/// Stored as `<data_dir>/app-state.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Flags {
    pub onboarding_done: bool,
    /// Apps for which the user chose "Don't ask again" on the call prompt.
    pub never_prompt_apps: Vec<String>,
}

impl Flags {
    pub fn path() -> PathBuf {
        paths::data_dir().join("app-state.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Err(e) = serde_json::to_string_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|s| std::fs::write(Self::path(), s))
        {
            tracing::warn!(error = %e, "saving app-state.json failed");
        }
    }
}

/// Whisper models loaded once and shared by the pipeline and dictation.
#[derive(Default)]
pub struct TranscriberCache {
    loaded: Mutex<HashMap<&'static str, Arc<WhisperTranscriber>>>,
    /// Every transcriber ever handed out (also ones evicted by `forget`/`clear` that a
    /// running job may still hold), so `shutdown` can free all GPU contexts.
    created: Mutex<Vec<std::sync::Weak<WhisperTranscriber>>>,
}

impl TranscriberCache {
    pub fn get(
        &self,
        models: &ModelManager,
        model: WhisperModel,
    ) -> Result<Arc<WhisperTranscriber>, Failure> {
        let mut map = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = map.get(minutae_asr::model_info(model).name)
            && t.model_path().exists()
        {
            return Ok(t.clone());
        }
        // `MINUTAE_WHISPER_CPU=1` skips Metal/Vulkan (diagnostics, broken GPU drivers).
        let cpu = std::env::var("MINUTAE_WHISPER_CPU").is_ok_and(|v| v == "1");
        let t = Arc::new(
            WhisperTranscriber::from_manager(models, model)
                .map_err(|e| e.to_failure())?
                .with_gpu(!cpu),
        );
        map.insert(minutae_asr::model_info(model).name, t.clone());
        let mut created = self.created.lock().unwrap_or_else(|e| e.into_inner());
        created.retain(|w| w.strong_count() > 0);
        created.push(Arc::downgrade(&t));
        Ok(t)
    }

    /// Unload every whisper model and wait for running transcriptions to let go of them
    /// (each is cancelled by its job's abort guard). Must run before the process exits.
    pub fn shutdown(&self, timeout: std::time::Duration) {
        self.clear();
        let all: Vec<_> = self
            .created
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        for t in all.iter().filter_map(|w| w.upgrade()) {
            t.unload_and_wait(timeout);
        }
    }
    pub fn forget(&self, model: WhisperModel) {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(minutae_asr::model_info(model).name);
    }
    pub fn clear(&self) {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

#[derive(Debug, Clone)]
pub struct Download {
    pub abort: tokio::task::AbortHandle,
    pub fraction: f32,
}

pub struct AppState {
    pub store: Store,
    pub settings: Arc<RwLock<AppSettings>>,
    pub flags: Mutex<Flags>,
    pub runner: OnceLock<JobRunner>,
    pub router: RwLock<Arc<Router>>,
    pub usage_sink: UsageSink,
    pub secrets: Arc<KeyringStore>,
    pub models: ModelManager,
    pub diarize_models: DiarizeModels,
    pub transcribers: Arc<TranscriberCache>,
    /// Active meeting recording. Async mutex: start/stop await blocking device work.
    pub recording: tokio::sync::Mutex<Option<ActiveRecording>>,
    /// Model downloads in flight, keyed by `WhisperModel` wire name or `"diarize"`.
    pub downloads: Mutex<HashMap<String, Download>>,
    pub calls: Mutex<CallState>,
    pub monitor: Mutex<Option<minutae_detect::MonitorHandle>>,
    pub dictation: OnceLock<DictationController>,
    pub shortcuts: Mutex<crate::shell::Shortcuts>,
    pub hud_generation: AtomicU64,
    pub tray_record_item: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>>,
    /// Folders the user chose with `pick_folder` this session: the only custom
    /// recordings/summaries folders `update_settings` accepts.
    pub picked_folders: Mutex<std::collections::HashSet<PathBuf>>,
    /// Last export archive, so the UI may reveal it (it lives outside the data folder).
    pub last_export: Mutex<Option<PathBuf>>,
    /// Dictation hotkey presses/releases, handled in order off the main thread.
    pub dictation_keys: OnceLock<std::sync::mpsc::Sender<crate::dictation::DictationKey>>,
    /// Which global shortcut backend is live and what it bound (Settings → Dictation).
    pub shortcut_status: Mutex<crate::dto::ShortcutStatus>,
    /// The portal backend once bound (Wayland), for `configure_shortcuts`.
    pub shortcut_portal: OnceLock<Arc<dyn minutae_dictation::ShortcutPortal>>,
    /// True while `import_legacy_run` works, so a second click can't start another run.
    pub importing: std::sync::atomic::AtomicBool,
}

impl AppState {
    pub fn new(store: Store, usage_sink: UsageSink) -> Self {
        let settings = store.get_settings().unwrap_or_default();
        let flags = Flags::load();
        let transcribers = TranscriberCache::default();
        let secrets = Arc::new(KeyringStore::default());
        let router = Arc::new(build_router(&settings, secrets.clone(), usage_sink.clone()));
        Self {
            store,
            settings: Arc::new(RwLock::new(settings)),
            flags: Mutex::new(flags),
            runner: OnceLock::new(),
            router: RwLock::new(router),
            usage_sink,
            secrets,
            models: ModelManager::new(),
            diarize_models: DiarizeModels::new(),
            transcribers: Arc::new(transcribers),
            recording: tokio::sync::Mutex::new(None),
            downloads: Mutex::new(HashMap::new()),
            calls: Mutex::new(CallState::default()),
            monitor: Mutex::new(None),
            dictation: OnceLock::new(),
            shortcuts: Mutex::new(Default::default()),
            hud_generation: AtomicU64::new(0),
            tray_record_item: Mutex::new(None),
            picked_folders: Mutex::new(Default::default()),
            last_export: Mutex::new(None),
            dictation_keys: OnceLock::new(),
            shortcut_status: Mutex::new(crate::dto::ShortcutStatus::native()),
            shortcut_portal: OnceLock::new(),
            importing: Default::default(),
        }
    }

    pub fn shortcut_status(&self) -> crate::dto::ShortcutStatus {
        self.shortcut_status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn hold_supported(&self) -> Option<bool> {
        self.shortcut_status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .hold_supported
    }

    pub fn settings(&self) -> AppSettings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn router(&self) -> Arc<Router> {
        self.router
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn rebuild_router(&self, settings: &AppSettings) {
        let r = Arc::new(build_router(
            settings,
            self.secrets.clone(),
            self.usage_sink.clone(),
        ));
        *self.router.write().unwrap_or_else(|e| e.into_inner()) = r;
    }

    pub fn runner(&self) -> Result<&JobRunner, String> {
        self.runner
            .get()
            .ok_or_else(|| "Minutae is still starting.".to_string())
    }

    pub fn flags(&self) -> std::sync::MutexGuard<'_, Flags> {
        self.flags.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn build_router(settings: &AppSettings, secrets: Arc<KeyringStore>, sink: UsageSink) -> Router {
    Router::from_settings(settings, secrets, ApiConfig::default()).with_usage_sink(sink)
}

/// Folder recordings go to: `settings.recordings_path` or the default.
pub fn recordings_root(settings: &AppSettings) -> PathBuf {
    settings
        .recordings_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(paths::recordings_dir)
}

pub fn summaries_root(settings: &AppSettings) -> PathBuf {
    settings
        .summaries_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(paths::summaries_dir)
}

/// Folders `open_path` / `reveal_path` may touch.
pub fn user_path_roots(settings: &AppSettings) -> Vec<PathBuf> {
    vec![
        paths::data_dir(),
        paths::recordings_dir(),
        paths::summaries_dir(),
        recordings_root(settings),
        summaries_root(settings),
    ]
}

pub fn emit_meetings_changed(app: &AppHandle, id: Option<MeetingId>) {
    let _ = app.emit(
        ev::MEETINGS_CHANGED,
        dto::MeetingsChanged {
            meeting_id: id.map(|i| i.to_string()),
        },
    );
    crate::shell::refresh_tray(app);
}

/// The pipeline's view of the app.
pub struct AppHost {
    pub app: AppHandle,
}

impl Host for AppHost {
    fn settings(&self) -> AppSettings {
        self.app.state::<AppState>().settings()
    }

    fn transcriber(&self, model: WhisperModel) -> Result<Arc<dyn Transcriber>, Failure> {
        let st = self.app.state::<AppState>();
        Ok(st.transcribers.get(&st.models, model)? as Arc<dyn Transcriber>)
    }

    fn diarizer(&self) -> Option<Arc<dyn Diarizer>> {
        let st = self.app.state::<AppState>();
        SherpaDiarizer::new(&st.diarize_models)
            .ok()
            .map(|d| Arc::new(d) as Arc<dyn Diarizer>)
    }

    fn echo_canceller(&self) -> Arc<dyn minutae_aec::EchoCanceller> {
        Arc::new(minutae_aec::OfflineAec::default())
    }

    fn router(&self) -> Arc<Router> {
        self.app.state::<AppState>().router()
    }

    fn summaries_dir(&self) -> PathBuf {
        summaries_root(&self.settings())
    }

    fn meetings_changed(&self, id: MeetingId) {
        emit_meetings_changed(&self.app, Some(id));
    }

    fn summary_ready(&self, meeting: &Meeting) {
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title("Summary ready")
            .body(&meeting.title)
            .show()
        {
            tracing::debug!(error = %e, "notification failed");
        }
    }
}
