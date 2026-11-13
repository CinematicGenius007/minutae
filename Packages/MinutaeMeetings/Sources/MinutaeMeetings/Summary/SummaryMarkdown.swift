import Foundation
import MinutaeCore

struct RenderedSummary: Sendable, Equatable {
    var file: String
    var body: String
}

/// Byte-for-byte port of `legacy/src/lib/markdown.ts` and `slugify`.
enum SummaryMarkdown {
    static func render(
        meeting: Meeting,
        summary: MeetingSummary,
        recordingFilename: String
    ) -> RenderedSummary {
        let body = body(summary)
        let duration = durationText(meeting.duration_seconds)
        let title = summary.title.replacingOccurrences(of: "\"", with: "\\\"")
        let participants = summary.participants.joined(separator: ", ")
        let front = [
            "---",
            "id: \(meeting.id)",
            "title: \"\(title)\"",
            "date: \(meeting.started_at)",
            "duration: \(duration)",
            "participants: [\(participants)]",
            "tags: []",
            "recording: ../recordings/\(recordingFilename)",
            "---",
            ""
        ].joined(separator: "\n")
        return RenderedSummary(file: front + "\n" + body, body: body)
    }

    static func body(_ summary: MeetingSummary) -> String {
        var lines = ["## Summary"]
        lines.append(contentsOf: summary.summary.map { "- \($0)" })
        if !summary.keyDecisions.isEmpty {
            lines.append("")
            lines.append("## Key Decisions")
            lines.append(contentsOf: summary.keyDecisions.map { "- \($0)" })
        }
        if !summary.actionItems.isEmpty {
            lines.append("")
            lines.append("## Action Items")
            lines.append(contentsOf: summary.actionItems.map { "- [ ] \($0.checklistText)" })
        }
        return lines.joined(separator: "\n") + "\n"
    }

    static func durationText(_ seconds: Int?) -> String {
        guard let seconds, seconds != 0 else { return "unknown" }
        return formatDurationLong(seconds)
    }

    /// `legacy/src/lib/utils.ts` `formatDurationLong`. Zero is handled by the caller.
    static func formatDurationLong(_ seconds: Int) -> String {
        if seconds < 60 { return "\(seconds)s" }
        let hours = seconds / 3600
        let minutes = (seconds % 3600) / 60
        let remainder = seconds % 60
        if hours > 0 { return "\(hours)h \(minutes)m" }
        return "\(minutes)m \(remainder)s"
    }

    /// Empty titles use `meeting`. Any other title is slugified, including ones
    /// that collapse to an empty slug (`!!!` → `DATE-.md`).
    static func slug(for title: String) -> String {
        slugify(title.isEmpty ? "meeting" : title)
    }

    /// JavaScript `slugify`: lowercase, non-alphanumerics to `-`, trim ends, 60 UTF-16 units.
    static func slugify(_ text: String) -> String {
        let lowered = text.lowercased(with: Locale(identifier: "en_US_POSIX"))
        var slug = ""
        var pendingHyphen = false
        var started = false
        for scalar in lowered.unicodeScalars {
            if isSlugLetter(scalar) {
                if pendingHyphen, started { slug.append("-") }
                pendingHyphen = false
                started = true
                slug.unicodeScalars.append(scalar)
            } else if started {
                pendingHyphen = true
            }
        }
        return utf16Prefix(slug, 60)
    }

    static func datePrefix(_ startedAt: String) -> String {
        String(startedAt.prefix(10))
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: "\\", with: "-")
            .replacingOccurrences(of: "\0", with: "")
            .replacingOccurrences(of: "..", with: "-")
    }

    static func recordingFilename(meetingID: String, path: String?) -> String {
        guard let path, let name = path.split(separator: "/").last else {
            return "\(meetingID).wav"
        }
        let filename = String(name)
        if filename.isEmpty || filename == "." || filename == ".." {
            return "\(meetingID).wav"
        }
        return filename
    }

    static func destination(
        directory: URL,
        startedAt: String,
        title: String,
        meetingID: String
    ) throws -> URL {
        let stem = "\(datePrefix(startedAt))-\(slug(for: title))"
        var suffix = ""
        var number = 2
        while number < 10_000 {
            let name = "\(stem)\(suffix).md"
            try validateFilename(name)
            let url = directory.appendingPathComponent(name)
            if isSymbolicLink(url) {
                suffix = "-\(number)"
                number += 1
                continue
            }
            if !FileManager.default.fileExists(atPath: url.path) || belongs(url, meetingID: meetingID) {
                return url
            }
            suffix = "-\(number)"
            number += 1
        }
        throw SummaryJobError(
            errorClass: "unavailable",
            message: "The summary file name is already in use."
        )
    }

    static func atomicWrite(_ text: String, to destination: URL) throws {
        let directory = destination.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let temp = directory.appendingPathComponent(".minutae-\(UUID().uuidString).tmp")
        do {
            try Data(text.utf8).write(to: temp, options: .withoutOverwriting)
            let handle = try FileHandle(forWritingTo: temp)
            do {
                try handle.synchronize()
                try handle.close()
            } catch {
                try? handle.close()
                throw error
            }
            if FileManager.default.fileExists(atPath: destination.path) {
                if let backup = try FileManager.default.replaceItemAt(destination, withItemAt: temp) {
                    try? FileManager.default.removeItem(at: backup)
                }
            } else {
                try FileManager.default.moveItem(at: temp, to: destination)
            }
        } catch {
            try? FileManager.default.removeItem(at: temp)
            throw error
        }
    }

    private static func validateFilename(_ name: String) throws {
        let bad =
            name.isEmpty || name.hasPrefix(".") || name.contains("/") || name.contains("\\")
            || name.contains("..") || name.contains("\0")
        if bad {
            throw SummaryJobError(
                errorClass: "unavailable",
                message: "The summary file name is not valid."
            )
        }
    }

    private static func belongs(_ url: URL, meetingID: String) -> Bool {
        guard !meetingID.isEmpty,
            let data = try? Data(contentsOf: url),
            let text = String(data: data.prefix(4_096), encoding: .utf8)
        else { return false }
        let header = text.split(separator: "\n", maxSplits: 12, omittingEmptySubsequences: false)
        return header.contains(Substring("id: \(meetingID)"))
    }

    private static func isSymbolicLink(_ url: URL) -> Bool {
        (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true
    }

    private static func isSlugLetter(_ scalar: Unicode.Scalar) -> Bool {
        guard scalar.value < 128 else { return false }
        let character = Character(scalar)
        return character.isNumber || (character >= "a" && character <= "z")
    }

    private static func utf16Prefix(_ text: String, _ limit: Int) -> String {
        let units = Array(text.utf16.prefix(limit))
        return String(utf16CodeUnits: units, count: units.count)
    }
}
