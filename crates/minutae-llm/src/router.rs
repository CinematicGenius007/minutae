//! Ordered fallback across transports (port of LLMRouter.swift).
//!
//! - Tries transports in order; a transport whose availability is not `Ready` is skipped.
//! - Falls through on NotInstalled, NotLoggedIn, RateLimited, Timeout, ModelRejected, ModelMissing.
//! - Stops immediately on Cancelled; other errors (Unavailable, Io, …) stop and are returned,
//!   as the Swift router did.
//! - When every transport fails, the error carries the last real call's kind and transport;
//!   `NoProviderReady` means no transport was ready to call at all.
//! - JSON mode: invalid JSON gets one repair attempt on the same transport; a failed repair
//!   counts as InvalidOutput and the router moves on.
//! - Every call (success or failure) emits a [`UsageRecord`].

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use minutae_core::{AppSettings, Failure, FailureKind, Transport};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::LlmTransport;
use crate::api::{ApiConfig, ApiTransport};
use crate::codex::CodexTransport;
use crate::failure::failure_for;
use crate::grok::GrokTransport;
use crate::ollama::OllamaTransport;
use crate::secrets::SecretStore;
use crate::types::{Availability, LlmError, Purpose, Request, Response, kind_name, transport_id};

/// One AI call, for `api_usage_log`. Persistence belongs to the store crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    /// `codex-cli`, `grok-cli`, `ollama`, or `anthropic`/`openai`/`gemini` for the API transport.
    pub provider: String,
    pub transport: Transport,
    pub model: Option<String>,
    pub tokens_in: Option<u32>,
    pub tokens_out: Option<u32>,
    /// Only set for the API transport.
    pub cost_usd: Option<f64>,
    pub purpose: Purpose,
    pub duration_ms: u64,
    /// None on success, else a stable class like `rateLimited`.
    pub error_class: Option<String>,
    pub at: DateTime<Utc>,
}

pub type UsageSink = Arc<dyn Fn(UsageRecord) + Send + Sync>;

/// A sink that forwards records to an unbounded channel.
pub fn usage_channel() -> (UsageSink, mpsc::UnboundedReceiver<UsageRecord>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Arc::new(move |r| {
            let _ = tx.send(r);
        }),
        rx,
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub transport: Transport,
    pub kind: FailureKind,
    pub detail: String,
}

/// Why routing failed. `kind` is `NoProviderReady` only when every transport was skipped as
/// not ready; otherwise it is the kind of the last transport that was actually called
/// (e.g. `Timeout`, `RateLimited`, `InvalidOutput`), so callers can retry transient failures.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{kind:?} after {} attempt(s)", attempts.len())]
pub struct RouteError {
    pub kind: FailureKind,
    /// The transport that produced a stopping error, if any.
    pub transport: Option<Transport>,
    pub attempts: Vec<Attempt>,
}

impl RouteError {
    pub fn failure(&self) -> Failure {
        failure_for(self.kind, self.transport)
    }
    /// Every transport was tried or skipped. `last_real` indexes the last attempt that
    /// reached a transport call (as opposed to an availability skip).
    fn exhausted(attempts: Vec<Attempt>, last_real: Option<usize>) -> Self {
        match last_real.and_then(|i| attempts.get(i)) {
            Some(a) => Self {
                kind: a.kind,
                transport: Some(a.transport),
                attempts,
            },
            None => Self {
                kind: FailureKind::NoProviderReady,
                transport: None,
                attempts,
            },
        }
    }

    fn stop(t: Transport, e: LlmError, mut attempts: Vec<Attempt>) -> Self {
        attempts.push(Attempt {
            transport: t,
            kind: e.kind,
            detail: e.detail,
        });
        let kind = attempts
            .last()
            .map(|a| a.kind)
            .unwrap_or(FailureKind::Internal);
        Self {
            kind,
            transport: Some(t),
            attempts,
        }
    }
}

/// Successful routed call.
#[derive(Debug, Clone, PartialEq)]
pub struct Routed {
    pub response: Response,
    pub transport: Transport,
    /// Transports tried before this one and why they were skipped.
    pub skipped: Vec<Attempt>,
}

pub struct Router {
    order: Vec<Transport>,
    transports: Vec<(Transport, Arc<dyn LlmTransport>)>,
    usage: Option<UsageSink>,
}

