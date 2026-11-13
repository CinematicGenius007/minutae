/// Keychain account for one Advanced API transport.
///
/// Raw values are the item's account attribute. The service is always `com.nebula.minutae`
/// in production, so these names cannot collide with another app's items.
public enum APIKeyAccount: String, CaseIterable, Codable, Hashable, Sendable {
    case anthropic = "api.anthropic"
    case openai = "api.openai"
    case gemini = "api.gemini"
}
