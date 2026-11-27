//! Legacy rows → v4 rows.
//!
//! | Legacy | v4 |
//! |---|---|
//! | `meetings` (ISO-8601 text times, nanoid or UUID id) | `meetings`; the UUID is kept when free, else a new one |
//! | status `done` / `failed` / `recording` `paused` `processing` | `done` / `failed` / `failed` ("hadn't finished") |
//! | newest `recordings.file_path` (one mixed file) | copied to `<recordings>/<id>/mic.<ext>`, `tracks` row `mic` |
//! | newest non-empty `transcripts` + `segments` (+ Swift `transcript_edits`) | `transcripts`, `segments` (track `mic`) |
//! | `segments.speaker_label` | one `speakers` row per distinct label (track `mic`) |
//! | `transcripts.raw_text` without segments | one segment spanning the meeting |
//! | newest `summaries.markdown_content` | `summaries` (takeaway, overview, decisions parsed from it) |
//! | `action_items` (fallback: `- [ ]` lines in the summary) | `action_items` |
//! | Swift `tags` / `item_tags` | `legacy_imports.extra` `{"tags": [...]}` (v4 has no tags yet) |
//! | Swift `dictations` with status `done` | `dictations` (edited text wins) |
//! | `embeddings`, `api_usage_log`, `jobs`, `settings`, `dictionary` | not imported |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use minutae_core::{
    ActionItem, Dictation, Failure, FailureKind, Meeting, MeetingStatus, Segment, Speaker, Summary,
    Track, Transcript,
};
use minutae_store::rusqlite::types::Value;
use minutae_store::{ImportedMeeting, Store, TrackFile};
use uuid::Uuid;

use crate::legacy::{LDictation, LMeeting, LegacyDb, int, text};
use crate::{ImportResult, Issue, Source};

/// Parse a legacy timestamp: RFC 3339 (both apps), SQLite `datetime()` text, or epoch
/// seconds / milliseconds.
pub fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    if let Value::Integer(_) | Value::Real(_) = v {
        return epoch(int(v.clone())?);
    }
    let s = text(v.clone())?;
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc));
    }
    for f in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, f) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    s.parse::<i64>().ok().and_then(epoch)
}

fn epoch(n: i64) -> Option<DateTime<Utc>> {
    if n <= 0 {
        return None;
    }
    if n > 100_000_000_000 {
        Utc.timestamp_millis_opt(n).single()
    } else {
        Utc.timestamp_opt(n, 0).single()
    }
}

/// Legacy audio path → a file that exists: the stored path, else the same file name in one
/// of the source's audio folders.
pub fn resolve_audio(stored: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let p = Path::new(stored);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }
    let name = p.file_name()?;
    dirs.iter().map(|d| d.join(name)).find(|c| c.is_file())
}

fn ms(t: DateTime<Utc>) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(t.timestamp_millis())
        .single()
        .unwrap_or(t)
}

/// Pick the v4 id: keep a legacy UUID (Swift ids, so `<id>.wav` names still line up) unless
/// something in v4 already uses it.
fn choose_id(store: &Store, legacy_id: &str) -> Uuid {
    if let Ok(u) = Uuid::parse_str(legacy_id)
        && !store.id_in_use(u).unwrap_or(true)
    {
        return u;
    }
    loop {
        let u = Uuid::new_v4();
        if !store.id_in_use(u).unwrap_or(false) {
            return u;
        }
    }
}

fn fail(out: &mut ImportResult, src: &Source, m: &LMeeting, reason: impl Into<String>) {
    out.failed += 1;
    out.issues.push(Issue {
        source: src.kind,
        legacy_id: m.id.clone(),
        title: m.title.clone(),
        reason: reason.into(),
        failed: true,
    });
}

fn warn(out: &mut ImportResult, src: &Source, m: &LMeeting, reason: impl Into<String>) {
    out.issues.push(Issue {
        source: src.kind,
        legacy_id: m.id.clone(),
        title: m.title.clone(),
        reason: reason.into(),
        failed: false,
    });
}

/// Copy legacy audio to `<root>/<id>/mic.<ext>`. Returns the track row and bytes copied.
fn copy_audio(src_file: &Path, root: &Path, id: Uuid) -> std::io::Result<(TrackFile, u64)> {
    let dir = root.join(id.to_string());
    std::fs::create_dir_all(&dir)?;
    let ext = src_file
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "wav".into());
    let dest = dir.join(format!("mic.{ext}"));
    let bytes = std::fs::copy(src_file, &dest)?;
    Ok((
        TrackFile {
            meeting_id: id,
            track: Track::Mic,
            path: dest.to_string_lossy().into_owned(),
            duration_ms: crate::wav::duration_ms(&dest),
            bytes: Some(bytes),
            deleted: false,
        },
        bytes,
    ))
}

