//! minutae-llm — the AI layer (port of the Swift `MinutaeLLM` package).
//!
//! - [`LlmTransport`]: one way to reach a model — `codex` CLI, `grok` CLI, local Ollama, or the
//!   optional API-key transport (Anthropic / OpenAI / Gemini).
//! - [`Router`]: ordered fallback across transports with the Swift router's skip/stop rules,
//!   JSON repair, and usage records.
//! - [`summarize`], [`ask`], [`tidy_dictation`]: the features built on top.
//! - [`failure_for`]: failure kind → one plain sentence + one fix.
//!
//! Safety rules: CLIs run by resolved real path (never a shell or shell function), with the
//! prompt on stdin or in a private temp file, in an empty `0700` temp directory, with a filtered
//! environment, and never with skip-permission flags. Other apps' credentials are never read.

pub mod api;
pub mod ask;
pub mod codex;
pub mod failure;
pub mod grok;
pub mod ollama;
pub mod pricing;
pub mod router;
pub mod secrets;
pub mod subprocess;
pub mod summary;
pub mod tidy;
pub mod types;

#[cfg(test)]
pub(crate) mod testutil;

use async_trait::async_trait;
use minutae_core::Transport;

pub use api::{ApiConfig, ApiModel, ApiProvider, ApiTransport};
pub use ask::{Answer, ContextChunk, ask};
pub use codex::CodexTransport;
pub use failure::failure_for;
pub use grok::GrokTransport;
pub use ollama::{OllamaStatus, OllamaTransport};
pub use router::{Attempt, RouteError, Routed, Router, UsageRecord, UsageSink, usage_channel};
pub use secrets::{KeyringStore, MemorySecretStore, SecretStore};
pub use summary::{SummaryMeta, SummaryOptions, SummaryOutcome, summarize};
pub use tidy::{Tidied, tidy_dictation};
pub use tokio_util::sync::CancellationToken;
pub use types::{Availability, LlmError, Purpose, Request, Response};

/// One way to reach a model.
#[async_trait]
pub trait LlmTransport: Send + Sync {
    fn id(&self) -> Transport;
    /// Display name, e.g. "Codex".
    fn name(&self) -> &str;
    /// Cheap readiness probe. Must not spend tokens.
    async fn availability(&self) -> Availability;
    /// The model this transport will call, when it chooses one itself (e.g. Codex's
    /// auto-selection). `None` = not applicable or unknown.
    async fn model(&self) -> Option<String> {
        None
    }
    async fn complete(&self, req: &Request) -> Result<Response, LlmError>;
}
