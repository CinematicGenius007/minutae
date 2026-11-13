import Foundation

/// Why a settings value was rejected. Descriptions never include the rejected text,
/// so a pasted secret cannot leak through an error.
public enum SettingsValidationError: Error, Equatable, Sendable {
    case emptyModelName
    case disallowedModelName
    case emptyPath
    case disallowedPathCharacters
    case pathNotAbsolute
    case pathTraversal
    case invalidPromptDelay
    case invalidRetention
}

extension SettingsValidationError: CustomStringConvertible, LocalizedError {
    public var description: String {
        switch self {
        case .emptyModelName:
            return "Model name is empty."
        case .disallowedModelName:
            return "Model name contains disallowed characters."
        case .emptyPath:
            return "Path is empty."
        case .disallowedPathCharacters:
            return "Path contains disallowed characters."
        case .pathNotAbsolute:
            return "Path must be absolute."
        case .pathTraversal:
            return "Path traversal is not allowed."
        case .invalidPromptDelay:
            return "Prompt delay is outside the allowed range."
        case .invalidRetention:
            return "Audio retention is invalid."
        }
    }

    public var errorDescription: String? { description }
}

/// Checks model names and storage paths before they are persisted.
public enum SettingsSanitizer {
    /// Seconds of mic activity before the meeting prompt. Matches the existing settings control.
    public static let promptDelayRange = 1...30

    public static func validateModelName(_ name: String) throws {
        if name.isEmpty || name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw SettingsValidationError.emptyModelName
        }
        if isDisallowedModelName(name) {
            throw SettingsValidationError.disallowedModelName
        }
    }

    public static func validateStoragePath(_ path: String) throws {
        if path.isEmpty {
            throw SettingsValidationError.emptyPath
        }
        if containsControlCharacters(path) || path.contains("\\") {
            throw SettingsValidationError.disallowedPathCharacters
        }
        if !path.hasPrefix("/") {
            throw SettingsValidationError.pathNotAbsolute
        }
        let components = path.split(separator: "/", omittingEmptySubsequences: true)
        if components.isEmpty {
            throw SettingsValidationError.emptyPath
        }
        if components.contains(where: isTraversalComponent) {
            throw SettingsValidationError.pathTraversal
        }
    }

    public static func validatePromptDelay(_ seconds: Int) throws {
        if !promptDelayRange.contains(seconds) {
            throw SettingsValidationError.invalidPromptDelay
        }
    }

    public static func validateRetention(_ retention: AudioRetention) throws {
        guard case .days(let count) = retention else { return }
        if count < 1 {
            throw SettingsValidationError.invalidRetention
        }
    }

    private static func isDisallowedModelName(_ name: String) -> Bool {
        if name.hasPrefix(".") || name.contains("/") || name.contains("\\") || name.contains("..") {
            return true
        }
        if containsControlCharacters(name) || containsWhitespace(name) {
            return true
        }
        return decodesToTraversal(name)
    }

    private static func isTraversalComponent(_ component: Substring) -> Bool {
        component == ".." || decodesToTraversal(String(component))
    }

    private static func decodesToTraversal(_ value: String) -> Bool {
        var current = value
        for _ in 0..<2 {
            guard let decoded = current.removingPercentEncoding, decoded != current else {
                return false
            }
            if decoded.contains("..") || decoded.contains("/") || decoded.contains("\\") {
                return true
            }
            current = decoded
        }
        return false
    }

    private static func containsControlCharacters(_ value: String) -> Bool {
        value.unicodeScalars.contains { scalar in
            scalar.value < 0x20 || scalar.value == 0x7F
                || scalar.properties.generalCategory == .control
        }
    }

    private static func containsWhitespace(_ value: String) -> Bool {
        value.rangeOfCharacter(from: .whitespacesAndNewlines) != nil
    }
}
