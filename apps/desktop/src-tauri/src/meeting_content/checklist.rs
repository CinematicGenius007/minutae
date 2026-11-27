//! The Action Items checklist of an export: reading it, and merging hand edits
//! with the app's items.
//!
//! The merge is three-way, between the checklist Minutae last wrote ([`Written`]),
//! the file as it is now and the app's items. It only ever changes lines Minutae
//! wrote and the user didn't touch in the same way. Anything it can't tell apart
//! safely fails the merge, so the caller leaves the file as it is.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use minutae_core::ActionItem;
use minutae_store::{ActionItemRow, new_action_id};
use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::{Deserialize, Serialize};

const HEADING: &str = "Action Items";

/// The checklist cannot be located or changed without guessing.
#[derive(Debug)]
pub(super) struct Ambiguous;

#[derive(Debug)]
struct Task {
    /// The whole line, including its ending.
    line: Range<usize>,
    /// Offset of the `[` of the checkbox.
    mark: usize,
    label: String,
    done: bool,
}

#[derive(Debug)]
pub(super) struct Section {
    tasks: Vec<Task>,
    /// Where new lines go: after the last task, or at the end of an empty section.
    append_at: usize,
    newline: &'static str,
}

/// The checklist as Minutae last wrote it or took it over from the file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Written {
    lines: Vec<Line>,
    /// Wording the app has since removed or replaced. A file holding it again
    /// may be an older copy, so it is never taken as a new task.
    retired: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Line {
    /// None for a line that never matched an app item. It is left alone.
    id: Option<i64>,
    label: String,
    done: bool,
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn timestamp(ms: u64) -> String {
    format!(" · {}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// The text of a row's checklist line, after the checkbox.
fn row_label(row: &ActionItemRow) -> String {
    let mut label = one_line(&minutae_llm::summary::checklist_text(&row.item));
    if let Some(ms) = row.source_ms {
        label.push_str(&timestamp(ms));
    }
    label
}

fn checkbox(done: bool) -> &'static str {
    if done { "[x]" } else { "[ ]" }
}

fn row_lines<'a>(rows: impl IntoIterator<Item = &'a ActionItemRow>, newline: &str) -> String {
    rows.into_iter()
        .map(|row| format!("- {} {}{newline}", checkbox(row.item.done), row_label(row)))
        .collect()
}

/// A line that starts its own block, so it doesn't continue the item above.
fn starts_block(line: &str) -> bool {
    let digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
    ["- ", "* ", "+ "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        || (digits.len() < line.len() && digits.starts_with(['.', ')']))
}

/// The Action Items section, None when there is no such heading.
pub(super) fn parse(markdown: &str) -> Result<Option<Section>, Ambiguous> {
    let options = Options::ENABLE_TASKLISTS | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut headings: Vec<(Range<usize>, String)> = Vec::new();
    let mut markers = Vec::new();
    let mut in_heading = false;
    let mut depth = 0;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 && matches!(tag, Tag::Heading { .. }) {
                    in_heading = true;
                    headings.push((range, String::new()));
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    in_heading = false;
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if in_heading && let Some((_, heading)) = headings.last_mut() {
                    heading.push_str(&text);
                }
            }
            Event::TaskListMarker(_) => markers.push(range.start),
            _ => {}
        }
    }
    let mut sections = headings
        .iter()
        .enumerate()
        .filter(|(_, (_, text))| text.trim().eq_ignore_ascii_case(HEADING));
    let Some((index, (heading, _))) = sections.next() else {
        return Ok(None);
    };
    if sections.next().is_some() {
        return Err(Ambiguous);
    }
    let body = heading.end;
    let end = headings
        .get(index + 1)
        .map_or(markdown.len(), |(range, _)| range.start);

    let mut tasks = Vec::new();
    let mut append_at = None;
    for &mark in markers.iter().filter(|&&mark| (body..end).contains(&mark)) {
        let start = markdown[..mark].rfind('\n').map_or(0, |at| at + 1);
        let stop = markdown[mark..]
            .find('\n')
            .map_or(markdown.len(), |at| mark + at + 1);
        // Only top-level bullets: nested, quoted and numbered tasks carry
        // structure a line rewrite could break.
        let prefix = &markdown[start..mark];
        if !prefix.starts_with(['-', '*', '+']) || !prefix[1..].chars().all(|c| c == ' ') {
            return Err(Ambiguous);
        }
        let mut following = markdown[stop..end].lines();
        let next = following.next().unwrap_or_default();
        let continued = !next.trim().is_empty() && !starts_block(next);
        let nested = std::iter::once(next)
            .chain(following)
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.starts_with([' ', '\t']));
        if continued || nested {
            return Err(Ambiguous);
        }
        append_at = Some(stop);
        let label = one_line(&markdown[mark + 3..stop]);
        // An empty checkbox isn't a task yet. It stays as it is.
        if !label.is_empty() {
            tasks.push(Task {
                line: start..stop,
                mark,
                label,
                done: matches!(markdown.as_bytes()[mark + 1], b'x' | b'X'),
            });
        }
    }
    let append_at = match append_at {
        Some(at) => at,
        // Without a task to follow, only a blank section is unambiguous.
        None if markdown[body..end].trim().is_empty() => end,
        None => return Err(Ambiguous),
    };
    let newline = if markdown[heading.clone()].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    Ok(Some(Section {
        tasks,
        append_at,
        newline,
    }))
}

