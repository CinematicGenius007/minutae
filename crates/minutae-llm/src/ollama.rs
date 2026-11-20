//! Ollama over its local HTTP API (port of OllamaTransport.swift).
//! Loopback only. Cloud models (`:cloud`, `remote_host`) are refused: a completion would leave
//! the machine. Nothing is downloaded unless `pull_model` is called.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use minutae_core::{FailureKind, Transport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::LlmTransport;
use crate::subprocess::{known_locations, resolve_with_fallbacks};
use crate::types::{Availability, LlmError, Request, Response, clip, transport_id};

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OllamaReadiness {
    NotInstalled,
    ServerDown,
    NoModel,
    CloudOnly,
    Ready,
    /// The daemon answered, but `/api/tags` was not a model list.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    pub name: String,
    pub is_local: bool,
    pub remote_host: Option<String>,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub readiness: OllamaReadiness,
    pub daemon_reachable: bool,
    pub installed: bool,
    pub binary_path: Option<PathBuf>,
    pub version: Option<String>,
    pub models: Vec<OllamaModelInfo>,
    pub selected_model: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProgress {
    pub status: String,
    pub digest: Option<String>,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}

pub struct OllamaTransport {
    requested_model: Option<String>,
    base_url: String,
    binary: Option<PathBuf>,
    client: reqwest::Client,
}

impl OllamaTransport {
    pub fn new(model: Option<&str>) -> Self {
        let binary = resolve_with_fallbacks("ollama", &ollama_locations());
        Self::with_base_url(model, DEFAULT_BASE_URL, binary)
    }

    pub fn with_base_url(model: Option<&str>, base_url: &str, binary: Option<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        Self {
            requested_model: model
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(String::from),
            base_url: base_url.trim_end_matches('/').to_string(),
            binary,
            client,
        }
    }

    pub async fn status(&self) -> OllamaStatus {
        self.load_status(Duration::from_secs(3)).await
    }

    async fn load_status(&self, probe: Duration) -> OllamaStatus {
        let installed = self.binary.is_some();
        let base = |readiness, reachable, detail: String| OllamaStatus {
            readiness,
            daemon_reachable: reachable,
            installed,
            binary_path: self.binary.clone(),
            version: None,
            models: vec![],
            selected_model: self.requested_model.clone(),
            detail,
        };
        if !is_loopback(&self.base_url) {
            return base(
                if installed {
                    OllamaReadiness::ServerDown
                } else {
                    OllamaReadiness::NotInstalled
                },
                false,
                "Ollama only talks to a loopback address.".into(),
            );
        }
        let tags = match self.get("/api/tags", probe).await {
            Ok(t) => t,
            Err(_) if !installed => {
                return base(
                    OllamaReadiness::NotInstalled,
                    false,
                    "Ollama is not installed.".into(),
                );
            }
            Err(_) => return base(OllamaReadiness::ServerDown, false, self.unreachable()),
        };
        let version = self
            .get("/api/version", probe)
            .await
            .ok()
            .and_then(|v| v.get("version").and_then(Value::as_str).map(String::from));
        let Some(models) = parse_models(&tags) else {
            let mut s = base(
                OllamaReadiness::Unknown,
                true,
                "Ollama is running, but the model list could not be read.".into(),
            );
            s.version = version;
            return s;
        };
        let names = |local: bool| {
            models
                .iter()
                .filter(|m| m.is_local == local)
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let has_local = models.iter().any(|m| m.is_local);
        let has_cloud = models.iter().any(|m| !m.is_local);
        let (readiness, mut detail) = if models.is_empty() {
            (OllamaReadiness::NoModel, "Ollama is running and no model is installed. Pull a local model to use it offline.".to_string())
        } else if !has_local {
            (
                OllamaReadiness::CloudOnly,
                format!(
                    "Ollama is running, but every installed model is a cloud model ({}). A completion would leave this machine.",
                    names(false)
                ),
            )
        } else if !has_cloud {
            (
                OllamaReadiness::Ready,
                format!("Ollama is running. Local models: {}.", names(true)),
            )
        } else {
            (
                OllamaReadiness::Ready,
                format!(
                    "Ollama is running. Local models: {}. Cloud models: {}.",
                    names(true),
                    names(false)
                ),
            )
        };
        if let Some(req) = &self.requested_model {
            match models.iter().find(|m| &m.name == req) {
                Some(m) if !m.is_local => {
                    detail.push_str(&format!(" The selected model {req} is a cloud model."))
                }
                Some(_) => {}
                None if is_cloud_name(req) => {
                    detail.push_str(&format!(" The selected model {req} is a cloud model."))
                }
                None => detail.push_str(&format!(" The selected model {req} is not installed.")),
            }
        }
        if !installed {
            detail.push_str(" The ollama binary was not found.");
        }
        let selected = self
            .requested_model
            .clone()
            .or_else(|| models.iter().find(|m| m.is_local).map(|m| m.name.clone()));
        OllamaStatus {
            readiness,
            daemon_reachable: true,
            installed,
            binary_path: self.binary.clone(),
            version,
            models,
            selected_model: selected,
            detail,
        }
    }

    fn model_to_run(&self, s: &OllamaStatus) -> Result<String, LlmError> {
        if !s.daemon_reachable {
            return Err(if s.installed {
                LlmError::unavailable(s.detail.clone())
            } else {
                LlmError::not_installed(s.detail.clone())
            });
        }
        if s.readiness == OllamaReadiness::Unknown {
            return Err(LlmError::invalid_output(s.detail.clone()));
        }
        if let Some(req) = &self.requested_model {
            if is_cloud_name(req) || s.models.iter().any(|m| &m.name == req && !m.is_local) {
                return Err(LlmError::model_rejected(format!(
                    "{req} is a cloud model. A completion would leave this machine."
                )));
            }
            if !s.models.iter().any(|m| &m.name == req && m.is_local) {
                return Err(LlmError::new(
                    FailureKind::ModelMissing,
                    format!("{req} is not installed. Pull it before using Ollama."),
                ));
            }
            return Ok(req.clone());
        }
        if let Some(m) = s.models.iter().find(|m| m.is_local) {
            return Ok(m.name.clone());
        }
        if s.models.is_empty() {
            return Err(LlmError::new(
                FailureKind::ModelMissing,
                "Ollama is running and no model is installed. Pull a local model to use it offline.",
            ));
        }
        Err(LlmError::model_rejected(
            "Every installed Ollama model is a cloud model. Pull a local model to use Ollama offline.",
        ))
    }

    fn unreachable(&self) -> String {
        format!(
            "Ollama is installed, but the server at {} is not reachable.",
            self.base_url.trim_start_matches("http://")
        )
    }

    async fn get(&self, path: &str, timeout: Duration) -> Result<Value, LlmError> {
        let resp = self
            .client
            .get(format!("{}{path}", self.base_url))
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| self.map_reqwest(e))?;
        read_json(resp).await
    }

    fn map_reqwest(&self, e: reqwest::Error) -> LlmError {
        if e.is_timeout() {
            LlmError::timeout()
        } else if e.is_connect() {
            if self.binary.is_none() {
                LlmError::not_installed("Ollama is not installed.")
            } else {
                LlmError::unavailable(self.unreachable())
            }
        } else {
            LlmError::unavailable("Ollama request failed.")
        }
    }

    pub(crate) fn chat_body(req: &Request, model: &str) -> Value {
        let mut messages = vec![];
        if let Some(s) = req
            .system
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            messages.push(json!({"role": "system", "content": s}));
        }
        messages.push(json!({"role": "user", "content": req.prompt}));
        let mut body = json!({"model": model, "messages": messages, "stream": false});
        // docs/llm-providers.md asks for `"format": "json"`; a schema object is Ollama's structured form.
        if let Some(schema) = &req.json_schema {
            body["format"] = if schema.is_object() {
                schema.clone()
            } else {
                json!("json")
            };
        }
        if let Some(n) = req.max_tokens {
            body["options"] = json!({"num_predict": n});
        }
        body
    }

    /// Downloads `name` through the daemon, reporting NDJSON progress.
    pub async fn pull_model(
        &self,
        name: &str,
        mut on_progress: impl FnMut(PullProgress) + Send,
    ) -> Result<(), LlmError> {
        let name = name.trim();
        if !is_acceptable_model_name(name) {
            return Err(LlmError::model_rejected(
                "Choose a model name before pulling.",
            ));
        }
        if !is_loopback(&self.base_url) {
            return Err(LlmError::unavailable(
                "Ollama only talks to a loopback address.",
            ));
        }
        let mut resp = self
            .client
            .post(format!("{}/api/pull", self.base_url))
            .json(&json!({"model": name, "stream": true}))
            .timeout(Duration::from_secs(3600))
            .send()
            .await
            .map_err(|e| self.map_reqwest(e))?;
        if !resp.status().is_success() {
            return read_json(resp).await.map(|_| ());
        }
        let mut buf = Vec::new();
        let mut succeeded = false;
        let mut handle = |line: &[u8], succeeded: &mut bool| -> Result<(), LlmError> {
            let line = String::from_utf8_lossy(line);
            let line = line.trim();
            if line.is_empty() {
                return Ok(());
            }
            let v: Value = serde_json::from_str(line)
                .map_err(|_| LlmError::invalid_output("Ollama pull progress was not JSON."))?;
            if let Some(e) = non_empty(&v["error"]) {
                return Err(classify_api_error(&e));
            }
            let Some(status) = non_empty(&v["status"]) else {
                return Ok(());
            };
            if status.eq_ignore_ascii_case("success") {
                *succeeded = true;
            }
            on_progress(PullProgress {
                status,
                digest: non_empty(&v["digest"]),
                completed: v["completed"].as_u64(),
                total: v["total"].as_u64(),
            });
            Ok(())
        };
        while let Some(chunk) = resp.chunk().await.map_err(|e| self.map_reqwest(e))? {
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                handle(&line, &mut succeeded)?;
            }
        }
        handle(&buf, &mut succeeded)?;
        if succeeded {
            Ok(())
        } else {
            Err(LlmError::unavailable("Ollama pull did not finish."))
        }
    }
}

