//! `codex exec` transport (port of CodexTransport.swift). Flags match docs/llm-providers.md.
//! Never reads `~/.codex`; never passes `--dangerously-*` flags.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use minutae_core::{FailureKind, Transport};
use serde_json::Value;

use crate::LlmTransport;
use crate::subprocess::{
    COMMON_PATTERNS, ErrorClassifier, SubprocessOutput, SubprocessRunner, known_locations,
    resolve_binary, resolve_with_fallbacks, scratch_dir,
};
use crate::types::{Availability, LlmError, Request, Response, transport_id};
use tokio_util::sync::CancellationToken;

/// Matched only against stderr and the messages of JSON `error` / `turn.failed` events,
/// never against model output on stdout. Phrases are narrow on purpose.
const PATTERNS: &[(&str, FailureKind)] = &[
    ("not logged in", FailureKind::NotLoggedIn),
    ("please sign in", FailureKind::NotLoggedIn),
    ("sign in again", FailureKind::NotLoggedIn),
    ("login required", FailureKind::NotLoggedIn),
    ("run codex login", FailureKind::NotLoggedIn),
    ("rate limit", FailureKind::RateLimited),
    ("too many requests", FailureKind::RateLimited),
    ("usage limit", FailureKind::RateLimited),
    ("model is not supported", FailureKind::ModelRejected),
    (
        "is not supported when using codex",
        FailureKind::ModelRejected,
    ),
    ("model not found", FailureKind::ModelRejected),
    ("model_not_found", FailureKind::ModelRejected),
    (
        "does not exist or you do not have access",
        FailureKind::ModelRejected,
    ),
];

pub struct CodexTransport {
    model: String,
    binary: Option<PathBuf>,
    runner: SubprocessRunner,
    classifier: ErrorClassifier,
}

/// Models offered by `codex debug models`, cached per binary for the whole process (the
/// router is rebuilt on every settings change; the catalog doesn't change that often).
struct Discovery {
    binary: PathBuf,
    /// Listed models in Codex's own priority order.
    candidates: Vec<String>,
    /// The first candidate that actually answered (ChatGPT accounts reject some models).
    chosen: Option<String>,
    at: Instant,
}

static DISCOVERY: tokio::sync::Mutex<Vec<Discovery>> = tokio::sync::Mutex::const_new(Vec::new());
/// A failed discovery is retried after this long (Settings polls availability).
const DISCOVERY_RETRY: Duration = Duration::from_secs(60);
/// Model rejections tolerated before giving up on auto-selection in one call.
const MAX_AUTO_TRIES: usize = 4;

/// Parses `codex debug models` JSON into selectable slugs: `visibility: "list"` (or no
/// visibility field), ordered by `priority`. Falls back to scanning `"slug"` keys when the
/// JSON doesn't parse (e.g. output was truncated).
pub fn parse_model_catalog(json: &str) -> Vec<String> {
    if let Ok(v) = serde_json::from_str::<Value>(json) {
        let models = v
            .get("models")
            .and_then(Value::as_array)
            .or_else(|| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut listed: Vec<(i64, usize, String)> = models
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let slug = m.get("slug").and_then(Value::as_str)?.trim();
                let visible = m
                    .get("visibility")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v == "list");
                (visible && !slug.is_empty()).then(|| {
                    let prio = m
                        .get("priority")
                        .and_then(Value::as_i64)
                        .unwrap_or(i64::MAX);
                    (prio, i, slug.to_string())
                })
            })
            .collect();
        listed.sort();
        let mut out: Vec<String> = Vec::new();
        for (_, _, s) in listed {
            if !out.contains(&s) {
                out.push(s);
            }
        }
        return out;
    }
    let mut out: Vec<String> = Vec::new();
    let mut rest = json;
    while let Some(i) = rest.find("\"slug\"") {
        rest = &rest[i + 6..];
        let Some(q1) = rest.find('"') else { break };
        let after = &rest[q1 + 1..];
        let Some(q2) = after.find('"') else { break };
        let slug = &after[..q2];
        if !slug.is_empty() && !out.iter().any(|s| s == slug) {
            out.push(slug.to_string());
        }
        rest = &after[q2..];
    }
    out
}

