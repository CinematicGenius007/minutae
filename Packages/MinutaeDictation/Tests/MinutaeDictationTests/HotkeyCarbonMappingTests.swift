import Carbon.HIToolbox
import Testing

@testable import MinutaeDictation

@Test func optionSpaceMatchesTheSpikeCarbonMask() {
    #expect(HotkeyChord.spaceKeyCode == UInt16(kVK_Space))
    #expect(HotkeyChord.maxKeyCode == UInt16(kVK_UpArrow))
    #expect(HotkeyCarbonMapping.modifierFlags(.option) == UInt32(optionKey))
    #expect(HotkeyCarbonMapping.modifierFlags(.command) == UInt32(cmdKey))
    #expect(HotkeyCarbonMapping.modifierFlags(.shift) == UInt32(shiftKey))
    #expect(HotkeyCarbonMapping.modifierFlags(.control) == UInt32(controlKey))
    let all = HotkeyCarbonMapping.modifierFlags([.command, .option, .shift, .control])
    #expect(all == UInt32(cmdKey | shiftKey | optionKey | controlKey))
    #expect(HotkeyCarbonStatus.alreadyRegistered == Int32(eventHotKeyExistsErr))
    #expect(HotkeyCarbonStatus.success == Int32(noErr))
}

@Test func modifierKeyCodesMatchHIToolbox() {
    let expected: Set<UInt16> = [
        UInt16(kVK_RightCommand),
        UInt16(kVK_Command),
        UInt16(kVK_Shift),
        UInt16(kVK_CapsLock),
        UInt16(kVK_Option),
        UInt16(kVK_Control),
        UInt16(kVK_RightShift),
        UInt16(kVK_RightOption),
        UInt16(kVK_RightControl),
        UInt16(kVK_Function),
    ]
    #expect(HotkeyChordValidation.modifierKeyCodes == expected)
}

@Test func carbonRegistersAnUnusedChordWithoutInputMonitoring() throws {
    let service = CarbonHotkeyService()
    defer { service.unregister() }
    let registration = HotkeyRegistration(
        mode: .hold,
        chord: HotkeyChord(
            keyCode: UInt16(kVK_F19),
            modifiers: [.control, .option, .shift, .command]
        )
    )
    try service.register(registration)
    #expect(service.registration == registration)
    service.unregister()
    #expect(service.registration == nil)
}

@Test func carbonServiceUnregisterBeforeRegisterIsSafe() async {
    let service: any HotkeyService = CarbonHotkeyService()
    service.unregister()
    #expect(service.registration == nil)
    let task = Task { service.registration }
    #expect(await task.value == nil)
}
