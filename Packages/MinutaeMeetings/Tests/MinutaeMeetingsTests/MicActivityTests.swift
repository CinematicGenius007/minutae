import Testing

@testable import MinutaeMeetings

private func step(_ d: inout MicActivityDetector, _ mic: Bool, _ own: Bool) -> Bool {
    d.ingest(micInUse: mic, ownCapture: own)
}

@Test func promptsOnlyAfterSustainedUse() {
    var d = MicActivityDetector(sustain: 3)
    #expect(!step(&d, true, false))
    #expect(!step(&d, true, false))
    #expect(step(&d, true, false))
    #expect(!step(&d, true, false))
}

@Test func ownCaptureAndIdleResetTheStreak() {
    var d = MicActivityDetector(sustain: 2)
    #expect(!step(&d, true, false))
    #expect(!step(&d, true, true))
    #expect(!step(&d, true, false))
    #expect(step(&d, true, false))
}

@Test func rearmsAfterTheMicGoesIdle() {
    var d = MicActivityDetector(sustain: 1)
    #expect(step(&d, true, false))
    #expect(!step(&d, true, false))
    #expect(!step(&d, false, false))
    #expect(step(&d, true, false))
}

@Test func disarmSuppressesUntilIdle() {
    var d = MicActivityDetector(sustain: 1)
    d.disarm()
    #expect(!step(&d, true, false))
    #expect(!step(&d, false, false))
    #expect(step(&d, true, false))
}
