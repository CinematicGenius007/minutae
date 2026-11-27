//! Meeting-owned files and the content ledger. File work runs on the blocking pool.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use minutae_core::{AppSettings, MeetingId, MeetingStatus, paths};
use minutae_jobs::{JobKind, JobState};
use minutae_store::{ActionChange, Store, StoreError, TrackFile};
use tauri::{AppHandle, Manager, State};

use crate::commands::uuid;
use crate::dto::{
    ActionItem, ActionItemInput, MeetingContents, MeetingDeletion, MeetingPart, MeetingPartFailure,
    MeetingPartInfo,
};
use crate::state::{AppState, emit_meetings_changed, recordings_root, summaries_root};

mod erase;
mod files;
mod publication;
#[cfg(test)]
pub(crate) use erase::erase_meeting_files;
pub(crate) use erase::{erase_meetings, recover_erased_files};
pub use files::{LocalMeetingFiles, MeetingFiles, read_export, summary_belongs_to};
use publication::publish_checklist;
pub use publication::publish_summary;

type R<T> = Result<T, String>;

const PARTS: [MeetingPart; 4] = [
    MeetingPart::Audio,
    MeetingPart::Transcript,
    MeetingPart::Summary,
    MeetingPart::Markdown,
];

const MARKDOWN_NOT_FOUND: &str = "No Markdown export was found in Minutae’s known folders.";

pub fn markdown_path(
    store: &Store,
    dir: &Path,
    id: MeetingId,
) -> minutae_store::Result<Option<PathBuf>> {
    saved_markdown_path(store, &LocalMeetingFiles, dir, id)
}

fn known_roots(
    store: &Store,
    kind: &str,
    current: &Path,
    default: PathBuf,
) -> minutae_store::Result<BTreeSet<PathBuf>> {
    let mut roots = BTreeSet::from([current.to_path_buf(), default]);
    roots.extend(store.storage_folders(kind)?.into_iter().map(PathBuf::from));
    Ok(roots)
}

fn known_markdown_paths(
    store: &Store,
    files: &LocalMeetingFiles,
    roots: &BTreeSet<PathBuf>,
    id: MeetingId,
) -> minutae_store::Result<BTreeSet<PathBuf>> {
    let mut paths: BTreeSet<_> = store
        .meeting_exports(id)?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let pending: BTreeSet<_> = store
        .pending_summary_scans(id)?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    for root in roots {
        let Ok(found) = files.find_markdowns(root, id) else {
            continue;
        };
        for path in found {
            if paths.insert(path.clone()) {
                store.remember_meeting_export(id, &path.to_string_lossy())?;
            }
        }
        if pending.contains(root) {
            store.finish_summary_scan(id, &root.to_string_lossy())?;
        }
    }
    Ok(paths)
}

