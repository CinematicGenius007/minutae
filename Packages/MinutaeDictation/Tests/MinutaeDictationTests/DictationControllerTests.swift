import Foundation
import MinutaeCore
import Testing

@testable import MinutaeDictation

@MainActor
struct DictationControllerTests {
    @Test func holdRunPersistsEachStageAndWritesMonthWav() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        let listening = try harness.onlyRow()
        #expect(listening.id == "dict-1")
        #expect(listening.target_app_bundle_id == "com.apple.TextEdit")
        #expect(listening.target_app_name == "TextEdit")
        #expect(listening.raw_text == "")
        #expect(listening.audio_path == nil)

        harness.release()
        #expect(await harness.speechGate.waitEntered())
        let processing = try harness.onlyRow()
        #expect(processing.status == "processing")
        #expect(processing.duration_ms == 1_000)
        let audio = try #require(processing.audio_path)
        #expect(audio.hasPrefix(harness.audioDirectory.path))
        #expect(audio.hasSuffix("/2026-04/dict-1.wav"))
        let bytes = try Data(contentsOf: URL(fileURLWithPath: audio))
        #expect(bytes.prefix(4) == Data("RIFF".utf8))
        #expect(bytes.count == 44 + 16_000 * 4)

        harness.speechGate.open()
        #expect(await harness.cleanGate.waitEntered())
        #expect(try harness.onlyRow().raw_text == "hello world")
        #expect(try harness.onlyRow().language == "en")
        #expect(await harness.speech.sampleCount == 16_000)

        harness.cleanGate.open()
        #expect(await harness.insertGate.waitEntered())
        let inserting = try harness.onlyRow()
        #expect(inserting.status == "inserting")
        #expect(inserting.final_text == "Hello world.")

        harness.insertGate.open()
        #expect(await harness.waitStatus("done"))
        let done = try harness.onlyRow()
        #expect(done.insert_method == "pasteboard")
        #expect(done.error == nil)
        #expect(harness.inserter.inputs == ["Hello world."])
        #expect(harness.phases() == [.idle, .listening, .processing, .inserting, .idle])
    }

    @Test func toggleReleaseKeepsListeningUntilTheNextPress() async throws {
        var plan = DictationHarnessPlan()
        plan.mode = .toggle
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        try await Task.sleep(for: .milliseconds(100))
        #expect(harness.speechGate.didEnter == false)
        #expect(try harness.onlyRow().status == "listening")
        harness.press()
        #expect(await harness.speechGate.waitEntered())
        harness.releaseGates()
        #expect(await harness.waitStatus("done"))
        #expect(try harness.dictations().count == 1)
    }

    @Test func holdRepeatWhileListeningIsIgnored() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.press()
        #expect(await harness.waitIgnored())
        #expect(try harness.dictations().count == 1)
        #expect(try harness.onlyRow().status == "listening")
        harness.release()
        harness.releaseGates()
        #expect(await harness.waitStatus("done"))
    }

    @Test func aPressDuringProcessingDoesNotStartAnotherRow() async throws {
        let harness = try await DictationHarness.open()
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.speechGate.waitEntered())
        harness.press()
        #expect(await harness.waitIgnored())
        #expect(try harness.dictations().count == 1)
        harness.releaseGates()
        #expect(await harness.waitStatus("done"))
        harness.press()
        #expect(await harness.waitStatus("listening"))
        let rows = try harness.dictations()
        #expect(rows.map(\.id) == ["dict-1", "dict-2"])
        #expect(rows[0].status == "done")
    }

    @Test func frontmostAppIsTheAppAtPress() async throws {
        var plan = DictationHarnessPlan()
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.frontmost.target = DictationTarget(
            bundleIdentifier: "com.apple.mail",
            name: "Mail"
        )
        harness.release()
        #expect(await harness.waitStatus("done"))
        let row = try harness.onlyRow()
        #expect(row.target_app_bundle_id == "com.apple.TextEdit")
        #expect(row.target_app_name == "TextEdit")
    }

    @Test func switchingModeWhileListeningStopsTheDictation() async throws {
        var plan = DictationHarnessPlan()
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        try harness.hotkey.register(HotkeyRegistration(mode: .toggle, chord: .optionSpace))
        #expect(await harness.waitStatus("done"))
        #expect(try harness.onlyRow().insert_method == "pasteboard")
    }

    @Test func emptyCleanedTextIsStoredWithoutInsertion() async throws {
        var plan = DictationHarnessPlan()
        plan.cleaned = " \n"
        plan.speechOpened = true
        plan.cleanOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        #expect(await harness.waitStatus("listening"))
        harness.release()
        #expect(await harness.waitStatus("done"))
        let row = try harness.onlyRow()
        #expect(row.insert_method == "none")
        #expect(row.final_text == " \n")
        #expect(harness.inserter.inputs.isEmpty)
    }

    @Test func clipboardFallbackIsACompletedDictation() async throws {
        var plan = DictationHarnessPlan()
        plan.insertion = .copiedToClipboard(.noMethodWorked)
        plan.speechOpened = true
        plan.cleanOpened = true
        plan.insertOpened = true
        let harness = try await DictationHarness.open(plan)
        defer { harness.cleanup() }
        harness.press()
        harness.release()
        #expect(await harness.waitStatus("done"))
        #expect(try harness.onlyRow().insert_method == "clipboardOnly")
        #expect(harness.lastHUD()?.banner == "Copied to the clipboard.")
    }
}
