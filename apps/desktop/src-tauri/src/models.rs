//! Whisper + speaker model downloads with `model://progress` events.

use std::time::{Duration, Instant};

use minutae_core::WhisperModel;
use tauri::{AppHandle, Emitter, Manager};

use crate::dto::{DiarizeModelInfo, ModelInfo, ModelProgress, ModelState, ev};
use crate::state::{AppState, Download};

pub const DIARIZE: &str = "diarize";
pub const ALL: [WhisperModel; 4] = [
    WhisperModel::Tiny,
    WhisperModel::Base,
    WhisperModel::Small,
    WhisperModel::Medium,
];

pub fn key(model: WhisperModel) -> String {
    minutae_asr::model_info(model).name.to_string()
}

fn downloading(st: &AppState, key: &str) -> Option<f32> {
    st.downloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .map(|d| d.fraction)
}

pub fn list(st: &AppState) -> Vec<ModelInfo> {
    ALL.iter()
        .map(|&m| {
            let s = st.models.status(m);
            let fraction = downloading(st, &key(m));
            ModelInfo {
                model: m,
                size_bytes: if s.downloaded {
                    s.size_bytes
                } else {
                    s.info.approx_bytes
                },
                state: if fraction.is_some() {
                    ModelState::Downloading
                } else if s.downloaded {
                    ModelState::Ready
                } else {
                    ModelState::Missing
                },
                fraction,
            }
        })
        .collect()
}

pub fn diarize_info(st: &AppState) -> DiarizeModelInfo {
    let s = st.diarize_models.status();
    let fraction = downloading(st, DIARIZE);
    DiarizeModelInfo {
        state: if fraction.is_some() {
            ModelState::Downloading
        } else if s.ready {
            ModelState::Ready
        } else {
            ModelState::Missing
        },
        fraction,
        size_bytes: if s.ready {
            s.size_bytes
        } else {
            s.download_bytes
        },
    }
}

fn emit(app: &AppHandle, key: &str, fraction: f32, done: bool, error: Option<String>) {
    let _ = app.emit(
        ev::MODEL_PROGRESS,
        ModelProgress {
            model: key.to_string(),
            fraction,
            done,
            error,
        },
    );
}

/// Throttled progress reporter (≤ 10 events/s).
fn reporter(app: AppHandle, key: String) -> impl FnMut(Option<f32>) {
    let mut last = Instant::now() - Duration::from_secs(1);
    move |fraction: Option<f32>| {
        let f = fraction.unwrap_or(0.0);
        if last.elapsed() >= Duration::from_millis(100) {
            last = Instant::now();
            if let Some(d) = app
                .state::<AppState>()
                .downloads
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&key)
            {
                d.fraction = f;
            }
            emit(&app, &key, f, false, None);
        }
    }
}

fn start(
    app: &AppHandle,
    key: String,
    task: impl Future<Output = Result<(), String>> + Send + 'static,
) {
    let st = app.state::<AppState>();
    let mut downloads = st.downloads.lock().unwrap_or_else(|e| e.into_inner());
    if downloads.contains_key(&key) {
        return;
    }
    let a = app.clone();
    let k = key.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let r = task.await;
        a.state::<AppState>()
            .downloads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&k);
        match r {
            Ok(()) => emit(&a, &k, 1.0, true, None),
            Err(e) => {
                tracing::warn!(model = %k, error = %e, "model download failed");
                emit(&a, &k, 0.0, true, Some(e));
            }
        }
    });
    downloads.insert(
        key,
        Download {
            abort: handle.inner().abort_handle(),
            fraction: 0.0,
        },
    );
}

pub fn download(app: &AppHandle, model: WhisperModel) {
    let st = app.state::<AppState>();
    let manager = st.models.clone();
    let mut report = reporter(app.clone(), key(model));
    start(app, key(model), async move {
        manager
            .download(model, |p| report(p.fraction()))
            .await
            .map(|_| ())
            .map_err(|e| e.to_failure().message)
    });
}

pub fn download_diarize(app: &AppHandle) {
    let st = app.state::<AppState>();
    let models = st.diarize_models.clone();
    let mut report = reporter(app.clone(), DIARIZE.into());
    start(app, DIARIZE.into(), async move {
        models
            .download(|p| report(Some(p.fraction())))
            .await
            .map_err(|e| e.to_failure().message)
    });
}

/// Abort an in-flight download (the `.part` file is kept for resume).
pub fn cancel(app: &AppHandle, key: &str) {
    let removed = app
        .state::<AppState>()
        .downloads
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(key);
    if let Some(d) = removed {
        d.abort.abort();
        emit(app, key, 0.0, true, Some("Cancelled".into()));
    }
}

pub fn delete(app: &AppHandle, model: WhisperModel) -> Result<(), String> {
    cancel(app, &key(model));
    let st = app.state::<AppState>();
    st.transcribers.forget(model);
    st.models.delete(model).map_err(|e| e.to_string())
}
