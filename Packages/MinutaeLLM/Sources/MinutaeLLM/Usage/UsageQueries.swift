import Foundation
import MinutaeCore

/// Inclusive start, exclusive end.
public struct UsageInterval: Sendable, Equatable {
    public var start: Date
    public var end: Date

    public init(start: Date, end: Date) {
        self.start = start
        self.end = end
    }
}

/// Calendar-day presets include today. `end` is the next local midnight.
public enum UsageWindow: Sendable, Equatable {
    case range(start: Date, end: Date)
    case today
    case last7Days
    case last30Days

    public func interval(now: Date, timeZone: TimeZone) -> UsageInterval {
        switch self {
        case .range(let start, let end):
            return UsageInterval(start: start, end: end)
        case .today:
            return Self.trailingDays(1, now: now, timeZone: timeZone)
        case .last7Days:
            return Self.trailingDays(7, now: now, timeZone: timeZone)
        case .last30Days:
            return Self.trailingDays(30, now: now, timeZone: timeZone)
        }
    }

    private static func trailingDays(_ count: Int, now: Date, timeZone: TimeZone) -> UsageInterval {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = timeZone
        let startOfToday = calendar.startOfDay(for: now)
        let days = max(count, 1)
        let start = calendar.date(byAdding: .day, value: 1 - days, to: startOfToday) ?? startOfToday
        let end = calendar.date(byAdding: .day, value: 1, to: startOfToday) ?? startOfToday
        return UsageInterval(start: start, end: end)
    }
}

public struct UsageErrorCount: Sendable, Equatable {
    public var errorClass: String
    public var count: Int

    public init(errorClass: String, count: Int) {
        self.errorClass = errorClass
        self.count = count
    }
}

public struct TransportUsage: Sendable, Equatable {
    public var transport: String
    public var callCount: Int
    public var successCount: Int
    /// Successes divided by calls. A success is a row whose `error_class` is null.
    public var successRate: Double
    public var averageDurationMs: Double?
    public var inputTokens: Int
    public var outputTokens: Int
    /// Sum of known `api` costs. Nil when this transport has no stored cost.
    public var costUSD: Double?
    public var errors: [UsageErrorCount]

    public init(
        transport: String,
        callCount: Int,
        successCount: Int,
        successRate: Double,
        averageDurationMs: Double?,
        inputTokens: Int,
        outputTokens: Int,
        costUSD: Double?,
        errors: [UsageErrorCount]
    ) {
        self.transport = transport
        self.callCount = callCount
        self.successCount = successCount
        self.successRate = successRate
        self.averageDurationMs = averageDurationMs
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.costUSD = costUSD
        self.errors = errors
    }
}

public struct UsageReport: Sendable, Equatable {
    public var transports: [TransportUsage]
    public var inputTokens: Int
    public var outputTokens: Int

    public init(transports: [TransportUsage], inputTokens: Int, outputTokens: Int) {
        self.transports = transports
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
    }
}

/// Aggregates for the Usage page. Token totals count only values the transport reported.
public struct UsageQueries: Sendable {
    private let database: MinutaeDatabase

    public init(_ database: MinutaeDatabase) {
        self.database = database
    }

    public func summary(
        _ window: UsageWindow,
        now: Date = Date(),
        timeZone: TimeZone = .current
    ) throws -> UsageReport {
        let interval = window.interval(now: now, timeZone: timeZone)
        let start = UsageTimestamp.string(from: interval.start)
        let end = UsageTimestamp.string(from: interval.end)
        let rows = try Repository<APIUsageLog>(database).all().filter { row in
            row.created_at >= start && row.created_at < end
        }
        return Self.summarize(rows)
    }

    private static func summarize(_ rows: [APIUsageLog]) -> UsageReport {
        let grouped = Dictionary(grouping: rows, by: transportID)
        let transports = grouped.keys.sorted().map { key in
            transportUsage(key, rows: grouped[key] ?? [])
        }
        return UsageReport(
            transports: transports,
            inputTokens: rows.compactMap(\.input_tokens).reduce(0, +),
            outputTokens: rows.compactMap(\.output_tokens).reduce(0, +)
        )
    }

    private static func transportID(_ row: APIUsageLog) -> String {
        if let transport = row.transport, !transport.isEmpty { return transport }
        return row.provider
    }

    private static func transportUsage(_ transport: String, rows: [APIUsageLog]) -> TransportUsage {
        let calls = rows.count
        let successes = rows.filter { $0.error_class == nil }.count
        let durations = rows.compactMap(\.duration_ms)
        let average = averageDuration(durations)
        let costs = rows.compactMap(\.cost_usd)
        var counts: [String: Int] = [:]
        for row in rows {
            if let name = row.error_class {
                counts[name, default: 0] += 1
            }
        }
        let errors = counts.keys.sorted().map { name in
            UsageErrorCount(errorClass: name, count: counts[name] ?? 0)
        }
        let rate = calls == 0 ? 0 : Double(successes) / Double(calls)
        return TransportUsage(
            transport: transport,
            callCount: calls,
            successCount: successes,
            successRate: rate,
            averageDurationMs: average,
            inputTokens: rows.compactMap(\.input_tokens).reduce(0, +),
            outputTokens: rows.compactMap(\.output_tokens).reduce(0, +),
            costUSD: costs.isEmpty ? nil : costs.reduce(0, +),
            errors: errors
        )
    }

    private static func averageDuration(_ durations: [Int]) -> Double? {
        guard !durations.isEmpty else { return nil }
        let total = durations.reduce(Int64(0)) { $0 + Int64($1) }
        return Double(total) / Double(durations.count)
    }
}
