//! Transcripts, segments and speakers.

use minutae_core::{MeetingId, Segment, Speaker, SpeakerId, Transcript};
use rusqlite::{OptionalExtension, params};

use crate::meetings::expect_one;
use crate::{Result, Store, enum_from, enum_str, now_ms, parse_uuid};

impl Store {
    /// Replace the meeting's transcript, segments and speakers in one transaction.
    /// Segments may only reference speakers in `speakers` (or none).
    pub fn replace_transcript(&self, transcript: &Transcript, speakers: &[Speaker]) -> Result<()> {
        let mid = transcript.meeting_id.to_string();
        self.transaction(|tx| {
            tx.execute("DELETE FROM segments WHERE meeting_id = ?1", [&mid])?;
            tx.execute("DELETE FROM speakers WHERE meeting_id = ?1", [&mid])?;
            tx.execute(
                "INSERT INTO transcripts (meeting_id, language, model, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(meeting_id) DO UPDATE SET language = excluded.language, model = excluded.model,
                   created_at = excluded.created_at",
                params![mid, transcript.language, transcript.model, now_ms()],
            )?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO speakers (id, meeting_id, label, display_name, track) VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                for s in speakers {
                    stmt.execute(params![s.id.to_string(), mid, s.label, s.display_name, enum_str(&s.track)])?;
                }
            }
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT INTO segments (meeting_id, start_ms, end_ms, text, track, speaker_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )?;
                for s in &transcript.segments {
                    stmt.execute(params![
                        mid,
                        s.start_ms,
                        s.end_ms,
                        s.text,
                        enum_str(&s.track),
                        s.speaker_id.map(|u| u.to_string())
                    ])?;
                }
            }
            Ok(())
        })
    }

    pub fn get_transcript(&self, id: MeetingId) -> Result<Option<Transcript>> {
        let head: Option<(Option<String>, String)> = self.read(|c| {
            Ok(c.query_row(
                "SELECT language, model FROM transcripts WHERE meeting_id = ?1",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })?;
        let Some((language, model)) = head else {
            return Ok(None);
        };
        Ok(Some(Transcript {
            meeting_id: id,
            language,
            model,
            segments: self.list_segments(id, None)?,
        }))
    }

    /// Segments in time order, optionally limited to `[from_ms, to_ms)`.
    pub fn list_segments(&self, id: MeetingId, range: Option<(u64, u64)>) -> Result<Vec<Segment>> {
        let (from, to) = range
            .map(|(a, b)| (a as i64, b as i64))
            .unwrap_or((0, i64::MAX));
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT start_ms, end_ms, text, track, speaker_id FROM segments
                 WHERE meeting_id = ?1 AND start_ms >= ?2 AND start_ms < ?3 ORDER BY start_ms, id",
            )?;
            let rows = stmt.query_map(params![id.to_string(), from, to], |r| {
                Ok(Segment {
                    start_ms: r.get(0)?,
                    end_ms: r.get(1)?,
                    text: r.get(2)?,
                    track: enum_from(&r.get::<_, String>(3)?)?,
                    speaker_id: r
                        .get::<_, Option<String>>(4)?
                        .as_deref()
                        .map(parse_uuid)
                        .transpose()?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn list_speakers(&self, id: MeetingId) -> Result<Vec<Speaker>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT id, label, display_name, track FROM speakers WHERE meeting_id = ?1 ORDER BY label",
            )?;
            let rows = stmt.query_map([id.to_string()], |r| {
                Ok(Speaker {
                    id: parse_uuid(&r.get::<_, String>(0)?)?,
                    meeting_id: id,
                    label: r.get(1)?,
                    display_name: r.get(2)?,
                    track: enum_from(&r.get::<_, String>(3)?)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Set (or clear with `None`) the name the user typed for a speaker.
    pub fn rename_speaker(&self, speaker: SpeakerId, display_name: Option<&str>) -> Result<()> {
        let name = display_name.map(str::trim).filter(|s| !s.is_empty());
        self.write(|c| {
            let n = c.execute(
                "UPDATE speakers SET display_name = ?2 WHERE id = ?1",
                params![speaker.to_string(), name],
            )?;
            expect_one(n, speaker)
        })
    }

    /// Link a meeting speaker to a stored voice profile (opt-in, DIA-02).
    pub fn link_speaker_profile(
        &self,
        speaker: SpeakerId,
        profile: Option<uuid::Uuid>,
    ) -> Result<()> {
        self.write(|c| {
            let n = c.execute(
                "UPDATE speakers SET profile_id = ?2 WHERE id = ?1",
                params![speaker.to_string(), profile.map(|p| p.to_string())],
            )?;
            expect_one(n, speaker)
        })
    }

    /// Insert or replace a voice profile (embedding stored as little-endian f32).
    pub fn upsert_speaker_profile(
        &self,
        id: uuid::Uuid,
        name: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let blob: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        let now = now_ms();
        self.write(|c| {
            c.execute(
                "INSERT INTO speaker_profiles (id, name, embedding, dim, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, embedding = excluded.embedding,
                   dim = excluded.dim, updated_at = excluded.updated_at",
                params![id.to_string(), name, blob, embedding.len() as i64, now],
            )?;
            Ok(())
        })
    }

    /// All voice profiles as `(id, name, embedding)`.
    pub fn list_speaker_profiles(&self) -> Result<Vec<(uuid::Uuid, String, Vec<f32>)>> {
        self.read(|c| {
            let mut stmt =
                c.prepare_cached("SELECT id, name, embedding FROM speaker_profiles ORDER BY name")?;
            let rows = stmt.query_map([], |r| {
                let blob: Vec<u8> = r.get(2)?;
                let emb = blob
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                Ok((parse_uuid(&r.get::<_, String>(0)?)?, r.get(1)?, emb))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}
