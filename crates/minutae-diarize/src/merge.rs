//! Speaker attribution: combine transcripts with diarization turns (docs/CROSS-PLATFORM.md,
//! "Multi-speaker plan").
//!
//! Pure functions, no I/O.

use std::collections::HashMap;

use minutae_core::{MeetingId, Segment, Speaker, Track};
use uuid::Uuid;

use crate::SpeakerTurn;

/// Label for the mic track in two-track mode.
pub const YOU_LABEL: &str = "You";

/// A segment with no overlapping turn is given the nearest turn's speaker if that turn
/// is at most this far away (whisper and the diarizer disagree on boundaries by a few
/// hundred ms). Further away it stays unattributed (`speaker_id: None`).
pub const MAX_GAP_MS: u64 = 2_000;

/// A mic segment that overlaps system speech by more than this fraction of its length…
pub const BLEED_MIN_OVERLAP: f64 = 0.5;
/// …and whose words match that system speech at least this much (token Jaccard) is
/// the remote audio picked up by the mic (laptop speakers), not the user, and is dropped.
pub const BLEED_MIN_SIMILARITY: f64 = 0.6;

/// Two-track merge.
///
/// - `mic` segments that are just the remote side leaking into the mic (see
///   [`BLEED_MIN_OVERLAP`] / [`BLEED_MIN_SIMILARITY`]) are dropped;
/// - every other `mic` segment → speaker **"You"** (track `Mic`);
/// - every `system` segment → the diarized speaker with the most time overlap,
///   labelled "Speaker 1", "Speaker 2"… in order of first appearance;
/// - if `system_turns` is empty (diarization off/failed) all system segments go to
///   a single "Speaker 1" — right for 1:1 calls;
/// - output is sorted by start time (mic first on exact ties).
///
/// Only speakers that end up with at least one segment are returned.
pub fn merge(
    mic_segments: &[Segment],
    system_segments: &[Segment],
    system_turns: &[SpeakerTurn],
    meeting_id: MeetingId,
) -> (Vec<Segment>, Vec<Speaker>) {
    let mut speakers = Vec::new();
    let mut out: Vec<(Segment, Slot)> =
        Vec::with_capacity(mic_segments.len() + system_segments.len());

    out.extend(
        mic_segments
            .iter()
            .filter(|s| !is_bleed(s, system_segments))
            .map(|s| (with_track(s, Track::Mic), Slot::You)),
    );
    let raw = assign(system_segments, system_turns);
    out.extend(
        system_segments
            .iter()
            .zip(raw)
            .map(|(s, r)| (with_track(s, Track::System), r)),
    );
    sort(&mut out);

    let mut you: Option<Uuid> = None;
    let mut labels = Labeller::new(meeting_id, Track::System);
    let segments = out
        .into_iter()
        .map(|(mut seg, slot)| {
            seg.speaker_id = match slot {
                Slot::You => Some(*you.get_or_insert_with(|| {
                    let id = Uuid::new_v4();
                    speakers.push(Speaker {
                        id,
                        meeting_id,
                        label: YOU_LABEL.into(),
                        display_name: None,
                        track: Track::Mic,
                    });
                    id
                })),
                Slot::Diarized(raw) => Some(labels.id_for(raw, &mut speakers)),
                Slot::Unknown => None,
            };
            seg
        })
        .collect();
    (segments, speakers)
}

/// Single-track (in-room) merge: one mic recording with several people in it.
/// No "You": every speaker comes from diarization ("Speaker 1"…, track `Mic`).
/// With no turns at all, everything goes to "Speaker 1".
pub fn merge_single_track(
    segments: &[Segment],
    turns: &[SpeakerTurn],
    meeting_id: MeetingId,
) -> (Vec<Segment>, Vec<Speaker>) {
    let track = segments.first().map(|s| s.track).unwrap_or(Track::Mic);
    let raw = assign(segments, turns);
    let mut out: Vec<(Segment, Slot)> = segments.iter().cloned().zip(raw).collect();
    sort(&mut out);
    let mut speakers = Vec::new();
    let mut labels = Labeller::new(meeting_id, track);
    let segments = out
        .into_iter()
        .map(|(mut seg, slot)| {
            seg.speaker_id = match slot {
                Slot::Diarized(raw) => Some(labels.id_for(raw, &mut speakers)),
                Slot::You | Slot::Unknown => None,
            };
            seg
        })
        .collect();
    (segments, speakers)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    You,
    /// Raw diarizer speaker index (not yet relabelled).
    Diarized(u32),
    Unknown,
}

