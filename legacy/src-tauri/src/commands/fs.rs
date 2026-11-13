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
    let filename = safe_filename(&filename)?;
    let path = app_data_dir(&app)?.join("recordings").join(filename);
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
    let filename = safe_filename(&filename)?;
    let path = app_data_dir(&app)?.join("summaries").join(filename);
    fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write summary: {e}"))?;
    path.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Invalid path".to_string())
}

/// Deletes a single file from the recordings folder (used for temporary dictation clips).
#[tauri::command]
pub fn delete_recording_file(app: tauri::AppHandle, filename: String) -> Result<(), String> {
    let filename = safe_filename(&filename)?;
    let path = app_data_dir(&app)?.join("recordings").join(filename);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Failed to delete {}: {e}", path.display()))?;
    }
    Ok(())
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

        // Only markdown files need their contents inspected; never slurp a WAV into memory.
        let is_markdown = path.extension().and_then(|ext| ext.to_str()) == Some("md");
        let contents = if is_markdown {
            fs::read_to_string(&path).unwrap_or_default()
        } else {
            String::new()
        };
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

/// Accepts a bare file name only. Rejects separators, parent refs and hidden files so a
/// caller can never write or delete outside the app's own folders.
fn safe_filename(name: &str) -> Result<&str, String> {
    let bad = name.is_empty()
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0');
    if bad {
        return Err(format!("Invalid file name: {name}"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::safe_filename;

    #[test]
    fn accepts_a_bare_file_name() {
        assert_eq!(
            safe_filename("2026-11-12-notes.md").unwrap(),
            "2026-11-12-notes.md"
        );
    }

    #[test]
    fn rejects_paths_and_hidden_names() {
        for name in ["", ".hidden", "../x", "a/b", "a\\b", "..", "a..b"] {
            assert!(safe_filename(name).is_err(), "{name}");
        }
    }
}
