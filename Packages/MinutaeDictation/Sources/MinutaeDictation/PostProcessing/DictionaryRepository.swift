import Foundation
import MinutaeCore

/// Enabled dictionary rows for one frontmost app.
///
/// A row with a null bundle id applies everywhere. A row whose bundle id
/// matches applies as well. Other apps' rows stay out. The replacer, not
/// this query, decides which of two matching rows wins.
public protocol DictionaryRepository: Sendable {
    func entries(forBundleID bundleID: String?) throws -> [DictionaryEntry]
}

public struct InMemoryDictionaryRepository: DictionaryRepository {
    private let stored: [DictionaryEntry]

    public init(_ entries: [DictionaryEntry] = []) {
        stored = entries
    }

    public func entries(forBundleID bundleID: String?) throws -> [DictionaryEntry] {
        stored.filter { DictionaryScope.applies($0, bundleID: bundleID) }
    }
}

/// Reads `dictionary` through `MinutaeCore`'s database and record types.
public struct DatabaseDictionaryRepository: DictionaryRepository {
    private let database: MinutaeDatabase

    public init(_ database: MinutaeDatabase) {
        self.database = database
    }

    public func entries(forBundleID bundleID: String?) throws -> [DictionaryEntry] {
        try Repository<DictionaryEntry>(database).all().filter { entry in
            DictionaryScope.applies(entry, bundleID: bundleID)
        }
    }
}

enum DictionaryScope {
    static func applies(_ entry: DictionaryEntry, bundleID: String?) -> Bool {
        guard entry.enabled else { return false }
        guard DictionaryKind(rawValue: entry.kind) != nil else { return false }
        guard !TranscriptText.normalizedPhrase(entry.term).isEmpty else { return false }
        guard let scope = entry.scope_bundle_id else { return true }
        return scope == bundleID
    }
}
