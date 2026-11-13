import Foundation
import MinutaeMeetings
import Observation

/// Watches the microphone and raises a "record this call?" prompt. Drives the sidebar pill.
@MainActor @Observable
public final class MeetingDetectionModel {
    public enum Phase: Equatable, Sendable {
        case off
        case listening
        case prompting
        case recording
    }

    public private(set) var phase: Phase = .listening
    private var detector: MicActivityDetector
    private let probe: @MainActor () -> Bool
    private let isEnabled: @MainActor () -> Bool
    private let isRecording: @MainActor () -> Bool
    private let isCapturingOtherwise: @MainActor () -> Bool
    private let interval: Duration

    public init(
        interval: Duration = .seconds(2),
        sustain: Int = 3,
        probe: @escaping @MainActor () -> Bool = { SystemMicProbe.isInUse() },
        isEnabled: @escaping @MainActor () -> Bool,
        isRecording: @escaping @MainActor () -> Bool,
        isCapturingOtherwise: @escaping @MainActor () -> Bool = { false }
    ) {
        self.interval = interval
        detector = MicActivityDetector(sustain: sustain)
        self.probe = probe
        self.isEnabled = isEnabled
        self.isRecording = isRecording
        self.isCapturingOtherwise = isCapturingOtherwise
    }

    /// One poll. Public so tests drive it without timers.
    public func tick() {
        if isRecording() {
            phase = .recording
            detector.disarm()
            return
        }
        guard isEnabled() else {
            phase = .off
            return
        }
        let raised = detector.ingest(micInUse: probe(), ownCapture: isCapturingOtherwise())
        if raised {
            phase = .prompting
        } else if phase != .prompting {
            phase = .listening
        }
    }

    public func run() async {
        while !Task.isCancelled {
            tick()
            try? await Task.sleep(for: interval)
        }
    }

    /// "Not now": stay quiet until the mic goes idle.
    public func dismissPrompt() {
        detector.disarm()
        phase = isEnabled() ? .listening : .off
    }
}
