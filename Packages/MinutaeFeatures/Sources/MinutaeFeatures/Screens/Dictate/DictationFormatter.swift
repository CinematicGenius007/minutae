import Foundation
import MinutaeCore

/// Copy for dictation rows and the detail header. Locale and time zone are injectable for tests.
public struct DictationFormatter: Sendable {
    public var locale: Locale
    public var timeZone: TimeZone

    public init(locale: Locale = .autoupdatingCurrent, timeZone: TimeZone = .autoupdatingCurrent) {
        self.locale = locale
        self.timeZone = timeZone
    }

    private var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.locale = locale
        calendar.timeZone = timeZone
        return calendar
    }

    /// "2:41 PM"
    public func time(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.timeZone = timeZone
        formatter.setLocalizedDateFormatFromTemplate("jmm")
        // ICU puts a narrow no-break space before AM/PM. Mono metadata reads better with a space.
        return formatter.string(from: date).replacingOccurrences(of: "\u{202F}", with: " ")
    }

    /// "Today", "Yesterday", or "Nov 12".
    public func day(_ date: Date, now: Date) -> String {
        if calendar.isDate(date, inSameDayAs: now) { return "Today" }
        let yesterday = calendar.date(byAdding: .day, value: -1, to: now)
        if let yesterday, calendar.isDate(date, inSameDayAs: yesterday) { return "Yesterday" }
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.timeZone = timeZone
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: now)
        formatter.setLocalizedDateFormatFromTemplate(sameYear ? "MMMd" : "yMMMd")
        return formatter.string(from: date)
    }

    /// "6 s", "1 m 12 s", "2 m". Rounded to the nearest second.
    public func duration(milliseconds: Int) -> String {
        let seconds = max(0, (milliseconds + 500) / 1_000)
        if seconds < 60 { return "\(seconds) s" }
        let minutes = seconds / 60
        let rest = seconds % 60
        return rest == 0 ? "\(minutes) m" : "\(minutes) m \(rest) s"
    }

    /// "0:02". Hours are folded into minutes.
    public func clock(seconds: TimeInterval) -> String {
        let whole = max(0, Int(seconds.rounded(.down)))
        return String(format: "%d:%02d", whole / 60, whole % 60)
    }

    /// Row subtitle: "2:41 PM · 6 s" today, "Yesterday, 2:41 PM · 6 s" otherwise.
    public func rowMeta(_ dictation: Dictation, now: Date) -> String {
        var parts: [String] = []
        if let date = Self.date(dictation.started_at) {
            let clock = time(date)
            let day = day(date, now: now)
            parts.append(day == "Today" ? clock : "\(day), \(clock)")
        }
        if let duration = dictation.duration_ms {
            parts.append(self.duration(milliseconds: duration))
        }
        return parts.joined(separator: " · ")
    }

    /// "Dictation · 2:41 PM"
    public func detailTitle(_ dictation: Dictation) -> String {
        guard let date = Self.date(dictation.started_at) else { return "Dictation" }
        return "Dictation · \(time(date))"
    }

    /// "Today · 6 s · pasted into Slack · whisper small". Unknown parts are left out.
    public func detailMeta(_ dictation: Dictation, now: Date) -> String {
        var parts: [String] = []
        if let date = Self.date(dictation.started_at) {
            parts.append(day(date, now: now))
        }
        if let duration = dictation.duration_ms {
            parts.append(self.duration(milliseconds: duration))
        }
        if let destination = Self.destination(dictation) {
            parts.append(destination)
        }
        if let model = dictation.speech_model, !model.isEmpty {
            parts.append("whisper \(model)")
        }
        return parts.joined(separator: " · ")
    }

    static func destination(_ dictation: Dictation) -> String? {
        let app = dictation.target_app_name.flatMap { $0.isEmpty ? nil : $0 }
        let into = app.map { " into \($0)" } ?? ""
        switch dictation.insert_method {
        case "pasteboard": return "pasted\(into)"
        case "accessibility": return "inserted\(into)"
        case "typing": return "typed\(into)"
        case "clipboardOnly": return "copied to the clipboard"
        default:
            switch dictation.status {
            case "failed": return "failed"
            case "cancelled": return "cancelled"
            default: return nil
            }
        }
    }

    static func date(_ string: String) -> Date? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        if let date = formatter.date(from: string) { return date }
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: string)
    }
}
