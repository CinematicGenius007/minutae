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
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.s20) {
                Text("Usage").font(.minutae.display(size: 36))
                SegmentedPills(
                    options: ["Today", "Last 7 days", "Last 30 days"], selection: $model.period
                )
                .onChange(of: model.period) { _, _ in model.refresh() }
                Text("AI CALLS BY PROVIDER").font(.minutae.mono(size: 12)).foregroundStyle(
                    Color.minutae.muted)
                ForEach(model.lines) { line in
                    VStack(alignment: .leading, spacing: Spacing.s8) {
                        HStack {
                            Text(line.id).font(.minutae.ui(size: 15, weight: .medium))
                            Spacer()
                            Text(
                                "\(line.calls) calls · \(line.input.map(String.init) ?? "—") in · \(line.output.map(String.init) ?? "—") out · \(line.cost.map { String(format: "$%.4f", $0) } ?? "—")"
                            )
                            .font(.minutae.mono(size: 12))
                        }
                        ProgressTrack(
                            value: Double(line.calls)
                                / Double(max(model.lines.map(\.calls).max() ?? 1, 1)))
                        HairlineDivider()
                    }
                }
                if model.lines.isEmpty {
                    Text("No calls in this period.").foregroundStyle(Color.minutae.muted)
                }
                if !model.error.isEmpty { Text(model.error).foregroundStyle(Color.minutae.red) }
            }.frame(maxWidth: 680, alignment: .leading).padding(Spacing.s28)
        }.foregroundStyle(Color.minutae.ink).background(Color.minutae.bg).task { model.refresh() }
    }
}

#Preview { UsageScreen(model: UsageViewModel(loadRows: { [] })) }
