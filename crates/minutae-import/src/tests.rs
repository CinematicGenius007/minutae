//! Fixture databases built in code from the older apps' own schema files (copied into
//! `src/fixtures/` from `legacy/src-tauri/migrations` and `Packages/MinutaeCore/.../Resources`).
//! No test touches a real legacy database.

use std::path::{Path, PathBuf};

use minutae_core::{MeetingStatus, Track};
use minutae_store::rusqlite::{Connection, params};
use minutae_store::{SearchFilters, Store};
use uuid::Uuid;

use super::*;
use crate::wav::write_test_wav;

const TAURI_INIT: &str = include_str!("fixtures/tauri_v1_0001_init.sql");
const TAURI_FTS: &str = include_str!("fixtures/tauri_v1_0002_fts_triggers.sql");
const SWIFT_V1: &str = include_str!("fixtures/swift_v1.sql");
const SWIFT_V2: &str = include_str!("fixtures/swift_v2.sql");

const SWIFT_MEETING: &str = "6f1c2d3e-4b5a-4c6d-8e7f-9a0b1c2d3e4f";

const SUMMARY_MD: &str = "## Summary\n- The quarterly budget moves to May.\n- Hiring pauses.\n\n## Key Decisions\n- Freeze hiring until June\n\n## Action Items\n- [ ] Alice: send the budget sheet\n- [x] Book the review\n";

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }
    fn p(&self, rel: &str) -> PathBuf {
        let p = self.dir.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    }
    fn recordings(&self) -> PathBuf {
        self.dir.path().join("v4").join("recordings")
    }
}

/// Tauri v1: three meetings — a finished one with labelled segments and audio, one still
/// processing whose audio file is gone, one with an unreadable start time.
fn tauri_fixture(env: &Env) -> Source {
    let db = env.p("tauri/com.nebula.minutae/minutae.db");
    let wav = env.p("tauri-data/recordings/2026-03-02-V1abc.wav");
    write_test_wav(&wav, 16_000, 2);
    let c = Connection::open(&db).unwrap();
    c.execute_batch(TAURI_INIT).unwrap();
    c.execute_batch(TAURI_FTS).unwrap();
    c.execute_batch(&format!(
        "INSERT INTO meetings VALUES ('V1abc', 'Budget review', '2026-03-02T10:00:00.000Z', NULL, 120, 'done');
         INSERT INTO meetings VALUES ('V1def', 'Half done', '2026-03-03T09:00:00.000Z', NULL, NULL, 'processing');
         INSERT INTO meetings VALUES ('V1bad', 'No time', 'yesterday-ish', NULL, NULL, 'done');
         INSERT INTO recordings VALUES ('r1', 'V1abc', '{wav}', 64044, '2026-03-02T10:02:00.000Z');
         INSERT INTO recordings VALUES ('r2', 'V1def', '/nowhere/2026-03-03-V1def.wav', 10, '2026-03-03T09:05:00.000Z');
         INSERT INTO transcripts VALUES ('t1', 'V1abc', 'raw', 'whisper-api', 'en', 4000, '2026-03-02T10:03:00.000Z');
         INSERT INTO segments VALUES ('s1', 't1', 0, 1500, 'Let us look at the budget.', 'A');
         INSERT INTO segments VALUES ('s2', 't1', 1500, 3000, 'It moves to May.', 'B');
         INSERT INTO segments VALUES ('s3', 't1', 3000, 4000, 'Agreed.', 'A');
         INSERT INTO summaries VALUES ('su1', 'V1abc', '{md}', 'gpt-4o-mini', 'openai', '2026-03-02T10:04:00.000Z');
         INSERT INTO action_items VALUES ('a1', 'V1abc', 'Send the budget sheet', 1, '2026-03-02T10:04:00.000Z');
         INSERT INTO action_items VALUES ('a2', 'V1abc', 'Book the review', 0, '2026-03-02T10:04:01.000Z');",
        wav = wav.display(),
        md = SUMMARY_MD.replace('\'', "''"),
    ))
    .unwrap();
    Source {
        kind: SourceKind::TauriV1,
        db,
        audio_dirs: vec![env.p("tauri-data/recordings/x").parent().unwrap().into()],
    }
}

