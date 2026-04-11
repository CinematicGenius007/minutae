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

/// Deletes local recording and summary files associated with a meeting.
#[tauri::command]
pub fn delete_meeting_assets(app: tauri::AppHandle, meeting_id: String) -> Result<(), String> {
    let data_dir = app_data_dir(&app)?;

    let recordings_dir = data_dir.join("recordings");
    if recordings_dir.exists() {
        delete_matching_files(&recordings_dir, |path, _| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains(&meeting_id))
                .unwrap_or(false)
        })?;
    }

    let summaries_dir = data_dir.join("summaries");
    if summaries_dir.exists() {
        delete_matching_files(&summaries_dir, |path, contents| {
            path.extension().and_then(|ext| ext.to_str()) == Some("md")
                && contents.contains(&meeting_id)
        })?;
    }

    Ok(())
}

fn delete_matching_files<F>(dir: &PathBuf, should_delete: F) -> Result<(), String>
where
    F: Fn(&PathBuf, &str) -> bool,
{
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read {}: {e}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let contents = fs::read_to_string(&path).unwrap_or_default();
        if should_delete(&path, &contents) {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete {}: {e}", path.display()))?;
        }
    }

    Ok(())
}

fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
