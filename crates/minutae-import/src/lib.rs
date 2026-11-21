//! minutae-import: bring meetings and dictations from the two older Minutae apps into v4 (MIG-10).
//!
//! Why a crate of its own: it is the only code that knows the old file layouts and copies
//! audio, and it runs once per install. `minutae-store` keeps owning every v4 write (one
//! transaction per meeting plus its `legacy_imports` mapping), so the store stays free of
//! legacy formats and the desktop app only wires a command to [`scan`] and [`run`].
//!
//! Sources (found from the old apps' code, not from a user's disk):
//! - **Swift app** (`Packages/MinutaeCore/Database.swift`, GRDB): `~/Library/Minutae/minutae.db`,
//!   recordings `~/Library/Minutae/recordings/<meeting-id>.wav` (32-bit float, 16 kHz, mono).
//!   macOS only.
//! - **Tauri v1 app** (`legacy/src-tauri`, tauri-plugin-sql): `<app_config_dir>/minutae.db` for
//!   identifier `com.nebula.minutae` — macOS `~/Library/Application Support/com.nebula.minutae/`,
//!   Windows `%APPDATA%\com.nebula.minutae\`, Linux `$XDG_CONFIG_HOME/com.nebula.minutae/`.
//!   Recordings `<app_data_dir>/recordings/<date>-<meeting-id>.wav` (absolute path stored).
//!
//! `MINUTAE_LEGACY_SWIFT_DB` / `MINUTAE_LEGACY_TAURI_DB` override the paths (dev, tests).
//!
//! Audio is copied (never moved) to `<recordings>/<v4-id>/mic.wav`, so the old apps keep
//! working; the cost is a second copy of every recording until the user deletes the old one.

mod legacy;
mod map;
mod markdown;
mod wav;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;

use minutae_store::{LegacyKind, Store};
use serde::{Deserialize, Serialize};

pub use legacy::LegacyDb;
pub use markdown::{ParsedSummary, parse as parse_summary_markdown};
pub use wav::duration_ms as wav_duration_ms;

/// Which older app a database came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Swift,
    TauriV1,
}

impl SourceKind {
    /// Stable key stored in `legacy_imports.source`. Never change.
    pub fn key(self) -> &'static str {
        match self {
            SourceKind::Swift => "swift",
            SourceKind::TauriV1 => "tauri-v1",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SourceKind::Swift => "Minutae for Mac",
            SourceKind::TauriV1 => "Minutae 1",
        }
    }
}

/// One legacy database to look at, plus folders where its audio may live if the stored
/// absolute path no longer resolves (e.g. the home folder was renamed).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub kind: SourceKind,
    pub db: PathBuf,
    pub audio_dirs: Vec<PathBuf>,
}

const TAURI_ID: &str = "com.nebula.minutae";

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The Swift app's database and recordings folder (macOS only).
pub fn swift_source() -> Option<Source> {
    let db = env_path("MINUTAE_LEGACY_SWIFT_DB").or_else(|| {
        if cfg!(target_os = "macos") {
            dirs::home_dir().map(|h| h.join("Library").join("Minutae").join("minutae.db"))
        } else {
            None
        }
    })?;
    let audio_dirs = db
        .parent()
        .map(|p| vec![p.join("recordings")])
        .unwrap_or_default();
    Some(Source {
        kind: SourceKind::Swift,
        db,
        audio_dirs,
    })
}

/// The Tauri v1 app's database (tauri-plugin-sql keeps it in `app_config_dir`) and its
/// recordings folder (`app_data_dir/recordings`).
pub fn tauri_v1_source() -> Option<Source> {
    let db = env_path("MINUTAE_LEGACY_TAURI_DB")
        .or_else(|| dirs::config_dir().map(|d| d.join(TAURI_ID).join("minutae.db")))?;
    let mut audio_dirs = vec![];
    if let Some(p) = db.parent() {
        audio_dirs.push(p.join("recordings"));
    }
    if let Some(d) = dirs::data_dir().map(|d| d.join(TAURI_ID).join("recordings"))
        && !audio_dirs.contains(&d)
    {
        audio_dirs.push(d);
    }
    Some(Source {
        kind: SourceKind::TauriV1,
        db,
        audio_dirs,
    })
}

/// Default sources whose database file exists.
pub fn detect() -> Vec<Source> {
    [swift_source(), tauri_v1_source()]
        .into_iter()
        .flatten()
        .filter(|s| s.db.is_file())
        .collect()
}

/// What a source holds, for the Settings screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScan {
    pub kind: SourceKind,
    pub name: String,
    pub path: String,
    pub meetings: u32,
    pub dictations: u32,
    /// Meetings and dictations from this source already in v4.
    pub already_imported: u32,
    /// Audio that a run would copy (not-yet-imported meetings whose file exists).
    pub audio_bytes: u64,
    /// Plain sentence when the database can't be read.
    pub error: Option<String>,
}

/// Emitted once per item while a run works.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub source: SourceKind,
    pub done: u32,
    pub total: u32,
    pub title: Option<String>,
}

