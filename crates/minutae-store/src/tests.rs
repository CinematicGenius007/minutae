use std::time::Instant;

use chrono::{Duration, TimeZone, Utc};
use minutae_core::*;
use uuid::Uuid;

use crate::*;

/// Timestamps are stored with millisecond precision.
fn meeting(title: &str, started: chrono::DateTime<Utc>) -> Meeting {
    let started = Utc
        .timestamp_millis_opt(started.timestamp_millis())
        .unwrap();
    Meeting {
        id: Uuid::new_v4(),
        title: title.into(),
        started_at: started,
        ended_at: Some(started + Duration::minutes(30)),
        status: MeetingStatus::Done,
        source_app: Some("Zoom".into()),
        failure: None,
    }
}

fn seg(start: u64, text: &str, speaker: Option<SpeakerId>) -> Segment {
    Segment {
        start_ms: start,
        end_ms: start + 4000,
        text: text.into(),
        track: Track::System,
        speaker_id: speaker,
    }
}

fn summary(mid: MeetingId, items: &[(&str, bool)]) -> Summary {
    Summary {
        meeting_id: mid,
        overview: "We discussed the quarterly budget and the hiring plan.".into(),
        decisions: vec!["Ship the redesign in May".into()],
        action_items: items
            .iter()
            .map(|(t, d)| ActionItem {
                text: (*t).into(),
                owner: Some("Alice".into()),
                due: None,
                done: *d,
            })
            .collect(),
        takeaway: "Redesign moves to May".into(),
        markdown: "# Notes".into(),
        provider: "codex".into(),
        model: "gpt-5".into(),
    }
}

