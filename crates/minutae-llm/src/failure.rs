//! Failure kind → one plain sentence + one fix (docs/REDESIGN.md §5).

use minutae_core::{Failure, FailureKind, Transport};

use crate::types::transport_label;

/// Plain-English failure for the UI. `transport` names the provider in the copy when known.
pub fn failure_for(kind: FailureKind, transport: Option<Transport>) -> Failure {
    let name = transport
        .map(transport_label)
        .unwrap_or("The summary provider");
    let api = transport == Some(Transport::Api);
    let (message, fix): (String, Option<&str>) = match kind {
        FailureKind::NotInstalled => (format!("{name} isn't installed."), Some("Show how")),
        FailureKind::NotLoggedIn if api => (
            "No working API key is saved.".into(),
            Some("Add an API key"),
        ),
        FailureKind::NotLoggedIn => (
            format!("{name} isn't signed in."),
            Some(match transport {
                Some(Transport::CodexCli) => "Sign in to Codex",
                Some(Transport::GrokCli) => "Sign in to Grok",
                _ => "Open Settings → Summaries",
            }),
        ),
        FailureKind::RateLimited => (format!("{name} is busy. Trying the next provider."), None),
        FailureKind::Timeout => ("The summary took too long.".into(), Some("Retry")),
        FailureKind::NoProviderReady => (
            "No summary provider is ready. Transcript is saved.".into(),
            Some("Open Settings → Summaries"),
        ),
        FailureKind::Cancelled => ("The summary was cancelled.".into(), Some("Retry")),
        FailureKind::InvalidOutput => ("The summary came back unreadable.".into(), Some("Retry")),
        FailureKind::ModelRejected => (
            format!("{name} rejected the selected model."),
            Some("Choose a model"),
        ),
        FailureKind::ModelMissing => (
            "The selected model isn't downloaded yet.".into(),
            Some("Download model"),
        ),
        FailureKind::Unavailable => (format!("{name} isn't available right now."), Some("Retry")),
        FailureKind::PermissionDenied => (
            format!("Minutae isn't allowed to use {}.", lower_first(name)),
            Some("Show how"),
        ),
        FailureKind::Io => (
            "Minutae couldn't read or write a file.".into(),
            Some("Retry"),
        ),
        FailureKind::Internal => (
            "Something went wrong. Transcript is saved.".into(),
            Some("Retry"),
        ),
    };
    Failure {
        kind,
        message,
        fix: fix.map(String::from),
    }
}

fn lower_first(s: &str) -> String {
    // "The API provider" → "the API provider"; product names stay capitalised.
    match s.strip_prefix("The ") {
        Some(rest) => format!("the {rest}"),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redesign_table() {
        let f = failure_for(FailureKind::NotInstalled, Some(Transport::CodexCli));
        assert_eq!(
            (f.message.as_str(), f.fix.as_deref()),
            ("Codex isn't installed.", Some("Show how"))
        );
        let f = failure_for(FailureKind::NotLoggedIn, Some(Transport::CodexCli));
        assert_eq!(
            (f.message.as_str(), f.fix.as_deref()),
            ("Codex isn't signed in.", Some("Sign in to Codex"))
        );
        let f = failure_for(FailureKind::RateLimited, Some(Transport::CodexCli));
        assert_eq!(
            (f.message.as_str(), f.fix),
            ("Codex is busy. Trying the next provider.", None)
        );
        let f = failure_for(FailureKind::Timeout, None);
        assert_eq!(
            (f.message.as_str(), f.fix.as_deref()),
            ("The summary took too long.", Some("Retry"))
        );
        let f = failure_for(FailureKind::NoProviderReady, None);
        assert_eq!(
            (f.message.as_str(), f.fix.as_deref()),
            (
                "No summary provider is ready. Transcript is saved.",
                Some("Open Settings → Summaries")
            )
        );
    }

    #[test]
    fn every_kind_has_a_sentence() {
        use FailureKind::*;
        for k in [
            NotInstalled,
            NotLoggedIn,
            RateLimited,
            Timeout,
            Cancelled,
            InvalidOutput,
            ModelRejected,
            Unavailable,
            NoProviderReady,
            PermissionDenied,
            ModelMissing,
            Io,
            Internal,
        ] {
            for t in [None, Some(Transport::Api), Some(Transport::Ollama)] {
                let f = failure_for(k, t);
                assert!(f.message.ends_with('.'), "{k:?}");
                assert_eq!(f.kind, k);
            }
        }
        assert_eq!(
            failure_for(NotLoggedIn, Some(Transport::Api))
                .fix
                .as_deref(),
            Some("Add an API key")
        );
    }
}
