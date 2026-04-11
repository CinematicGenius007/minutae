use std::fs;
use std::path::PathBuf;
use tauri::Manager;

/// Ensures the app's required directories exist under ~/Library/Minutae/
#[tauri::command]
pub fn ensure_app_dirs(app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app_data_dir(&app)?;
    for dir in &["recordings", "summaries", "models", "exports"] {
        fs::create_dir_all(data_dir.join(dir))
            .map_err(|e| format!("Failed to create {dir}: {e}"))?;
    }
    Ok(())
}

/// Saves raw audio bytes to ~/Library/Minutae/recordings/ and returns the path.
#[tauri::command]
pub fn save_recording(
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    filename: String,
) -> Result<String, String> {
    let path = app_data_dir(&app)?.join("recordings").join(&filename);
    fs::write(&path, &bytes).map_err(|e| format!("Failed to write recording: {e}"))?;
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid path".to_string())
}

/// Writes a markdown summary file to ~/Library/Minutae/summaries/.
#[tauri::command]
pub fn write_summary_file(
    app: tauri::AppHandle,
    filename: String,
    content: String,
) -> Result<String, String> {
    let path = app_data_dir(&app)?.join("summaries").join(&filename);
    fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write summary: {e}"))?;
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid path".to_string())
}

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
