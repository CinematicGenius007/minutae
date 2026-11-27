//! Store side of importing older Minutae databases (MIG-10).
//!
//! Reading the old files lives in `minutae-import`; this module only writes v4 rows. Each
//! legacy row is written in one transaction together with its `legacy_imports` mapping, so a
//! re-run (or a crash halfway) never duplicates a meeting and never leaves half of one.

use std::collections::HashSet;

use minutae_core::{Dictation, Meeting, Speaker, Summary, Transcript};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::meetings::{Track, insert_meeting, write_track};
use crate::{Result, Store, now_ms, parse_uuid};

/// What a `legacy_imports` row points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LegacyKind {
    Meeting,
    Dictation,
}

impl LegacyKind {
    fn as_str(self) -> &'static str {
        match self {
            LegacyKind::Meeting => "meeting",
            LegacyKind::Dictation => "dictation",
        }
    }
}

/// One legacy meeting mapped to v4 rows. Every id inside must equal `meeting.id`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMeeting {
    pub meeting: Meeting,
    pub tracks: Vec<Track>,
    pub transcript: Option<Transcript>,
    pub speakers: Vec<Speaker>,
    pub summary: Option<Summary>,
    /// Legacy data with no v4 column yet, kept as JSON in `legacy_imports.extra`.
    pub extra: Option<serde_json::Value>,
}

fn mapped(
    c: &rusqlite::Connection,
    source: &str,
    kind: LegacyKind,
    legacy_id: &str,
) -> Result<Option<Uuid>> {
    let id: Option<String> = c
        .query_row(
            "SELECT v4_id FROM legacy_imports WHERE source = ?1 AND kind = ?2 AND legacy_id = ?3",
            params![source, kind.as_str(), legacy_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id.map(|s| parse_uuid(&s)).transpose()?)
}

fn map(
    c: &rusqlite::Connection,
    source: &str,
    kind: LegacyKind,
    legacy_id: &str,
    v4_id: Uuid,
    extra: Option<&serde_json::Value>,
) -> Result<()> {
    c.execute(
        "INSERT INTO legacy_imports (source, kind, legacy_id, v4_id, imported_at, extra) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            source,
            kind.as_str(),
            legacy_id,
            v4_id.to_string(),
            now_ms(),
            extra.map(serde_json::to_string).transpose()?
        ],
    )?;
    Ok(())
}

impl Store {
    /// The v4 id a legacy row was imported as, if it was.
    pub fn legacy_mapping(
        &self,
        source: &str,
        kind: LegacyKind,
        legacy_id: &str,
    ) -> Result<Option<Uuid>> {
        self.read(|c| mapped(c, source, kind, legacy_id))
    }

    /// Every legacy id of `kind` already imported from `source`.
    pub fn legacy_imported_ids(&self, source: &str, kind: LegacyKind) -> Result<HashSet<String>> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT legacy_id FROM legacy_imports WHERE source = ?1 AND kind = ?2")?;
            let rows = stmt.query_map(params![source, kind.as_str()], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// True when a meeting or dictation row (or any mapping) already uses `id`.
    pub fn id_in_use(&self, id: Uuid) -> Result<bool> {
        let id = id.to_string();
        self.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?1)
                     OR EXISTS(SELECT 1 FROM dictations WHERE id = ?1)
                     OR EXISTS(SELECT 1 FROM legacy_imports WHERE v4_id = ?1)
                     OR EXISTS(SELECT 1 FROM pending_file_cleanup WHERE meeting_id = ?1)
                     OR EXISTS(SELECT 1 FROM discarded_recordings WHERE meeting_id = ?1)",
                [&id],
                |r| r.get(0),
            )?)
        })
    }

    /// Write one imported meeting and its mapping atomically. Returns `false` (and writes
    /// nothing) when `legacy_id` from `source` was already imported.
    pub fn import_legacy_meeting(
        &self,
        source: &str,
        legacy_id: &str,
        m: &ImportedMeeting,
    ) -> Result<bool> {
        let id = m.meeting.id;
        let ids_match = m.tracks.iter().all(|t| t.meeting_id == id)
            && m.speakers.iter().all(|s| s.meeting_id == id)
            && m.transcript.as_ref().is_none_or(|t| t.meeting_id == id)
            && m.summary.as_ref().is_none_or(|s| s.meeting_id == id);
        if !ids_match {
            return Err(crate::StoreError::Invalid(
                "imported rows must all belong to the same meeting".into(),
            ));
        }
        self.transaction(|tx| {
            if mapped(tx, source, LegacyKind::Meeting, legacy_id)?.is_some() {
                return Ok(false);
            }
            insert_meeting(tx, &m.meeting)?;
            for t in &m.tracks {
                write_track(tx, t)?;
            }
            if let Some(t) = &m.transcript {
                crate::transcripts::write_transcript(tx, t, &m.speakers)?;
            }
            if let Some(s) = &m.summary {
                crate::summaries::write_summary(tx, s)?;
                crate::summaries::curate_completed(tx, id)?;
            }
            map(
                tx,
                source,
                LegacyKind::Meeting,
                legacy_id,
                id,
                m.extra.as_ref(),
            )?;
            Ok(true)
        })
    }

    /// Write one imported dictation and its mapping atomically. `false` when already imported.
    pub fn import_legacy_dictation(
        &self,
        source: &str,
        legacy_id: &str,
        d: &Dictation,
        extra: Option<&serde_json::Value>,
    ) -> Result<bool> {
        self.transaction(|tx| {
            if mapped(tx, source, LegacyKind::Dictation, legacy_id)?.is_some() {
                return Ok(false);
            }
            crate::dictations::write_dictation(tx, d)?;
            map(tx, source, LegacyKind::Dictation, legacy_id, d.id, extra)?;
            Ok(true)
        })
    }

    /// Remember the latest import run's report for `source` (the `imports` table).
    pub fn record_import_run(&self, source: &str, report: &serde_json::Value) -> Result<()> {
        let json = serde_json::to_string(report)?;
        self.write(|c| {
            c.execute(
                "INSERT OR REPLACE INTO imports (source, imported_at, report) VALUES (?1, ?2, ?3)",
                params![source, now_ms(), json],
            )?;
            Ok(())
        })
    }

    /// The latest recorded import report for `source`.
    pub fn last_import_run(&self, source: &str) -> Result<Option<serde_json::Value>> {
        let json: Option<String> = self.read(|c| {
            Ok(c.query_row(
                "SELECT report FROM imports WHERE source = ?1",
                [source],
                |r| r.get(0),
            )
            .optional()?)
        })?;
        Ok(json.map(|s| serde_json::from_str(&s)).transpose()?)
    }
}
