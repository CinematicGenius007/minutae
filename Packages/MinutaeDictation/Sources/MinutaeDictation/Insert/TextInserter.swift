import AppKit
import ApplicationServices
import Carbon

public enum InsertionMethod: String, Sendable, CaseIterable { case paste, accessibility, unicode }
public enum ClipboardReason: Equatable, Sendable { case accessibilityNotTrusted, noMethodWorked }
public enum InsertionResult: Equatable, Sendable {
    case inserted(InsertionMethod)
    case copiedToClipboard(ClipboardReason)
    case refusedSecureField
    case failed
}

@MainActor public protocol TextInserter {
    func insert(_ text: String) async -> InsertionResult
}
@MainActor public protocol InsertionMethodStore: AnyObject {
    func method(for bundleID: String) -> InsertionMethod?
    func setMethod(_ method: InsertionMethod, for bundleID: String)
}
@MainActor public final class InMemoryInsertionMethodStore: InsertionMethodStore {
    private var values: [String: InsertionMethod] = [:]
    public init() {}
    public func method(for bundleID: String) -> InsertionMethod? { values[bundleID] }
    public func setMethod(_ method: InsertionMethod, for bundleID: String) { values[bundleID] = method }
}
public typealias PasteboardSnapshot = [[String: Data]]
@MainActor public protocol InsertionPasteboard {
    var changeCount: Int { get }
    func snapshot() -> PasteboardSnapshot?
    func writeText(_ text: String) -> Bool
    func restore(_ snapshot: PasteboardSnapshot) -> Bool
}
@MainActor public protocol InsertionEvents {
    func paste() -> Bool
    func accessibilityInsert(_ text: String) -> Bool
    func typeUnicode(_ text: String) -> Bool
}
@MainActor public final class SystemPasteboard: InsertionPasteboard {
    private let board: NSPasteboard
    public init(_ board: NSPasteboard = .general) { self.board = board }
    public var changeCount: Int { board.changeCount }
    public func snapshot() -> PasteboardSnapshot? {
        var result: PasteboardSnapshot = []
        for item in board.pasteboardItems ?? [] {
            var entry: [String: Data] = [:]
            for type in item.types {
                guard let data = item.data(forType: type) else { return nil }
                entry[type.rawValue] = data
            }
            result.append(entry)
        }
        return result
    }
    public func writeText(_ text: String) -> Bool {
        board.clearContents()
        return board.setString(text, forType: .string)
    }
    public func restore(_ snapshot: PasteboardSnapshot) -> Bool {
        let items = snapshot.map { entry in
            let item = NSPasteboardItem()
            for (type, data) in entry { item.setData(data, forType: NSPasteboard.PasteboardType(type)) }
            return item
        }
        board.clearContents()
        return items.isEmpty || board.writeObjects(items)
    }
}
@MainActor public final class SystemInsertionEvents: InsertionEvents {
    public init() {}
    public func paste() -> Bool {
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: 9, keyDown: false) else { return false }
        down.flags = .maskCommand; up.flags = .maskCommand
        down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
        return true
    }
    public func accessibilityInsert(_ text: String) -> Bool {
        let system = AXUIElementCreateSystemWide()
        var focused: CFTypeRef?
        guard AXUIElementCopyAttributeValue(system, kAXFocusedUIElementAttribute as CFString, &focused) == .success,
              let focused, CFGetTypeID(focused) == AXUIElementGetTypeID() else { return false }
        let element = focused as! AXUIElement
        return AXUIElementSetAttributeValue(element, kAXSelectedTextAttribute as CFString, text as CFString) == .success
    }
    public func typeUnicode(_ text: String) -> Bool {
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) else { return false }
        let units = Array(text.utf16)
        units.withUnsafeBufferPointer { buffer in
            down.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
            up.keyboardSetUnicodeString(stringLength: buffer.count, unicodeString: buffer.baseAddress)
        }
        down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
        return true
    }
}
@MainActor public final class SystemTextInserter: TextInserter {
    private let pasteboard: any InsertionPasteboard
    private let events: any InsertionEvents
    private let store: any InsertionMethodStore
    private let trusted: () -> Bool
    private let secure: () -> Bool
    private let frontmost: () -> String?
    public init(pasteboard: any InsertionPasteboard = SystemPasteboard(), events: any InsertionEvents = SystemInsertionEvents(), store: any InsertionMethodStore = InMemoryInsertionMethodStore(), trusted: @escaping () -> Bool, secure: @escaping () -> Bool = { IsSecureEventInputEnabled() }, frontmost: @escaping () -> String? = { NSWorkspace.shared.frontmostApplication?.bundleIdentifier }) {
        self.pasteboard = pasteboard; self.events = events; self.store = store
        self.trusted = trusted; self.secure = secure; self.frontmost = frontmost
    }
    public func insert(_ text: String) async -> InsertionResult {
        guard !secure() else { return .refusedSecureField }
        let app = frontmost()
        guard trusted() else { return pasteboard.writeText(text) ? .copiedToClipboard(.accessibilityNotTrusted) : .failed }
        var methods = InsertionMethod.allCases
        if let app, let preferred = store.method(for: app) {
            methods.removeAll { $0 == preferred }; methods.insert(preferred, at: 0)
        }
        for method in methods {
            guard !secure() else { return .refusedSecureField }
            let success: Bool
            switch method {
            case .paste:
                if let snapshot = pasteboard.snapshot(), pasteboard.writeText(text) {
                    let count = pasteboard.changeCount
                    success = events.paste()
                    try? await Task.sleep(for: .milliseconds(250))
                    if pasteboard.changeCount == count { _ = pasteboard.restore(snapshot) }
                } else { success = false }
            case .accessibility: success = events.accessibilityInsert(text)
            case .unicode: success = events.typeUnicode(text)
            }
            if success {
                if let app { store.setMethod(method, for: app) }
                return .inserted(method)
            }
        }
        return pasteboard.writeText(text) ? .copiedToClipboard(.noMethodWorked) : .failed
    }
}
@MainActor public final class FakeTextInserter: TextInserter {
    public var result: InsertionResult
    public private(set) var inputs: [String] = []
    public init(result: InsertionResult = .inserted(.paste)) { self.result = result }
    public func insert(_ text: String) async -> InsertionResult { inputs.append(text); return result }
}
