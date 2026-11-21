//! Meetings, tracks, moments, list/detail read models and week stats.

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use minutae_core::{
    EchoCancelReport, Failure, Meeting, MeetingId, MeetingStatus, Moment, Progress, Speaker, Stage,
    Summary, Transcript,
};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::schema::title_key;
use crate::summaries::ActionItemRow;
use crate::{
    Result, Store, StoreError, enum_from, enum_str, from_ms, json_col, ms, now_ms, parse_uuid,
};

/// One captured audio file per track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub meeting_id: MeetingId,
    pub track: minutae_core::Track,
    pub path: String,
    pub duration_ms: Option<u64>,
    pub bytes: Option<u64>,
}

/// Row for the Meetings list (Paper 01).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingListItem {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub duration_ms: Option<u64>,
    pub takeaway: Option<String>,
    /// Display names (or labels) of the meeting's speakers.
    pub people: Vec<String>,
    pub open_actions: u32,
    /// Progress of the active (running or queued) job for this meeting, if any.
    pub progress: Option<Progress>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayGroup {
    pub date: NaiveDate,
    pub items: Vec<MeetingListItem>,
}

/// Everything the Meeting detail screen (Paper 03) needs in one read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetail {
    pub meeting: Meeting,
    pub speakers: Vec<Speaker>,
    pub transcript: Option<Transcript>,
    pub summary: Option<Summary>,
    pub action_items: Vec<ActionItemRow>,
    pub moments: Vec<Moment>,
    pub tracks: Vec<Track>,
    pub progress: Option<Progress>,
    /// What echo cancellation did before the last transcription (`None`: never ran).
    pub echo_cancel: Option<EchoCancelReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekStats {
    pub meetings: u32,
    /// Sum of finished meeting durations started since `since`.
    pub recorded_ms: u64,
    /// Open action items across all meetings.
    pub open_actions: u32,
    pub dictations: u32,
    pub dictation_words: u64,
}

/// Group list rows by calendar day in a fixed UTC offset (minutes east of UTC).
/// Order is preserved, so pass rows newest first.
pub fn group_by_local_day(items: Vec<MeetingListItem>, utc_offset_minutes: i32) -> Vec<DayGroup> {
    let tz = FixedOffset::east_opt(utc_offset_minutes * 60)
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("utc"));
    let mut groups: Vec<DayGroup> = Vec::new();
    for item in items {
        let date = item.meeting.started_at.with_timezone(&tz).date_naive();
        match groups.last_mut() {
            Some(g) if g.date == date => g.items.push(item),
            _ => groups.push(DayGroup {
                date,
                items: vec![item],
            }),
        }
    }
    groups
}

const MEETING_COLS: &str =
    "m.id, m.title, m.started_at, m.ended_at, m.status, m.source_app, m.failure";

pub(crate) fn meeting_from_row(r: &Row<'_>) -> rusqlite::Result<Meeting> {
    let failure: Option<String> = r.get(6)?;
    Ok(Meeting {
        id: parse_uuid(&r.get::<_, String>(0)?)?,
        title: r.get(1)?,
        started_at: from_ms(r.get(2)?),
        ended_at: r.get::<_, Option<i64>>(3)?.map(from_ms),
        status: enum_from(&r.get::<_, String>(4)?)?,
        source_app: r.get(5)?,
        failure: failure.as_deref().map(json_col::<Failure>).transpose()?,
    })
}

pub(crate) fn kind_stage(kind: &str) -> Option<Stage> {
    match kind {
        "transcribe" => Some(Stage::Transcribing),
        "diarize" => Some(Stage::Diarizing),
        "summarize" => Some(Stage::Summarizing),
        "index" => Some(Stage::Indexing),
        _ => None,
    }
}

/// Columns 0..4: kind, stage, fraction, eta, meeting_id
fn progress_from(
    meeting_id: MeetingId,
    kind: Option<String>,
    stage: Option<String>,
    fraction: Option<f64>,
    eta: Option<u32>,
) -> rusqlite::Result<Option<Progress>> {
    let Some(kind) = kind else { return Ok(None) };
    let stage = match stage {
        Some(s) => Some(enum_from::<Stage>(&s)?),
        None => kind_stage(&kind),
    };
    Ok(stage.map(|stage| Progress {
        meeting_id: Some(meeting_id),
        stage,
        fraction: fraction.unwrap_or(0.0) as f32,
        eta_seconds: eta,
    }))
}

pub(crate) fn insert_meeting(c: &rusqlite::Connection, m: &Meeting) -> Result<()> {
    let now = now_ms();
    c.execute(
        "INSERT INTO meetings (id, title, started_at, ended_at, status, source_app, failure, created_at, updated_at, title_lc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
        params![
            m.id.to_string(),
            m.title,
            ms(m.started_at),
            m.ended_at.map(ms),
            enum_str(&m.status),
            m.source_app,
            m.failure.as_ref().map(serde_json::to_string).transpose()?,
            now,
            title_key(&m.title)
        ],
    )?;
    Ok(())
}

