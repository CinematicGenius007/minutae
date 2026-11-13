public enum MinutaeDesignModule {
    public static let name = "MinutaeDesign"
}

/// Registers the bundled variable fonts for this process.
///
/// Safe to call more than once. A missing or unreadable file is reported and does not throw.
/// Text falls back to system fonts when a family did not register.
public enum MinutaeDesign: Sendable {
    @discardableResult
    public static func registerFonts() -> FontRegistrationResult {
        FontBook.shared.registerBundle()
    }
}
