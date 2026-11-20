//! Per-OS data locations (docs/CROSS-PLATFORM.md §4).

use std::path::PathBuf;

/// Root data directory.
/// - macOS: `~/Library/Minutae` (unchanged from earlier versions)
/// - Windows: `%APPDATA%\Minutae`
/// - Linux: `$XDG_DATA_HOME/minutae` (falls back to `~/.local/share/minutae`)
///
/// `MINUTAE_DATA_DIR` overrides it (used by tests and dev builds).
pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MINUTAE_DATA_DIR") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library")
            .join("Minutae")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().unwrap_or_default().join("Minutae")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::data_dir().unwrap_or_default().join("minutae")
    }
}

pub fn database_file() -> PathBuf {
    data_dir().join("minutae-v4.db")
}

pub fn recordings_dir() -> PathBuf {
    data_dir().join("recordings")
}

pub fn summaries_dir() -> PathBuf {
    data_dir().join("summaries")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

/// Ensure every directory exists.
pub fn ensure_all() -> std::io::Result<()> {
    for d in [data_dir(), recordings_dir(), summaries_dir(), models_dir()] {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}
