//! Dictations (Paper 04).

use chrono::{DateTime, Utc};
use minutae_core::{Dictation, DictationId};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Result, Store, from_ms, ms, parse_uuid};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStats {
    pub count: u32,
    pub words: u64,
    pub duration_ms: u64,
}

const COLS: &str = "id, created_at, text, raw_text, target_app, word_count, duration_ms, inserted";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Dictation> {
    Ok(Dictation {
        id: parse_uuid(&r.get::<_, String>(0)?)?,
        created_at: from_ms(r.get(1)?),
        text: r.get(2)?,
        raw_text: r.get(3)?,
        target_app: r.get(4)?,
        word_count: r.get(5)?,
        duration_ms: r.get(6)?,
        inserted: r.get(7)?,
    })
}

pub(crate) fn write_dictation(c: &rusqlite::Connection, d: &Dictation) -> Result<()> {
    c.execute(
        &format!("INSERT INTO dictations ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
        params![
            d.id.to_string(),
            ms(d.created_at),
            d.text,
            d.raw_text,
            d.target_app,
            d.word_count,
            d.duration_ms,
            d.inserted
        ],
    )?;
    Ok(())
}

impl Store {
    pub fn insert_dictation(&self, d: &Dictation) -> Result<()> {
        self.write(|c| write_dictation(c, d))
    }

    pub fn get_dictation(&self, id: DictationId) -> Result<Option<Dictation>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM dictations WHERE id = ?1"),
                [id.to_string()],
                row,
            )
            .optional()?)
        })
    }

    /// Newest first.
    pub fn list_dictations(&self, limit: u32, offset: u32) -> Result<Vec<Dictation>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {COLS} FROM dictations ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
            ))?;
            let rows = stmt.query_map(params![limit, offset], row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// IDs eligible for history cleanup, without loading their transcripts.
    pub fn dictation_ids_before(&self, cutoff: Option<DateTime<Utc>>) -> Result<Vec<DictationId>> {
        self.read(|c| {
            let mut stmt =
                c.prepare_cached("SELECT id FROM dictations WHERE ?1 IS NULL OR created_at < ?1")?;
            let rows = stmt.query_map([cutoff.map(ms)], |r| parse_uuid(&r.get::<_, String>(0)?))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn delete_dictation(&self, id: DictationId) -> Result<bool> {
        self.write(|c| Ok(c.execute("DELETE FROM dictations WHERE id = ?1", [id.to_string()])? > 0))
    }

    /// Totals since `since` (e.g. "this week" on the Dictate page).
    pub fn dictation_stats(&self, since: DateTime<Utc>) -> Result<DictationStats> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT count(*), COALESCE(SUM(word_count), 0), COALESCE(SUM(duration_ms), 0) FROM dictations WHERE created_at >= ?1",
                [ms(since)],
                |r| Ok(DictationStats { count: r.get(0)?, words: r.get::<_, i64>(1)? as u64, duration_ms: r.get::<_, i64>(2)? as u64 }),
            )?)
        })
    }
}
