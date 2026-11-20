//! API / provider usage log (Settings → Usage).

use chrono::{DateTime, Utc};
use minutae_core::MeetingId;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Result, Store, ms};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub at: DateTime<Utc>,
    /// e.g. "openai", "anthropic", "ollama", "codex".
    pub provider: String,
    /// e.g. "codexCli", "api" (free-form; see `minutae_core::Transport`).
    pub transport: Option<String>,
    pub model: String,
    /// e.g. "summary", "ask", "tidy", "transcribe".
    pub purpose: String,
    pub meeting_id: Option<MeetingId>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAggregate {
    /// "YYYY-MM" (UTC).
    pub month: String,
    pub provider: String,
    pub calls: u64,
    pub failed_calls: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

impl Store {
    pub fn log_usage(&self, u: &UsageEntry) -> Result<()> {
        self.write(|c| {
            c.execute(
                "INSERT INTO api_usage (at, provider, transport, model, purpose, meeting_id, tokens_in, tokens_out, cost_usd, duration_ms, ok)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    ms(u.at),
                    u.provider,
                    u.transport,
                    u.model,
                    u.purpose,
                    u.meeting_id.map(|m| m.to_string()),
                    u.tokens_in as i64,
                    u.tokens_out as i64,
                    u.cost_usd,
                    u.duration_ms as i64,
                    u.ok
                ],
            )?;
            Ok(())
        })
    }

    /// Per-month, per-provider totals since `since`, newest month first.
    pub fn usage_by_month(&self, since: DateTime<Utc>) -> Result<Vec<UsageAggregate>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT strftime('%Y-%m', at / 1000, 'unixepoch') AS month, provider, count(*),
                        SUM(CASE WHEN ok THEN 0 ELSE 1 END), SUM(tokens_in), SUM(tokens_out), SUM(cost_usd)
                 FROM api_usage WHERE at >= ?1 GROUP BY month, provider ORDER BY month DESC, provider",
            )?;
            let rows = stmt.query_map([ms(since)], |r| {
                Ok(UsageAggregate {
                    month: r.get(0)?,
                    provider: r.get(1)?,
                    calls: r.get::<_, i64>(2)? as u64,
                    failed_calls: r.get::<_, i64>(3)? as u64,
                    tokens_in: r.get::<_, i64>(4)? as u64,
                    tokens_out: r.get::<_, i64>(5)? as u64,
                    cost_usd: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}
