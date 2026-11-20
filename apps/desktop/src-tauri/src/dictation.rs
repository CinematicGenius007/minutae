//! Dictation wiring: mic capture (minutae-audio) + whisper (minutae-asr) + system paste,
//! driven by `DictationController`; HUD events go to the `hud` window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use minutae_asr::{ModelManager, TranscribeOptions, Transcriber};
use minutae_audio::{CaptureConfig, Recorder, RecordingHandle};
use minutae_core::{AppSettings, WhisperModel};
use minutae_dictation::{
    AudioSource, DictationConfig, DictationController, DictationDeps, DictationEvent, HudState,
    SpeechToText, SystemInserter,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

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
    async fn transcribe(&self, samples: &[f32], vocabulary: &[String]) -> anyhow::Result<String> {
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
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                DictationEvent::Hud(h) => on_hud(&app, &h),
                DictationEvent::Finished(f) => {
                    let st = app.state::<AppState>();
                    let d = f.dictation.clone();
                    if let Err(e) = st.store.call(move |s| s.insert_dictation(&d)).await {
                        tracing::warn!(error = %e, "saving dictation failed");
                    }
                    let _ = app.emit(ev::DICTATIONS_CHANGED, ());
                }
            }
        }
    });
}

fn on_hud(app: &AppHandle, h: &HudState) {
    let st = app.state::<AppState>();
    let generation = st.hud_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = app.emit(ev::HUD, dto::HudState::from(h));
    match h {
        HudState::Idle => crate::shell::hide_aux(app, crate::shell::HUD),
        HudState::Listening { level, .. } if *level > 0.0 => {} // already visible; avoid 30 Hz show calls
        HudState::Listening { .. } | HudState::Processing => {
            crate::shell::show_aux(app, crate::shell::HUD)
        }
        HudState::Inserted { .. } | HudState::Copied { .. } | HudState::Error { .. } => {
            crate::shell::show_aux(app, crate::shell::HUD);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                let st = app.state::<AppState>();
                if st.hud_generation.load(Ordering::SeqCst) == generation {
                    crate::shell::hide_aux(&app, crate::shell::HUD);
                    let _ = app.emit(ev::HUD, dto::HudState::Idle);
                }
            });
        }
    }
}

pub fn apply_settings(app: &AppHandle, settings: &AppSettings) {
    if let Some(c) = app.state::<AppState>().dictation.get() {
        c.set_config(DictationConfig::from_settings(settings));
    }
}
