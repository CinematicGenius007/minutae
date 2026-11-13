import AppKit

/// Opens the Privacy pane that matches a permission. Does not request access.
public enum PermissionSettingsLink {
    public static func url(for permission: Permission) -> URL {
        let pane = paneIdentifier(for: permission)
        let string =
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?\(pane)"
        guard let url = URL(string: string) else {
            preconditionFailure(
                "System Settings URL could not be formed for \(permission.rawValue)")
        }
        return url
    }

    @MainActor
    @discardableResult
    public static func open(_ permission: Permission) -> Bool {
        NSWorkspace.shared.open(url(for: permission))
    }

    public static func paneIdentifier(for permission: Permission) -> String {
        switch permission {
        case .microphone:
            "Privacy_Microphone"
        case .accessibility:
            "Privacy_Accessibility"
        case .screenRecording:
            "Privacy_ScreenCapture"
        case .calendar:
            "Privacy_Calendars"
        }
    }
}
