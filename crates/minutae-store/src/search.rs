//! Keyword search (⌘K / Library) over FTS5: transcript segments, summaries,
//! action items, dictations, plus meeting titles. Ranked by bm25.

use chrono::{DateTime, Utc};
use minutae_core::{DictationId, MeetingId};
use rusqlite::params_from_iter;
use serde::{Deserialize, Serialize};

use crate::{Result, Store, from_ms, ms, parse_uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchKind {
    #[default]
    All,
    Meetings,
    Dictations,
    ActionItems,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchFilters {
    pub kind: SearchKind,
    pub since: Option<DateTime<Utc>>,
    /// Max hits returned (and per source before merging).
    pub limit: u32,
}

impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            kind: SearchKind::All,
            since: None,
            limit: 50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HitKind {
    /// Meeting title matched.
    Meeting,
    Transcript,
    Summary,
    ActionItem,
    Dictation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: HitKind,
    pub meeting_id: Option<MeetingId>,
    pub dictation_id: Option<DictationId>,
    pub action_item_id: Option<i64>,
    /// Meeting title, or the first words of a dictation.
    pub title: String,
    /// Plain text excerpt.
    pub snippet: String,
    /// Same excerpt, HTML-escaped, matches wrapped in `<mark>`. Safe to render.
    pub snippet_html: String,
    /// Match ranges in `snippet` as UTF-16 code-unit offsets `[start, end)` (JS string indices).
    pub highlights: Vec<[u32; 2]>,
    /// Position in the recording for transcript hits.
    pub at_ms: Option<u64>,
    /// Meeting start / dictation time.
    pub date: DateTime<Utc>,
    /// Higher is better (negated bm25; title hits rank first).
    pub score: f64,
}

/// Split user input into lowercase word tokens (Unicode letters/digits).
pub fn search_tokens(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// All tokens must match; the last one is a prefix (search-as-you-type).
/// Returns `None` when there is nothing to search for.
pub fn fts_and_query(tokens: &[String]) -> Option<String> {
    let n = tokens.len();
    if n == 0 {
        return None;
    }
    Some(
        tokens
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i + 1 == n {
                    format!("\"{t}\"*")
                } else {
                    format!("\"{t}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Any token may match (used by retrieval for "Ask").
pub fn fts_or_query(tokens: &[String]) -> Option<String> {
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

const OPEN: char = '\u{E000}';
const CLOSE: char = '\u{E001}';

fn escape_html(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// Turn a snippet with private-use markers into (plain, html, utf16 ranges).
fn render_snippet(raw: &str) -> (String, String, Vec<[u32; 2]>) {
    let mut plain = String::with_capacity(raw.len());
    let mut html = String::with_capacity(raw.len() + 16);
    let mut ranges = Vec::new();
    let mut pos: u32 = 0;
    let mut start = None;
    for ch in raw.chars() {
        match ch {
            OPEN => {
                start = Some(pos);
                html.push_str("<mark>");
            }
            CLOSE => {
                if let Some(s) = start.take() {
                    ranges.push([s, pos]);
                }
                html.push_str("</mark>");
            }
            c => {
                plain.push(c);
                let mut buf = [0u8; 4];
                escape_html(c.encode_utf8(&mut buf), &mut html);
                pos += c.len_utf16() as u32;
            }
        }
    }
    (plain, html, ranges)
}

/// Highlight token occurrences (case-insensitive, prefix) in plain text.
fn mark_tokens(text: &str, tokens: &[String]) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let mut i = 0;
    let lower: Vec<(usize, char)> = text.char_indices().collect();
    while i < lower.len() {
        let (byte, ch) = lower[i];
        let at_word_start = ch.is_alphanumeric() && (i == 0 || !lower[i - 1].1.is_alphanumeric());
        if at_word_start {
            let rest = &text[byte..];
            let rest_lower = rest.to_lowercase();
            if let Some(t) = tokens.iter().find(|t| rest_lower.starts_with(t.as_str())) {
                let n = t.chars().count();
                out.push(OPEN);
                out.extend(lower[i..(i + n).min(lower.len())].iter().map(|(_, c)| *c));
                out.push(CLOSE);
                i += n;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    out
}

fn dictation_title(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(8).collect();
    let mut t = words.join(" ");
    if text.split_whitespace().count() > 8 {
        t.push('…');
    }
    t
}

impl Store {
    /// Keyword search. Empty or punctuation-only queries return no hits.
    pub fn search(&self, query: &str, filters: &SearchFilters) -> Result<Vec<SearchHit>> {
        let tokens = search_tokens(query);
        let Some(fts) = fts_and_query(&tokens) else {
            return Ok(Vec::new());
        };
        let since = filters.since.map(ms).unwrap_or(i64::MIN);
        let limit = filters.limit.max(1);
        let meetings = matches!(filters.kind, SearchKind::All | SearchKind::Meetings);
        let actions = matches!(filters.kind, SearchKind::All | SearchKind::ActionItems);
        let dictations = matches!(filters.kind, SearchKind::All | SearchKind::Dictations);

        self.read(|c| {
            let mut hits = Vec::new();
            let snip = |raw: String| render_snippet(&raw);

            if meetings {
                // Titles: every token must appear (last as prefix — `instr` covers both).
                let mut sql = String::from("SELECT id, title, started_at FROM meetings WHERE started_at >= ?1");
                for i in 0..tokens.len().min(8) {
                    sql.push_str(&format!(" AND instr(lower(title), ?{}) > 0", i + 2));
                }
                sql.push_str(&format!(" ORDER BY started_at DESC LIMIT {limit}"));
                let mut args: Vec<rusqlite::types::Value> = vec![since.into()];
                args.extend(tokens.iter().take(8).map(|t| rusqlite::types::Value::from(t.clone())));
                let mut stmt = c.prepare_cached(&sql)?;
                let rows = stmt.query_map(params_from_iter(args), |r| {
                    Ok((parse_uuid(&r.get::<_, String>(0)?)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })?;
                for row in rows {
                    let (id, title, started) = row?;
                    let (snippet, snippet_html, highlights) = snip(mark_tokens(&title, &tokens));
                    hits.push(SearchHit {
                        kind: HitKind::Meeting,
                        meeting_id: Some(id),
                        dictation_id: None,
                        action_item_id: None,
                        title,
                        snippet,
                        snippet_html,
                        highlights,
                        at_ms: None,
                        date: from_ms(started),
                        score: 1000.0,
                    });
                }

                let mut stmt = c.prepare_cached(
                    "SELECT s.meeting_id, m.title, m.started_at, s.start_ms,
                            snippet(segments_fts, 0, char(57344), char(57345), '…', 16), bm25(segments_fts) AS rank
                     FROM segments_fts JOIN segments s ON s.id = segments_fts.rowid
                     JOIN meetings m ON m.id = s.meeting_id
                     WHERE segments_fts MATCH ?1 AND m.started_at >= ?2 ORDER BY rank LIMIT ?3",
                )?;
                let rows = stmt.query_map(rusqlite::params![fts, since, limit], |r| {
                    let (snippet, snippet_html, highlights) = snip(r.get(4)?);
                    Ok(SearchHit {
                        kind: HitKind::Transcript,
                        meeting_id: Some(parse_uuid(&r.get::<_, String>(0)?)?),
                        dictation_id: None,
                        action_item_id: None,
                        title: r.get(1)?,
                        snippet,
                        snippet_html,
                        highlights,
                        at_ms: Some(r.get::<_, i64>(3)? as u64),
                        date: from_ms(r.get(2)?),
                        score: -r.get::<_, f64>(5)?,
                    })
                })?;
                for h in rows {
                    hits.push(h?);
                }

                let mut stmt = c.prepare_cached(
                    "SELECT s.meeting_id, m.title, m.started_at,
                            snippet(summaries_fts, -1, char(57344), char(57345), '…', 16), bm25(summaries_fts, 2.0, 1.0, 1.0) AS rank
                     FROM summaries_fts JOIN summaries s ON s.id = summaries_fts.rowid
                     JOIN meetings m ON m.id = s.meeting_id
                     WHERE summaries_fts MATCH ?1 AND m.started_at >= ?2 ORDER BY rank LIMIT ?3",
                )?;
                let rows = stmt.query_map(rusqlite::params![fts, since, limit], |r| {
                    let (snippet, snippet_html, highlights) = snip(r.get(3)?);
                    Ok(SearchHit {
                        kind: HitKind::Summary,
                        meeting_id: Some(parse_uuid(&r.get::<_, String>(0)?)?),
                        dictation_id: None,
                        action_item_id: None,
                        title: r.get(1)?,
                        snippet,
                        snippet_html,
                        highlights,
                        at_ms: None,
                        date: from_ms(r.get(2)?),
                        score: -r.get::<_, f64>(4)?,
                    })
                })?;
                for h in rows {
                    hits.push(h?);
                }
            }

            if actions {
                let mut stmt = c.prepare_cached(
                    "SELECT a.id, a.meeting_id, m.title, m.started_at,
                            snippet(action_items_fts, 0, char(57344), char(57345), '…', 24), bm25(action_items_fts) AS rank
                     FROM action_items_fts JOIN action_items a ON a.id = action_items_fts.rowid
                     JOIN meetings m ON m.id = a.meeting_id
                     WHERE action_items_fts MATCH ?1 AND m.started_at >= ?2 ORDER BY rank LIMIT ?3",
                )?;
                let rows = stmt.query_map(rusqlite::params![fts, since, limit], |r| {
                    let (snippet, snippet_html, highlights) = snip(r.get(4)?);
                    Ok(SearchHit {
                        kind: HitKind::ActionItem,
                        meeting_id: Some(parse_uuid(&r.get::<_, String>(1)?)?),
                        dictation_id: None,
                        action_item_id: Some(r.get(0)?),
                        title: r.get(2)?,
                        snippet,
                        snippet_html,
                        highlights,
                        at_ms: None,
                        date: from_ms(r.get(3)?),
                        score: -r.get::<_, f64>(5)?,
                    })
                })?;
                for h in rows {
                    hits.push(h?);
                }
            }

            if dictations {
                let mut stmt = c.prepare_cached(
                    "SELECT d.id, d.text, d.created_at,
                            snippet(dictations_fts, 0, char(57344), char(57345), '…', 16), bm25(dictations_fts) AS rank
                     FROM dictations_fts JOIN dictations d ON d.seq = dictations_fts.rowid
                     WHERE dictations_fts MATCH ?1 AND d.created_at >= ?2 ORDER BY rank LIMIT ?3",
                )?;
                let rows = stmt.query_map(rusqlite::params![fts, since, limit], |r| {
                    let (snippet, snippet_html, highlights) = snip(r.get(3)?);
                    Ok(SearchHit {
                        kind: HitKind::Dictation,
                        meeting_id: None,
                        dictation_id: Some(parse_uuid(&r.get::<_, String>(0)?)?),
                        action_item_id: None,
                        title: dictation_title(&r.get::<_, String>(1)?),
                        snippet,
                        snippet_html,
                        highlights,
                        at_ms: None,
                        date: from_ms(r.get(2)?),
                        score: -r.get::<_, f64>(4)?,
                    })
                })?;
                for h in rows {
                    hits.push(h?);
                }
            }

            hits.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.date.cmp(&a.date)));
            hits.truncate(limit as usize);
            Ok(hits)
        })
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn snippet_rendering_escapes_and_ranges() {
        let raw = format!("a <b> {OPEN}café{CLOSE} 🙂 {OPEN}x{CLOSE}");
        let (plain, html, ranges) = render_snippet(&raw);
        assert_eq!(plain, "a <b> café 🙂 x");
        assert_eq!(html, "a &lt;b&gt; <mark>café</mark> 🙂 <mark>x</mark>");
        // "a <b> " = 6 units, café = 4, emoji = 2 UTF-16 units.
        assert_eq!(ranges, vec![[6, 10], [14, 15]]);
    }

    #[test]
    fn query_building() {
        let t = search_tokens("Budget, Q3 \"roadmap\"");
        assert_eq!(t, vec!["budget", "q3", "roadmap"]);
        assert_eq!(fts_and_query(&t).unwrap(), "\"budget\" \"q3\" \"roadmap\"*");
        assert_eq!(
            fts_or_query(&t).unwrap(),
            "\"budget\" OR \"q3\" OR \"roadmap\""
        );
        assert!(fts_and_query(&search_tokens("  ?! ")).is_none());
    }
}
