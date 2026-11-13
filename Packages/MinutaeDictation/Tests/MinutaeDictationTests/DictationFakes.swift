import Foundation
import MinutaeAudio
import MinutaeSpeech
import Testing

@testable import MinutaeDictation

struct DictationHarnessPlan {
    var mode: HotkeyMode = .hold
    var samples: [Float] = Array(repeating: 0.25, count: 16_000)
    var target = DictationTarget(bundleIdentifier: "com.apple.TextEdit", name: "TextEdit")
    var speechFails = false
    var cleanFails = false
    var cleaned: String? = "Hello world."
    var insertion: InsertionResult = .inserted(method: .paste, confirmed: true)
    var startError: Error?
    var stopError: Error?
    var speechOpened = false
    var cleanOpened = false
    var insertOpened = false
}

struct SampleFailure: Error, CustomStringConvertible, Sendable {
    var message: String
    var description: String { message }
}

struct CleanError: Error, CustomStringConvertible, Sendable {
    var description: String { "clean-broke" }
}

final class Gate: @unchecked Sendable {
    private let lock = NSLock()
    private var opened: Bool
    private var entered = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(opened: Bool = false) {
        self.opened = opened
    }

    func open() {
        let pending: [CheckedContinuation<Void, Never>] = lock.withLock {
            opened = true
            let pending = waiters
            waiters.removeAll()
            return pending
        }
        for waiter in pending {
            waiter.resume()
        }
    }

    func wait() async {
        await withCheckedContinuation { continuation in
            let resumeNow = lock.withLock { () -> Bool in
                entered = true
                if opened { return true }
                waiters.append(continuation)
                return false
            }
            if resumeNow {
                continuation.resume()
            }
        }
    }

    var didEnter: Bool {
        lock.withLock { entered }
    }

    func waitEntered() async -> Bool {
        let deadline = ContinuousClock().now.advanced(by: .seconds(3))
        while ContinuousClock().now < deadline {
            if didEnter { return true }
            try? await Task.sleep(for: .milliseconds(5))
        }
        Issue.record("timed out waiting for a collaborator")
        return false
    }
}

final class ScriptedCapture: AudioCapture, @unchecked Sendable {
    let buffer = CaptureBuffer()
    var samples: [Float]
    var startError: Error?
    var stopError: Error?

    init(samples: [Float]) {
        self.samples = samples
    }

    func events() -> AsyncStream<CaptureEvent> { buffer.events() }

    func subscribe() -> AsyncStream<[Float]> { buffer.subscribe() }

    func start(retainFullCapture: Bool) throws {
        if let startError { throw startError }
        try buffer.begin(retainFullCapture: retainFullCapture)
        if !samples.isEmpty {
            try buffer.push(samples)
        }
    }

    func stop() throws -> CaptureResult {
        if let stopError { throw stopError }
        return try buffer.end()
    }
}

final class ScriptedFrontmost: FrontmostAppSource, @unchecked Sendable {
    var target: DictationTarget

    init(_ target: DictationTarget) {
        self.target = target
    }

    func frontmostApp() async -> DictationTarget { target }
}

final class SequenceID: @unchecked Sendable {
    private let lock = NSLock()
    private var value = 0

    func next() -> String {
        lock.withLock {
            value += 1
            return "dict-\(value)"
        }
    }
}

actor GatedSpeech: SpeechEngine {
    private let gate: Gate
    private let transcript: MinutaeSpeech.Transcript
    private let fails: Bool
    private(set) var sampleCount = 0

    init(gate: Gate, transcript: MinutaeSpeech.Transcript, fails: Bool) {
        self.gate = gate
        self.transcript = transcript
        self.fails = fails
    }

    var isLoaded: Bool { true }

    func load(model: URL) async throws {}

    func unload() async {}

    func transcribe(
        samples: [Float],
        options: TranscriptionOptions
    ) async throws -> MinutaeSpeech.Transcript {
        sampleCount = samples.count
        await gate.wait()
        if fails { throw SpeechError.notLoaded }
        return transcript
    }
}

struct GatedCleaner: DictationCleaning {
    var gate: Gate
    var replacement: String?
    var fails = false

    func process(text: String, bundleID: String?) async throws -> String {
        await gate.wait()
        if fails { throw CleanError() }
        return replacement ?? text
    }
}

@MainActor
final class GatedInserter: TextInserter {
    let gate: Gate
    var result: InsertionResult
    private(set) var inputs: [String] = []

    init(gate: Gate, result: InsertionResult) {
        self.gate = gate
        self.result = result
    }

    func insert(_ text: String) async -> InsertionResult {
        inputs.append(text)
        await gate.wait()
        return result
    }
}

final class EventLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [DictationControllerEvent] = []

    init(_ stream: AsyncStream<DictationControllerEvent>) {
        Task {
            for await event in stream {
                self.lock.withLock { self.stored.append(event) }
            }
        }
    }

    func snapshot() -> [DictationControllerEvent] {
        lock.withLock { stored }
    }

    func ignored() -> Int {
        snapshot().reduce(into: 0) { count, event in
            if case .ignoredWhileBusy = event { count += 1 }
        }
    }
}

func collapsedPhases(_ events: [DictationControllerEvent]) -> [DictationPhase] {
    events.reduce(into: [DictationPhase]()) { phases, event in
        guard case .state(let state) = event else { return }
        if phases.last != state.phase {
            phases.append(state.phase)
        }
    }
}