/// Swift app: WAL database with a writer still open (data only in `-wal`), a meeting whose
/// stored audio path moved (found by file name), edits, tags and dictations.
fn swift_fixture(env: &Env) -> (Source, Connection) {
    let db = env.p("swift/Library/Minutae/minutae.db");
    let wav = env.p(&format!(
        "swift/Library/Minutae/recordings/{SWIFT_MEETING}.wav"
    ));
    write_test_wav(&wav, 16_000, 3);
    let c = Connection::open(&db).unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    c.execute_batch(SWIFT_V1).unwrap();
    c.execute_batch(SWIFT_V2).unwrap();
    c.execute_batch(
        "ALTER TABLE dictations ADD COLUMN edited_text TEXT;
         ALTER TABLE dictations ADD COLUMN edited_at TEXT;
         ALTER TABLE dictations ADD COLUMN speech_model TEXT;",
    )
    .unwrap();
    let m = SWIFT_MEETING;
    c.execute_batch(&format!(
        "INSERT INTO meetings VALUES ('{m}', 'Design sync', '2026-04-01T14:00:00Z', '2026-04-01T14:30:00Z', 1800, 'done');
         INSERT INTO recordings VALUES ('rec', '{m}', '/Users/old-name/Library/Minutae/recordings/{m}.wav', 96044, '2026-04-01T14:30:00Z');
         INSERT INTO transcripts VALUES ('job1', '{m}', 'Hello there. Ship it in May.', 'whisper-local', 'en', 3000, '2026-04-01T14:31:00Z');
         INSERT INTO segments VALUES ('sa', 'job1', 0, 1000, 'Hello there.', NULL);
         INSERT INTO segments VALUES ('sb', 'job1', 1000, 3000, 'Ship it in Mai.', NULL);
         INSERT INTO transcript_edits VALUES ('e1', 'sb', 'text', 'Ship it in Mai.', 'Ship it in May.', '2026-04-02T09:00:00Z');
         INSERT INTO transcript_edits VALUES ('e2', 'sb', 'speaker', '', 'Priya', '2026-04-02T09:01:00Z');
         INSERT INTO summaries VALUES ('job2', '{m}', '{md}', 'gpt-5', 'codex', '2026-04-01T14:32:00Z');
         INSERT INTO tags VALUES ('tg', 'Work', NULL, '2026-04-01T00:00:00Z');
         INSERT INTO item_tags VALUES ('tg', 'meeting', '{m}');
         INSERT INTO dictations (id, started_at, duration_ms, target_app_name, raw_text, final_text, insert_method, status, created_at, edited_text)
           VALUES ('d-ok', '2026-04-03T08:00:00Z', 4000, 'Mail', 'raw words here', 'Final words here.', 'pasteboard', 'done', '2026-04-03T08:00:04Z', 'Edited words win.');
         INSERT INTO dictations (id, started_at, raw_text, final_text, status, created_at)
           VALUES ('d-cancel', '2026-04-03T09:00:00Z', '', '', 'cancelled', '2026-04-03T09:00:01Z');",
        md = SUMMARY_MD.replace('\'', "''"),
    ))
    .unwrap();
    (
        Source {
            kind: SourceKind::Swift,
            db: db.clone(),
            audio_dirs: vec![db.parent().unwrap().join("recordings")],
        },
        c,
    )
}

fn run_all(store: &Store, sources: &[Source], root: &Path) -> (ImportResult, Vec<Progress>) {
    let mut seen = vec![];
    let r = run(
        store,
        sources,
        RunOptions {
            recordings_root: root.to_path_buf(),
            progress: &mut |p| seen.push(p.clone()),
        },
    );
    (r, seen)
}

