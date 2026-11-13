import Foundation
import MinutaeCore
import UniformTypeIdentifiers

/// Sample rows from the Paper artboards, for previews.
@MainActor
enum DictatePreview {
    static let first = sample(
        "preview-1",
        started: "2026-11-13T14:41:00Z",
        text: "Remind me to send Priya the onboarding mockups before Friday's review.",
        durationMs: 6_000
    )

    static let second = sample(
        "preview-2",
        started: "2026-11-13T13:12:00Z",
        text: "Thanks for the quick turnaround, I'll take a look this afternoon and send comments.",
        durationMs: 8_000
    )

    static let rows = [first, second]

    static let formatter = DictationFormatter(
        locale: Locale(identifier: "en_US"),
        timeZone: TimeZone(secondsFromGMT: 0) ?? .gmt
    )

    static let now: @Sendable () -> Date = {
        DictationFormatter.date("2026-11-13T16:00:00Z") ?? Date()
    }

    static func dependencies() -> DictateDependencies {
        DictateDependencies(
            archive: InMemoryDictationArchive(rows),
            dictation: nil,
            clipboard: PreviewClipboard(),
            exportDestination: PreviewDestination(),
            makePlayer: { _ in PreviewPlayer() },
            formatter: formatter,
            now: now
        )
    }

    static func listModel() -> DictateViewModel {
        DictateViewModel(dependencies: dependencies())
    }

    static func detailModel() -> DictationDetailViewModel {
        DictationDetailViewModel(id: "preview-1", dependencies: dependencies())
    }

    static func sample(
        _ id: String,
        started: String,
        text: String,
        durationMs: Int,
        status: String = "done"
    ) -> Dictation {
        Dictation(
            id: id, started_at: started, duration_ms: durationMs,
            target_app_bundle_id: "com.tinyspeck.slackmacgap", target_app_name: "Slack",
            raw_text: text, final_text: text, language: "en", insert_method: "pasteboard",
            status: status, audio_path: "/dev/null", error: nil, created_at: started,
            speech_model: "small")
    }
}

@MainActor
final class PreviewClipboard: ClipboardWriting {
    func copy(_ text: String) {}
}

@MainActor
final class PreviewDestination: ExportDestinationPicking {
    func chooseDestination(suggestedName: String, contentType: UTType) async -> URL? { nil }
}

@MainActor
final class PreviewPlayer: AudioPlaying {
    var duration: TimeInterval = 6
    var currentTime: TimeInterval = 2.1
    var isPlaying = false

    @discardableResult
    func play() -> Bool {
        isPlaying = true
        return true
    }

    func pause() { isPlaying = false }
}
