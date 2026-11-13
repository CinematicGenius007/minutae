/// Physical transition delivered by Carbon for the registered chord.
public enum HotkeyPhase: Sendable, Hashable {
    case pressed
    case released
}

/// One press or release of the registered chord.
public struct HotkeyEvent: Sendable, Hashable {
    public var phase: HotkeyPhase
    public var chord: HotkeyChord

    public init(phase: HotkeyPhase, chord: HotkeyChord) {
        self.phase = phase
        self.chord = chord
    }
}

/// Dictation session boundary after hold or toggle interpretation.
public enum HotkeySessionEdge: Sendable, Hashable {
    case began
    case ended
}

/// Hold and toggle interpretation of press and release.
///
/// Hold begins on press and ends on release. A repeat press while held is ignored.
/// Toggle flips on each press and ignores release. `setMode` ends an active session
/// when the mode actually changes, so a hold cannot stay stuck after the user switches.
public struct HotkeySessionMachine: Sendable {
    public private(set) var mode: HotkeyMode
    public private(set) var isActive: Bool

    public init(mode: HotkeyMode) {
        self.mode = mode
        self.isActive = false
    }

    public mutating func setMode(_ mode: HotkeyMode) -> HotkeySessionEdge? {
        guard self.mode != mode else { return nil }
        self.mode = mode
        guard isActive else { return nil }
        isActive = false
        return .ended
    }

    public mutating func handle(_ phase: HotkeyPhase) -> HotkeySessionEdge? {
        switch mode {
        case .hold:
            return handleHold(phase)
        case .toggle:
            return handleToggle(phase)
        }
    }

    private mutating func handleHold(_ phase: HotkeyPhase) -> HotkeySessionEdge? {
        switch phase {
        case .pressed:
            guard !isActive else { return nil }
            isActive = true
            return .began
        case .released:
            guard isActive else { return nil }
            isActive = false
            return .ended
        }
    }

    private mutating func handleToggle(_ phase: HotkeyPhase) -> HotkeySessionEdge? {
        switch phase {
        case .pressed:
            isActive.toggle()
            return isActive ? .began : .ended
        case .released:
            return nil
        }
    }
}