pub(crate) fn write_track(c: &rusqlite::Connection, t: &Track) -> Result<()> {
    c.execute(
        "INSERT INTO tracks (meeting_id, track, path, duration_ms, bytes) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(meeting_id, track) DO UPDATE SET path = excluded.path,
           duration_ms = excluded.duration_ms, bytes = excluded.bytes",
        params![t.meeting_id.to_string(), enum_str(&t.track), t.path, t.duration_ms, t.bytes],
    )?;
    Ok(())
}

const ACTIVE_JOB: &str = "(SELECT id FROM jobs WHERE meeting_id = m.id AND state IN ('running','queued') \
     ORDER BY state = 'running' DESC, created_at LIMIT 1)";

impl Store {
    pub fn create_meeting(&self, m: &Meeting) -> Result<()> {
        self.write(|c| insert_meeting(c, m))
    }

    /// Overwrite every column of an existing meeting.
    pub fn update_meeting(&self, m: &Meeting) -> Result<()> {
        self.write(|c| {
            let n = c.execute(
                "UPDATE meetings SET title = ?2, started_at = ?3, ended_at = ?4, status = ?5, source_app = ?6,
                 failure = ?7, updated_at = ?8, title_lc = ?9 WHERE id = ?1",
                params![
                    m.id.to_string(),
                    m.title,
                    ms(m.started_at),
                    m.ended_at.map(ms),
                    enum_str(&m.status),
                    m.source_app,
                    m.failure.as_ref().map(serde_json::to_string).transpose()?,
                    now_ms(),
                    title_key(&m.title)
                ],
            )?;
            expect_one(n, m.id)
        })
    }

