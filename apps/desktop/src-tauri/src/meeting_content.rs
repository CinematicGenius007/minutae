//! Meeting-owned files and the content ledger. File work runs on the blocking pool.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use minutae_core::{AppSettings, Meeting, MeetingId, MeetingStatus, Summary, paths};
use minutae_jobs::{JobKind, JobState};
use minutae_store::{ActionChange, ActionItemRow, Store, StoreError, TrackFile};
use tauri::{AppHandle, Manager, State};

use crate::commands::uuid;
use crate::dto::{
    ActionItem, ActionItemInput, MeetingContents, MeetingDeletion, MeetingPart, MeetingPartFailure,
    MeetingPartInfo,
};
use crate::pipeline::{find_summary_file, summary_file_name};
use crate::state::{AppState, emit_meetings_changed, recordings_root, summaries_root};

mod files;
pub use files::{LocalMeetingFiles, MeetingFiles, audio_sizes, summary_belongs_to};
use files::{present, unsafe_path};

type R<T> = Result<T, String>;

const PARTS: [MeetingPart; 4] = [
    MeetingPart::Audio,
    MeetingPart::Transcript,
    MeetingPart::Summary,
    MeetingPart::Markdown,
];

pub fn markdown_path(
    store: &Store,
    dir: &Path,
    id: MeetingId,
) -> minutae_store::Result<Option<PathBuf>> {
    let tracked = store.meeting_markdown_file(id)?.map(PathBuf::from);
    Ok(tracked
        .or_else(|| find_summary_file(dir, id))
        .or_else(|| find_summary_file(&paths::summaries_dir(), id)))
}

fn render_actions(markdown: &str, actions: &[ActionItemRow]) -> String {
    let heading = "## Action Items";
    let mut result = markdown.to_string();
    if let Some(start) = markdown
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some((start, line))
        })
        .find_map(|(offset, line)| {
            (line.trim_end_matches(['\r', '\n']) == heading).then_some(offset)
        })
    {
        let end = markdown[start + heading.len()..]
            .find("\n## ")
            .map_or(markdown.len(), |offset| start + heading.len() + offset + 1);
        result.replace_range(start..end, "");
    }
    if actions.is_empty() {
        return result.trim_end().to_string() + "\n";
    }
    result.truncate(result.trim_end().len());
    result.push_str("\n\n## Action Items\n");
    for row in actions {
        let check = if row.item.done { 'x' } else { ' ' };
        let label = minutae_llm::summary::checklist_text(&row.item);
        result.push_str(&format!(
            "- [{check}] {}",
            label.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
        if let Some(ms) = row.source_ms {
            result.push_str(&format!(" · {}:{:02}", ms / 60_000, ms / 1000 % 60));
        }
        result.push('\n');
    }
    result
}

fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(unsafe_path)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Leaves the old export in place until a complete replacement is ready, and
/// restores it if the SQLite transaction cannot commit.
fn publish_checklist<T>(
    path: Option<&Path>,
    id: MeetingId,
    update: impl FnOnce(&mut minutae_store::PublishChecklist<'_>) -> minutae_store::Result<T>,
) -> minutae_store::Result<T> {
    let io_error =
        |e: io::Error| StoreError::Invalid(format!("The summary Markdown couldn't be saved: {e}"));
    let mut backup = None;
    let outcome = update(&mut |markdown, actions| {
        let rendered = render_actions(markdown, actions);
        if let Some(path) = path {
            let previous = present(fs::symlink_metadata(path)).map_err(io_error)?;
            let bytes = if previous.is_some() {
                if !summary_belongs_to(path, id).map_err(io_error)? {
                    return Err(io_error(unsafe_path()));
                }
                Some(fs::read(path).map_err(io_error)?)
            } else {
                None
            };
            atomic_write(path, rendered.as_bytes()).map_err(io_error)?;
            backup = Some(bytes);
        }
        Ok(rendered)
    });
    if outcome.is_err()
        && let (Some(path), Some(backup)) = (path, backup)
    {
        let restore = match backup {
            Some(bytes) => atomic_write(path, &bytes),
            None => fs::remove_file(path),
        };
        if let Err(e) = restore {
            return Err(io_error(e));
        }
    }
    outcome
}

pub fn publish_summary(
    store: &Store,
    meeting: &Meeting,
    summary: &Summary,
    title: Option<&str>,
    dir: &Path,
) -> minutae_store::Result<()> {
    let path = markdown_path(store, dir, meeting.id)?
        .unwrap_or_else(|| dir.join(summary_file_name(meeting)));
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| StoreError::Invalid("Invalid summary folder.".into()))?,
    )
    .map_err(|e| StoreError::Invalid(format!("The summary folder couldn't be created: {e}")))?;
    publish_checklist(Some(&path), meeting.id, |publish| {
        store.publish_summary(summary, title, &path.to_string_lossy(), publish)
    })
}