pub(crate) fn import_meeting(
    store: &Store,
    db: &LegacyDb,
    src: &Source,
    m: &LMeeting,
    recordings_root: &Path,
    out: &mut ImportResult,
) {
    let read = (|| {
        Ok::<_, minutae_store::rusqlite::Error>((
            db.recording(&m.id)?,
            db.transcript(&m.id)?,
            db.summary(&m.id)?,
            db.action_items(&m.id)?,
            db.tags("meeting", &m.id)?,
        ))
    })();
    let (recording, transcript, summary, items, tags) = match read {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!(error = %e, legacy_id = %m.id, "reading a legacy meeting failed");
            return fail(out, src, m, "Its rows couldn't be read.");
        }
    };
    let started = m
        .started_at
        .as_ref()
        .and_then(parse_time)
        .or_else(|| recording.as_ref()?.created_at.as_ref().and_then(parse_time));
    let Some(started) = started.map(ms) else {
        return fail(out, src, m, "It has no readable start time.");
    };
    let id = choose_id(store, &m.id);

    // Transcript first: its length is a fallback for the meeting's end.
    let segments_raw = match &transcript {
        Some(t) => match db.segments(&t.id) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "reading legacy segments failed");
                warn(out, src, m, "Its transcript segments couldn't be read.");
                vec![]
            }
        },
        None => vec![],
    };

    let mut ended = m.ended_at.as_ref().and_then(parse_time).map(ms);
    let transcript_ms = transcript
        .as_ref()
        .and_then(|t| t.duration_ms)
        .filter(|d| *d > 0)
        .or_else(|| segments_raw.last().map(|s| s.end_ms));
    if ended.is_none_or(|e| e < started) {
        ended = m
            .duration_seconds
            .filter(|d| *d > 0)
            .map(|d| d * 1000)
            .or(transcript_ms)
            .map(|d| started + chrono::Duration::milliseconds(d));
    }

    // Speakers: one per distinct label, in first-seen order.
    let mut speakers: Vec<Speaker> = vec![];
    let mut by_label: BTreeMap<String, Uuid> = BTreeMap::new();
    let mut segments: Vec<Segment> = vec![];
    for s in &segments_raw {
        let speaker_id = s.speaker_label.as_ref().map(|label| {
            *by_label.entry(label.clone()).or_insert_with(|| {
                let sid = Uuid::new_v4();
                speakers.push(Speaker {
                    id: sid,
                    meeting_id: id,
                    label: label.clone(),
                    display_name: None,
                    track: Track::Mic,
                });
                sid
            })
        });
        segments.push(Segment {
            start_ms: s.start_ms as u64,
            end_ms: s.end_ms as u64,
            text: s.text.trim().to_string(),
            track: Track::Mic,
            speaker_id,
        });
    }
    let transcript = transcript.map(|t| {
        if segments.is_empty() && !t.raw_text.trim().is_empty() {
            let end = transcript_ms
                .or_else(|| ended.map(|e| (e - started).num_milliseconds()))
                .unwrap_or(0)
                .max(0) as u64;
            segments.push(Segment {
                start_ms: 0,
                end_ms: end,
                text: t.raw_text.trim().to_string(),
                track: Track::Mic,
                speaker_id: None,
            });
        }
        Transcript {
            meeting_id: id,
            language: t.language.clone(),
            model: t.provider.clone().unwrap_or_else(|| "whisper".into()),
            segments: std::mem::take(&mut segments),
        }
    });

    let summary = summary.map(|s| {
        let parsed = crate::markdown::parse(&s.markdown);
        let action_items = if items.is_empty() {
            parsed.action_items.clone()
        } else {
            items.clone()
        }
        .into_iter()
        .map(|(text, done)| ActionItem {
            text,
            owner: None,
            due: None,
            done,
        })
        .collect();
        Summary {
            meeting_id: id,
            overview: parsed.overview,
            decisions: parsed.decisions,
            action_items,
            takeaway: parsed.takeaway,
            markdown: s.markdown.clone(),
            provider: s.provider.clone().unwrap_or_else(|| "imported".into()),
            model: s.model.clone().unwrap_or_default(),
        }
    });

    let legacy_status = m.status.as_deref().unwrap_or("done");
    let (status, failure) = match legacy_status {
        "done" => (MeetingStatus::Done, None),
        "failed" => (
            MeetingStatus::Failed,
            Some(Failure {
                kind: FailureKind::Unavailable,
                message: "Processing failed in the older Minutae app.".into(),
                fix: None,
            }),
        ),
        _ if summary.is_some() => (MeetingStatus::Done, None),
        _ => (
            MeetingStatus::Failed,
            Some(Failure {
                kind: FailureKind::Unavailable,
                message: "The older Minutae app hadn't finished this meeting.".into(),
                fix: None,
            }),
        ),
    };

    // Audio: copy before the database write; undo the copy if the write fails.
    let mut tracks = vec![];
    let mut copied = 0u64;
    if let Some(rec) = &recording {
        match resolve_audio(&rec.file_path, &src.audio_dirs) {
            Some(file) => match copy_audio(&file, recordings_root, id) {
                Ok((t, n)) => {
                    tracks.push(t);
                    copied = n;
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(recordings_root.join(id.to_string()));
                    return fail(out, src, m, format!("Its audio couldn't be copied: {e}"));
                }
            },
            None => warn(out, src, m, "Imported without audio: the file is missing."),
        }
    }

    let title = m
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled meeting")
        .to_string();
    let mut extra = serde_json::Map::new();
    if !tags.is_empty() {
        extra.insert("tags".into(), serde_json::json!(tags));
    }
    if legacy_status != "done" {
        extra.insert("legacyStatus".into(), serde_json::json!(legacy_status));
    }
    let bundle = ImportedMeeting {
        meeting: Meeting {
            id,
            title,
            started_at: started,
            ended_at: ended,
            status,
            source_app: None,
            failure,
        },
        tracks,
        transcript,
        speakers,
        summary,
        extra: (!extra.is_empty()).then_some(serde_json::Value::Object(extra)),
    };
    match store.import_legacy_meeting(src.kind.key(), &m.id, &bundle) {
        Ok(true) => {
            out.imported += 1;
            out.copied_bytes += copied;
        }
        Ok(false) => {
            out.skipped += 1;
            if copied > 0 {
                let _ = std::fs::remove_dir_all(recordings_root.join(id.to_string()));
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, legacy_id = %m.id, "writing an imported meeting failed");
            if copied > 0 {
                let _ = std::fs::remove_dir_all(recordings_root.join(id.to_string()));
            }
            fail(out, src, m, "It couldn't be saved to the new database.");
        }
    }
}

