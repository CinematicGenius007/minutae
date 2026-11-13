import Testing

@testable import MinutaeDictation

@Test func holdPressBeginsAndReleaseEnds() {
    var machine = HotkeySessionMachine(mode: .hold)
    #expect(machine.handle(.pressed) == .began)
    #expect(machine.isActive)
    #expect(machine.handle(.released) == .ended)
    #expect(!machine.isActive)
}

@Test func holdIgnoresRepeatPressAndReleaseWhileIdle() {
    var machine = HotkeySessionMachine(mode: .hold)
    #expect(machine.handle(.released) == nil)
    #expect(machine.handle(.pressed) == .began)
    #expect(machine.handle(.pressed) == nil)
    #expect(machine.isActive)
    #expect(machine.handle(.released) == .ended)
    #expect(machine.handle(.released) == nil)
}

@Test func toggleFlipsOnPressAndIgnoresRelease() {
    var machine = HotkeySessionMachine(mode: .toggle)
    #expect(machine.handle(.pressed) == .began)
    #expect(machine.handle(.released) == nil)
    #expect(machine.isActive)
    #expect(machine.handle(.pressed) == .ended)
    #expect(!machine.isActive)
    #expect(machine.handle(.released) == nil)
}

@Test func setModeEndsAnActiveSessionOnlyWhenTheModeChanges() {
    var machine = HotkeySessionMachine(mode: .hold)
    #expect(machine.handle(.pressed) == .began)
    #expect(machine.setMode(.hold) == nil)
    #expect(machine.isActive)
    #expect(machine.setMode(.toggle) == .ended)
    #expect(!machine.isActive)
    #expect(machine.mode == .toggle)
    #expect(machine.handle(.pressed) == .began)
}

@Test func setModeWhileIdleKeepsTheSessionIdle() {
    var machine = HotkeySessionMachine(mode: .hold)
    #expect(machine.setMode(.toggle) == nil)
    #expect(!machine.isActive)
    #expect(machine.mode == .toggle)
}
