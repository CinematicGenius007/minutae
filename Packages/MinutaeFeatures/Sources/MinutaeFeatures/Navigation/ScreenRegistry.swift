import Foundation
import MinutaeCore
import SwiftUI

/// One closure per route. Shell views call `view(for:)` and do not name screens.
///
/// Install a screen by replacing that route's closure. `AppShellView` does not
/// mention the screen types, so the swap stays in this file.
@MainActor
public struct ScreenRegistry {
    public var meetings: () -> AnyView
    public var library: () -> AnyView
    public var dictate: () -> AnyView
    public var usage: () -> AnyView
    public var settings: () -> AnyView

    public init(
        meetings: @escaping () -> AnyView,
        library: @escaping () -> AnyView,
        dictate: @escaping () -> AnyView,
        usage: @escaping () -> AnyView,
        settings: @escaping () -> AnyView
    ) {
        self.meetings = meetings
        self.library = library
        self.dictate = dictate
        self.usage = usage
        self.settings = settings
    }

    /// Library and Dictate are placeholders. Meetings, Settings, and Usage render
    /// with stand-in data until the app replaces those closures.
    public static let placeholders = ScreenRegistry(
        meetings: { AnyView(MeetingsScreen(model: MeetingsScreenStandIn.model)) },
        library: { AnyView(LibraryScreen()) },
        dictate: { AnyView(PlaceholderScreen(title: "Dictate")) },
        usage: { AnyView(UsageScreen(model: UsageViewModel(loadRows: { [] }))) },
        settings: {
            AnyView(SettingsScreen(model: SettingsViewModel(store: InMemorySettingsStore())))
        }
    )

    public func view(for route: Route) -> AnyView {
        switch route {
        case .meetings: meetings()
        case .library: library()
        case .dictate: dictate()
        case .usage: usage()
        case .settings: settings()
        }
    }

    public func replacing(_ route: Route, with make: @escaping () -> AnyView) -> ScreenRegistry {
        var copy = self
        switch route {
        case .meetings: copy.meetings = make
        case .library: copy.library = make
        case .dictate: copy.dictate = make
        case .usage: copy.usage = make
        case .settings: copy.settings = make
        }
        return copy
    }
}

/// Empty meetings database in a temp directory. The app replaces this before the
/// window shows the real store. The default database path is `~/Library/Minutae`.
@MainActor
private enum MeetingsScreenStandIn {
    static let model: MeetingsViewModel = {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "minutae-shell-meetings",
            isDirectory: true
        )
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("meetings.db").path
        guard let database = try? MinutaeDatabase(path: path) else {
            preconditionFailure("The meetings stand-in database could not be opened.")
        }
        return MeetingsViewModel(
            database: database,
            runner: JobRunner(database: database, handlers: [:]),
            summariesDirectory: directory
        )
    }()
}