#[async_trait]
impl LlmTransport for OllamaTransport {
    fn id(&self) -> Transport {
        Transport::Ollama
    }
    fn name(&self) -> &str {
        "Ollama"
    }

    async fn availability(&self) -> Availability {
        let s = self.status().await;
        match s.readiness {
            OllamaReadiness::NotInstalled => Availability::NotInstalled,
            OllamaReadiness::Ready => match self.model_to_run(&s) {
                Ok(_) => Availability::Ready,
                Err(e) => Availability::Unavailable(e.detail),
            },
            _ => Availability::Unavailable(s.detail),
        }
    }

    async fn complete(&self, req: &Request) -> Result<Response, LlmError> {
        let probe = req.timeout.min(Duration::from_secs(10));
        let status = tokio::select! {
            s = self.load_status(probe) => s,
            _ = req.cancel.cancelled() => return Err(LlmError::cancelled()),
        };
        let model = self.model_to_run(&status)?;
        let send = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&Self::chat_body(req, &model))
            .timeout(req.timeout)
            .send();
        let resp = tokio::select! {
            r = send => r.map_err(|e| self.map_reqwest(e))?,
            _ = req.cancel.cancelled() => return Err(LlmError::cancelled()),
        };
        let v = tokio::select! {
            v = read_json(resp) => v?,
            _ = req.cancel.cancelled() => return Err(LlmError::cancelled()),
        };
        if let Some(e) = non_empty(&v["error"]) {
            return Err(classify_api_error(&e));
        }
        let text = non_empty(&v["message"]["content"])
            .ok_or_else(|| LlmError::invalid_output("Ollama returned no message."))?;
        Ok(Response {
            text,
            model: non_empty(&v["model"]).unwrap_or(model),
            tokens_in: v["prompt_eval_count"].as_u64().map(|n| n as u32),
            tokens_out: v["eval_count"].as_u64().map(|n| n as u32),
            provider: transport_id(Transport::Ollama).into(),
            cost_usd: None,
        })
    }
}

