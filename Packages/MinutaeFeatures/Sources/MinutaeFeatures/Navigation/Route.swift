/// Sidebar destinations. Command-1 through Command-5 follow this order.
public enum Route: String, CaseIterable, Hashable, Identifiable, Sendable {
    case meetings
    case library
    case dictate
    case usage
    case settings

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .meetings: "Meetings"
        case .library: "Library"
        case .dictate: "Dictate"
        case .usage: "Usage"
        case .settings: "Settings"
        }
    }

    public var systemImage: String {
        switch self {
        case .meetings: "calendar"
        case .library: "books.vertical"
        case .dictate: "waveform"
        case .usage: "chart.bar"
        case .settings: "gearshape"
        }
    }

    /// 1-based index for ⌘1 … ⌘5.
    public var commandNumber: Int {
        switch self {
        case .meetings: 1
        case .library: 2
        case .dictate: 3
        case .usage: 4
        case .settings: 5
        }
    }

    public init?(commandNumber: Int) {
        switch commandNumber {
        case 1: self = .meetings
        case 2: self = .library
        case 3: self = .dictate
        case 4: self = .usage
        case 5: self = .settings
        default: return nil
        }
    }
}
