import Testing

@testable import MinutaeDictation

@Test func fakeHoldEmitsPressReleaseAndSessionEdges() async throws {
    let service = FakeHotkeyService()
    var events = service.events.makeAsyncIterator()
    var session = service.session.makeAsyncIterator()

    try service.register(.default)
    #expect(service.registration == .default)
    #expect(service.simulate(.pressed))
    #expect(service.simulate(.released))

    let pressed = try #require(await events.next())
    let released = try #require(await events.next())
    #expect(pressed == HotkeyEvent(phase: .pressed, chord: .optionSpace))
    #expect(released == HotkeyEvent(phase: .released, chord: .optionSpace))
    #expect(await session.next() == .began)
    #expect(await session.next() == .ended)
    #expect(!service.isActive)
}

@Test func fakeToggleIgnoresReleaseUntilTheNextPress() async throws {
    let service = FakeHotkeyService()
    var session = service.session.makeAsyncIterator()
    try service.register(HotkeyRegistration(mode: .toggle, chord: .optionSpace))

    #expect(service.simulate(.pressed))
    #expect(service.simulate(.released))
    #expect(service.isActive)
    #expect(await session.next() == .began)

    #expect(service.simulate(.pressed))
    #expect(await session.next() == .ended)
    #expect(!service.isActive)
}

@Test func fakeRepeatPressInHoldDoesNotStartTwice() async throws {
    let service = FakeHotkeyService()
    var session = service.session.makeAsyncIterator()
    try service.register(.default)

    #expect(service.simulate(.pressed))
    #expect(service.simulate(.pressed))
    #expect(await session.next() == .began)
    #expect(service.isActive)
}

@Test func changingRegistrationEndsAnActiveSession() async throws {
    let service = FakeHotkeyService()
    var session = service.session.makeAsyncIterator()
    try service.register(.default)
    #expect(service.simulate(.pressed))
    #expect(await session.next() == .began)

    let toggled = HotkeyRegistration(mode: .toggle, chord: .optionSpace)
    try service.register(toggled)
    #expect(await session.next() == .ended)
    #expect(service.registration == toggled)
    #expect(!service.isActive)
    #expect(service.systemRegisterCallCount() == 1)
}

@Test func unregisterEndsTheSessionWithoutASyntheticRelease() async throws {
    let service = FakeHotkeyService()
    var events = service.events.makeAsyncIterator()
    var session = service.session.makeAsyncIterator()
    try service.register(.default)
    #expect(service.simulate(.pressed))
    _ = await events.next()
    #expect(await session.next() == .began)

    service.unregister()
    #expect(service.registration == nil)
    #expect(!service.isActive)
    #expect(await session.next() == .ended)
    #expect(service.emittedEvents() == [HotkeyEvent(phase: .pressed, chord: .optionSpace)])
    #expect(service.emittedSession() == [.began, .ended])
    #expect(!service.simulate(.pressed))
}

@Test func chordChangeEndsTheSessionAndSameRegistrationDoesNot() async throws {
    let service = FakeHotkeyService()
    try service.register(.default)
    #expect(service.simulate(.pressed))
    let replacement = HotkeyRegistration(
        mode: .hold,
        chord: HotkeyChord(keyCode: 40, modifiers: .command)
    )
    try service.register(replacement)
    #expect(service.emittedSession() == [.began, .ended])
    #expect(!service.isActive)

    #expect(service.simulate(.pressed))
    try service.register(replacement)
    #expect(service.isActive)
    #expect(service.emittedSession() == [.began, .ended, .began])
}

@Test func invalidChordFailsBeforeTheSystemAndSuggestsAFallback() throws {
    let service = FakeHotkeyService()
    let bare = HotkeyRegistration(mode: .hold, chord: HotkeyChord(keyCode: 49, modifiers: []))

    let error = #expect(throws: HotkeyRegistrationFailure.self) {
        try service.register(bare)
    }
    let failure = try #require(error)
    #expect(failure.reason == .invalid(.missingModifier))
    #expect(failure.suggestedFallback == .optionSpace)
    #expect(failure.message.contains("Suggested fallback"))
    #expect(failure.message.contains("key 49"))
    #expect(service.registration == nil)
    #expect(service.systemRegisterCallCount() == 0)
    #expect(service.registerCallCount() == 1)
}

@Test func systemRejectionKeepsThePreviousChordAndReportsTheStatus() throws {
    let service = FakeHotkeyService()
    try service.register(.default)
    service.setSystemStatus(HotkeyCarbonStatus.alreadyRegistered)

    let taken = HotkeyRegistration(
        mode: .hold,
        chord: HotkeyChord(keyCode: 40, modifiers: .command)
    )
    let error = #expect(throws: HotkeyRegistrationFailure.self) {
        try service.register(taken)
    }
    let failure = try #require(error)
    #expect(failure.chord == taken.chord)
    #expect(failure.reason == .system(HotkeyCarbonStatus.alreadyRegistered))
    #expect(failure.suggestedFallback == .optionSpace)
    #expect(failure.message.contains("-9878"))
    #expect(service.registration == .default)
    #expect(service.isActive == false)
}

@Test func subscribersDoNotReplayAndEachGetsTheNextEvent() async throws {
    let service = FakeHotkeyService()
    try service.register(.default)
    #expect(service.simulate(.pressed))

    var late = service.events.makeAsyncIterator()
    var first = service.session.makeAsyncIterator()
    var second = service.session.makeAsyncIterator()
    #expect(service.simulate(.released))

    #expect(await late.next()?.phase == .released)
    #expect(await first.next() == .ended)
    #expect(await second.next() == .ended)
}

@Test func fakeServiceIsSendable() async throws {
    let service: any HotkeyService = FakeHotkeyService()
    let task = Task {
        try service.register(.default)
        return service.registration
    }
    #expect(try await task.value == .default)
}
