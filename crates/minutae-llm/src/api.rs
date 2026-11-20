//! Optional Advanced transport: direct HTTPS calls with the user's own API key
//! (Anthropic, OpenAI, Gemini). Keys are read from a [`SecretStore`] at call time
//! and are never logged.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use minutae_core::{FailureKind, Transport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::LlmTransport;
use crate::pricing::cost_usd;
use crate::secrets::SecretStore;
use crate::types::{Availability, LlmError, Request, Response, clip};

pub const ANTHROPIC_VERSION: &str = "2026-10-21";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiProvider {
    Anthropic,
    OpenAi,
    Gemini,
}

impl ApiProvider {
    pub fn id(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => "anthropic",
            ApiProvider::OpenAi => "openai",
            ApiProvider::Gemini => "gemini",
        }
    }
    /// Keychain account name for this provider's key.
    pub fn secret_account(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => "anthropic-api-key",
            ApiProvider::OpenAi => "openai-api-key",
            ApiProvider::Gemini => "gemini-api-key",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => "claude-opus-5-5",
            ApiProvider::OpenAi => "gpt-5-mini",
            ApiProvider::Gemini => "gemini-2.5-flash",
        }
    }
    pub fn default_base_url(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => "https://api.anthropic.com",
            ApiProvider::OpenAi => "https://api.openai.com",
            ApiProvider::Gemini => "https://generativelanguage.googleapis.com",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ApiProvider::Anthropic => "Anthropic",
            ApiProvider::OpenAi => "OpenAI",
            ApiProvider::Gemini => "Gemini",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiModel {
    pub provider: ApiProvider,
    pub model: String,
    /// Override for tests or compatible gateways. None = the provider's public endpoint.
    pub base_url: Option<String>,
}

impl ApiModel {
    pub fn new(provider: ApiProvider) -> Self {
        Self {
            provider,
            model: provider.default_model().into(),
            base_url: None,
        }
    }
    pub fn with_model(provider: ApiProvider, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            base_url: None,
        }
    }
}

/// Which providers to try (in order) and with which models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiConfig {
    pub providers: Vec<ApiModel>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            providers: vec![
                ApiModel::new(ApiProvider::Anthropic),
                ApiModel::new(ApiProvider::OpenAi),
                ApiModel::new(ApiProvider::Gemini),
            ],
        }
    }
}

pub struct ApiTransport {
    config: ApiConfig,
    secrets: Arc<dyn SecretStore>,
    client: reqwest::Client,
}

impl ApiTransport {
    pub fn new(config: ApiConfig, secrets: Arc<dyn SecretStore>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            config,
            secrets,
            client,
        }
    }

    fn keyed(&self) -> Result<Vec<(&ApiModel, String)>, LlmError> {
        let mut out = vec![];
        for m in &self.config.providers {
            if let Some(k) = self.secrets.get(m.provider.secret_account())? {
                out.push((m, k));
            }
        }
        Ok(out)
    }

    async fn call(&self, m: &ApiModel, key: &str, req: &Request) -> Result<Response, LlmError> {
        let base = m
            .base_url
            .as_deref()
            .unwrap_or(m.provider.default_base_url())
            .trim_end_matches('/');
        let builder = match m.provider {
            ApiProvider::Anthropic => self
                .client
                .post(format!("{base}/v1/messages"))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&anthropic_body(req, &m.model)),
            ApiProvider::OpenAi => self
                .client
                .post(format!("{base}/v1/chat/completions"))
                .bearer_auth(key)
                .json(&openai_body(req, &m.model)),
            ApiProvider::Gemini => self
                .client
                .post(format!("{base}/v1beta/models/{}:generateContent", m.model))
                .header("x-goog-api-key", key)
                .json(&gemini_body(req)),
        };
        let send = async {
            let resp = builder
                .timeout(req.timeout)
                .send()
                .await
                .map_err(map_reqwest)?;
            let status = resp.status().as_u16();
            let bytes = resp.bytes().await.map_err(map_reqwest)?;
            Ok::<_, LlmError>((status, bytes))
        };
        let (status, bytes) = tokio::select! {
            r = send => r?,
            _ = req.cancel.cancelled() => return Err(LlmError::cancelled()),
        };
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            return Err(classify_http(m.provider, status, &v));
        }
        let (text, model, tin, tout) = match m.provider {
            ApiProvider::Anthropic => parse_anthropic(&v)?,
            ApiProvider::OpenAi => parse_openai(&v)?,
            ApiProvider::Gemini => parse_gemini(&v)?,
        };
        let model = model.unwrap_or_else(|| m.model.clone());
        Ok(Response {
            cost_usd: cost_usd(m.provider.id(), &model, tin, tout),
            text,
            model,
            tokens_in: tin,
            tokens_out: tout,
            provider: m.provider.id().into(),
        })
    }
}

#[async_trait]
impl LlmTransport for ApiTransport {
    fn id(&self) -> Transport {
        Transport::Api
    }
    fn name(&self) -> &str {
        "API"
    }

