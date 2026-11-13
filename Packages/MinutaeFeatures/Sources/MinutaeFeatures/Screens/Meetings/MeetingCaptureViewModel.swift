import Foundation
import MinutaeMeetings
import Observation

/// Coordinates capture actions initiated by the meeting prompt and recording pill.
@MainActor
@Observable
public final class MeetingCaptureViewModel {
    public private(set) var startedAt: Date?
    public private(set) var errorMessage: String?
    public var isPromptPresented = false
    private let recorder: MeetingRecorder

    public init(recorder: MeetingRecorder) {
        self.recorder = recorder
    }

    public func start() async {
        do {
            try await recorder.start()
            startedAt = Date()
            isPromptPresented = false
            errorMessage = nil
        } catch {
            errorMessage = String(describing: error)
        }
    }

    public func stop() async {
        do {
            try await recorder.stop()
            startedAt = nil
            errorMessage = nil
        } catch {
            errorMessage = String(describing: error)
        }
    }
}
