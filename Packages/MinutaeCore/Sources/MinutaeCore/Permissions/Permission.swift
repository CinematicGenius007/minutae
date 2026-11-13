/// A system capability Minutae asks for only when a feature needs it.
public enum Permission: String, Sendable, Hashable, CaseIterable {
    case microphone
    case accessibility
    /// Screen Recording, which also gates capture of other apps' audio.
    case screenRecording
    case calendar

    /// Plain-language reason shown when this permission is requested.
    public var reason: String {
        switch self {
        case .microphone:
            "Minutae needs the microphone to record what you say in meetings and dictation."
        case .accessibility:
            "Minutae needs Accessibility to put dictated text into the app you are using."
        case .screenRecording:
            "Minutae needs permission to capture audio from other apps in a meeting. It does not record your screen."
        case .calendar:
            "Minutae needs calendar access to name a meeting after the event on your calendar."
        }
    }
}

public enum PermissionStatus: String, Sendable, Hashable, CaseIterable {
    case notDetermined
    case granted
    case denied
    case restricted
}

/// What Minutae does when the user denies a permission. See ARCHITECTURE §9.
public enum PermissionFallback: String, Sendable, Hashable, CaseIterable {
    /// Microphone: the feature is disabled and the user is told why.
    case disabledWithPrompt
    /// Accessibility: copy the text to the clipboard and tell the user.
    case copyToClipboard
    /// Screen recording: record the microphone only.
    case microphoneOnly
    /// Calendar: meetings have no calendar titles.
    case noTitles

    public static func defined(for permission: Permission) -> PermissionFallback {
        switch permission {
        case .microphone:
            .disabledWithPrompt
        case .accessibility:
            .copyToClipboard
        case .screenRecording:
            .microphoneOnly
        case .calendar:
            .noTitles
        }
    }
}

public struct PermissionChange: Sendable, Equatable {
    public let permission: Permission
    public let status: PermissionStatus

    public init(permission: Permission, status: PermissionStatus) {
        self.permission = permission
        self.status = status
    }
}