    async fn availability(&self) -> Availability {
        match self.keyed() {
            Ok(k) if !k.is_empty() => Availability::Ready,
            Ok(_) => Availability::NotLoggedIn,
            Err(e) => Availability::Unavailable(e.detail),
        }
    }

    /// Tries each provider that has a key; moves on when one is rate limited or rejects the model.
    async fn complete(&self, req: &Request) -> Result<Response, LlmError> {
        let keyed = self.keyed()?;
        if keyed.is_empty() {
            return Err(LlmError::not_logged_in("No API key is saved."));
        }
        let mut last = None;
        for (m, key) in keyed {
            match self.call(m, &key, req).await {
                Ok(r) => return Ok(r),
                Err(e)
                    if matches!(
                        e.kind,
                        FailureKind::RateLimited
                            | FailureKind::ModelRejected
                            | FailureKind::NotLoggedIn
                    ) =>
                {
                    tracing::debug!(provider = m.provider.id(), kind = ?e.kind, "api provider skipped");
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| LlmError::unavailable("No API provider answered.")))
    }
}

/// (text, reported model, tokens in, tokens out)
type Parsed = (String, Option<String>, Option<u32>, Option<u32>);

fn map_reqwest(e: reqwest::Error) -> LlmError {
    if e.is_timeout() {
        LlmError::timeout()
    } else if e.is_connect() {
        LlmError::unavailable("Could not reach the API. Check the connection.")
    } else {
        LlmError::unavailable("The API request failed.")
    }
}

fn error_message(v: &Value) -> String {
    v.pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| v.get("message").and_then(Value::as_str))
        .or_else(|| v.get("error").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// HTTP status + body → failure kind. Messages are clipped; keys never appear in them.
pub(crate) fn classify_http(provider: ApiProvider, status: u16, body: &Value) -> LlmError {
    let msg = error_message(body);
    let lower = msg.to_lowercase();
    let detail = clip(&format!("{} HTTP {status}: {msg}", provider.label()), 240);
    match status {
        401 | 403 => LlmError::not_logged_in(detail),
        429 | 529 => LlmError::rate_limited(detail),
        404 => LlmError::model_rejected(detail),
        400 if lower.contains("api key") || lower.contains("api_key") => {
            LlmError::not_logged_in(detail)
        }
        400 if lower.contains("model") => LlmError::model_rejected(detail),
        400 | 413 | 422 => LlmError::invalid_output(detail),
        408 | 504 => LlmError::new(FailureKind::Timeout, detail),
        _ => LlmError::unavailable(detail),
    }
}

pub(crate) fn anthropic_body(req: &Request, model: &str) -> Value {
    let mut b = json!({
        "model": model,
        "max_tokens": req.max_tokens.unwrap_or(16_000),
        "messages": [{"role": "user", "content": req.prompt}],
    });
    if let Some(s) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        b["system"] = json!(s);
    }
    b
}

fn parse_anthropic(v: &Value) -> Result<Parsed, LlmError> {
    if v.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        return Err(LlmError::model_rejected("The model declined this request."));
    }
    let text: String = v
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err(LlmError::invalid_output("Anthropic returned no text."));
    }
    Ok((
        text,
        v.get("model").and_then(Value::as_str).map(String::from),
        u32_at(v, "/usage/input_tokens"),
        u32_at(v, "/usage/output_tokens"),
    ))
}

pub(crate) fn openai_body(req: &Request, model: &str) -> Value {
    let mut messages = vec![];
    if let Some(s) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        messages.push(json!({"role": "system", "content": s}));
    }
    messages.push(json!({"role": "user", "content": req.prompt}));
    let mut b = json!({"model": model, "messages": messages});
    if let Some(n) = req.max_tokens {
        b["max_completion_tokens"] = json!(n);
    }
    if req.json_schema.is_some() {
        b["response_format"] = json!({"type": "json_object"});
    }
    b
}

fn parse_openai(v: &Value) -> Result<Parsed, LlmError> {
    if let Some(r) = v
        .pointer("/choices/0/message/refusal")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        return Err(LlmError::model_rejected(clip(r, 200)));
    }
    let text = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| LlmError::invalid_output("OpenAI returned no text."))?;
    Ok((
        text.to_string(),
        v.get("model").and_then(Value::as_str).map(String::from),
        u32_at(v, "/usage/prompt_tokens"),
        u32_at(v, "/usage/completion_tokens"),
    ))
}

pub(crate) fn gemini_body(req: &Request) -> Value {
    let mut b = json!({"contents": [{"role": "user", "parts": [{"text": req.prompt}]}]});
    if let Some(s) = req.system.as_deref().filter(|s| !s.trim().is_empty()) {
        b["systemInstruction"] = json!({"parts": [{"text": s}]});
    }
    let mut cfg = serde_json::Map::new();
    if let Some(n) = req.max_tokens {
        cfg.insert("maxOutputTokens".into(), json!(n));
    }
    if req.json_schema.is_some() {
        cfg.insert("responseMimeType".into(), json!("application/json"));
    }
    if !cfg.is_empty() {
        b["generationConfig"] = Value::Object(cfg);
    }
    b
}