impl CodexTransport {
    /// `model` comes from the `codexModel` setting. Empty means "not chosen yet".
    pub fn new(model: Option<&str>) -> Self {
        Self::with_binary(model, None)
    }

    pub fn with_binary(model: Option<&str>, binary: Option<PathBuf>) -> Self {
        let binary = match binary {
            Some(b) => b.to_str().and_then(|s| resolve_binary(s, None)),
            None => resolve_with_fallbacks("codex", &known_locations("codex", &[".codex/bin"])),
        };
        Self {
            model: model.unwrap_or("").trim().to_string(),
            binary,
            runner: SubprocessRunner::default(),
            classifier: ErrorClassifier::new(PATTERNS).registering(COMMON_PATTERNS),
        }
    }

    pub fn binary_path(&self) -> Option<&PathBuf> {
        self.binary.as_ref()
    }

    /// The model calls will use: the configured one, else the auto-selected one
    /// (discovering the catalog if needed). `None` when nothing could be found.
    pub async fn resolved_model(&self) -> Option<String> {
        self.candidates()
            .await
            .ok()
            .and_then(|c| c.into_iter().next())
    }

    /// Models to try, best first. A configured model is the only candidate.
    async fn candidates(&self) -> Result<Vec<String>, LlmError> {
        if !self.model.is_empty() {
            return Ok(vec![self.model.clone()]);
        }
        let Some(binary) = &self.binary else {
            return Err(LlmError::not_installed("codex was not found"));
        };
        let mut cache = DISCOVERY.lock().await;
        let fresh = cache.iter().any(|d| {
            &d.binary == binary && (!d.candidates.is_empty() || d.at.elapsed() < DISCOVERY_RETRY)
        });
        if !fresh {
            let candidates = self.discover(binary).await;
            if candidates.is_empty() {
                tracing::warn!("codex: no models found in `codex debug models`");
            } else {
                tracing::info!(models = ?candidates, "codex: discovered models");
            }
            cache.retain(|d| &d.binary != binary);
            cache.push(Discovery {
                binary: binary.clone(),
                candidates,
                chosen: None,
                at: Instant::now(),
            });
        }
        let Some(d) = cache.iter().find(|d| &d.binary == binary) else {
            return Err(LlmError::model_rejected(
                "Choose a Codex model in settings.",
            ));
        };
        let mut out = d.candidates.clone();
        if let Some(c) = &d.chosen {
            out.retain(|m| m != c);
            out.insert(0, c.clone());
        }
        if out.is_empty() {
            return Err(LlmError::model_rejected(
                "Codex didn't list any models. Choose a Codex model in settings.",
            ));
        }
        Ok(out)
    }

    async fn discover(&self, binary: &std::path::Path) -> Vec<String> {
        let Ok(dir) = scratch_dir("minutae-codex-models-") else {
            return vec![];
        };
        // The catalog is ~1 MB of JSON (it embeds prompt templates).
        let runner = SubprocessRunner::new(32 * 1_048_576);
        let args: Vec<OsString> = ["debug", "models"].iter().map(OsString::from).collect();
        match runner
            .run(
                binary,
                &args,
                "",
                dir.path(),
                Duration::from_secs(30),
                &CancellationToken::new(),
            )
            .await
        {
            Ok(out) if out.success() => parse_model_catalog(&out.stdout),
            Ok(out) => {
                tracing::warn!(code = ?out.exit_code, stderr = %crate::types::clip(&out.stderr, 200), "codex debug models failed");
                vec![]
            }
            Err(e) => {
                tracing::warn!(error = %e.detail, "codex debug models failed");
                vec![]
            }
        }
    }

