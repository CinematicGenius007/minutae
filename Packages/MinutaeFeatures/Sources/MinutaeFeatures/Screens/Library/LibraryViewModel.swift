import Foundation
import MinutaeCore
import Observation

/// One row in the Library: a meeting or a dictation, newest first.
public struct LibraryItem: Identifiable, Equatable, Sendable {
    public enum Kind: String, CaseIterable, Sendable { case meeting, dictation }
    public let id: String
    public let kind: Kind
    public let title: String
    public let detail: String
    public let startedAt: String

    public var target: OpenTarget { kind == .meeting ? .meeting(id) : .dictation(id) }
}

public enum LibraryFilter: String, CaseIterable, Sendable {
    case all = "All", meetings = "Meetings", dictations = "Dictations"
}

@MainActor @Observable
public final class LibraryViewModel {
    public var filter: LibraryFilter = .all
    public var query = ""
    public private(set) var items: [LibraryItem] = []
    public private(set) var error: String?
    private let database: MinutaeDatabase

    public init(database: MinutaeDatabase) { self.database = database }

    public var visible: [LibraryItem] {
        items.filter { item in
            switch filter {
            case .all: true
            case .meetings: item.kind == .meeting
            case .dictations: item.kind == .dictation
            }
        }.filter {
            query.isEmpty || $0.title.localizedCaseInsensitiveContains(query)
                || $0.detail.localizedCaseInsensitiveContains(query)
        }
    }

    public func refresh() {
        do {
            let meetings = try Repository<Meeting>(database).newestFirst().map {
                LibraryItem(
                    id: $0.id, kind: .meeting, title: $0.title.isEmpty ? "Untitled meeting" : $0.title,
                    detail: Formatting.duration($0.duration_seconds), startedAt: $0.started_at)
            }
            let dictations = try Repository<Dictation>(database).all().map { row -> LibraryItem in
                let text = (row.edited_text ?? row.final_text).trimmingCharacters(in: .whitespacesAndNewlines)
                return LibraryItem(
                    id: row.id, kind: .dictation, title: Self.preview(text),
                    detail: row.target_app_name ?? "", startedAt: row.started_at)
            }
            items = (meetings + dictations).sorted { $0.startedAt > $1.startedAt }
            error = nil
        } catch { self.error = error.localizedDescription }
    }

    static func preview(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        if line.isEmpty { return "Empty dictation" }
        return line.count > 90 ? String(line.prefix(90)) + "…" : line
    }
}
