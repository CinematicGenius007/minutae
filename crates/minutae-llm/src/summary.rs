//! Meeting summaries: prompt, robust parsing, map-reduce for long transcripts,
//! and the markdown file (YAML frontmatter, CLAUDE.md "File Output Format").

use std::time::Duration;

use chrono::NaiveDateTime;
use minutae_core::{ActionItem, MeetingId, Summary, SummaryLength};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::router::{RouteError, Router};
use crate::types::{Purpose, Request};

/// Tokens per map chunk (≈4 chars per token, as in the Swift router).
pub const DEFAULT_TOKEN_BUDGET: usize = 8_000;
pub const TAKEAWAY_MAX: usize = 90;

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryMeta {
    pub meeting_id: MeetingId,
    pub title: String,
    /// Local wall-clock start time, written as `2026-04-11T14:30:00`.
    pub date: NaiveDateTime,
    pub duration: Option<Duration>,
    pub participants: Vec<String>,
    /// File name of the recording, e.g. `2026-04-11-abc123.wav`. Omitted from frontmatter when None.
    pub recording: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SummaryOptions {
    pub length: SummaryLength,
    pub token_budget: usize,
    pub timeout: Duration,
    pub cancel: tokio_util::sync::CancellationToken,
}

impl Default for SummaryOptions {
    fn default() -> Self {
        Self {
            length: SummaryLength::Brief,
            token_budget: DEFAULT_TOKEN_BUDGET,
            timeout: Duration::from_secs(180),
            cancel: Default::default(),
        }
    }
}

/// What the model returns. Lenient: accepts the legacy Swift shape too
/// (`summary[]`, `keyDecisions`, `title`) and string action items.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SummaryPayload {
    #[serde(deserialize_with = "text_or_list")]
    pub overview: String,
    #[serde(
        alias = "summary",
        alias = "highlights",
        deserialize_with = "string_list"
    )]
    pub points: Vec<String>,
    #[serde(deserialize_with = "text_or_list")]
    pub takeaway: String,
    #[serde(
        alias = "keyDecisions",
        alias = "key_decisions",
        deserialize_with = "string_list"
    )]
    pub decisions: Vec<String>,
    #[serde(alias = "action_items", deserialize_with = "action_list")]
    pub action_items: Vec<PayloadAction>,
    #[serde(
        alias = "title_suggestion",
        alias = "title",
        deserialize_with = "opt_text"
    )]
    pub title_suggestion: Option<String>,
    #[serde(deserialize_with = "string_list")]
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PayloadAction {
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
}

/// A generated summary plus what the router learned.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryOutcome {
    pub summary: Summary,
    pub title_suggestion: Option<String>,
    /// Summary bullets (rendered under `## Summary`).
    pub points: Vec<String>,
    pub participants: Vec<String>,
}

pub const SYSTEM: &str = "You are a meeting assistant. Given a meeting transcript, extract structured information and return it as JSON.\n\nBe concise and factual. Only include information that is clearly stated in the transcript. Never invent names, dates, decisions or tasks.";

const MAP_PREAMBLE: &str = "Summarise this part of a meeting transcript as plain notes. Be concise and factual. Only include information that is clearly stated. Keep speaker names, decisions, tasks with owners and due dates, and open questions.";

const REDUCE_PREAMBLE: &str = "Combine these partial meeting notes into one set of notes. Remove duplicates, keep every decision and task with its owner and due date, keep names.";

pub fn json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "overview": {"type": "string"},
            "points": {"type": "array", "items": {"type": "string"}},
            "takeaway": {"type": "string"},
            "decisions": {"type": "array", "items": {"type": "string"}},
            "actionItems": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "text": {"type": "string"},
                        "owner": {"type": ["string", "null"]},
                        "due": {"type": ["string", "null"]}
                    },
                    "required": ["text", "owner", "due"]
                }
            },
            "titleSuggestion": {"type": "string"},
            "participants": {"type": "array", "items": {"type": "string"}}
        },
        "required": ["overview", "points", "takeaway", "decisions", "actionItems", "titleSuggestion", "participants"]
    })
}