/// A row that failed, or imported with something missing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub source: SourceKind,
    /// Empty when the whole source failed.
    pub legacy_id: String,
    pub title: Option<String>,
    pub reason: String,
    /// `false`: imported anyway (e.g. without its audio).
    pub failed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: u32,
    pub dictations: u32,
    /// Already imported earlier, or nothing to import (e.g. a cancelled dictation).
    pub skipped: u32,
    pub failed: u32,
    pub copied_bytes: u64,
    pub issues: Vec<Issue>,
}

impl ImportResult {
    fn absorb(&mut self, o: ImportResult) {
        self.imported += o.imported;
        self.dictations += o.dictations;
        self.skipped += o.skipped;
        self.failed += o.failed;
        self.copied_bytes += o.copied_bytes;
        self.issues.extend(o.issues);
    }
}

/// Count what each source holds and how much is already imported. Never writes.
pub fn scan(store: &Store, sources: &[Source]) -> Vec<SourceScan> {
    sources.iter().map(|s| scan_one(store, s)).collect()
}

fn scan_one(store: &Store, src: &Source) -> SourceScan {
    let mut out = SourceScan {
        kind: src.kind,
        name: src.kind.name().into(),
        path: src.db.to_string_lossy().into_owned(),
        meetings: 0,
        dictations: 0,
        already_imported: 0,
        audio_bytes: 0,
        error: None,
    };
    let counted = (|| -> Result<(), String> {
        let db = LegacyDb::open(&src.db)?;
        let meetings = db.meetings().map_err(read_err)?;
        let dictations: Vec<_> = db
            .dictations()
            .map_err(read_err)?
            .into_iter()
            .filter(map::dictation_importable)
            .collect();
        let done_m = store
            .legacy_imported_ids(src.kind.key(), LegacyKind::Meeting)
            .map_err(|e| e.to_string())?;
        let done_d = store
            .legacy_imported_ids(src.kind.key(), LegacyKind::Dictation)
            .map_err(|e| e.to_string())?;
        out.meetings = meetings.len() as u32;
        out.dictations = dictations.len() as u32;
        out.already_imported = meetings.iter().filter(|m| done_m.contains(&m.id)).count() as u32
            + dictations.iter().filter(|d| done_d.contains(&d.id)).count() as u32;
        for m in meetings.iter().filter(|m| !done_m.contains(&m.id)) {
            if let Ok(Some(rec)) = db.recording(&m.id)
                && let Some(p) = map::resolve_audio(&rec.file_path, &src.audio_dirs)
            {
                out.audio_bytes += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            }
        }
        Ok(())
    })();
    if let Err(e) = counted {
        out.error = Some(e);
    }
    out
}

fn read_err(e: minutae_store::rusqlite::Error) -> String {
    tracing::warn!(error = %e, "reading a legacy database failed");
    "This file isn't a Minutae database, or it is damaged.".into()
}

/// Where imported audio goes and how progress is reported.
pub struct RunOptions<'a> {
    /// v4 recordings root (`settings.recordingsPath` or the default).
    pub recordings_root: PathBuf,
    pub progress: &'a mut dyn FnMut(&Progress),
}

/// Import every source. Safe to re-run: rows already mapped in `legacy_imports` are skipped.
pub fn run(store: &Store, sources: &[Source], opts: RunOptions<'_>) -> ImportResult {
    let RunOptions {
        recordings_root,
        progress,
    } = opts;
    let mut result = ImportResult::default();
    // Open everything first so progress has a real total.
    let mut opened = vec![];
    for src in sources {
        match LegacyDb::open(&src.db).and_then(|db| {
            let meetings = db.meetings().map_err(read_err)?;
            let dictations = db.dictations().map_err(read_err)?;
            Ok((db, meetings, dictations))
        }) {
            Ok(x) => opened.push((src, x)),
            Err(reason) => {
                result.failed += 1;
                result.issues.push(Issue {
                    source: src.kind,
                    legacy_id: String::new(),
                    title: None,
                    reason,
                    failed: true,
                });
            }
        }
    }
    let total: u32 = opened
        .iter()
        .map(|(_, (_, m, d))| (m.len() + d.len()) as u32)
        .sum();
    let mut done = 0u32;
    for (src, (db, meetings, dictations)) in &opened {
        let key = src.kind.key();
        let mut part = ImportResult::default();
        let done_m: HashSet<String> = store
            .legacy_imported_ids(key, LegacyKind::Meeting)
            .unwrap_or_default();
        for m in meetings {
            if done_m.contains(&m.id) {
                part.skipped += 1;
            } else {
                map::import_meeting(store, db, src, m, &recordings_root, &mut part);
            }
            done += 1;
            progress(&Progress {
                source: src.kind,
                done,
                total,
                title: m.title.clone(),
            });
        }
        let done_d: HashSet<String> = store
            .legacy_imported_ids(key, LegacyKind::Dictation)
            .unwrap_or_default();
        for d in dictations {
            if done_d.contains(&d.id) || !map::dictation_importable(d) {
                part.skipped += 1;
            } else {
                map::import_dictation(store, db, src, d, &mut part);
            }
            done += 1;
            progress(&Progress {
                source: src.kind,
                done,
                total,
                title: None,
            });
        }
        if let Ok(json) = serde_json::to_value(&part) {
            let _ = store.record_import_run(key, &json);
        }
        result.absorb(part);
    }
    result
}
