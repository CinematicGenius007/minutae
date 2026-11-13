import Foundation
import GRDB
import MinutaeCore

/// Stored dictations for the Dictate screens: list, edit, delete, and export.
///
/// An edit writes `edited_text` and `edited_at` only. `raw_text`, `final_text`, and the WAV file
/// are never changed by an edit.
public protocol DictationArchiving: Sendable {
    /// Newest first, at most `limit` rows. The first value is the current state.
    func observeRecent(limit: Int) -> AsyncThrowingStream<[Dictation], any Error>
    /// The row, then every change to it. Nil when the row is deleted.
    func observe(id: String) -> AsyncThrowingStream<Dictation?, any Error>
    func dictation(id: String) throws -> Dictation?
    /// Saves `text` as the edit. Text equal to the original clears the edit.
    @discardableResult func saveEdit(id: String, text: String) throws -> Dictation?
    /// Deletes the row and its WAV. Rows that are still recording or processing are refused.
    func delete(id: String) throws -> DictationDeletion
    /// Deletes every finished row and its WAV. Rows still in progress are kept.
    func deleteAll() throws -> DictationDeletion
}

/// What a delete removed. `audioFailures` lists WAV paths that could not be removed.
public struct DictationDeletion: Sendable, Equatable {
    public var deletedRows: Int
    public var deletedAudioFiles: Int
    public var audioFailures: [String]

    public init(deletedRows: Int = 0, deletedAudioFiles: Int = 0, audioFailures: [String] = []) {
        self.deletedRows = deletedRows
        self.deletedAudioFiles = deletedAudioFiles
        self.audioFailures = audioFailures
    }
}

public enum DictationArchiveError: Error, Equatable, Sendable {
    case notFound
    case inProgress
}

extension Dictation {
    /// Listening, processing, and inserting rows are still owned by `DictationController`.
    public var isInProgress: Bool {
        ["listening", "processing", "inserting"].contains(status)
    }

    /// The text the edit is compared against: the inserted text, else the raw transcript.
    public var originalText: String {
        final_text.isEmpty ? raw_text : final_text
    }
}

public struct DatabaseDictationArchive: DictationArchiving {
    private let database: MinutaeDatabase
    private let audioRoot: URL
    private let now: @Sendable () -> Date

    /// `audioRoot` is the dictations folder. A WAV outside it is never deleted.
    public init(
        database: MinutaeDatabase,
        audioRoot: URL,
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.database = database
        self.audioRoot = audioRoot
        self.now = now
    }

    public func observeRecent(limit: Int) -> AsyncThrowingStream<[Dictation], any Error> {
        let observation = ValueObservation.tracking { connection in
            try Dictation.order(Column("started_at").desc, Column("id").desc)
                .limit(limit)
                .fetchAll(connection)
        }
        return Self.stream(observation, in: database.pool)
    }

    public func observe(id: String) -> AsyncThrowingStream<Dictation?, any Error> {
        let observation = ValueObservation.tracking { connection in
            try Dictation.fetchOne(connection, key: id)
        }
        return Self.stream(observation, in: database.pool)
    }

    public func dictation(id: String) throws -> Dictation? {
        try database.pool.read { connection in try Dictation.fetchOne(connection, key: id) }
    }

    @discardableResult
    public func saveEdit(id: String, text: String) throws -> Dictation? {
        let stamp = DictationTimestamp.string(now())
        return try database.pool.write { connection in
            guard let row = try Dictation.fetchOne(connection, key: id) else {
                throw DictationArchiveError.notFound
            }
            guard !row.isInProgress else { throw DictationArchiveError.inProgress }
            let edited: String? = text == row.originalText ? nil : text
            try connection.execute(
                sql: "UPDATE dictations SET edited_text = ?, edited_at = ? WHERE id = ?",
                arguments: [edited, edited == nil ? nil : stamp, id]
            )
            return try Dictation.fetchOne(connection, key: id)
        }
    }

    public func delete(id: String) throws -> DictationDeletion {
        guard let row = try dictation(id: id) else { throw DictationArchiveError.notFound }
        guard !row.isInProgress else { throw DictationArchiveError.inProgress }
        return try remove([row])
    }

    public func deleteAll() throws -> DictationDeletion {
        let rows = try database.pool.read { connection in try Dictation.fetchAll(connection) }
        return try remove(rows.filter { !$0.isInProgress })
    }

    private func remove(_ rows: [Dictation]) throws -> DictationDeletion {
        guard !rows.isEmpty else { return DictationDeletion() }
        let ids = rows.map(\.id)
        let deleted = try database.pool.write { connection -> Int in
            var count = 0
            for id in ids {
                // Same cleanup as `Repository.delete`: tags and jobs have no foreign key.
                try connection.execute(
                    sql:
                        "DELETE FROM item_tags WHERE subject_type = 'dictation' AND subject_id = ?",
                    arguments: [id])
                try connection.execute(
                    sql: "DELETE FROM jobs WHERE subject_type = 'dictation' AND subject_id = ?",
                    arguments: [id])
                if try Dictation.deleteOne(connection, key: id) { count += 1 }
            }
            return count
        }
        var result = DictationDeletion(deletedRows: deleted)
        for path in rows.compactMap(\.audio_path) {
            switch removeAudio(at: path) {
            case .removed: result.deletedAudioFiles += 1
            case .missing: break
            case .failed: result.audioFailures.append(path)
            }
        }
        return result
    }

    private enum AudioRemoval { case removed, missing, failed }

    private func removeAudio(at path: String) -> AudioRemoval {
        let url = URL(fileURLWithPath: path)
        guard DictationAudioLocation.isInside(url, root: audioRoot) else { return .failed }
        guard FileManager.default.fileExists(atPath: url.path) else { return .missing }
        do {
            try FileManager.default.removeItem(at: url)
            return .removed
        } catch {
            return .failed
        }
    }

    private static func stream<Value: Sendable>(
        _ observation: ValueObservation<ValueReducers.Fetch<Value>>,
        in pool: DatabasePool
    ) -> AsyncThrowingStream<Value, any Error> {
        AsyncThrowingStream { continuation in
            let cancellable = observation.start(
                in: pool,
                scheduling: .async(onQueue: DispatchQueue(label: "minutae.dictation-archive")),
                onError: { error in continuation.finish(throwing: error) },
                onChange: { value in continuation.yield(value) }
            )
            continuation.onTermination = { _ in cancellable.cancel() }
        }
    }
}

enum DictationAudioLocation {
    /// True when `url` resolves to a `.wav` file below `root`.
    static func isInside(_ url: URL, root: URL) -> Bool {
        guard url.pathExtension.lowercased() == "wav" else { return false }
        let file = url.standardizedFileURL.resolvingSymlinksInPath().path
        var base = root.standardizedFileURL.resolvingSymlinksInPath().path
        if !base.hasSuffix("/") { base += "/" }
        return file.hasPrefix(base)
    }
}

enum DictationTimestamp {
    static func string(_ date: Date) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter.string(from: date)
    }

    static func date(_ string: String) -> Date? {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        if let date = formatter.date(from: string) { return date }
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: string)
    }
}
