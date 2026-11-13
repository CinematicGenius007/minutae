import Observation

/// A specific item a screen should open as soon as it is visible.
public enum OpenTarget: Equatable, Sendable {
    case meeting(String)
    case dictation(String)
}

/// Which sidebar destination is showing.
@MainActor
@Observable
public final class NavigationStore {
    public private(set) var route: Route
    /// Set by `open(_:)`. The destination screen clears it once it has opened the item.
    public var pending: OpenTarget?

    public init(route: Route = .meetings) {
        self.route = route
    }

    public func select(_ route: Route) {
        self.route = route
    }

    public func open(_ target: OpenTarget) {
        switch target {
        case .meeting: route = .meetings
        case .dictation: route = .dictate
        }
        pending = target
    }

    /// Selects the destination for ⌘1 … ⌘5. Other numbers do nothing.
    public func select(commandNumber: Int) {
        guard let route = Route(commandNumber: commandNumber) else { return }
        select(route)
    }
}