#[test]
fn tauri_v1_mapping() {
    let env = Env::new();
    let src = tauri_fixture(&env);
    let store = Store::open_in_memory().unwrap();

    let scan = scan(&store, std::slice::from_ref(&src));
    assert_eq!(scan[0].meetings, 3);
    assert_eq!(scan[0].dictations, 0);
    assert_eq!(scan[0].already_imported, 0);
    assert_eq!(scan[0].audio_bytes, 64_044);
    assert!(scan[0].error.is_none());

    let (r, progress) = run_all(&store, std::slice::from_ref(&src), &env.recordings());
    assert_eq!((r.imported, r.failed, r.skipped), (2, 1, 0), "{r:?}");
    assert_eq!(r.copied_bytes, 64_044);
    assert_eq!(progress.len(), 3);
    assert_eq!(progress.last().unwrap().done, 3);
    assert_eq!(progress.last().unwrap().total, 3);
    let bad = r.issues.iter().find(|i| i.failed).unwrap();
    assert_eq!(bad.legacy_id, "V1bad");
    assert!(bad.reason.contains("start time"));
    let missing = r.issues.iter().find(|i| i.legacy_id == "V1def").unwrap();
    assert!(!missing.failed && missing.reason.contains("without audio"));

    let id = store
        .legacy_mapping("tauri-v1", LegacyKind::Meeting, "V1abc")
        .unwrap()
        .unwrap();
    let d = store.meeting_detail(id).unwrap().unwrap();
    assert_eq!(d.meeting.title, "Budget review");
    assert_eq!(d.meeting.status, MeetingStatus::Done);
    assert_eq!(
        d.meeting.started_at.to_rfc3339(),
        "2026-03-02T10:00:00+00:00"
    );
    assert_eq!(
        (d.meeting.ended_at.unwrap() - d.meeting.started_at).num_seconds(),
        120
    );
    let t = d.transcript.unwrap();
    assert_eq!(t.language.as_deref(), Some("en"));
    assert_eq!(t.model, "whisper-api");
    assert_eq!(t.segments.len(), 3);
    assert!(t.segments.iter().all(|s| s.track == Track::Mic));
    assert_eq!(d.speakers.len(), 2);
    assert_eq!(d.speakers[0].label, "A");
    assert_eq!(t.segments[0].speaker_id, t.segments[2].speaker_id);
    assert_ne!(t.segments[0].speaker_id, t.segments[1].speaker_id);
    let s = d.summary.unwrap();
    assert_eq!(s.takeaway, "The quarterly budget moves to May.");
    assert_eq!(s.decisions, vec!["Freeze hiring until June"]);
    assert_eq!(
        (s.provider.as_str(), s.model.as_str()),
        ("openai", "gpt-4o-mini")
    );
    // The action_items table wins over the Markdown checklist (keeps `done`).
    let items: Vec<_> = d
        .action_items
        .iter()
        .map(|a| (a.item.text.as_str(), a.item.done))
        .collect();
    assert_eq!(
        items,
        vec![("Send the budget sheet", true), ("Book the review", false)]
    );
    // Audio copied into the v4 layout.
    let tracks = store.list_tracks(id).unwrap();
    assert_eq!(tracks.len(), 1);
    let expected = env.recordings().join(id.to_string()).join("mic.wav");
    assert_eq!(PathBuf::from(&tracks[0].path), expected);
    assert!(expected.is_file());
    assert_eq!(tracks[0].duration_ms, Some(2000));

    // Unfinished legacy meeting: imported, marked failed with a reason.
    let half = store
        .legacy_mapping("tauri-v1", LegacyKind::Meeting, "V1def")
        .unwrap()
        .unwrap();
    let h = store.get_meeting(half).unwrap().unwrap();
    assert_eq!(h.status, MeetingStatus::Failed);
    assert!(h.failure.unwrap().message.contains("hadn't finished"));
    // nanoid legacy ids get fresh UUIDs.
    assert_ne!(id.to_string(), "V1abc");

    // Imported text is in the FTS index.
    let hits = store.search("budget", &SearchFilters::default()).unwrap();
    assert!(hits.iter().any(|h| h.meeting_id == Some(id)));
}

