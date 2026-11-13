import Foundation
import MinutaeCore

/// One router attempt. Prompt, transcript, and output text are not fields here.
public struct UsageEvent: Sendable, Equatable {
    public var transport: TransportID
    public var purpose: Purpose
    public var model: String?
    public var usage: Usage?
    public var duration: Duration
    public var errorClass: String?
    public var recordedAt: Date

    public init(
        transport: TransportID,
        purpose: Purpose,
        model: String? = nil,
        usage: Usage? = nil,
        duration: Duration,
        errorClass: String? = nil,
        recordedAt: Date = Date()
    ) {
        self.transport = transport
        self.purpose = purpose
        self.model = model
        self.usage = usage
        self.duration = duration
        self.errorClass = errorClass
        self.recordedAt = recordedAt
    }
}

/// Writes one `api_usage_log` row per attempt. Dollar cost is kept only for `api`.
public actor DatabaseUsageObserver: UsageObserver {
    private let database: MinutaeDatabase
    private let clock: @Sendable () -> Date

    public init(database: MinutaeDatabase, clock: @escaping @Sendable () -> Date = { Date() }) {
        self.database = database
        self.clock = clock
    }

    public func record(_ event: UsageEvent) async throws {
        let row = APIUsageLog(
            id: UUID().uuidString.lowercased(),
            meeting_id: nil,
            provider: event.transport.rawValue,
            model: event.model ?? "",
            endpoint: event.purpose.rawValue,
            input_tokens: event.usage?.inputTokens,
            output_tokens: event.usage?.outputTokens,
            cost_usd: event.transport == .api ? event.usage?.costUSD : nil,
            created_at: UsageTimestamp.string(from: event.recordedAt),
            transport: event.transport.rawValue,
            duration_ms: UsageDuration.milliseconds(event.duration),
            error_class: event.errorClass
        )
        try Repository<APIUsageLog>(database).save(row)
    }

    public func record(
        transport: TransportID,
        purpose: Purpose,
        duration: Duration,
        errorClass: String?
    ) async {
        _ = try? await record(
            UsageEvent(
                transport: transport,
                purpose: purpose,
                duration: duration,
                errorClass: errorClass,
                recordedAt: clock()
            ))
    }
}

enum UsageTimestamp {
    static func string(from date: Date) -> String {
        ISO8601DateFormatter().string(from: date)
    }
}

enum UsageDuration {
    static func milliseconds(_ duration: Duration) -> Int {
        let (seconds, attoseconds) = duration.components
        let scaled = seconds.multipliedReportingOverflow(by: 1_000)
        let fraction = attoseconds / 1_000_000_000_000_000
        let total = scaled.partialValue.addingReportingOverflow(fraction)
        guard !scaled.overflow, !total.overflow else {
            return seconds < 0 ? 0 : Int.max
        }
        if total.partialValue > Int64(Int.max) { return Int.max }
        if total.partialValue < 0 { return 0 }
        return Int(total.partialValue)
    }
}