async fn change_action(
    app: &AppHandle,
    id: MeetingId,
    change: ActionChange,
) -> R<Option<ActionItem>> {
    let st = app.state::<AppState>();
    let _operation = st.meeting_ops.lock(id).await;
    let dir = summaries_root(&st.settings());
    let row = st
        .store
        .call(move |store| {
            let path = markdown_path(store, &dir, id)?;
            // If the user deleted the export, editing the checklist only updates SQLite.
            let path = path.as_deref().filter(|p| match fs::symlink_metadata(p) {
                Err(e) => e.kind() != io::ErrorKind::NotFound,
                Ok(_) => true,
            });
            publish_checklist(path, id, |publish| store.change_action(id, change, publish))
        })
        .await
        .map_err(|e| e.to_string())?;
    emit_meetings_changed(app, Some(id));
    Ok(row.as_ref().map(ActionItem::from))
}

fn action_input(item: ActionItemInput) -> R<(minutae_core::ActionItem, Option<u64>)> {
    let single_line = |v: &str| v.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = single_line(&item.text);
    if text.is_empty() {
        return Err("Enter action item text.".into());
    }
    if item.source_ms.is_some_and(|ms| ms > 9_007_199_254_740_991) {
        return Err("The action item timestamp is invalid.".into());
    }
    Ok((
        minutae_core::ActionItem {
            text,
            owner: item
                .owner
                .map(|v| single_line(&v))
                .filter(|v| !v.is_empty()),
            due: item.due.map(|v| single_line(&v)).filter(|v| !v.is_empty()),
            done: false,
        },
        item.source_ms,
    ))
}

fn action_id(id: &str) -> R<i64> {
    id.parse().map_err(|_| "Invalid action item id.".into())
}

#[tauri::command]
pub async fn add_action_item(app: AppHandle, id: String, item: ActionItemInput) -> R<ActionItem> {
    let (item, source) = action_input(item)?;
    change_action(&app, uuid(&id)?, ActionChange::Add(item, source))
        .await?
        .ok_or_else(|| "The action item couldn't be added.".into())
}

#[tauri::command]
pub async fn edit_action_item(
    app: AppHandle,
    id: String,
    item_id: String,
    item: ActionItemInput,
) -> R<ActionItem> {
    let (item, source) = action_input(item)?;
    change_action(
        &app,
        uuid(&id)?,
        ActionChange::Edit(action_id(&item_id)?, item, source),
    )
    .await?
    .ok_or_else(|| "The action item couldn't be edited.".into())
}

#[tauri::command]
pub async fn delete_action_item(app: AppHandle, id: String, item_id: String) -> R<()> {
    change_action(&app, uuid(&id)?, ActionChange::Delete(action_id(&item_id)?))
        .await
        .map(|_| ())
}