#[test]
fn swift_mapping_reads_wal_and_keeps_uuid() {
    let env = Env::new();
    let (src, writer) = swift_fixture(&env);
    // Everything is still in the -wal file: the snapshot must include it.
    assert!(PathBuf::from(format!("{}-wal", src.db.display())).is_file());
    let before = std::fs::read(&src.db).unwrap();
    let store = Store::open_in_memory().unwrap();

    let sc = scan(&store, std::slice::from_ref(&src));
    assert_eq!((sc[0].meetings, sc[0].dictations), (1, 1), "{sc:?}");
    assert!(sc[0].audio_bytes > 0);

    let (r, _) = run_all(&store, std::slice::from_ref(&src), &env.recordings());
    assert_eq!((r.imported, r.dictations, r.failed), (1, 1, 0), "{r:?}");
    assert_eq!(r.skipped, 1, "the cancelled dictation is skipped");

    // The Swift UUID is kept, so the meeting id matches its old recording/summary names.
    let id = Uuid::parse_str(SWIFT_MEETING).unwrap();
    let d = store.meeting_detail(id).unwrap().unwrap();
    assert_eq!(d.meeting.title, "Design sync");
    let t = d.transcript.unwrap();
    assert_eq!(t.segments[1].text, "Ship it in May.", "text edit applied");
    assert_eq!(d.speakers.len(), 1);
    assert_eq!(d.speakers[0].label, "Priya", "speaker edit applied");
    assert_eq!(t.segments[0].speaker_id, None);
    // No action_items rows: fall back to the Markdown checklist.
    let items: Vec<_> = d
        .action_items
        .iter()
        .map(|a| (a.item.text.as_str(), a.item.done))
        .collect();
    assert_eq!(
        items,
        vec![
            ("Alice: send the budget sheet", false),
            ("Book the review", true)
        ]
    );
    // Moved home folder: audio found by file name next to the database.
    let tracks = store.list_tracks(id).unwrap();
    assert_eq!(tracks[0].duration_ms, Some(3000));

    let did = store
        .legacy_mapping("swift", LegacyKind::Dictation, "d-ok")
        .unwrap()
        .unwrap();
    let dict = store.get_dictation(did).unwrap().unwrap();
    assert_eq!(dict.text, "Edited words win.");
    assert_eq!(dict.raw_text, "raw words here");
    assert_eq!(dict.target_app.as_deref(), Some("Mail"));
    assert_eq!(dict.word_count, 3);
    assert!(dict.inserted);

    // Tags are kept for later in legacy_imports.extra.
    let extra: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT extra FROM legacy_imports WHERE legacy_id = ?1",
                params![SWIFT_MEETING],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(extra.contains("Work"));

    // The legacy database file was not modified.
    assert_eq!(std::fs::read(&src.db).unwrap(), before);
    drop(writer);
}

#[test]
fn rerun_is_idempotent() {
    let env = Env::new();
    let tauri = tauri_fixture(&env);
    let (swift, _w) = swift_fixture(&env);
    let sources = [swift, tauri];
    let store = Store::open_in_memory().unwrap();
    let (first, _) = run_all(&store, &sources, &env.recordings());
    assert_eq!(first.imported, 3);
    let meetings = store.count_meetings().unwrap();
    let dirs = std::fs::read_dir(env.recordings()).unwrap().count();

    let sc = scan(&store, &sources);
    assert_eq!(sc.iter().map(|s| s.already_imported).sum::<u32>(), 4);
    assert_eq!(sc[1].audio_bytes, 0, "nothing left to copy");

    let (second, progress) = run_all(&store, &sources, &env.recordings());
    assert_eq!((second.imported, second.dictations), (0, 0), "{second:?}");
    // Already-imported rows are skipped; the meeting with a bad time still fails.
    assert_eq!(second.failed, 1);
    assert_eq!(second.skipped, 5);
    assert_eq!(second.copied_bytes, 0);
    assert_eq!(progress.len(), 6);
    assert_eq!(store.count_meetings().unwrap(), meetings);
    assert_eq!(std::fs::read_dir(env.recordings()).unwrap().count(), dirs);
    assert_eq!(store.list_dictations(100, 0).unwrap().len(), 1);

    // Deleting an imported meeting in v4 does not bring it back on the next run.
    let id = Uuid::parse_str(SWIFT_MEETING).unwrap();
    store.delete_meeting(id).unwrap();
    let (third, _) = run_all(&store, &sources, &env.recordings());
    assert_eq!(third.imported, 0);
    assert!(store.get_meeting(id).unwrap().is_none());
}

