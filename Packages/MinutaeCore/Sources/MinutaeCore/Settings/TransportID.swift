/// LLM transport identifier from ARCHITECTURE §5.2.
///
/// Encoded raw values match the Swift case names (`codexCLI`, not `codex-cli`).
public enum TransportID: String, Codable, CaseIterable, Hashable, Sendable {
    case codexCLI
    case grokCLI
    case ollama
    case api

    /// Subscription and local transports, in the architecture's fallback order.
    /// The Advanced `api` transport is off this list until the user enables it.
    public static let defaultFallbackOrder: [TransportID] = [.codexCLI, .grokCLI, .ollama]
}