pub fn user_prompt(
    transcript: &str,
    meta: &SummaryMeta,
    length: SummaryLength,
    from_notes: bool,
) -> String {
    let (overview_rule, points_rule) = match length {
        SummaryLength::Brief => ("2-3 sentences", "3-5 concise bullet points"),
        SummaryLength::Detailed => (
            "one paragraph of 4-6 sentences",
            "5-10 bullet points covering every topic discussed",
        ),
    };
    let what = if from_notes {
        "notes taken from a long meeting"
    } else {
        "the meeting transcript"
    };
    let participants = if meta.participants.is_empty() {
        "unknown".to_string()
    } else {
        meta.participants.join(", ")
    };
    format!(
        r#"Meeting: "{title}" on {date}. Known participants: {participants}.

Here is {what}:

<transcript>
{transcript}
</transcript>

Return a JSON object with exactly this shape:
{{
  "overview": "What the meeting was about and what came out of it",
  "points": ["bullet point 1", "bullet point 2"],
  "takeaway": "The single most important outcome, at most 90 characters",
  "decisions": ["decision 1"],
  "actionItems": [{{"text": "Share updated mockups", "owner": "Alice", "due": "Friday"}}],
  "titleSuggestion": "A short, descriptive meeting title (max 60 chars)",
  "participants": ["Alice", "Bob"]
}}

Rules:
- overview: {overview_rule}
- points: {points_rule}
- takeaway: one line, at most 90 characters, no trailing period needed
- decisions: things that were explicitly decided (can be empty)
- actionItems: concrete tasks assigned to people (can be empty). owner and due are null when not stated
- participants: names mentioned or speaking (can be empty)
- Return ONLY the JSON object, no markdown fences, no explanation"#,
        title = meta.title.replace('"', "'"),
        date = meta.date.format("%Y-%m-%d"),
    )
}

/// Summarise a transcript (`Speaker: text` lines). Long transcripts are reduced first.
pub async fn summarize(
    router: &Router,
    transcript: &str,
    meta: &SummaryMeta,
    opts: &SummaryOptions,
) -> Result<SummaryOutcome, RouteError> {
    let budget_chars = opts.token_budget.max(1) * 4;
    let (input, from_notes) = if transcript.chars().count() <= budget_chars {
        (transcript.to_string(), false)
    } else {
        (map_reduce(router, transcript, opts).await?, true)
    };
    let req = Request::new(user_prompt(&input, meta, opts.length, from_notes))
        .system(SYSTEM)
        .json_schema(json_schema())
        .timeout(opts.timeout)
        .purpose(Purpose::Summary)
        .cancel(opts.cancel.clone());
    let (payload, routed): (SummaryPayload, _) = router.complete_json(&req).await?;
    Ok(build(
        payload,
        meta,
        &routed.response.provider,
        &routed.response.model,
    ))
}

/// Map each chunk to notes, then reduce until the notes fit one budget.
async fn map_reduce(
    router: &Router,
    transcript: &str,
    opts: &SummaryOptions,
) -> Result<String, RouteError> {
    let limit = opts.token_budget.max(1) * 4;
    let base = |prompt: String| {
        Request::new(prompt)
            .timeout(opts.timeout)
            .purpose(Purpose::Summary)
            .cancel(opts.cancel.clone())
    };
    let mut notes = vec![];
    for part in chunk_text(transcript, limit) {
        let r = router
            .complete(&base(format!("{MAP_PREAMBLE}\n\n{part}")))
            .await?;
        notes.push(r.response.text);
    }
    for _ in 0..16 {
        let joined = notes.join("\n\n");
        if joined.chars().count() <= limit {
            return Ok(joined);
        }
        let mut next = vec![];
        for group in group_to_fit(&notes, limit) {
            let r = router
                .complete(&base(format!("{REDUCE_PREAMBLE}\n\n{group}")))
                .await?;
            next.push(r.response.text);
        }
        // Guard against a reduce step that doesn't shrink anything.
        if next.len() >= notes.len() && next.join("").chars().count() >= joined.chars().count() {
            return Ok(truncate_chars(&next.join("\n\n"), limit));
        }
        notes = next;
    }
    Ok(truncate_chars(&notes.join("\n\n"), limit))
}

