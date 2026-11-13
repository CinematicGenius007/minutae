import Foundation
import MinutaeCore
import Testing

@testable import MinutaeDictation

@MainActor
struct DictationOutcomeTests {
    private func openedPlan() -> DictationHarnessPlan {
        var plan = DictationHarnessPlan()
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        return plan
    }

    @Test func insertedRunEndsWithInsertedOutcome() async throws {
        let harness = try await DictationHarness.open(openedPlan())
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        #expect(harness.lastHUD()?.outcome == nil)
        harness.release()
        #expect(await harness.waitStatus("done"))
        #expect(
            await harness.waitUntil("inserted outcome") {
                harness.lastHUD()?.phase == .idle && harness.lastHUD()?.outcome == .inserted
            })
        #expect(harness.lastHUD()?.targetAppName == "TextEdit")
    }

    @Test func clipboardRunEndsWithCopiedOutcome() async throws {
        var plan = openedPlan()
        plan.insertion = .copiedToClipboard(.accessibilityNotTrusted)
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(
            await harness.waitUntil("copied outcome") {
                harness.lastHUD()?.outcome == .copiedToClipboard
            })
    }

    @Test func emptyTextEndsWithNothingHeard() async throws {
        var plan = openedPlan()
        plan.cleaned = " "
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(
            await harness.waitUntil("nothing heard") {
                harness.lastHUD()?.outcome == .nothingHeard
            })
    }

    @Test func failureMarksErrorAndIdleAsFailed() async throws {
        var plan = openedPlan()
        plan.speechFails = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(await harness.waitStatus("failed"))
        #expect(
            await harness.waitUntil("failed idle") {
                harness.lastHUD()?.phase == .idle && harness.lastHUD()?.outcome == .failed
            })
        let errors = harness.log.snapshot().compactMap { event -> DictationHUDState? in
            if case .state(let state) = event, state.phase == .error { return state }
            return nil
        }
        #expect(errors.last?.outcome == .failed)
    }

    @Test func toggleFromAppStartsAndStopsADictation() async throws {
        let harness = try await DictationHarness.open(openedPlan())
        defer { harness.cleanup() }
        await harness.controller.toggleFromApp()
        #expect(await harness.waitStatus("listening"))
        await harness.controller.toggleFromApp()
        #expect(await harness.waitStatus("done"))
        #expect(try harness.dictations().count == 1)
    }

    @Test func speechModelNameIsStoredOnTheRow() async throws {
        let paths = try DictationPaths.make()
        defer { try? FileManager.default.removeItem(at: paths.root) }
        let store = DatabaseDictationStore(database: paths.database)
        let row = Dictation(
            id: "a", started_at: "2026-04-11T15:00:00Z", duration_ms: nil,
            target_app_bundle_id: nil, target_app_name: nil, raw_text: "", final_text: "",
            language: nil, insert_method: nil, status: "listening", audio_path: nil, error: nil,
            created_at: "2026-04-11T15:00:00Z", speech_model: "small")
        try store.insert(row)
        #expect(try Repository<Dictation>(paths.database).find("a")?.speech_model == "small")
    }
}
