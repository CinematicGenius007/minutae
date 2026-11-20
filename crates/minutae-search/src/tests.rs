use chrono::{Duration, TimeZone};
use minutae_core::{Meeting, MeetingStatus, Transcript};
use uuid::Uuid;

use super::*;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0).unwrap()
}

fn seg(start_s: u64, text: &str, track: Track, speaker: Option<SpeakerId>) -> Segment {
    Segment {
        start_ms: start_s * 1000,
        end_ms: start_s * 1000 + 4000,
        text: text.into(),
        track,
        speaker_id: speaker,
    }
}

fn add_meeting(
    store: &Store,
    title: &str,
    days_ago: i64,
    segments: Vec<Segment>,
    speakers: Vec<Speaker>,
) -> MeetingId {
    let id = speakers
        .first()
        .map(|s| s.meeting_id)
        .unwrap_or_else(Uuid::new_v4);
    store
        .create_meeting(&Meeting {
            id,
            title: title.into(),
            started_at: now() - Duration::days(days_ago),
            ended_at: None,
            status: MeetingStatus::Done,
            source_app: None,
            failure: None,
        })
        .unwrap();
    store
        .replace_transcript(
            &Transcript {
                meeting_id: id,
                language: None,
                model: "t".into(),
                segments,
            },
            &speakers,
        )
        .unwrap();
    id
}

#[test]
fn keyword_extraction_drops_stopwords() {
    assert_eq!(
        keywords("What did we decide about the Q3 pricing?"),
        vec!["decide", "q3", "pricing"]
    );
    assert_eq!(keywords("pricing PRICING pricing"), vec!["pricing"]);
    assert!(keywords("what was said?").is_empty());
}

#[test]
fn chunks_are_windowed_with_speaker_labels() {
    let mid = Uuid::new_v4();
    let alice = Speaker {
        id: Uuid::new_v4(),
        meeting_id: mid,
        label: "Speaker 1".into(),
        display_name: Some("Alice".into()),
        track: Track::System,
    };
    let bob = Speaker {
        id: Uuid::new_v4(),
        meeting_id: mid,
        label: "Speaker 2".into(),
        display_name: None,
        track: Track::System,
    };
    let segs = vec![
        seg(0, "Hello.", Track::Mic, None),
        seg(5, "Hi, quick update.", Track::System, Some(alice.id)),
        seg(10, "Pricing is done.", Track::System, Some(alice.id)),
        seg(50, "Next window.", Track::System, Some(bob.id)),
        seg(95, "Third.", Track::System, None),
    ];
    let w = chunk_segments(&segs, &[alice, bob], 45_000);
    assert_eq!(w.len(), 3);
    assert_eq!(
        w[0].text,
        "You: Hello.\nAlice: Hi, quick update. Pricing is done."
    );
    assert_eq!((w[0].start_ms, w[0].end_ms), (0, 14_000));
    assert_eq!(w[1].text, "Speaker 2: Next window.");
    assert_eq!(w[1].start_ms, 45_000);
    assert_eq!(w[2].text, "Them: Third.");
}

#[test]
fn retrieves_relevant_windows_ranked_by_bm25_and_recency() {
    let store = Store::open_in_memory().unwrap();
    let mid = Uuid::new_v4();
    let alice = Speaker {
        id: Uuid::new_v4(),
        meeting_id: mid,
        label: "Speaker 1".into(),
        display_name: Some("Alice".into()),
        track: Track::System,
    };
    let recent = add_meeting(
        &store,
        "Pricing sync",
        1,
        vec![
            seg(
                0,
                "Let's talk about the enterprise pricing tiers.",
                Track::System,
                Some(alice.id),
            ),
            seg(
                6,
                "I think pricing should go up ten percent for enterprise.",
                Track::Mic,
                None,
            ),
            seg(120, "Unrelated chat about lunch.", Track::Mic, None),
        ],
        vec![alice],
    );
    let old = add_meeting(
        &store,
        "Old pricing",
        200,
        vec![seg(
            0,
            "We debated enterprise pricing at length, pricing pricing.",
            Track::System,
            None,
        )],
        vec![],
    );
    add_meeting(
        &store,
        "Hiring",
        2,
        vec![seg(0, "We need two more designers.", Track::Mic, None)],
        vec![],
    );

    let r = Retriever::new(store.clone());
    let chunks = r
        .retrieve_at("What did we decide about enterprise pricing?", 5, now())
        .unwrap();
    assert!(chunks.len() >= 2);
    assert_eq!(chunks[0].meeting_id, recent);
    assert_eq!(chunks[0].title, "Pricing sync");
    assert_eq!(chunks[0].start_ms, 0);
    assert!(chunks[0].text.starts_with("Alice: Let's talk"));
    assert!(chunks[0].text.contains("You: I think pricing"));
    assert!(chunks.iter().any(|c| c.meeting_id == old));
    assert!(chunks.iter().all(|c| !c.text.contains("designers")));
    assert!(chunks.iter().all(|c| !c.text.contains("lunch")));

    // `since` filter and k.
    let r2 = Retriever::new(store.clone()).with_options(RetrieveOptions {
        since: Some(now() - Duration::days(30)),
        ..Default::default()
    });
    let c2 = r2.retrieve_at("enterprise pricing", 10, now()).unwrap();
    assert!(c2.iter().all(|c| c.meeting_id != old));
    assert_eq!(
        r.retrieve_at("enterprise pricing", 1, now()).unwrap().len(),
        1
    );
    // Nothing searchable.
    assert!(r.retrieve("what was said?", 5).unwrap().is_empty());
    // OR semantics: one matching keyword is enough.
    assert!(
        !r.retrieve_at("designers xylophone", 5, now())
            .unwrap()
            .is_empty()
    );
}

struct FakeEmbedder;

impl Embedder for FakeEmbedder {
    /// 2-d "embedding": [mentions food (lunch/food), mentions pricing].
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, SearchError> {
        Ok(texts
            .iter()
            .map(|t| {
                let t = t.to_lowercase();
                vec![
                    (t.contains("lunch") || t.contains("food")) as u8 as f32,
                    t.contains("pricing") as u8 as f32 + 0.01,
                ]
            })
            .collect())
    }
}

#[test]
fn embedder_reranks_candidates() {
    let store = Store::open_in_memory().unwrap();
    add_meeting(
        &store,
        "A",
        1,
        vec![seg(0, "budget budget budget pricing", Track::Mic, None)],
        vec![],
    );
    add_meeting(
        &store,
        "B",
        1,
        vec![seg(0, "budget for lunch", Track::Mic, None)],
        vec![],
    );
    // "food" never appears in a transcript: keyword ranking favours A (more "budget").
    let kw = Retriever::new(store.clone())
        .retrieve_at("budget food", 2, now())
        .unwrap();
    assert_eq!(kw.len(), 2);
    assert_eq!(kw[0].title, "A");
    // The embedder knows food ≈ lunch and lifts B.
    let sem = Retriever::new(store.clone())
        .with_embedder(Arc::new(FakeEmbedder))
        .with_options(RetrieveOptions {
            semantic_weight: 0.9,
            ..Default::default()
        })
        .retrieve_at("budget food", 2, now())
        .unwrap();
    assert_eq!(sem[0].title, "B");
}