/// Splits on line boundaries into chunks of at most `max_chars` (long lines are split by chars).
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.max(1);
    let mut out = vec![];
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for line in text.lines() {
        let len = line.chars().count();
        if len > max {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            let chars: Vec<char> = line.chars().collect();
            out.extend(chars.chunks(max).map(|c| c.iter().collect::<String>()));
            continue;
        }
        let extra = if cur.is_empty() { len } else { len + 1 };
        if cur_len + extra > max {
            out.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if !cur.is_empty() {
            cur.push('\n');
            cur_len += 1;
        }
        cur.push_str(line);
        cur_len += len;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn group_to_fit(items: &[String], limit: usize) -> Vec<String> {
    let mut groups = vec![];
    let mut group = String::new();
    for item in items.iter().flat_map(|i| chunk_text(i, limit)) {
        let candidate_len = group.chars().count() + 2 + item.chars().count();
        if !group.is_empty() && candidate_len > limit {
            groups.push(std::mem::take(&mut group));
        }
        if !group.is_empty() {
            group.push_str("\n\n");
        }
        group.push_str(&item);
    }
    if !group.is_empty() {
        groups.push(group);
    }
    groups
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Validates and normalises a payload into the core `Summary` (with markdown).
pub fn build(p: SummaryPayload, meta: &SummaryMeta, provider: &str, model: &str) -> SummaryOutcome {
    let clean = |v: Vec<String>| -> Vec<String> {
        v.into_iter()
            .map(|s| one_line(&s))
            .map(|s| s.trim_start_matches(['-', '*', '•', ' ']).to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let points = clean(p.points);
    let decisions = clean(p.decisions);
    let mut overview = p.overview.trim().to_string();
    if overview.is_empty() {
        overview = points.join(" ");
    }
    let takeaway = shorten(
        &if p.takeaway.trim().is_empty() {
            first_sentence(&overview)
        } else {
            one_line(&p.takeaway)
        },
        TAKEAWAY_MAX,
    );
    let action_items: Vec<ActionItem> = p
        .action_items
        .into_iter()
        .filter(|a| !a.text.trim().is_empty())
        .map(|a| ActionItem {
            text: one_line(&a.text),
            owner: a.owner,
            due: a.due,
            done: false,
        })
        .collect();
    let mut participants: Vec<String> = meta.participants.clone();
    for name in clean(p.participants) {
        if !participants.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            participants.push(name);
        }
    }
    let title_suggestion = p
        .title_suggestion
        .map(|t| shorten(&one_line(&t), 60))
        .filter(|t| !t.is_empty());
    let bullets = if points.is_empty() && !overview.is_empty() {
        vec![one_line(&overview)]
    } else {
        points.clone()
    };
    let markdown = render_markdown(meta, &participants, &bullets, &decisions, &action_items);
    SummaryOutcome {
        summary: Summary {
            meeting_id: meta.meeting_id,
            overview,
            decisions,
            action_items,
            takeaway,
            markdown,
            provider: provider.to_string(),
            model: model.to_string(),
        },
        title_suggestion,
        points,
        participants,
    }
}

/// The `.md` file: YAML frontmatter, then `## Summary`, `## Key Decisions`, `## Action Items`.
/// Decisions and action items are omitted when empty (as the legacy writer did).
pub fn render_markdown(
    meta: &SummaryMeta,
    participants: &[String],
    points: &[String],
    decisions: &[String],
    actions: &[ActionItem],
) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!("id: {}\n", meta.meeting_id));
    s.push_str(&format!(
        "title: \"{}\"\n",
        yaml_escape(&one_line(&meta.title))
    ));
    s.push_str(&format!(
        "date: {}\n",
        meta.date.format("%Y-%m-%dT%H:%M:%S")
    ));
    s.push_str(&format!("duration: {}\n", duration_text(meta.duration)));
    let names: Vec<String> = participants.iter().map(|p| yaml_flow_item(p)).collect();
    s.push_str(&format!("participants: [{}]\n", names.join(", ")));
    s.push_str("tags: []\n");
    if let Some(rec) = meta
        .recording
        .as_deref()
        .map(safe_file_name)
        .filter(|r| !r.is_empty())
    {
        s.push_str(&format!("recording: ../recordings/{rec}\n"));
    }
    s.push_str("---\n\n## Summary\n");
    for p in points {
        s.push_str(&format!("- {p}\n"));
    }
    if !decisions.is_empty() {
        s.push_str("\n## Key Decisions\n");
        for d in decisions {
            s.push_str(&format!("- {d}\n"));
        }
    }
    if !actions.is_empty() {
        s.push_str("\n## Action Items\n");
        for a in actions {
            s.push_str(&format!(
                "- [{}] {}\n",
                if a.done { "x" } else { " " },
                checklist_text(a)
            ));
        }
    }
    s
}

/// `Owner: text (due X)`, skipping parts already written into the text (legacy behaviour).
pub fn checklist_text(a: &ActionItem) -> String {
    let mut line = a.text.clone();
    let has = |line: &str, part: &str| line.to_lowercase().contains(&part.to_lowercase());
    if let Some(o) = a.owner.as_deref().filter(|o| !o.is_empty())
        && !has(&line, o)
    {
        line = format!("{o}: {line}");
    }
    if let Some(d) = a.due.as_deref().filter(|d| !d.is_empty())
        && !has(&line, d)
    {
        line = format!("{line} (due {d})");
    }
    line
}

/// `formatDurationLong`: `45s`, `32m 14s`, `1h 5m`; zero/None → `unknown`.
pub fn duration_text(d: Option<Duration>) -> String {
    let secs = d.map(|d| d.as_secs()).unwrap_or(0);
    if secs == 0 {
        return "unknown".into();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m {s}s")
    }
}

fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn yaml_flow_item(s: &str) -> String {
    let s = one_line(s);
    let needs_quotes = s.is_empty()
        || s.chars().any(|c| ",[]{}:#&*!|>'\"%@`".contains(c))
        || s.starts_with(['-', '?', ' '])
        || s.ends_with(' ');
    if needs_quotes {
        format!("\"{}\"", yaml_escape(&s))
    } else {
        s
    }
}

fn safe_file_name(s: &str) -> String {
    let name = s.rsplit(['/', '\\']).next().unwrap_or("");
    if name == "." || name == ".." {
        String::new()
    } else {
        one_line(name)
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_sentence(s: &str) -> String {
    let s = one_line(s);
    match s.find(". ") {
        Some(i) => s[..i + 1].to_string(),
        None => s,
    }
}

/// At most `max` chars, cut at a word boundary with an ellipsis.
pub fn shorten(s: &str, max: usize) -> String {
    let s = one_line(s);
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max - 1).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > max / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches([' ', ',', ';', ':', '.', '-']))
}

// ---- lenient deserializers ----

fn text_or_list<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Array(a) => a
            .iter()
            .filter_map(value_text)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    })
}

