import Foundation
import Testing

@testable import MinutaeDictation

@MainActor
struct DictationControllerFailureTests {
    @Test func captureStartFailureStillInsertsAFailedRow() async throws {
        var plan = DictationHarnessPlan()
        plan.startError = SampleFailure(message: "mic-off")
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.audio_path == nil)
        #expect(row.raw_text == "")
        #expect(row.target_app_name == "TextEdit")
        #expect(row.error?.hasPrefix("capture start:") == true)
        #expect(row.error?.contains("mic-off") == true)
        #expect(harness.phases() == [.idle, .listening, .error, .idle])
    }

    @Test func captureStopFailureStillInsertsAFailedRow() async throws {
        var plan = DictationHarnessPlan()
        plan.stopError = SampleFailure(message: "stop-broke")
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.audio_path == nil)
        #expect(row.error?.hasPrefix("capture stop:") == true)
        #expect(row.error?.contains("stop-broke") == true)
    }

    @Test func wavFailureKeepsTheRowAndDuration() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        try FileManager.default.createDirectory(
            at: harness.audioDirectory,
            withIntermediateDirectories: true
        )
        let blocked = harness.audioDirectory.appendingPathComponent("2026-04")
        FileManager.default.createFile(atPath: blocked.path, contents: Data([0]))
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.audio_path == nil)
        #expect(row.duration_ms == 1_000)
        #expect(row.error?.hasPrefix("wav:") == true)
        #expect(harness.speechGate.didEnter == false)
    }

    @Test func transcriptionFailureKeepsTheWav() async throws {
        var plan = DictationHarnessPlan()
        plan.speechFails = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.speechGate.waitEntered())
        harness.speechGate.open()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "")
        #expect(row.audio_path?.hasSuffix("/2026-04/dict-1.wav") == true)
        #expect(row.error?.hasPrefix("transcribe:") == true)
        #expect(harness.inserter.inputs.isEmpty)
    }

    @Test func postProcessFailureKeepsRawText() async throws {
        var plan = DictationHarnessPlan()
        plan.cleanFails = true
        plan.speechOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.cleanGate.waitEntered())
        harness.cleanGate.open()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.raw_text == "hello world")
        #expect(row.final_text == "")
        #expect(row.audio_path != nil)
        #expect(row.error?.hasPrefix("post-process:") == true)
        #expect(harness.inserter.inputs.isEmpty)
    }

    @Test func insertFailureKeepsFinalText() async throws {
        var plan = DictationHarnessPlan()
        plan.insertion = .failed
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.final_text == "Hello world.")
        #expect(row.raw_text == "hello world")
        #expect(row.insert_method == "none")
        #expect(row.error == "insert: failed")
    }

    @Test func secureFieldRefusalIsAFailedRow() async throws {
        var plan = DictationHarnessPlan()
        plan.insertion = .refusedSecureField
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(await harness.waitStatus("failed"))
        let row = try harness.onlyRow()
        #expect(row.insert_method == "none")
        #expect(row.final_text == "Hello world.")
        #expect(row.error == "insert: secure field")
    }
}