/// True if `mic` is mostly a copy of what the system track was saying at the same time.
fn is_bleed(mic: &Segment, system: &[Segment]) -> bool {
    let dur = mic.end_ms.saturating_sub(mic.start_ms);
    let mut overlapping = 0u64;
    let mut text = String::new();
    for s in system {
        let o = overlap(mic.start_ms, mic.end_ms, s.start_ms, s.end_ms);
        // A zero-length mic segment "overlaps" a system segment that contains it.
        let touches = o > 0 || (dur == 0 && s.start_ms <= mic.start_ms && mic.start_ms < s.end_ms);
        if touches {
            overlapping += o;
            text.push(' ');
            text.push_str(&s.text);
        }
    }
    let overlap_ok = if dur == 0 {
        !text.is_empty()
    } else {
        overlapping as f64 > BLEED_MIN_OVERLAP * dur as f64
    };
    overlap_ok && jaccard(&mic.text, &text) >= BLEED_MIN_SIMILARITY
}

/// Lower-cased alphanumeric word set.
fn tokens(text: &str) -> std::collections::HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Token-set Jaccard similarity, 0.0 when either side has no words.
fn jaccard(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count();
    inter as f64 / (a.len() + b.len() - inter) as f64
}

fn with_track(s: &Segment, track: Track) -> Segment {
    Segment {
        track,
        speaker_id: None,
        ..s.clone()
    }
}

fn sort(v: &mut [(Segment, Slot)]) {
    // Stable: equal keys keep input order (mic before system).
    v.sort_by_key(|(s, _)| (s.start_ms, s.end_ms, s.track == Track::System));
}

fn overlap(a0: u64, a1: u64, b0: u64, b1: u64) -> u64 {
    a1.min(b1).saturating_sub(a0.max(b0))
}

/// Raw speaker per segment by maximum total overlap.
/// Ties → the speaker whose overlapping speech starts earliest within the segment.
fn assign(segments: &[Segment], turns: &[SpeakerTurn]) -> Vec<Slot> {
    if turns.is_empty() {
        return vec![Slot::Diarized(0); segments.len()];
    }
    segments
        .iter()
        .map(|seg| {
            // speaker -> (total overlap, earliest overlap start)
            let mut acc: HashMap<u32, (u64, u64)> = HashMap::new();
            for t in turns {
                let o = overlap(seg.start_ms, seg.end_ms, t.start_ms, t.end_ms);
                if o > 0 {
                    let e = acc.entry(t.speaker).or_insert((0, u64::MAX));
                    e.0 += o;
                    e.1 = e.1.min(t.start_ms.max(seg.start_ms));
                }
            }
            if let Some((spk, _)) = acc.into_iter().max_by(|(sa, (oa, fa)), (sb, (ob, fb))| {
                oa.cmp(ob).then(fb.cmp(fa)).then(sb.cmp(sa))
            }) {
                return Slot::Diarized(spk);
            }
            // No overlap (zero-length segment or a gap): nearest turn within MAX_GAP_MS.
            turns
                .iter()
                .map(|t| {
                    // Distance between the intervals (0 if they touch/overlap).
                    let gap = seg
                        .start_ms
                        .saturating_sub(t.end_ms)
                        .max(t.start_ms.saturating_sub(seg.end_ms));
                    (gap, t.start_ms, t.speaker)
                })
                .min()
                .filter(|(gap, _, _)| *gap <= MAX_GAP_MS)
                .map(|(_, _, s)| Slot::Diarized(s))
                .unwrap_or(Slot::Unknown)
        })
        .collect()
}

/// Maps raw diarizer ids → "Speaker N" in order of first appearance.
struct Labeller {
    meeting_id: MeetingId,
    track: Track,
    ids: HashMap<u32, Uuid>,
}