fn opt_text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(value_text(&Value::deserialize(d)?).filter(|s| !s.trim().is_empty()))
}

fn string_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a
            .iter()
            .filter_map(value_text)
            .filter(|s| !s.trim().is_empty())
            .collect(),
        Value::String(s) if !s.trim().is_empty() => vec![s],
        _ => vec![],
    })
}

fn action_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PayloadAction>, D::Error> {
    let Value::Array(items) = Value::deserialize(d)? else {
        return Ok(vec![]);
    };
    Ok(items
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(PayloadAction {
                text: s.clone(),
                owner: None,
                due: None,
            }),
            Value::Object(o) => {
                let text = o
                    .get("text")
                    .or_else(|| o.get("task"))
                    .and_then(value_text)?;
                let field = |k: &str| {
                    o.get(k)
                        .and_then(value_text)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty() && s != "null")
                };
                Some(PayloadAction {
                    text,
                    owner: field("owner").or_else(|| field("assignee")),
                    due: field("due").or_else(|| field("dueDate")),
                })
            }
            _ => None,
        })
        .collect())
}

fn value_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LlmTransport;
    use crate::testutil::FakeTransport;
    use minutae_core::Transport;
    use std::sync::Arc;

    fn meta() -> SummaryMeta {
        SummaryMeta {
            meeting_id: uuid::Uuid::parse_str("6f1c2a3b-0000-4000-8000-000000000001").unwrap(),
            title: "Q2 Design Sync".into(),
            date: NaiveDateTime::parse_from_str("2026-04-11T14:30:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
            duration: Some(Duration::from_secs(32 * 60 + 14)),
            participants: vec!["Alice".into(), "Bob".into()],
            recording: Some("2026-04-11-abc123.wav".into()),
        }
    }

    const GOOD: &str = r#"{
      "overview": "The team reviewed the redesign timeline. Launch moves to May.",
      "points": ["Decided to push the redesign launch to May", "Performance budget needs review"],
      "takeaway": "Redesign launch moves to May",
      "decisions": ["Push the redesign launch to May"],
      "actionItems": [
        {"text": "Share updated mockups", "owner": "Alice", "due": "Friday"},
        {"text": "Bob to review performance budget", "owner": "Bob", "due": null}
      ],
      "titleSuggestion": "Redesign timeline",
      "participants": ["Alice", "Carol"]
    }"#;

    const GOLDEN: &str = "---