/// Replaces the lines of `section`: `replace` gives each task its new text, and
/// `added` goes after the last one.
fn splice(
    markdown: &str,
    section: &Section,
    mut replace: impl FnMut(usize, &str) -> String,
    added: &str,
) -> String {
    let mut result = String::with_capacity(markdown.len() + added.len());
    let mut copied = 0;
    for (index, task) in section.tasks.iter().enumerate() {
        result.push_str(&markdown[copied..task.line.start]);
        result.push_str(&replace(index, &markdown[task.line.clone()]));
        copied = task.line.end;
    }
    result.push_str(&markdown[copied..section.append_at]);
    if !added.is_empty() {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push_str(section.newline);
        }
        result.push_str(added);
    }
    result.push_str(&markdown[section.append_at..]);
    result
}

/// Minutae's own copy of the summary with its checklist set to `rows`.
pub(super) fn replace_all(markdown: &str, section: &Section, rows: &[ActionItemRow]) -> String {
    splice(
        markdown,
        section,
        |_, _| String::new(),
        &row_lines(rows, section.newline),
    )
}

pub(super) fn append_section(markdown: &str, rows: &[ActionItemRow]) -> String {
    format!(
        "{}\n\n## {HEADING}\n{}",
        markdown.trim_end(),
        row_lines(rows, "\n")
    )
}

impl Written {
    /// What `markdown`, Minutae's own text, says about `rows`. Used for text
    /// Minutae just wrote, and for exports made before this was recorded.
    pub(super) fn of(markdown: &str, rows: &[ActionItemRow]) -> Self {
        let lines = match parse(markdown) {
            Ok(Some(section)) => {
                let mut unmatched: Vec<&ActionItemRow> = rows.iter().collect();
                section
                    .tasks
                    .into_iter()
                    .map(|task| {
                        let id = unmatched
                            .iter()
                            .position(|row| row_label(row) == task.label)
                            .map(|at| unmatched.remove(at).id);
                        Line {
                            id,
                            label: task.label,
                            done: task.done,
                        }
                    })
                    .collect()
            }
            Ok(None) => Vec::new(),
            // The saved text can't be read, so assume it listed every item.
            Err(Ambiguous) => rows
                .iter()
                .map(|row| Line {
                    id: Some(row.id),
                    label: row_label(row),
                    done: row.item.done,
                })
                .collect(),
        };
        Self {
            lines,
            retired: BTreeSet::new(),
        }
    }

    /// This record after Minutae replaced the whole file, remembering what
    /// `previous` listed and the new text doesn't.
    pub(super) fn replacing(mut self, previous: &Written) -> Self {
        self.retired = previous.retired.clone();
        self.retired
            .extend(previous.lines.iter().map(|line| line.label.clone()));
        for line in &self.lines {
            self.retired.remove(&line.label);
        }
        self
    }
}

/// Shared words make a rewritten line recognisably the same task.
fn similar(a: &str, b: &str) -> bool {
    let words = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let (a, b) = (words(a), words(b));
    let shortest = a.len().min(b.len());
    shortest > 0 && a.intersection(&b).count() * 2 >= shortest
}

