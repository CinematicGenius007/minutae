//! USD per 1M tokens for API models. CLI and Ollama transports cost 0 (not recorded).
//! Prices change: treat results as estimates. Anthropic rates are first-party list prices
//! (Nov 2026); OpenAI and Gemini rows are best-known list prices and should be re-checked.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input_per_m: f64,
    pub output_per_m: f64,
}

/// (provider, model id or prefix, input, output)
const TABLE: &[(&str, &str, f64, f64)] = &[
    ("anthropic", "claude-fable-5-2", 8.0, 40.0),
    ("anthropic", "claude-opus-5-5", 4.0, 20.0),
    ("anthropic", "claude-opus-5", 5.0, 25.0),
    ("anthropic", "claude-opus-4", 5.0, 25.0),
    ("anthropic", "claude-sonnet-5-5", 2.0, 10.0),
    ("anthropic", "claude-sonnet-5", 2.0, 10.0),
    ("anthropic", "claude-sonnet-4-6", 3.0, 15.0),
    ("anthropic", "claude-haiku-5-5", 0.10, 0.50),
    ("anthropic", "claude-haiku-4-5", 1.0, 5.0),
    ("openai", "gpt-5-nano", 0.05, 0.40),
    ("openai", "gpt-5-mini", 0.25, 2.0),
    ("openai", "gpt-5", 1.25, 10.0),
    ("openai", "gpt-4.1-mini", 0.40, 1.60),
    ("openai", "gpt-4o-mini", 0.15, 0.60),
    ("gemini", "gemini-2.5-flash-lite", 0.10, 0.40),
    ("gemini", "gemini-2.5-flash", 0.30, 2.50),
    ("gemini", "gemini-2.5-pro", 1.25, 10.0),
];

/// Exact model match first, then the longest matching prefix within the provider.
pub fn price(provider: &str, model: &str) -> Option<Price> {
    let model = model.trim().to_lowercase();
    let rows = TABLE.iter().filter(|r| r.0 == provider);
    rows.clone()
        .find(|r| r.1 == model)
        .or_else(|| {
            rows.filter(|r| model.starts_with(r.1))
                .max_by_key(|r| r.1.len())
        })
        .map(|r| Price {
            input_per_m: r.2,
            output_per_m: r.3,
        })
}

/// Cost in USD, or None when the model is unknown or a count is missing.
pub fn cost_usd(
    provider: &str,
    model: &str,
    tokens_in: Option<u32>,
    tokens_out: Option<u32>,
) -> Option<f64> {
    let p = price(provider, model)?;
    let (i, o) = (tokens_in? as f64, tokens_out? as f64);
    Some((i * p.input_per_m + o * p.output_per_m) / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        assert_eq!(
            price("anthropic", "claude-opus-5-5").unwrap().input_per_m,
            4.0
        );
        // Prefix: dated/variant ids fall back to their family.
        assert_eq!(
            price("openai", "gpt-5-mini-2026-01-01")
                .unwrap()
                .input_per_m,
            0.25
        );
        assert_eq!(
            price("gemini", "gemini-2.5-flash-lite")
                .unwrap()
                .input_per_m,
            0.10
        );
        assert!(price("anthropic", "gpt-5").is_none());
        let c = cost_usd(
            "anthropic",
            "claude-haiku-5-5",
            Some(1_000_000),
            Some(100_000),
        )
        .unwrap();
        assert!((c - 0.15).abs() < 1e-9);
        assert_eq!(
            cost_usd("anthropic", "claude-opus-5-5", None, Some(1)),
            None
        );
    }
}
