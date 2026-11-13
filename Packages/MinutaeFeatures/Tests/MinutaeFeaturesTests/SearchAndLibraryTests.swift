import AppKit
import Foundation
import MinutaeCore
import MinutaeDesign
import MinutaeLibrary
import SwiftUI
import Testing

@testable import MinutaeFeatures

@MainActor
@Suite struct SearchAndLibraryTests {
    private func makeDB() throws -> MinutaeDatabase {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return try MinutaeDatabase(path: dir.appendingPathComponent("t.db").path)
    }

    private func seed(_ db: MinutaeDatabase) throws {
        try Repository<Meeting>(db).save(
            Meeting(id: "m1", title: "Design sync", started_at: "2026-01-02T10:00:00Z",
                ended_at: nil, duration_seconds: 120, status: "done"))
        try Repository<Dictation>(db).save(
            Dictation(id: "d1", started_at: "2026-01-03T09:00:00Z", duration_ms: 3000,
                target_app_bundle_id: nil, target_app_name: "Notes", raw_text: "buy oat milk",
                final_text: "Buy oat milk.", language: "en", insert_method: "pasteboard",
                status: "done", audio_path: nil, error: nil, created_at: "2026-01-03T09:00:00Z"))
    }

    @Test func libraryMergesAndFilters() throws {
        let db = try makeDB()
        try seed(db)
        let model = LibraryViewModel(database: db)
        model.refresh()
        #expect(model.items.map(\.id) == ["d1", "m1"])
        model.filter = .meetings
        #expect(model.visible.map(\.id) == ["m1"])
        model.filter = .all
        model.query = "notes"
        #expect(model.visible.map(\.id) == ["d1"])
    }

    @Test func navigationOpenSelectsRouteAndPending() {
        let nav = NavigationStore()
        nav.open(.dictation("d1"))
        #expect(nav.route == .dictate)
        #expect(nav.pending == .dictation("d1"))
        nav.open(.meeting("m1"))
        #expect(nav.route == .meetings)
    }

    @Test func searchFindsKeywordHitsAndMovesSelection() async throws {
        let db = try makeDB()
        try seed(db)
        let service = SearchService(database: db, embeddings: HashEmbeddingProvider())
        let model = SearchModel(service: service, meetingIDs: { ["m1"] }, debounce: .milliseconds(1))
        model.query = "oat"
        await model.runNow()
        #expect(model.hits.contains { $0.id == "d1" })
        model.move(5)
        #expect(model.selection == model.hits.count - 1)
        model.move(-99)
        #expect(model.selection == 0)
        model.reset()
        #expect(model.hits.isEmpty && model.query.isEmpty)
    }

    @Test func rendersSearchSheetAndLibrary() async throws {
        _ = MinutaeDesign.registerFonts()
        let db = try makeDB()
        try seed(db)
        let lib = LibraryViewModel(database: db)
        lib.refresh()
        let views: [(String, AnyView)] = [
            ("library", AnyView(LibraryContent(model: lib, navigation: nil))),
            ("search-empty", AnyView(SearchSheet(model: SearchModel(
                service: SearchService(database: db, embeddings: HashEmbeddingProvider()),
                meetingIDs: { [] })))),
        ]
        for (name, view) in views {
            let host = NSHostingView(rootView: view.frame(width: 700, height: 600)
                .background(Color.minutae.bg).environment(\.colorScheme, .light))
            host.frame = NSRect(x: 0, y: 0, width: 700, height: 600)
            let window = NSWindow(contentRect: host.frame, styleMask: [.titled], backing: .buffered, defer: false)
            window.appearance = NSAppearance(named: .aqua)
            window.contentView = host
            host.layoutSubtreeIfNeeded()
            try? await Task.sleep(for: .milliseconds(300))
            let rep = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
            host.cacheDisplay(in: host.bounds, to: rep)
            let png = try #require(rep.representation(using: .png, properties: [:]))
            if let dir = ProcessInfo.processInfo.environment["MINUTAE_SNAPSHOT_DIR"] {
                try png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(name).png"))
            }
        }
    }
}
