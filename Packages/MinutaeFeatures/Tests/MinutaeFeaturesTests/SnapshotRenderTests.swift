import AppKit
import MinutaeCore
import MinutaeAudio
import MinutaeDesign
import MinutaeMeetings
import SwiftUI
import Testing

@testable import MinutaeFeatures

/// Renders screens to PNG so layout can be reviewed without screen-recording access.
/// Set MINUTAE_SNAPSHOT_DIR to choose the output folder; otherwise the test only checks that
/// each screen produces a non-empty image at several window sizes.
@MainActor
@Suite struct SnapshotRenderTests {
    private func render<V: View>(_ name: String, _ view: V, width: CGFloat, height: CGFloat) async throws {
        _ = MinutaeDesign.registerFonts()
        let content = view
            .frame(width: width, height: height)
            .background(Color.minutae.bg)
            .environment(\.colorScheme, .light)
        let host = NSHostingView(rootView: content)
        host.frame = NSRect(x: 0, y: 0, width: width, height: height)
        let window = NSWindow(
            contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.appearance = NSAppearance(named: .aqua)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        try? await Task.sleep(for: .milliseconds(300))
        host.layoutSubtreeIfNeeded()
        let rep = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: rep)
        let png = try #require(rep.representation(using: .png, properties: [:]))
        #expect(rep.pixelsWide > 0)
        if let dir = ProcessInfo.processInfo.environment["MINUTAE_SNAPSHOT_DIR"] {
            try png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(name).png"))
        }
    }

    @Test(arguments: [(968.0, 800.0), (760.0, 640.0), (1400.0, 900.0)])
    func screensRender(size: (CGFloat, CGFloat)) async throws {
        let tag = "\(Int(size.0))x\(Int(size.1))"
        try await render("settings-\(tag)", SettingsScreen(model: SettingsViewModel(store: InMemorySettingsStore())), width: size.0, height: size.1)
        try await render("usage-\(tag)", UsageScreen(model: UsageViewModel(loadRows: { [] })), width: size.0, height: size.1)
        try await render("dictate-\(tag)", DictateScreen(model: DictatePreview.listModel(), onOpen: { _ in }), width: size.0, height: size.1)
        try await render("detail-\(tag)", DictationDetailScreen(model: DictatePreview.detailModel(), onBack: {}), width: size.0, height: size.1)
        try await render(
            "shell-\(tag)",
            AppShellView(
                navigation: NavigationStore(), isRecording: false, canRecord: true,
                onToggleRecording: {}, onSearch: {}, isSearchPresented: .constant(false)),
            width: size.0, height: size.1)
    }

    @Test func meetingsWithRowsRender() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let db = try MinutaeDatabase(path: dir.appendingPathComponent("t.db").path)
        let runner = JobRunner(database: db, handlers: [:])
        let iso = ISO8601DateFormatter()
        let now = Date()
        let rows: [(String, Date, Int, String)] = [
            ("Q2 design sync", now.addingTimeInterval(-3600), 1934, "done"),
            ("Standup", now.addingTimeInterval(-7200), 540, "processing"),
            ("Customer call: Acme", now.addingTimeInterval(-86400 - 3600), 3725, "done"),
            ("Family Test", now.addingTimeInterval(-86400 * 4), 45, "failed"),
        ]
        for (i, row) in rows.enumerated() {
            try Repository<Meeting>(db).save(
                Meeting(
                    id: "m\(i)", title: row.0, started_at: iso.string(from: row.1), ended_at: nil,
                    duration_seconds: row.2, status: row.3))
        }
        let model = MeetingsViewModel(database: db, runner: runner, summariesDirectory: dir)
        model.refresh()
        for (w, h) in [(968.0, 800.0), (760.0, 640.0)] {
            try await render("meetings-\(Int(w))x\(Int(h))", MeetingsScreen(model: model), width: w, height: h)
        }
        let detail = MeetingDetailViewModel(
            meeting: try #require(model.meetings.first), database: db, runner: runner,
            summariesDirectory: dir)
        try await render("meeting-detail", MeetingDetailScreen(model: detail), width: 720, height: 640)
    }
}