async fn read_json(resp: reqwest::Response) -> Result<Value, LlmError> {
    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| {
        if e.is_timeout() {
            LlmError::timeout()
        } else {
            LlmError::unavailable("Ollama request failed.")
        }
    })?;
    let parsed: Option<Value> = serde_json::from_slice(&bytes).ok();
    if !status.is_success() {
        if let Some(e) = parsed.as_ref().and_then(|v| non_empty(&v["error"])) {
            return Err(classify_api_error(&e));
        }
        if status.as_u16() == 404 {
            return Err(LlmError::model_rejected(
                "Ollama could not find that model.",
            ));
        }
        return Err(LlmError::unavailable(format!(
            "Ollama returned HTTP {}.",
            status.as_u16()
        )));
    }
    parsed.filter(Value::is_object).ok_or_else(|| {
        LlmError::invalid_output("Ollama returned a response that is not a JSON object.")
    })
}

fn parse_models(v: &Value) -> Option<Vec<OllamaModelInfo>> {
    let rows = v.get("models")?.as_array()?;
    Some(
        rows.iter()
            .filter_map(|r| {
                let name = non_empty(&r["name"]).or_else(|| non_empty(&r["model"]))?;
                let remote_host = non_empty(&r["remote_host"]);
                let remote_model = non_empty(&r["remote_model"]);
                let cloud = remote_host.is_some() || remote_model.is_some() || is_cloud_name(&name);
                Some(OllamaModelInfo {
                    name,
                    is_local: !cloud,
                    remote_host,
                    digest: non_empty(&r["digest"]),
                })
            })
            .collect(),
    )
}

