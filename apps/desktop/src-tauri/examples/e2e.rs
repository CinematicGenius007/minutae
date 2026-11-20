//! End-to-end pipeline without the UI: record (or use given WAVs) → transcribe → diarize →
//! summarize → index, with the real crates and the app's job handlers.
//!
//! ```sh
//! MINUTAE_DATA_DIR=/tmp/minutae-e2e-data cargo run -p minutae-desktop --example e2e -- 8
//! MINUTAE_DATA_DIR=... cargo run -p minutae-desktop --example e2e -- --mic a.wav --system b.wav
//! ```
//! Models are read from `$MINUTAE_DATA_DIR/models` (whisper `ggml-base.bin` + `diarize/`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use minutae_asr::{ModelManager, Transcriber, WhisperTranscriber};
use minutae_audio::{CaptureConfig, RecordedTracks, Recorder};
use minutae_core::{AppSettings, Failure, Meeting, MeetingId, MeetingStatus, WhisperModel, paths};
use minutae_desktop_lib::pipeline::{self, Host};
use minutae_diarize::{DiarizeModels, Diarizer, SherpaDiarizer};
use minutae_jobs::{JobKind, JobRunner};
use minutae_llm::{ApiConfig, KeyringStore, Router};
use minutae_store::Store;

struct CliHost {
    settings: AppSettings,
    router: Arc<Router>,
    whisper: Arc<WhisperTranscriber>,
}

impl Host for CliHost {
    fn settings(&self) -> AppSettings {
        self.settings.clone()
    }
    fn transcriber(&self, _model: WhisperModel) -> Result<Arc<dyn Transcriber>, Failure> {
        Ok(self.whisper.clone())
    }
    fn diarizer(&self) -> Option<Arc<dyn Diarizer>> {
        SherpaDiarizer::new(&DiarizeModels::new())
            .ok()
            .map(|d| Arc::new(d) as Arc<dyn Diarizer>)
    }
    fn router(&self) -> Arc<Router> {
        self.router.clone()
    }
    fn summaries_dir(&self) -> PathBuf {
        paths::summaries_dir()
    }
    fn meetings_changed(&self, _id: MeetingId) {}
    fn summary_ready(&self, m: &Meeting) {
        println!("summary ready: {}", m.title);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,whisper_rs=warn")
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    paths::ensure_all()?;
    let store = Store::open(paths::database_file())?;
    let settings = AppSettings {
        whisper_model: WhisperModel::Base,
        ..store.get_settings()?
    };
    let id = uuid::Uuid::new_v4();
    let started = Utc::now();
    store.create_meeting(&Meeting {
        id,
        title: pipeline::DEFAULT_TITLE.into(),
        started_at: started,
        ended_at: None,
        status: MeetingStatus::Recording,
        source_app: None,
        failure: None,
    })?;

    let tracks = if let Some(mic) = arg("--mic") {
        let system = arg("--system");
        let duration_ms = minutae_asr::read_wav_16k_mono(&mic)?.len() as u64 / 16;
        RecordedTracks {
            mic,
            system,
            duration_ms,
        }
    } else {
        let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(8);
        let dir = paths::recordings_dir().join(id.to_string());
        let handle = Recorder::new().start(CaptureConfig {
            include_system: true,
            ..CaptureConfig::new(dir)
        })?;
        println!(
            "recording {secs} s (system audio: {})",
            handle.system_error().unwrap_or("on")
        );
        tokio::time::sleep(Duration::from_secs(secs)).await;
        handle.stop()?
    };
    println!(
        "tracks: mic={} system={:?} {} ms",
        tracks.mic.display(),
        tracks.system,
        tracks.duration_ms
    );
    minutae_desktop_lib::recording::finish_recorded(&store, id, &tracks, Utc::now()).await?;

    let gpu = std::env::var("NO_GPU").map(|v| v != "1").unwrap_or(true);
    let whisper = Arc::new(
        WhisperTranscriber::from_manager(&ModelManager::new(), WhisperModel::Base)?.with_gpu(gpu),
    );
    let (sink, mut usage) = minutae_llm::usage_channel();
    let router = Arc::new(
        Router::from_settings(
            &settings,
            Arc::new(KeyringStore::default()),
            ApiConfig::default(),
        )
        .with_usage_sink(sink),
    );
    for (t, a) in router.statuses().await {
        println!("provider {t:?}: {a:?}");
    }
    let model = whisper.clone();
    let host = Arc::new(CliHost {
        settings,
        router,
        whisper,
    });
    let runner = JobRunner::new(store.clone(), pipeline::handlers(host));
    let mut rx = runner.subscribe();
    let s2 = store.clone();
    tokio::spawn(async move {
        while let Ok(u) = rx.recv().await {
            if u.progress.is_none() {
                println!(
                    "job {:?} → {:?} {:?}",
                    u.kind,
                    u.state,
                    u.failure.as_ref().map(|f| &f.message)
                );
            }
            pipeline::apply_job_update(&s2, &u);
        }
    });
    tokio::spawn(async move {
        while let Some(r) = usage.recv().await {
            println!(
                "llm call: {} {:?} {:?} {} ms",
                r.provider, r.model, r.error_class, r.duration_ms
            );
        }
    });
    runner.start().await?;
    runner
        .enqueue(JobKind::Transcribe, serde_json::json!({}), Some(id))
        .await?;
    let deadline = std::time::Instant::now() + Duration::from_secs(600);
    let m = loop {
        let m = store.get_meeting(id)?.unwrap();
        if matches!(m.status, MeetingStatus::Done | MeetingStatus::Failed)
            || std::time::Instant::now() > deadline
        {
            break m;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    runner.shutdown().await;
    // ggml-metal asserts at exit if a Metal context is still alive.
    tokio::task::spawn_blocking(move || model.unload_and_wait(Duration::from_secs(10))).await?;

    println!("\n== meeting {} — {:?} — {}", m.id, m.status, m.title);
    if let Some(f) = &m.failure {
        println!("failure: {:?}: {} (fix: {:?})", f.kind, f.message, f.fix);
    }
    let speakers = store.list_speakers(id)?;
    for s in &speakers {
        println!("speaker {:?} {}", s.track, s.label);
    }
    if let Some(t) = store.get_transcript(id)? {
        println!(
            "transcript ({} segments, {}):\n{}",
            t.segments.len(),
            t.model,
            pipeline::transcript_text(&t.segments, &speakers)
        );
    }
    if let Some(s) = store.get_summary(id)? {
        println!("summary by {} / {}:\n{}", s.provider, s.model, s.markdown);
    }
    Ok(())
}
