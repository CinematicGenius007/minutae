import Foundation
import MinutaeCore

/// One dictation as a Markdown file with YAML frontmatter, like meeting summaries.
public enum DictationMarkdown {
    public static func render(_ dictation: Dictation) -> String {
        var lines = ["---", "type: dictation", "id: \(quoted(dictation.id))"]
        lines.append("date: \(quoted(dictation.started_at))")
        if let duration = dictation.duration_ms {
            lines.append("duration_ms: \(duration)")
        }
        if let app = dictation.target_app_name, !app.isEmpty {
            lines.append("app: \(quoted(app))")
        }
        if let model = dictation.speech_model, !model.isEmpty {
            lines.append("model: \(quoted("whisper \(model)"))")
        }
        if let language = dictation.language, !language.isEmpty {
            lines.append("language: \(quoted(language))")
        }
        if let edited = dictation.edited_at {
            lines.append("edited: \(quoted(edited))")
        }
        if let audio = dictation.audio_path, !audio.isEmpty {
            lines.append("audio: \(quoted(audio))")
        }
        lines.append("---")
        lines.append("")
        lines.append(dictation.displayText.trimmingCharacters(in: .whitespacesAndNewlines))
        lines.append("")
        return lines.joined(separator: "\n")
    }

    /// `dictation-2026-11-13-1441.md` in the local time zone.
    public static func suggestedFileName(
        for dictation: Dictation,
        extension fileExtension: String,
        timeZone: TimeZone = .current
    ) -> String {
        guard let date = DictationTimestamp.date(dictation.started_at) else {
            return "dictation-\(dictation.id).\(fileExtension)"
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = timeZone
        formatter.dateFormat = "yyyy-MM-dd-HHmm"
        return "dictation-\(formatter.string(from: date)).\(fileExtension)"
    }

    /// Double-quoted YAML scalar with backslash, quote, and newline escaped.
    static func quoted(_ value: String) -> String {
        var escaped = value.replacingOccurrences(of: "\\", with: "\\\\")
        escaped = escaped.replacingOccurrences(of: "\"", with: "\\\"")
        escaped = escaped.replacingOccurrences(of: "\n", with: "\\n")
        escaped = escaped.replacingOccurrences(of: "\r", with: "\\r")
        return "\"\(escaped)\""
    }
}
