CREATE TABLE IF NOT EXISTS meetings (
  id          TEXT PRIMARY KEY NOT NULL,
  title       TEXT NOT NULL DEFAULT 'Untitled Meeting',
  started_at  TEXT NOT NULL,
  ended_at    TEXT,
  duration_seconds INTEGER,
  status      TEXT NOT NULL DEFAULT 'recording'
                CHECK(status IN ('recording','paused','processing','done','failed'))
);

CREATE TABLE IF NOT EXISTS recordings (
  id            TEXT PRIMARY KEY NOT NULL,
  meeting_id    TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  file_path     TEXT NOT NULL,
  file_size_bytes INTEGER,
  created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transcripts (
  id          TEXT PRIMARY KEY NOT NULL,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  raw_text    TEXT NOT NULL,
  provider    TEXT NOT NULL DEFAULT 'whisper-local'
                CHECK(provider IN ('whisper-local','whisper-api')),
  language    TEXT,
  duration_ms INTEGER,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS segments (
  id            TEXT PRIMARY KEY NOT NULL,
  transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
  start_ms      INTEGER NOT NULL,
  end_ms        INTEGER NOT NULL,
  text          TEXT NOT NULL,
  speaker_label TEXT
);

CREATE TABLE IF NOT EXISTS summaries (
  id               TEXT PRIMARY KEY NOT NULL,
  meeting_id       TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  markdown_content TEXT NOT NULL,
  model            TEXT NOT NULL,
  provider         TEXT NOT NULL,
  created_at       TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS action_items (
  id          TEXT PRIMARY KEY NOT NULL,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  text        TEXT NOT NULL,
  done        INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS embeddings (
  id          TEXT PRIMARY KEY NOT NULL,
  meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
  chunk_text  TEXT NOT NULL,
  chunk_index INTEGER NOT NULL,
  vector      BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS api_usage_log (
  id            TEXT PRIMARY KEY NOT NULL,
  meeting_id    TEXT REFERENCES meetings(id) ON DELETE SET NULL,
  provider      TEXT NOT NULL,
  model         TEXT NOT NULL,
  endpoint      TEXT NOT NULL,
  input_tokens  INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd      REAL NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key        TEXT PRIMARY KEY NOT NULL,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_meetings_started_at ON meetings(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_recordings_meeting_id ON recordings(meeting_id);
CREATE INDEX IF NOT EXISTS idx_transcripts_meeting_id ON transcripts(meeting_id);
CREATE INDEX IF NOT EXISTS idx_segments_transcript_id ON segments(transcript_id);
CREATE INDEX IF NOT EXISTS idx_summaries_meeting_id ON summaries(meeting_id);
CREATE INDEX IF NOT EXISTS idx_action_items_meeting_id ON action_items(meeting_id);
CREATE INDEX IF NOT EXISTS idx_embeddings_meeting_id ON embeddings(meeting_id);
CREATE INDEX IF NOT EXISTS idx_api_usage_created_at ON api_usage_log(created_at DESC);

-- Full-text search on transcripts
CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts USING fts5(
  raw_text,
  content='transcripts',
  content_rowid='rowid'
);
-- FTS5 content-table triggers: keep transcripts_fts in sync with transcripts

CREATE TRIGGER IF NOT EXISTS transcripts_ai AFTER INSERT ON transcripts BEGIN
  INSERT INTO transcripts_fts(rowid, raw_text) VALUES (new.rowid, new.raw_text);
END;

CREATE TRIGGER IF NOT EXISTS transcripts_ad AFTER DELETE ON transcripts BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, raw_text)
    VALUES ('delete', old.rowid, old.raw_text);
END;

CREATE TRIGGER IF NOT EXISTS transcripts_au AFTER UPDATE ON transcripts BEGIN
  INSERT INTO transcripts_fts(transcripts_fts, rowid, raw_text)
    VALUES ('delete', old.rowid, old.raw_text);
  INSERT INTO transcripts_fts(rowid, raw_text) VALUES (new.rowid, new.raw_text);
END;
