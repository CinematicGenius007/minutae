import AppKit
import Foundation
import MinutaeAudio
import MinutaeCore
import MinutaeSpeech
import os

/// Published dictation phase. `error` is delivered once, then the controller
/// returns to `idle`.
public enum DictationPhase: String, Sendable, Equatable {
    case idle
    case listening
    case processing
    case inserting
    case error
}

/// How the last dictation ended. Set on the `error` and `idle` events that close a run and
/// cleared when the next run starts listening.
public enum DictationOutcome: String, Sendable, Equatable {
    /// Text went into the target app by paste, Accessibility, or typing.
    case inserted
    /// Text is on the clipboard because insertion was not possible.
    case copiedToClipboard
    /// The transcript was empty, so nothing was inserted.
    case nothingHeard
    case cancelled
    case failed
}

/// Snapshot the HUD can render without reading the database.
public struct DictationHUDState: Sendable, Equatable {
    public var phase: DictationPhase
    public var dictationID: String?
    public var targetBundleID: String?
    public var targetAppName: String?
    public var level: Float
    public var banner: String?
    public var outcome: DictationOutcome?

    public init(
        phase: DictationPhase,
        dictationID: String?,
        targetBundleID: String?,
        targetAppName: String?,
        level: Float,
        banner: String?,
        outcome: DictationOutcome? = nil
    ) {
        self.phase = phase
        self.dictationID = dictationID
        self.targetBundleID = targetBundleID
        self.targetAppName = targetAppName
        self.level = level
        self.banner = banner
        self.outcome = outcome
    }
}

public enum DictationControllerEvent: Sendable, Equatable {
    case state(DictationHUDState)
    /// A press arrived that neither starts nor stops the active dictation.
    /// It is dropped. It is not queued.
    case ignoredWhileBusy
}

/// Frontmost app captured when the dictation starts.
public struct DictationTarget: Sendable, Equatable {
    public var bundleIdentifier: String?
    public var name: String?

    public init(bundleIdentifier: String? = nil, name: String? = nil) {
        self.bundleIdentifier = bundleIdentifier
        self.name = name
    }
}

public protocol FrontmostAppSource: Sendable {
    func frontmostApp() async -> DictationTarget
}

public struct WorkspaceFrontmostAppSource: FrontmostAppSource {
    public init() {}

    public func frontmostApp() async -> DictationTarget {
        await MainActor.run {
            let app = NSWorkspace.shared.frontmostApplication
            return DictationTarget(
                bundleIdentifier: app?.bundleIdentifier,
                name: app?.localizedName
            )
        }
    }
}

/// Esc and any other cancel gesture feed this stream. The controller does not
/// install a global key monitor.
public protocol DictationCancelSource: Sendable {
    var cancels: AsyncStream<Void> { get }
}

public final class DictationCancelSignal: DictationCancelSource, Sendable {
    public let cancels: AsyncStream<Void>
    private let continuation: AsyncStream<Void>.Continuation

    public init() {
        let pair = AsyncStream<Void>.makeStream()
        cancels = pair.stream
        continuation = pair.continuation
    }

    public func send() {
        continuation.yield(())
    }
}

/// Cleaning step. `PostProcessor` does not throw; this protocol does, so a
/// failed cleaner still lands in the failure path.
public protocol DictationCleaning: Sendable {
    func process(text: String, bundleID: String?) async throws -> String
}

public struct PostProcessorClient: DictationCleaning {
    private let processor: any PostProcessor

    public init(_ processor: any PostProcessor) {
        self.processor = processor
    }

    public func process(text: String, bundleID: String?) async throws -> String {
        await processor.process(text: text, bundleID: bundleID)
    }
}

public protocol DictationInserting: Sendable {
    func insert(_ text: String) async -> InsertionResult
}

/// `TextInserter` is main-actor isolated. The adapter is the value the
/// controller stores.
public struct TextInserterClient: DictationInserting, @unchecked Sendable {
    private let inserter: any TextInserter

    @MainActor public init(_ inserter: any TextInserter) {
        self.inserter = inserter
    }

    public func insert(_ text: String) async -> InsertionResult {
        await inserter.insert(text)
    }
}

public protocol DictationStore: Sendable {
    func insert(_ row: Dictation) throws
    func update(_ row: Dictation) throws
}

public struct DatabaseDictationStore: DictationStore {
    private let database: MinutaeDatabase

    public init(database: MinutaeDatabase) {
        self.database = database
    }

    public func insert(_ row: Dictation) throws {
        try database.pool.write { database in
            try row.insert(database)
        }
    }

    public func update(_ row: Dictation) throws {
        try database.pool.write { database in
            try row.update(database)
        }
    }
}

public struct DictationControllerOptions: Sendable {
    public var dictationsDirectory: URL
    public var now: @Sendable () -> Date
    public var makeID: @Sendable () -> String
    /// Speech model name stored on each new row, such as `base`. Nil leaves it unknown.
    public var speechModelName: @Sendable () -> String?

