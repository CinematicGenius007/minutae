//! AppSettings, stored as one JSON row. Unknown/missing fields fall back to
//! defaults (`#[serde(default)]` on `AppSettings`), so older rows keep loading.

use minutae_core::AppSettings;
use rusqlite::{OptionalExtension, params};

use crate::{Result, Store, now_ms};

impl Store {
    pub fn get_settings(&self) -> Result<AppSettings> {
        let json: Option<String> = self.read(|c| {
            Ok(
                c.query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
                    .optional()?,
            )
        })?;
        match json {
            None => Ok(AppSettings::default()),
            Some(j) => match serde_json::from_str(&j) {
                Ok(s) => Ok(s),
                Err(e) => {
                    tracing::warn!(error = %e, "settings row unreadable; using defaults");
                    Ok(AppSettings::default())
                }
            },
        }
    }

    pub fn set_settings(&self, s: &AppSettings) -> Result<()> {
        let json = serde_json::to_string(s)?;
        self.write(|c| {
            c.execute(
                "INSERT INTO settings (id, json, updated_at) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
                params![json, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Read-modify-write under the writer lock; returns the new settings.
    pub fn update_settings(&self, f: impl FnOnce(&mut AppSettings)) -> Result<AppSettings> {
        self.write(|c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let json: Option<String> = tx.query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0)).optional()?;
            let mut s: AppSettings = json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
            f(&mut s);
            tx.execute(
                "INSERT INTO settings (id, json, updated_at) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
                params![serde_json::to_string(&s)?, now_ms()],
            )?;
            tx.commit()?;
            Ok(s)
        })
    }
}
