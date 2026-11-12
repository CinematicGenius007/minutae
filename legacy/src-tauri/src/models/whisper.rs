use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{Emitter, Manager};
use tauri::Window;

#[derive(Debug, Serialize, Deserialize)]
pub struct WhisperSegment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhisperResult {
    pub text: String,
    pub language: String,
    pub segments: Vec<WhisperSegment>,
    pub duration_ms: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalTranscriptionStatus {
    pub whisper_binary_path: String,
    pub model_path: String,
    pub binary_exists: bool,
    pub model_exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WhisperModelDownloadResult {
    pub model_path: String,
    pub source_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperDownloadProgress {
    pub model: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub percent: Option<f64>,
}

/// Transcribe an audio file using the bundled whisper.cpp binary.
/// The binary is expected at: <resourceDir>/whisper
/// Models live at: <appDataDir>/models/ggml-{model}.bin
#[tauri::command]
pub fn transcribe_local(
    app: tauri::AppHandle,
    audio_path: String,
    model: String,
) -> Result<WhisperResult, String> {
    let status = local_transcription_status(&app, &model)?;
    let whisper_bin = std::path::PathBuf::from(&status.whisper_binary_path);
    let model_path = std::path::PathBuf::from(&status.model_path);

    if !status.binary_exists {
        return Err(format!(
            "Local whisper.cpp is not bundled yet. Expected binary at {}. Switch to Whisper API in Settings or bundle the whisper executable for local transcription.",
            whisper_bin.display(),
        ));
    }
    if !status.model_exists {
        return Err(format!(
            "Model ggml-{model}.bin not found at {}. Download or place the model in ~/Library/Minutae/models/.",
            model_path.display(),
        ));
    }

    let output_base = std::env::temp_dir().join(format!(
        "minutae-whisper-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Failed to compute temp output path: {e}"))?
            .as_millis()
    ));
    let output_json = output_base.with_extension("json");

    // whisper-cli flags:
    //   -m  model path
    //   -f  audio file
    //   -oj output JSON file
    //   -of output file base path
    //   -l  language (auto-detect)
    let output = Command::new(&whisper_bin)
        .args([
            "-m", model_path.to_str().unwrap_or(""),
            "-f", &audio_path,
            "-oj",
            "-of", output_base.to_str().unwrap_or(""),
            "--language", "auto",
        ])
        .output()
        .map_err(|e| format!("Failed to run whisper: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("whisper exited with error: {stderr}"));
    }

    let json = fs::read_to_string(&output_json).map_err(|e| {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        format!(
            "whisper completed but JSON output was not found at {}: {e}\nstdout:\n{}\nstderr:\n{}",
            output_json.display(),
            stdout,
            stderr
        )
    })?;

    let result = parse_whisper_output(&json);
    let _ = fs::remove_file(&output_json);
    result
}

#[tauri::command]
pub fn get_local_transcription_status(
    app: tauri::AppHandle,
    model: String,
) -> Result<LocalTranscriptionStatus, String> {
    local_transcription_status(&app, &model)
}

#[tauri::command]
pub fn download_whisper_model(
    window: Window,
    app: tauri::AppHandle,
    model: String,
) -> Result<WhisperModelDownloadResult, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let models_dir = data_dir.join("models");
    fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {e}"))?;

    let filename = format!("ggml-{model}.bin");
    let model_path = models_dir.join(&filename);
    let source_url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}?download=true"
    );

    let mut response = reqwest::blocking::get(&source_url)
        .map_err(|e| format!("Failed to download model: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download model: HTTP {}",
            response.status()
        ));
    }

    let total_bytes = response.content_length();
    let mut file = fs::File::create(&model_path)
        .map_err(|e| format!("Failed to create model file at {}: {e}", model_path.display()))?;

    let mut downloaded_bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read model download: {e}"))?;
        if read == 0 {
            break;
        }

        file.write_all(&buffer[..read])
            .map_err(|e| format!("Failed to save model to {}: {e}", model_path.display()))?;

        downloaded_bytes += read as u64;
        emit_download_progress(&window, &model, downloaded_bytes, total_bytes)?;
    }

    emit_download_progress(&window, &model, downloaded_bytes, total_bytes)?;

    Ok(WhisperModelDownloadResult {
        model_path: model_path.display().to_string(),
        source_url,
    })
}

