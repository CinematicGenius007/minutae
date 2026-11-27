//! Erase All removes library rows and retains only offline file cleanup work.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use minutae_core::{AppSettings, MeetingId};
use minutae_store::{PendingFileCleanup, Store};

use super::files::{LocalMeetingFiles, MeetingFiles, storage_unavailable};
use super::{ContentFolders, R, audio_directories, known_roots};
use crate::state::AppState;

fn cleanup_file(item: &PendingFileCleanup) -> io::Result<()> {
    let path = Path::new(&item.path);
    let id = item.meeting_id;
    match item.kind.as_str() {
        "audio" => LocalMeetingFiles.delete_audio(path, id, item.imported_name.as_deref()),
        "markdown" => LocalMeetingFiles.delete_markdown(path, id),
        "summaryFolder" => {
            for path in LocalMeetingFiles.find_markdowns(path, id)? {
                LocalMeetingFiles.delete_markdown(&path, id)?;
            }
            Ok(())
        }
        _ => Err(io::Error::other("Unknown file cleanup request.")),
    }
}

pub(crate) fn erase_meeting_files(
    store: &Store,
    folders: &ContentFolders,
    id: MeetingId,
) -> minutae_store::Result<Vec<String>> {
    if store.get_meeting(id)?.is_none() {
        return Ok(Vec::new());
    }
    store.settle_meeting_for_deletion(id)?;
    let (audio_removed, markdown_removed) = store.meeting_storage_state(id)?;
    let tracks = store.list_tracks(id)?;
    let (mut audio, invalid_audio) = audio_directories(
        id,
        &tracks,
        folders,
        store.is_imported_meeting(id)?,
        audio_removed,
    );
    let default_audio = folders.data.join("recordings");
    for root in known_roots(
        store,
        "recordings",
        &folders.recordings,
        default_audio.clone(),
    )? {
        audio.entry(root.join(id.to_string())).or_default().required |=
            !audio_removed && root != default_audio;
    }
    let default_summaries = folders.data.join("summaries");
    let summary_roots = known_roots(
        store,
        "summaries",
        &folders.summaries,
        default_summaries.clone(),
    )?;
    let tracked: BTreeSet<_> = store
        .meeting_exports(id)?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let mut errors: Vec<String> = invalid_audio.into_iter().collect();
    let mut pending = Vec::new();
    let mut clean = |kind: &str, path: &Path, required: bool, imported_name| {
        let item = PendingFileCleanup {
            meeting_id: id,
            kind: kind.into(),
            path: path.to_string_lossy().into_owned(),
            imported_name,
        };
        if let Err(error) = cleanup_file(&item) {
            let offline = storage_unavailable(&error);
            if !required && offline && error.kind() == io::ErrorKind::NotFound {
                return;
            }
            errors.push(format!("{}: {error}", path.display()));
            if offline {
                pending.push(item);
            }
        }
    };
    for (dir, location) in audio {
        clean("audio", &dir, location.required, location.imported);
    }
    for path in tracked {
        clean("markdown", &path, true, None);
    }
    // Scan every known root to cover exports made before paths were recorded.
    for root in summary_roots {
        clean(
            "summaryFolder",
            &root,
            !markdown_removed && root != default_summaries,
            None,
        );
    }
    store.erase_meeting_records(id, &pending)?;
    Ok(errors)
}

pub(super) fn retry_cleanup(
    store: &Store,
    item: &PendingFileCleanup,
) -> minutae_store::Result<Option<String>> {
    let result = cleanup_file(item);
    if !result.as_ref().is_err_and(storage_unavailable) {
        store.forget_file_cleanup(item)?;
    }
    Ok(result.err().map(|e| format!("{}: {e}", item.path)))
}

pub(crate) async fn recover_erased_files(state: &AppState) -> R<Vec<String>> {
    let pending = state
        .store
        .call(Store::pending_file_cleanups)
        .await
        .map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    for item in pending {
        let operation = state.meeting_ops.lock(item.meeting_id).await;
        if let Some(error) = state
            .store
            .call(move |store| {
                let _operation = operation;
                retry_cleanup(store, &item)
            })
            .await
            .map_err(|e| e.to_string())?
        {
            warnings.push(error);
        }
    }
    Ok(warnings)
}

fn erase_discard(
    store: &Store,
    id: MeetingId,
    path: &str,
) -> minutae_store::Result<Option<String>> {
    let result = LocalMeetingFiles.delete_audio(Path::new(path), id, None);
    if !result.as_ref().is_err_and(storage_unavailable) {
        store.finish_recording_discard(id)?;
    }
    Ok(result.err().map(|e| format!("{path}: {e}")))
}

async fn erase_discarded_files(state: &AppState) -> R<Vec<String>> {
    let pending = state
        .store
        .call(Store::pending_recording_discards)
        .await
        .map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    for (id, path) in pending {
        let operation = state.meeting_ops.lock(id).await;
        if let Some(error) = state
            .store
            .call(move |store| {
                let _operation = operation;
                erase_discard(store, id, &path)
            })
            .await
            .map_err(|e| e.to_string())?
        {
            warnings.push(error);
        }
    }
    Ok(warnings)
}

pub(crate) async fn erase_meetings(state: &AppState, settings: &AppSettings) -> R<Vec<String>> {
    let mut warnings = recover_erased_files(state).await?;
    warnings.extend(erase_discarded_files(state).await?);
    let folders = Arc::new(ContentFolders::current(settings));
    let ids = state
        .store
        .call(Store::meeting_ids)
        .await
        .map_err(|e| e.to_string())?;
    for id in ids {
        let operation = state.meeting_ops.lock(id).await;
        let folders = folders.clone();
        warnings.extend(
            state
                .store
                .call(move |store| {
                    let _operation = operation;
                    erase_meeting_files(store, &folders, id)
                })
                .await
                .map_err(|e| e.to_string())?,
        );
    }
    // Erase All has already attempted every recorded root. Offline locations
    // are now recorded individually and no longer depend on folder settings.
    state
        .store
        .call(|store| {
            store.write(|c| {
                c.execute("DELETE FROM storage_folders", [])?;
                Ok(())
            })
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn erase_discards_keep_only_unavailable_storage_records() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();
        let create = |root: &Path| {
            let id = uuid::Uuid::new_v4();
            let dir = root.join(id.to_string());
            store
                .create_meeting(&minutae_core::Meeting {
                    id,
                    title: "Discard".into(),
                    started_at: chrono::Utc::now(),
                    ended_at: None,
                    status: minutae_core::MeetingStatus::Recording,
                    source_app: None,
                    failure: None,
                })
                .unwrap();
            store
                .begin_recording_discard(id, dir.to_str().unwrap())
                .unwrap();
            (id, dir)
        };
        let (available, dir) = create(tmp.path());
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("notes.txt"), b"Private notes").unwrap();
        fs::write(dir.join("mic.wav"), b"Audio").unwrap();
        let (offline, would_be) = create(&tmp.path().join("offline"));
        assert_eq!(
            erase_discard(&store, available, dir.to_str().unwrap()).unwrap(),
            None
        );
        assert!(
            erase_discard(&store, offline, would_be.to_str().unwrap())
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store.pending_recording_discards().unwrap(),
            vec![(offline, would_be.to_string_lossy().into_owned())]
        );
        assert_eq!(fs::read(dir.join("notes.txt")).unwrap(), b"Private notes");
        assert!(!dir.join("mic.wav").exists());
    }
}
