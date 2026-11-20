//! Read-only access to an older Minutae database.
//!
//! Both older apps share one core schema (the Swift app's `v1.sql` is the Tauri v1
//! `0001_init.sql` without the FTS triggers): `meetings`, `recordings`, `transcripts`,
//! `segments`, `summaries`, `action_items`. The Swift app adds `dictations`, `tags`,
//! `item_tags` and `transcript_edits`. Every query checks which tables and columns exist, so
//! an older or partly migrated file still reads; values are read loosely (text or number).
//!
//! The file is never opened in place: the database and its `-wal` are copied into a private
//! temp dir first. That keeps the legacy folder untouched (no `-shm`, no checkpoint) even when
//! the old app is still running.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use minutae_store::rusqlite::{self, Connection, OpenFlags, types::Value};

#[derive(Debug, Clone, PartialEq)]
pub struct LMeeting {
    pub id: String,
    pub title: Option<String>,
    pub started_at: Option<Value>,
    pub ended_at: Option<Value>,
    pub duration_seconds: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LRecording {
    pub file_path: String,
    pub created_at: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LTranscript {
    pub id: String,
    pub raw_text: String,
    pub provider: Option<String>,
    pub language: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LSegment {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub speaker_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LSummary {
    pub markdown: String,
    pub model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LDictation {
    pub id: String,
    pub started_at: Option<Value>,
    pub created_at: Option<Value>,
    pub duration_ms: Option<i64>,
    pub target_app: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub edited_text: Option<String>,
    pub insert_method: Option<String>,
    pub status: Option<String>,
}

pub struct LegacyDb {
    conn: Connection,
    tables: HashSet<String>,
    columns: HashMap<String, HashSet<String>>,
    _snapshot: tempfile::TempDir,
}

pub fn text(v: Value) -> Option<String> {
    match v {
        Value::Text(s) => Some(s),
        Value::Integer(i) => Some(i.to_string()),
        Value::Real(f) => Some(f.to_string()),
        Value::Blob(b) => String::from_utf8(b).ok(),
        Value::Null => None,
    }
}

pub fn int(v: Value) -> Option<i64> {
    match v {
        Value::Integer(i) => Some(i),
        Value::Real(f) => Some(f as i64),
        Value::Text(s) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

fn opt_text(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<String>> {
    Ok(text(r.get::<_, Value>(i)?).filter(|s| !s.trim().is_empty()))
}

fn opt_value(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<Option<Value>> {
    Ok(match r.get::<_, Value>(i)? {
        Value::Null => None,
        v => Some(v),
    })
}

impl LegacyDb {
    /// Snapshot `path` (and its `-wal`) and open the copy. Fails with a plain sentence when the
    /// file is missing, unreadable or not a SQLite database.
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err("The database file is missing.".into());
        }
        let snapshot = tempfile::Builder::new()
            .prefix("minutae-legacy-")
            .tempdir()
            .map_err(|e| format!("Couldn't make a temporary copy: {e}"))?;
        let copy = snapshot.path().join("legacy.db");
        std::fs::copy(path, &copy).map_err(|e| format!("Couldn't read the database: {e}"))?;
        let wal = PathBuf::from(format!("{}-wal", path.display()));
        if wal.is_file() {
            let _ = std::fs::copy(&wal, snapshot.path().join("legacy.db-wal"));
        }
        let conn = Connection::open_with_flags(
            &copy,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| "The database couldn't be opened.".to_string())?;
        let tables: HashSet<String> = (|| -> rusqlite::Result<HashSet<String>> {
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect()
        })()
        .map_err(|_| "This file isn't a Minutae database, or it is damaged.".to_string())?;
        if !tables.contains("meetings") {
            return Err("This file isn't a Minutae database (no meetings table).".into());
        }
        let mut columns = HashMap::new();
        for t in &tables {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info(\"{}\")", t.replace('"', "")))
                .map_err(|e| e.to_string())?;
            let cols: HashSet<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .and_then(|rows| rows.collect())
                .map_err(|e| e.to_string())?;
            columns.insert(t.clone(), cols);
        }
        Ok(Self {
            conn,
            tables,
            columns,
            _snapshot: snapshot,
        })
    }

    pub fn has_table(&self, t: &str) -> bool {
        self.tables.contains(t)
    }

    fn has(&self, table: &str, col: &str) -> bool {
        self.columns.get(table).is_some_and(|c| c.contains(col))
    }

    /// `col` when the column exists, else `NULL` (so one SELECT works on every schema version).
    fn c(&self, table: &str, col: &str) -> String {
        if self.has(table, col) {
            format!("\"{col}\"")
        } else {
            "NULL".into()
        }
    }

    pub fn meetings(&self) -> rusqlite::Result<Vec<LMeeting>> {
        let sql = format!(
            "SELECT id, {}, {}, {}, {}, {} FROM meetings ORDER BY {} ",
            self.c("meetings", "title"),
            self.c("meetings", "started_at"),
            self.c("meetings", "ended_at"),
            self.c("meetings", "duration_seconds"),
            self.c("meetings", "status"),
            if self.has("meetings", "started_at") {
                "started_at"
            } else {
                "rowid"
            },
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(LMeeting {
                id: text(r.get(0)?).unwrap_or_default(),
                title: opt_text(r, 1)?,
                started_at: opt_value(r, 2)?,
                ended_at: opt_value(r, 3)?,
                duration_seconds: int(r.get(4)?),
                status: opt_text(r, 5)?,
            })
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|m| !m.id.is_empty())
            .collect())
    }

    /// Newest recording row for the meeting.
    pub fn recording(&self, meeting_id: &str) -> rusqlite::Result<Option<LRecording>> {
        if !self.has_table("recordings") || !self.has("recordings", "file_path") {
            return Ok(None);
        }
        let sql = format!(
            "SELECT file_path, {} FROM recordings WHERE meeting_id = ?1 ORDER BY {} DESC, rowid DESC",
            self.c("recordings", "created_at"),
            self.c("recordings", "created_at"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map([meeting_id], |r| {
            Ok(LRecording {
                file_path: text(r.get(0)?).unwrap_or_default(),
                created_at: opt_value(r, 1)?,
            })
        })?;
        Ok(rows.next().transpose()?.filter(|r| !r.file_path.is_empty()))
    }

    /// The meeting's transcript: the newest one that has segments or text.
    pub fn transcript(&self, meeting_id: &str) -> rusqlite::Result<Option<LTranscript>> {
        if !self.has_table("transcripts") {
            return Ok(None);
        }
        let sql = format!(
            "SELECT id, {}, {}, {}, {} FROM transcripts WHERE meeting_id = ?1 ORDER BY {} DESC, rowid DESC",
            self.c("transcripts", "raw_text"),
            self.c("transcripts", "provider"),
            self.c("transcripts", "language"),
            self.c("transcripts", "duration_ms"),
            self.c("transcripts", "created_at"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let all = stmt
            .query_map([meeting_id], |r| {
                Ok(LTranscript {
                    id: text(r.get(0)?).unwrap_or_default(),
                    raw_text: text(r.get(1)?).unwrap_or_default(),
                    provider: opt_text(r, 2)?,
                    language: opt_text(r, 3)?,
                    duration_ms: int(r.get(4)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for t in &all {
            if !t.raw_text.trim().is_empty() || self.segment_count(&t.id)? > 0 {
                return Ok(Some(t.clone()));
            }
        }
        Ok(None)
    }

    fn segment_count(&self, transcript_id: &str) -> rusqlite::Result<i64> {
        if !self.has_table("segments") {
            return Ok(0);
        }
        self.conn.query_row(
            "SELECT count(*) FROM segments WHERE transcript_id = ?1",
            [transcript_id],
            |r| r.get(0),
        )
    }

    /// Segments in time order, with the Swift app's latest edits applied.
    pub fn segments(&self, transcript_id: &str) -> rusqlite::Result<Vec<LSegment>> {
        if !self.has_table("segments") {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT {}, start_ms, end_ms, text, {} FROM segments WHERE transcript_id = ?1 ORDER BY start_ms, rowid",
            if self.has("segments", "id") {
                "id"
            } else {
                "rowid"
            },
            self.c("segments", "speaker_label"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut segs = stmt
            .query_map([transcript_id], |r| {
                let start = int(r.get(1)?).unwrap_or(0).max(0);
                Ok(LSegment {
                    id: text(r.get(0)?).unwrap_or_default(),
                    start_ms: start,
                    end_ms: int(r.get(2)?).unwrap_or(start).max(start),
                    text: text(r.get(3)?).unwrap_or_default(),
                    speaker_label: opt_text(r, 4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if self.has_table("transcript_edits") {
            let mut stmt = self.conn.prepare(
                "SELECT e.segment_id, e.kind, e.edited_text FROM transcript_edits e
                 JOIN segments s ON s.id = e.segment_id
                 WHERE s.transcript_id = ?1 ORDER BY e.edited_at, e.rowid",
            )?;
            let edits = stmt
                .query_map([transcript_id], |r| {
                    Ok((
                        text(r.get(0)?).unwrap_or_default(),
                        text(r.get(1)?).unwrap_or_default(),
                        text(r.get(2)?).unwrap_or_default(),
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            // Later edits win: apply in order.
            for (sid, kind, edited) in edits {
                if let Some(s) = segs.iter_mut().find(|s| s.id == sid) {
                    match kind.as_str() {
                        "speaker" => {
                            s.speaker_label = Some(edited).filter(|t| !t.trim().is_empty())
                        }
                        _ => s.text = edited,
                    }
                }
            }
        }
        segs.retain(|s| !s.text.trim().is_empty());
        Ok(segs)
    }

    /// Newest summary for the meeting.
    pub fn summary(&self, meeting_id: &str) -> rusqlite::Result<Option<LSummary>> {
        if !self.has_table("summaries") || !self.has("summaries", "markdown_content") {
            return Ok(None);
        }
        let sql = format!(
            "SELECT markdown_content, {}, {} FROM summaries WHERE meeting_id = ?1 ORDER BY {} DESC, rowid DESC",
            self.c("summaries", "model"),
            self.c("summaries", "provider"),
            self.c("summaries", "created_at"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query_map([meeting_id], |r| {
            Ok(LSummary {
                markdown: text(r.get(0)?).unwrap_or_default(),
                model: opt_text(r, 1)?,
                provider: opt_text(r, 2)?,
            })
        })?;
        Ok(rows
            .next()
            .transpose()?
            .filter(|s| !s.markdown.trim().is_empty()))
    }

    /// `(text, done)` in creation order.
    pub fn action_items(&self, meeting_id: &str) -> rusqlite::Result<Vec<(String, bool)>> {
        if !self.has_table("action_items") {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT text, {} FROM action_items WHERE meeting_id = ?1 ORDER BY {}, rowid",
            self.c("action_items", "done"),
            self.c("action_items", "created_at"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([meeting_id], |r| {
            Ok((
                text(r.get(0)?).unwrap_or_default(),
                int(r.get(1)?).unwrap_or(0) != 0,
            ))
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(t, _)| !t.trim().is_empty())
            .collect())
    }

    /// Swift app tag names on a meeting or dictation.
    pub fn tags(&self, subject_type: &str, subject_id: &str) -> rusqlite::Result<Vec<String>> {
        if !self.has_table("tags") || !self.has_table("item_tags") {
            return Ok(vec![]);
        }
        let mut stmt = self.conn.prepare(
            "SELECT t.name FROM item_tags i JOIN tags t ON t.id = i.tag_id
             WHERE i.subject_type = ?1 AND i.subject_id = ?2 ORDER BY t.name",
        )?;
        let rows = stmt.query_map([subject_type, subject_id], |r| r.get(0))?;
        rows.collect()
    }

    /// Swift app dictations (the Tauri v1 app kept none).
    pub fn dictations(&self) -> rusqlite::Result<Vec<LDictation>> {
        if !self.has_table("dictations") {
            return Ok(vec![]);
        }
        let t = "dictations";
        let sql = format!(
            "SELECT id, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM dictations ORDER BY rowid",
            self.c(t, "started_at"),
            self.c(t, "created_at"),
            self.c(t, "duration_ms"),
            self.c(t, "target_app_name"),
            self.c(t, "raw_text"),
            self.c(t, "final_text"),
            self.c(t, "edited_text"),
            self.c(t, "insert_method"),
            self.c(t, "status"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(LDictation {
                id: text(r.get(0)?).unwrap_or_default(),
                started_at: opt_value(r, 1)?,
                created_at: opt_value(r, 2)?,
                duration_ms: int(r.get(3)?),
                target_app: opt_text(r, 4)?,
                raw_text: text(r.get(5)?).unwrap_or_default(),
                final_text: text(r.get(6)?).unwrap_or_default(),
                edited_text: text(r.get(7)?),
                insert_method: opt_text(r, 8)?,
                status: opt_text(r, 9)?,
            })
        })?;
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|d| !d.id.is_empty())
            .collect())
    }
}