fn saved_markdown_path(
    store: &Store,
    files: &LocalMeetingFiles,
    dir: &Path,
    id: MeetingId,
) -> minutae_store::Result<Option<PathBuf>> {
    let primary = store.meeting_markdown_file(id)?.map(PathBuf::from);
    if let Some(path) = &primary
        && files
            .markdown_bytes(path, id)
            .is_ok_and(|size| size.is_some())
    {
        return Ok(primary);
    }
    let roots = known_roots(store, "summaries", dir, paths::summaries_dir())?;
    let paths = known_markdown_paths(store, files, &roots, id)?;
    Ok(paths
        .into_iter()
        .find(|path| {
            files
                .markdown_bytes(path, id)
                .is_ok_and(|size| size.is_some())
        })
        .or(primary))
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
            publish_checklist(path.as_deref(), id, |publish| {
                store.change_action(id, change, publish)
            })
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

#[derive(Default)]
struct AudioLocation {
    required: bool,
    imported: Option<String>,
}

fn audio_directories(
    id: MeetingId,
    tracks: &[TrackFile],
    folders: &ContentFolders,
    imported: bool,
    removed: bool,
) -> (BTreeMap<PathBuf, AudioLocation>, Option<String>) {
    let name = id.to_string();
    let default = folders.data.join("recordings");
    let mut dirs = BTreeMap::new();
    if !removed {
        dirs.insert(default.join(&name), AudioLocation::default());
        // Without a track row, chunks may still live in the configured folder.
        dirs.insert(
            folders.recordings.join(&name),
            AudioLocation {
                required: folders.recordings != default && tracks.is_empty(),
                imported: None,
            },
        );
    }
    let mut reason = None;
    for track in tracks.iter().filter(|t| !t.deleted) {
        if let Some((dir, extra)) = stored_track_path(track, id, imported) {
            let entry = dirs.entry(dir.to_path_buf()).or_default();
            entry.required = true;
            if let Some(extra) = extra {
                entry.imported = Some(extra.to_owned());
            }
        } else {
            reason = Some("A stored audio path is outside the meeting's own recording folder or has an unverified name.".into());
        }
    }
    (dirs, reason)
}

fn stored_track_path(
    track: &TrackFile,
    id: MeetingId,
    imported: bool,
) -> Option<(&Path, Option<&str>)> {
    let path = Path::new(&track.path);
    let directory = path.parent()?;
    if directory.file_name()?.to_str()? != id.to_string() {
        return None;
    }
    let file = path.file_name()?.to_str()?;
    let name = match track.track {
        minutae_core::Track::Mic => "mic.wav",
        minutae_core::Track::System => "system.wav",
    };
    if file == name {
        return Some((directory, None));
    }
    let extension = file.strip_prefix("mic.")?;
    (imported
        && track.track == minutae_core::Track::Mic
        && !extension.is_empty()
        && !extension.contains('.')
        && extension == extension.to_ascii_lowercase())
    .then_some((directory, Some(file)))
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
    audio_dirs: BTreeMap<PathBuf, AudioLocation>,
    markdown: BTreeSet<PathBuf>,
}

pub(crate) struct ContentFolders {
    data: PathBuf,
    recordings: PathBuf,
    summaries: PathBuf,
}

impl ContentFolders {
    pub(crate) fn current(settings: &AppSettings) -> Self {
        Self {
            data: paths::data_dir(),
            recordings: recordings_root(settings),
            summaries: summaries_root(settings),
        }
    }
}

fn ledger(
    store: &Store,
    files: &dyn MeetingFiles,
    folders: &ContentFolders,
    id: MeetingId,
) -> R<Ledger> {
    let detail = store
        .meeting_detail(id)
        .map_err(|e| e.to_string())?
        .ok_or("This meeting no longer exists.")?;
    let (audio_removed, markdown_removed) =
        store.meeting_storage_state(id).map_err(|e| e.to_string())?;
    let imported = store.is_imported_meeting(id).map_err(|e| e.to_string())?;
    let (audio_dirs, audio_blocked) =
        audio_directories(id, &detail.tracks, folders, imported, audio_removed);
    let mut total = None;
    let mut audio_error = None;
    let mut existing_audio_dirs = BTreeMap::new();
    for (dir, entry) in audio_dirs {
        match files.audio_bytes(&dir, id, entry.required, entry.imported.as_deref()) {
            Ok(Some(bytes)) => total = Some(total.unwrap_or(0) + bytes),
            Ok(None) => continue,
            Err(e) => audio_error = Some(e),
        }
        existing_audio_dirs.insert(dir, entry);
    }
    let audio = disk_part(
        MeetingPart::Audio,
        audio_error.map_or(Ok(total), Err),
        audio_blocked,
    );

    let roots = known_roots(
        store,
        "summaries",
        &folders.summaries,
        folders.data.join("summaries"),
    )
    .map_err(|e| e.to_string())?;
    let markdown =
        known_markdown_paths(store, &LocalMeetingFiles, &roots, id).map_err(|e| e.to_string())?;
    let mut markdown_path = None;
    let mut markdown_bytes = None;
    let mut markdown_error = None;
    for path in &markdown {
        match files.markdown_bytes(path, id) {
            Ok(Some(bytes)) => {
                markdown_bytes = Some(markdown_bytes.unwrap_or(0) + bytes);
                markdown_path.get_or_insert(path);
            }
            Ok(None) => {}
            Err(e) => {
                markdown_error = Some(e);
            }
        }
    }
    if markdown.is_empty()
        && !markdown_removed
        && folders.summaries != folders.data.join("summaries")
        && let Err(error) = LocalMeetingFiles.find_markdowns(&folders.summaries, id)
    {
        markdown_error = Some(error);
    }
    let pending = store.pending_summary_scans(id).map_err(|e| e.to_string())?;
    if let Some(root) = pending.first() {
        markdown_error = Some(io::Error::other(format!(
            "The storage folder ‘{root}’ was unavailable when Minutae looked for this meeting’s export."
        )));
    }
    let md = disk_part(
        MeetingPart::Markdown,
        markdown_error.map_or(Ok(markdown_bytes), Err),
        None,
    );
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
                .filter(|track| !track.deleted && files.track_exists(Path::new(&track.path), id))
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
            markdown_path: markdown_path.map(|p| p.to_string_lossy().into_owned()),
            markdown_notice: (!md.present).then_some(MARKDOWN_NOT_FOUND),
            parts: vec![audio, transcript, summary, md],
        },
        audio_dirs: existing_audio_dirs,
        markdown,
    })
}

