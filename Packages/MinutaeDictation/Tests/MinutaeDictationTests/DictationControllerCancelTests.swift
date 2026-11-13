import Foundation
import Testing

@testable import MinutaeDictation

@MainActor
struct DictationControllerCancelTests {
    @Test func escapeWhileListeningKeepsTheWavAndCancelsTheRow() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.signal.send()
        #expect(await harness.waitStatus("cancelled"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "")
        #expect(row.final_text == "")
        #expect(row.error == nil)
        let audio = try #require(row.audio_path)
        #expect(audio.hasSuffix("/2026-04/dict-1.wav"))
        #expect(FileManager.default.fileExists(atPath: audio))
        #expect(harness.inserter.inputs.isEmpty)
        #expect(harness.speechGate.didEnter == false)
        #expect(harness.phases().contains(.error) == false)
        #expect(harness.lastHUD()?.phase == .idle)
        #expect(harness.lastHUD()?.banner == "cancelled")
    }

    @Test func cancelDuringTranscriptionKeepsAudioAndSkipsLaterStages() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        try await listenAndStop(harness)
        #expect(await harness.speechGate.waitEntered())
        await harness.controller.cancel()
        harness.speechGate.open()
        #expect(await harness.waitStatus("cancelled"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "")
        #expect(row.final_text == "")
        #expect(row.audio_path?.hasSuffix("/2026-04/dict-1.wav") == true)
        #expect(row.error == nil)
        #expect(harness.inserter.inputs.isEmpty)
        #expect(harness.phases().contains(.inserting) == false)
    }

    @Test func cancelDuringPostProcessKeepsRawText() async throws {
        var plan = DictationHarnessPlan()
        plan.speechOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        try await listenAndStop(harness)
        #expect(await harness.cleanGate.waitEntered())
        await harness.controller.cancel()
        harness.cleanGate.open()
        #expect(await harness.waitStatus("cancelled"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "hello world")
        #expect(row.final_text == "")
        #expect(row.audio_path != nil)
        #expect(harness.inserter.inputs.isEmpty)
    }

    @Test func cancelDuringInsertKeepsBothTexts() async throws {
        var plan = DictationHarnessPlan()
        plan.speechOpened = true
        plan.cleanOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        try await listenAndStop(harness)
        #expect(await harness.insertGate.waitEntered())
        await harness.controller.cancel()
        harness.insertGate.open()
        #expect(await harness.waitStatus("cancelled"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "hello world")
        #expect(row.final_text == "Hello world.")
        #expect(row.insert_method == nil)
        #expect(row.status == "cancelled")
        #expect(harness.phases().contains(.error) == false)
    }
}

@MainActor
private func listenAndStop(_ harness: DictationHarness) async throws {
    harness.press()
    #expect(await harness.waitStatus("listening"))
    harness.release()
}
