/// How long captured audio is kept. `never` means the app does not purge it.
public enum AudioRetention: Hashable, Sendable, Codable {
    case never
    case days(count: Int)
}
