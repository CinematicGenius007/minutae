import Foundation
import MinutaeLibrary
import Observation

/// State behind the ⌘K sheet: debounced query, ranked hits, keyboard selection.
@MainActor @Observable
public final class SearchModel {
    public var query = "" { didSet { scheduleSearch() } }
    public private(set) var hits: [SearchHit] = []
    public private(set) var isSearching = false
    public private(set) var error: String?
    public var selection = 0

    private let service: SearchService
    private let meetingIDs: @Sendable () -> [String]
    private var indexed: Set<String> = []
    private var task: Task<Void, Never>?
    private let debounce: Duration

    public init(
        service: SearchService,
        meetingIDs: @escaping @Sendable () -> [String],
        debounce: Duration = .milliseconds(180)
    ) {
        self.service = service
        self.meetingIDs = meetingIDs
        self.debounce = debounce
    }

    /// Embeds transcripts for meetings not yet indexed in this run. Safe to call on every open.
    public func prepare() async {
        for id in meetingIDs() where !indexed.contains(id) {
            do {
                try await service.indexMeeting(id)
                indexed.insert(id)
            } catch { continue }
        }
    }

    public func reset() {
        task?.cancel()
        query = ""
        hits = []
        selection = 0
        error = nil
    }

    public func move(_ delta: Int) {
        guard !hits.isEmpty else { return }
        selection = min(max(selection + delta, 0), hits.count - 1)
    }

    public var selectedHit: SearchHit? {
        hits.indices.contains(selection) ? hits[selection] : nil
    }

    /// Runs the current query now. Tests call this directly.
    public func runNow() async {
        let text = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else {
            hits = []
            isSearching = false
            return
        }
        isSearching = true
        defer { isSearching = false }
        do {
            let found = try await service.search(query: text, limit: 20)
            guard !Task.isCancelled, text == query.trimmingCharacters(in: .whitespacesAndNewlines)
            else { return }
            hits = found
            selection = 0
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func scheduleSearch() {
        task?.cancel()
        task = Task { [debounce] in
            try? await Task.sleep(for: debounce)
            guard !Task.isCancelled else { return }
            await runNow()
        }
    }
}