/// Dictations worth importing: finished, with text.
pub(crate) fn dictation_importable(d: &LDictation) -> bool {
    matches!(d.status.as_deref(), None | Some("done")) && !dictation_text(d).is_empty()
}

fn dictation_text(d: &LDictation) -> String {
    d.edited_text
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .or(Some(d.final_text.as_str()).filter(|t| !t.trim().is_empty()))
        .unwrap_or(&d.raw_text)
        .trim()
        .to_string()
}

pub(crate) fn import_dictation(
    store: &Store,
    db: &LegacyDb,
    src: &Source,
    d: &LDictation,
    out: &mut ImportResult,
) {
    let issue = |reason: &str| Issue {
        source: src.kind,
        legacy_id: d.id.clone(),
        title: None,
        reason: reason.into(),
        failed: true,
    };
    let Some(created) = d
        .started_at
        .as_ref()
        .and_then(parse_time)
        .or_else(|| d.created_at.as_ref().and_then(parse_time))
        .map(ms)
    else {
        out.failed += 1;
        out.issues.push(issue("A dictation has no readable time."));
        return;
    };
    let text = dictation_text(d);
    let raw = if d.raw_text.trim().is_empty() {
        text.clone()
    } else {
        d.raw_text.trim().to_string()
    };
    let id = choose_id(store, &d.id);
    let dictation = Dictation {
        app_identity: None,
        shape: None,
        id,
        created_at: created,
        word_count: text.split_whitespace().count() as u32,
        text,
        raw_text: raw,
        target_app: d.target_app.clone(),
        duration_ms: d.duration_ms.unwrap_or(0).max(0) as u64,
        inserted: matches!(
            d.insert_method.as_deref(),
            Some("pasteboard" | "accessibility" | "typing")
        ),
    };
    let tags = db.tags("dictation", &d.id).unwrap_or_default();
    let extra = (!tags.is_empty()).then(|| serde_json::json!({ "tags": tags }));
    match store.import_legacy_dictation(src.kind.key(), &d.id, &dictation, extra.as_ref()) {
        Ok(true) => out.dictations += 1,
        Ok(false) => out.skipped += 1,
        Err(e) => {
            tracing::warn!(error = %e, "writing an imported dictation failed");
            out.failed += 1;
            out.issues
                .push(issue("A dictation couldn't be saved to the new database."));
        }
    }
}
