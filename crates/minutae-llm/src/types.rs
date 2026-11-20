//! Request/response/error types shared by every transport.

use std::time::Duration;

use minutae_core::{FailureKind, Transport};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Why a call is made. Recorded in usage rows; lets the Usage page split by feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Purpose {
    #[default]
    Summary,
    Title,
    Cleanup,
    Ask,
    /// The Settings "Test" action.
    Test,
}

/// One completion request. `cancel` lets the caller abort in-flight work
/// (the subprocess is killed, the HTTP request dropped).
#[derive(Debug, Clone)]
pub struct Request {
    pub system: Option<String>,
    pub prompt: String,
    /// When set, the caller wants JSON. Transports that support a schema pass it on;
    /// others ask for JSON mode only.
    pub json_schema: Option<Value>,
    pub max_tokens: Option<u32>,
    pub timeout: Duration,
    pub purpose: Purpose,
    pub cancel: CancellationToken,
}

impl Request {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            system: None,
            prompt: prompt.into(),
            json_schema: None,
            max_tokens: None,
            timeout: Duration::from_secs(120),
            purpose: Purpose::Summary,
            cancel: CancellationToken::new(),
        }
    }
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
    pub fn json_schema(mut self, schema: Value) -> Self {
        self.json_schema = Some(schema);
        self
    }
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }
    pub fn purpose(mut self, p: Purpose) -> Self {
        self.purpose = p;
        self
    }
    pub fn cancel(mut self, token: CancellationToken) -> Self {
        self.cancel = token;
        self
    }

    /// System and prompt joined, for CLIs that take a single text input.
    pub(crate) fn combined_prompt(&self) -> String {
        match self.system.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => format!("{s}\n\n{}", self.prompt),
            _ => self.prompt.clone(),
        }
    }
}

/// A completed call.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub text: String,
    /// Model that answered (reported by the provider when possible).
    pub model: String,
    pub tokens_in: Option<u32>,
    pub tokens_out: Option<u32>,
    /// Provider name for usage rows: the transport id, or `anthropic`/`openai`/`gemini` for `api`.
    pub provider: String,
    /// Only the API transport sets this.
    pub cost_usd: Option<f64>,
}

/// Cheap readiness probe used by the router before calling a transport and by Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state", content = "reason")]
pub enum Availability {
    Ready,
    NotInstalled,
    NotLoggedIn,
    Unavailable(String),
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        matches!(self, Availability::Ready)
    }
    pub fn failure_kind(&self) -> Option<FailureKind> {
        match self {
            Availability::Ready => None,
            Availability::NotInstalled => Some(FailureKind::NotInstalled),
            Availability::NotLoggedIn => Some(FailureKind::NotLoggedIn),
            Availability::Unavailable(_) => Some(FailureKind::Unavailable),
        }
    }
}

/// Classified failure. `detail` is for logs and Settings, never the prompt or output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct LlmError {
    pub kind: FailureKind,
    pub detail: String,
}

impl LlmError {
    pub fn new(kind: FailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
    pub fn not_installed(d: impl Into<String>) -> Self {
        Self::new(FailureKind::NotInstalled, d)
    }
    pub fn not_logged_in(d: impl Into<String>) -> Self {
        Self::new(FailureKind::NotLoggedIn, d)
    }
    pub fn rate_limited(d: impl Into<String>) -> Self {
        Self::new(FailureKind::RateLimited, d)
    }
    pub fn timeout() -> Self {
        Self::new(FailureKind::Timeout, "timed out")
    }
    pub fn cancelled() -> Self {
        Self::new(FailureKind::Cancelled, "cancelled")
    }
    pub fn invalid_output(d: impl Into<String>) -> Self {
        Self::new(FailureKind::InvalidOutput, d)
    }
    pub fn model_rejected(d: impl Into<String>) -> Self {
        Self::new(FailureKind::ModelRejected, d)
    }
    pub fn unavailable(d: impl Into<String>) -> Self {
        Self::new(FailureKind::Unavailable, d)
    }
}

/// Stable string ids used in usage rows and logs (matches the TypeScript `TransportId`).
pub fn transport_id(t: Transport) -> &'static str {
    match t {
        Transport::CodexCli => "codex-cli",
        Transport::GrokCli => "grok-cli",
        Transport::Ollama => "ollama",
        Transport::Api => "api",
    }
}

/// Human name used in failure copy.
pub fn transport_label(t: Transport) -> &'static str {
    match t {
        Transport::CodexCli => "Codex",
        Transport::GrokCli => "Grok",
        Transport::Ollama => "Ollama",
        Transport::Api => "The API provider",
    }
}

/// Stable lowercase class name for a failure kind (usage `error_class`).
pub fn kind_name(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::NotInstalled => "notInstalled",
        FailureKind::NotLoggedIn => "notLoggedIn",
        FailureKind::RateLimited => "rateLimited",
        FailureKind::Timeout => "timeout",
        FailureKind::Cancelled => "cancelled",
        FailureKind::InvalidOutput => "invalidOutput",
        FailureKind::ModelRejected => "modelRejected",
        FailureKind::Unavailable => "unavailable",
        FailureKind::NoProviderReady => "noProviderReady",
        FailureKind::PermissionDenied => "permissionDenied",
        FailureKind::ModelMissing => "modelMissing",
        FailureKind::Io => "io",
        FailureKind::Internal => "internal",
    }
}

/// Clip vendor messages before they go into `detail`.
pub(crate) fn clip(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut out: String = t.chars().take(max).collect();
        out.push('…');
        out
    }
}
