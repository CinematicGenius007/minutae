//! Hook for importing older databases (MIG-10): the Tauri/Drizzle `minutae.db`
//! and the Swift/GRDB database. Importers are implemented later; the store only
//! guarantees each source is imported once (recorded in the `imports` table).

use serde::{Deserialize, Serialize};

use crate::{Result, Store, now_ms};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub meetings: u32,
    pub dictations: u32,
    pub skipped: u32,
    pub notes: Vec<String>,
}

/// One legacy source. `source_key` must be stable (e.g. "legacy-drizzle:/path/minutae.db").
pub trait Importer {
    fn source_key(&self) -> String;
    /// Read the old database (read-only) and write rows through `store`'s repository methods.
    fn import(&self, store: &Store) -> Result<ImportReport>;
}

impl Store {
    pub fn was_imported(&self, source_key: &str) -> Result<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM imports WHERE source = ?1",
                [source_key],
                |r| r.get::<_, i64>(0),
            )? > 0)
        })
    }

    /// Run `importer` unless its source was already imported. Returns `None` when skipped.
    pub fn import_once(&self, importer: &dyn Importer) -> Result<Option<ImportReport>> {
        let key = importer.source_key();
        if self.was_imported(&key)? {
            return Ok(None);
        }
        let report = importer.import(self)?;
        let json = serde_json::to_string(&report)?;
        self.write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO imports (source, imported_at, report) VALUES (?1, ?2, ?3)",
                rusqlite::params![key, now_ms(), json],
            )?;
            Ok(())
        })?;
        Ok(Some(report))
    }
}
