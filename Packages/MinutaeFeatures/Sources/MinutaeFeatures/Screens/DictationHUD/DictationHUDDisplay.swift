import Foundation
import MinutaeDictation

/// What the HUD pill shows. Paper: "16 · Dictation HUD states".
public enum DictationHUDDisplay: Equatable, Sendable {
    case listening
    case transcribing
    case inserted(appName: String?)
    case copied
    case error(String)

    /// Inserted, copied, and error dismiss themselves.
    public var isTerminal: Bool {
        switch self {
        case .listening, .transcribing: false
        case .inserted, .copied, .error: true
        }
    }

    public var title: String {
        switch self {
        case .listening: "Listening…"
        case .transcribing: "Transcribing…"
        case .inserted(let app?) where !app.isEmpty: "Inserted into \(app)"
        case .inserted: "Inserted"
        case .copied: "Copied to the clipboard. Press ⌘V."
        case .error(let message): message
        }
    }

    /// Maps one controller snapshot. Nil hides the HUD.
    ///
    /// The controller ends every run with `idle` and an outcome; a failure is `error` and then
    /// `idle`, both with `.failed`, so both map to the same error pill.
    public static func from(_ state: DictationHUDState) -> DictationHUDDisplay? {
        switch state.phase {
        case .listening:
            return .listening
        case .processing, .inserting:
            return .transcribing
        case .error:
            return .error(DictationHUDCopy.error(state.banner))
        case .idle:
            switch state.outcome {
            case .inserted: return .inserted(appName: state.targetAppName)
            case .copiedToClipboard: return .copied
            case .nothingHeard: return .error(DictationHUDCopy.nothingHeard)
            case .failed: return .error(DictationHUDCopy.error(state.banner))
            case .cancelled, nil: return nil
            }
        }
    }
}

/// Plain-language error copy. The controller's banner is a stage prefix and a detail,
/// such as `transcribe: notLoaded`; the HUD never shows that raw text.
public enum DictationHUDCopy {
    public static let nothingHeard = "Couldn't hear anything. Check the microphone."
    public static let microphone = "Couldn't use the microphone. Check it in System Settings."
    public static let transcription = "Couldn't transcribe. Check the Whisper model in Settings."
    public static let secureField = "That's a password field, so nothing was typed."
    public static let insertion = "Couldn't paste the text. It's saved in Minutae."
    public static let saving = "Couldn't save this dictation."
    public static let generic = "Dictation failed. Try again."

    public static func error(_ banner: String?) -> String {
        guard let banner, !banner.isEmpty else { return generic }
        if banner.hasPrefix("capture") || banner.hasPrefix("wav") { return microphone }
        if banner.hasPrefix("transcribe") { return transcription }
        if banner == "insert: secure field" { return secureField }
        if banner.hasPrefix("insert") || banner.hasPrefix("post-process") { return insertion }
        if banner.hasPrefix("store") { return saving }
        return generic
    }
}
