import Foundation
import MinutaeDictation
import Testing

@testable import MinutaeFeatures

struct DictationHUDMappingTests {
    @Test func phasesMapToTheFiveStates() {
        #expect(DictationHUDDisplay.from(hudState(.listening)) == .listening)
        #expect(DictationHUDDisplay.from(hudState(.processing)) == .transcribing)
        #expect(DictationHUDDisplay.from(hudState(.inserting)) == .transcribing)
        #expect(
            DictationHUDDisplay.from(hudState(.idle, outcome: .inserted))
                == .inserted(appName: "Slack"))
        #expect(
            DictationHUDDisplay.from(
                hudState(.idle, banner: "Copied to the clipboard.", outcome: .copiedToClipboard))
                == .copied)
        #expect(
            DictationHUDDisplay.from(hudState(.idle, outcome: .nothingHeard))
                == .error(DictationHUDCopy.nothingHeard))
        #expect(
            DictationHUDDisplay.from(
                hudState(.error, banner: "transcribe: notLoaded", outcome: .failed))
                == .error(DictationHUDCopy.transcription))
        #expect(
            DictationHUDDisplay.from(
                hudState(.idle, banner: "transcribe: notLoaded", outcome: .failed))
                == .error(DictationHUDCopy.transcription))
    }

    @Test func idleWithoutAnOutcomeOrCancelledHides() {
        #expect(DictationHUDDisplay.from(hudState(.idle)) == nil)
        #expect(
            DictationHUDDisplay.from(hudState(.idle, banner: "cancelled", outcome: .cancelled))
                == nil)
    }

    @Test func titlesMatchTheDesign() {
        #expect(DictationHUDDisplay.listening.title == "Listening…")
        #expect(DictationHUDDisplay.transcribing.title == "Transcribing…")
        #expect(DictationHUDDisplay.inserted(appName: "Slack").title == "Inserted into Slack")
        #expect(DictationHUDDisplay.inserted(appName: nil).title == "Inserted")
        #expect(DictationHUDDisplay.copied.title == "Copied to the clipboard. Press ⌘V.")
        #expect(DictationHUDDisplay.error("Boom.").title == "Boom.")
        #expect(DictationHUDDisplay.listening.isTerminal == false)
        #expect(DictationHUDDisplay.copied.isTerminal)
    }

    @Test func errorCopyNeverShowsTheRawBanner() {
        #expect(DictationHUDCopy.error("capture start: denied") == DictationHUDCopy.microphone)
        #expect(DictationHUDCopy.error("wav: disk full") == DictationHUDCopy.microphone)
        #expect(DictationHUDCopy.error("insert: secure field") == DictationHUDCopy.secureField)
        #expect(DictationHUDCopy.error("insert: failed") == DictationHUDCopy.insertion)
        #expect(DictationHUDCopy.error("post-process: boom") == DictationHUDCopy.insertion)
        #expect(DictationHUDCopy.error("store: locked") == DictationHUDCopy.saving)
        #expect(DictationHUDCopy.error("something else") == DictationHUDCopy.generic)
        #expect(DictationHUDCopy.error(nil) == DictationHUDCopy.generic)
    }
}

@MainActor
struct DictationHUDModelTests {
    @Test func aFullRunShowsEachStateAndDismissesTheResult() async {
        let sleeper = ManualSleep()
        let model = DictationHUDModel(
            now: Fixtures.now, sleep: { _ in await sleeper.sleep() })
        var changes: [DictationHUDDisplay?] = []
        model.onChange = { changes.append($0) }

        model.apply(.state(hudState(.idle)))
        model.apply(.state(hudState(.listening)))
        #expect(model.listeningSince == Fixtures.now())
        model.apply(.state(hudState(.listening)))
        model.apply(.state(hudState(.processing)))
        #expect(model.listeningSince == nil)
        model.apply(.state(hudState(.inserting)))
        model.apply(.state(hudState(.idle, outcome: .inserted)))
        #expect(model.display == .inserted(appName: "Slack"))
        #expect(changes == [.listening, .transcribing, .inserted(appName: "Slack")])

        #expect(await eventually("dismiss scheduled") { await sleeper.pending == 1 })
        await sleeper.wake()
        #expect(await eventually("dismissed") { model.display == nil })
        #expect(changes.last == .some(nil))
    }

    @Test func errorThenFailedIdleKeepsOneDismissal() async {
        let sleeper = ManualSleep()
        let model = DictationHUDModel(sleep: { _ in await sleeper.sleep() })
        var changes = 0
        model.onChange = { _ in changes += 1 }
        model.apply(.state(hudState(.listening)))
        model.apply(.state(hudState(.error, banner: "capture: lost", outcome: .failed)))
        model.apply(.state(hudState(.idle, banner: "capture: lost", outcome: .failed)))
        #expect(model.display == .error(DictationHUDCopy.microphone))
        #expect(changes == 2)
        #expect(await eventually("one dismissal") { await sleeper.pending == 1 })
    }

    @Test func aNewRunCancelsAPendingDismissal() async {
        let sleeper = ManualSleep()
        let model = DictationHUDModel(sleep: { _ in await sleeper.sleep() })
        model.apply(.state(hudState(.idle, outcome: .copiedToClipboard)))
        #expect(await eventually("scheduled") { await sleeper.pending == 1 })
        model.apply(.state(hudState(.listening)))
        await sleeper.wake()
        try? await Task.sleep(for: .milliseconds(30))
        #expect(model.display == .listening)
    }

    @Test func cancelHidesAtOnce() {
        let model = DictationHUDModel()
        model.apply(.state(hudState(.listening)))
        model.apply(.state(hudState(.idle, banner: "cancelled", outcome: .cancelled)))
        #expect(model.display == nil)
        model.apply(.ignoredWhileBusy)
        #expect(model.display == nil)
    }

    @Test func runFollowsAControllerStream() async {
        let control = FakeDictationControl()
        let model = DictationHUDModel()
        let task = Task { await model.run(await control.events()) }
        #expect(await eventually("subscribed") { await control.subscriberCount == 1 })
        await control.send(.state(hudState(.listening)))
        #expect(await eventually("listening") { model.display == .listening })
        await control.finish()
        await task.value
    }
}
