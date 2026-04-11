use serde::{Deserialize, Serialize};
use std::process::Command;
use tauri::Manager;

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

/// Transcribe an audio file using the bundled whisper.cpp binary.
/// The binary is expected at: <resourceDir>/whisper
/// Models live at: <appDataDir>/models/ggml-{model}.bin
#[tauri::command]
pub fn transcribe_local(
    app: tauri::AppHandle,
    audio_path: String,
    model: String,
) -> Result<WhisperResult, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let whisper_bin = resource_dir.join("whisper");
    let model_path = data_dir.join("models").join(format!("ggml-{model}.bin"));

    if !whisper_bin.exists() {
        return Err(format!(
            "whisper binary not found at {}",
            whisper_bin.display()
        ));
    }
    if !model_path.exists() {
        return Err(format!(
            "Model ggml-{model}.bin not found. Download it in Settings → Transcription."
        ));
    }

    // whisper-cli flags:
    //   -m  model path
    //   -f  audio file
    //   -oj output JSON to stdout
    //   -l  language (auto-detect)
    let output = Command::new(&whisper_bin)
        .args([
            "-m", model_path.to_str().unwrap_or(""),
            "-f", &audio_path,
            "-oj",
            "--language", "auto",
            "--no-timestamps", "false",
        ])
        .output()
        .map_err(|e| format!("Failed to run whisper: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("whisper exited with error: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_whisper_output(&stdout)
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
