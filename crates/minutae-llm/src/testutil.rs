//! Test helpers: a tiny loopback HTTP server and a scripted fake transport.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use minutae_core::Transport;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::LlmTransport;
use crate::types::{Availability, LlmError, Request, Response, transport_id};

/// (path, body, raw headers)
pub type Recorded = (String, String, String);

pub struct MockServer {
    addr: std::net::SocketAddr,
    seen: Arc<Mutex<Vec<Recorded>>>,
}

impl MockServer {
    /// Each route answers every request whose path (without query) starts with the route path.
    pub async fn start(routes: Vec<(&'static str, u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let routes = Arc::new(routes);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                let seen = seen2.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    let header_end = loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while buf.len() < header_end + len {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
                    let path = head
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap_or("/")
                        .to_string();
                    seen.lock().unwrap().push((path.clone(), body, head));
                    let (status, resp_body) = routes
                        .iter()
                        .find(|(p, _, _)| path.starts_with(p))
                        .map(|(_, s, b)| (*s, b.clone()))
                        .unwrap_or((404, "{\"error\":\"no route\"}".into()));
                    let out = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp_body}",
                        resp_body.len()
                    );
                    let _ = sock.write_all(out.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { addr, seen }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.seen.lock().unwrap().clone()
    }
}

/// A transport that returns scripted results in order and records prompts.
pub struct FakeTransport {
    pub id: Transport,
    pub availability: Availability,
    pub script: Mutex<VecDeque<Result<String, LlmError>>>,
    pub prompts: Mutex<Vec<String>>,
}

impl FakeTransport {
    pub fn new(id: Transport, script: Vec<Result<&str, LlmError>>) -> Arc<Self> {
        Arc::new(Self {
            id,
            availability: Availability::Ready,
            script: Mutex::new(script.into_iter().map(|r| r.map(String::from)).collect()),
            prompts: Mutex::new(vec![]),
        })
    }
    pub fn unavailable(id: Transport, a: Availability) -> Arc<Self> {
        Arc::new(Self {
            id,
            availability: a,
            script: Mutex::new(VecDeque::new()),
            prompts: Mutex::new(vec![]),
        })
    }
    pub fn calls(&self) -> usize {
        self.prompts.lock().unwrap().len()
    }
}

#[async_trait]
impl LlmTransport for FakeTransport {
    fn id(&self) -> Transport {
        self.id
    }
    fn name(&self) -> &str {
        "Fake"
    }
    async fn availability(&self) -> Availability {
        self.availability.clone()
    }
    async fn complete(&self, req: &Request) -> Result<Response, LlmError> {
        self.prompts.lock().unwrap().push(req.prompt.clone());
        let next = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(LlmError::unavailable("script exhausted")));
        next.map(|text| Response {
            text,
            model: "fake-model".into(),
            tokens_in: Some(10),
            tokens_out: Some(5),
            provider: transport_id(self.id).into(),
            cost_usd: None,
        })
    }
}
