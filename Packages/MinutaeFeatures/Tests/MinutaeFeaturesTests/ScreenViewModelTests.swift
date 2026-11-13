import Foundation
import MinutaeCore
import Testing

@testable import MinutaeFeatures

@Test @MainActor func settingsPersist() async throws {
    let store = InMemorySettingsStore()
    let model = SettingsViewModel(store: store)
    await model.load()
    model.settings.keepDictationAudio = false
    await model.save()
    #expect(try await store.load().keepDictationAudio == false)
}

@Test @MainActor func usagePreservesUnknownValues() {
    let row = APIUsageLog(
        id: "1", meeting_id: nil, provider: "codex", model: "m", endpoint: "test",
        input_tokens: nil, output_tokens: nil, cost_usd: nil, created_at: "2026-11-13T12:00:00Z",
        transport: "codexCLI", duration_ms: nil, error_class: nil)
    let model = UsageViewModel(loadRows: { [row] })
    model.refresh(
        now: ISO8601DateFormatter().date(from: "2026-11-13T14:00:00Z")!,
        timeZone: TimeZone(secondsFromGMT: 0)!)
    #expect(model.lines.count == 1)
    #expect(model.lines.first?.input == nil)
    #expect(model.lines.first?.cost == nil)
}
