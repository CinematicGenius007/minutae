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
