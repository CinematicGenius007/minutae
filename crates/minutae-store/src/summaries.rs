//! Summaries and action items.

use std::collections::HashMap;

use minutae_core::{ActionItem, MeetingId, Summary};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::meetings::expect_one;
use crate::{Result, Store, json_col, now_ms, parse_uuid};

/// An action item with its row id (needed to toggle it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItemRow {
    pub id: i64,
    pub meeting_id: MeetingId,
    pub position: u32,
    pub source_ms: Option<u64>,
    #[serde(flatten)]
    pub item: ActionItem,
}

// Action ids stay opaque and cannot be recycled by SQLite when the previous
// highest row is deleted, including across summary refreshes.
fn new_action_id() -> i64 {
    (uuid::Uuid::new_v4().as_u128() as i64) & i64::MAX
}

/// Insert or replace a summary and its action items inside an open transaction.
pub(crate) fn write_summary(tx: &rusqlite::Connection, s: &Summary) -> Result<()> {
    let mid = s.meeting_id.to_string();
    let decisions_json = serde_json::to_string(&s.decisions)?;
    let decisions = s.decisions.join("\n");
    let now = now_ms();
    tx.execute(
            "INSERT INTO summaries (meeting_id, takeaway, overview, decisions, decisions_json, markdown, provider, model, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT(meeting_id) DO UPDATE SET takeaway = excluded.takeaway, overview = excluded.overview,
               decisions = excluded.decisions, decisions_json = excluded.decisions_json, markdown = excluded.markdown,
               provider = excluded.provider, model = excluded.model, updated_at = excluded.updated_at",
            params![mid, s.takeaway, s.overview, decisions, decisions_json, s.markdown, s.provider, s.model, now],
        )?;
    let curated: bool = tx.query_row(
        "SELECT checklist_curated FROM meetings WHERE id = ?1",
        [&mid],
        |r| r.get(0),
    )?;
    if curated {
        return Ok(());
    }
    let previous: HashMap<String, bool> = {
        let mut stmt =
            tx.prepare_cached("SELECT text, done FROM action_items WHERE meeting_id = ?1")?;
        let rows = stmt.query_map([&mid], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    tx.execute("DELETE FROM action_items WHERE meeting_id = ?1", [&mid])?;
    let mut stmt = tx.prepare_cached(
            "INSERT INTO action_items (id, meeting_id, position, text, owner, due, done, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
    for (i, a) in s.action_items.iter().enumerate() {
        let done = a.done || previous.get(&a.text).copied().unwrap_or(false);
        stmt.execute(params![
            new_action_id(),
            mid,
            i as i64,
            a.text,
            a.owner,
            a.due,
            done,
            now
        ])?;
    }
    Ok(())
}

/// User changes curate the whole checklist, including intentional deletions.
pub enum ActionChange {
    Add(ActionItem, Option<u64>),
    Edit(i64, ActionItem, Option<u64>),
    Delete(i64),
    SetDone(i64, bool),
}

/// Publish the rendered checklist in the same SQLite transaction as the action change.
pub type PublishChecklist<'a> = dyn FnMut(&str, &[ActionItemRow]) -> Result<String> + 'a;

fn publish_checklist(
    tx: &rusqlite::Connection,
    id: MeetingId,
    publish: &mut PublishChecklist<'_>,
) -> Result<()> {
    let mid = id.to_string();
    let markdown: Option<String> = tx
        .query_row(
            "SELECT markdown FROM summaries WHERE meeting_id = ?1",
            [&mid],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(markdown) = markdown {
        let markdown = publish(&markdown, &action_rows(tx, id)?)?;
        tx.execute(
            "UPDATE summaries SET markdown = ?2, updated_at = ?3 WHERE meeting_id = ?1",
            params![mid, markdown, now_ms()],
        )?;
    }
    Ok(())
}

fn action_rows(c: &rusqlite::Connection, id: MeetingId) -> Result<Vec<ActionItemRow>> {
    let mut stmt = c.prepare_cached(
        "SELECT id, meeting_id, position, text, owner, due, done, source_ms
         FROM action_items WHERE meeting_id = ?1 ORDER BY position, id",
    )?;
    let rows = stmt.query_map([id.to_string()], action_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

impl Store {
    pub fn publish_summary(
        &self,
        summary: &Summary,
        title: Option<&str>,
        markdown_path: &str,
        publish: &mut PublishChecklist<'_>,
    ) -> Result<()> {
        self.transaction(|tx| {
            write_summary(tx, summary)?;
            tx.execute(
                "UPDATE meetings SET markdown_file=?2 WHERE id=?1",
                params![summary.meeting_id.to_string(), markdown_path],
            )?;
            if let Some(title) = title {
                tx.execute(
                    "UPDATE meetings SET title=?2, title_lc=?3, updated_at=?4 WHERE id=?1",
                    params![
                        summary.meeting_id.to_string(),
                        title,
                        crate::schema::title_key(title),
                        now_ms()
                    ],
                )?;
            }
            publish_checklist(tx, summary.meeting_id, publish)
        })
    }

    pub fn action_item_meeting(&self, id: i64) -> Result<MeetingId> {
        self.read(|c| {
            c.query_row(
                "SELECT meeting_id FROM action_items WHERE id=?1",
                [id],
                |r| parse_uuid(&r.get::<_, String>(0)?),
            )
            .optional()?
            .ok_or_else(|| crate::StoreError::NotFound(id.to_string()))
        })
    }

    pub fn change_action(
        &self,
        meeting: MeetingId,
        change: ActionChange,
        publish: &mut PublishChecklist<'_>,
    ) -> Result<Option<ActionItemRow>> {
        let mid = meeting.to_string();
        self.transaction(|tx| {
            expect_one(tx.execute(
                "UPDATE meetings SET checklist_curated=1 WHERE id=?1 AND status != 'recording'",
                [&mid],
            )?, &mid)?;
            let row_id = match change {
                ActionChange::Add(a, source) => tx.query_row(
                    "INSERT INTO action_items (meeting_id, position, text, owner, due, done, created_at, source_ms, id)
                     VALUES (?1, (SELECT COALESCE(MAX(position)+1, 0) FROM action_items WHERE meeting_id=?1),
                             ?2, ?3, ?4, 0, ?5, ?6, ?7) RETURNING id",
                    params![mid, a.text, a.owner, a.due, now_ms(), source, new_action_id()], |r| r.get(0),
                )?,
                ActionChange::Edit(id, a, source) => {
                    expect_one(tx.execute(
                        "UPDATE action_items SET text=?3, owner=?4, due=?5, source_ms=?6 WHERE meeting_id=?1 AND id=?2",
                        params![mid, id, a.text, a.owner, a.due, source],
                    )?, id)?;
                    id
                }
                ActionChange::Delete(id) => {
                    expect_one(tx.execute("DELETE FROM action_items WHERE meeting_id=?1 AND id=?2", params![mid, id])?, id)?;
                    id
                }
                ActionChange::SetDone(id, done) => {
                    expect_one(tx.execute("UPDATE action_items SET done=?3 WHERE meeting_id=?1 AND id=?2", params![mid, id, done])?, id)?;
                    id
                }
            };
            publish_checklist(tx, meeting, publish)?;
            Ok(action_rows(tx, meeting)?.into_iter().find(|a| a.id == row_id))
        })
    }

    /// Insert or replace the meeting's summary and its action items.
    ///
    /// Re-summarising keeps the `done` state of items whose text is unchanged,
    /// so ticking an item survives a "Retry summary".
    pub fn upsert_summary(&self, s: &Summary) -> Result<()> {
        self.transaction(|tx| write_summary(tx, s))
    }

    pub fn get_summary(&self, id: MeetingId) -> Result<Option<Summary>> {
        let head = self.read(|c| {
            Ok(c.query_row(
                "SELECT takeaway, overview, decisions_json, markdown, provider, model FROM summaries WHERE meeting_id = ?1",
                [id.to_string()],
                |r| {
                    Ok(Summary {
                        meeting_id: id,
                        takeaway: r.get(0)?,
                        overview: r.get(1)?,
                        decisions: json_col(&r.get::<_, String>(2)?)?,
                        markdown: r.get(3)?,
                        provider: r.get(4)?,
                        model: r.get(5)?,
                        action_items: Vec::new(),
                    })
                },
            )
            .optional()?)
        })?;
        let Some(mut summary) = head else {
            return Ok(None);
        };
        summary.action_items = self
            .list_action_items(id)?
            .into_iter()
            .map(|r| r.item)
            .collect();
        Ok(Some(summary))
    }

    pub fn list_action_items(&self, id: MeetingId) -> Result<Vec<ActionItemRow>> {
        self.read(|c| action_rows(c, id))
    }

    /// Open action items across all meetings, newest meeting first.
    pub fn list_open_action_items(&self, limit: u32) -> Result<Vec<ActionItemRow>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT a.id, a.meeting_id, a.position, a.text, a.owner, a.due, a.done, a.source_ms FROM action_items a
                 JOIN meetings m ON m.id = a.meeting_id WHERE a.done = 0
                 ORDER BY m.started_at DESC, a.position LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit], action_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    pub fn set_action_item_done(&self, id: i64, done: bool) -> Result<()> {
        self.write(|c| {
            let n = c.execute(
                "UPDATE action_items SET done = ?2 WHERE id = ?1",
                params![id, done],
            )?;
            expect_one(n, id)
        })
    }

    /// Flip `done` and return the new value.
    pub fn toggle_action_item(&self, id: i64) -> Result<bool> {
        self.write(|c| {
            let v: Option<bool> = c
                .query_row(
                    "UPDATE action_items SET done = 1 - done WHERE id = ?1 RETURNING done",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            v.ok_or_else(|| crate::StoreError::NotFound(id.to_string()))
        })
    }
}

fn action_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActionItemRow> {
    Ok(ActionItemRow {
        id: r.get(0)?,
        meeting_id: parse_uuid(&r.get::<_, String>(1)?)?,
        position: r.get(2)?,
        source_ms: r.get(7)?,
        item: ActionItem {
            text: r.get(3)?,
            owner: r.get(4)?,
            due: r.get(5)?,
            done: r.get(6)?,
        },
    })
}
