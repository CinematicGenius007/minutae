/// Why `register` did not arm a chord. `suggestedFallback` is always a different
/// chord the caller can offer.
public struct HotkeyRegistrationFailure: Error, Sendable, Hashable {
    public var chord: HotkeyChord
    public var reason: HotkeyRejection
    public var suggestedFallback: HotkeyChord

    public init(
        chord: HotkeyChord,
        reason: HotkeyRejection,
        suggestedFallback: HotkeyChord
    ) {
        self.chord = chord
        self.reason = reason
        self.suggestedFallback = suggestedFallback
    }

    public var message: String {
        let failed = chord.spoken
        let fallback = suggestedFallback.spoken
        switch reason {
        case .invalid(let problem):
            return
                "Could not register \(failed): \(problem.detail) Suggested fallback: \(fallback)."
        case .system(let status):
            return
                "Could not register \(failed): status \(status). Suggested fallback: \(fallback)."
        }
    }

    static func reporting(_ chord: HotkeyChord, reason: HotkeyRejection)
        -> HotkeyRegistrationFailure
    {
        HotkeyRegistrationFailure(
            chord: chord,
            reason: reason,
            suggestedFallback: HotkeyFallback.suggestion(after: chord)
        )
    }
}

public enum HotkeyRejection: Sendable, Hashable {
    case invalid(HotkeyChordProblem)
    /// Carbon `OSStatus` from `RegisterEventHotKey` or handler installation.
    /// `HotkeyCarbonStatus.alreadyRegistered` (-9878) means this process, or an
    /// exclusive owner, already has the chord.
    case system(Int32)
}

/// Global dictation hotkey. Press and release arrive on `events`. Hold and toggle
/// are interpreted onto `session`. Neither stream replays.
///
/// The Carbon implementation uses `RegisterEventHotKey` only. It does not install
/// an event tap or an `NSEvent` monitor, so it does not need Input Monitoring.
public protocol HotkeyService: Sendable {
    var events: AsyncStream<HotkeyEvent> { get }

    var session: AsyncStream<HotkeySessionEdge> { get }

    /// The armed registration, or nil when nothing is registered.
    var registration: HotkeyRegistration? { get }

    /// Arms `registration`, replacing any previous chord.
    ///
    /// On failure the previous registration stays armed. The error names the chord
    /// that failed and a suggested fallback. Re-registering the same chord and mode
    /// succeeds without a system call.
    func register(_ registration: HotkeyRegistration) throws

    /// Drops the armed chord. An active session ends.
    func unregister()
}
