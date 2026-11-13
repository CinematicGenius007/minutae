
CREATE TABLE IF NOT EXISTS dictations (
  id TEXT PRIMARY KEY NOT NULL,
  started_at TEXT NOT NULL,
  duration_ms INTEGER,
  target_app_bundle_id TEXT,
  target_app_name TEXT,
  raw_text TEXT NOT NULL DEFAULT '',
  final_text TEXT NOT NULL DEFAULT '',
  language TEXT,
  insert_method TEXT CHECK (insert_method IN ('pasteboard','accessibility','typing','clipboardOnly','none')),
  status TEXT NOT NULL DEFAULT 'listening'
    CHECK (status IN ('listening','processing','inserting','done','failed','cancelled')),
  audio_path TEXT,
  error TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dictations_started_at ON dictations(started_at DESC);

CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('transcribe','summarise','embed','export','retention')),
  subject_type TEXT NOT NULL CHECK (subject_type IN ('meeting','dictation')),
  subject_id TEXT NOT NULL,            -- polymorphic, so no foreign key
  payload TEXT,                        -- JSON
  state TEXT NOT NULL DEFAULT 'queued'
    CHECK (state IN ('queued','running','succeeded','failed','cancelled')),
  attempts INTEGER NOT NULL DEFAULT 0,
  max_attempts INTEGER NOT NULL DEFAULT 3,
  last_error TEXT,
  run_after TEXT,                      -- backoff: not eligible before this time
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_jobs_state_run_after ON jobs(state, run_after);
CREATE INDEX IF NOT EXISTS idx_jobs_subject ON jobs(subject_type, subject_id);

CREATE TABLE IF NOT EXISTS tags (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL COLLATE NOCASE UNIQUE,
  color TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS item_tags (
  tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  subject_type TEXT NOT NULL CHECK (subject_type IN ('meeting','dictation')),
  subject_id TEXT NOT NULL,
  PRIMARY KEY (tag_id, subject_type, subject_id)
);
CREATE INDEX IF NOT EXISTS idx_item_tags_subject ON item_tags(subject_type, subject_id);

CREATE TABLE IF NOT EXISTS dictionary (
  id TEXT PRIMARY KEY NOT NULL,
  term TEXT NOT NULL,
  replacement TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'word' CHECK (kind IN ('word','snippet')),
  scope_bundle_id TEXT,                -- NULL = all apps
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_dictionary_unique
  ON dictionary(term, kind, COALESCE(scope_bundle_id, ''));

CREATE TABLE IF NOT EXISTS transcript_edits (
  id TEXT PRIMARY KEY NOT NULL,
  segment_id TEXT NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
  kind TEXT NOT NULL DEFAULT 'text' CHECK (kind IN ('text','speaker')),
  original_text TEXT NOT NULL,
  edited_text TEXT NOT NULL,
  edited_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_transcript_edits_segment ON transcript_edits(segment_id);
