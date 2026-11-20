//! Real smoke test: `cargo run -p minutae-llm --example summarize -- transcript.txt`
//!
//! Env: `MINUTAE_CODEX_MODEL` (codex model id), `MINUTAE_OLLAMA_MODEL`,
//! `MINUTAE_ORDER` (comma list of codex,grok,ollama). Uses the real keychain only if
//! `MINUTAE_API=1`.

use std::sync::Arc;

use minutae_core::{AppSettings, Transport};
use minutae_llm::{
    ApiConfig, KeyringStore, MemorySecretStore, Router, SecretStore, SummaryMeta, SummaryOptions,
    summarize, usage_channel,
};

#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: summarize <transcript.txt>");
    let transcript = std::fs::read_to_string(&path).expect("read transcript");

    let mut settings = AppSettings {
        codex_model: std::env::var("MINUTAE_CODEX_MODEL").ok(),
        ..Default::default()
    };
    if let Ok(m) = std::env::var("MINUTAE_OLLAMA_MODEL") {
        settings.ollama_model = m;
    }
    if let Ok(order) = std::env::var("MINUTAE_ORDER") {
        settings.transport_order = order
            .split(',')
            .filter_map(|s| match s.trim() {
                "codex" => Some(Transport::CodexCli),
                "grok" => Some(Transport::GrokCli),
                "ollama" => Some(Transport::Ollama),
                _ => None,
            })
            .collect();
    }
    let secrets: Arc<dyn SecretStore> = if std::env::var("MINUTAE_API").as_deref() == Ok("1") {
        settings.api_fallback_enabled = true;
        Arc::new(KeyringStore::default())
    } else {
        Arc::new(MemorySecretStore::default())
    };
    let (sink, mut usage) = usage_channel();
    let router =
        Router::from_settings(&settings, secrets, ApiConfig::default()).with_usage_sink(sink);

    for (t, a) in router.statuses().await {
        eprintln!("{t:?}: {a:?}");
    }
    let meta = SummaryMeta {
        meeting_id: uuid_like(),
        title: "Smoke test".into(),
        date: chrono::Local::now().naive_local(),
        duration: None,
        participants: vec![],
        recording: None,
    };
    let started = std::time::Instant::now();
    match summarize(&router, &transcript, &meta, &SummaryOptions::default()).await {
        Ok(out) => {
            println!("{}", out.summary.markdown);
            eprintln!("takeaway: {}", out.summary.takeaway);
            eprintln!(
                "provider: {} model: {} in {:.1}s",
                out.summary.provider,
                out.summary.model,
                started.elapsed().as_secs_f32()
            );
        }
        Err(e) => {
            let f = e.failure();
            eprintln!("FAILED: {} [{:?}] fix={:?}", f.message, f.kind, f.fix);
            for a in &e.attempts {
                eprintln!("  {:?}: {:?} {}", a.transport, a.kind, a.detail);
            }
        }
    }
    drop(router);
    while let Ok(r) = usage.try_recv() {
        eprintln!(
            "usage: {} {:?} in={:?} out={:?} {}ms err={:?}",
            r.provider, r.model, r.tokens_in, r.tokens_out, r.duration_ms, r.error_class
        );
    }
}

fn uuid_like() -> minutae_core::MeetingId {
    minutae_core::MeetingId::nil()
}
