import Foundation
import MinutaeCore
import MinutaeDictation
import MinutaeLibrary
import Testing

@testable import MinutaeFeatures

@MainActor
struct DictateViewModelTests {
    @MainActor
    struct Setup {
        let archive: InMemoryDictationArchive
        let control = FakeDictationControl()
        let clipboard = RecordingClipboard()
        let model: DictateViewModel
        let runner: Task<Void, Never>

        init(_ rows: [Dictation]) {
            archive = InMemoryDictationArchive(rows)
            let model = DictateViewModel(
                archive: archive, dictation: control, clipboard: clipboard,
                formatter: Fixtures.formatter, now: Fixtures.now,
                copyFeedback: .milliseconds(20))
            self.model = model
            runner = Task { await model.run() }
        }
    }

    @Test func rowsAreNewestFirstWithDesignedMeta() async {
        let old = Fixtures.row(
            "old", started: "2026-11-13T13:12:00Z", text: "Thanks.", durationMs: 8_000)
        let live = Fixtures.row(
            "live", started: "2026-11-13T15:59:00Z", text: "", status: "processing")
        let setup = Setup([old, Fixtures.row("new"), live])
        defer { setup.runner.cancel() }
        #expect(await eventually("loaded") { setup.model.hasLoaded })
        let rows = setup.model.rows
        #expect(rows.map(\.id) == ["live", "new", "old"])
        #expect(
            rows[0]
                == DictationRowItem(
                    id: "live", text: "Transcribing…", meta: "3:59 PM · 6 s", isPlaceholder: true,
                    canCopy: false))
        #expect(rows[1].text == "Remind me to send Priya the mockups.")
        #expect(rows[1].meta == "2:41 PM · 6 s")
        #expect(rows[2].meta == "1:12 PM · 8 s")
    }

    @Test func editedTextIsWhatTheListShowsAndCopies() async throws {
        var row = Fixtures.row("a")
        row.edited_text = "Edited words."
        let setup = Setup([row])
        defer { setup.runner.cancel() }
        #expect(await eventually("loaded") { setup.model.hasLoaded })
        #expect(setup.model.rows.first?.text == "Edited words.")
        setup.model.copy(id: "a")
        #expect(setup.clipboard.copies == ["Edited words."])
        #expect(setup.model.copiedID == "a")
        #expect(await eventually("copy feedback clears") { setup.model.copiedID == nil })
    }

    @Test func clearAllNeedsConfirmationAndKeepsRunsInProgress() async {
        let live = Fixtures.row("live", text: "", status: "listening")
        let setup = Setup([Fixtures.row("a"), Fixtures.row("b"), live])
        defer { setup.runner.cancel() }
        #expect(await eventually("loaded") { setup.model.rows.count == 3 })
        #expect(setup.model.canClearAll)
        setup.model.requestClearAll()
        #expect(setup.model.isConfirmingClearAll)
        setup.model.clearAll()
        #expect(!setup.model.isConfirmingClearAll)
        #expect(await eventually("cleared") { setup.model.rows.map(\.id) == ["live"] })
        #expect(!setup.model.canClearAll)
        setup.model.requestClearAll()
        #expect(!setup.model.isConfirmingClearAll)
    }

    @Test func clearAllFailureIsShown() async {
        let setup = Setup([Fixtures.row("a")])
        defer { setup.runner.cancel() }
        #expect(await eventually("loaded") { setup.model.hasLoaded })
        setup.archive.failNextWrite(with: .notFound)
        setup.model.clearAll()
        #expect(setup.model.errorMessage?.hasPrefix("Couldn't clear the dictations.") == true)
        setup.model.dismissError()
        #expect(setup.model.errorMessage == nil)
    }

    @Test func micFollowsTheControllerAndTogglesIt() async {
        let setup = Setup([])
        defer { setup.runner.cancel() }
        #expect(await eventually("subscribed") { await setup.control.subscriberCount == 1 })
        await setup.model.toggleDictation()
        #expect(await setup.control.toggles == 1)
        await setup.control.send(.state(hudState(.listening)))
        #expect(await eventually("listening") { setup.model.isListening })
        await setup.control.send(.state(hudState(.processing)))
        #expect(await eventually("busy") { setup.model.isBusy })
        await setup.model.toggleDictation()
        #expect(await setup.control.toggles == 1)
        await setup.control.send(.state(hudState(.error, banner: "x", outcome: .failed)))
        #expect(await eventually("idle after error") { setup.model.phase == .idle })
    }

    @Test func audioWarningCountsFiles() {
        #expect(DictateViewModel.audioWarning(DictationDeletion(deletedRows: 1)) == nil)
        #expect(
            DictateViewModel.audioWarning(DictationDeletion(audioFailures: ["/a.wav"]))
                == "1 audio file could not be removed from disk.")
        #expect(
            DictateViewModel.audioWarning(DictationDeletion(audioFailures: ["/a", "/b"]))
                == "2 audio files could not be removed from disk.")
    }
}
