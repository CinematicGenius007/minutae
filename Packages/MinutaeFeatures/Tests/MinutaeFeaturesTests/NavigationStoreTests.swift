import Testing

@testable import MinutaeFeatures

@MainActor @Test func navigationStartsOnMeetings() {
    let store = NavigationStore()
    #expect(store.route == .meetings)
}

@MainActor @Test func selectChangesTheRoute() {
    let store = NavigationStore()
    store.select(.settings)
    #expect(store.route == .settings)
    store.select(.meetings)
    #expect(store.route == .meetings)
}

@MainActor @Test func commandNumbersFollowSidebarOrder() {
    #expect(
        Route.allCases.map(\.title) == [
            "Meetings", "Library", "Dictate", "Usage", "Settings",
        ])
    #expect(Route.allCases.map(\.commandNumber) == [1, 2, 3, 4, 5])
    for route in Route.allCases {
        #expect(Route(commandNumber: route.commandNumber) == route)
    }
    #expect(Route(commandNumber: 0) == nil)
    #expect(Route(commandNumber: 6) == nil)
}

@MainActor @Test func commandNumberSelectsThatRoute() {
    let store = NavigationStore()
    store.select(commandNumber: 4)
    #expect(store.route == .usage)
    store.select(commandNumber: 2)
    #expect(store.route == .library)
}

@MainActor @Test func commandNumberOutsideTheSidebarDoesNothing() {
    let store = NavigationStore(route: .dictate)
    store.select(commandNumber: 0)
    store.select(commandNumber: 6)
    store.select(commandNumber: -1)
    #expect(store.route == .dictate)
}