fn delete_parts(
    store: &Store,
    files: &dyn MeetingFiles,
    folders: &ContentFolders,
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
    let before = ledger(store, files, folders, id)?;
    let mut deleted = Vec::new();
    let mut failures = Vec::new();
    // When a job owns the meeting lock, this command waits for it. Any already
    // claimed successor checks its persisted row after acquiring the lock.
    let removing_content = before.contents.parts.iter().any(|entry| {
        parts.contains(&entry.part) && entry.present && entry.blocked_reason.is_none()
    });
    if removing_content || before.contents.parts.iter().all(|entry| !entry.present) {
        store
            .settle_meeting_for_deletion(id)
            .map_err(|e| e.to_string())?;
    }
    for info in &before.contents.parts {
        let part = info.part;
        if !parts.contains(&part) {
            continue;
        }
        let mut problems = Vec::new();
        match part {
            Audio if info.blocked_reason.is_none() => {
                for (dir, entry) in &before.audio_dirs {
                    if let Err(e) = files.delete_audio(dir, id, entry.imported.as_deref()) {
                        problems.push(e.to_string());
                    }
                }
            }
            Markdown if !before.markdown.is_empty() => {
                for path in &before.markdown {
                    let result = files
                        .delete_markdown(path, id)
                        .map_err(|e| e.to_string())
                        .and_then(|()| {
                            store
                                .forget_meeting_export(id, &path.to_string_lossy())
                                .map_err(|e| e.to_string())
                        });
                    if let Err(error) = result {
                        problems.push(error);
                    }
                }
                if problems.is_empty()
                    && let Some(root) = store
                        .pending_summary_scans(id)
                        .map_err(|e| e.to_string())?
                        .first()
                {
                    problems.push(format!("The storage folder ‘{root}’ is unavailable."));
                }
            }
            _ => problems.extend(info.blocked_reason.clone()),
        }
        if problems.is_empty() && (info.present || matches!(part, Audio | Markdown)) {
            let result = if part == Markdown {
                store.mark_exports_removed(id)
            } else {
                store.trim_meeting(id, part == Audio, part == Transcript, part == Summary)
            };
            if let Err(e) = result {
                problems.push(e.to_string());
            }
        }
        if !problems.is_empty() {
            failures.push(MeetingPartFailure {
                part,
                reason: problems.join(". "),
            });
        } else if info.present {
            deleted.push(part);
        }
    }
    let after = ledger(store, files, folders, id)?.contents;
    let meeting_removed = failures.is_empty() && after.parts.iter().all(|part| !part.present);
    if meeting_removed {
        store.delete_meeting(id).map_err(|e| e.to_string())?;
    }
    Ok(MeetingDeletion {
        deleted,
        failures,
        meeting_removed,
        remaining: (!meeting_removed).then_some(after),
        markdown_notice: parts
            .contains(&Markdown)
            .then_some(before.contents.markdown_notice)
            .flatten(),
    })
}

