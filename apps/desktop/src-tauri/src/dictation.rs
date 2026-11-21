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
    spawn_key_worker(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                DictationEvent::Hud(h) => on_hud(&app, &h),
                DictationEvent::Finished(f) => {
                    let st = app.state::<AppState>();
                    if let Some(samples) = f.audio.as_ref().filter(|a| !a.is_empty()) {
                        let path = dictation_audio_path(f.dictation.id);
                        let samples = samples.clone();
                        let r = tokio::task::spawn_blocking(move || {
                            save_dictation_audio(&path, &samples)
                        })
                        .await;
                        if let Ok(Err(e)) | Err(e) = r.map_err(std::io::Error::other) {
                            tracing::warn!(error = %e, "saving dictation audio failed");
                        }
                    }
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

/// A dictation key event from a global shortcut (plugin or portal) or the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationKey {
    Down,
    Up,
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
                if let Some(c) = a.state::<AppState>().dictation.get() {
                    match key {
                        DictationKey::Down => c.key_down(),
                        DictationKey::Up => c.key_up(),
                        DictationKey::Toggle => c.toggle(),
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

/// `<data_dir>/dictations/<id>.wav` (kept when "Keep dictation audio" is on).
pub fn dictation_audio_path(id: uuid::Uuid) -> PathBuf {
    minutae_core::paths::data_dir()
        .join("dictations")
        .join(format!("{id}.wav"))
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
