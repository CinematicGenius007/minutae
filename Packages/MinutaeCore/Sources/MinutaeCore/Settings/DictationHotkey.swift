/// How the dictation chord starts and stops capture.
public enum DictationHotkeyMode: String, Codable, Hashable, Sendable {
    case hold
    case toggle
}

/// Modifier keys stored independently of Carbon or `NSEvent` bit masks.
public struct HotkeyModifier: OptionSet, Hashable, Sendable, Codable {
    public let rawValue: UInt

    public init(rawValue: UInt) {
        self.rawValue = rawValue
    }

    public static let command = HotkeyModifier(rawValue: 1 << 0)
    public static let option = HotkeyModifier(rawValue: 1 << 1)
    public static let shift = HotkeyModifier(rawValue: 1 << 2)
    public static let control = HotkeyModifier(rawValue: 1 << 3)
}

/// A user-changeable key chord. `keyCode` is a macOS virtual key code (`kVK_*`).
public struct HotkeyChord: Hashable, Sendable, Codable {
    /// Virtual key code for Space (`kVK_Space` in Events.h).
    public static let spaceKeyCode: UInt16 = 49

    public var keyCode: UInt16
    public var modifiers: HotkeyModifier

    public init(keyCode: UInt16, modifiers: HotkeyModifier) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    public static let optionSpace = HotkeyChord(keyCode: spaceKeyCode, modifiers: .option)
}

/// Dictation hotkey: a mode plus a chord. The default is hold Option-Space.
public struct DictationHotkey: Hashable, Sendable, Codable {
    public var mode: DictationHotkeyMode
    public var chord: HotkeyChord

    public init(mode: DictationHotkeyMode, chord: HotkeyChord) {
        self.mode = mode
        self.chord = chord
    }

    public static let `default` = DictationHotkey(mode: .hold, chord: .optionSpace)
}