#[tauri::command]
pub async fn meeting_contents(state: State<'_, AppState>, id: String) -> R<MeetingContents> {
    let id = uuid(&id)?;
    let operation = state.meeting_ops.lock(id).await;
    let folders = ContentFolders::current(&state.settings());
    let store = state.store.clone();
    tokio::task::spawn_blocking(move || {
        let _operation = operation;
        ledger(&store, &LocalMeetingFiles, &folders, id).map(|l| l.contents)
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
    let folders = ContentFolders::current(&st.settings());
    let store = st.store.clone();
    let result = tauri::async_runtime::spawn(async move {
        let operation = operations.lock(id).await;
        tokio::task::spawn_blocking(move || {
            let _operation = operation;
            delete_parts(&store, &LocalMeetingFiles, &folders, id, &parts)
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

/// Save verified paths as well as the folder itself. A folder that is offline
/// during a settings change is still searched when it becomes available.
pub(crate) async fn remember_storage(state: &AppState, settings: &AppSettings) -> R<()> {
    let folders = ContentFolders::current(settings);
    let scan = state
        .store
        .call(move |store| scan_storage(store, &folders))
        .await
        .map_err(|e| e.to_string())?;
    for (id, path) in scan {
        let operation = state.meeting_ops.lock(id).await;
        state
            .store
            .call(move |store| {
                let _operation = operation;
                if LocalMeetingFiles
                    .markdown_bytes(&path, id)
                    .is_ok_and(|size| size.is_some())
                {
                    store.remember_meeting_export(id, &path.to_string_lossy())?;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn scan_storage(
    store: &Store,
    folders: &ContentFolders,
) -> minutae_store::Result<Vec<(MeetingId, PathBuf)>> {
    store.remember_storage_folder("recordings", &folders.recordings.to_string_lossy())?;
    store.remember_storage_folder("summaries", &folders.summaries.to_string_lossy())?;
    let ids = store.meeting_ids()?;
    let default = folders.data.join("summaries");
    let mut verified = Vec::new();
    for root in BTreeSet::from([default.clone(), folders.summaries.clone()]) {
        match LocalMeetingFiles.scan_markdowns(&root, &ids) {
            Ok(found) => verified.extend(found),
            Err(_) if root != default => store.defer_summary_scan(&root.to_string_lossy())?,
            Err(_) => {}
        }
    }
    Ok(verified)
}

pub(crate) fn prune_audio(store: &Store, folders: &ContentFolders, id: MeetingId) -> R<()> {
    let tracks = store.list_tracks(id).map_err(|e| e.to_string())?;
    if tracks.iter().all(|t| t.deleted) {
        return Ok(());
    }
    let imported = store.is_imported_meeting(id).map_err(|e| e.to_string())?;
    let (dirs, invalid) = audio_directories(id, &tracks, folders, imported, false);
    if let Some(reason) = invalid {
        return Err(reason);
    }
    for (dir, entry) in dirs {
        let imported = entry.imported.as_deref();
        if LocalMeetingFiles
            .audio_bytes(&dir, id, entry.required, imported)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            LocalMeetingFiles
                .delete_audio(&dir, id, imported)
                .map_err(|e| e.to_string())?;
        }
    }
    store
        .trim_meeting(id, true, false, false)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use minutae_core::{ActionItem as CoreAction, Meeting, Segment, Summary, Track, Transcript};
    use std::fs;

    use super::erase::retry_cleanup;
    use crate::pipeline::summary_file_name;

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

    #[test]
    fn checklist_edit_preserves_handwritten_bytes_and_rerun_keeps_a_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            tmp.path(),
        )
        .unwrap();
        let path = markdown_path(&store, tmp.path(), id).unwrap().unwrap();
        let original = fs::read_to_string(&path).unwrap();
        let edited = original.replace("- Saved", "- Handwritten point")
            + "\n## Personal notes\n\nKeep this exactly.  \n";
        fs::write(&path, &edited).unwrap();
        let row = store.list_action_items(id).unwrap().pop().unwrap();
        publish_checklist(Some(&path), id, |publish| {
            store.change_action(id, ActionChange::SetDone(row.id, true), publish)
        })
        .unwrap();
        let ticked = edited.replace("- [ ] Send notes", "- [x] Send notes");
        assert_eq!(fs::read_to_string(&path).unwrap(), ticked);

        let ambiguous = ticked.clone() + "\n## Action Items\n- [ ] Duplicate\n";
        fs::write(&path, &ambiguous).unwrap();
        assert!(
            publish_checklist(Some(&path), id, |publish| {
                store.change_action(id, ActionChange::SetDone(row.id, false), publish)
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), ambiguous);
        assert!(store.list_action_items(id).unwrap()[0].item.done);

        fs::write(&path, &ticked).unwrap();
        for text in ["New summary", "Another summary"] {
            publish_summary(&store, &meeting, &summary(&meeting, text), None, tmp.path()).unwrap();
            assert!(fs::read_to_string(&path).unwrap().contains(text));
            assert!(
                fs::read_dir(tmp.path())
                    .unwrap()
                    .flatten()
                    .any(|entry| entry.path() != path
                        && fs::read_to_string(entry.path()).ok().as_deref() == Some(&ticked))
            );
        }
    }

    #[test]
    fn retired_failed_meeting_keeps_its_audio_retention_status() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let mut meeting = meeting(&store, MeetingStatus::Failed);
        meeting.started_at -= chrono::Duration::days(90);
        store.update_meeting(&meeting).unwrap();
        let id = meeting.id;
        let audio = saved_track(&store, id, &folders.recordings);
        store
            .replace_transcript(
                &Transcript {
                    meeting_id: id,
                    language: None,
                    model: "fake".into(),
                    segments: vec![],
                },
                &[],
            )
            .unwrap();
        let result =
            delete_parts(&store, &files, &folders, id, &[MeetingPart::Transcript]).unwrap();
        assert_eq!(result.deleted, [MeetingPart::Transcript]);
        assert!(!crate::retention_eligible(
            &store.get_meeting(id).unwrap().unwrap(),
            chrono::Utc::now()
        ));
        assert!(audio.exists());
    }

    #[test]
    fn markdown_with_open_or_duplicate_frontmatter_is_never_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let files = LocalMeetingFiles;
        let id = uuid::Uuid::new_v4();
        let path = tmp.path().join("summary.md");
        for bytes in [
            format!("---\nid: {id}\n"),
            format!("---\nid: {id}\nid: {id}\n---\n"),
            format!("---\nid: {id}\n\"id\": {id}\n---\n"),
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(files.delete_markdown(&path, id).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn deleted_export_can_be_replaced_in_current_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let mut folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            &folders.summaries,
        )
        .unwrap();
        assert_eq!(
            delete_parts(&store, &files, &folders, id, &[MeetingPart::Markdown])
                .unwrap()
                .deleted,
            [MeetingPart::Markdown]
        );
        let previous = folders.summaries;
        fs::remove_dir(&previous).unwrap();
        folders.summaries = tmp.path().join("new-summaries");
        assert_eq!(store.meeting_markdown_file(id).unwrap(), None);
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "New"),
            None,
            &folders.summaries,
        )
        .unwrap();
        assert!(!previous.exists());
        assert!(folders.summaries.join(summary_file_name(&meeting)).exists());
    }

    #[test]
    fn legacy_export_discovery_persists_the_verified_path() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let path = folders.summaries.join(summary_file_name(&meeting));
        fs::write(&path, summary(&meeting, "Legacy").markdown).unwrap();
        assert_eq!(
            saved_markdown_path(&store, &files, &folders.summaries, id).unwrap(),
            Some(path.clone())
        );
        assert_eq!(
            store.meeting_markdown_file(id).unwrap(),
            Some(path.to_string_lossy().into_owned())
        );
        assert_eq!(
            saved_markdown_path(&store, &files, &tmp.path().join("new"), id).unwrap(),
            Some(path)
        );
    }

    #[test]
    fn removed_tracks_do_not_require_their_old_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        store.upsert_summary(&summary(&meeting, "Saved")).unwrap();
        let old = tmp.path().join("old-recordings");
        saved_track(&store, id, &old);
        assert_eq!(
            delete_parts(&store, &files, &folders, id, &[MeetingPart::Audio])
                .unwrap()
                .deleted,
            [MeetingPart::Audio]
        );
        fs::remove_dir(&old).unwrap();
        assert!(
            delete_parts(&store, &files, &folders, id, &PARTS)
                .unwrap()
                .meeting_removed
        );
    }

    #[test]
    fn imported_track_owns_its_exact_extension_in_the_meeting_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let id = meeting(&store, MeetingStatus::Done).id;
        let dir = folders.recordings.join(id.to_string());
        fs::create_dir(&dir).unwrap();
        let path = dir.join("mic.m4a");
        fs::write(&path, b"imported").unwrap();
        let other = dir.join("mic.mp3");
        fs::write(&other, b"personal").unwrap();
        store
            .upsert_track(&TrackFile {
                meeting_id: id,
                track: Track::Mic,
                path: path.to_string_lossy().into_owned(),
                duration_ms: None,
                bytes: Some(8),
                deleted: false,
            })
            .unwrap();
        store
            .write(|c| {
                c.execute(
                    "INSERT INTO legacy_imports (source, kind, legacy_id, v4_id, imported_at)
                 VALUES ('swift', 'meeting', 'old', ?1, 0)",
                    [id.to_string()],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(
            delete_parts(&store, &files, &folders, id, &PARTS)
                .unwrap()
                .meeting_removed
        );
        assert!(!path.exists());
        assert_eq!(fs::read(other).unwrap(), b"personal");
    }

    #[cfg(unix)]
    #[test]
    fn data_and_default_storage_roots_may_be_symlinked() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let actual_data = tmp.path().join("actual-data");
        fs::create_dir(&actual_data).unwrap();
        symlink(&actual_data, &folders.data).unwrap();
        symlink(&folders.recordings, actual_data.join("recordings")).unwrap();
        symlink(&folders.summaries, actual_data.join("summaries")).unwrap();
        let audio = saved_track(&store, id, &folders.data.join("recordings"));
        let path = folders
            .data
            .join("summaries")
            .join(summary_file_name(&meeting));
        fs::write(&path, summary(&meeting, "Saved").markdown).unwrap();
        assert!(
            files
                .audio_bytes(audio.parent().unwrap(), id, true, None)
                .unwrap()
                .is_some()
        );
        assert!(files.markdown_bytes(&path, id).unwrap().is_some());
    }

    #[test]
    fn all_action_mutations_preserve_content_outside_the_checklist() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            tmp.path(),
        )
        .unwrap();
        let path = markdown_path(&store, tmp.path(), id).unwrap().unwrap();
        let initial = store.list_action_items(id).unwrap().pop().unwrap();
        let prefix = format!(
            "---\r\nid: {id}\r\n---\r\n\r\n## Notes\r\n\r\n{}\r\n## Action Items\r\n",
            "~~~markdown\r\n## Action Items\r\n- [ ] A literal example\r\n~~~\r\n"
        );
        let tail = "\r\n### Personal note\r\nKeep  exact spacing.\r\n";
        fs::write(&path, format!("{prefix}- [ ] Send notes\r\n{tail}")).unwrap();
        let mutate = |change| {
            publish_checklist(Some(&path), id, |publish| {
                store.change_action(id, change, publish)
            })
            .unwrap()
        };
        let action = |text: &str| CoreAction {
            text: text.into(),
            owner: None,
            due: None,
            done: false,
        };
        let added = mutate(ActionChange::Add(action("New"), None)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{prefix}- [ ] Send notes\r\n- [ ] New\r\n{tail}")
        );
        mutate(ActionChange::Edit(added.id, action("Updated"), None));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{prefix}- [ ] Send notes\r\n- [ ] Updated\r\n{tail}")
        );
        mutate(ActionChange::Delete(initial.id));
        mutate(ActionChange::Delete(added.id));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{prefix}{tail}")
        );
    }

    #[test]
    fn legacy_exports_from_previous_settings_are_found_or_reported_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut folders = test_folders(tmp.path());
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        store.upsert_summary(&summary(&meeting, "Legacy")).unwrap();
        // The old folder is recorded during a settings change even if it was
        // unavailable at the time and its exports could not be backfilled.
        let old = folders.summaries.clone();
        fs::remove_dir(&old).unwrap();
        assert!(scan_storage(&store, &folders).unwrap().is_empty());
        folders.summaries = tmp.path().join("new-summaries");
        fs::create_dir(&folders.summaries).unwrap();
        let offline = delete_parts(&store, &LocalMeetingFiles, &folders, id, &PARTS).unwrap();
        assert_eq!(offline.deleted, [MeetingPart::Summary]);
        assert!(!offline.meeting_removed);
        assert_eq!(offline.failures[0].part, MeetingPart::Markdown);
        fs::create_dir(&old).unwrap();
        let export = old.join(summary_file_name(&meeting));
        fs::write(&export, summary(&meeting, "Legacy").markdown).unwrap();
        let before = ledger(&store, &LocalMeetingFiles, &folders, id)
            .unwrap()
            .contents;
        assert_eq!(before.markdown_path.as_deref(), export.to_str());
        assert!(before.markdown_notice.is_none());
        assert!(store.meeting_markdown_file(id).unwrap().is_some());
        let removed = delete_parts(&store, &LocalMeetingFiles, &folders, id, &PARTS).unwrap();
        assert_eq!(removed.deleted, [MeetingPart::Markdown]);
        assert!(removed.markdown_notice.is_none());
        assert!(!export.exists());

        let another = self::meeting(&store, MeetingStatus::Done);
        let removed =
            delete_parts(&store, &LocalMeetingFiles, &folders, another.id, &PARTS).unwrap();
        assert!(removed.deleted.is_empty() && removed.meeting_removed);
        assert_eq!(removed.markdown_notice, Some(MARKDOWN_NOT_FOUND));
    }

    #[test]
    fn erase_cleans_historical_exports_and_retries_only_unavailable_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let previous = tmp.path().join("previous-summaries");
        fs::create_dir(&previous).unwrap();
        let verified = previous.join(summary_file_name(&meeting));
        fs::write(&verified, summary(&meeting, "Old").markdown).unwrap();
        store
            .remember_storage_folder("summaries", previous.to_str().unwrap())
            .unwrap();
        let unowned = folders.summaries.join("replaced.md");
        fs::write(&unowned, b"Private notes").unwrap();
        store
            .remember_meeting_export(id, unowned.to_str().unwrap())
            .unwrap();
        let offline = tmp.path().join("offline-recordings");
        let path = saved_track(&store, id, &offline);
        let moved = tmp.path().join("away");
        fs::rename(&offline, &moved).unwrap();

        let errors = erase_meeting_files(&store, &folders, id).unwrap();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(store.get_meeting(id).unwrap().is_none());
        assert!(store.id_in_use(id).unwrap());
        assert!(!verified.exists());
        assert_eq!(fs::read(unowned).unwrap(), b"Private notes");
        let pending = store.pending_file_cleanups().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "audio");
        assert!(retry_cleanup(&store, &pending[0]).unwrap().is_some());
        fs::rename(moved, offline).unwrap();
        assert_eq!(retry_cleanup(&store, &pending[0]).unwrap(), None);
        assert!(!path.exists());
        assert!(store.pending_file_cleanups().unwrap().is_empty());
    }

    struct FailingAudio;
    impl MeetingFiles for FailingAudio {
        fn audio_bytes(
            &self,
            _: &Path,
            _: MeetingId,
            _: bool,
            _: Option<&str>,
        ) -> io::Result<Option<u64>> {
            Ok(Some(120))
        }
        fn track_exists(&self, _: &Path, _: MeetingId) -> bool {
            false
        }
        fn delete_audio(&self, _: &Path, _: MeetingId, _: Option<&str>) -> io::Result<()> {
            Err(io::Error::other("audio is busy"))
        }
        fn markdown_bytes(&self, _: &Path, _: MeetingId) -> io::Result<Option<u64>> {
            Ok(None)
        }
        fn delete_markdown(&self, _: &Path, _: MeetingId) -> io::Result<()> {
            Ok(())
        }
    }

    fn test_folders(root: &Path) -> ContentFolders {
        let settings = AppSettings {
            recordings_path: Some(
                root.join("custom-recordings")
                    .to_string_lossy()
                    .into_owned(),
            ),
            summaries_path: Some(root.join("custom-summaries").to_string_lossy().into_owned()),
            ..AppSettings::default()
        };
        let folders = ContentFolders {
            data: root.join("minutae"),
            recordings: recordings_root(&settings),
            summaries: summaries_root(&settings),
        };
        fs::create_dir(&folders.recordings).unwrap();
        fs::create_dir(&folders.summaries).unwrap();
        folders
    }

    fn saved_track(store: &Store, id: MeetingId, root: &Path) -> PathBuf {
        let dir = root.join(id.to_string());
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mic.wav");
        fs::write(&path, b"audio").unwrap();
        store
            .upsert_track(&TrackFile {
                meeting_id: id,
                track: Track::Mic,
                path: path.to_string_lossy().into_owned(),
                duration_ms: None,
                bytes: Some(5),
                deleted: false,
            })
            .unwrap();
        path
    }

    #[test]
    fn partial_delete_reports_failure_keeps_summary_and_retires_stale_jobs() {
        let temp = tempfile::tempdir().unwrap();
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
            &test_folders(temp.path()),
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
            MeetingStatus::Failed
        );
    }

    #[test]
    fn markdown_can_outlive_summary_and_the_last_part_removes_the_meeting() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let files = LocalMeetingFiles;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            &folders.summaries,
        )
        .unwrap();
        let path = saved_markdown_path(&store, &files, &folders.summaries, id)
            .unwrap()
            .unwrap();
        let first = delete_parts(&store, &files, &folders, id, &[MeetingPart::Summary]).unwrap();
        assert_eq!(first.deleted, [MeetingPart::Summary]);
        assert!(!first.meeting_removed);
        assert!(path.is_file());
        assert_eq!(
            first.remaining.unwrap().parts[3].bytes,
            Some(fs::metadata(&path).unwrap().len())
        );
        let last = delete_parts(&store, &files, &folders, id, &[MeetingPart::Markdown]).unwrap();
        assert_eq!(last.deleted, [MeetingPart::Markdown]);
        assert!(last.meeting_removed);
        assert!(!path.exists());
        assert!(store.get_meeting(id).unwrap().is_none());
    }

    #[test]
    fn deletes_only_owned_audio_in_default_and_configured_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        for root in [folders.data.join("recordings"), folders.recordings.clone()] {
            let id = meeting(&store, MeetingStatus::Done).id;
            let dir = root.join(id.to_string());
            fs::create_dir_all(dir.join("chunks")).unwrap();
            let owned = dir.join("chunks/mic-000000.wav");
            let foreign = dir.join("chunks/notes.txt");
            fs::write(&owned, b"audio").unwrap();
            fs::write(&foreign, b"keep").unwrap();
            let oversized = dir.join("chunks/mic-99999999999999999999.wav");
            let noncanonical = dir.join("chunks/mic-0000000.wav");
            fs::write(&oversized, b"keep").unwrap();
            fs::write(&noncanonical, b"keep").unwrap();
            let result = delete_parts(&store, &files, &folders, id, &[MeetingPart::Audio]).unwrap();
            assert_eq!(result.deleted, [MeetingPart::Audio]);
            assert!(result.meeting_removed);
            assert!(!owned.exists());
            assert_eq!(fs::read(foreign).unwrap(), b"keep");
            assert!(oversized.exists());
            assert!(noncanonical.exists());
        }
    }

    #[test]
    fn saved_track_locates_audio_after_the_recordings_folder_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let id = meeting(&store, MeetingStatus::Done).id;
        let path = saved_track(&store, id, &tmp.path().join("previous-recordings"));
        let result = delete_parts(&store, &files, &folders, id, &[MeetingPart::Audio]).unwrap();
        assert_eq!(result.deleted, [MeetingPart::Audio]);
        assert!(result.meeting_removed);
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn unavailable_saved_folders_keep_the_meeting_for_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        let audio = saved_track(&store, id, &folders.recordings);
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            &folders.summaries,
        )
        .unwrap();
        let markdown = saved_markdown_path(&store, &files, &folders.summaries, id)
            .unwrap()
            .unwrap();
        let offline_audio = tmp.path().join("offline-audio");
        let offline_summaries = tmp.path().join("offline-summaries");
        fs::rename(&folders.recordings, &offline_audio).unwrap();
        fs::rename(&folders.summaries, &offline_summaries).unwrap();

        let result = delete_parts(&store, &files, &folders, id, &PARTS).unwrap();
        assert!(!result.meeting_removed);
        assert_eq!(result.failures.len(), 2);
        for (failure, part) in result
            .failures
            .iter()
            .zip([MeetingPart::Audio, MeetingPart::Markdown])
        {
            assert_eq!(failure.part, part);
            assert!(failure.reason.contains("storage folder"));
            assert!(failure.reason.contains("unavailable"));
        }
        assert!(store.get_meeting(id).unwrap().is_some());
        assert_eq!(store.list_tracks(id).unwrap().len(), 1);
        assert!(store.meeting_markdown_file(id).unwrap().is_some());

        fs::rename(offline_audio, &folders.recordings).unwrap();
        fs::rename(offline_summaries, &folders.summaries).unwrap();
        let result = delete_parts(&store, &files, &folders, id, &PARTS).unwrap();
        assert!(result.meeting_removed);
        assert!(!audio.exists());
        assert!(!markdown.exists());
    }

    #[test]
    fn unavailable_configured_folders_block_cleanup_even_without_saved_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let id = meeting(&store, MeetingStatus::Done).id;
        fs::remove_dir(&folders.recordings).unwrap();
        fs::remove_dir(&folders.summaries).unwrap();

        let result = delete_parts(&store, &files, &folders, id, &PARTS).unwrap();
        assert!(!result.meeting_removed);
        assert_eq!(result.failures.len(), 2);
        assert!(
            result
                .failures
                .iter()
                .all(|f| f.reason.contains("unavailable"))
        );
        assert!(store.get_meeting(id).unwrap().is_some());
    }

    #[test]
    fn custom_markdown_with_a_different_meeting_id_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let folders = test_folders(tmp.path());
        let files = LocalMeetingFiles;
        let store = Store::open_in_memory().unwrap();
        let meeting = meeting(&store, MeetingStatus::Done);
        let id = meeting.id;
        publish_summary(
            &store,
            &meeting,
            &summary(&meeting, "Saved"),
            None,
            &folders.summaries,
        )
        .unwrap();
        let path = saved_markdown_path(&store, &files, &folders.summaries, id)
            .unwrap()
            .unwrap();
        let foreign = format!(
            "---\nid: {}\n---\nKeep this export.\n",
            uuid::Uuid::new_v4()
        );
        fs::write(&path, &foreign).unwrap();

        let result = delete_parts(&store, &files, &folders, id, &[MeetingPart::Markdown]).unwrap();
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].part, MeetingPart::Markdown);
        assert_eq!(fs::read_to_string(path).unwrap(), foreign);
    }

    #[cfg(unix)]
    #[test]
    fn owned_audio_deletion_never_follows_symlinks() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let mut folders = test_folders(tmp.path());
        let store = Store::open_in_memory().unwrap();
        let id = meeting(&store, MeetingStatus::Done).id;
        let external = outside.path().join(id.to_string());
        fs::create_dir(&external).unwrap();
        let victim = external.join("mic.wav");
        fs::write(&victim, b"keep").unwrap();
        let files = LocalMeetingFiles;
        assert!(files.delete_audio(outside.path(), id, None).is_err());

        // The user chooses this link as their recordings root.
        fs::remove_dir(&folders.recordings).unwrap();
        symlink(outside.path(), &folders.recordings).unwrap();
        let path = folders.recordings.join(id.to_string());
        fs::rename(&path, outside.path().join("other-meeting")).unwrap();
        let victim = outside.path().join("other-meeting/mic.wav");
        symlink(victim.parent().unwrap(), &path).unwrap();
        assert!(files.delete_audio(&path, id, None).is_err());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        symlink(&victim, path.join("mic.wav")).unwrap();
        fs::write(path.join("system.wav.tmp"), b"leftover audio").unwrap();
        // Simulate changing the setting while the stored path still uses the link.
        store
            .upsert_track(&TrackFile {
                meeting_id: id,
                track: Track::Mic,
                path: path.join("mic.wav").to_string_lossy().into_owned(),
                duration_ms: None,
                bytes: None,
                deleted: false,
            })
            .unwrap();
        folders.recordings = tmp.path().join("new-recordings");
        let result = delete_parts(&store, &files, &folders, id, &[MeetingPart::Audio]).unwrap();
        assert_eq!(result.deleted, [MeetingPart::Audio]);
        assert!(result.meeting_removed);
        assert!(
            fs::symlink_metadata(path.join("mic.wav"))
                .unwrap()
                .is_symlink()
        );
        assert!(!path.join("system.wav.tmp").exists());
        assert_eq!(fs::read(victim).unwrap(), b"keep");
    }
}
