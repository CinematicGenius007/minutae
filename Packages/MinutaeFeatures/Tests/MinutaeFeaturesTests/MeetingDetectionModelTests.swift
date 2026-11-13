import Testing

@testable import MinutaeFeatures

@MainActor
@Suite struct MeetingDetectionModelTests {
    private final class Box { var mic = false; var enabled = true; var recording = false }

    private func model(_ box: Box) -> MeetingDetectionModel {
        MeetingDetectionModel(
            sustain: 2, probe: { box.mic }, isEnabled: { box.enabled },
            isRecording: { box.recording })
    }

    @Test func promptsAfterSustainedMicUseThenDismissStaysQuiet() {
        let box = Box()
        let m = model(box)
        m.tick()
        #expect(m.phase == .listening)
        box.mic = true
        m.tick()
        #expect(m.phase == .listening)
        m.tick()
        #expect(m.phase == .prompting)
        m.dismissPrompt()
        m.tick()
        #expect(m.phase == .listening)
        box.mic = false
        m.tick()
        box.mic = true
        m.tick()
        m.tick()
        #expect(m.phase == .prompting)
    }

    @Test func disabledSettingShowsOffAndRecordingWins() {
        let box = Box()
        let m = model(box)
        box.enabled = false
        m.tick()
        #expect(m.phase == .off)
        box.recording = true
        m.tick()
        #expect(m.phase == .recording)
    }
}
