import Foundation
import MinutaeCore
import MinutaeLibrary
import os

/// Archive kept in memory, for previews and tests. Deletes never touch the disk.
public final class InMemoryDictationArchive: DictationArchiving, Sendable {
    private enum Listener {
        case recent(Int, AsyncThrowingStream<[Dictation], any Error>.Continuation)
        case one(String, AsyncThrowingStream<Dictation?, any Error>.Continuation)
    }

    private struct State {
        var rows: [String: Dictation] = [:]
        var listeners: [UUID: Listener] = [:]
        var failure: DictationArchiveError?
    }

    private let state = OSAllocatedUnfairLock(initialState: State())
    private let editedAt: String

    public init(_ rows: [Dictation] = [], editedAt: String = "2026-11-13T15:00:00Z") {
        self.editedAt = editedAt
        state.withLock { state in
            for row in rows { state.rows[row.id] = row }
        }
    }

    public var all: [Dictation] {
        state.withLock { Self.sorted(Array($0.rows.values)) }
    }

    /// Inserts or replaces a row and notifies observers.
    public func put(_ row: Dictation) {
        state.withLock { $0.rows[row.id] = row }
        notify()
    }

    /// Makes the next write throw `error`.
    public func failNextWrite(with error: DictationArchiveError) {
        state.withLock { $0.failure = error }
    }

    public func observeRecent(limit: Int) -> AsyncThrowingStream<[Dictation], any Error> {
        let (stream, continuation) = AsyncThrowingStream<[Dictation], any Error>.makeStream()
        let key = UUID()
        let current = state.withLock { state -> [Dictation] in
            state.listeners[key] = .recent(limit, continuation)
            return Array(Self.sorted(Array(state.rows.values)).prefix(limit))
        }
        continuation.yield(current)
        continuation.onTermination = { [weak self] _ in
            self?.state.withLock { _ = $0.listeners.removeValue(forKey: key) }
        }
        return stream
    }

    public func observe(id: String) -> AsyncThrowingStream<Dictation?, any Error> {
        let (stream, continuation) = AsyncThrowingStream<Dictation?, any Error>.makeStream()
        let key = UUID()
        let current = state.withLock { state -> Dictation? in
            state.listeners[key] = .one(id, continuation)
            return state.rows[id]
        }
        continuation.yield(current)
        continuation.onTermination = { [weak self] _ in
            self?.state.withLock { _ = $0.listeners.removeValue(forKey: key) }
        }
        return stream
    }

    public func dictation(id: String) throws -> Dictation? {
        state.withLock { $0.rows[id] }
    }

    @discardableResult
    public func saveEdit(id: String, text: String) throws -> Dictation? {
        let stamp = editedAt
        let saved = try state.withLock { state -> Dictation in
            try Self.takeFailure(&state)
            guard var row = state.rows[id] else { throw DictationArchiveError.notFound }
            guard !row.isInProgress else { throw DictationArchiveError.inProgress }
            let edited = text == row.originalText ? nil : text
            row.edited_text = edited
            row.edited_at = edited == nil ? nil : stamp
            state.rows[id] = row
            return row
        }
        notify()
        return saved
    }

    public func delete(id: String) throws -> DictationDeletion {
        let result = try state.withLock { state -> DictationDeletion in
            try Self.takeFailure(&state)
            guard let row = state.rows[id] else { throw DictationArchiveError.notFound }
            guard !row.isInProgress else { throw DictationArchiveError.inProgress }
            state.rows[id] = nil
            return DictationDeletion(
                deletedRows: 1, deletedAudioFiles: row.audio_path == nil ? 0 : 1)
        }
        notify()
        return result
    }

    public func deleteAll() throws -> DictationDeletion {
        let result = try state.withLock { state -> DictationDeletion in
            try Self.takeFailure(&state)
            let finished = state.rows.values.filter { !$0.isInProgress }
            for row in finished { state.rows[row.id] = nil }
            return DictationDeletion(
                deletedRows: finished.count,
                deletedAudioFiles: finished.filter { $0.audio_path != nil }.count)
        }
        notify()
        return result
    }

    private func notify() {
        let (rows, listeners) = state.withLock { ($0.rows, Array($0.listeners.values)) }
        let sorted = Self.sorted(Array(rows.values))
        for listener in listeners {
            switch listener {
            case .recent(let limit, let continuation):
                continuation.yield(Array(sorted.prefix(limit)))
            case .one(let id, let continuation):
                continuation.yield(rows[id])
            }
        }
    }

    private static func takeFailure(_ state: inout State) throws {
        if let failure = state.failure {
            state.failure = nil
            throw failure
        }
    }

    private static func sorted(_ rows: [Dictation]) -> [Dictation] {
        rows.sorted { left, right in
            if left.started_at != right.started_at { return left.started_at > right.started_at }
            return left.id > right.id
        }
    }
}
