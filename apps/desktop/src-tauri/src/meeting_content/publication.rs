//! Checklist-only edits and publication of regenerated summaries.

use std::fs;
use std::io;
use std::path::Path;

use minutae_core::{Meeting, MeetingId, Summary};
use minutae_store::{ActionItemRow, ChecklistState, ChecklistUpdate, Store, StoreError};

use super::checklist::{self, Written};
use super::files::{
    ExportChange, LocalMeetingFiles, MeetingFiles, change_export, not_owned, present,
};
use super::markdown_path;
use crate::pipeline::summary_file_name;

const CHECKLIST_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because its Action Items checklist couldn’t be identified safely.";
const UNVERIFIED_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because it can’t be verified as this meeting’s export.";
const DETACHED_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because the summary it came from was removed from Minutae.";

/// A new section for Markdown without one, if it can be read back. An unclosed
/// code fence, for one, would swallow it.
fn with_new_section(markdown: &str, actions: &[ActionItemRow]) -> Option<String> {
    let appended = checklist::append_section(markdown, actions);
    matches!(checklist::parse(&appended), Ok(Some(_))).then_some(appended)
}

/// Minutae's own copy of the summary with its checklist brought up to date, or
/// None when it cannot be located safely.
fn update_copy(markdown: &str, actions: &[ActionItemRow]) -> Option<String> {
    match checklist::parse(markdown) {
        Ok(Some(section)) => Some(checklist::replace_all(markdown, &section, actions)),
        Ok(None) if actions.is_empty() => Some(markdown.to_owned()),
        Ok(None) => with_new_section(markdown, actions),
        Err(_) => None,
    }
}

/// The export merged with the app's items, or None to leave it as it is. A
/// missing section is only added to a file that is still Minutae's own text
/// (`unedited`).
fn update_export(
    text: &str,
    base: &Written,
    state: &ChecklistState<'_>,
    unedited: bool,
) -> Option<checklist::Merged> {
    let unchanged = |markdown: String, written| checklist::Merged {
        markdown,
        rows: state.rows.to_vec(),
        written,
    };
    match checklist::parse(text) {
        Ok(Some(section)) => checklist::merge(text, &section, base, state.before, state.rows).ok(),
        Ok(None) if unedited && state.rows.is_empty() => {
            let written = Written::of(text, &[]).replacing(base);
            Some(unchanged(text.to_owned(), written))
        }
        Ok(None) if unedited => {
            let appended = with_new_section(text, state.rows)?;
            let written = Written::of(&appended, state.rows).replacing(base);
            Some(unchanged(appended, written))
        }
        Ok(None) | Err(_) => None,
    }
}

#[derive(Clone, Copy)]
enum Publication<'a> {
    Checklist,
    Summary(Option<&'a str>),
}

