//! minutae-search — retrieval for "Ask across meetings" (LIB-11).
//!
//! Transcripts are cut into fixed ~45 s windows (aligned to multiples of the
//! window length, so the same window always has the same bounds) and rendered
//! with speaker labels. Retrieval is keyword-first:
//!
//! 1. question → tokens, stopwords dropped, OR-ed into an FTS5 query;
//! 2. matching segments ranked by bm25, grouped into their windows;
//! 3. window score = Σ(−bm25) × keyword coverage × recency decay;
//! 4. optional semantic rerank through an [`Embedder`] (cosine similarity).
//!
//! The keyword path needs no model and is what the app ships first; an
//! embedding backend (e.g. fastembed / ONNX all-MiniLM) can be plugged in later
//! by implementing [`Embedder`] — no schema change needed for reranking.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use minutae_core::{MeetingId, Segment, Speaker, SpeakerId, Track};
use minutae_store::{Store, StoreError, fts_or_query, search_tokens};
use serde::{Deserialize, Serialize};

pub const DEFAULT_WINDOW_MS: u64 = 45_000;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("embedding failed: {0}")]
    Embedding(String),
}

/// A retrieved transcript window, ready to paste into an LLM prompt as a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub meeting_id: MeetingId,
    pub title: String,
    pub meeting_started_at: DateTime<Utc>,
    pub start_ms: u64,
    pub end_ms: u64,
    /// "Alice: …\nYou: …" lines.
    pub text: String,
    pub score: f64,
}

/// Text embedder for semantic rerank. Vectors should be L2-normalised or at
/// least comparable by cosine similarity.
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, SearchError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrieveOptions {
    pub window_ms: u64,
    /// Only meetings started at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Age (days) at which the recency factor halves.
    pub recency_half_life_days: f64,
    /// Weight of recency in [0, 1]; 0 = ignore dates.
    pub recency_weight: f64,
    /// Matching segments considered before grouping.
    pub candidate_segments: u32,
    /// Weight of embedding similarity in [0, 1] when an embedder is set.
    pub semantic_weight: f64,
}

impl Default for RetrieveOptions {
    fn default() -> Self {
        Self {
            window_ms: DEFAULT_WINDOW_MS,
            since: None,
            recency_half_life_days: 30.0,
            recency_weight: 0.3,
            candidate_segments: 300,
            semantic_weight: 0.5,
        }
    }
}

const STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "against", "all", "also", "am", "an", "and", "any",
    "are", "as", "at", "be", "because", "been", "before", "being", "below", "between", "both",
    "but", "by", "can", "could", "did", "do", "does", "doing", "down", "during", "each", "ever",
    "few", "for", "from", "further", "get", "got", "had", "has", "have", "having", "he", "her",
    "here", "hers", "him", "his", "how", "i", "if", "in", "into", "is", "it", "its", "just",
    "last", "me", "meeting", "meetings", "more", "most", "my", "no", "nor", "not", "now", "of",
    "off", "on", "once", "only", "or", "other", "our", "ours", "out", "over", "own", "said",
    "same", "say", "says", "she", "should", "so", "some", "such", "talk", "talked", "tell", "than",
    "that", "the", "their", "them", "then", "there", "these", "they", "this", "those", "through",
    "to", "too", "under", "until", "up", "us", "very", "was", "we", "were", "what", "when",
    "where", "which", "while", "who", "whom", "why", "will", "with", "would", "you", "your",
    "yours",
];