/// Whether a failure moves on to the next transport.
pub fn falls_through(kind: FailureKind) -> bool {
    matches!(
        kind,
        FailureKind::NotInstalled
            | FailureKind::NotLoggedIn
            | FailureKind::RateLimited
            | FailureKind::Timeout
            | FailureKind::ModelRejected
            | FailureKind::ModelMissing
    )
}

impl Router {
    pub fn new(order: Vec<Transport>, transports: Vec<Arc<dyn LlmTransport>>) -> Self {
        let mut seen = Vec::new();
        for t in order {
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        Self {
            order: seen,
            transports: transports.into_iter().map(|t| (t.id(), t)).collect(),
            usage: None,
        }
    }

    /// The real transports, ordered per settings. `api` goes last and only when enabled.
    pub fn from_settings(
        settings: &AppSettings,
        secrets: Arc<dyn SecretStore>,
        api: ApiConfig,
    ) -> Self {
        let transports: Vec<Arc<dyn LlmTransport>> = vec![
            Arc::new(CodexTransport::new(settings.codex_model.as_deref())),
            Arc::new(GrokTransport::new(None)),
            Arc::new(OllamaTransport::new(Some(&settings.ollama_model))),
            Arc::new(ApiTransport::new(api, secrets)),
        ];
        Self::new(Self::order_from_settings(settings), transports)
    }

    pub fn order_from_settings(settings: &AppSettings) -> Vec<Transport> {
        let mut order: Vec<Transport> = settings
            .transport_order
            .iter()
            .copied()
            .filter(|t| *t != Transport::Api)
            .collect();
        if settings.api_fallback_enabled {
            order.push(Transport::Api);
        }
        order
    }

    pub fn with_usage_sink(mut self, sink: UsageSink) -> Self {
        self.usage = Some(sink);
        self
    }

    pub fn order(&self) -> &[Transport] {
        &self.order
    }

    pub fn transport(&self, id: Transport) -> Option<&Arc<dyn LlmTransport>> {
        self.transports
            .iter()
            .find(|(t, _)| *t == id)
            .map(|(_, t)| t)
    }

    /// Availability of every transport in order (for Settings).
    pub async fn statuses(&self) -> Vec<(Transport, Availability)> {
        let mut out = vec![];
        for id in &self.order {
            let a = match self.transport(*id) {
                Some(t) => t.availability().await,
                None => Availability::NotInstalled,
            };
            out.push((*id, a));
        }
        out
    }

    /// Plain text completion.
    pub async fn complete(&self, req: &Request) -> Result<Routed, RouteError> {
        let mut attempts = vec![];
        let mut last_real = None;
        for id in self.order.clone() {
            let Some(t) = self.ready(id, req, &mut attempts).await? else {
                continue;
            };
            match self.call(&t, req).await {
                Ok(response) => {
                    return Ok(Routed {
                        response,
                        transport: id,
                        skipped: attempts,
                    });
                }
                Err(e) => {
                    self.dispose(id, e, &mut attempts)?;
                    last_real = Some(attempts.len() - 1);
                }
            }
        }
        Err(RouteError::exhausted(attempts, last_real))
    }

    /// JSON completion decoded into `T`, with one repair attempt per transport.
    pub async fn complete_json<T: DeserializeOwned>(
        &self,
        req: &Request,
    ) -> Result<(T, Routed), RouteError> {
        let mut attempts = vec![];
        let mut last_real = None;
        for id in self.order.clone() {
            let Some(t) = self.ready(id, req, &mut attempts).await? else {
                continue;
            };
            let response = match self.call(&t, req).await {
                Ok(r) => r,
                Err(e) => {
                    self.dispose(id, e, &mut attempts)?;
                    last_real = Some(attempts.len() - 1);
                    continue;
                }
            };
            let first_err = match decode_json::<T>(&response.text) {
                Ok(v) => {
                    return Ok((
                        v,
                        Routed {
                            response,
                            transport: id,
                            skipped: attempts,
                        },
                    ));
                }
                Err(e) => e,
            };
            tracing::debug!(
                transport = transport_id(id),
                "invalid JSON, attempting repair"
            );
            let repair = repair_request(req, &first_err, &response.text);
            match self.call(&t, &repair).await {
                Ok(fixed) => match decode_json::<T>(&fixed.text) {
                    Ok(v) => {
                        return Ok((
                            v,
                            Routed {
                                response: fixed,
                                transport: id,
                                skipped: attempts,
                            },
                        ));
                    }
                    Err(e) => attempts.push(Attempt {
                        transport: id,
                        kind: FailureKind::InvalidOutput,
                        detail: e,
                    }),
                },
                Err(e) => self.dispose(id, e, &mut attempts)?,
            }
            last_real = Some(attempts.len() - 1);
        }
        Err(RouteError::exhausted(attempts, last_real))
    }

    async fn ready(
        &self,
        id: Transport,
        req: &Request,
        attempts: &mut Vec<Attempt>,
    ) -> Result<Option<Arc<dyn LlmTransport>>, RouteError> {
        if req.cancel.is_cancelled() {
            return Err(RouteError::stop(
                id,
                LlmError::cancelled(),
                std::mem::take(attempts),
            ));
        }
        let Some(t) = self.transport(id).cloned() else {
            attempts.push(Attempt {
                transport: id,
                kind: FailureKind::NotInstalled,
                detail: "not configured".into(),
            });
            return Ok(None);
        };
        match t.availability().await {
            Availability::Ready => Ok(Some(t)),
            other => {
                let kind = other.failure_kind().unwrap_or(FailureKind::Unavailable);
                let detail = match other {
                    Availability::Unavailable(r) => r,
                    _ => kind_name(kind).into(),
                };
                attempts.push(Attempt {
                    transport: id,
                    kind,
                    detail,
                });
                Ok(None)
            }
        }
    }

    /// Records a failure and decides whether to continue.
    fn dispose(
        &self,
        id: Transport,
        e: LlmError,
        attempts: &mut Vec<Attempt>,
    ) -> Result<(), RouteError> {
        if falls_through(e.kind) {
            tracing::info!(
                transport = transport_id(id),
                kind = kind_name(e.kind),
                "falling through"
            );
            attempts.push(Attempt {
                transport: id,
                kind: e.kind,
                detail: e.detail,
            });
            Ok(())
        } else {
            Err(RouteError::stop(id, e, std::mem::take(attempts)))
        }
    }

    async fn call(&self, t: &Arc<dyn LlmTransport>, req: &Request) -> Result<Response, LlmError> {
        let started = Instant::now();
        let result = if req.cancel.is_cancelled() {
            Err(LlmError::cancelled())
        } else {
            t.complete(req).await
        };
        let result = match result {
            Ok(_) if req.cancel.is_cancelled() => Err(LlmError::cancelled()),
            other => other,
        };
        if let Some(sink) = &self.usage {
            let duration_ms = started.elapsed().as_millis() as u64;
            let record = match &result {
                Ok(r) => UsageRecord {
                    provider: r.provider.clone(),
                    transport: t.id(),
                    model: Some(r.model.clone()),
                    tokens_in: r.tokens_in,
                    tokens_out: r.tokens_out,
                    cost_usd: r.cost_usd,
                    purpose: req.purpose,
                    duration_ms,
                    error_class: None,
                    at: Utc::now(),
                },
                Err(e) => UsageRecord {
                    provider: transport_id(t.id()).into(),
                    transport: t.id(),
                    model: None,
                    tokens_in: None,
                    tokens_out: None,
                    cost_usd: None,
                    purpose: req.purpose,
                    duration_ms,
                    error_class: Some(kind_name(e.kind).into()),
                    at: Utc::now(),
                },
            };
            sink(record);
        }
        result
    }
}

fn repair_request(original: &Request, error: &str, output: &str) -> Request {
    let mut r = original.clone();
    let schema = original
        .json_schema
        .as_ref()
        .map(|s| format!("\nSchema:\n{s}"))
        .unwrap_or_default();
    r.prompt = format!(
        "Repair the following JSON to satisfy the requested schema. Return only JSON.{schema}\nDecode error: {error}\nInvalid output:\n{output}"
    );
    r
}

/// Decodes JSON from model output, tolerating code fences and prose around one object.
pub fn decode_json<T: DeserializeOwned>(raw: &str) -> Result<T, String> {
    let candidate = extract_json(raw).ok_or_else(|| "no JSON object found".to_string())?;
    serde_json::from_str(candidate).map_err(|e| e.to_string())
}

/// The first balanced `{…}` or `[…]` in `raw` (after stripping ``` fences), string-aware.
pub fn extract_json(raw: &str) -> Option<&str> {
    let t = raw.trim();
    let start = t.find(['{', '['])?;
    let bytes = t.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match b {
                _ if esc => esc = false,
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&t[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FakeTransport;
    use serde::Deserialize;
    use tokio_util::sync::CancellationToken;

    #[derive(Debug, Deserialize, PartialEq)]
    struct J {
        a: u32,
    }

    fn router(ts: Vec<Arc<FakeTransport>>) -> Router {
        let order = ts.iter().map(|t| t.id).collect();
        Router::new(
            order,
            ts.into_iter().map(|t| t as Arc<dyn LlmTransport>).collect(),
        )
    }

    #[tokio::test]
    async fn first_ready_wins_and_order_is_respected() {
        let a = FakeTransport::new(Transport::Ollama, vec![Ok("from ollama")]);
        let b = FakeTransport::new(Transport::CodexCli, vec![Ok("from codex")]);
        let r = router(vec![a.clone(), b.clone()])
            .complete(&Request::new("x"))
            .await
            .unwrap();
        assert_eq!(r.response.text, "from ollama");
        assert_eq!(b.calls(), 0);
    }

    #[tokio::test]
    async fn skips_unready_and_fall_through_errors() {
        let a = FakeTransport::unavailable(Transport::CodexCli, Availability::NotInstalled);
        let b = FakeTransport::new(
            Transport::GrokCli,
            vec![Err(LlmError::rate_limited("busy"))],
        );
        let c = FakeTransport::new(Transport::Ollama, vec![Err(LlmError::timeout())]);
        let d = FakeTransport::new(Transport::Api, vec![Ok("api")]);
        let r = router(vec![a, b, c, d])
            .complete(&Request::new("x"))
            .await
            .unwrap();
        assert_eq!(r.transport, Transport::Api);
        let kinds: Vec<_> = r.skipped.iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            vec![
                FailureKind::NotInstalled,
                FailureKind::RateLimited,
                FailureKind::Timeout
            ]
        );
    }

    #[tokio::test]
    async fn stops_on_cancel_and_on_hard_errors() {
        let a = FakeTransport::new(Transport::CodexCli, vec![Err(LlmError::cancelled())]);
        let b = FakeTransport::new(Transport::Ollama, vec![Ok("never")]);
        let e = router(vec![a, b.clone()])
            .complete(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(e.kind, FailureKind::Cancelled);
        assert_eq!(b.calls(), 0);

        let a = FakeTransport::new(
            Transport::CodexCli,
            vec![Err(LlmError::unavailable("exit 2"))],
        );
        let b = FakeTransport::new(Transport::Ollama, vec![Ok("never")]);
        let e = router(vec![a, b.clone()])
            .complete(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.transport),
            (FailureKind::Unavailable, Some(Transport::CodexCli))
        );
        assert_eq!(b.calls(), 0);
    }

    #[tokio::test]
    async fn pre_cancelled_request_never_calls() {
        let a = FakeTransport::new(Transport::CodexCli, vec![Ok("x")]);
        let token = CancellationToken::new();
        token.cancel();
        let e = router(vec![a.clone()])
            .complete(&Request::new("x").cancel(token))
            .await
            .unwrap_err();
        assert_eq!(e.kind, FailureKind::Cancelled);
        assert_eq!(a.calls(), 0);
    }

    #[tokio::test]
    async fn all_skipped_is_no_provider_ready() {
        let a = FakeTransport::unavailable(Transport::CodexCli, Availability::NotLoggedIn);
        let b =
            FakeTransport::unavailable(Transport::Ollama, Availability::Unavailable("down".into()));
        let e = router(vec![a, b])
            .complete(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(e.kind, FailureKind::NoProviderReady);
        assert_eq!(e.attempts.len(), 2);
        assert_eq!(
            e.failure().message,
            "No summary provider is ready. Transcript is saved."
        );
    }

    #[tokio::test]
    async fn exhausted_reports_last_real_attempt_not_no_provider() {
        // Grok times out, Ollama is skipped as not ready: the job should see a retryable Timeout.
        let a = FakeTransport::new(Transport::GrokCli, vec![Err(LlmError::timeout())]);
        let b = FakeTransport::unavailable(Transport::Ollama, Availability::NotInstalled);
        let e = router(vec![a, b])
            .complete(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.transport),
            (FailureKind::Timeout, Some(Transport::GrokCli))
        );
        assert_eq!(e.attempts.len(), 2);

        let a = FakeTransport::new(Transport::CodexCli, vec![Err(LlmError::timeout())]);
        let b = FakeTransport::new(Transport::Ollama, vec![Err(LlmError::rate_limited("x"))]);
        let e = router(vec![a, b])
            .complete(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.transport),
            (FailureKind::RateLimited, Some(Transport::Ollama))
        );

        // JSON: invalid output twice, nothing else ready.
        let a = FakeTransport::new(Transport::CodexCli, vec![Ok("nope"), Ok("still nope")]);
        let b = FakeTransport::unavailable(Transport::Ollama, Availability::NotInstalled);
        let e = router(vec![a, b])
            .complete_json::<J>(&Request::new("x"))
            .await
            .unwrap_err();
        assert_eq!(
            (e.kind, e.transport),
            (FailureKind::InvalidOutput, Some(Transport::CodexCli))
        );
    }

    #[tokio::test]
    async fn json_repair_once_then_next_transport() {
        // Codex: garbage, repair fixes it.
        let a = FakeTransport::new(
            Transport::CodexCli,
            vec![Ok("Sure! {\"a\": oops}"), Ok("```json\n{\"a\": 7}\n```")],
        );
        let (v, r) = router(vec![a.clone()])
            .complete_json::<J>(&Request::new("x"))
            .await
            .unwrap();
        assert_eq!(v, J { a: 7 });
        assert_eq!(r.transport, Transport::CodexCli);
        assert!(a.prompts.lock().unwrap()[1].starts_with("Repair the following JSON"));

        // Codex: garbage twice → InvalidOutput, Ollama answers.
        let a = FakeTransport::new(Transport::CodexCli, vec![Ok("nope"), Ok("still nope")]);
        let b = FakeTransport::new(Transport::Ollama, vec![Ok("{\"a\":1}")]);
        let (v, r) = router(vec![a.clone(), b])
            .complete_json::<J>(&Request::new("x"))
            .await
            .unwrap();
        assert_eq!(v.a, 1);
        assert_eq!(a.calls(), 2, "exactly one repair attempt");
        assert_eq!(r.skipped[0].kind, FailureKind::InvalidOutput);
    }

    #[tokio::test]
    async fn usage_is_emitted_for_success_and_failure() {
        let a = FakeTransport::new(Transport::GrokCli, vec![Err(LlmError::rate_limited("x"))]);
        let b = FakeTransport::new(Transport::Ollama, vec![Ok("ok")]);
        let (sink, mut rx) = usage_channel();
        let r = router(vec![a, b]).with_usage_sink(sink);
        r.complete(&Request::new("x").purpose(Purpose::Ask))
            .await
            .unwrap();
        let first = rx.recv().await.unwrap();
        assert_eq!(first.error_class.as_deref(), Some("rateLimited"));
        assert_eq!(first.provider, "grok-cli");
        let second = rx.recv().await.unwrap();
        assert_eq!(
            (second.provider.as_str(), second.tokens_in, second.purpose),
            ("ollama", Some(10), Purpose::Ask)
        );
        assert!(second.error_class.is_none());
    }

    #[test]
    fn order_from_settings_puts_api_last_only_when_enabled() {
        let mut s = AppSettings {
            transport_order: vec![
                Transport::Api,
                Transport::Ollama,
                Transport::CodexCli,
                Transport::Ollama,
            ],
            ..Default::default()
        };
        let r = Router::new(Router::order_from_settings(&s), vec![]);
        assert_eq!(r.order(), &[Transport::Ollama, Transport::CodexCli]);
        s.api_fallback_enabled = true;
        assert_eq!(
            Router::order_from_settings(&s).last(),
            Some(&Transport::Api)
        );
    }

    #[test]
    fn extract_json_variants() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), Some("{\"a\":1}"));
        assert_eq!(
            extract_json("Here: {\"a\":\"}\"} trailing"),
            Some("{\"a\":\"}\"}")
        );
        assert_eq!(extract_json("no json"), None);
        assert_eq!(extract_json("{\"a\":"), None);
    }
}
