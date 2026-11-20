//! Headless `grok` transport (port of GrokTransport.swift). Flags match docs/llm-providers.md.
//! The prompt goes in a private `0600` file (`--prompt-file`) and on stdin, never in argv.
//! Never opens `~/.grok`; never passes `--always-approve` or bypass permission modes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use minutae_core::{FailureKind, Transport};
use serde_json::Value;

use crate::LlmTransport;
use crate::subprocess::{
    COMMON_PATTERNS, ErrorClassifier, SubprocessRunner, known_locations, resolve_binary,
    resolve_with_fallbacks, scratch_dir,
};
use crate::types::{Availability, LlmError, Request, Response, transport_id};

/// Phrases from grok 1.0.77.
const PATTERNS: &[(&str, FailureKind)] = &[
    ("not signed in", FailureKind::NotLoggedIn),
    ("grok login", FailureKind::NotLoggedIn),
    ("authentication required", FailureKind::NotLoggedIn),
    ("xai credential", FailureKind::NotLoggedIn),
    ("rate limit", FailureKind::RateLimited),
    ("too many requests", FailureKind::RateLimited),
];

pub struct GrokTransport {
    model: Option<String>,
    binary: Option<PathBuf>,
    runner: SubprocessRunner,
    classifier: ErrorClassifier,
}

impl GrokTransport {
    pub fn new(model: Option<&str>) -> Self {
        Self::with_binary(model, None)
    }

    pub fn with_binary(model: Option<&str>, binary: Option<PathBuf>) -> Self {
        let binary = match binary {
            Some(b) => b.to_str().and_then(|s| resolve_binary(s, None)),
            None => resolve_with_fallbacks("grok", &known_locations("grok", &[".grok/bin"])),
        };
        Self {
            model: model
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(String::from),
            binary,
            runner: SubprocessRunner::default(),
            classifier: ErrorClassifier::new(PATTERNS).registering(COMMON_PATTERNS),
        }
    }

    pub fn arguments(
        prompt_file: &Path,
        cwd: &Path,
        model: Option<&str>,
        schema: Option<&str>,
    ) -> Vec<OsString> {
        let mut a: Vec<OsString> = vec![
            "--prompt-file".into(),
            prompt_file.into(),
            "--output-format".into(),
            "json".into(),
            "--tools".into(),
            "".into(),
            "--disable-web-search".into(),
            "--no-subagents".into(),
            "--max-turns".into(),
            "1".into(),
            "--cwd".into(),
            cwd.into(),
            "--verbatim".into(),
        ];
        if let Some(m) = model {
            a.push("--model".into());
            a.push(m.into());
        }
        if let Some(s) = schema {
            a.push("--json-schema".into());
            a.push(s.into());
        }
        a
    }

    pub(crate) fn parse(stdout: &str, fallback_model: Option<&str>) -> Result<Response, LlmError> {
        let v: Value = serde_json::from_str(stdout.trim())
            .map_err(|_| LlmError::invalid_output("Grok returned malformed JSON."))?;
        match v
            .get("stopReason")
            .and_then(Value::as_str)
            .map(str::to_lowercase)
            .as_deref()
        {
            Some("cancelled") => return Err(LlmError::cancelled()),
            Some("rate_limited") | Some("rate-limited") => {
                return Err(LlmError::rate_limited("Grok is rate limited"));
            }
            _ => {}
        }
        let text = v
            .get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| LlmError::invalid_output("Grok returned no text."))?;
        let model = v
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(String::from)
            .or_else(|| {
                let mu = v.get("modelUsage")?.as_object()?;
                (mu.len() == 1).then(|| mu.keys().next().cloned()).flatten()
            })
            .or_else(|| fallback_model.map(String::from))
            .unwrap_or_else(|| "grok".into());
        let count = |k: &str| {
            v.get("usage")
                .and_then(|u| u.get(k))
                .and_then(Value::as_f64)
                .map(|n| n as u32)
        };
        Ok(Response {
            text: text.to_string(),
            model,
            tokens_in: count("input_tokens"),
            tokens_out: count("output_tokens"),
            provider: transport_id(Transport::GrokCli).into(),
            // Grok reports a dollar figure, but only the API transport records cost.
            cost_usd: None,
        })
    }
}

#[async_trait]
impl LlmTransport for GrokTransport {
    fn id(&self) -> Transport {
        Transport::GrokCli
    }
    fn name(&self) -> &str {
        "Grok"
    }

    async fn availability(&self) -> Availability {
        if self.binary.is_none() {
            Availability::NotInstalled
        } else {
            Availability::Ready
        }
    }