#[test]
fn migrations_are_idempotent_on_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("m.db");
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), schema::SCHEMA_VERSION);
    let m = meeting("A", Utc::now());
    s.create_meeting(&m).unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.get_meeting(m.id).unwrap().unwrap(), m);
    let mode: String = s
        .read(|c| Ok(c.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
        .unwrap();
    assert_eq!(mode, "wal");
}

#[test]
fn meeting_lifecycle_and_list() {
    let s = Store::open_in_memory().unwrap();
    let now = Utc::now();
    let mut m = meeting("Design sync", now - Duration::hours(1));
    m.ended_at = None;
    m.status = MeetingStatus::Recording;
    s.create_meeting(&m).unwrap();
    s.end_meeting(m.id, now).unwrap();
    s.rename_meeting(m.id, "Q2 Design sync").unwrap();
    let failure = Failure {
        kind: FailureKind::NotLoggedIn,
        message: "Codex isn't signed in.".into(),
        fix: Some("Sign in".into()),
    };
    s.set_meeting_status(m.id, MeetingStatus::Failed, Some(&failure))
        .unwrap();
    let got = s.get_meeting(m.id).unwrap().unwrap();
    assert_eq!(got.title, "Q2 Design sync");
    assert_eq!(got.status, MeetingStatus::Failed);
    assert_eq!(got.failure, Some(failure));
    assert!(matches!(
        s.rename_meeting(Uuid::new_v4(), "x"),
        Err(StoreError::NotFound(_))
    ));

    let alice = Speaker {
        id: Uuid::new_v4(),
        meeting_id: m.id,
        label: "Speaker 1".into(),
        display_name: None,
        track: Track::System,
    };
    let you = Speaker {
        id: Uuid::new_v4(),
        meeting_id: m.id,
        label: "You".into(),
        display_name: None,
        track: Track::Mic,
    };
    let t = Transcript {
        meeting_id: m.id,
        language: Some("en".into()),
        model: "small".into(),
        segments: vec![
            seg(0, "hello", Some(you.id)),
            seg(5000, "hi there", Some(alice.id)),
        ],
    };
    s.replace_transcript(&t, &[alice.clone(), you.clone()])
        .unwrap();
    s.rename_speaker(alice.id, Some("Alice")).unwrap();
    s.upsert_summary(&summary(
        m.id,
        &[("Send mockups", false), ("Review budget", true)],
    ))
    .unwrap();
    s.add_moment(&Moment {
        meeting_id: m.id,
        at_ms: 5000,
        label: "Decision".into(),
    })
    .unwrap();
    s.insert_job(&NewJob::new(
        JobKind::Summarize,
        serde_json::json!({}),
        Some(m.id),
    ))
    .unwrap();

    let list = s.list_meetings(10, 0).unwrap();
    assert_eq!(list.len(), 1);
    let row = &list[0];
    assert_eq!(row.takeaway.as_deref(), Some("Redesign moves to May"));
    assert_eq!(row.people, vec!["Alice".to_string(), "You".to_string()]);
    assert_eq!(row.open_actions, 1);
    assert_eq!(row.duration_ms, Some(3_600_000));
    assert_eq!(row.progress.as_ref().unwrap().stage, Stage::Summarizing);

    let d = s.meeting_detail(m.id).unwrap().unwrap();
    assert_eq!(d.transcript.unwrap().segments, t.segments);
    assert_eq!(d.summary.unwrap().action_items.len(), 2);
    assert_eq!(d.moments.len(), 1);
    assert_eq!(
        d.speakers
            .iter()
            .find(|x| x.id == alice.id)
            .unwrap()
            .display_name
            .as_deref(),
        Some("Alice")
    );

    // Re-transcribe atomically replaces everything.
    let t2 = Transcript {
        segments: vec![seg(0, "only one", None)],
        ..t.clone()
    };
    s.replace_transcript(&t2, &[]).unwrap();
    assert_eq!(s.get_transcript(m.id).unwrap().unwrap().segments.len(), 1);
    assert!(s.list_speakers(m.id).unwrap().is_empty());
    assert!(
        s.search("hello", &SearchFilters::default())
            .unwrap()
            .is_empty()
    );

    // Cascade delete clears children and FTS.
    assert!(s.delete_meeting(m.id).unwrap());
    assert!(
        s.search("only", &SearchFilters::default())
            .unwrap()
            .is_empty()
    );
    assert!(
        s.search("budget", &SearchFilters::default())
            .unwrap()
            .is_empty()
    );
    let leftover: i64 = s
        .read(|c| Ok(c.query_row("SELECT (SELECT count(*) FROM segments)+(SELECT count(*) FROM action_items)+(SELECT count(*) FROM jobs)+(SELECT count(*) FROM moments)", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(leftover, 0);
}

#[test]
fn group_by_day() {
    let s = Store::open_in_memory().unwrap();
    let day1 = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
    for (i, t) in [day1, day1 - Duration::hours(2), day1 - Duration::days(1)]
        .into_iter()
        .enumerate()
    {
        s.create_meeting(&meeting(&format!("M{i}"), t)).unwrap();
    }
    let groups = group_by_local_day(s.list_meetings(50, 0).unwrap(), 0);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].items.len(), 2);
    assert_eq!(s.list_meetings(1, 2).unwrap()[0].meeting.title, "M2");
}

#[test]
fn summary_upsert_keeps_done_and_toggle() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("A", Utc::now());
    s.create_meeting(&m).unwrap();
    s.upsert_summary(&summary(m.id, &[("Send mockups", false)]))
        .unwrap();
    let id = s.list_action_items(m.id).unwrap()[0].id;
    assert!(s.toggle_action_item(id).unwrap());
    s.upsert_summary(&summary(
        m.id,
        &[("Send mockups", false), ("New thing", false)],
    ))
    .unwrap();
    let items = s.list_action_items(m.id).unwrap();
    assert!(items[0].item.done);
    assert!(!items[1].item.done);
    assert_eq!(s.list_open_action_items(10).unwrap().len(), 1);
    assert_eq!(
        s.week_stats(Utc::now() - Duration::days(7))
            .unwrap()
            .open_actions,
        1
    );
}

#[test]
fn dictations_and_stats() {
    let s = Store::open_in_memory().unwrap();
    let now = Utc::now();
    for (i, words) in [5u32, 7, 11].iter().enumerate() {
        s.insert_dictation(&Dictation {
            id: Uuid::new_v4(),
            created_at: now - Duration::days(i as i64 * 5),
            text: format!("please email the quarterly report number {i}"),
            raw_text: "um please email".into(),
            target_app: Some("Mail".into()),
            word_count: *words,
            duration_ms: 1000,
            inserted: true,
        })
        .unwrap();
    }
    let list = s.list_dictations(10, 0).unwrap();
    assert_eq!(list.len(), 3);
    let st = s.dictation_stats(now - Duration::days(7)).unwrap();
    assert_eq!((st.count, st.words), (2, 12));
    let hits = s
        .search(
            "quarterly rep",
            &SearchFilters {
                kind: SearchKind::Dictations,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hits.len(), 3);
    assert!(hits[0].snippet_html.contains("<mark>quarterly</mark>"));
    assert!(s.delete_dictation(list[0].id).unwrap());
    assert_eq!(
        s.search("quarterly", &SearchFilters::default())
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn settings_roundtrip() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.get_settings().unwrap(), AppSettings::default());
    let out = s
        .update_settings(|x| x.appearance = Appearance::Dark)
        .unwrap();
    assert_eq!(out.appearance, Appearance::Dark);
    assert_eq!(s.get_settings().unwrap().appearance, Appearance::Dark);
    // Partial / older rows still load.
    s.write(|c| Ok(c.execute("UPDATE settings SET json = '{\"launchAtLogin\":false}'", [])?))
        .unwrap();
    let got = s.get_settings().unwrap();
    assert!(!got.launch_at_login);
    assert_eq!(got.prompt_delay_seconds, 5);
}

#[test]
fn usage_monthly_aggregates() {
    let s = Store::open_in_memory().unwrap();
    let base = Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap();
    let e = |at, provider: &str, ok| UsageEntry {
        at,
        provider: provider.into(),
        transport: Some("api".into()),
        model: "m".into(),
        purpose: "summary".into(),
        meeting_id: None,
        tokens_in: 100,
        tokens_out: 10,
        cost_usd: 0.5,
        duration_ms: 10,
        ok,
    };
    s.log_usage(&e(base, "openai", true)).unwrap();
    s.log_usage(&e(base + Duration::days(1), "openai", false))
        .unwrap();
    s.log_usage(&e(base + Duration::days(30), "openai", true))
        .unwrap();
    s.log_usage(&e(base, "anthropic", true)).unwrap();
    let agg = s.usage_by_month(base - Duration::days(60)).unwrap();
    assert_eq!(agg.len(), 3);
    assert_eq!(agg[0].month, "2026-11");
    let oct_openai = agg
        .iter()
        .find(|a| a.month == "2026-10" && a.provider == "openai")
        .unwrap();
    assert_eq!(
        (
            oct_openai.calls,
            oct_openai.failed_calls,
            oct_openai.tokens_in
        ),
        (2, 1, 200)
    );
    assert!((oct_openai.cost_usd - 1.0).abs() < 1e-9);
}

#[test]
fn job_state_transitions() {
    let s = Store::open_in_memory().unwrap();
    let j = s
        .insert_job(&NewJob::new(
            JobKind::Transcribe,
            serde_json::json!({"a":1}),
            None,
        ))
        .unwrap();
    assert_eq!(j.state, JobState::Queued);
    assert_eq!(j.max_attempts, 2);
    assert_eq!(s.due_jobs(Utc::now(), 10).unwrap().len(), 1);
    let claimed = s.claim_job(j.id).unwrap().unwrap();
    assert_eq!((claimed.state, claimed.attempts), (JobState::Running, 1));
    assert!(s.claim_job(j.id).unwrap().is_none());
    s.set_job_progress(
        j.id,
        &Progress {
            meeting_id: None,
            stage: Stage::Transcribing,
            fraction: 0.5,
            eta_seconds: Some(3),
        },
    )
    .unwrap();
    assert_eq!(
        s.get_job(j.id).unwrap().unwrap().progress.unwrap().fraction,
        0.5
    );
    assert_eq!(s.recover_running_jobs().unwrap(), 1);
    let j2 = s.get_job(j.id).unwrap().unwrap();
    // The interrupted attempt counts.
    assert_eq!((j2.state, j2.attempts), (JobState::Queued, 1));
    assert!(s.cancel_job(j.id).unwrap());
    assert!(!s.cancel_job(j.id).unwrap());
    assert!(s.requeue_job(j.id).unwrap());
}

#[test]
fn queued_meeting_job_is_not_duplicated() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("Dup", Utc::now());
    s.create_meeting(&m).unwrap();
    let job = || NewJob::new(JobKind::Diarize, serde_json::json!({}), Some(m.id));
    let a = s.insert_job(&job()).unwrap();
    // The previous stage crashed after queueing this one and ran again.
    let b = s.insert_job(&job()).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(s.list_jobs(&[JobState::Queued], 10).unwrap().len(), 1);
    // Once it runs, a new request is a new job (e.g. the user asked to redo it).
    s.claim_job(a.id).unwrap().unwrap();
    assert_ne!(s.insert_job(&job()).unwrap().id, a.id);
    // Jobs without a meeting are never merged.
    let idx = || NewJob::new(JobKind::Index, serde_json::json!(null), None);
    assert_ne!(
        s.insert_job(&idx()).unwrap().id,
        s.insert_job(&idx()).unwrap().id
    );
}

#[test]
fn crash_looping_job_stops_after_max_attempts() {
    let s = Store::open_in_memory().unwrap();
    let j = s
        .insert_job(&NewJob::new(
            JobKind::Transcribe,
            serde_json::json!({}),
            None,
        ))
        .unwrap();
    assert_eq!(j.max_attempts, 2);
    // Each launch claims the job and the process dies mid-run.
    s.claim_job(j.id).unwrap().unwrap();
    assert_eq!(s.recover_running_jobs().unwrap(), 1);
    s.claim_job(j.id).unwrap().unwrap();
    assert_eq!(s.recover_running_jobs().unwrap(), 0);
    let j2 = s.get_job(j.id).unwrap().unwrap();
    assert_eq!((j2.state, j2.attempts), (JobState::Failed, 2));
    assert!(j2.last_error.is_some());
    assert!(s.claim_job(j.id).unwrap().is_none());
}

#[test]
fn graceful_release_refunds_attempt() {
    let s = Store::open_in_memory().unwrap();
    let j = s
        .insert_job(&NewJob::new(
            JobKind::Summarize,
            serde_json::json!({}),
            None,
        ))
        .unwrap();
    s.claim_job(j.id).unwrap().unwrap();
    assert!(s.release_job(j.id).unwrap());
    let j2 = s.get_job(j.id).unwrap().unwrap();
    assert_eq!((j2.state, j2.attempts), (JobState::Queued, 0));
}

#[test]
fn progress_is_ignored_once_job_finished() {
    let s = Store::open_in_memory().unwrap();
    let j = s
        .insert_job(&NewJob::new(
            JobKind::Summarize,
            serde_json::json!({}),
            None,
        ))
        .unwrap();
    s.claim_job(j.id).unwrap().unwrap();
    assert!(s.complete_job(j.id).unwrap());
    s.set_job_progress(
        j.id,
        &Progress {
            meeting_id: None,
            stage: Stage::Summarizing,
            fraction: 0.4,
            eta_seconds: None,
        },
    )
    .unwrap();
    assert!(s.get_job(j.id).unwrap().unwrap().progress.is_none());
}

#[test]
fn legacy_meeting_import_is_atomic_and_idempotent() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("Imported sync", Utc::now());
    let you = Speaker {
        id: Uuid::new_v4(),
        meeting_id: m.id,
        label: "Speaker A".into(),
        display_name: None,
        track: Track::Mic,
    };
    let bundle = ImportedMeeting {
        meeting: m.clone(),
        tracks: vec![TrackFile {
            meeting_id: m.id,
            track: Track::Mic,
            path: "/r/mic.wav".into(),
            duration_ms: Some(1000),
            bytes: Some(44),
        }],
        transcript: Some(Transcript {
            meeting_id: m.id,
            language: Some("en".into()),
            model: "whisper".into(),
            segments: vec![seg(0, "the quarterly budget moved", Some(you.id))],
        }),
        speakers: vec![you],
        summary: Some(summary(m.id, &[("Send notes", true)])),
        extra: Some(serde_json::json!({ "tags": ["Work"] })),
    };
    assert!(
        s.import_legacy_meeting("swift", "legacy-1", &bundle)
            .unwrap()
    );
    // Same legacy id again: nothing written.
    let mut again = bundle.clone();
    again.meeting.id = Uuid::new_v4();
    again.tracks.clear();
    again.transcript = None;
    again.speakers.clear();
    again.summary = None;
    assert!(
        !s.import_legacy_meeting("swift", "legacy-1", &again)
            .unwrap()
    );
    assert_eq!(s.count_meetings().unwrap(), 1);
    assert_eq!(
        s.legacy_mapping("swift", LegacyKind::Meeting, "legacy-1")
            .unwrap(),
        Some(m.id)
    );
    assert!(s.id_in_use(m.id).unwrap());
    assert_eq!(s.list_action_items(m.id).unwrap().len(), 1);
    // Imported text is searchable.
    let hits = s.search("budget", &SearchFilters::default()).unwrap();
    assert!(hits.iter().any(|h| h.meeting_id == Some(m.id)));
    // Rows that belong to another meeting are refused before anything is written.
    let mut bad = bundle.clone();
    bad.meeting.id = Uuid::new_v4();
    assert!(s.import_legacy_meeting("swift", "legacy-2", &bad).is_err());
    assert!(
        s.legacy_mapping("swift", LegacyKind::Meeting, "legacy-2")
            .unwrap()
            .is_none()
    );
    // A failing write rolls back the mapping too (duplicate meeting id).
    let mut dup = bundle.clone();
    dup.extra = None;
    assert!(s.import_legacy_meeting("swift", "legacy-3", &dup).is_err());
    assert!(
        s.legacy_mapping("swift", LegacyKind::Meeting, "legacy-3")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        s.legacy_imported_ids("swift", LegacyKind::Meeting)
            .unwrap()
            .len(),
        1
    );
    s.record_import_run("swift", &serde_json::json!({ "imported": 1 }))
        .unwrap();
    assert_eq!(s.last_import_run("swift").unwrap().unwrap()["imported"], 1);
}