fn parse_gemini(v: &Value) -> Result<Parsed, LlmError> {
    if v.pointer("/promptFeedback/blockReason").is_some() {
        return Err(LlmError::model_rejected("The model declined this request."));
    }
    let text: String = v
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err(LlmError::invalid_output("Gemini returned no text."));
    }
    Ok((
        text,
        v.get("modelVersion")
            .and_then(Value::as_str)
            .map(String::from),
        u32_at(v, "/usageMetadata/promptTokenCount"),
        u32_at(v, "/usageMetadata/candidatesTokenCount"),
    ))
}

fn u32_at(v: &Value, ptr: &str) -> Option<u32> {
    v.pointer(ptr).and_then(Value::as_u64).map(|n| n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;
    use crate::testutil::MockServer;

    fn transport(
        server: &MockServer,
        providers: &[ApiProvider],
        keys: &[(&str, &str)],
    ) -> ApiTransport {
        let cfg = ApiConfig {
            providers: providers
                .iter()
                .map(|p| ApiModel {
                    provider: *p,
                    model: p.default_model().into(),
                    base_url: Some(server.url()),
                })
                .collect(),
        };
        ApiTransport::new(cfg, Arc::new(MemorySecretStore::with(keys)))
    }

    #[tokio::test]
    async fn anthropic_success_and_headers() {
        let server = MockServer::start(vec![(
            "/v1/messages",
            200,
            r#"{"model":"claude-opus-5-5","content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"a\":1}"}],"stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":100}}"#.into(),
        )])
        .await;
        let t = transport(
            &server,
            &[ApiProvider::Anthropic],
            &[("anthropic-api-key", "sk-test")],
        );
        assert_eq!(t.availability().await, Availability::Ready);
        let r = t.complete(&Request::new("p").system("s")).await.unwrap();
        assert_eq!(r.text, "{\"a\":1}");
        assert_eq!(r.provider, "anthropic");
        let cost = r.cost_usd.unwrap();
        assert!((cost - (1000.0 * 4.0 + 100.0 * 20.0) / 1e6).abs() < 1e-12);
        let (_, body, head) = &server.requests()[0];
        let head = head.to_lowercase();
        assert!(head.contains("x-api-key: sk-test"));
        assert!(head.contains("anthropic-version: 2026-10-21"));
        let b: Value = serde_json::from_str(body).unwrap();
        assert_eq!(b["system"], "s");
        assert!(b.get("temperature").is_none());
    }

    #[tokio::test]
    async fn falls_through_rate_limited_provider() {
        let server = MockServer::start(vec![
            ("/v1/chat/completions", 429, r#"{"error":{"message":"Rate limit reached"}}"#.into()),
            (
                "/v1beta/models/",
                200,
                r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]}}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":1},"modelVersion":"gemini-2.5-flash"}"#.into(),
            ),
        ])
        .await;
        let t = transport(
            &server,
            &[
                ApiProvider::Anthropic,
                ApiProvider::OpenAi,
                ApiProvider::Gemini,
            ],
            &[("openai-api-key", "o"), ("gemini-api-key", "g")],
        );
        let r = t
            .complete(&Request::new("p").json_schema(json!({})))
            .await
            .unwrap();
        assert_eq!((r.text.as_str(), r.provider.as_str()), ("ok", "gemini"));
        let reqs = server.requests();
        assert_eq!(reqs.len(), 2, "anthropic had no key and must not be called");
        assert!(reqs[1].2.to_lowercase().contains("x-goog-api-key: g"));
        let openai_body: Value = serde_json::from_str(&reqs[0].1).unwrap();
        assert_eq!(openai_body["response_format"]["type"], "json_object");
    }

    #[tokio::test]
    async fn no_keys_is_not_logged_in() {
        let server = MockServer::start(vec![]).await;
        let t = transport(&server, &[ApiProvider::Anthropic], &[]);
        assert_eq!(t.availability().await, Availability::NotLoggedIn);
        assert_eq!(
            t.complete(&Request::new("p")).await.unwrap_err().kind,
            FailureKind::NotLoggedIn
        );
    }

    #[test]
    fn http_classification() {
        let body = |m: &str| json!({"error": {"message": m}});
        let c = |s, m| classify_http(ApiProvider::OpenAi, s, &body(m)).kind;
        assert_eq!(
            c(401, "Incorrect API key provided"),
            FailureKind::NotLoggedIn
        );
        assert_eq!(
            c(429, "You exceeded your current quota"),
            FailureKind::RateLimited
        );
        assert_eq!(c(529, "Overloaded"), FailureKind::RateLimited);
        assert_eq!(
            c(404, "The model `gpt-x` does not exist"),
            FailureKind::ModelRejected
        );
        assert_eq!(
            c(400, "API key not valid. Please pass a valid API key."),
            FailureKind::NotLoggedIn
        );
        assert_eq!(c(400, "model: not_found"), FailureKind::ModelRejected);
        assert_eq!(c(500, "internal"), FailureKind::Unavailable);
    }

    #[test]
    fn refusals_are_rejections() {
        let v = json!({"stop_reason": "refusal", "content": []});
        assert_eq!(
            parse_anthropic(&v).unwrap_err().kind,
            FailureKind::ModelRejected
        );
    }
}
