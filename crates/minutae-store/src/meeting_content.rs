//! Persisted cache, content deletion and live-discard recovery.

use minutae_core::MeetingId;
use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

impl Store {
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
                 VALUES (?1, ?2, ?3, ?4, ?5)",
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
        self.transaction(|tx| {
            tx.execute("DELETE FROM jobs WHERE meeting_id=?1", [&mid])?;
            tx.execute(
                "UPDATE meetings SET status='done', failure=NULL, updated_at=?2 WHERE id=?1",
                params![mid, crate::now_ms()],
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
