import MinutaeCore
import Testing

@testable import MinutaeFeatures

@MainActor
@Suite struct SettingsApplyTests {
    private final class Log { var applied: [AppSettings] = []; var installed: Set<String> = [] }

    @Test func savingAppliesSettingsAndModelReadyFollowsInstalledFiles() async throws {
        let log = Log()
        log.installed = ["base"]
        let model = SettingsViewModel(
            store: InMemorySettingsStore(),
            applySettings: { settings in
                log.applied.append(settings)
                return []
            },
            isModelInstalled: { log.installed.contains($0) })
        await model.load()
        #expect(model.modelReady == (log.installed.contains(model.settings.whisperModelName)))
        model.settings.whisperModelName = "small"
        await model.save()
        #expect(log.applied.last?.whisperModelName == "small")
        model.refreshModelReady()
        #expect(!model.modelReady)
        log.installed.insert("small")
        model.refreshModelReady()
        #expect(model.modelReady)
    }
}