    pub fn get_meeting(&self, id: MeetingId) -> Result<Option<Meeting>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {MEETING_COLS} FROM meetings m WHERE m.id = ?1"),
                [id.to_string()],
                meeting_from_row,
            )
            .optional()?)
        })
    }

    pub fn rename_meeting(&self, id: MeetingId, title: &str) -> Result<()> {
        self.write(|c| {
            let n = c.execute(
                "UPDATE meetings SET title = ?2, updated_at = ?3, title_lc = ?4 WHERE id = ?1",
                params![id.to_string(), title, now_ms(), title_key(title)],
            )?;
            expect_one(n, id)
        })
    }

    /// Set status; `failure` is stored when given and cleared otherwise.
    pub fn set_meeting_status(
        &self,
        id: MeetingId,
        status: MeetingStatus,
        failure: Option<&Failure>,
    ) -> Result<()> {
        let failure = failure.map(serde_json::to_string).transpose()?;
        self.write(|c| {
            let n = c.execute(
                "UPDATE meetings SET status = ?2, failure = ?3, updated_at = ?4 WHERE id = ?1",
                params![id.to_string(), enum_str(&status), failure, now_ms()],
            )?;
            expect_one(n, id)
        })
    }

    /// Mark the recording as ended and move it to `processing`.
    pub fn end_meeting(&self, id: MeetingId, ended_at: DateTime<Utc>) -> Result<()> {
        self.write(|c| {
            let n = c.execute(
                "UPDATE meetings SET ended_at = ?2, status = 'processing', updated_at = ?3 WHERE id = ?1",
                params![id.to_string(), ms(ended_at), now_ms()],
            )?;
            expect_one(n, id)
        })
    }

    /// Delete a meeting and everything that belongs to it (cascade).
    /// Audio files on disk are the caller's job (see [`Store::list_tracks`]).
    pub fn delete_meeting(&self, id: MeetingId) -> Result<bool> {
        self.write(|c| Ok(c.execute("DELETE FROM meetings WHERE id = ?1", [id.to_string()])? > 0))
    }

    /// Newest first. `limit`/`offset` page through the list.
    pub fn list_meetings(&self, limit: u32, offset: u32) -> Result<Vec<MeetingListItem>> {
        let sql = format!(
            "SELECT {MEETING_COLS},
               (SELECT takeaway FROM summaries s WHERE s.meeting_id = m.id),
               (SELECT group_concat(name, char(31)) FROM
                  (SELECT COALESCE(display_name, label) AS name FROM speakers sp WHERE sp.meeting_id = m.id ORDER BY sp.label)),
               (SELECT count(*) FROM action_items a WHERE a.meeting_id = m.id AND a.done = 0),
               j.kind, j.stage, j.fraction, j.eta_seconds
             FROM meetings m LEFT JOIN jobs j ON j.id = {ACTIVE_JOB}
             ORDER BY m.started_at DESC LIMIT ?1 OFFSET ?2"
        );
        self.read(|c| {
            let mut stmt = c.prepare_cached(&sql)?;
            let rows = stmt.query_map(params![limit, offset], |r| {
                let meeting = meeting_from_row(r)?;
                let people: Option<String> = r.get(8)?;
                let progress =
                    progress_from(meeting.id, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?)?;
                Ok(MeetingListItem {
                    duration_ms: meeting
                        .ended_at
                        .map(|e| (e - meeting.started_at).num_milliseconds().max(0) as u64),
                    takeaway: r.get(7)?,
                    people: people
                        .map(|p| p.split('\u{1f}').map(str::to_owned).collect())
                        .unwrap_or_default(),
                    open_actions: r.get(9)?,
                    progress,
                    meeting,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn count_meetings(&self) -> Result<u64> {
        self.read(|c| Ok(c.query_row("SELECT count(*) FROM meetings", [], |r| r.get(0))?))
    }

    pub fn meeting_detail(&self, id: MeetingId) -> Result<Option<MeetingDetail>> {
        let Some(meeting) = self.get_meeting(id)? else {
            return Ok(None);
        };
        let progress = self.read(|c| {
            let row = c
                .query_row(
                    &format!("SELECT j.kind, j.stage, j.fraction, j.eta_seconds FROM meetings m JOIN jobs j ON j.id = {ACTIVE_JOB} WHERE m.id = ?1"),
                    [id.to_string()],
                    |r| progress_from(id, r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?),
                )
                .optional()?;
            Ok(row.flatten())
        })?;
        Ok(Some(MeetingDetail {
            speakers: self.list_speakers(id)?,
            transcript: self.get_transcript(id)?,
            summary: self.get_summary(id)?,
            action_items: self.list_action_items(id)?,
            moments: self.list_moments(id)?,
            tracks: self.list_tracks(id)?,
            echo_cancel: self.echo_cancel(id)?,
            progress,
            meeting,
        }))
    }

    // ---- echo cancellation ----

    /// Records what the echo-cancellation step did (replacing an earlier run's report).
    pub fn set_echo_cancel(&self, id: MeetingId, report: &EchoCancelReport) -> Result<()> {
        let json = serde_json::to_string(report)?;
        self.write(|c| {
            c.execute(
                "UPDATE meetings SET echo_cancel = ?2 WHERE id = ?1",
                params![id.to_string(), json],
            )?;
            Ok(())
        })
    }

    pub fn echo_cancel(&self, id: MeetingId) -> Result<Option<EchoCancelReport>> {
        self.read(|c| {
            let v: Option<Option<String>> = c
                .query_row(
                    "SELECT echo_cancel FROM meetings WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(v.flatten()
                .as_deref()
                .map(json_col::<EchoCancelReport>)
                .transpose()?)
        })
    }

    // ---- tracks ----

    pub fn upsert_track(&self, t: &Track) -> Result<()> {
        self.write(|c| write_track(c, t))
    }

    pub fn list_tracks(&self, id: MeetingId) -> Result<Vec<Track>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached("SELECT meeting_id, track, path, duration_ms, bytes FROM tracks WHERE meeting_id = ?1 ORDER BY track")?;
            let rows = stmt.query_map([id.to_string()], |r| {
                Ok(Track {
                    meeting_id: parse_uuid(&r.get::<_, String>(0)?)?,
                    track: enum_from(&r.get::<_, String>(1)?)?,
                    path: r.get(2)?,
                    duration_ms: r.get(3)?,
                    bytes: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    // ---- moments ----

    pub fn add_moment(&self, m: &Moment) -> Result<i64> {
        self.write(|c| {
            c.execute(
                "INSERT INTO moments (meeting_id, at_ms, label, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![m.meeting_id.to_string(), m.at_ms, m.label, now_ms()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_moments(&self, id: MeetingId) -> Result<Vec<Moment>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT at_ms, label FROM moments WHERE meeting_id = ?1 ORDER BY at_ms",
            )?;
            let rows = stmt.query_map([id.to_string()], |r| {
                Ok(Moment {
                    meeting_id: id,
                    at_ms: r.get(0)?,
                    label: r.get(1)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    // ---- stats ----

    /// Stats for the header of the Meetings list. `since` is the start of the
    /// user's week (the caller knows the locale's first weekday and time zone).
    pub fn week_stats(&self, since: DateTime<Utc>) -> Result<WeekStats> {
        let since = ms(since);
        self.read(|c| {
            let (meetings, recorded): (u32, i64) = c.query_row(
                "SELECT count(*), COALESCE(SUM(CASE WHEN ended_at IS NOT NULL THEN ended_at - started_at ELSE 0 END), 0)
                 FROM meetings WHERE started_at >= ?1",
                [since],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let open_actions: u32 = c.query_row("SELECT count(*) FROM action_items WHERE done = 0", [], |r| r.get(0))?;
            let (dictations, words): (u32, i64) = c.query_row(
                "SELECT count(*), COALESCE(SUM(word_count), 0) FROM dictations WHERE created_at >= ?1",
                [since],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok(WeekStats {
                meetings,
                recorded_ms: recorded.max(0) as u64,
                open_actions,
                dictations,
                dictation_words: words.max(0) as u64,
            })
        })
    }
}

pub(crate) fn expect_one(n: usize, id: impl std::fmt::Display) -> Result<()> {
    if n == 0 {
        Err(StoreError::NotFound(id.to_string()))
    } else {
        Ok(())
    }
}