/// `row` reworded to read `label` in the checklist, keeping the owner, due date
/// and timestamp that the new wording still shows.
fn reworded(row: &ActionItemRow, label: &str) -> ActionItemRow {
    let mut body = label;
    let mut source_ms = None;
    if let Some(ms) = row.source_ms
        && let Some(rest) = body.strip_suffix(&timestamp(ms))
    {
        (body, source_ms) = (rest, Some(ms));
    } else if let Some((rest, time)) = body.rsplit_once(" · ")
        && let Some((minutes, seconds)) = time.split_once(':')
        && let (Ok(minutes), Ok(seconds)) = (minutes.parse::<u32>(), seconds.parse::<u32>())
    {
        let ms = u64::from(minutes) * 60_000 + u64::from(seconds) * 1000;
        if timestamp(ms) == format!(" · {time}") {
            (body, source_ms) = (rest, Some(ms));
        }
    }
    let mut text = body;
    let owner = row.item.owner.clone().filter(|owner| {
        if let Some(rest) = text.strip_prefix(&format!("{owner}: ")) {
            text = rest;
            return true;
        }
        text.to_lowercase().contains(&owner.to_lowercase())
    });
    let due = row.item.due.clone().filter(|due| {
        if let Some(rest) = text.strip_suffix(&format!(" (due {due})")) {
            text = rest;
            return true;
        }
        text.to_lowercase().contains(&due.to_lowercase())
    });
    let mut result = row.clone();
    result.source_ms = source_ms;
    result.item.text = text.to_string();
    result.item.owner = owner;
    result.item.due = due;
    if row_label(&result) != label {
        result.item.text = body.to_string();
        result.item.owner = None;
        result.item.due = None;
    }
    result
}

pub(super) struct Merged {
    pub(super) markdown: String,
    /// The app's items with the file's edits and new tasks, in order.
    pub(super) rows: Vec<ActionItemRow>,
    pub(super) written: Written,
}

/// Pairs each task in the file with the line it was written as, returning the
/// pairs by task. Fails when a line was changed beyond recognition or removed.
fn match_lines(
    tasks: &[Task],
    base: &[Line],
    alive: impl Fn(i64) -> bool,
) -> Result<Vec<Option<usize>>, Ambiguous> {
    let mut matched: Vec<Option<usize>> = vec![None; tasks.len()];
    let mut used = vec![false; base.len()];
    // Unchanged lines first, then lines whose tick alone changed.
    for exact in [true, false] {
        for (task, slot) in tasks.iter().zip(&mut matched) {
            if slot.is_some() {
                continue;
            }
            if let Some(at) = base.iter().enumerate().position(|(at, line)| {
                !used[at] && line.label == task.label && (!exact || line.done == task.done)
            }) {
                used[at] = true;
                *slot = Some(at);
            }
        }
    }
    // A reworded line is one that sits between the same unchanged neighbours.
    let anchors = matched.clone();
    let beside = |task: Option<usize>, line: Option<usize>| match (task, line) {
        (None, None) => true,
        (Some(task), Some(line)) => anchors[task] == Some(line),
        _ => false,
    };
    // Removed by the app and no longer in the file as Minutae wrote it.
    let mut removed_by_app = Vec::new();
    for (at, line) in base.iter().enumerate() {
        let Some(id) = line.id else { continue };
        if used[at] {
            continue;
        }
        if !alive(id) {
            removed_by_app.push(&line.label);
            continue;
        }
        let reworded = (0..tasks.len())
            .find(|&task| {
                anchors[task].is_none()
                    && matched[task].is_none()
                    && beside(task.checked_sub(1), at.checked_sub(1))
                    && beside(
                        Some(task + 1).filter(|&next| next < tasks.len()),
                        Some(at + 1).filter(|&next| next < base.len()),
                    )
                    && similar(&tasks[task].label, &line.label)
            })
            .ok_or(Ambiguous)?;
        matched[reworded] = Some(at);
    }
    // A leftover line like one of those could be its reworded form.
    let leftover = |(task, matched): (&Task, &Option<usize>)| {
        matched.is_none() && removed_by_app.iter().any(|gone| similar(&task.label, gone))
    };
    if tasks.iter().zip(&matched).any(leftover) {
        return Err(Ambiguous);
    }
    Ok(matched)
}