#[test]
fn search_kinds_and_ranking() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("Budget review", Utc::now());
    s.create_meeting(&m).unwrap();
    let t = Transcript {
        meeting_id: m.id,
        language: None,
        model: "x".into(),
        segments: vec![
            seg(0, "the budget for marketing is tight", None),
            seg(9000, "nothing to see <here>", None),
        ],
    };
    s.replace_transcript(&t, &[]).unwrap();
    s.upsert_summary(&summary(m.id, &[("Review budget with finance", false)]))
        .unwrap();
    let hits = s.search("budget", &SearchFilters::default()).unwrap();
    let kinds: Vec<HitKind> = hits.iter().map(|h| h.kind).collect();
    assert_eq!(kinds[0], HitKind::Meeting);
    assert!(kinds.contains(&HitKind::Transcript));
    assert!(kinds.contains(&HitKind::Summary));
    assert!(kinds.contains(&HitKind::ActionItem));
    let tr = hits.iter().find(|h| h.kind == HitKind::Transcript).unwrap();
    assert_eq!(tr.at_ms, Some(0));
    assert_eq!(
        &tr.snippet[tr.highlights[0][0] as usize..tr.highlights[0][1] as usize],
        "budget"
    );
    let only_actions = s
        .search(
            "budget",
            &SearchFilters {
                kind: SearchKind::ActionItems,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        only_actions
            .iter()
            .all(|h| h.kind == HitKind::ActionItem && h.action_item_id.is_some())
    );
    // Stemming + prefix
    assert!(
        !s.search("budgets", &SearchFilters::default())
            .unwrap()
            .is_empty()
    );
    assert!(
        !s.search("mark", &SearchFilters::default())
            .unwrap()
            .is_empty()
    );
    // since filter
    let future = SearchFilters {
        since: Some(Utc::now() + Duration::days(1)),
        ..Default::default()
    };
    assert!(s.search("budget", &future).unwrap().is_empty());
    // Hostile input never errors.
    for q in ["\"", "AND OR NOT", "*", "NEAR(", "a\"b", ""] {
        s.search(q, &SearchFilters::default()).unwrap();
    }
}

