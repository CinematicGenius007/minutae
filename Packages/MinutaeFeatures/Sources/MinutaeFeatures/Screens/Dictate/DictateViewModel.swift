import Foundation
import MinutaeCore
import MinutaeDictation
import MinutaeLibrary
import Observation

/// One row in RECENT.
public struct DictationRowItem: Identifiable, Equatable, Sendable {
    public var id: String
    public var text: String
    public var meta: String
    /// True when `text` is a status line ("Transcribing…") rather than dictated words.
    public var isPlaceholder: Bool
    public var canCopy: Bool

    public init(id: String, text: String, meta: String, isPlaceholder: Bool, canCopy: Bool) {
        self.id = id
        self.text = text
        self.meta = meta
        self.isPlaceholder = isPlaceholder
        self.canCopy = canCopy
    }

    static func make(_ row: Dictation, formatter: DictationFormatter, now: Date) -> Self {
        let words = row.displayText.trimmingCharacters(in: .whitespacesAndNewlines)
        let meta = formatter.rowMeta(row, now: now)
        if !words.isEmpty {
            return Self(id: row.id, text: words, meta: meta, isPlaceholder: false, canCopy: true)
        }
        return Self(
            id: row.id, text: placeholder(row), meta: meta, isPlaceholder: true, canCopy: false)
    }

    private static func placeholder(_ row: Dictation) -> String {
        switch row.status {
        case "listening": "Listening…"
        case "processing", "inserting": "Transcribing…"
        case "failed": "Didn't finish. The audio is kept."
        case "cancelled": "Cancelled."
        default: "Nothing heard."
        }
    }
}

/// State for the Dictate screen: the mic, the RECENT list, copy, and Clear all.
@MainActor
@Observable
public final class DictateViewModel {
    public static let recentLimit = 50

    public private(set) var rows: [DictationRowItem] = []
    public private(set) var phase: DictationPhase = .idle
    public private(set) var hasLoaded = false
    /// Row whose text was copied last. Cleared after a short delay.
    public private(set) var copiedID: String?
    public private(set) var errorMessage: String?
    public var isConfirmingClearAll = false

    @ObservationIgnored private let archive: any DictationArchiving
    @ObservationIgnored private let dictation: (any DictationControlling)?
    @ObservationIgnored private let clipboard: any ClipboardWriting
    @ObservationIgnored private let formatter: DictationFormatter
    @ObservationIgnored private let now: @Sendable () -> Date
    @ObservationIgnored private var stored: [Dictation] = []
    @ObservationIgnored private var copyReset: Task<Void, Never>?
    @ObservationIgnored private let copyFeedback: Duration

    public init(
        archive: any DictationArchiving,
        dictation: (any DictationControlling)?,
        clipboard: any ClipboardWriting,
        formatter: DictationFormatter = DictationFormatter(),
        now: @escaping @Sendable () -> Date = { Date() },
        copyFeedback: Duration = .milliseconds(1200)
    ) {
        self.archive = archive
        self.dictation = dictation
        self.clipboard = clipboard
        self.formatter = formatter
        self.now = now
        self.copyFeedback = copyFeedback
    }

    public convenience init(dependencies: DictateDependencies) {
        self.init(
            archive: dependencies.archive,
            dictation: dependencies.dictation,
            clipboard: dependencies.clipboard,
            formatter: dependencies.formatter,
            now: dependencies.now
        )
    }

    public var isListening: Bool { phase == .listening }

    /// True while transcribing or inserting, when the mic button cannot start another run.
    public var isBusy: Bool { phase == .processing || phase == .inserting }

    public var canClearAll: Bool { stored.contains { !$0.isInProgress } }

    /// Observes the archive and the controller until the calling task is cancelled.
    public func run() async {
        await withTaskGroup(of: Void.self) { group in
            group.addTask { await self.observeRows() }
            group.addTask { await self.observeDictation() }
        }
    }

    public func toggleDictation() async {
        guard let dictation, !isBusy else { return }
        await dictation.toggleFromApp()
    }

    public func copy(id: String) {
        guard let row = stored.first(where: { $0.id == id }) else { return }
        let text = row.displayText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        clipboard.copy(text)
        copiedID = id
        copyReset?.cancel()
        let delay = copyFeedback
        copyReset = Task { [weak self] in
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled else { return }
            self?.copiedID = nil
        }
    }

    public func requestClearAll() {
        guard canClearAll else { return }
        isConfirmingClearAll = true
    }

    /// Deletes every finished dictation and its audio. Rows still in progress stay.
    public func clearAll() {
        isConfirmingClearAll = false
        do {
            let result = try archive.deleteAll()
            errorMessage = Self.audioWarning(result)
        } catch {
            errorMessage = "Couldn't clear the dictations. \(error.localizedDescription)"
        }
    }

    public func dismissError() {
        errorMessage = nil
    }

    func apply(rows newRows: [Dictation]) {
        stored = newRows
        let current = now()
        rows = newRows.map { DictationRowItem.make($0, formatter: formatter, now: current) }
        hasLoaded = true
    }

    func apply(event: DictationControllerEvent) {
        guard case .state(let state) = event else { return }
        phase = state.phase == .error ? .idle : state.phase
    }

    private func observeRows() async {
        do {
            for try await rows in archive.observeRecent(limit: Self.recentLimit) {
                apply(rows: rows)
            }
        } catch {
            errorMessage = "Couldn't read the dictations. \(error.localizedDescription)"
        }
    }

    private func observeDictation() async {
        guard let dictation else { return }
        let events = await dictation.events()
        for await event in events {
            apply(event: event)
        }
    }

    static func audioWarning(_ result: DictationDeletion) -> String? {
        guard !result.audioFailures.isEmpty else { return nil }
        let count = result.audioFailures.count
        let noun = count == 1 ? "audio file" : "audio files"
        return "\(count) \(noun) could not be removed from disk."
    }
}
