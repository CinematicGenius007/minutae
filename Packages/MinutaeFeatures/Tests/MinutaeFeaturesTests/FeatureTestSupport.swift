import Foundation
import MinutaeCore
import MinutaeDictation
import Testing
import UniformTypeIdentifiers

@testable import MinutaeFeatures

enum Fixtures {
    static let formatter = DictationFormatter(
        locale: Locale(identifier: "en_US"),
        timeZone: TimeZone(secondsFromGMT: 0) ?? .gmt
    )

    static let now: @Sendable () -> Date = { date("2026-11-13T16:00:00Z") }

    static func date(_ string: String) -> Date {
        DictationFormatter.date(string) ?? Date(timeIntervalSince1970: 0)
    }

    static func row(
        _ id: String,
        started: String = "2026-11-13T14:41:00Z",
        text: String = "Remind me to send Priya the mockups.",
        status: String = "done",
        audioPath: String? = nil,
        durationMs: Int? = 6_000
    ) -> Dictation {
        Dictation(
            id: id, started_at: started, duration_ms: durationMs,
            target_app_bundle_id: "com.tinyspeck.slackmacgap", target_app_name: "Slack",
            raw_text: text.lowercased(), final_text: text, language: "en",
            insert_method: status == "done" ? "pasteboard" : nil, status: status,
            audio_path: audioPath, error: nil, created_at: started, speech_model: "small")
    }

    static func temporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("minutae-features-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}

@MainActor
final class RecordingClipboard: ClipboardWriting {
    private(set) var copies: [String] = []
    func copy(_ text: String) { copies.append(text) }
}

@MainActor
final class ScriptedDestination: ExportDestinationPicking {
    var url: URL?
    private(set) var requests: [(name: String, type: UTType)] = []

    init(url: URL?) { self.url = url }

    func chooseDestination(suggestedName: String, contentType: UTType) async -> URL? {
        requests.append((suggestedName, contentType))
        return url
    }
}

@MainActor
final class FakePlayer: AudioPlaying {
    var duration: TimeInterval
    var currentTime: TimeInterval = 0
    var isPlaying = false
    var playSucceeds = true

    init(duration: TimeInterval = 6) { self.duration = duration }

    @discardableResult
    func play() -> Bool {
        isPlaying = playSucceeds
        return playSucceeds
    }

    func pause() { isPlaying = false }
}

actor FakeDictationControl: DictationControlling {
    private(set) var toggles = 0
    private var continuations: [AsyncStream<DictationControllerEvent>.Continuation] = []

    func events() -> AsyncStream<DictationControllerEvent> {
        let (stream, continuation) = AsyncStream<DictationControllerEvent>.makeStream()
        continuations.append(continuation)
        return stream
    }

    func toggleFromApp() { toggles += 1 }

    var subscriberCount: Int { continuations.count }

    func send(_ event: DictationControllerEvent) {
        for continuation in continuations { continuation.yield(event) }
    }

    func finish() {
        for continuation in continuations { continuation.finish() }
    }
}

/// A sleep that returns only when the test opens it.
actor ManualSleep {
    private var waiting: [CheckedContinuation<Void, Never>] = []

    func sleep() async {
        await withCheckedContinuation { waiting.append($0) }
    }

    var pending: Int { waiting.count }

    func wake() {
        let all = waiting
        waiting.removeAll()
        for continuation in all { continuation.resume() }
    }
}

func hudState(
    _ phase: DictationPhase,
    app: String? = "Slack",
    banner: String? = nil,
    outcome: DictationOutcome? = nil
) -> DictationHUDState {
    DictationHUDState(
        phase: phase, dictationID: "d", targetBundleID: nil, targetAppName: app, level: 0,
        banner: banner, outcome: outcome)
}

@MainActor
func eventually(
    _ label: String,
    timeout: Duration = .seconds(2),
    _ condition: () async -> Bool
) async -> Bool {
    let deadline = ContinuousClock().now.advanced(by: timeout)
    while ContinuousClock().now < deadline {
        if await condition() { return true }
        try? await Task.sleep(for: .milliseconds(5))
    }
    Issue.record("timed out: \(label)")
    return false
}
