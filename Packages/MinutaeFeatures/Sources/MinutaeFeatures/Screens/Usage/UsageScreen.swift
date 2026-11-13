import Foundation
import MinutaeCore
import MinutaeDesign
import MinutaeLLM
import Observation
import SwiftUI

public struct UsageLine: Identifiable, Sendable {
    public let id: String
    public let calls: Int
    public let input: Int?
    public let output: Int?
    public let cost: Double?
}

@MainActor @Observable
public final class UsageViewModel {
    public var period = "Last 30 days"
    public var lines: [UsageLine] = []
    public var error = ""
    private let loadRows: @Sendable () throws -> [APIUsageLog]

    public init(loadRows: @escaping @Sendable () throws -> [APIUsageLog]) {
        self.loadRows = loadRows
    }
    public convenience init(database: MinutaeDatabase) {
        self.init { try Repository<APIUsageLog>(database).all() }
    }

    public func refresh(now: Date = Date(), timeZone: TimeZone = .current) {
        let window: UsageWindow =
            period == "Today" ? .today : (period == "Last 7 days" ? .last7Days : .last30Days)
        let interval = window.interval(now: now, timeZone: timeZone)
        let formatter = ISO8601DateFormatter()
        do {
            let rows = try loadRows().filter {
                guard let date = formatter.date(from: $0.created_at) else { return false }
                return date >= interval.start && date < interval.end
            }
            let groups = Dictionary(grouping: rows) { $0.transport ?? $0.provider }
            lines = groups.keys.sorted().map { key in
                let group = groups[key] ?? []
                let inputs = group.compactMap(\.input_tokens)
                let outputs = group.compactMap(\.output_tokens)
                let costs = group.compactMap(\.cost_usd)
                return UsageLine(
                    id: key, calls: group.count, input: inputs.isEmpty ? nil : inputs.reduce(0, +),
                    output: outputs.isEmpty ? nil : outputs.reduce(0, +),
                    cost: key == "api" && !costs.isEmpty ? costs.reduce(0, +) : nil)
            }
        } catch { self.error = error.localizedDescription }
    }
}

public struct UsageScreen: View {
    @State private var model: UsageViewModel
    public init(model: UsageViewModel) { _model = State(initialValue: model) }
    public var body: some View {
        @Bindable var model = model
        let peak = max(model.lines.map(\.calls).max() ?? 1, 1)
        PageScaffold("Usage") {
            SegmentedPills(options: ["Today", "Last 7 days", "Last 30 days"], selection: $model.period)
                .onChange(of: model.period) { _, _ in model.refresh() }
            Text("AI CALLS BY PROVIDER")
                .font(.minutae.mono(size: 12, weight: .medium)).foregroundStyle(Color.minutae.muted)
                .padding(.top, Spacing.s8)
            if model.lines.isEmpty {
                Text("No AI calls in this period.")
                    .font(.minutae.ui(size: 14)).foregroundStyle(Color.minutae.muted)
            }
            ForEach(model.lines) { line in
                VStack(alignment: .leading, spacing: Spacing.s8) {
                    HStack(alignment: .firstTextBaseline) {
                        Text(Formatting.provider(line.id)).font(.minutae.ui(size: 15, weight: .medium))
                        Spacer(minLength: Spacing.s16)
                        Text(Formatting.count(line.calls, "call"))
                            .font(.minutae.ui(size: 13)).foregroundStyle(Color.minutae.muted)
                    }
                    ProgressTrack(value: Double(line.calls) / Double(peak))
                    HStack(spacing: Spacing.s16) {
                        if let input = line.input { stat("\(Formatting.tokens(input)) in") }
                        if let output = line.output { stat("\(Formatting.tokens(output)) out") }
                        if let cost = line.cost { stat(String(format: "$%.4f", cost)) }
                        if line.cost == nil && line.id != "api" { stat("Included in your subscription") }
                    }
                    .padding(.bottom, Spacing.s4)
                    HairlineDivider()
                }
            }
            if !model.error.isEmpty { Text(model.error).foregroundStyle(Color.minutae.red) }
        }
        .task { model.refresh() }
    }

    private func stat(_ text: String) -> some View {
        Text(text).font(.minutae.mono(size: 12)).foregroundStyle(Color.minutae.muted)
    }
}

#Preview { UsageScreen(model: UsageViewModel(loadRows: { [] })) }
