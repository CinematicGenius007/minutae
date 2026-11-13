import Foundation
import MinutaeAudio
import MinutaeCore
import MinutaeDictation
import MinutaeLLM
import MinutaeMeetings
import MinutaeSpeech

enum AppMessages {
    static func missingModel(_ name: String) -> String {
        """
        The Whisper model “\(name)” is not installed. \
        Download it in this window before transcription or dictation.
        """
    }

    static func permissionBlocked(_ permission: Permission, status: PermissionStatus) -> String {
        "\(permissionName(permission)) is \(statusName(status)). \(permission.reason) \(fallback(permission))"
    }

    static func permissionName(_ permission: Permission) -> String {
        switch permission {
        case .microphone:
            "Microphone"
        case .accessibility:
            "Accessibility"
        case .screenRecording:
            "Screen Recording"
        case .calendar:
            "Calendars"
        }
    }

    static func statusName(_ status: PermissionStatus) -> String {
        switch status {
        case .notDetermined:
            "not yet requested"
        case .granted:
            "granted"
        case .denied:
            "denied"
        case .restricted:
            "restricted"
        }
    }

    static func transportName(_ id: TransportID) -> String {
        switch id {
        case .codexCLI:
            "Codex"
        case .grokCLI:
            "Grok"
        case .ollama:
            "Ollama"
        case .api:
            "API"
        }
    }

    static func describe(_ error: Error) -> String {
        if let failure = error as? SpeechLoadFailure { return failure.message }
        if let failure = error as? AppEnvironmentError { return failure.message }
        if let failure = error as? CaptureFailure { return failure.message }
        if let failure = error as? HotkeyRegistrationFailure { return failure.message }
        if let failure = error as? SummaryJobError { return failure.message }
        if let failure = error as? NonRetryableJobError { return failure.message }
        if let failure = error as? MeetingRecorderError { return meeting(failure) }
        if let failure = error as? LLMError { return llm(failure) }
        if let failure = error as? RouterAggregateError { return router(failure) }
        if let failure = error as? SpeechError { return speech(failure) }
        if let failure = error as? ModelError { return model(failure) }
        if let failure = error as? CaptureError { return capture(failure) }
        if let failure = error as? SettingsStoreError { return failure.description }
        let text = error.localizedDescription.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.isEmpty { return "Something failed." }
        return text
    }

    static func transportLine(_ status: TransportStatus) -> String {
        var parts = [status.installed ? "installed" : "not installed"]
        if let signedIn = status.signedIn {
            parts.append(signedIn ? "signed in" : "not signed in")
        } else {
            parts.append("sign-in not checked")
        }
        if let model = status.model, !model.isEmpty {
            parts.append("model \(model)")
        }
        if let detail = status.detail, !detail.isEmpty {
            parts.append(detail)
        }
        return parts.joined(separator: " · ")
    }

    private static func fallback(_ permission: Permission) -> String {
        switch permission {
        case .microphone:
            "Recording and dictation stay off until the microphone is allowed."
        case .accessibility:
            "Dictated text is copied to the clipboard instead of inserted."
        case .screenRecording:
            "Meetings record the microphone only."
        case .calendar:
            "Meetings do not use calendar titles."
        }
    }

    private static func meeting(_ error: MeetingRecorderError) -> String {
        switch error {
        case .alreadyRecording:
            "A meeting is already recording."
        case .notRecording:
            "No meeting is recording."
        case .finishFailed(_, let reason):
            "The recording could not be finished. \(reason)"
        }
    }

    private static func llm(_ error: LLMError) -> String {
        switch error {
        case .notInstalled:
            "This AI tool is not installed."
        case .notLoggedIn:
            "This AI tool is not signed in. Sign in with its own CLI, then try again."
        case .rateLimited:
            "This AI tool is rate limited. Wait, or try the next transport."
        case .timeout:
            "The AI request timed out."
        case .cancelled:
            "The AI request was cancelled."
        case .invalidOutput(let message):
            message.isEmpty ? "The AI tool returned text that could not be used." : message
        case .modelRejected(let message):
            message.isEmpty ? "The selected model was rejected." : message
        case .unavailable(let message):
            message.isEmpty ? "This AI tool is unavailable." : message
        }
    }

    private static func router(_ error: RouterAggregateError) -> String {
        if error.failures.isEmpty {
            return "No AI transport is available."
        }
        let parts = error.failures.map { failure in
            "\(transportName(failure.transport)): \(className(failure.errorClass))"
        }
        return "Every AI transport failed. \(parts.joined(separator: " "))"
    }

    private static func className(_ token: String) -> String {
        switch token {
        case "notInstalled":
            "not installed"
        case "notLoggedIn":
            "not signed in"
        case "rateLimited":
            "rate limited"
        case "timeout":
            "timed out"
        case "cancelled":
            "cancelled"
        case "invalidOutput":
            "invalid output"
        case "modelRejected":
            "model rejected"
        default:
            "unavailable"
        }
    }

    private static func speech(_ error: SpeechError) -> String {
        switch error {
        case .modelLoadFailed:
            "The Whisper model could not be loaded."
        case .notLoaded:
            "The Whisper model is not loaded."
        case .transcriptionFailed:
            "Transcription failed."
        case .invalidLanguage:
            "That transcription language is not supported."
        }
    }

    private static func model(_ error: ModelError) -> String {
        switch error {
        case .invalidName:
            "That Whisper model name is not allowed."
        case .unsupportedModel:
            "That Whisper model is not one Minutae can download."
        case .badResponse:
            "The model download did not come from the official Whisper repository."
        case .invalidSize, .invalidChecksum:
            "The downloaded Whisper model failed its check. Try the download again."
        }
    }

    private static func capture(_ error: CaptureError) -> String {
        switch error {
        case .alreadyRecording:
            "The microphone is already in use by a meeting or a dictation."
        case .notRecording:
            "Nothing is recording."
        case .conversionFailed:
            "The microphone audio could not be converted."
        }
    }
}

struct AppEnvironmentError: Error, CustomStringConvertible {
    var message: String
    var description: String { message }
}

struct SpeechLoadFailure: Error, CustomStringConvertible {
    var message: String
    var description: String { message }
}