    async fn remember_choice(binary: &std::path::Path, model: &str) {
        if let Some(d) = DISCOVERY
            .lock()
            .await
            .iter_mut()
            .find(|d| d.binary == binary)
            && d.chosen.as_deref() != Some(model)
        {
            tracing::info!(model, "codex: using auto-selected model");
            d.chosen = Some(model.to_string());
        }
    }

    async fn forget_candidate(binary: &std::path::Path, model: &str) {
        if let Some(d) = DISCOVERY
            .lock()
            .await
            .iter_mut()
            .find(|d| d.binary == binary)
        {
            d.candidates.retain(|m| m != model);
            if d.chosen.as_deref() == Some(model) {
                d.chosen = None;
            }
        }
    }

    /// The exact verified argument list. `-` reads the prompt from stdin.
    pub fn arguments(model: &str) -> Vec<OsString> {
        [
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--json",
            "--color",
            "never",
            "-C",
            ".",
            "-o",
            "last-message.txt",
            "--model",
            model,
            "-",
        ]
        .iter()
        .map(OsString::from)
        .collect()
    }

    /// Parses `--json` JSONL: last `agent_message` text, `usage` counts, `error` events.
    pub(crate) fn parse(stdout: &str) -> ParsedCodex {
        let mut out = ParsedCodex::default();
        for line in stdout.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(item) = v.get("item")
                && item.get("type").and_then(Value::as_str) == Some("agent_message")
                && let Some(t) = item.get("text").and_then(Value::as_str)
            {
                out.message = Some(t.to_string());
            }
            if let Some(u) = v.get("usage") {
                out.tokens_in = u
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32);
                out.tokens_out = u
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .map(|n| n as u32);
            }
            match v.get("type").and_then(Value::as_str) {
                Some("error") => {
                    out.error = v.get("message").and_then(Value::as_str).map(String::from)
                }
                Some("turn.failed") => {
                    if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
                        out.error = Some(m.to_string());
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct ParsedCodex {
    pub message: Option<String>,
    pub tokens_in: Option<u32>,
    pub tokens_out: Option<u32>,
    pub error: Option<String>,
}

#[async_trait]
impl LlmTransport for CodexTransport {
    fn id(&self) -> Transport {
        Transport::CodexCli
    }
    fn name(&self) -> &str {
        "Codex"
    }

    async fn availability(&self) -> Availability {
        if self.binary.is_none() {
            return Availability::NotInstalled;
        }
        if self.candidates().await.is_err() {
            return Availability::Unavailable(
                "Couldn't find a Codex model. Choose one in settings.".into(),
            );
        }
        // Sign-in state is not checkable without reading Codex's credentials; unknown = ready.
        Availability::Ready
    }

    async fn model(&self) -> Option<String> {
        self.resolved_model().await
    }

    async fn complete(&self, req: &Request) -> Result<Response, LlmError> {
        let Some(binary) = &self.binary else {
            return Err(LlmError::not_installed("codex was not found"));
        };
        let auto = self.model.is_empty();
        let candidates = self.candidates().await?;
        let mut last = None;
        for model in candidates
            .iter()
            .take(if auto { MAX_AUTO_TRIES } else { 1 })
        {
            match self.complete_with(binary, model, req).await {
                Ok(r) => {
                    if auto {
                        Self::remember_choice(binary, model).await;
                    }
                    return Ok(r);
                }
                // ChatGPT sign-ins only allow some catalog models: try the next one.
                Err(e) if auto && e.kind == FailureKind::ModelRejected => {
                    tracing::info!(model, detail = %e.detail, "codex: model rejected, trying the next one");
                    Self::forget_candidate(binary, model).await;
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last
            .unwrap_or_else(|| LlmError::model_rejected("No Codex model accepted the request.")))
    }
}

impl CodexTransport {
    async fn complete_with(
        &self,
        binary: &std::path::Path,
        model: &str,
        req: &Request,
    ) -> Result<Response, LlmError> {
        let dir = scratch_dir("minutae-codex-")?;
        let result = self
            .runner
            .run(
                binary,
                &Self::arguments(model),
                &req.combined_prompt(),
                dir.path(),
                req.timeout,
                &req.cancel,
            )
            .await?;
        let parsed = Self::parse(&result.stdout);
        // Classify from stderr and Codex's own error events only: stdout also carries the
        // model's text, which may legitimately say "sign in" or "rate limit".
        let diagnostics = SubprocessOutput {
            exit_code: result.exit_code,
            stdout: parsed.error.clone().unwrap_or_default(),
            stderr: result.stderr.clone(),
        };
        if let Some(mut e) = self.classifier.classify(&diagnostics) {
            if let Some(msg) = &parsed.error {
                e.detail = crate::types::clip(msg, 240);
                if e.kind == FailureKind::Unavailable
                    && let Some(k) = self.classifier.match_text(msg)
                {
                    e.kind = k;
                }
            }
            return Err(e);
        }
        let message = parsed.message.filter(|m| !m.trim().is_empty()).or_else(|| {
            std::fs::read_to_string(dir.path().join("last-message.txt"))
                .ok()
                .filter(|m| !m.trim().is_empty())
        });
        let Some(text) = message else {
            if let Some(err) = parsed.error {
                let kind = self
                    .classifier
                    .match_text(&err)
                    .unwrap_or(FailureKind::InvalidOutput);
                return Err(LlmError::new(kind, crate::types::clip(&err, 240)));
            }
            return Err(LlmError::invalid_output("Codex returned no agent message."));
        };
        Ok(Response {
            text,
            model: model.to_string(),
            tokens_in: parsed.tokens_in,
            tokens_out: parsed.tokens_out,
            provider: transport_id(Transport::CodexCli).into(),
            cost_usd: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    const REJECTED: &str = r#"{"type":"thread.started","thread_id":"t"}
{"type":"turn.started"}
{"type":"error","message":"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."}
{"type":"turn.failed","error":{"message":"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."}}
"#;

    const OK: &str = r#"{"type":"thread.started","thread_id":"t"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"i0","type":"reasoning","text":"thinking"}}
{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"pong"}}
{"type":"turn.completed","usage":{"input_tokens":1200,"cached_input_tokens":0,"output_tokens":7}}
"#;

    #[test]
    fn args_are_the_verified_safe_set() {
        let args: Vec<String> = CodexTransport::arguments("m")
            .iter()
            .map(|a| a.to_string_lossy().into())
            .collect();
        assert_eq!(args.first().map(String::as_str), Some("exec"));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        for flag in [
            "--ephemeral",
            "--ignore-user-config",
            "--ignore-rules",
            "--json",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(
            args.iter()
                .all(|a| !a.contains("dangerous") && !a.contains("bypass"))
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--sandbox" && w[1] == "read-only")
        );
    }

    #[test]
    fn parses_success_jsonl() {
        let p = CodexTransport::parse(OK);
        assert_eq!(p.message.as_deref(), Some("pong"));
        assert_eq!((p.tokens_in, p.tokens_out), (Some(1200), Some(7)));
    }

    #[cfg(unix)]
    mod process {
        use super::*;
        use crate::subprocess::tests::script;

        fn fake(body: &str) -> (tempfile::TempDir, CodexTransport) {
            let dir = tempfile::tempdir().unwrap();
            let bin = script(dir.path(), "codex", body);
            let t = CodexTransport::with_binary(Some("gpt-test"), Some(bin));
            (dir, t)
        }

        #[tokio::test]
        async fn model_rejection_from_real_output() {
            let (_d, t) = fake(&format!(
                "cat > /dev/null; cat <<'EOF'\n{REJECTED}EOF\nexit 1"
            ));
            let e = t.complete(&Request::new("hi")).await.unwrap_err();
            assert_eq!(e.kind, FailureKind::ModelRejected, "detail: {}", e.detail);
            assert!(e.detail.contains("not supported"));
        }

        #[tokio::test]
        async fn success_reads_stdin_prompt() {
            // Echo back the prompt as the agent message to prove stdin delivery.
            let (_d, t) = fake(
                r#"p=$(cat | tr '\n' '|'); printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$p"; echo '{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":1}}'"#,
            );
            let r = t
                .complete(&Request::new("hello").system("sys"))
                .await
                .unwrap();
            assert_eq!(r.text, "sys||hello");
            assert_eq!(r.model, "gpt-test");
            assert_eq!(r.tokens_in, Some(3));
        }

        #[tokio::test]
        async fn logged_out_and_rate_limited() {
            let (_d, t) =
                fake("cat >/dev/null; echo 'Error: Not logged in. Run codex login' >&2; exit 1");
            assert_eq!(
                t.complete(&Request::new("x")).await.unwrap_err().kind,
                FailureKind::NotLoggedIn
            );
            let (_d, t) = fake(
                "cat >/dev/null; echo '{\"type\":\"error\",\"message\":\"You have hit your usage limit. Try again in 3 hours.\"}'; exit 1",
            );
            assert_eq!(
                t.complete(&Request::new("x")).await.unwrap_err().kind,
                FailureKind::RateLimited
            );
        }

        #[tokio::test]
        async fn model_text_on_stdout_is_not_classified() {
            // Failure for an unrelated reason; the agent message mentions signing in.
            let (_d, t) = fake(
                r#"cat >/dev/null; echo '{"type":"item.completed","item":{"type":"agent_message","text":"Action: please sign in to the portal; rate limit is 5/s"}}'; echo 'error: unexpected argument' >&2; exit 2"#,
            );
            let e = t.complete(&Request::new("x")).await.unwrap_err();
            assert_eq!(e.kind, FailureKind::Unavailable);
            assert_eq!(e.detail, "error: unexpected argument");

            // Success: the same text is just the answer.
            let (_d, t) = fake(
                r#"cat >/dev/null; echo '{"type":"item.completed","item":{"type":"agent_message","text":"Bob will sign in again tomorrow"}}'"#,
            );
            let r = t.complete(&Request::new("x")).await.unwrap();
            assert_eq!(r.text, "Bob will sign in again tomorrow");

            // A real turn.failed event is still classified.
            let (_d, t) = fake(
                r#"cat >/dev/null; echo '{"type":"turn.failed","error":{"message":"stream error: 429 Too Many Requests"}}'; exit 1"#,
            );
            assert_eq!(
                t.complete(&Request::new("x")).await.unwrap_err().kind,
                FailureKind::RateLimited
            );
        }

        #[tokio::test]
        async fn runs_in_empty_private_temp_dir() {
            let (_d, t) = fake(
                r#"cat >/dev/null; n=$(ls -A | wc -l | tr -d ' '); m=$(stat -c %a . 2>/dev/null || stat -f %Lp .); printf '{"item":{"type":"agent_message","text":"%s %s"}}\n' "$n" "$m""#,
            );
            let r = t.complete(&Request::new("x")).await.unwrap();
            assert_eq!(r.text, "0 700");
        }

        /// Fake `codex`: `debug models` prints a catalog; `exec` rejects `gpt-a` the way a
        /// ChatGPT account does and echoes the model name for anything else.
        const AUTO: &str = r#"if [ "$1" = "debug" ]; then cat <<'JSON'
{"models":[{"slug":"hidden","visibility":"hide","priority":0},{"slug":"gpt-b","visibility":"list","priority":2},{"slug":"gpt-a","visibility":"list","priority":1}]}
JSON
exit 0; fi
cat >/dev/null
m=""; next=0
for a in "$@"; do if [ $next = 1 ]; then m=$a; next=0; fi; [ "$a" = "--model" ] && next=1; done
if [ "$m" = "gpt-a" ]; then echo "{\"type\":\"error\",\"message\":\"The 'gpt-a' model is not supported when using Codex with a ChatGPT account.\"}"; exit 1; fi
printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$m""#;

        #[tokio::test]
        async fn auto_selects_first_model_that_answers() {
            let dir = tempfile::tempdir().unwrap();
            let bin = script(dir.path(), "codex", AUTO);
            let t = CodexTransport::with_binary(None, Some(bin.clone()));
            assert_eq!(t.availability().await, Availability::Ready);
            assert_eq!(t.resolved_model().await.as_deref(), Some("gpt-a"));
            let r = t.complete(&Request::new("x")).await.unwrap();
            assert_eq!((r.text.as_str(), r.model.as_str()), ("gpt-b", "gpt-b"));
            // Cached for new transports too (the router is rebuilt on settings changes).
            let t2 = CodexTransport::with_binary(Some(" "), Some(bin));
            assert_eq!(t2.model().await.as_deref(), Some("gpt-b"));
        }

        #[tokio::test]
        async fn configured_model_skips_discovery() {
            let dir = tempfile::tempdir().unwrap();
            let bin = script(dir.path(), "codex", AUTO);
            let t = CodexTransport::with_binary(Some("gpt-a"), Some(bin));
            assert_eq!(t.model().await.as_deref(), Some("gpt-a"));
            let e = t.complete(&Request::new("x")).await.unwrap_err();
            assert_eq!(e.kind, FailureKind::ModelRejected);
        }

        #[tokio::test]
        async fn no_catalog_is_unavailable_without_running_exec() {
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("ran");
            let bin = script(
                dir.path(),
                "codex",
                &format!(
                    "if [ \"$1\" = debug ]; then echo boom >&2; exit 2; fi; touch '{}'",
                    marker.display()
                ),
            );
            let t = CodexTransport::with_binary(Some("  "), Some(bin));
            assert!(matches!(
                t.availability().await,
                Availability::Unavailable(_)
            ));
            assert_eq!(
                t.complete(&Request::new("x")).await.unwrap_err().kind,
                FailureKind::ModelRejected
            );
            assert!(!marker.exists(), "exec must not run without a model");
        }
    }

    /// `cargo test -p minutae-llm real_codex_discovery -- --ignored` (needs a real `codex`).
    #[tokio::test]
    #[ignore]
    async fn real_codex_discovery() {
        let t = CodexTransport::new(None);
        if t.binary_path().is_none() {
            return;
        }
        let m = t.resolved_model().await;
        eprintln!("auto-selected codex model: {m:?}");
        assert!(m.is_some());
    }

    #[test]
    fn parses_real_catalog_fixture() {
        let json = include_str!("../tests/fixtures/codex-debug-models.json");
        let models = parse_model_catalog(json);
        assert_eq!(models.first().map(String::as_str), Some("gpt-6.2-sol"));
        assert!(
            !models
                .iter()
                .any(|m| m == "gpt-reserve" || m == "codex-auto-review")
        );
        assert!(models.contains(&"gpt-6.1-sol".to_string()));
        // Truncated output still yields slugs in order.
        let cut = &json[..json.len() / 2];
        let partial = parse_model_catalog(cut);
        assert_eq!(partial.first().map(String::as_str), Some("gpt-6.2-sol"));
        assert!(parse_model_catalog("not json").is_empty());
    }

    #[tokio::test]
    async fn missing_binary() {
        let t = CodexTransport::with_binary(Some("m"), Some(PathBuf::from("/nonexistent/codex")));
        assert_eq!(t.availability().await, Availability::NotInstalled);
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::NotInstalled
        );
    }
}
