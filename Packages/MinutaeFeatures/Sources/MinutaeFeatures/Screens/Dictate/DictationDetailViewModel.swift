import Foundation
import MinutaeCore
import MinutaeLibrary
import Observation
import UniformTypeIdentifiers

/// State for one dictation: metadata, playback, the editable text, copy, export, and delete.
///
/// Edits go to `edited_text` through `DictationArchiving.saveEdit`. The original transcript,
/// the inserted text, and the WAV are never written.
@MainActor
@Observable
public final class DictationDetailViewModel {
    public enum SaveState: Equatable, Sendable {
        case idle
        case pending
        case saved
        case failed(String)
    }

    public let id: String
    public private(set) var dictation: Dictation?
    public private(set) var hasLoaded = false
    public private(set) var isDeleted = false
    public private(set) var draft = ""
    public private(set) var saveState: SaveState = .idle
    /// Short confirmation such as "Copied" or "Exported dictation.md".
    public private(set) var notice: String?
    public private(set) var errorMessage: String?
    public var isConfirmingDelete = false
    public let playback: AudioPlaybackModel

    @ObservationIgnored private let archive: any DictationArchiving
    @ObservationIgnored private let clipboard: any ClipboardWriting
    @ObservationIgnored private let destination: any ExportDestinationPicking
    @ObservationIgnored private let formatter: DictationFormatter
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private let saveDelay: Duration
    @ObservationIgnored private var pendingSave: Task<Void, Never>?
    @ObservationIgnored private var hasUnsavedDraft = false

    public init(
        id: String,
        archive: any DictationArchiving,
        clipboard: any ClipboardWriting,
        destination: any ExportDestinationPicking,
        makePlayer: @escaping AudioPlayerFactory,
        formatter: DictationFormatter = DictationFormatter(),
        now: @escaping @Sendable () -> Date = { Date() },
        saveDelay: Duration = .milliseconds(600)
    ) {
        self.id = id
        self.archive = archive
        self.clipboard = clipboard
        self.destination = destination
        self.formatter = formatter
        self.now = now
        self.saveDelay = saveDelay
        playback = AudioPlaybackModel(makePlayer: makePlayer)
    }

    public convenience init(id: String, dependencies: DictateDependencies) {
        self.init(
            id: id,
            archive: dependencies.archive,
            clipboard: dependencies.clipboard,
            destination: dependencies.exportDestination,
            makePlayer: dependencies.makePlayer,
            formatter: dependencies.formatter,
            now: dependencies.now
        )
    }

    public var title: String {
        dictation.map(formatter.detailTitle) ?? "Dictation"
    }

    public var meta: String {
        dictation.map { formatter.detailMeta($0, now: now()) } ?? ""
    }

    public var timeLabel: String {
        "\(formatter.clock(seconds: playback.currentTime)) / "
            + formatter.clock(seconds: playback.duration)
    }

    /// Rows still owned by the controller cannot be edited or deleted.
    public var isEditable: Bool { dictation.map { !$0.isInProgress } ?? false }

    public var canExportAudio: Bool { playback.isAvailable }

    public var isEdited: Bool { dictation?.edited_text != nil }

    /// Observes the row until the calling task is cancelled.
    public func run() async {
        do {
            for try await row in archive.observe(id: id) {
                apply(row: row)
            }
        } catch {
            errorMessage = "Couldn't read this dictation. \(error.localizedDescription)"
        }
    }

    /// Called by the text editor. Saves after a short pause in typing.
    public func updateDraft(_ text: String) {
        guard text != draft else { return }
        draft = text
        guard isEditable else { return }
        hasUnsavedDraft = true
        saveState = .pending
        pendingSave?.cancel()
        let delay = saveDelay
        pendingSave = Task { [weak self] in
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            self?.flush()
        }
    }

    /// Saves a pending edit now. Called before copy, export, and when the screen goes away.
    public func flush() {
        pendingSave?.cancel()
        pendingSave = nil
        guard hasUnsavedDraft, isEditable else { return }
        do {
            let saved = try archive.saveEdit(id: id, text: draft)
            hasUnsavedDraft = false
            saveState = .saved
            if let saved { dictation = saved }
        } catch {
            saveState = .failed("Couldn't save the edit. \(error.localizedDescription)")
        }
    }

    public func copyText() {
        flush()
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        clipboard.copy(text)
        notice = "Copied"
    }

    public func exportMarkdown() async {
        flush()
        guard var row = dictation else { return }
        if hasUnsavedDraft { row.edited_text = draft }
        let name = DictationMarkdown.suggestedFileName(
            for: row, extension: "md", timeZone: formatter.timeZone)
        guard
            let url = await destination.chooseDestination(
                suggestedName: name, contentType: Self.markdownType)
        else { return }
        do {
            try Data(DictationMarkdown.render(row).utf8).write(to: url, options: .atomic)
            notice = "Exported \(url.lastPathComponent)"
            errorMessage = nil
        } catch {
            errorMessage = "Couldn't export the text. \(error.localizedDescription)"
        }
    }

    /// Copies the WAV to a place the person picks. The original stays where it is.
    public func exportAudio() async {
        guard let row = dictation, let path = row.audio_path, canExportAudio else { return }
        let name = DictationMarkdown.suggestedFileName(
            for: row, extension: "wav", timeZone: formatter.timeZone)
        guard let url = await destination.chooseDestination(suggestedName: name, contentType: .wav)
        else { return }
        let source = URL(fileURLWithPath: path)
        guard url.standardizedFileURL != source.standardizedFileURL else {
            errorMessage = "Pick a different place. That is the original audio file."
            return
        }
        do {
            let manager = FileManager.default
            if manager.fileExists(atPath: url.path) {
                try manager.removeItem(at: url)
            }
            try manager.copyItem(at: source, to: url)
            notice = "Exported \(url.lastPathComponent)"
            errorMessage = nil
        } catch {
            errorMessage = "Couldn't export the audio. \(error.localizedDescription)"
        }
    }

    public func requestDelete() {
        guard isEditable else { return }
        isConfirmingDelete = true
    }

    /// Deletes the row and its WAV. Returns true when the row is gone.
    @discardableResult
    public func delete() -> Bool {
        isConfirmingDelete = false
        pendingSave?.cancel()
        hasUnsavedDraft = false
        playback.stop()
        do {
            let result = try archive.delete(id: id)
            isDeleted = true
            errorMessage = DictateViewModel.audioWarning(result)
            return true
        } catch DictationArchiveError.notFound {
            isDeleted = true
            return true
        } catch {
            errorMessage = "Couldn't delete this dictation. \(error.localizedDescription)"
            return false
        }
    }

    public func dismissNotice() {
        notice = nil
    }

    func apply(row: Dictation?) {
        hasLoaded = true
        dictation = row
        guard let row else {
            isDeleted = true
            playback.stop()
            return
        }
        if !hasUnsavedDraft {
            draft = row.displayText
        }
        let fallback = row.duration_ms.map { TimeInterval($0) / 1_000 }
        playback.load(path: row.audio_path, fallbackDuration: fallback)
    }

    static let markdownType = UTType(filenameExtension: "md") ?? .plainText
}
