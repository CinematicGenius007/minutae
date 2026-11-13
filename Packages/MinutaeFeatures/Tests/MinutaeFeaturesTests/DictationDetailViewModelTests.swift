import Foundation
import MinutaeCore
import MinutaeLibrary
import Testing

@testable import MinutaeFeatures

@MainActor
struct DictationDetailViewModelTests {
    @MainActor
    struct Setup {
        let directory: URL
        let wav: URL
        let archive: InMemoryDictationArchive
        let clipboard = RecordingClipboard()
        let destination: ScriptedDestination
        let player = FakePlayer()
        let model: DictationDetailViewModel
        let runner: Task<Void, Never>

        init(
            status: String = "done", audio: Bool = true, saveDelay: Duration = .seconds(60)
        ) throws {
            directory = try Fixtures.temporaryDirectory()
            wav = directory.appendingPathComponent("a.wav")
            try Data("RIFF-original".utf8).write(to: wav)
            archive = InMemoryDictationArchive([
                Fixtures.row("a", status: status, audioPath: audio ? wav.path : nil)
            ])
            destination = ScriptedDestination(url: nil)
            let player = player
            let model = DictationDetailViewModel(
                id: "a", archive: archive, clipboard: clipboard, destination: destination,
                makePlayer: { _ in player }, formatter: Fixtures.formatter, now: Fixtures.now,
                saveDelay: saveDelay)
            self.model = model
            runner = Task { await model.run() }
        }

        func loaded() async -> Bool {
            await eventually("loaded") { model.dictation != nil }
        }

        func cleanup() {
            runner.cancel()
            try? FileManager.default.removeItem(at: directory)
        }
    }

    @Test func headerAndPlayerMatchTheDesign() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        #expect(setup.model.title == "Dictation · 2:41 PM")
        #expect(setup.model.meta == "Today · 6 s · pasted into Slack · whisper small")
        #expect(setup.model.draft == "Remind me to send Priya the mockups.")
        #expect(setup.model.playback.isAvailable)
        setup.player.currentTime = 2.4
        setup.model.playback.sync()
        #expect(setup.model.timeLabel == "0:02 / 0:06")
        #expect(setup.model.isEditable)
        #expect(setup.model.canExportAudio)
    }

    @Test func editsSaveAfterThePauseAndKeepTheOriginal() async throws {
        let setup = try Setup(saveDelay: .milliseconds(10))
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        setup.model.updateDraft("Remind me to send Priya the final mockups.")
        #expect(setup.model.saveState == .pending)
        #expect(await eventually("saved") { setup.model.saveState == .saved })
        let stored = try #require(try setup.archive.dictation(id: "a"))
        #expect(stored.edited_text == "Remind me to send Priya the final mockups.")
        #expect(stored.final_text == "Remind me to send Priya the mockups.")
        #expect(stored.raw_text == "remind me to send priya the mockups.")
        #expect(try Data(contentsOf: setup.wav) == Data("RIFF-original".utf8))
        #expect(setup.model.isEdited)
    }

    @Test func flushSavesAtOnceAndCopyUsesTheDraft() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        setup.model.updateDraft("  New text.  ")
        setup.model.copyText()
        #expect(setup.clipboard.copies == ["New text."])
        #expect(setup.model.notice == "Copied")
        #expect(try setup.archive.dictation(id: "a")?.edited_text == "  New text.  ")
    }

    @Test func aFailedSaveIsReportedAndRetried() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        setup.archive.failNextWrite(with: .notFound)
        setup.model.updateDraft("Changed.")
        setup.model.flush()
        guard case .failed = setup.model.saveState else {
            Issue.record("expected a failed save, got \(setup.model.saveState)")
            return
        }
        setup.model.flush()
        #expect(setup.model.saveState == .saved)
    }

    @Test func rowsInProgressAreReadOnly() async throws {
        let setup = try Setup(status: "processing")
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        #expect(!setup.model.isEditable)
        setup.model.updateDraft("x")
        setup.model.flush()
        #expect(try setup.archive.dictation(id: "a")?.edited_text == nil)
        setup.model.requestDelete()
        #expect(!setup.model.isConfirmingDelete)
    }

    @Test func exportMarkdownWritesTheChosenFile() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        let target = setup.directory.appendingPathComponent("out.md")
        setup.destination.url = target
        setup.model.updateDraft("Edited for export.")
        await setup.model.exportMarkdown()
        #expect(setup.destination.requests.first?.name == "dictation-2026-11-13-1441.md")
        let text = try String(contentsOf: target, encoding: .utf8)
        #expect(text.hasPrefix("---\ntype: dictation\n"))
        #expect(text.hasSuffix("\nEdited for export.\n"))
        #expect(setup.model.notice == "Exported out.md")
    }

    @Test func exportAudioCopiesAndCancelDoesNothing() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        await setup.model.exportAudio()
        #expect(setup.model.notice == nil)
        let target = setup.directory.appendingPathComponent("copy.wav")
        try Data("stale".utf8).write(to: target)
        setup.destination.url = target
        await setup.model.exportAudio()
        #expect(try Data(contentsOf: target) == Data("RIFF-original".utf8))
        #expect(FileManager.default.fileExists(atPath: setup.wav.path))
        #expect(setup.destination.requests.last?.type == .wav)

        setup.destination.url = setup.wav
        await setup.model.exportAudio()
        #expect(setup.model.errorMessage?.contains("original audio file") == true)
        #expect(try Data(contentsOf: setup.wav) == Data("RIFF-original".utf8))
    }

    @Test func missingAudioDisablesPlaybackAndExport() async throws {
        let setup = try Setup(audio: false)
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        #expect(!setup.model.playback.isAvailable)
        #expect(setup.model.playback.unavailableReason == "No audio was saved for this dictation.")
        #expect(!setup.model.canExportAudio)
        #expect(setup.model.timeLabel == "0:00 / 0:06")
    }

    @Test func deleteNeedsConfirmationAndRemovesTheRow() async throws {
        let setup = try Setup()
        defer { setup.cleanup() }
        #expect(await setup.loaded())
        setup.model.requestDelete()
        #expect(setup.model.isConfirmingDelete)
        #expect(setup.model.delete())
        #expect(setup.model.isDeleted)
        #expect(try setup.archive.dictation(id: "a") == nil)
    }
}