fn publish_export<T>(
    path: Option<&Path>,
    id: MeetingId,
    publication: Publication<'_>,
    update: impl FnOnce(&mut minutae_store::PublishChecklist<'_>) -> minutae_store::Result<T>,
) -> minutae_store::Result<(T, Option<&'static str>)> {
    let io_error =
        |e: io::Error| StoreError::Invalid(format!("The summary Markdown couldn't be saved: {e}"));
    let mut rollback = None;
    let mut notice = None;
    let outcome = update(&mut |state| {
        let markdown = state.markdown;
        let saved: Option<Written> = state
            .written
            .and_then(|json| serde_json::from_str(json).ok());
        // What Minutae last wrote. Exports made before this was recorded held
        // the saved copy.
        let base = saved.unwrap_or_else(|| {
            let saved_copy = match publication {
                Publication::Checklist => markdown,
                Publication::Summary(baseline) => baseline.unwrap_or_default(),
            };
            Written::of(saved_copy, state.before)
        });
        // The copy in the database. A new summary replaces the file with it.
        let replacement = match publication {
            Publication::Checklist => None,
            Publication::Summary(_) => {
                Some(update_copy(markdown, state.rows).ok_or_else(|| {
                    StoreError::Invalid(
                        "The new summary has no usable Action Items checklist.".into(),
                    )
                })?)
            }
        };
        let mut rows = state.rows.to_vec();
        let mut written = None;
        if let Some(path) = path {
            let mut change = None;
            let mut inspected = false;
            let result = change_export(path, id, |previous| {
                inspected = true;
                let (bytes, keep_edited_copy) = match (publication, &replacement) {
                    (Publication::Summary(baseline), Some(replacement)) => (
                        replacement.as_bytes().to_vec(),
                        previous.is_some_and(|bytes| Some(bytes) != baseline.map(str::as_bytes)),
                    ),
                    _ => {
                        let Some(previous) = previous else {
                            return Ok(None);
                        };
                        // No stored summary: this export outlived it, and its
                        // checklist is no longer the app's to rewrite.
                        if markdown.is_empty() {
                            notice = Some(DETACHED_NOTICE);
                            return Ok(None);
                        }
                        let unedited = previous == markdown.as_bytes();
                        let Some(merged) = std::str::from_utf8(previous)
                            .ok()
                            .and_then(|text| update_export(text, &base, &state, unedited))
                        else {
                            notice = Some(CHECKLIST_NOTICE);
                            return Ok(None);
                        };
                        rows = merged.rows;
                        written = Some(merged.written);
                        if merged.markdown.as_bytes() == previous {
                            return Ok(None);
                        }
                        (merged.markdown.into_bytes(), false)
                    }
                };
                change = Some((previous.map(<[u8]>::to_vec), bytes.clone()));
                Ok(Some(ExportChange::Write {
                    bytes,
                    keep_edited_copy,
                }))
            });
            match result {
                Err(error)
                    if !inspected
                        && matches!(publication, Publication::Checklist)
                        && not_owned(&error) =>
                {
                    notice = Some(UNVERIFIED_NOTICE);
                }
                result => result.map_err(io_error)?,
            }
            rollback = change;
        }
        let (copy, written) = match replacement {
            Some(copy) => {
                let written = Written::of(&copy, &rows).replacing(&base);
                (copy, written)
            }
            // A checklist change is saved even when the copy has no checklist
            // that can be rewritten. A file left alone keeps its old record.
            None => (
                update_copy(markdown, &rows).unwrap_or_else(|| markdown.to_owned()),
                written.unwrap_or(base),
            ),
        };
        Ok(ChecklistUpdate {
            markdown: copy,
            rows,
            written: Some(serde_json::to_string(&written)?),
        })
    });
    if outcome.is_err()
        && let (Some(path), Some((previous, published))) = (path, rollback)
    {
        change_export(path, id, |current| {
            if current != Some(&published) {
                return Err(io::Error::other(
                    "The Markdown changed before the previous export could be restored.",
                ));
            }
            Ok(Some(previous.map_or(ExportChange::Delete, |bytes| {
                ExportChange::Write {
                    bytes,
                    keep_edited_copy: false,
                }
            })))
        })
        .map_err(io_error)?;
    }
    outcome.map(|value| (value, notice))
}

/// Saves a checklist change. The second value explains why an existing Markdown
/// file was left as it was.
pub(super) fn publish_checklist<T>(
    path: Option<&Path>,
    id: MeetingId,
    update: impl FnOnce(&mut minutae_store::PublishChecklist<'_>) -> minutae_store::Result<T>,
) -> minutae_store::Result<(T, Option<&'static str>)> {
    publish_export(path, id, Publication::Checklist, update)
}

pub fn publish_summary(
    store: &Store,
    meeting: &Meeting,
    summary: &Summary,
    title: Option<&str>,
    dir: &Path,
) -> minutae_store::Result<()> {
    let path = markdown_path(store, dir, meeting.id)?
        .filter(|path| {
            path.parent() == Some(dir)
                || present(fs::symlink_metadata(path)).is_ok_and(|entry| entry.is_some())
                || LocalMeetingFiles
                    .markdown_bytes(path, meeting.id)
                    .is_ok_and(|size| size.is_some())
        })
        .unwrap_or_else(|| dir.join(summary_file_name(meeting)));
    if path.parent() == Some(dir) {
        fs::create_dir_all(dir).map_err(|e| {
            StoreError::Invalid(format!("The summary folder couldn't be created: {e}"))
        })?;
    }
    let baseline = store.get_summary(meeting.id)?;
    publish_export(
        Some(&path),
        meeting.id,
        Publication::Summary(baseline.as_ref().map(|s| s.markdown.as_str())),
        |publish| store.publish_summary(summary, title, &path.to_string_lossy(), publish),
    )
    .map(|(saved, _)| saved)
}