#[test]
fn corrupt_and_missing_sources_fail_gracefully() {
    let env = Env::new();
    let garbage = env.p("bad/minutae.db");
    std::fs::write(&garbage, b"this is definitely not sqlite, just some bytes").unwrap();
    let not_minutae = env.p("other/minutae.db");
    Connection::open(&not_minutae)
        .unwrap()
        .execute_batch("CREATE TABLE notes (id TEXT);")
        .unwrap();
    let missing = env.p("gone/minutae.db");
    let good = tauri_fixture(&env);
    let sources = vec![
        Source {
            kind: SourceKind::Swift,
            db: garbage,
            audio_dirs: vec![],
        },
        Source {
            kind: SourceKind::Swift,
            db: not_minutae,
            audio_dirs: vec![],
        },
        Source {
            kind: SourceKind::Swift,
            db: missing,
            audio_dirs: vec![],
        },
        good,
    ];
    let store = Store::open_in_memory().unwrap();
    let sc = scan(&store, &sources);
    assert!(
        sc[0].error.as_deref().unwrap().contains("damaged"),
        "{sc:?}"
    );
    assert!(
        sc[1]
            .error
            .as_deref()
            .unwrap()
            .contains("no meetings table")
    );
    assert!(sc[2].error.as_deref().unwrap().contains("missing"));
    assert!(sc[3].error.is_none());

    // A broken source doesn't stop the good one.
    let (r, _) = run_all(&store, &sources, &env.recordings());
    assert_eq!(r.imported, 2);
    let source_failures: Vec<_> = r
        .issues
        .iter()
        .filter(|i| i.failed && i.legacy_id.is_empty())
        .collect();
    assert_eq!(source_failures.len(), 3);
    assert_eq!(r.failed, 4);
}

#[test]
fn audio_copy_failure_rolls_back_the_meeting() {
    let env = Env::new();
    let src = tauri_fixture(&env);
    // The recordings root is a file, so the copy can't create its folder.
    let root = env.p("v4/recordings-is-a-file");
    std::fs::write(&root, b"x").unwrap();
    let store = Store::open_in_memory().unwrap();
    let (r, _) = run_all(&store, std::slice::from_ref(&src), &root);
    let f = r.issues.iter().find(|i| i.legacy_id == "V1abc").unwrap();
    assert!(f.failed && f.reason.contains("audio couldn't be copied"));
    assert!(
        store
            .legacy_mapping("tauri-v1", LegacyKind::Meeting, "V1abc")
            .unwrap()
            .is_none(),
        "a failed meeting stays importable on the next run"
    );
}

#[test]
fn degrades_on_old_or_partial_schemas() {
    let env = Env::new();
    let db = env.p("min/minutae.db");
    // Only a bare meetings table with epoch seconds and no status column.
    Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT, started_at INTEGER);
             INSERT INTO meetings VALUES ('m1', '', 1767225600);",
        )
        .unwrap();
    let store = Store::open_in_memory().unwrap();
    let src = Source {
        kind: SourceKind::TauriV1,
        db,
        audio_dirs: vec![],
    };
    let (r, _) = run_all(&store, &[src], &env.recordings());
    assert_eq!(r.imported, 1, "{r:?}");
    let id = store
        .legacy_mapping("tauri-v1", LegacyKind::Meeting, "m1")
        .unwrap()
        .unwrap();
    let m = store.get_meeting(id).unwrap().unwrap();
    assert_eq!(m.title, "Untitled meeting");
    assert_eq!(m.started_at.to_rfc3339(), "2026-01-01T00:00:00+00:00");
    assert_eq!(m.status, MeetingStatus::Done);
}

#[test]
fn time_parsing() {
    use minutae_store::rusqlite::types::Value;
    let t = |v: Value| map::parse_time(&v).map(|t| t.to_rfc3339());
    assert_eq!(
        t(Value::Text("2026-04-01T14:00:00Z".into())).as_deref(),
        Some("2026-04-01T14:00:00+00:00")
    );
    assert_eq!(
        t(Value::Text("2026-04-01T16:00:00.123+02:00".into())).as_deref(),
        Some("2026-04-01T14:00:00.123+00:00")
    );
    assert_eq!(
        t(Value::Text("2026-04-01 14:00:00".into())).as_deref(),
        Some("2026-04-01T14:00:00+00:00")
    );
    assert_eq!(
        t(Value::Integer(1_767_225_600_000)).as_deref(),
        Some("2026-01-01T00:00:00+00:00")
    );
    assert_eq!(t(Value::Text("soon".into())), None);
    assert_eq!(t(Value::Integer(0)), None);
}

#[test]
fn default_paths_follow_the_old_apps() {
    let tauri = tauri_v1_source().unwrap();
    assert!(tauri.db.ends_with("com.nebula.minutae/minutae.db"));
    if cfg!(target_os = "macos") && std::env::var_os("MINUTAE_LEGACY_SWIFT_DB").is_none() {
        let swift = swift_source().unwrap();
        assert!(swift.db.ends_with("Library/Minutae/minutae.db"));
        assert!(swift.audio_dirs[0].ends_with("Library/Minutae/recordings"));
    }
}