#[test]
fn search_10k_segments_under_100ms() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("bench.db")).unwrap();
    let words = [
        "budget",
        "roadmap",
        "hiring",
        "design",
        "launch",
        "customer",
        "pricing",
        "latency",
        "onboarding",
        "metrics",
        "quarter",
        "review",
        "partner",
        "contract",
        "release",
        "testing",
        "mobile",
        "desktop",
        "security",
        "migration",
    ];
    let mut rng: u64 = 42;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for mi in 0..100 {
        let m = meeting(&format!("Meeting {mi}"), Utc::now() - Duration::days(mi));
        s.create_meeting(&m).unwrap();
        let segments = (0..100)
            .map(|i| {
                let text: Vec<&str> = (0..14)
                    .map(|_| words[(next() % words.len() as u64) as usize])
                    .collect();
                seg(
                    i * 5000,
                    &format!("so the {} and we said", text.join(" ")),
                    None,
                )
            })
            .collect();
        s.replace_transcript(
            &Transcript {
                meeting_id: m.id,
                language: None,
                model: "x".into(),
                segments,
            },
            &[],
        )
        .unwrap();
    }
    let n: i64 = s
        .read(|c| Ok(c.query_row("SELECT count(*) FROM segments", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 10_000);
    s.search("warm", &SearchFilters::default()).unwrap();
    for q in ["budget", "pricing laten", "security migration release"] {
        let t0 = Instant::now();
        let hits = s.search(q, &SearchFilters::default()).unwrap();
        let el = t0.elapsed();
        eprintln!("search {q:?}: {} hits in {el:?}", hits.len());
        assert!(!hits.is_empty());
        // Target is <100 ms in release; allow slack for debug builds / CI.
        assert!(
            el.as_millis() < if cfg!(debug_assertions) { 500 } else { 100 },
            "{q}: {el:?}"
        );
    }
}

#[test]
fn title_search_folds_non_ascii_case() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("Über Sync", Utc::now());
    s.create_meeting(&m).unwrap();
    let titles = |q: &str| -> Vec<String> {
        s.search(q, &SearchFilters::default())
            .unwrap()
            .into_iter()
            .filter(|h| h.kind == HitKind::Meeting)
            .map(|h| h.title)
            .collect()
    };
    assert_eq!(titles("über"), vec!["Über Sync".to_string()]);
    assert_eq!(titles("ÜBER sy"), vec!["Über Sync".to_string()]);
    // Renames keep the search key in step.
    s.rename_meeting(m.id, "Ärger Review").unwrap();
    assert!(titles("über").is_empty());
    assert_eq!(titles("ärger"), vec!["Ärger Review".to_string()]);
    let mut m2 = s.get_meeting(m.id).unwrap().unwrap();
    m2.title = "ÉTÉ Planning".into();
    s.update_meeting(&m2).unwrap();
    assert_eq!(titles("été"), vec!["ÉTÉ Planning".to_string()]);
}

