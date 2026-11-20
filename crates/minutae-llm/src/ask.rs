//! "Ask your meetings": answer only from retrieved context, cite sources.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::router::{RouteError, Router};
use crate::types::{Purpose, Request};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextChunk {
    pub meeting_title: String,
    pub start_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub text: String,
    /// 0-based indexes into the `chunks` passed to [`ask`].
    pub sources: Vec<usize>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub const NOT_FOUND: &str = "I couldn't find that in your meetings.";

const SYSTEM: &str = "You answer questions about the user's meetings using ONLY the numbered context passages provided. If the passages do not contain the answer, say you couldn't find it. Never use outside knowledge. Cite the passage numbers you used. Return JSON only.";

#[derive(Deserialize, Default)]
#[serde(default)]
struct Payload {
    answer: String,
    #[serde(alias = "citations")]
    sources: Vec<Value>,
}

pub fn prompt(question: &str, chunks: &[ContextChunk]) -> String {
    let mut ctx = String::new();
    for (i, c) in chunks.iter().enumerate() {
        ctx.push_str(&format!(
            "[{}] {} @ {}\n{}\n\n",
            i + 1,
            c.meeting_title,
            timestamp(c.start_ms),
            c.text.trim()
        ));
    }
    format!(
        "<context>\n{ctx}</context>\n\nQuestion: {question}\n\nReturn a JSON object: {{\"answer\": \"short answer in plain sentences\", \"sources\": [1, 2]}}\nsources lists the passage numbers that support the answer. If the context does not answer the question, return {{\"answer\": \"{NOT_FOUND}\", \"sources\": []}}."
    )
}

pub async fn ask(
    router: &Router,
    question: &str,
    chunks: &[ContextChunk],
) -> Result<Answer, RouteError> {
    if chunks.is_empty() || question.trim().is_empty() {
        return Ok(Answer {
            text: NOT_FOUND.into(),
            sources: vec![],
            provider: None,
            model: None,
        });
    }
    let schema = json!({
        "type": "object",
        "properties": {"answer": {"type": "string"}, "sources": {"type": "array", "items": {"type": "integer"}}},
        "required": ["answer", "sources"]
    });
    let req = Request::new(prompt(question, chunks))
        .system(SYSTEM)
        .json_schema(schema)
        .timeout(Duration::from_secs(60))
        .purpose(Purpose::Ask);
    let (p, routed): (Payload, _) = router.complete_json(&req).await?;
    let mut sources: Vec<usize> = p
        .sources
        .iter()
        .filter_map(|v| {
            v.as_u64().or_else(|| {
                v.as_str()
                    .and_then(|s| s.trim_matches(['[', ']', ' ']).parse().ok())
            })
        })
        .filter(|n| *n >= 1 && (*n as usize) <= chunks.len())
        .map(|n| n as usize - 1)
        .collect();
    sources.sort_unstable();
    sources.dedup();
    let text = if p.answer.trim().is_empty() {
        NOT_FOUND.to_string()
    } else {
        p.answer.trim().to_string()
    };
    Ok(Answer {
        text,
        sources,
        provider: Some(routed.response.provider),
        model: Some(routed.response.model),
    })
}

fn timestamp(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LlmTransport;
    use crate::testutil::FakeTransport;
    use minutae_core::Transport;
    use std::sync::Arc;

    fn chunks() -> Vec<ContextChunk> {
        vec![
            ContextChunk {
                meeting_title: "Sync".into(),
                start_ms: 65_000,
                text: "We ship in May.".into(),
            },
            ContextChunk {
                meeting_title: "Retro".into(),
                start_ms: 3_725_000,
                text: "Bob owns perf.".into(),
            },
        ]
    }

    #[tokio::test]
    async fn cites_valid_sources_only() {
        let t = FakeTransport::new(
            Transport::Ollama,
            vec![Ok(r#"{"answer":"May.","sources":[1, 9, "2", 1]}"#)],
        );
        let r = Router::new(
            vec![Transport::Ollama],
            vec![t.clone() as Arc<dyn LlmTransport>],
        );
        let a = ask(&r, "When do we ship?", &chunks()).await.unwrap();
        assert_eq!(a.text, "May.");
        assert_eq!(a.sources, vec![0, 1]);
        let p = &t.prompts.lock().unwrap()[0];
        assert!(p.contains("[1] Sync @ 1:05") && p.contains("[2] Retro @ 1:02:05"));
    }

    #[tokio::test]
    async fn empty_context_skips_the_model() {
        let r = Router::new(vec![], vec![]);
        let a = ask(&r, "anything", &[]).await.unwrap();
        assert_eq!(a.text, NOT_FOUND);
    }
}