impl Labeller {
    fn new(meeting_id: MeetingId, track: Track) -> Self {
        Self {
            meeting_id,
            track,
            ids: HashMap::new(),
        }
    }

    fn id_for(&mut self, raw: u32, speakers: &mut Vec<Speaker>) -> Uuid {
        if let Some(id) = self.ids.get(&raw) {
            return *id;
        }
        let id = Uuid::new_v4();
        let n = self.ids.len() + 1;
        self.ids.insert(raw, id);
        speakers.push(Speaker {
            id,
            meeting_id: self.meeting_id,
            label: format!("Speaker {n}"),
            display_name: None,
            track: self.track,
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: u64, end: u64, text: &str, track: Track) -> Segment {
        Segment {
            start_ms: start,
            end_ms: end,
            text: text.into(),
            track,
            speaker_id: None,
        }
    }
    fn turn(start: u64, end: u64, speaker: u32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms: start,
            end_ms: end,
            speaker,
        }
    }
    fn label_of<'a>(s: &Segment, speakers: &'a [Speaker]) -> Option<&'a str> {
        s.speaker_id
            .map(|id| speakers.iter().find(|p| p.id == id).unwrap().label.as_str())
    }
    fn labels(segs: &[Segment], speakers: &[Speaker]) -> Vec<Option<String>> {
        segs.iter()
            .map(|s| label_of(s, speakers).map(str::to_string))
            .collect()
    }
    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    #[test]
    fn two_track_interleaves_and_labels_by_first_appearance() {
        let m = Uuid::new_v4();
        let mic = [
            seg(0, 2_000, "hi all", Track::Mic),
            seg(9_000, 10_000, "thanks", Track::Mic),
        ];
        let sys = [
            seg(2_500, 4_000, "hello", Track::System),
            seg(4_200, 6_000, "hey there", Track::System),
            seg(6_500, 8_000, "me again", Track::System),
        ];
        // Raw diarizer ids deliberately not in appearance order.
        let turns = [
            turn(2_400, 4_100, 7),
            turn(4_150, 6_100, 3),
            turn(6_400, 8_100, 7),
        ];
        let (out, speakers) = merge(&mic, &sys, &turns, m);
        assert_eq!(
            out.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["hi all", "hello", "hey there", "me again", "thanks"]
        );
        assert_eq!(
            labels(&out, &speakers),
            [
                s("You"),
                s("Speaker 1"),
                s("Speaker 2"),
                s("Speaker 1"),
                s("You")
            ]
        );
        assert_eq!(speakers.len(), 3);
        assert!(speakers.iter().all(|p| p.meeting_id == m));
        let you = speakers.iter().find(|p| p.label == "You").unwrap();
        assert_eq!(you.track, Track::Mic);
        assert!(
            speakers
                .iter()
                .filter(|p| p.label != "You")
                .all(|p| p.track == Track::System)
        );
        assert_eq!(out[0].track, Track::Mic);
        assert_eq!(out[1].track, Track::System);
    }

    #[test]
    fn maximum_overlap_wins_across_multiple_turns() {
        // Segment 0..10s; speaker 0 has 3s+3s=6s total, speaker 1 has one 4s turn.
        let sys = [seg(0, 10_000, "long", Track::System)];
        let turns = [
            turn(0, 3_000, 0),
            turn(3_000, 7_000, 1),
            turn(7_000, 10_000, 0),
        ];
        let (out, speakers) = merge(&[], &sys, &turns, Uuid::new_v4());
        assert_eq!(speakers.len(), 1);
        // The winning raw speaker 0 is the first (and only) one to appear → "Speaker 1".
        assert_eq!(label_of(&out[0], &speakers), Some("Speaker 1"));
        let (out2, sp2) = merge(
            &[seg(0, 1, "x", Track::Mic)],
            &[sys[0].clone(), seg(3_100, 6_900, "mid", Track::System)],
            &turns,
            Uuid::new_v4(),
        );
        assert_eq!(
            labels(&out2, &sp2),
            [s("You"), s("Speaker 1"), s("Speaker 2")]
        );
    }

    #[test]
    fn tie_goes_to_speaker_who_started_first_in_segment() {
        let sys = [seg(1_000, 3_000, "tie", Track::System)];
        // Both overlap exactly 1s; speaker 5 starts at 2_000, speaker 9 at 1_000.
        let turns = [turn(2_000, 4_000, 5), turn(0, 2_000, 9)];
        let (out, speakers) = merge(&[], &sys, &turns, Uuid::new_v4());
        assert_eq!(speakers.len(), 1);
        // Speaker 9 should win; verify via a second segment that is clearly speaker 5.
        let sys2 = [sys[0].clone(), seg(3_100, 3_900, "five", Track::System)];
        let (out2, sp2) = merge(&[], &sys2, &turns, Uuid::new_v4());
        assert_eq!(labels(&out2, &sp2), [s("Speaker 1"), s("Speaker 2")]);
        assert_ne!(out2[0].speaker_id, out2[1].speaker_id);
        assert!(out[0].speaker_id.is_some());
    }

    #[test]
    fn exact_tie_same_start_is_deterministic() {
        let sys = [
            seg(0, 2_000, "x", Track::System),
            seg(5_000, 6_000, "y", Track::System),
        ];
        let turns = [
            turn(0, 1_000, 4),
            turn(1_000, 2_000, 2),
            turn(5_000, 6_000, 2),
        ];
        // Overlap equal (1s each), speaker 4 starts first → seg x = raw 4, seg y = raw 2.
        for _ in 0..20 {
            let (out, sp) = merge(&[], &sys, &turns, Uuid::new_v4());
            assert_eq!(labels(&out, &sp), [s("Speaker 1"), s("Speaker 2")]);
        }
    }

    #[test]
    fn gaps_use_nearest_turn_within_limit() {
        let sys = [
            seg(5_000, 5_500, "near A", Track::System), // 500ms after A ends
            seg(8_600, 9_000, "near B", Track::System), // 600ms before B starts
            seg(20_000, 21_000, "far", Track::System),  // > MAX_GAP_MS from everything
        ];
        let turns = [turn(0, 4_500, 0), turn(9_600, 12_000, 1)];
        let (out, sp) = merge(&[], &sys, &turns, Uuid::new_v4());
        assert_eq!(labels(&out, &sp), [s("Speaker 1"), s("Speaker 2"), None]);
        assert_eq!(sp.len(), 2);
    }

    #[test]
    fn no_turns_means_single_remote_speaker() {
        let mic = [seg(1_000, 2_000, "me", Track::Mic)];
        let sys = [
            seg(0, 900, "them", Track::System),
            seg(3_000, 4_000, "them again", Track::System),
        ];
        let (out, sp) = merge(&mic, &sys, &[], Uuid::new_v4());
        assert_eq!(
            labels(&out, &sp),
            [s("Speaker 1"), s("You"), s("Speaker 1")]
        );
        assert_eq!(sp.len(), 2);
    }

    #[test]
    fn empty_inputs() {
        let (out, sp) = merge(&[], &[], &[turn(0, 10, 0)], Uuid::new_v4());
        assert!(out.is_empty() && sp.is_empty());
        let (out, sp) = merge(&[seg(0, 1, "a", Track::Mic)], &[], &[], Uuid::new_v4());
        assert_eq!(out.len(), 1);
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].label, "You");
    }

    #[test]
    fn mic_only_speakers_never_include_unused_diarized_ids() {
        let (_, sp) = merge(
            &[seg(0, 1_000, "a", Track::Mic)],
            &[],
            &[turn(0, 1_000, 0), turn(1_000, 2_000, 1)],
            Uuid::new_v4(),
        );
        assert_eq!(
            sp.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
            ["You"]
        );
    }

    #[test]
    fn sorts_unsorted_input_and_mic_first_on_ties() {
        let mic = [
            seg(5_000, 6_000, "m2", Track::Mic),
            seg(1_000, 2_000, "m1", Track::Mic),
        ];
        let sys = [
            seg(1_000, 2_000, "s1", Track::System),
            seg(0, 500, "s0", Track::System),
        ];
        let (out, _) = merge(&mic, &sys, &[turn(0, 2_000, 0)], Uuid::new_v4());
        assert_eq!(
            out.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["s0", "m1", "s1", "m2"]
        );
    }

    #[test]
    fn overwrites_incoming_track_and_speaker() {
        let stray = Some(Uuid::new_v4());
        let mut m = seg(0, 1_000, "m", Track::System);
        m.speaker_id = stray;
        let (out, sp) = merge(&[m], &[], &[], Uuid::new_v4());
        assert_eq!(out[0].track, Track::Mic);
        assert_eq!(out[0].speaker_id, Some(sp[0].id));
    }

    #[test]
    fn zero_length_segment_uses_containing_turn() {
        let (out, sp) = merge(
            &[],
            &[seg(1_500, 1_500, "blip", Track::System)],
            &[turn(1_000, 2_000, 3)],
            Uuid::new_v4(),
        );
        assert_eq!(label_of(&out[0], &sp), Some("Speaker 1"));
    }

    #[test]
    fn jaccard_similarity() {
        assert_eq!(jaccard("Hello, world!", "hello WORLD"), 1.0);
        assert_eq!(jaccard("a b c d", "a b"), 0.5);
        assert_eq!(jaccard("", "x"), 0.0);
        assert_eq!(jaccard("don't stop", "Don't stop."), 1.0);
    }

    #[test]
    fn mic_bleed_of_remote_speech_is_dropped() {
        let sys = [
            seg(
                1_000,
                4_000,
                "Let's ship the beta on Friday.",
                Track::System,
            ),
            seg(4_000, 6_000, "Any objections?", Track::System),
        ];
        let mic = [
            // Speakers leaking into the mic: same words, same time (slightly off).
            seg(1_200, 4_100, "let's ship the beta on friday", Track::Mic),
            // The user really answering while overlapping: different words.
            seg(4_500, 5_500, "No, sounds good to me", Track::Mic),
            // Similar words but at a different time: a genuine echo by the user.
            seg(8_000, 9_000, "ship the beta on Friday", Track::Mic),
            // Spans both system segments: compared against their joined text.
            seg(
                1_000,
                6_000,
                "let's ship the beta on friday any objections",
                Track::Mic,
            ),
        ];
        let (out, sp) = merge(&mic, &sys, &[], Uuid::new_v4());
        assert_eq!(
            out.iter()
                .map(|s| (s.text.as_str(), label_of(s, &sp).unwrap()))
                .collect::<Vec<_>>(),
            [
                ("Let's ship the beta on Friday.", "Speaker 1"),
                ("Any objections?", "Speaker 1"),
                ("No, sounds good to me", "You"),
                ("ship the beta on Friday", "You"),
            ]
        );
    }

    #[test]
    fn mic_with_little_overlap_is_kept_even_if_similar() {
        // Same words, but only 1 s of the 3 s mic segment overlaps (33% < 50%).
        let sys = [seg(0, 1_000, "okay thanks everyone", Track::System)];
        let mic = [seg(0, 3_000, "okay thanks everyone", Track::Mic)];
        let (out, _) = merge(&mic, &sys, &[], Uuid::new_v4());
        assert_eq!(out.len(), 2);
        // Zero-length mic segment inside a matching system segment: bleed.
        let mic = [seg(500, 500, "okay thanks everyone", Track::Mic)];
        let (out, _) = merge(&mic, &sys, &[], Uuid::new_v4());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].track, Track::System);
    }

    #[test]
    fn single_track_in_room_mode() {
        let segs = [
            seg(0, 2_000, "a", Track::Mic),
            seg(2_000, 4_000, "b", Track::Mic),
            seg(4_000, 6_000, "c", Track::Mic),
        ];
        let turns = [
            turn(0, 2_100, 1),
            turn(2_100, 4_000, 0),
            turn(4_000, 6_000, 1),
        ];
        let (out, sp) = merge_single_track(&segs, &turns, Uuid::new_v4());
        assert_eq!(
            labels(&out, &sp),
            [s("Speaker 1"), s("Speaker 2"), s("Speaker 1")]
        );
        assert!(
            sp.iter()
                .all(|p| p.track == Track::Mic && p.label != YOU_LABEL)
        );
        let (out, sp) = merge_single_track(&segs, &[], Uuid::new_v4());
        assert_eq!(sp.len(), 1);
        assert!(out.iter().all(|s| s.speaker_id == Some(sp[0].id)));
    }
}