pub async fn set_action_done(app: &AppHandle, id: &str, done: bool) -> R<()> {
    let row = action_id(id)?;
    let mid = app
        .state::<AppState>()
        .store
        .call(move |s| s.action_item_meeting(row))
        .await
        .map_err(|e| e.to_string())?;
    change_action(app, mid, ActionChange::SetDone(row, done))
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn summarize_again(app: AppHandle, id: String) -> R<()> {
    let id = uuid(&id)?;
    let st = app.state::<AppState>();
    let _operation = st.meeting_ops.lock(id).await;
    let can_start = st
        .store
        .call(move |s| {
            if !s
                .get_meeting(id)?
                .is_some_and(|m| m.status == MeetingStatus::Done)
            {
                return Err(StoreError::Invalid(
                    "Only finished meetings can be summarized again.".into(),
                ));
            }
            if s.get_transcript(id)?.is_none() {
                return Err(StoreError::Invalid(
                    "A saved transcript is required to summarize again.".into(),
                ));
            }
            Ok(!s
                .jobs_for_meeting(id)?
                .iter()
                .any(|j| matches!(j.state, JobState::Queued | JobState::Running)))
        })
        .await
        .map_err(|e| e.to_string())?;
    if can_start {
        let runner = st.runner()?;
        runner
            .enqueue(
                JobKind::Summarize,
                serde_json::json!({"refresh": true}),
                Some(id),
            )
            .await
            .map_err(|e| e.to_string())?;
        st.store
            .call(move |s| s.set_meeting_status(id, MeetingStatus::Done, None))
            .await
            .map_err(|e| e.to_string())?;
        emit_meetings_changed(&app, Some(id));
    }
    Ok(())
}

fn audio_directories(
    id: MeetingId,
    tracks: &[TrackFile],
    settings: &AppSettings,
) -> (BTreeSet<PathBuf>, Option<String>) {
    let mut dirs = BTreeSet::from([paths::recordings_dir().join(id.to_string())]);
    let mut reason = None;
    // A custom recording folder is relevant only when it actually has this meeting.
    let configured = recordings_root(settings).join(id.to_string());
    if !matches!(fs::symlink_metadata(&configured), Err(e) if e.kind() == io::ErrorKind::NotFound) {
        dirs.insert(configured);
    }
    for track in tracks {
        let path = Path::new(&track.path);
        let name = match track.track {
            minutae_core::Track::Mic => "mic.wav",
            minutae_core::Track::System => "system.wav",
        };
        match path.parent() {
            Some(dir)
                if dir.ends_with(id.to_string()) && path.file_name().is_some_and(|n| n == name) =>
            {
                dirs.insert(dir.to_path_buf());
            }
            _ => {
                reason = Some(
                    "A stored audio path is outside the meeting's own recording folder.".into(),
                )
            }
        }
    }
    (dirs, reason)
}

fn disk_part(
    part: MeetingPart,
    measured: io::Result<Option<u64>>,
    blocked_reason: Option<String>,
) -> MeetingPartInfo {
    match measured {
        Ok(bytes) => MeetingPartInfo {
            part,
            present: bytes.is_some() || blocked_reason.is_some(),
            bytes: bytes
                .or(Some(0))
                .filter(|_| blocked_reason.is_none() || bytes.is_some()),
            blocked_reason,
        },
        Err(error) => MeetingPartInfo {
            part,
            present: true,
            bytes: None,
            blocked_reason: blocked_reason.or_else(|| Some(error.to_string())),
        },
    }
}

fn db_part(part: MeetingPart, present: bool, bytes: u64) -> MeetingPartInfo {
    MeetingPartInfo {
        part,
        present,
        bytes: Some(bytes),
        blocked_reason: None,
    }
}

struct Ledger {
    contents: MeetingContents,
    audio_dirs: BTreeSet<PathBuf>,
    markdown: Option<PathBuf>,
}

fn ledger(
    store: &Store,
    files: &dyn MeetingFiles,
    settings: &AppSettings,
    id: MeetingId,
) -> R<Ledger> {
    let detail = store
        .meeting_detail(id)
        .map_err(|e| e.to_string())?
        .ok_or("This meeting no longer exists.")?;
    let (audio_dirs, mut audio_blocked) = audio_directories(id, &detail.tracks, settings);
    let mut total = None;
    let mut audio_error = None;
    let mut existing_audio_dirs = BTreeSet::new();
    for dir in audio_dirs {
        match files.audio_bytes(&dir) {
            Ok(Some(bytes)) => total = Some(total.unwrap_or(0) + bytes),
            Ok(None) => continue,
            Err(e) => audio_error = Some(e),
        }
        if let Err(e) = files.check_path(&dir) {
            audio_blocked = Some(e.to_string());
        }
        existing_audio_dirs.insert(dir);
    }
    let audio = disk_part(
        MeetingPart::Audio,
        audio_error.map_or(Ok(total), Err),
        audio_blocked,
    );
    let markdown =
        markdown_path(store, &summaries_root(settings), id).map_err(|e| e.to_string())?;
    let md = match &markdown {
        Some(path) => {
            let bytes = files.markdown_bytes(path);
            let blocked = if matches!(bytes, Ok(None)) {
                None
            } else {
                files
                    .check_path(path.parent().ok_or("Invalid Markdown path.")?)
                    .err()
                    .map(|e| e.to_string())
                    .or_else(|| {
                        (!summary_belongs_to(path, id).unwrap_or(false))
                            .then(|| "The Markdown file does not identify this meeting.".into())
                    })
            };
            disk_part(MeetingPart::Markdown, bytes, blocked)
        }
        None => db_part(MeetingPart::Markdown, false, 0),
    };
    let segments = detail
        .transcript
        .as_ref()
        .map_or(&[][..], |t| t.segments.as_slice());
    let transcript_bytes = detail.transcript.as_ref().map_or(0, |t| {
        t.model.len()
            + t.language.as_ref().map_or(0, String::len)
            + segments.iter().map(|s| s.text.len()).sum::<usize>()
            + detail
                .speakers
                .iter()
                .map(|s| {
                    s.display_name
                        .as_ref()
                        .map_or(s.label.len(), |name| s.label.len() + name.len())
                })
                .sum::<usize>()
    }) as u64;
    let summary_bytes = detail.summary.as_ref().map_or(0, |s| {
        s.markdown.len()
            + s.overview.len()
            + s.takeaway.len()
            + s.provider.len()
            + s.model.len()
            + s.decisions.iter().map(String::len).sum::<usize>()
    }) + detail
        .action_items
        .iter()
        .map(|a| {
            a.item.text.len()
                + a.item.owner.as_ref().map_or(0, String::len)
                + a.item.due.as_ref().map_or(0, String::len)
        })
        .sum::<usize>()
        + detail.moments.iter().map(|m| m.label.len()).sum::<usize>();
    let transcript = db_part(
        MeetingPart::Transcript,
        detail.transcript.is_some(),
        transcript_bytes,
    );
    let summary = db_part(
        MeetingPart::Summary,
        detail.summary.is_some() || !detail.action_items.is_empty() || !detail.moments.is_empty(),
        summary_bytes as u64,
    );
    let duration_ms = detail
        .tracks
        .iter()
        .filter_map(|t| t.duration_ms)
        .max()
        .or_else(|| {
            detail
                .meeting
                .ended_at
                .map(|end| (end - detail.meeting.started_at).num_milliseconds().max(0) as u64)
        });
    Ok(Ledger {
        contents: MeetingContents {
            duration_ms,
            track_count: detail
                .tracks
                .iter()
                .filter(|track| fs::metadata(&track.path).is_ok_and(|m| m.is_file()))
                .count(),
            line_count: segments.len(),
            speaker_count: detail.speakers.len(),
            transcript_excerpt: segments
                .first()
                .map(|s| minutae_llm::summary::shorten(&s.text, 120)),
            decision_count: detail.summary.as_ref().map_or(0, |s| s.decisions.len()),
            action_count: detail.action_items.len(),
            mark_count: detail.moments.len(),
            summary_excerpt: detail
                .summary
                .as_ref()
                .map(|s| minutae_llm::summary::shorten(&s.overview, 120)),
            markdown_path: md
                .present
                .then(|| markdown.as_ref().map(|p| p.to_string_lossy().into_owned()))
                .flatten(),
            parts: vec![audio, transcript, summary, md],
        },
        audio_dirs: existing_audio_dirs,
        markdown,
    })
}

fn delete_parts(
    store: &Store,
    files: &dyn MeetingFiles,
    settings: &AppSettings,
    id: MeetingId,
    parts: &[MeetingPart],
) -> R<MeetingDeletion> {
    use MeetingPart::{Audio, Markdown, Summary, Transcript};
    if parts.is_empty() {
        return Err("Select at least one part to delete.".into());
    }
    let meeting = store
        .get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or("This meeting no longer exists.")?;
    if meeting.status == MeetingStatus::Recording {
        return Err("Use Discard to delete a live recording.".into());
    }
    let before = ledger(store, files, settings, id)?;
    let mut deleted = Vec::new();
    let mut failures = Vec::new();
    // When a job owns the meeting lock, this command waits for it. Any already
    // claimed successor checks its persisted row after acquiring the lock.
    let removing_content = before.contents.parts.iter().any(|entry| {
        parts.contains(&entry.part) && entry.present && entry.blocked_reason.is_none()
    });
    let empty = before.contents.parts.iter().all(|entry| !entry.present);
    if (removing_content || empty)
        && (matches!(
            meeting.status,
            MeetingStatus::Processing | MeetingStatus::Failed
        ) || store
            .jobs_for_meeting(id)
            .map_err(|e| e.to_string())?
            .iter()
            .any(|j| !j.state.is_terminal()))
    {
        store
            .settle_meeting_for_deletion(id)
            .map_err(|e| e.to_string())?;
    }
    for info in &before.contents.parts {
        let part = info.part;
        if !parts.contains(&part) || !info.present {
            continue;
        }
        if let Some(reason) = &info.blocked_reason {
            failures.push(MeetingPartFailure {
                part,
                reason: reason.clone(),
            });
            continue;
        }
        let disk_result = match part {
            Audio => {
                let mut problems = Vec::new();
                for dir in &before.audio_dirs {
                    if let Err(e) = files.delete_audio(dir, id) {
                        problems.push(e.to_string());
                    }
                }
                if problems.is_empty() {
                    Ok(())
                } else {
                    Err(problems.join(". "))
                }
            }
            Markdown => before
                .markdown
                .as_deref()
                .map_or(Ok(()), |path| files.delete_markdown(path, id))
                .map_err(|e| e.to_string()),
            _ => Ok(()),
        };
        let result = disk_result.and_then(|()| {
            if part == Markdown {
                return Ok(());
            }
            store
                .trim_meeting(id, part == Audio, part == Transcript, part == Summary)
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => deleted.push(part),
            Err(reason) => failures.push(MeetingPartFailure { part, reason }),
        }
    }
    let after = ledger(store, files, settings, id)?.contents;
    let meeting_removed = failures.is_empty() && after.parts.iter().all(|part| !part.present);
    if meeting_removed {
        store.delete_meeting(id).map_err(|e| e.to_string())?;
    }
    Ok(MeetingDeletion {
        deleted,
        failures,
        meeting_removed,
        remaining: (!meeting_removed).then_some(after),
    })
}

#[tauri::command]
pub async fn meeting_contents(state: State<'_, AppState>, id: String) -> R<MeetingContents> {
    let id = uuid(&id)?;
    let settings = state.settings();
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        ledger(&store, &LocalMeetingFiles(paths::data_dir()), &settings, id).map(|l| l.contents)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_meeting_parts(
    app: AppHandle,
    id: String,
    parts: Vec<MeetingPart>,
) -> R<MeetingDeletion> {
    let id = uuid(&id)?;
    let st = app.state::<AppState>();
    let operations = st.meeting_ops.clone();
    let settings = st.settings();
    let store = st.store.clone();
    let result = tauri::async_runtime::spawn(async move {
        let operation = operations.lock(id).await;
        tokio::task::spawn_blocking(move || {
            let _operation = operation;
            delete_parts(
                &store,
                &LocalMeetingFiles(paths::data_dir()),
                &settings,
                id,
                &parts,
            )
        })
        .await
        .map_err(|e| e.to_string())?
    })
    .await
    .map_err(|e| e.to_string())?;
    emit_meetings_changed(&app, Some(id));
    result
}

pub async fn delete_entire_meeting(app: AppHandle, id: String) -> R<()> {
    let result = delete_meeting_parts(app, id, PARTS.to_vec()).await?;
    if result.meeting_removed {
        Ok(())
    } else {
        Err(result
            .failures
            .iter()
            .map(|f| f.reason.as_str())
            .collect::<Vec<_>>()
            .join(". "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minutae_core::{ActionItem as CoreAction, Segment, Track, Transcript};

    fn meeting(store: &Store, status: MeetingStatus) -> Meeting {
        let now = chrono::Utc::now();
        let meeting = Meeting {
            id: uuid::Uuid::new_v4(),
            title: "Planning".into(),
            started_at: now,
            ended_at: Some(now),
            status,
            source_app: None,
            failure: None,
        };
        store.create_meeting(&meeting).unwrap();
        meeting
    }

    fn summary(meeting: &Meeting, text: &str) -> Summary {
        Summary {
            meeting_id: meeting.id,
            overview: text.into(),
            takeaway: text.into(),
            decisions: vec!["Ship in May".into()],
            action_items: vec![CoreAction {
                text: "Send notes".into(),
                owner: None,
                due: None,
                done: false,
            }],
            markdown: format!("---\nid: {}\n---\n\n## Summary\n- {text}\n", meeting.id),
            provider: "Test".into(),
            model: "fake".into(),
        }
    }

    #[test]
    fn rerun_preserves_curated_rows_and_failed_publish_rolls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Old summary"),
            None,
            tmp.path(),
        )
        .unwrap();
        let path = markdown_path(&store, tmp.path(), id).unwrap().unwrap();
        let generated = store.list_action_items(id).unwrap().pop().unwrap();
        publish_checklist(Some(&path), id, |publish| {
            store.change_action(id, ActionChange::Delete(generated.id), publish)
        })
        .unwrap();
        let manual = CoreAction {
            text: "Send the handoff".into(),
            owner: Some("Alice".into()),
            due: Some("Friday".into()),
            done: false,
        };
        let row = publish_checklist(Some(&path), id, |publish| {
            store.change_action(id, ActionChange::Add(manual, Some(724_000)), publish)
        })
        .unwrap()
        .unwrap();

        let old_markdown = fs::read_to_string(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(
            publish_summary(
                &store,
                &meeting,
                &summary(&meeting, "Failed rerun"),
                None,
                tmp.path()
            )
            .is_err()
        );
        let saved = store.get_summary(id).unwrap().unwrap();
        assert_eq!(saved.overview, "Old summary");
        assert_eq!(saved.markdown, old_markdown);
        fs::remove_dir(&path).unwrap();
        fs::write(&path, old_markdown).unwrap();

        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "New summary"),
            None,
            tmp.path(),
        )
        .unwrap();
        assert_eq!(store.list_action_items(id).unwrap(), vec![row]);
        let exported = fs::read_to_string(&path).unwrap();
        assert!(exported.contains("New summary"));
        assert!(exported.contains("Alice: Send the handoff (due Friday) · 12:04"));
        assert!(!exported.contains("Send notes"));
        assert_eq!(store.get_summary(id).unwrap().unwrap().markdown, exported);
    }

    struct FailingAudio;
    impl MeetingFiles for FailingAudio {
        fn audio_bytes(&self, _: &Path) -> io::Result<Option<u64>> {
            Ok(Some(120))
        }
        fn delete_audio(&self, _: &Path, _: MeetingId) -> io::Result<()> {
            Err(io::Error::other("audio is busy"))
        }
        fn markdown_bytes(&self, _: &Path) -> io::Result<Option<u64>> {
            Ok(None)
        }
        fn delete_markdown(&self, _: &Path, _: MeetingId) -> io::Result<()> {
            Ok(())
        }
        fn check_path(&self, _: &Path) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn partial_delete_reports_failure_keeps_summary_and_retires_stale_jobs() {
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Processing);
        let id = meeting.id;
        store.upsert_summary(&summary(&meeting, "Saved")).unwrap();
        store
            .replace_transcript(
                &Transcript {
                    meeting_id: id,
                    language: None,
                    model: "fake".into(),
                    segments: vec![Segment {
                        start_ms: 0,
                        end_ms: 500,
                        text: "May".into(),
                        track: Track::Mic,
                        speaker_id: None,
                    }],
                },
                &[],
            )
            .unwrap();
        let job = store
            .insert_job(&minutae_jobs::NewJob::new(
                JobKind::Summarize,
                serde_json::json!({}),
                Some(id),
            ))
            .unwrap();
        let result = delete_parts(
            &store,
            &FailingAudio,
            &AppSettings::default(),
            id,
            &[MeetingPart::Audio, MeetingPart::Transcript],
        )
        .unwrap();
        assert_eq!(result.deleted, [MeetingPart::Transcript]);
        assert_eq!(result.failures[0].part, MeetingPart::Audio);
        assert!(!result.meeting_removed);
        assert!(store.get_job(job.id).unwrap().is_none());
        assert!(store.get_transcript(id).unwrap().is_none());
        assert!(store.get_summary(id).unwrap().is_some());
        assert_eq!(
            store.get_meeting(id).unwrap().unwrap().status,
            MeetingStatus::Done
        );
    }

    #[test]
    fn markdown_can_outlive_summary_and_the_last_part_removes_the_meeting() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let settings = AppSettings {
            recordings_path: Some(root.join("recordings").to_string_lossy().into_owned()),
            summaries_path: Some(root.join("summaries").to_string_lossy().into_owned()),
            ..AppSettings::default()
        };
        let files = LocalMeetingFiles(root.to_path_buf());
        let dir = summaries_root(&settings);
        publish_summary(&store, &meeting, &summary(&meeting, "Saved"), None, &dir).unwrap();
        let path = markdown_path(&store, &dir, id).unwrap().unwrap();
        let first = delete_parts(&store, &files, &settings, id, &[MeetingPart::Summary]).unwrap();
        assert_eq!(first.deleted, [MeetingPart::Summary]);
        assert!(!first.meeting_removed);
        assert!(path.is_file());
        assert_eq!(
            first.remaining.unwrap().parts[3].bytes,
            Some(fs::metadata(&path).unwrap().len())
        );
        let last = delete_parts(&store, &files, &settings, id, &[MeetingPart::Markdown]).unwrap();
        assert_eq!(last.deleted, [MeetingPart::Markdown]);
        assert!(last.meeting_removed);
        assert!(!path.exists());
        assert!(store.get_meeting(id).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn owned_audio_deletion_never_follows_symlinks() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let external = outside.path().join(id.to_string());
        fs::create_dir(&external).unwrap();
        let victim = external.join("mic.wav");
        fs::write(&victim, b"keep").unwrap();
        let files = LocalMeetingFiles(tmp.path().to_path_buf());
        assert!(files.delete_audio(&external, id).is_err());

        let path = tmp.path().join(id.to_string());
        symlink(&external, &path).unwrap();
        assert!(files.delete_audio(&path, id).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        symlink(&victim, path.join("mic.wav")).unwrap();
        fs::write(path.join("system.wav.tmp"), b"leftover audio").unwrap();
        files.delete_audio(&path, id).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(victim).unwrap(), b"keep");
    }
}
