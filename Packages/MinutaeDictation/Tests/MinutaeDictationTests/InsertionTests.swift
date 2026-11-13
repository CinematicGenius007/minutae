import AppKit
import CoreGraphics
import Testing
@testable import MinutaeDictation

@MainActor private final class Events: InsertionEvents {
    var calls: [InsertionMethod] = []
    var succeeds: InsertionMethod? = nil
    func paste() -> Bool { calls.append(.paste); return succeeds == .paste }
    func typeUnicode(_ text: String) -> Bool { calls.append(.unicode); return succeeds == .unicode }
}
@MainActor @Test func insertionFallbackAndMemory() async {
    let board = FakeBoard([])
    let events = Events()
    let ax = FakeAccessibilityInserter(succeeds: true)
    let inserter = SystemTextInserter(pasteboard: board, events: events, accessibility: ax, wait: { _ in }, trusted: { true }, secure: { false }, frontmost: { "example.app" })
    #expect(await inserter.insert("hello") == .inserted(method: .accessibility, confirmed: true))
    #expect(events.calls == [.paste])
    #expect(ax.inputs == ["hello"])
    events.calls = []
    #expect(await inserter.insert("again") == .inserted(method: .accessibility, confirmed: true))
    #expect(events.calls.isEmpty)
    #expect(ax.inputs == ["hello", "again"])
}
@MainActor @Test func secureRefusal() async {
    let events = Events()
    let inserter = SystemTextInserter(pasteboard: FakeBoard([]), events: events, trusted: { true }, secure: { true }, frontmost: { nil })
    #expect(await inserter.insert("secret") == .refusedSecureField)
    #expect(events.calls.isEmpty)
}
private let interactivePasteboardAvailable: Bool = {
    guard ProcessInfo.processInfo.environment["CI"] == nil,
          let session = CGSessionCopyCurrentDictionary() as? [String: Any] else { return false }
    return session["kCGSessionOnConsoleKey"] as? Bool == true
}()

@MainActor @Test(.enabled(if: interactivePasteboardAvailable, "Requires an interactive macOS desktop session; pasteboard services are unavailable on headless CI")) func namedPasteboardRestoresData() async {
    let board = NSPasteboard(name: .init(UUID().uuidString))
    defer { board.releaseGlobally() }
    let item = NSPasteboardItem()
    let bytes = Data([0, 1, 255, 42])
    item.setData(bytes, forType: .init("org.minutae.test"))
    board.writeObjects([item])
    let events = Events(); events.succeeds = .paste
    let inserter = SystemTextInserter(pasteboard: SystemPasteboard(board), events: events, trusted: { true }, secure: { false })
    #expect(await inserter.insert("hello") == .inserted(method: .paste, confirmed: false))
    #expect(board.pasteboardItems?.first?.data(forType: .init("org.minutae.test")) == bytes)
}

@MainActor private final class FakeBoard: InsertionPasteboard {
    var items: PasteboardSnapshot
    var changeCount = 0
    init(_ items: PasteboardSnapshot) { self.items = items }
    func snapshot() -> PasteboardSnapshot? { items }
    func writeText(_ text: String) -> Bool { items = [["text": Data(text.utf8)]]; changeCount += 1; return true }
    func restore(_ snapshot: PasteboardSnapshot) -> Bool { items = snapshot; changeCount += 1; return true }
}
@MainActor @Test func fakePasteboardRestoresAllBytes() async {
    let original: PasteboardSnapshot = [["a": Data([0, 255]), "b": Data([1, 2, 3])], ["c": Data([128, 0]), "d": Data()]]
    let board = FakeBoard(original)
    let events = Events(); events.succeeds = .paste
    let inserter = SystemTextInserter(pasteboard: board, events: events, wait: { _ in }, trusted: { true }, secure: { false }, frontmost: { nil })
    #expect(await inserter.insert("text") == .inserted(method: .paste, confirmed: false))
    #expect(board.items == original)
}
@MainActor @Test func externalClipboardWriteIsPreserved() async {
    let board = FakeBoard([["old": Data([1])]])
    let events = Events(); events.succeeds = .paste
    let inserter = SystemTextInserter(pasteboard: board, events: events, wait: { _ in _ = board.writeText("external") }, trusted: { true }, secure: { false }, frontmost: { nil })
    #expect(await inserter.insert("text") == .inserted(method: .paste, confirmed: false))
    #expect(board.items == [["text": Data("external".utf8)]])
}
