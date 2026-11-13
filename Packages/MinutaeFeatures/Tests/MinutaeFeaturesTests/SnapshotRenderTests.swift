import AppKit
import MinutaeCore
import MinutaeDesign
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
}
