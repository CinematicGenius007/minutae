import Foundation
import GRDB

public enum JobKind: String, CaseIterable, Sendable {
    case transcribe, summarise, embed, export, retention
    public var concurrency: Int {
        switch self { case .summarise, .export: 2; default: 1 }
    }
    public var maxAttempts: Int {
        switch self { case .transcribe: 2; case .retention: 1; default: 3 }
    }
}

public struct NonRetryableJobError: Error, Sendable {
    public let message: String
    public init(_ message: String) { self.message = message }
}

public typealias JobHandler = @Sendable (Job) async throws -> Void

public actor JobRunner {
    private let database: MinutaeDatabase
    private let handlers: [JobKind: JobHandler]
    private let limits: [JobKind: Int]
    private let delay: @Sendable (Int) -> TimeInterval
    private let clock: @Sendable () -> Date
    private var pending: [String: (Job, String, String?, String?)] = [:]
    private var errorObservers: [UUID: AsyncStream<String>.Continuation] = [:]
    private var tasks: [String: Task<Void, Never>] = [:]
    private var scheduler: Task<Void, Never>?

    public init(database: MinutaeDatabase, handlers: [JobKind: JobHandler], limits: [JobKind: Int] = [:], clock: @escaping @Sendable () -> Date = { Date() }, delay: @escaping @Sendable (Int) -> TimeInterval = { attempt in
        min(30 * pow(2, Double(max(0, attempt - 1))), 900) * Double.random(in: 0.9...1.1)
    }) {
        self.database = database
        self.handlers = handlers
        self.limits = limits
        self.delay = delay
        self.clock = clock
    }

    public func start() throws {
        guard scheduler == nil else { return }
        try database.pool.write { db in
            try db.execute(sql: "UPDATE jobs SET state = 'queued', attempts = MAX(attempts - 1, 0) WHERE state = 'running'")
        }
        scheduler = Task { [weak self] in
            while !Task.isCancelled {
                await self?.poll()
                try? await Task.sleep(for: .milliseconds(200))
            }
        }
    }

    public func stop() {
        scheduler?.cancel()
        scheduler = nil
        for task in tasks.values { task.cancel() }
    }

    public func enqueue(kind: JobKind, subjectType: String, subjectID: String, payload: String? = nil) throws -> String {
        let id = UUID().uuidString.lowercased()
        let now = Self.timestamp(clock())
        let job = Job(id: id, kind: kind.rawValue, subject_type: subjectType, subject_id: subjectID, payload: payload, state: "queued", attempts: 0, max_attempts: kind.maxAttempts, last_error: nil, run_after: nil, created_at: now, updated_at: now)
        try database.pool.write { try job.insert($0) }
        return id
    }

    public func cancel(_ id: String) throws {
        try database.pool.write { db in
            try db.execute(sql: "UPDATE jobs SET state = 'cancelled', updated_at = ? WHERE id = ? AND state IN ('queued', 'running')", arguments: [Self.timestamp(clock()), id])
        }
        tasks[id]?.cancel()
    }

    public nonisolated func observeJobs() -> ValueObservation<ValueReducers.Fetch<[Job]>> {
        ValueObservation.tracking { try Job.order(Column("created_at")).fetchAll($0) }
    }

    public func databaseErrors() -> AsyncStream<String> {
        let id = UUID()
        return AsyncStream { continuation in
            errorObservers[id] = continuation
            continuation.onTermination = { [weak self] _ in
                Task { await self?.removeObserver(id) }
            }
        }
    }

    private func removeObserver(_ id: UUID) { errorObservers[id] = nil }
    private func report(_ error: any Error) {
        for observer in errorObservers.values { observer.yield(String(describing: error)) }
    }

    private func poll() {
        for id in Array(pending.keys) { persistResult(id) }
        do {
            let jobs = try database.pool.read { db in
                try Job.filter(Column("state") == "queued").order(Column("created_at")).fetchAll(db)
            }
            for job in jobs {
                guard let kind = JobKind(rawValue: job.kind), let handler = handlers[kind], tasks[job.id] == nil else { continue }
                let count = tasks.keys.filter { id in jobsRunningKinds[id] == kind }.count
                guard count < max(0, limits[kind] ?? kind.concurrency) else { continue }
                if let after = job.run_after {
                    guard let date = Self.parseTimestamp(after), date <= clock() else { continue }
                }
                if job.attempts >= job.max_attempts {
                    try database.pool.write { db in
                        try db.execute(sql: "UPDATE jobs SET state = 'failed', updated_at = ? WHERE id = ? AND state = 'queued'", arguments: [Self.timestamp(clock()), job.id])
                    }
                    continue
                }
                let claimed = try database.pool.write { db -> Bool in
                    try db.execute(sql: "UPDATE jobs SET state = 'running', attempts = attempts + 1, updated_at = ? WHERE id = ? AND state = 'queued'", arguments: [Self.timestamp(clock()), job.id])
                    return db.changesCount == 1
                }
                guard claimed else { continue }
                var running = job
                running.state = "running"
                running.attempts += 1
                let claimedJob = running
                jobsRunningKinds[job.id] = kind
                tasks[job.id] = Task {
                    do {
                        try await handler(claimedJob)
                        self.finish(claimedJob, error: nil)
                    } catch {
                        self.finish(claimedJob, error: error)
                    }
                }
            }
        } catch {
            report(error)
        }
    }

    private var jobsRunningKinds: [String: JobKind] = [:]

    private func finish(_ job: Job, error: (any Error)?) {
        let state: String
        let message: String?
        var after: String?
        if let error {
            message = (error as? NonRetryableJobError)?.message ?? String(describing: error)
            if error is CancellationError { state = "cancelled" }
            else if error is NonRetryableJobError || job.attempts >= job.max_attempts { state = "failed" }
            else { state = "queued"; after = Self.timestamp(clock().addingTimeInterval(max(0, delay(job.attempts)))) }
        } else { state = "succeeded"; message = nil }
        pending[job.id] = (job, state, message, after)
        persistResult(job.id)
    }

    private func persistResult(_ id: String) {
        guard let (_, state, message, after) = pending[id] else { return }
        do {
            try database.pool.write { db in
                try db.execute(sql: "UPDATE jobs SET state = ?, last_error = ?, run_after = ?, updated_at = ? WHERE id = ? AND state = 'running'", arguments: [state, message, after, Self.timestamp(clock()), id])
            }
            pending[id] = nil
            tasks[id] = nil
            jobsRunningKinds[id] = nil
        } catch {
            report(error)
        }
    }

    private nonisolated static func parseTimestamp(_ value: String) -> Date? {
        let formatter = ISO8601DateFormatter()
        if let date = formatter.date(from: value) { return date }
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter.date(from: value)
    }

    private nonisolated static func timestamp(_ date: Date) -> String {
        ISO8601DateFormatter().string(from: date)
    }
}