    public init(
        dictationsDirectory: URL,
        now: @escaping @Sendable () -> Date = { Date() },
        makeID: @escaping @Sendable () -> String = { UUID().uuidString.lowercased() },
        speechModelName: @escaping @Sendable () -> String? = { nil }
    ) {
        self.dictationsDirectory = dictationsDirectory
        self.now = now
        self.makeID = makeID
        self.speechModelName = speechModelName
    }
}

/// `capture` is not `Sendable`. The controller owns it after `init`.
public struct DictationControllerClients: @unchecked Sendable {
    public var hotkey: any HotkeyService
    public var capture: any AudioCapture
    public var speech: any SpeechEngine
    public var cleaning: any DictationCleaning
    public var inserting: any DictationInserting

    public init(
        hotkey: any HotkeyService,
        capture: any AudioCapture,
        speech: any SpeechEngine,
        cleaning: any DictationCleaning,
        inserting: any DictationInserting
    ) {
        self.hotkey = hotkey
        self.capture = capture
        self.speech = speech
        self.cleaning = cleaning
        self.inserting = inserting
    }
}

public struct DictationControllerEnvironment {
    public var store: any DictationStore
    public var frontmost: any FrontmostAppSource
    public var cancels: any DictationCancelSource
    public var options: DictationControllerOptions

    public init(
        store: any DictationStore,
        frontmost: any FrontmostAppSource,
        cancels: any DictationCancelSource,
        options: DictationControllerOptions
    ) {
        self.store = store
        self.frontmost = frontmost
        self.cancels = cancels
        self.options = options
    }
}

enum DictationRowStatus {
    static let listening = "listening"
    static let processing = "processing"
    static let inserting = "inserting"
    static let done = "done"
    static let failed = "failed"
    static let cancelled = "cancelled"
}

enum DictationAudioFile {
    /// `AudioCapture` publishes 16 kHz mono Float32.
    static let sampleRate = 16_000

    static func durationMilliseconds(sampleCount: Int) -> Int {
        guard sampleCount > 0 else { return 0 }
        return sampleCount * 1_000 / sampleRate
    }

    static func monthFolder(_ date: Date) -> String {
        var calendar = Calendar(identifier: .gregorian)
        if let utc = TimeZone(secondsFromGMT: 0) {
            calendar.timeZone = utc
        }
        let parts = calendar.dateComponents([.year, .month], from: date)
        return String(format: "%04d-%02d", parts.year ?? 0, parts.month ?? 0)
    }

    static func timestamp(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter.string(from: date)
    }

    static func write(samples: [Float], directory: URL, started: Date, id: String) throws -> URL {
        let folder = directory.appending(
            path: monthFolder(started),
            directoryHint: .isDirectory
        )
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let url = folder.appending(path: "\(id).wav")
        let writer = try WAVWriter(url: url)
        do {
            if !samples.isEmpty {
                try writer.append(samples)
            }
            try writer.close()
        } catch {
            try? writer.close()
            throw error
        }
        return url
    }
}

func dictationStageDetail(_ error: Error) -> String {
    if let failure = error as? CaptureFailure {
        return failure.message
    }
    return String(describing: error)
}

func dictationInsertMethod(_ method: InsertionMethod) -> String {
    switch method {
    case .paste:
        "pasteboard"
    case .accessibility:
        "accessibility"
    case .unicode:
        "typing"
    }
}

func dictationClipboardBanner(_ reason: ClipboardReason) -> String {
    switch reason {
    case .accessibilityNotTrusted:
        "Copied to the clipboard because Accessibility is not trusted."
    case .noMethodWorked:
        "Copied to the clipboard."
    }
}

func joinedTranscript(_ transcript: MinutaeSpeech.Transcript) -> String {
    transcript.segments.map(\.text).joined()
}

final class DictationEventHub: Sendable {
    private let subscribers = OSAllocatedUnfairLock(
        initialState: [UUID: AsyncStream<DictationControllerEvent>.Continuation]()
    )

    func stream() -> AsyncStream<DictationControllerEvent> {
        AsyncStream { continuation in
            let id = UUID()
            subscribers.withLock { $0[id] = continuation }
            continuation.onTermination = { [subscribers] _ in
                subscribers.withLock { $0[id] = nil }
            }
        }
    }

    func emit(_ event: DictationControllerEvent) {
        let continuations = subscribers.withLock { Array($0.values) }
        for continuation in continuations {
            continuation.yield(event)
        }
    }

    deinit {
        let continuations = subscribers.withLock { state in
            let values = Array(state.values)
            state.removeAll()
            return values
        }
        for continuation in continuations {
            continuation.finish()
        }
    }
}

final class OwnedCapture: @unchecked Sendable {
    let capture: any AudioCapture
    init(_ capture: any AudioCapture) {
        self.capture = capture
    }
}