/// Question → search keywords (lowercase, deduplicated, stopwords removed).
pub fn keywords(question: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    let mut seen = HashSet::new();
    search_tokens(question)
        .into_iter()
        .filter(|t| {
            !stop.contains(t.as_str())
                && (t.chars().count() > 1 || t.chars().all(|c| c.is_numeric()))
        })
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// Label for a segment: the speaker's typed name, else its diarization label,
/// else "You" (mic) / "Them" (system).
fn label_for(seg: &Segment, speakers: &HashMap<SpeakerId, &Speaker>) -> String {
    if let Some(s) = seg.speaker_id.and_then(|id| speakers.get(&id)) {
        return s.display_name.clone().unwrap_or_else(|| s.label.clone());
    }
    match seg.track {
        Track::Mic => "You".into(),
        Track::System => "Them".into(),
    }
}

/// A window of transcript text: `(start_ms, end_ms, text)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Cut segments (sorted by start) into windows aligned to `window_ms`, with
/// consecutive lines from the same speaker merged. Used both for retrieval and
/// for building an embedding index.
pub fn chunk_segments(segments: &[Segment], speakers: &[Speaker], window_ms: u64) -> Vec<Window> {
    let window_ms = window_ms.max(1);
    let by_id: HashMap<SpeakerId, &Speaker> = speakers.iter().map(|s| (s.id, s)).collect();
    let mut out: Vec<Window> = Vec::new();
    // (window index, max end_ms, [(speaker, merged text)])
    type Open = (u64, u64, Vec<(String, String)>);
    let mut current: Option<Open> = None;
    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        let bucket = seg.start_ms / window_ms;
        if current.as_ref().is_some_and(|(b, _, _)| *b != bucket) {
            let (b, end, lines) = current.take().expect("checked");
            out.push(render_window(b * window_ms, end, lines));
        }
        let cur = current.get_or_insert_with(|| (bucket, 0, Vec::new()));
        cur.1 = cur.1.max(seg.end_ms);
        let who = label_for(seg, &by_id);
        match cur.2.last_mut() {
            Some((w, t)) if *w == who => {
                t.push(' ');
                t.push_str(text);
            }
            _ => cur.2.push((who, text.to_owned())),
        }
    }
    if let Some((b, end, lines)) = current {
        out.push(render_window(b * window_ms, end, lines));
    }
    out
}

