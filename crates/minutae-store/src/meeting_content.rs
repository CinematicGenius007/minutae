//! Persisted cache, content deletion and live-discard recovery.

use minutae_core::{Failure, FailureKind, MeetingId};
use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

/// A file or folder to revisit once its storage becomes available again.
#[derive(Debug, PartialEq, Eq)]
pub struct PendingFileCleanup {
    pub meeting_id: MeetingId,
    pub kind: String,
    pub path: String,
    pub imported_name: Option<String>,
}

impl Store {
    pub fn meeting_storage_state(&self, id: MeetingId) -> Result<(bool, bool)> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT audio_removed, markdown_removed FROM meetings WHERE id=?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
    }

    pub fn jobs_retired(&self, id: MeetingId) -> Result<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT jobs_retired FROM meetings WHERE id=?1",
                [id.to_string()],
                |r| r.get(0),
            )?)
        })
    }

    pub fn meeting_ids(&self) -> Result<Vec<MeetingId>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT id FROM meetings")?;
            Ok(stmt
                .query_map([], |r| crate::parse_uuid(&r.get::<_, String>(0)?))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn is_imported_meeting(&self, id: MeetingId) -> Result<bool> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT EXISTS(SELECT 1 FROM legacy_imports WHERE kind='meeting' AND v4_id=?1)",
                [id.to_string()],
                |r| r.get(0),
            )?)
        })
    }

    pub fn remember_storage_folder(&self, kind: &str, path: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT OR IGNORE INTO storage_folders VALUES (?1, ?2)",
                params![kind, path],
            )?;
            Ok(())
        })
    }

    pub fn storage_folders(&self, kind: &str) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT path FROM storage_folders WHERE kind=?1")?;
            Ok(stmt
                .query_map([kind], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// A known folder was offline while legacy exports had no tracked paths.
    pub fn defer_summary_scan(&self, folder: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT OR IGNORE INTO pending_summary_scans (meeting_id, folder)
                 SELECT m.id, ?1 FROM meetings m JOIN summaries s ON s.meeting_id=m.id
                 WHERE m.markdown_file IS NULL AND m.markdown_removed=0",
                [folder],
            )?;
            Ok(())
        })
    }

    pub fn pending_summary_scans(&self, id: MeetingId) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT folder FROM pending_summary_scans WHERE meeting_id=?1")?;
            Ok(stmt
                .query_map([id.to_string()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn finish_summary_scan(&self, id: MeetingId, folder: &str) -> Result<()> {
        self.write(|c| {
            c.execute(
                "DELETE FROM pending_summary_scans WHERE meeting_id=?1 AND folder=?2",
                params![id.to_string(), folder],
            )?;
            Ok(())
        })
    }

    pub fn remember_meeting_export(&self, id: MeetingId, path: &str) -> Result<()> {
        let mid = id.to_string();
        self.transaction(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO meeting_exports (meeting_id, path)
                 SELECT id, ?2 FROM meetings WHERE id=?1",
                params![mid, path],
            )?;
            tx.execute(
                "UPDATE meetings SET markdown_file=COALESCE(markdown_file, ?2), markdown_removed=0 WHERE id=?1",
                params![mid, path],
            )?;
            Ok(())
        })
    }

    pub fn meeting_exports(&self, id: MeetingId) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT path FROM meeting_exports WHERE meeting_id=?1
                 UNION SELECT markdown_file FROM meetings WHERE id=?1 AND markdown_file IS NOT NULL",
            )?;
            Ok(stmt.query_map([id.to_string()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn forget_meeting_export(&self, id: MeetingId, path: &str) -> Result<()> {
        let mid = id.to_string();
        self.transaction(|tx| {
            tx.execute(
                "DELETE FROM meeting_exports WHERE meeting_id=?1 AND path=?2",
                params![mid, path],
            )?;
            tx.execute(
                "UPDATE meetings SET markdown_file=NULL WHERE id=?1 AND markdown_file=?2",
                params![mid, path],
            )?;
            Ok(())
        })
    }

    pub fn mark_exports_removed(&self, id: MeetingId) -> Result<()> {
        self.write(|c| {
            c.execute(
                "UPDATE meetings SET markdown_file=NULL, markdown_removed=1 WHERE id=?1",
                [id.to_string()],
            )?;
            Ok(())
        })
    }

    pub fn pending_file_cleanups(&self) -> Result<Vec<PendingFileCleanup>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT meeting_id, kind, path, imported_name FROM pending_file_cleanup",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    Ok(PendingFileCleanup {
                        meeting_id: crate::parse_uuid(&r.get::<_, String>(0)?)?,
                        kind: r.get(1)?,
                        path: r.get(2)?,
                        imported_name: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn forget_file_cleanup(&self, item: &PendingFileCleanup) -> Result<()> {
        self.write(|c| {
            c.execute(
                "DELETE FROM pending_file_cleanup WHERE meeting_id=?1 AND kind=?2 AND path=?3",
                params![item.meeting_id.to_string(), item.kind, item.path],
            )?;
            Ok(())
        })
    }

    pub fn erase_meeting_records(
        &self,
        id: MeetingId,
        pending: &[PendingFileCleanup],
    ) -> Result<()> {
        let mid = id.to_string();
        self.transaction(|tx| {
            for item in pending {
                tx.execute(
                    "INSERT OR REPLACE INTO pending_file_cleanup (meeting_id, kind, path, imported_name)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![mid, item.kind, item.path, item.imported_name],
                )?;
            }
            tx.execute("DELETE FROM meetings WHERE id=?1", [&mid])?;
            Ok(())
        })
    }

    pub fn meeting_markdown_file(&self, id: MeetingId) -> Result<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT markdown_file FROM meetings WHERE id=?1",
                [id.to_string()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
        })
    }

    pub fn waveform(
        &self,
        id: MeetingId,
        track: &str,
        fingerprint: &str,
        request: &str,
    ) -> Result<Option<Vec<f32>>> {
        self.read(|c| {
            let levels: Option<String> = c
                .query_row(
                    "SELECT levels FROM meeting_waveforms
                 WHERE meeting_id=?1 AND track=?2 AND fingerprint=?3 AND request=?4",
                    params![id.to_string(), track, fingerprint, request],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(levels.map(|json| serde_json::from_str(&json)).transpose()?)
        })
    }

    pub fn cache_waveform(
        &self,
        id: MeetingId,
        track: &str,
        fingerprint: &str,
        request: &str,
        levels: &[f32],
    ) -> Result<()> {
        let mid = id.to_string();
        let levels = serde_json::to_string(levels)?;
        self.transaction(|tx| {
            tx.execute(
                "INSERT OR REPLACE INTO meeting_waveforms (meeting_id, track, fingerprint, request, levels)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (
                    SELECT 1 FROM tracks WHERE meeting_id=?1 AND track=?2 AND deleted=0
                 )",
                params![mid, track, fingerprint, request, levels],
            )?;
            tx.execute(
                "DELETE FROM meeting_waveforms WHERE meeting_id=?1 AND track=?2
                 AND (fingerprint != ?3 OR rowid NOT IN (
                    SELECT rowid FROM meeting_waveforms WHERE meeting_id=?1 AND track=?2
                    ORDER BY rowid DESC LIMIT 12
                 ))",
                params![mid, track, fingerprint],
            )?;
            Ok(())
        })
    }

    /// Drop pending work before changing its inputs, and prevent recovery from
    /// automatically running a new pipeline for a meeting deliberately trimmed.
    pub fn settle_meeting_for_deletion(&self, id: MeetingId) -> Result<()> {
        let mid = id.to_string();
        let stopped = serde_json::to_string(&Failure {
            kind: FailureKind::Cancelled,
            message: "Processing stopped when meeting content was removed.".into(),
            fix: None,
        })?;
        self.transaction(|tx| {
            tx.execute("DELETE FROM jobs WHERE meeting_id=?1", [&mid])?;
            tx.execute(
                "UPDATE meetings SET jobs_retired=1, updated_at=?2,
                 failure=CASE WHEN status='processing' THEN ?3 ELSE failure END,
                 status=CASE WHEN status='processing' THEN 'failed' ELSE status END WHERE id=?1",
                params![mid, crate::now_ms(), stopped],
            )?;
            Ok(())
        })
    }

    pub fn trim_meeting(
        &self,
        id: MeetingId,
        audio: bool,
        transcript: bool,
        summary: bool,
    ) -> Result<()> {
        let mid = id.to_string();
        self.transaction(|tx| {
            // Keep track rows as provenance for the UI's deleted-track state.
            if audio {
                tx.execute("DELETE FROM meeting_waveforms WHERE meeting_id=?1", [&mid])?;
                tx.execute("UPDATE tracks SET deleted=1 WHERE meeting_id=?1", [&mid])?;
                tx.execute("UPDATE meetings SET audio_removed=1 WHERE id=?1", [&mid])?;
            }
            if transcript {
                tx.execute("DELETE FROM segments WHERE meeting_id=?1", [&mid])?;
                tx.execute("DELETE FROM transcripts WHERE meeting_id=?1", [&mid])?;
                tx.execute("DELETE FROM speakers WHERE meeting_id=?1", [&mid])?;
            }
            if summary {
                tx.execute("DELETE FROM action_items WHERE meeting_id=?1", [&mid])?;
                tx.execute("DELETE FROM moments WHERE meeting_id=?1", [&mid])?;
                tx.execute("DELETE FROM summaries WHERE meeting_id=?1", [&mid])?;
                tx.execute(
                    "UPDATE meetings SET checklist_curated=0 WHERE id=?1",
                    [&mid],
                )?;
            }
            Ok(())
        })
    }

    pub fn begin_recording_discard(&self, id: MeetingId, directory: &str) -> Result<()> {
        let mid = id.to_string();
        self.transaction(|tx| {
            crate::meetings::expect_one(
                tx.execute(
                    "DELETE FROM meetings WHERE id=?1 AND status='recording'",
                    [&mid],
                )?,
                &mid,
            )?;
            tx.execute(
                "INSERT INTO discarded_recordings (meeting_id, directory) VALUES (?1, ?2)",
                params![mid, directory],
            )?;
            Ok(())
        })
    }

    pub fn pending_recording_discards(&self) -> Result<Vec<(MeetingId, String)>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT meeting_id, directory FROM discarded_recordings")?;
            Ok(stmt
                .query_map([], |r| {
                    Ok((crate::parse_uuid(&r.get::<_, String>(0)?)?, r.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    pub fn finish_recording_discard(&self, id: MeetingId) -> Result<()> {
        self.write(|c| {
            c.execute(
                "DELETE FROM discarded_recordings WHERE meeting_id=?1",
                [id.to_string()],
            )?;
            Ok(())
        })
    }
}