pub(crate) fn classify_api_error(message: &str) -> LlmError {
    let l = message.to_lowercase();
    if l.contains("rate limit") || l.contains("too many requests") {
        LlmError::rate_limited(clip(message, 240))
    } else if l.contains("sign in")
        || l.contains("not logged")
        || l.contains("unauthorized")
        || l.contains("authentication")
    {
        LlmError::not_logged_in(clip(message, 240))
    } else if l.contains("not found") || l.contains("not supported") || l.contains("unknown model")
    {
        LlmError::model_rejected(clip(message, 240))
    } else {
        LlmError::unavailable(clip(message, 240))
    }
}

fn is_cloud_name(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(":cloud") || l.ends_with("-cloud")
}

fn is_loopback(base: &str) -> bool {
    let rest = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(
        host.to_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1"
    )
}

fn is_acceptable_model_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 256
        && !n.contains("..")
        && !n.starts_with('/')
        && !n.starts_with('.')
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:+-/".contains(c))
}

fn non_empty(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn ollama_locations() -> Vec<PathBuf> {
    // `mut` is only used by the macOS/Windows additions below.
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(unused_mut))]
    let mut v = known_locations("ollama", &[]);
    #[cfg(target_os = "macos")]
    v.push(PathBuf::from(
        "/Applications/Ollama.app/Contents/Resources/ollama",
    ));
    #[cfg(windows)]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        v.insert(
            0,
            PathBuf::from(local)
                .join("Programs")
                .join("Ollama")
                .join("ollama.exe"),
        );
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::MockServer;

    const TAGS_LOCAL: &str = r#"{"models":[{"name":"llama3.1:8b","model":"llama3.1:8b","digest":"abc"},{"name":"gemma4:31b-cloud","remote_model":"gemma4:31b","remote_host":"https://ollama.com"}]}"#;
    const TAGS_CLOUD_ONLY: &str = r#"{"models":[{"name":"gemma4:31b-cloud","model":"gemma4:31b-cloud","remote_model":"gemma4:31b","remote_host":"https://ollama.com"}]}"#;

    #[test]
    fn loopback_check() {
        assert!(is_loopback("http://127.0.0.1:11434"));
        assert!(is_loopback("http://localhost:1"));
        assert!(is_loopback("http://[::1]:11434"));
        assert!(!is_loopback("http://example.com:11434"));
        assert!(!is_loopback("http://user@127.0.0.1:11434"));
        assert!(!is_loopback("ftp://127.0.0.1"));
    }

    #[test]
    fn chat_body_shape() {
        let req = Request::new("p")
            .system("s")
            .json_schema(json!({"type":"object"}))
            .max_tokens(50);
        let b = OllamaTransport::chat_body(&req, "m");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["format"]["type"], "object");
        assert_eq!(b["options"]["num_predict"], 50);
        assert_eq!(b["stream"], false);
    }

    #[tokio::test]
    async fn cloud_only_is_refused() {
        let server = MockServer::start(vec![
            ("/api/tags", 200, TAGS_CLOUD_ONLY.into()),
            ("/api/version", 200, r#"{"version":"0.40"}"#.into()),
        ])
        .await;
        let t = OllamaTransport::with_base_url(None, &server.url(), Some(PathBuf::from("/x")));
        let s = t.status().await;
        assert_eq!(s.readiness, OllamaReadiness::CloudOnly);
        assert!(matches!(
            t.availability().await,
            Availability::Unavailable(_)
        ));
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::ModelRejected
        );
    }

    #[tokio::test]
    async fn chat_success_picks_local_model() {
        let server = MockServer::start(vec![
            ("/api/tags", 200, TAGS_LOCAL.into()),
            ("/api/version", 200, r#"{"version":"0.40"}"#.into()),
            ("/api/chat", 200, r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"hi"},"prompt_eval_count":10,"eval_count":2}"#.into()),
        ])
        .await;
        let t = OllamaTransport::with_base_url(None, &server.url(), Some(PathBuf::from("/x")));
        assert_eq!(t.availability().await, Availability::Ready);
        let r = t.complete(&Request::new("x")).await.unwrap();
        assert_eq!(
            (r.text.as_str(), r.model.as_str(), r.tokens_in, r.tokens_out),
            ("hi", "llama3.1:8b", Some(10), Some(2))
        );
        let chat = server
            .requests()
            .into_iter()
            .find(|r| r.0 == "/api/chat")
            .unwrap();
        assert!(chat.1.contains("\"model\":\"llama3.1:8b\""));
    }

    #[tokio::test]
    async fn missing_selected_model_and_errors() {
        let server = MockServer::start(vec![("/api/tags", 200, TAGS_LOCAL.into())]).await;
        let t = OllamaTransport::with_base_url(
            Some("qwen3:4b"),
            &server.url(),
            Some(PathBuf::from("/x")),
        );
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::ModelMissing
        );
        let t = OllamaTransport::with_base_url(
            Some("gemma4:31b-cloud"),
            &server.url(),
            Some(PathBuf::from("/x")),
        );
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::ModelRejected
        );
    }

    #[tokio::test]
    async fn server_down() {
        // Port 9 on loopback: nothing listens.
        let t = OllamaTransport::with_base_url(None, "http://127.0.0.1:9", None);
        assert_eq!(t.availability().await, Availability::NotInstalled);
        let t =
            OllamaTransport::with_base_url(None, "http://127.0.0.1:9", Some(PathBuf::from("/x")));
        assert!(matches!(
            t.availability().await,
            Availability::Unavailable(_)
        ));
        assert_eq!(
            t.complete(&Request::new("x")).await.unwrap_err().kind,
            FailureKind::Unavailable
        );
    }

    #[tokio::test]
    async fn pull_progress() {
        let body = "{\"status\":\"pulling manifest\"}\n{\"status\":\"downloading\",\"digest\":\"d\",\"completed\":5,\"total\":10}\n{\"status\":\"success\"}\n";
        let server = MockServer::start(vec![("/api/pull", 200, body.into())]).await;
        let t = OllamaTransport::with_base_url(None, &server.url(), None);
        let mut seen = vec![];
        t.pull_model("llama3.2:3b", |p| seen.push(p)).await.unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[1].completed, Some(5));
        assert!(t.pull_model("../etc", |_| {}).await.is_err());
    }

    #[test]
    fn api_error_classes() {
        assert_eq!(
            classify_api_error("model 'x' not found, try pulling it first").kind,
            FailureKind::ModelRejected
        );
        assert_eq!(
            classify_api_error("unauthorized").kind,
            FailureKind::NotLoggedIn
        );
        assert_eq!(
            classify_api_error("Too Many Requests").kind,
            FailureKind::RateLimited
        );
    }
}
