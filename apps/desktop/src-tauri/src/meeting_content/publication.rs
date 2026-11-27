//! Checklist-only edits and publication of regenerated summaries.

use std::fs;
use std::io;
use std::path::Path;

use minutae_core::{Meeting, MeetingId, Summary};
use minutae_store::{ActionItemRow, Store, StoreError};

use super::files::{
    ExportChange, LocalMeetingFiles, MeetingFiles, change_export, not_owned, present,
};
use super::markdown_path;
use crate::pipeline::summary_file_name;

fn checklist_lines(actions: &[ActionItemRow], newline: &str) -> String {
    let mut result = String::new();
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
        result.push_str(newline);
    }
    result
}

const ACTION_HEADING: &str = "## Action Items";

const CHECKLIST_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because its Action Items checklist couldn’t be identified safely.";
const UNVERIFIED_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because it can’t be verified as this meeting’s export.";
const DETACHED_NOTICE: &str = "Saved in Minutae. The Markdown file was left unchanged because the summary it came from was removed from Minutae.";

fn edit_checklist(markdown: &str, actions: &[ActionItemRow]) -> io::Result<Option<String>> {
    use pulldown_cmark::{Event, Options, Parser, Tag};

    let ambiguous = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "The Markdown export does not have an unambiguous Action Items checklist.",
        )
    };
    let options = Options::ENABLE_TASKLISTS | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut headings = Vec::new();
    let mut task_markers = Vec::new();
    let mut heading = None;
    let mut depth = 0;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 && matches!(tag, Tag::Heading { .. }) {
                    heading = Some(String::new());
                    headings.push((range, String::new()));
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0
                    && let Some(text) = heading.take()
                {
                    headings.last_mut().expect("started heading").1 = text;
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = &mut heading {
                    heading.push_str(&text);
                }
            }
            Event::TaskListMarker(_) => task_markers.push(range.start),
            _ => {}
        }
    }
    let mut sections = headings
        .iter()
        .enumerate()
        .filter(|(_, (_, text))| text.trim().eq_ignore_ascii_case("Action Items"));
    let Some((index, (heading, _))) = sections.next() else {
        return Ok(None);
    };
    if sections.next().is_some() {
        return Err(ambiguous());
    }

    let body = heading.end;
    let section_end = headings
        .get(index + 1)
        .map_or(markdown.len(), |(range, _)| range.start);
    let mut start = body;
    let mut end = body;
    let mut task_count = 0;
    for line in markdown[body..section_end].split_inclusive('\n') {
        if ["- [ ] ", "- [x] ", "- [X] "]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        {
            // The parser confirms the marker is in a real task list rather
            // than a code block or other literal content.
            if task_markers.binary_search(&(end + 2)).is_err() {
                break;
            }
            end += line.len();
            task_count += 1;
        } else if task_count == 0 && line.trim().is_empty() {
            start += line.len();
            end = start;
        } else {
            break;
        }
    }
    let marker_count = task_markers
        .iter()
        .filter(|&&offset| (body..section_end).contains(&offset))
        .count();
    if task_count != marker_count || (task_count == 0 && start != section_end) {
        return Err(ambiguous());
    }
    if task_count > 0
        && markdown[end..section_end]
            .lines()
            .next()
            .is_some_and(|line| !line.trim().is_empty() && line.starts_with([' ', '\t']))
    {
        return Err(ambiguous());
    }
    let newline = if markdown[heading.clone()].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines = checklist_lines(actions, newline);
    if !lines.is_empty() && !markdown[..start].ends_with('\n') {
        lines.insert_str(0, newline);
    }
    let mut result = markdown.to_owned();
    result.replace_range(start..end, &lines);
    Ok(Some(result))
}

/// The checklist brought up to date, or None when it cannot be located safely.
/// A missing section is only added to Markdown known to be Minutae's own text
/// (`unedited`).
fn update_checklist(markdown: &str, actions: &[ActionItemRow], unedited: bool) -> Option<String> {
    match edit_checklist(markdown, actions) {
        Ok(Some(edited)) => Some(edited),
        Ok(None) if unedited && actions.is_empty() => Some(markdown.to_owned()),
        Ok(None) if unedited => {
            let appended = format!(
                "{}\n\n{ACTION_HEADING}\n{}",
                markdown.trim_end(),
                checklist_lines(actions, "\n")
            );
            // An unclosed code fence, for one, would swallow the new section.
            matches!(edit_checklist(&appended, actions), Ok(Some(again)) if again == appended)
                .then_some(appended)
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
    let outcome = update(&mut |markdown, actions| {
        // The copy in the database. A checklist change is saved even when this
        // copy has no checklist that can be rewritten.
        let rendered = match (update_checklist(markdown, actions, true), publication) {
            (Some(rendered), _) => rendered,
            (None, Publication::Checklist) => markdown.to_owned(),
            (None, Publication::Summary(_)) => {
                return Err(StoreError::Invalid(
                    "The new summary has no usable Action Items checklist.".into(),
                ));
            }
        };
        if let Some(path) = path {
            let mut change = None;
            let mut inspected = false;
            let result = change_export(path, id, |previous| {
                inspected = true;
                let (bytes, keep_edited_copy) = match publication {
                    Publication::Summary(baseline) => (
                        rendered.as_bytes().to_vec(),
                        previous.is_some_and(|bytes| Some(bytes) != baseline.map(str::as_bytes)),
                    ),
                    Publication::Checklist => {
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
                        let Some(updated) = std::str::from_utf8(previous)
                            .ok()
                            .and_then(|text| update_checklist(text, actions, unedited))
                        else {
                            notice = Some(CHECKLIST_NOTICE);
                            return Ok(None);
                        };
                        if updated.as_bytes() == previous {
                            return Ok(None);
                        }
                        (updated.into_bytes(), false)
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
        Ok(rendered)
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