fn render_window(start_ms: u64, end_ms: u64, lines: Vec<(String, String)>) -> Window {
    let text = lines
        .into_iter()
        .map(|(w, t)| format!("{w}: {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    Window {
        start_ms,
        end_ms: end_ms.max(start_ms),
        text,
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        dot += (*x as f64) * (*y as f64);
        na += (*x as f64).powi(2);
        nb += (*y as f64).powi(2);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Retrieval over a [`Store`]. Cheap to clone.
#[derive(Clone)]
pub struct Retriever {
    store: Store,
    embedder: Option<Arc<dyn Embedder>>,
    pub options: RetrieveOptions,
}

impl Retriever {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            embedder: None,
            options: RetrieveOptions::default(),
        }
    }

    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    pub fn with_options(mut self, options: RetrieveOptions) -> Self {
        self.options = options;
        self
    }

    /// Top `k` transcript windows for `question`, best first.
    /// Synchronous (SQLite); from async code wrap it in `spawn_blocking`.
    pub fn retrieve(&self, question: &str, k: usize) -> Result<Vec<Chunk>, SearchError> {
        self.retrieve_at(question, k, Utc::now())
    }

    /// [`Retriever::retrieve`] with an explicit "now" for recency (tests).
    pub fn retrieve_at(
        &self,
        question: &str,
        k: usize,
        now: DateTime<Utc>,
    ) -> Result<Vec<Chunk>, SearchError> {
        let kws = keywords(question);
        let Some(fts) = fts_or_query(&kws) else {
            return Ok(Vec::new());
        };
        if k == 0 {
            return Ok(Vec::new());
        }
        let o = &self.options;
        let window_ms = o.window_ms.max(1) as i64;
        let since = o.since.map(|t| t.timestamp_millis()).unwrap_or(i64::MIN);

        // (meeting, window index) -> (title, started_at ms, Σ -bm25)
        type Key = (String, i64);
        let windows: Vec<(Key, String, i64, f64)> = self.store.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT s.meeting_id, s.start_ms, m.title, m.started_at, bm25(segments_fts) AS rank
                 FROM segments_fts JOIN segments s ON s.id = segments_fts.rowid
                 JOIN meetings m ON m.id = s.meeting_id
                 WHERE segments_fts MATCH ?1 AND m.started_at >= ?2 ORDER BY rank LIMIT ?3",
            )?;
            let mut acc: HashMap<Key, (String, i64, f64)> = HashMap::new();
            let rows = stmt.query_map(
                minutae_store::rusqlite::params![fts, since, o.candidate_segments],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, f64>(4)?,
                    ))
                },
            )?;
            for row in rows {
                let (mid, start, title, started, rank) = row?;
                let e = acc
                    .entry((mid, start / window_ms))
                    .or_insert((title, started, 0.0));
                e.2 += -rank;
            }
            Ok(acc
                .into_iter()
                .map(|(k, (t, s, score))| (k, t, s, score))
                .collect())
        })?;

        // Pre-rank on keyword score × recency, then build text for the best few.
        let recency = |started_ms: i64| {
            let age_days = ((now.timestamp_millis() - started_ms) as f64 / 86_400_000.0).max(0.0);
            let decay = 0.5f64.powf(age_days / o.recency_half_life_days.max(0.01));
            (1.0 - o.recency_weight) + o.recency_weight * decay
        };
        let mut ranked: Vec<_> = windows
            .into_iter()
            .map(|(k, t, s, score)| (score * recency(s), k, t, s))
            .collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        ranked.truncate(k.saturating_mul(4).max(k));

        let mut speakers_cache: HashMap<String, Vec<Speaker>> = HashMap::new();
        let mut chunks = Vec::with_capacity(ranked.len());
        for (pre, (mid, bucket), title, started) in ranked {
            let meeting_id =
                uuid::Uuid::parse_str(&mid).map_err(|e| StoreError::Invalid(e.to_string()))?;
            if !speakers_cache.contains_key(&mid) {
                speakers_cache.insert(mid.clone(), self.store.list_speakers(meeting_id)?);
            }
            let from = (bucket * window_ms) as u64;
            let segs = self
                .store
                .list_segments(meeting_id, Some((from, from + window_ms as u64)))?;
            let Some(w) = chunk_segments(&segs, &speakers_cache[&mid], window_ms as u64)
                .into_iter()
                .next()
            else {
                continue;
            };
            let lower = w.text.to_lowercase();
            let covered = kws.iter().filter(|kw| lower.contains(kw.as_str())).count();
            let coverage = 1.0 + covered as f64 / kws.len() as f64;
            chunks.push(Chunk {
                meeting_id,
                title,
                meeting_started_at: chrono::DateTime::from_timestamp_millis(started)
                    .unwrap_or_default(),
                start_ms: w.start_ms,
                end_ms: w.end_ms,
                text: w.text,
                score: pre * coverage,
            });
        }

        if let Some(embedder) = &self.embedder
            && !chunks.is_empty()
        {
            let mut inputs = Vec::with_capacity(chunks.len() + 1);
            inputs.push(question.to_owned());
            inputs.extend(chunks.iter().map(|c| c.text.clone()));
            let vecs = embedder.embed(&inputs)?;
            if vecs.len() == inputs.len() {
                let max_kw = chunks
                    .iter()
                    .map(|c| c.score)
                    .fold(f64::MIN, f64::max)
                    .max(f64::EPSILON);
                let w = o.semantic_weight.clamp(0.0, 1.0);
                for (c, v) in chunks.iter_mut().zip(&vecs[1..]) {
                    c.score = (1.0 - w) * (c.score / max_kw) + w * cosine(&vecs[0], v);
                }
            } else {
                tracing::warn!(
                    got = vecs.len(),
                    want = inputs.len(),
                    "embedder returned wrong count; skipping rerank"
                );
            }
        }

        chunks.sort_by(|a, b| b.score.total_cmp(&a.score));
        chunks.truncate(k);
        Ok(chunks)
    }
}

/// Convenience: keyword retrieval with default options.
pub fn retrieve(store: &Store, question: &str, k: usize) -> Result<Vec<Chunk>, SearchError> {
    Retriever::new(store.clone()).retrieve(question, k)
}

#[cfg(test)]
mod tests;
