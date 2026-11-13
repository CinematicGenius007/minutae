import Testing

@testable import MinutaeCore

@Test(arguments: [
    (Permission.microphone, PermissionFallback.disabledWithPrompt),
    (Permission.accessibility, PermissionFallback.copyToClipboard),
    (Permission.screenRecording, PermissionFallback.microphoneOnly),
    (Permission.calendar, PermissionFallback.noTitles),
])
func fallbackMatchesArchitecture(_ permission: Permission, _ expected: PermissionFallback) {
    let service = FakePermissionService()
    #expect(service.fallback(for: permission) == expected)
    #expect(PermissionFallback.defined(for: permission) == expected)
}

@Test func reasonsAreDistinctPlainLanguage() {
    var seen = Set<String>()
    for permission in Permission.allCases {
        let reason = permission.reason
        #expect(!reason.isEmpty)
        #expect(seen.insert(reason).inserted)
    }
}

@Test func requestFromNotDeterminedGrantsAndEmitsToEverySubscriber() async {
    let service = FakePermissionService()
    let first = service.changes
    let second = service.changes

    let status = await service.request(.microphone)

    #expect(status == .granted)
    #expect(service.status(of: .microphone) == .granted)
    #expect(service.status(of: .calendar) == .notDetermined)
    #expect(service.requestCount(for: .microphone) == 1)
    let expected = PermissionChange(permission: .microphone, status: .granted)
    #expect(service.emittedChanges() == [expected])
    var firstIterator = first.makeAsyncIterator()
    var secondIterator = second.makeAsyncIterator()
    #expect(await firstIterator.next() == expected)
    #expect(await secondIterator.next() == expected)
}

@Test func scriptedRequestDenialEmitsDenied() async {
    let service = FakePermissionService(requestResults: [.calendar: .denied])
    let stream = service.changes

    let status = await service.request(.calendar)

    #expect(status == .denied)
    #expect(service.emittedChanges() == [.init(permission: .calendar, status: .denied)])
    var iterator = stream.makeAsyncIterator()
    #expect(await iterator.next() == PermissionChange(permission: .calendar, status: .denied))
}

@Test func requestLeavesGrantedDeniedAndRestrictedUntouched() async {
    let service = FakePermissionService(statuses: [
        .microphone: .granted,
        .accessibility: .denied,
        .screenRecording: .restricted,
    ])
    let stream = service.changes

    #expect(await service.request(.microphone) == .granted)
    #expect(await service.request(.accessibility) == .denied)
    #expect(await service.request(.screenRecording) == .restricted)
    #expect(service.requestCount(for: .microphone) == 0)
    #expect(service.requestCount(for: .accessibility) == 0)
    #expect(service.requestCount(for: .screenRecording) == 0)
    #expect(service.emittedChanges().isEmpty)
    _ = stream
}

@Test func statusReadDoesNotRequestOrEmit() async {
    let service = FakePermissionService(statuses: [.microphone: .denied])
    #expect(service.status(of: .microphone) == .denied)
    #expect(service.status(of: .calendar) == .notDetermined)
    #expect(service.requestCount(for: .microphone) == 0)
    #expect(service.emittedChanges().isEmpty)
}

@Test func setStatusEmitsWithoutCountingAsARequest() async {
    let service = FakePermissionService()
    let stream = service.changes

    service.setStatus(.granted, for: .accessibility)
    service.setStatus(.granted, for: .accessibility)

    #expect(service.status(of: .accessibility) == .granted)
    #expect(service.requestCount(for: .accessibility) == 0)
    let expected = PermissionChange(permission: .accessibility, status: .granted)
    #expect(service.emittedChanges() == [expected])
    var iterator = stream.makeAsyncIterator()
    #expect(await iterator.next() == expected)
}

@Test func settingsLinksUseThePrivacyPanes() {
    let expected: [Permission: String] = [
        .microphone: "Privacy_Microphone",
        .accessibility: "Privacy_Accessibility",
        .screenRecording: "Privacy_ScreenCapture",
        .calendar: "Privacy_Calendars",
    ]
    for (permission, pane) in expected {
        let url = PermissionSettingsLink.url(for: permission)
        #expect(PermissionSettingsLink.paneIdentifier(for: permission) == pane)
        #expect(
            url.absoluteString
                == "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?\(pane)"
        )
    }
}

@Test func debugDescriptionIncludesStatusFallbackAndReason() {
    let line = PermissionDebugDescription.line(permission: .calendar, status: .denied)
    #expect(line.contains("calendar"))
    #expect(line.contains("status=denied"))
    #expect(line.contains("fallback=noTitles"))
    #expect(line.contains(Permission.calendar.reason))
}

@Test func systemStatusReadsWithoutPromptingOrCrashing() {
    let service = SystemPermissionService()
    for permission in Permission.allCases {
        let status = service.status(of: permission)
        #expect(PermissionStatus.allCases.contains(status))
        #expect(service.fallback(for: permission) == PermissionFallback.defined(for: permission))
        let line = PermissionDebugDescription.line(permission: permission, status: status)
        #expect(line.contains(permission.rawValue))
    }
    _ = service.changes
}

@Test func fakeServiceIsSendable() async {
    let service: any PermissionService = FakePermissionService(statuses: [
        .calendar: .restricted
    ])
    let task = Task { service.status(of: .calendar) }
    #expect(await task.value == .restricted)
}
