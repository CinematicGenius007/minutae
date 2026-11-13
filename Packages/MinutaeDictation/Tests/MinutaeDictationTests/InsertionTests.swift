import AppKit
import Testing
@testable import MinutaeDictation

@MainActor private final class Events: InsertionEvents {
    var calls: [InsertionMethod] = []
    var succeeds: InsertionMethod? = nil
    func paste() -> Bool { calls.append(.paste); return succeeds == .paste }
    func accessibilityInsert(_ text: String) -> Bool { calls.append(.accessibility); return succeeds == .accessibility }
    func typeUnicode(_ text: String) -> Bool { calls.append(.unicode); return succeeds == .unicode }
}
@MainActor @Test func insertionFallbackAndMemory() async {
    let board = NSPasteboard(name: .init(UUID().uuidString))
    defer { board.releaseGlobally() }
    let events = Events()
    events.succeeds = .accessibility
    let inserter = SystemTextInserter(pasteboard: SystemPasteboard(board), events: events, trusted: { true }, secure: { false }, frontmost: { "example.app" })
    #expect(await inserter.insert("hello") == .inserted(.accessibility))
    #expect(events.calls == [.paste, .accessibility])
    events.calls = []
    #expect(await inserter.insert("again") == .inserted(.accessibility))
    #expect(events.calls == [.accessibility])
}
@MainActor @Test func secureRefusal() async {
    let events = Events()
    let inserter = SystemTextInserter(events: events, trusted: { true }, secure: { true })
    #expect(await inserter.insert("secret") == .refusedSecureField)
    #expect(events.calls.isEmpty)
}
@MainActor @Test func namedPasteboardRestoresData() async {
    let board = NSPasteboard(name: .init(UUID().uuidString))
    defer { board.releaseGlobally() }
    let item = NSPasteboardItem()
    let bytes = Data([0, 1, 255, 42])
    item.setData(bytes, forType: .init("org.minutae.test"))
    board.writeObjects([item])
    let events = Events(); events.succeeds = .paste
    let inserter = SystemTextInserter(pasteboard: SystemPasteboard(board), events: events, trusted: { true }, secure: { false })
    #expect(await inserter.insert("hello") == .inserted(.paste))
    #expect(board.pasteboardItems?.first?.data(forType: .init("org.minutae.test")) == bytes)
}
