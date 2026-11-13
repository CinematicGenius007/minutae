import AppKit
import Foundation
import MinutaeCore
import MinutaeDictation
import MinutaeLibrary
import UniformTypeIdentifiers

/// The part of `DictationController` the Dictate screens use.
public protocol DictationControlling: Sendable {
    func events() async -> AsyncStream<DictationControllerEvent>
    func toggleFromApp() async
}

extension DictationController: DictationControlling {}

@MainActor
public protocol ClipboardWriting: AnyObject {
    func copy(_ text: String)
}

@MainActor
public final class SystemClipboard: ClipboardWriting {
    private let board: NSPasteboard

    public init(_ board: NSPasteboard = .general) {
        self.board = board
    }

    public func copy(_ text: String) {
        board.clearContents()
        board.setString(text, forType: .string)
    }
}

/// Asks where an export goes. Nil means the person cancelled.
@MainActor
public protocol ExportDestinationPicking: AnyObject {
    func chooseDestination(suggestedName: String, contentType: UTType) async -> URL?
}

@MainActor
public final class SavePanelDestinationPicker: ExportDestinationPicking {
    public init() {}

    public func chooseDestination(suggestedName: String, contentType: UTType) async -> URL? {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = suggestedName
        panel.allowedContentTypes = [contentType]
        panel.canCreateDirectories = true
        panel.isExtensionHidden = false
        return panel.runModal() == .OK ? panel.url : nil
    }
}

/// Everything `DictateRootView` needs. The shell builds it once with `live(...)`.
@MainActor
public struct DictateDependencies {
    public var archive: any DictationArchiving
    public var dictation: (any DictationControlling)?
    public var clipboard: any ClipboardWriting
    public var exportDestination: any ExportDestinationPicking
    public var makePlayer: AudioPlayerFactory
    public var formatter: DictationFormatter
    public var now: @Sendable () -> Date
    /// Shown in the hint, such as "⌥ Space".
    public var hotkeyLabel: String

    public init(
        archive: any DictationArchiving,
        dictation: (any DictationControlling)?,
        clipboard: any ClipboardWriting = SystemClipboard(),
        exportDestination: any ExportDestinationPicking = SavePanelDestinationPicker(),
        makePlayer: @escaping AudioPlayerFactory = SystemAudioPlayer.make,
        formatter: DictationFormatter = DictationFormatter(),
        now: @escaping @Sendable () -> Date = { Date() },
        hotkeyLabel: String = "⌥ Space"
    ) {
        self.archive = archive
        self.dictation = dictation
        self.clipboard = clipboard
        self.exportDestination = exportDestination
        self.makePlayer = makePlayer
        self.formatter = formatter
        self.now = now
        self.hotkeyLabel = hotkeyLabel
    }

    /// Production wiring: the app database, the dictations folder, and the running controller.
    public static func live(
        database: MinutaeDatabase,
        dictationsDirectory: URL,
        dictation: DictationController
    ) -> DictateDependencies {
        DictateDependencies(
            archive: DatabaseDictationArchive(database: database, audioRoot: dictationsDirectory),
            dictation: dictation
        )
    }
}