#[test]
fn v2_migration_backfills_title_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.db");
    let s = Store::open(&path).unwrap();
    let m = meeting("Über Sync", Utc::now());
    s.create_meeting(&m).unwrap();
    // Roll the file back to a v1 database.
    s.write(|c| {
        c.execute_batch(
            "ALTER TABLE meetings DROP COLUMN title_lc;
             ALTER TABLE meetings DROP COLUMN echo_cancel;
             ALTER TABLE meetings DROP COLUMN markdown_file;
             ALTER TABLE meetings DROP COLUMN checklist_curated;
             ALTER TABLE action_items DROP COLUMN source_ms;
             DROP TABLE meeting_waveforms;
             DROP TABLE discarded_recordings;
             DROP TABLE legacy_imports;
             PRAGMA user_version = 1;",
        )?;
        Ok(())
    })
    .unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), schema::SCHEMA_VERSION);
    let hits = s.search("über", &SearchFilters::default()).unwrap();
    assert!(
        hits.iter()
            .any(|h| h.kind == HitKind::Meeting && h.meeting_id == Some(m.id))
    );
}

#[test]
fn echo_cancel_report_round_trips_and_shows_in_detail() {
    let s = Store::open_in_memory().unwrap();
    let m = meeting("Call", Utc::now());
    s.create_meeting(&m).unwrap();
    assert_eq!(s.echo_cancel(m.id).unwrap(), None);
    assert_eq!(s.meeting_detail(m.id).unwrap().unwrap().echo_cancel, None);

    let r = EchoCancelReport {
        ran: true,
        skipped: None,
        erle_db: Some(31.5),
        delay_ms: Some(42.0),
        drift_ppm: Some(-12.0),
        elapsed_ms: 900,
    };
    s.set_echo_cancel(m.id, &r).unwrap();
    assert_eq!(s.echo_cancel(m.id).unwrap(), Some(r));
    // A later run (e.g. retry with the setting off) replaces it.
    let off = EchoCancelReport::skipped(EchoSkip::Off);
    s.set_echo_cancel(m.id, &off).unwrap();
    assert_eq!(
        s.meeting_detail(m.id).unwrap().unwrap().echo_cancel,
        Some(off)
    );
    assert_eq!(s.echo_cancel(Uuid::new_v4()).unwrap(), None);
}
