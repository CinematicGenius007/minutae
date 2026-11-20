//! Versioned migrations, tracked with `PRAGMA user_version`.
//!
//! Each entry runs once inside a transaction; re-opening a database is a no-op.
//! Never edit a shipped migration: append a new one.

use rusqlite::Connection;

use crate::Result;

/// Timestamps are INTEGER milliseconds since the Unix epoch (UTC).
/// IDs exposed to the app are UUID strings. Tables that back an FTS5 index use
/// an `INTEGER PRIMARY KEY` so the FTS rowid stays stable across VACUUM.
const V1: &str = r#"
CREATE TABLE meetings (
  id          TEXT PRIMARY KEY NOT NULL,
  title       TEXT NOT NULL,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER,
  status      TEXT NOT NULL CHECK (status IN ('recording','processing','done','failed')),
  source_app  TEXT,
  failure     TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX meetings_started ON meetings(started_at DESC);

CREATE TABLE tracks (
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  track       TEXT NOT NULL CHECK (track IN ('mic','system')),
  path        TEXT NOT NULL,
  duration_ms INTEGER,
  bytes       INTEGER,
  PRIMARY KEY (meeting_id, track)
);

CREATE TABLE speaker_profiles (
  id          TEXT PRIMARY KEY NOT NULL,
  name        TEXT NOT NULL,
  embedding   BLOB NOT NULL,
  dim         INTEGER NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE speakers (
  id           TEXT PRIMARY KEY NOT NULL,
  meeting_id   TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  label        TEXT NOT NULL,
  display_name TEXT,
  track        TEXT NOT NULL CHECK (track IN ('mic','system')),
  profile_id   TEXT REFERENCES speaker_profiles(id) ON DELETE SET NULL
);
CREATE INDEX speakers_meeting ON speakers(meeting_id);

CREATE TABLE transcripts (
  meeting_id  TEXT PRIMARY KEY NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  language    TEXT,
  model       TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);

CREATE TABLE segments (
  id          INTEGER PRIMARY KEY,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  start_ms    INTEGER NOT NULL,
  end_ms      INTEGER NOT NULL,
  text        TEXT NOT NULL,
  track       TEXT NOT NULL CHECK (track IN ('mic','system')),
  speaker_id  TEXT REFERENCES speakers(id) ON DELETE SET NULL
);
CREATE INDEX segments_meeting ON segments(meeting_id, start_ms);

CREATE TABLE summaries (
  id             INTEGER PRIMARY KEY,
  meeting_id     TEXT NOT NULL UNIQUE REFERENCES meetings(id) ON DELETE CASCADE,
  takeaway       TEXT NOT NULL,
  overview       TEXT NOT NULL,
  decisions      TEXT NOT NULL,      -- newline-joined, indexed by FTS
  decisions_json TEXT NOT NULL,      -- JSON array, source of truth
  markdown       TEXT NOT NULL,
  provider       TEXT NOT NULL,
  model          TEXT NOT NULL,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);

CREATE TABLE action_items (
  id          INTEGER PRIMARY KEY,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  text        TEXT NOT NULL,
  owner       TEXT,
  due         TEXT,
  done        INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL
);
CREATE INDEX action_items_meeting ON action_items(meeting_id, position);
CREATE INDEX action_items_open ON action_items(done);

CREATE TABLE moments (
  id          INTEGER PRIMARY KEY,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  at_ms       INTEGER NOT NULL,
  label       TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);
CREATE INDEX moments_meeting ON moments(meeting_id, at_ms);

CREATE TABLE dictations (
  seq         INTEGER PRIMARY KEY,
  id          TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL,
  text        TEXT NOT NULL,
  raw_text    TEXT NOT NULL,
  target_app  TEXT,
  word_count  INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  inserted    INTEGER NOT NULL
);
CREATE INDEX dictations_created ON dictations(created_at DESC);

CREATE TABLE jobs (
  id           TEXT PRIMARY KEY NOT NULL,
  kind         TEXT NOT NULL,
  state        TEXT NOT NULL CHECK (state IN ('queued','running','succeeded','failed','cancelled')),
  meeting_id   TEXT REFERENCES meetings(id) ON DELETE CASCADE,
  payload      TEXT NOT NULL DEFAULT 'null',
  attempts     INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL,
  run_after    INTEGER NOT NULL,
  last_error   TEXT,
  stage        TEXT,
  fraction     REAL,
  eta_seconds  INTEGER,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX jobs_state ON jobs(state, run_after);
CREATE INDEX jobs_meeting ON jobs(meeting_id);

CREATE TABLE api_usage (
  id            INTEGER PRIMARY KEY,
  at            INTEGER NOT NULL,
  provider      TEXT NOT NULL,
  transport     TEXT,
  model         TEXT NOT NULL,
  purpose       TEXT NOT NULL,
  meeting_id    TEXT REFERENCES meetings(id) ON DELETE SET NULL,
  tokens_in     INTEGER NOT NULL DEFAULT 0,
  tokens_out    INTEGER NOT NULL DEFAULT 0,
  cost_usd      REAL NOT NULL DEFAULT 0,
  duration_ms   INTEGER NOT NULL DEFAULT 0,
  ok            INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX api_usage_at ON api_usage(at);

CREATE TABLE settings (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  json        TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE imports (
  source      TEXT PRIMARY KEY NOT NULL,
  imported_at INTEGER NOT NULL,
  report      TEXT NOT NULL
);

-- Full-text search (external content tables kept in sync by triggers).
CREATE VIRTUAL TABLE segments_fts USING fts5(
  text, content='segments', content_rowid='id',
  tokenize='porter unicode61 remove_diacritics 2');
CREATE TRIGGER segments_ai AFTER INSERT ON segments BEGIN
  INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER segments_ad AFTER DELETE ON segments BEGIN
  INSERT INTO segments_fts(segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER segments_au AFTER UPDATE OF text ON segments BEGIN
  INSERT INTO segments_fts(segments_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE VIRTUAL TABLE summaries_fts USING fts5(
  takeaway, overview, decisions, content='summaries', content_rowid='id',
  tokenize='porter unicode61 remove_diacritics 2');
CREATE TRIGGER summaries_ai AFTER INSERT ON summaries BEGIN
  INSERT INTO summaries_fts(rowid, takeaway, overview, decisions)
    VALUES (new.id, new.takeaway, new.overview, new.decisions);
END;
CREATE TRIGGER summaries_ad AFTER DELETE ON summaries BEGIN
  INSERT INTO summaries_fts(summaries_fts, rowid, takeaway, overview, decisions)
    VALUES ('delete', old.id, old.takeaway, old.overview, old.decisions);
END;
CREATE TRIGGER summaries_au AFTER UPDATE ON summaries BEGIN
  INSERT INTO summaries_fts(summaries_fts, rowid, takeaway, overview, decisions)
    VALUES ('delete', old.id, old.takeaway, old.overview, old.decisions);
  INSERT INTO summaries_fts(rowid, takeaway, overview, decisions)
    VALUES (new.id, new.takeaway, new.overview, new.decisions);
END;

CREATE VIRTUAL TABLE action_items_fts USING fts5(
  text, owner, content='action_items', content_rowid='id',
  tokenize='porter unicode61 remove_diacritics 2');
CREATE TRIGGER action_items_ai AFTER INSERT ON action_items BEGIN
  INSERT INTO action_items_fts(rowid, text, owner) VALUES (new.id, new.text, new.owner);
END;
CREATE TRIGGER action_items_ad AFTER DELETE ON action_items BEGIN
  INSERT INTO action_items_fts(action_items_fts, rowid, text, owner)
    VALUES ('delete', old.id, old.text, old.owner);
END;
CREATE TRIGGER action_items_au AFTER UPDATE OF text, owner ON action_items BEGIN
  INSERT INTO action_items_fts(action_items_fts, rowid, text, owner)
    VALUES ('delete', old.id, old.text, old.owner);
  INSERT INTO action_items_fts(rowid, text, owner) VALUES (new.id, new.text, new.owner);
END;

CREATE VIRTUAL TABLE dictations_fts USING fts5(
  text, content='dictations', content_rowid='seq',
  tokenize='porter unicode61 remove_diacritics 2');
CREATE TRIGGER dictations_ai AFTER INSERT ON dictations BEGIN
  INSERT INTO dictations_fts(rowid, text) VALUES (new.seq, new.text);
END;
CREATE TRIGGER dictations_ad AFTER DELETE ON dictations BEGIN
  INSERT INTO dictations_fts(dictations_fts, rowid, text) VALUES ('delete', old.seq, old.text);
END;
CREATE TRIGGER dictations_au AFTER UPDATE OF text ON dictations BEGIN
  INSERT INTO dictations_fts(dictations_fts, rowid, text) VALUES ('delete', old.seq, old.text);
  INSERT INTO dictations_fts(rowid, text) VALUES (new.seq, new.text);
END;
"#;

/// v2: `meetings.title_lc`, the title lowercased by Rust (Unicode-aware, unlike SQLite's
/// ASCII-only `lower()`), used by title search. Maintained by the meeting write paths.
const V2: &str = r#"
ALTER TABLE meetings ADD COLUMN title_lc TEXT NOT NULL DEFAULT '';
"#;

/// One schema step: SQL, then an optional Rust data step in the same transaction.
struct Migration {
    sql: &'static str,
    data: Option<fn(&rusqlite::Transaction<'_>) -> Result<()>>,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        sql: V1,
        data: None,
    },
    Migration {
        sql: V2,
        data: Some(backfill_title_lc),
    },
];

/// Lowercased form stored in `meetings.title_lc`; must match how search tokens are lowercased.
pub(crate) fn title_key(title: &str) -> String {
    title.to_lowercase()
}

fn backfill_title_lc(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, title FROM meetings")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut update = tx.prepare("UPDATE meetings SET title_lc = ?2 WHERE id = ?1")?;
    for (id, title) in rows {
        update.execute(rusqlite::params![id, title_key(&title)])?;
    }
    Ok(())
}

/// Latest schema version this build knows.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

pub(crate) fn migrate(conn: &mut Connection) -> Result<u32> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(crate::StoreError::Invalid(format!(
            "database schema v{current} is newer than this app (v{SCHEMA_VERSION})"
        )));
    }
    for (i, m) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let version = i as u32 + 1;
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        if let Some(data) = m.data {
            data(&tx)?;
        }
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
        tracing::info!(version, "applied migration");
    }
    Ok(SCHEMA_VERSION)
}
