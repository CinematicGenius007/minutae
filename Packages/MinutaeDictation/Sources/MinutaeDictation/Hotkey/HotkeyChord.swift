/// Modifier keys stored independently of Carbon and `NSEvent` bit masks.
///
/// The bits match the settings chord in MinutaeCore (`command` 1<<0, `option` 1<<1,
/// `shift` 1<<2, `control` 1<<3) so the app can copy fields. This package does not
/// import MinutaeCore. The fn key is not a modifier here: ARCHITECTURE §9 does not
/// offer it, because it needs Input Monitoring.
public struct HotkeyModifier: OptionSet, Hashable, Sendable, Codable {
    public let rawValue: UInt

    public init(rawValue: UInt) {
        self.rawValue = rawValue
    }

    public static let command = HotkeyModifier(rawValue: 1 << 0)
    public static let option = HotkeyModifier(rawValue: 1 << 1)
    public static let shift = HotkeyModifier(rawValue: 1 << 2)
    public static let control = HotkeyModifier(rawValue: 1 << 3)

    var spoken: String {
        var names: [String] = []
        if contains(.control) { names.append("Control") }
        if contains(.option) { names.append("Option") }
        if contains(.shift) { names.append("Shift") }
        if contains(.command) { names.append("Command") }
        return names.joined(separator: "+")
    }
}

/// How a registered chord starts and stops dictation.
public enum HotkeyMode: String, Codable, Hashable, Sendable {
    case hold
    case toggle
}

/// User-changeable dictation chord. This is the settings value: a virtual key code
/// plus modifiers, with no dependency on another package. The default is Option-Space.
public struct HotkeyChord: Hashable, Sendable, Codable {
    /// Virtual key code for Space (`kVK_Space` in Events.h).
    public static let spaceKeyCode: UInt16 = 49

    /// Highest virtual key code defined by HIToolbox (`kVK_UpArrow`).
    public static let maxKeyCode: UInt16 = 0x7E

    public var keyCode: UInt16
    public var modifiers: HotkeyModifier

    public init(keyCode: UInt16, modifiers: HotkeyModifier) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    public static let optionSpace = HotkeyChord(keyCode: spaceKeyCode, modifiers: .option)

    var spoken: String {
        let mods = modifiers.spoken
        if mods.isEmpty { return "key \(keyCode)" }
        return "\(mods)+key \(keyCode)"
    }
}

/// Mode plus chord passed to `HotkeyService.register`. The chord alone is the
/// persisted settings value.
public struct HotkeyRegistration: Hashable, Sendable, Codable {
    public var mode: HotkeyMode
    public var chord: HotkeyChord

    public init(mode: HotkeyMode, chord: HotkeyChord) {
        self.mode = mode
        self.chord = chord
    }

    public static let `default` = HotkeyRegistration(mode: .hold, chord: .optionSpace)
}

/// Why a chord is rejected before Carbon is called.
public enum HotkeyChordProblem: Sendable, Hashable {
    /// `keyCode` is above `HotkeyChord.maxKeyCode`.
    case keyCodeOutOfRange
    /// The key is itself a modifier, so it cannot be the hotkey's key.
    case keyIsModifier
    /// A bare key would swallow typing. At least one modifier is required.
    case missingModifier

    var detail: String {
        switch self {
        case .keyCodeOutOfRange:
            "the key code is outside 0...\(HotkeyChord.maxKeyCode)."
        case .keyIsModifier:
            "the key is a modifier."
        case .missingModifier:
            "the chord has no modifier."
        }
    }
}

/// Pure checks for a chord Minutae is willing to register.
public enum HotkeyChordValidation {
    /// HIToolbox modifier virtual key codes. A hotkey's key cannot be one of these.
    static let modifierKeyCodes: Set<UInt16> = [
        0x36,  // kVK_RightCommand
        0x37,  // kVK_Command
        0x38,  // kVK_Shift
        0x39,  // kVK_CapsLock
        0x3A,  // kVK_Option
        0x3B,  // kVK_Control
        0x3C,  // kVK_RightShift
        0x3D,  // kVK_RightOption
        0x3E,  // kVK_RightControl
        0x3F,  // kVK_Function
    ]

    public static func problem(with chord: HotkeyChord) -> HotkeyChordProblem? {
        if chord.keyCode > HotkeyChord.maxKeyCode {
            return .keyCodeOutOfRange
        }
        if modifierKeyCodes.contains(chord.keyCode) {
            return .keyIsModifier
        }
        if chord.modifiers.isEmpty {
            return .missingModifier
        }
        return nil
    }
}

/// Carbon `eventHotKeyExistsErr` (-9878). Named here so failure tests do not
/// need the Carbon framework.
public enum HotkeyCarbonStatus {
    public static let success: Int32 = 0
    public static let alreadyRegistered: Int32 = -9878
}

/// Next chord to offer when registration of `failed` does not succeed.
///
/// An option-only chord (the default, and the combination macOS 15 has broken
/// in some releases) is recovered by adding Control on the same key. Any other
/// failure walks `candidates`, skipping the chord that just failed.
public enum HotkeyFallback {
    public static let controlOptionSpace = HotkeyChord(
        keyCode: HotkeyChord.spaceKeyCode,
        modifiers: [.control, .option]
    )

    public static let candidates: [HotkeyChord] = [
        .optionSpace,
        controlOptionSpace,
        HotkeyChord(keyCode: HotkeyChord.spaceKeyCode, modifiers: [.command, .option]),
        HotkeyChord(keyCode: HotkeyChord.spaceKeyCode, modifiers: [.control, .shift]),
    ]

    public static func suggestion(after failed: HotkeyChord) -> HotkeyChord {
        var preferred: [HotkeyChord] = []
        if failed.modifiers == .option {
            let recovered = HotkeyChord(
                keyCode: failed.keyCode,
                modifiers: [.control, .option]
            )
            preferred.append(recovered)
        }
        preferred.append(contentsOf: candidates)
        if let match = preferred.first(where: { candidate in
            candidate != failed && HotkeyChordValidation.problem(with: candidate) == nil
        }) {
            return match
        }
        let lastResort = HotkeyChord(
            keyCode: HotkeyChord.spaceKeyCode,
            modifiers: [.command, .shift]
        )
        if lastResort != failed, HotkeyChordValidation.problem(with: lastResort) == nil {
            return lastResort
        }
        return .optionSpace
    }
}