@MainActor
struct AudioPlaybackModelTests {
    @Test func playPauseSeekAndRestartAtTheEnd() throws {
        let directory = try Fixtures.temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let wav = directory.appendingPathComponent("a.wav")
        try Data().write(to: wav)
        let player = FakePlayer(duration: 6)
        let model = AudioPlaybackModel(makePlayer: { _ in player }, tick: .seconds(60))
        model.load(path: wav.path, fallbackDuration: 1)
        #expect(model.duration == 6)
        model.togglePlay()
        #expect(model.isPlaying)
        model.togglePlay()
        #expect(!model.isPlaying)
        model.seek(to: 0.5)
        #expect(player.currentTime == 3)
        #expect(model.progress == 0.5)
        model.seek(to: 4)
        #expect(player.currentTime == 6)
        model.togglePlay()
        #expect(player.currentTime == 0)
        model.skip(by: 1.5)
        #expect(player.currentTime == 1.5)
        model.stop()
        #expect(!player.isPlaying)
    }

    @Test func unreadableFilesKeepTheFallbackDuration() throws {
        struct Broken: Error {}
        let directory = try Fixtures.temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let wav = directory.appendingPathComponent("a.wav")
        try Data().write(to: wav)
        let model = AudioPlaybackModel(makePlayer: { _ in throw Broken() })
        model.load(path: wav.path, fallbackDuration: 6)
        #expect(!model.isAvailable)
        #expect(model.duration == 6)
        #expect(model.unavailableReason == "The audio file could not be opened.")
        model.load(path: directory.appendingPathComponent("gone.wav").path, fallbackDuration: 2)
        #expect(model.unavailableReason == "The audio file is missing.")
    }
}