/// Merges the file's checklist with the app's items. `before` and `after` are
/// the items around the change being saved, which the file must not override.
pub(super) fn merge(
    markdown: &str,
    section: &Section,
    base: &Written,
    before: &[ActionItemRow],
    after: &[ActionItemRow],
) -> Result<Merged, Ambiguous> {
    let current: BTreeMap<i64, &ActionItemRow> = after.iter().map(|row| (row.id, row)).collect();
    let earlier: BTreeMap<i64, &ActionItemRow> = before.iter().map(|row| (row.id, row)).collect();
    let matched = match_lines(&section.tasks, &base.lines, |id| current.contains_key(&id))?;

    let mut rows = after.to_vec();
    let mut lines = Vec::new();
    let mut replacements = Vec::new();
    // The item of the previous line, which a new task is placed after.
    let mut previous = None;
    for (task, matched) in section.tasks.iter().zip(&matched) {
        let original = &markdown[task.line.clone()];
        let mut kept = Line {
            id: None,
            label: task.label.clone(),
            done: task.done,
        };
        let Some(line) = matched.map(|at| &base.lines[at]) else {
            if base.retired.contains(&task.label) {
                return Err(Ambiguous);
            }
            // A task the user typed.
            let blank = ActionItemRow {
                id: new_action_id(),
                meeting_id: Default::default(),
                position: 0,
                source_ms: None,
                item: ActionItem {
                    text: String::new(),
                    owner: None,
                    due: None,
                    done: task.done,
                },
            };
            let row = reworded(&blank, &task.label);
            let at = previous
                .and_then(|id| rows.iter().position(|row| row.id == id))
                .map_or(0, |at| at + 1);
            previous = Some(row.id);
            kept.id = Some(row.id);
            rows.insert(at, row);
            lines.push(kept);
            replacements.push(original.to_string());
            continue;
        };
        let Some(id) = line.id else {
            lines.push(kept);
            replacements.push(original.to_string());
            continue;
        };
        let Some(&row) = current.get(&id) else {
            // Deleted in the app. A hand tick doesn't keep it, new wording does.
            if task.label != line.label {
                return Err(Ambiguous);
            }
            replacements.push(String::new());
            continue;
        };
        let app = row_label(row);
        let label = if task.label == line.label {
            app.clone()
        } else if base.retired.contains(&task.label) {
            return Err(Ambiguous);
        } else if app == line.label || app == task.label {
            task.label.clone()
        } else {
            return Err(Ambiguous);
        };
        let done = if task.done != line.done {
            task.done
        } else {
            row.item.done
        };
        let wording = |row: &ActionItemRow| {
            (
                row.item.text.clone(),
                row.item.owner.clone(),
                row.item.due.clone(),
                row.source_ms,
            )
        };
        if let Some(was) = earlier.get(&id)
            && ((was.item.done != row.item.done && done != row.item.done)
                || (wording(was) != wording(row) && label != app))
        {
            return Err(Ambiguous);
        }
        let mut updated = if label == app {
            row.clone()
        } else {
            reworded(row, &label)
        };
        updated.item.done = done;
        if let Some(slot) = rows.iter_mut().find(|row| row.id == id) {
            *slot = updated;
        }
        replacements.push(if label != task.label {
            let ending = &original[original.trim_end_matches(['\r', '\n']).len()..];
            let bullet = &markdown[task.line.start..task.mark];
            format!("{bullet}{} {label}{ending}", checkbox(done))
        } else if done != task.done {
            let mut line = original.to_string();
            let mark = task.mark - task.line.start;
            line.replace_range(mark..mark + 3, checkbox(done));
            line
        } else {
            original.to_string()
        });
        previous = Some(id);
        lines.push(Line {
            id: Some(id),
            label,
            done,
        });
    }

    // Items the file never listed go after the last task.
    let listed: BTreeSet<i64> = base
        .lines
        .iter()
        .chain(&lines)
        .filter_map(|line| line.id)
        .collect();
    let added: Vec<&ActionItemRow> = after
        .iter()
        .filter(|row| !listed.contains(&row.id))
        .collect();
    lines.extend(added.iter().map(|row| Line {
        id: Some(row.id),
        label: row_label(row),
        done: row.item.done,
    }));
    let markdown = splice(
        markdown,
        section,
        |index, _| std::mem::take(&mut replacements[index]),
        &row_lines(added, section.newline),
    );

    // Remember wording the app took out of the file.
    let mut retired = base.retired.clone();
    for (at, line) in base.lines.iter().enumerate() {
        let hand_edit = matched
            .iter()
            .position(|&matched| matched == Some(at))
            .is_some_and(|task| section.tasks[task].label != line.label);
        if line.id.is_some() && !hand_edit {
            retired.insert(line.label.clone());
        }
    }
    for line in &lines {
        retired.remove(&line.label);
    }
    for (position, row) in rows.iter_mut().enumerate() {
        row.position = position as u32;
    }
    Ok(Merged {
        markdown,
        rows,
        written: Written { lines, retired },
    })
}