id: 6f1c2a3b-0000-4000-8000-000000000001
title: \"Q2 Design Sync\"
date: 2026-04-11T14:30:00
duration: 32m 14s
participants: [Alice, Bob, Carol]
tags: []
recording: ../recordings/2026-04-11-abc123.wav
---

## Summary
- Decided to push the redesign launch to May
- Performance budget needs review

## Key Decisions
- Push the redesign launch to May

## Action Items
- [ ] Alice: Share updated mockups (due Friday)
- [ ] Bob to review performance budget
";

    #[test]
    fn markdown_golden() {
        let p: SummaryPayload = serde_json::from_str(GOOD).unwrap();
        let out = build(p, &meta(), "codex-cli", "gpt-test");
        assert_eq!(out.summary.markdown, GOLDEN);
        assert_eq!(out.summary.takeaway, "Redesign launch moves to May");
        assert_eq!(out.title_suggestion.as_deref(), Some("Redesign timeline"));
        assert_eq!(out.summary.action_items[0].owner.as_deref(), Some("Alice"));
        assert_eq!(out.summary.action_items[1].due, None);
    }

    #[test]
    fn legacy_shape_and_messy_values_parse() {
        let p: SummaryPayload = serde_json::from_str(
            r#"{"title":"Old","summary":["a","b"],"keyDecisions":"only one","actionItems":["Do X", {"task":"Do Y","assignee":"Al","due":""}],"participants":null}"#,
        )
        .unwrap();
        assert_eq!(p.points, vec!["a", "b"]);
        assert_eq!(p.decisions, vec!["only one"]);
        assert_eq!(
            p.action_items[1],
            PayloadAction {
                text: "Do Y".into(),
                owner: Some("Al".into()),
                due: None
            }
        );
        assert_eq!(p.title_suggestion.as_deref(), Some("Old"));
        let out = build(p, &meta(), "x", "y");
        // No overview: falls back to the points; takeaway derived.
        assert_eq!(out.summary.overview, "a b");
        assert!(!out.summary.takeaway.is_empty());
    }

    #[test]
    fn empty_sections_and_yaml_quoting() {
        let mut m = meta();
        m.title = "Plan: \"v4\"".into();
        m.participants = vec!["O'Neil, Jr".into()];
        m.duration = None;
        m.recording = None;
        let md = render_markdown(&m, &m.participants, &["x".into()], &[], &[]);
        assert!(md.contains("title: \"Plan: \\\"v4\\\"\"\n"));
        assert!(md.contains("participants: [\"O'Neil, Jr\"]\n"));
        assert!(md.contains("duration: unknown\n"));
        assert!(!md.contains("recording:"));
        assert!(!md.contains("## Key Decisions") && !md.contains("## Action Items"));
    }

    #[test]
    fn takeaway_is_capped() {
        let p = SummaryPayload {
            overview: "o".into(),
            takeaway: "word ".repeat(40),
            ..Default::default()
        };
        let out = build(p, &meta(), "x", "y");
        assert!(out.summary.takeaway.chars().count() <= TAKEAWAY_MAX);
        assert!(out.summary.takeaway.ends_with('…'));
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(Some(Duration::from_secs(45))), "45s");
        assert_eq!(duration_text(Some(Duration::from_secs(3900))), "1h 5m");
        assert_eq!(duration_text(Some(Duration::ZERO)), "unknown");
    }

    #[test]
    fn chunking_respects_lines_and_limits() {
        let text = "Alice: hello there\nBob: hi\n".repeat(50) + &"x".repeat(250);
        let chunks = chunk_text(&text, 100);
        assert!(chunks.iter().all(|c| c.chars().count() <= 100));
        assert!(chunks[0].starts_with("Alice:") && !chunks[0].ends_with('\n'));
        let rejoined: usize = chunks.iter().map(|c| c.chars().count()).sum();
        assert!(
            rejoined >= text.chars().count() - 101,
            "no content lost except newlines"
        );
        // Multibyte safety.
        assert_eq!(chunk_text(&"é".repeat(10), 3).len(), 4);
    }

    #[tokio::test]
    async fn short_transcript_is_one_json_call() {
        let t = FakeTransport::new(Transport::CodexCli, vec![Ok(GOOD)]);
        let router = Router::new(
            vec![Transport::CodexCli],
            vec![t.clone() as Arc<dyn LlmTransport>],
        );
        let out = summarize(
            &router,
            "Alice: hi\nBob: hello",
            &meta(),
            &SummaryOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(t.calls(), 1);
        assert_eq!(out.summary.provider, "codex-cli");
        assert_eq!(out.summary.model, "fake-model");
        assert!(t.prompts.lock().unwrap()[0].contains("Alice: hi\nBob: hello"));
    }

    #[tokio::test]
    async fn long_transcript_map_reduces() {
        // Budget 25 tokens = 100 chars; 10 lines of ~40 chars → 4-5 map calls, then the final JSON call.
        let transcript: String = (0..10)
            .map(|i| format!("Speaker {i}: we discussed item number {i:03}\n"))
            .collect();
        let n_chunks = chunk_text(&transcript, 100).len();
        let mut script: Vec<Result<&str, crate::LlmError>> = vec![Ok("notes"); n_chunks];
        script.push(Ok(GOOD));
        let t = FakeTransport::new(Transport::Ollama, vec![]);
        *t.script.lock().unwrap() = script.into_iter().map(|r| r.map(String::from)).collect();
        let router = Router::new(
            vec![Transport::Ollama],
            vec![t.clone() as Arc<dyn LlmTransport>],
        );
        let opts = SummaryOptions {
            token_budget: 25,
            ..Default::default()
        };
        let out = summarize(&router, &transcript, &meta(), &opts).await;
        let prompts = t.prompts.lock().unwrap().clone();
        let maps = prompts
            .iter()
            .filter(|p| p.starts_with("Summarise this part"))
            .count();
        assert!(maps >= 4 && maps == n_chunks, "map calls: {maps}");
        let last = prompts.last().unwrap();
        assert!(last.contains("notes taken from a long meeting"));
        assert!(
            !last.contains("Speaker 3:"),
            "final call sees notes, not the raw transcript"
        );
        // The final call may be followed by nothing else; the outcome parses.
        let out = out.unwrap();
        assert_eq!(out.summary.decisions.len(), 1);
    }
}
