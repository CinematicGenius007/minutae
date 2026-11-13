import SwiftUI
import Testing

@testable import MinutaeFeatures

@MainActor @Test func viewCallsOnlyTheClosureForThatRoute() {
    let hits = ScreenHits()
    let registry = probeRegistry(hits)
    _ = registry.view(for: .library)
    #expect(hits.marks == ["library"])
}

@MainActor @Test func replacingOneRouteLeavesTheOthers() {
    let hits = ScreenHits()
    let registry = probeRegistry(hits)
    let swapped = registry.replacing(.settings) { hits.mark("custom") }
    _ = registry.view(for: .settings)
    _ = swapped.view(for: .settings)
    _ = swapped.view(for: .meetings)
    #expect(hits.marks == ["settings", "custom", "meetings"])
}

@MainActor @Test func placeholdersBuildEveryRoute() {
    let registry = ScreenRegistry.placeholders
    for route in Route.allCases {
        _ = registry.view(for: route)
    }
}

@MainActor
private final class ScreenHits {
    var marks: [String] = []

    func mark(_ name: String) -> AnyView {
        marks.append(name)
        return AnyView(EmptyView())
    }
}

@MainActor
private func probeRegistry(_ hits: ScreenHits) -> ScreenRegistry {
    ScreenRegistry(
        meetings: { hits.mark("meetings") },
        library: { hits.mark("library") },
        dictate: { hits.mark("dictate") },
        usage: { hits.mark("usage") },
        settings: { hits.mark("settings") }
    )
}