    async fn complete(&self, req: &Request) -> Result<Response, LlmError> {
        let Some(binary) = &self.binary else {
            return Err(LlmError::not_installed("grok was not found"));
        };
        let schema = req.json_schema.as_ref().map(|s| s.to_string());
        let prompt = req.combined_prompt();
        let root = scratch_dir("minutae-grok-")?;
        let cwd = root.path().join("cwd");
        let prompt_file = root.path().join("prompt.txt");
        write_private(&cwd, &prompt_file, &prompt)?;
        let args = Self::arguments(&prompt_file, &cwd, self.model.as_deref(), schema.as_deref());
        let out = self
            .runner
            .run(binary, &args, &prompt, &cwd, req.timeout, &req.cancel)
            .await?;
        if let Some(e) = self.classifier.classify(&out) {
            return Err(e);
        }
        Self::parse(&out.stdout, self.model.as_deref())
    }
}

impl GrokTransport {
    /// `grok --version`, for the Settings row.
    pub async fn version(&self) -> Option<String> {
        let bin = self.binary.as_ref()?;
        let dir = scratch_dir("minutae-grok-").ok()?;
        let out = self
            .runner
            .run(
                bin,
                &["--version".into()],
                "",
                dir.path(),
                Duration::from_secs(5),
                &Default::default(),
            )
            .await
            .ok()?;
        out.success()
            .then(|| out.stdout.lines().next().unwrap_or("").trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

fn write_private(cwd: &Path, file: &Path, prompt: &str) -> Result<(), LlmError> {
    let io = |e: std::io::Error| {
        LlmError::new(
            FailureKind::Io,
            format!("could not prepare grok scratch: {e}"),
        )
    };
    std::fs::create_dir(cwd).map_err(io)?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(cwd, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(file).map_err(io)?;
    f.write_all(prompt.as_bytes()).map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_success_and_model() {
        let r = GrokTransport::parse(
            r#"{"text":"pong","stopReason":"end_turn","num_turns":1,"model":"grok-4.7-build","usage":{"input_tokens":15000,"output_tokens":2},"total_cost_usd":0.01}"#,
            None,
        )
        .unwrap();
        assert_eq!(r.text, "pong");
        assert_eq!(r.model, "grok-4.7-build");
        assert_eq!(r.tokens_in, Some(15000));
        assert_eq!(r.cost_usd, None);
        let r = GrokTransport::parse(
            r#"{"text":"x","modelUsage":{"grok-a":{}}}"#,
            Some("fallback"),
        )
        .unwrap();
        assert_eq!(r.model, "grok-a");
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            GrokTransport::parse("not json", None).unwrap_err().kind,
            FailureKind::InvalidOutput
        );
        assert_eq!(
            GrokTransport::parse(r#"{"text":"  "}"#, None)
                .unwrap_err()
                .kind,
            FailureKind::InvalidOutput
        );
        assert_eq!(
            GrokTransport::parse(r#"{"text":"x","stopReason":"rate_limited"}"#, None)
                .unwrap_err()
                .kind,
            FailureKind::RateLimited
        );
        assert_eq!(
            GrokTransport::parse(r#"{"text":"x","stopReason":"cancelled"}"#, None)
                .unwrap_err()
                .kind,
            FailureKind::Cancelled
        );
    }

    #[test]
    fn args_never_carry_prompt_or_bypass_flags() {
        let a: Vec<String> = GrokTransport::arguments(
            Path::new("/t/p.txt"),
            Path::new("/t/cwd"),
            Some("m"),
            Some("{}"),
        )
        .iter()
        .map(|s| s.to_string_lossy().into())
        .collect();
        assert!(a.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()));
        assert!(
            a.windows(2)
                .any(|w| w[0] == "--json-schema" && w[1] == "{}")
        );
        assert!(
            !a.iter()
                .any(|s| s == "-p" || s.contains("always-approve") || s.contains("bypass"))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_prompt_file_and_classifies() {
        use crate::subprocess::tests::script;
        let dir = tempfile::tempdir().unwrap();
        // Echo the prompt file back as text; check its mode is 600.
        let bin = script(
            dir.path(),
            "grok",
            r#"cat >/dev/null; f="$2"; m=$(stat -f %Lp "$f" 2>/dev/null || stat -c %a "$f"); printf '{"text":"%s|%s","stopReason":"end_turn"}' "$(cat "$f")" "$m""#,
        );
        let t = GrokTransport::with_binary(None, Some(bin));
        let r = t.complete(&Request::new("hello")).await.unwrap();
        assert_eq!(r.text, "hello|600");

        let bin = script(
            dir.path(),
            "grok2",
            "echo 'Error: not signed in. Run grok login.' >&2; exit 1",
        );
        let t = GrokTransport::with_binary(None, Some(bin));
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::NotLoggedIn
        );
    }
}
