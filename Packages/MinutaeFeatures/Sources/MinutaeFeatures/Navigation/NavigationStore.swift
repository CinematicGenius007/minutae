import Observation

/// Which sidebar destination is showing.
@MainActor
@Observable
public final class NavigationStore {
    public private(set) var route: Route

    public init(route: Route = .meetings) {
        self.route = route
    }

    public func select(_ route: Route) {
        self.route = route
    }

    /// Selects the destination for ⌘1 … ⌘5. Other numbers do nothing.
    public func select(commandNumber: Int) {
        guard let route = Route(commandNumber: commandNumber) else { return }
        select(route)
    }
}
