import Foundation
import MinutaeLLM

/// Human-readable strings for stored values. Never show raw ids or ISO timestamps.
enum Formatting {
    static func date(_ iso: String) -> Date? {
        let plain = ISO8601DateFormatter()
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return plain.date(from: iso) ?? fractional.date(from: iso)
    }

    static func dayHeader(_ date: Date, now: Date = Date(), calendar: Calendar = .current) -> String {
        if calendar.isDate(date, inSameDayAs: now) { return "Today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now),
            calendar.isDate(date, inSameDayAs: yesterday)
        {
            return "Yesterday"
        }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: now)
        let base = date.formatted(.dateTime.weekday(.wide).day().month(.wide))
        return sameYear ? base : base + ", " + date.formatted(.dateTime.year())
    }

    static func time(_ date: Date) -> String {
        date.formatted(.dateTime.hour().minute())
    }

    static func fullDate(_ iso: String) -> String {
        guard let date = date(iso) else { return iso }
        return date.formatted(.dateTime.weekday(.wide).day().month(.wide).hour().minute())
    }

    /// 32m 14s, 1h 05m, 45s.
    static func duration(_ seconds: Int?) -> String {
        guard let seconds, seconds > 0 else { return "—" }
        let h = seconds / 3600
        let m = (seconds % 3600) / 60
        let s = seconds % 60
        if h > 0 { return String(format: "%dh %02dm", h, m) }
        if m > 0 { return s == 0 ? "\(m)m" : "\(m)m \(s)s" }
        return "\(s)s"
    }

    static func clock(_ seconds: Int) -> String {
        String(format: "%d:%02d", seconds / 60, seconds % 60)
    }

    static func provider(_ id: String) -> String {
        switch id {
        case "codexCLI": "Codex"
        case "grokCLI": "Grok"
        case "ollama": "Ollama"
        case "api": "API key"
        default: id
        }
    }

    static func count(_ n: Int, _ noun: String) -> String { "\(n) \(noun)\(n == 1 ? "" : "s")" }

    static func tokens(_ n: Int) -> String {
        n.formatted(.number.notation(.compactName))
    }
}