fn local_transcription_status(
    app: &tauri::AppHandle,
    model: &str,
) -> Result<LocalTranscriptionStatus, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let whisper_bin = resolve_whisper_binary(app);
    let model_path = data_dir.join("models").join(format!("ggml-{model}.bin"));

    Ok(LocalTranscriptionStatus {
        whisper_binary_path: whisper_bin.display().to_string(),
        model_path: model_path.display().to_string(),
        binary_exists: whisper_bin.exists(),
        model_exists: model_path.exists(),
    })
}

fn resolve_whisper_binary(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(explicit_path) = std::env::var("MINUTAE_WHISPER_PATH") {
        let path = PathBuf::from(explicit_path);
        if path.exists() {
            return path;
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        for candidate in binary_candidates(&resource_dir) {
            if candidate.exists() {
                return candidate;
            }
        }
    }

    if let Ok(project_dir) = std::env::current_dir() {
        let resources_dir = project_dir.join("src-tauri").join("resources");
        for candidate in binary_candidates(&resources_dir) {
            if candidate.exists() {
                return candidate;
            }
        }
    }

    for executable in ["whisper-cli", "whisper"] {
        if let Some(path) = command_on_path(executable) {
            return path;
        }
    }

    if let Some(prefix) = brew_prefix("whisper-cpp") {
        for candidate in binary_candidates(&prefix.join("bin")) {
            if candidate.exists() {
                return candidate;
            }
        }
    }

    PathBuf::from("whisper-cli")
}

fn binary_candidates(base: &Path) -> [PathBuf; 2] {
    [base.join("whisper"), base.join("whisper-cli")]
}

fn command_on_path(command: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(command).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn brew_prefix(formula: &str) -> Option<PathBuf> {
    let output = Command::new("brew")
        .args(["--prefix", formula])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn emit_download_progress(
    window: &Window,
    model: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) -> Result<(), String> {
    let percent = total_bytes
        .filter(|total| *total > 0)
        .map(|total| (downloaded_bytes as f64 / total as f64) * 100.0);

    window
        .emit(
            "whisper:model-download-progress",
            WhisperDownloadProgress {
                model: model.to_string(),
                downloaded_bytes,
                total_bytes,
                percent,
            },
        )
        .map_err(|e| format!("Failed to emit download progress: {e}"))
}

fn parse_whisper_output(json: &str) -> Result<WhisperResult, String> {
    #[derive(Deserialize)]
    struct RawOutput {
        transcription: Vec<RawSegment>,
    }

    #[derive(Deserialize)]
    struct RawSegment {
        text: String,
        timestamps: RawTimestamps,
    }

    #[derive(Deserialize)]
    struct RawTimestamps {
        from: String, // "00:00:00,000"
        to: String,
    }

    fn parse_ts(ts: &str) -> i64 {
        // Format: "HH:MM:SS,mmm"
        let parts: Vec<&str> = ts.splitn(2, ',').collect();
        let ms: i64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let hms: Vec<i64> = parts[0]
            .split(':')
            .filter_map(|s| s.parse().ok())
            .collect();
        let seconds = match hms.as_slice() {
            [h, m, s] => h * 3600 + m * 60 + s,
            [m, s] => m * 60 + s,
            [s] => *s,
            _ => 0,
        };
        seconds * 1000 + ms
    }

    let raw: RawOutput = serde_json::from_str(json)
        .map_err(|e| format!("Failed to parse whisper JSON: {e}\nOutput: {json}"))?;

    let segments: Vec<WhisperSegment> = raw
        .transcription
        .iter()
        .map(|s| WhisperSegment {
            start_ms: parse_ts(&s.timestamps.from),
            end_ms: parse_ts(&s.timestamps.to),
            text: s.text.trim().to_string(),
        })
        .collect();

    let full_text = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
    let duration_ms = segments.last().map(|s| s.end_ms).unwrap_or(0);

    Ok(WhisperResult {
        text: full_text,
        language: "auto".to_string(),
        segments,
        duration_ms,
    })
}
