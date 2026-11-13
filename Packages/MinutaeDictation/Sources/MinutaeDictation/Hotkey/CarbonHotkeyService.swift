import Carbon.HIToolbox
import Foundation

/// Carbon modifier mask for `RegisterEventHotKey`. Option is `optionKey`, the
/// mask SPIKE-01 registered for Option-Space.
enum HotkeyCarbonMapping {
    static func modifierFlags(_ modifiers: HotkeyModifier) -> UInt32 {
        var flags: UInt32 = 0
        if modifiers.contains(.command) {
            flags |= UInt32(cmdKey)
        }
        if modifiers.contains(.shift) {
            flags |= UInt32(shiftKey)
        }
        if modifiers.contains(.option) {
            flags |= UInt32(optionKey)
        }
        if modifiers.contains(.control) {
            flags |= UInt32(controlKey)
        }
        return flags
    }
}

/// `RegisterEventHotKey` implementation of `HotkeyService`.
///
/// Press is `kEventHotKeyPressed` and release is `kEventHotKeyReleased`. Both
/// are delivered by Carbon for a registered chord, so this type does not watch
/// modifier flags, install a `CGEvent` tap, or call `NSEvent` monitors. Those
/// paths need Input Monitoring, which dictation does not request.
///
/// Registration uses `kEventHotKeyNoOptions`, the call SPIKE-01 showed succeeding
/// with status 0. The API is not thread-safe; public methods hop to the main thread.
public final class CarbonHotkeyService: HotkeyService, @unchecked Sendable {
    /// Four-character code 'MNUT'. The event callback in this file reads both.
    fileprivate static let signature: OSType = 0x4D4E_5554
    fileprivate static let hotKeyID: UInt32 = 1

    private let runtime = HotkeyRuntime()
    private var hotKey: EventHotKeyRef?
    private var handler: EventHandlerRef?

    public init() {}

    public var events: AsyncStream<HotkeyEvent> {
        runtime.eventsStream()
    }

    public var session: AsyncStream<HotkeySessionEdge> {
        runtime.sessionStream()
    }

    public var registration: HotkeyRegistration? {
        runtime.registration
    }

    public func register(_ registration: HotkeyRegistration) throws {
        try onMain { try self.registerOnMain(registration) }
    }

    public func unregister() {
        onMain { self.unregisterOnMain() }
    }

    deinit {
        let hotKey = hotKey
        let handler = handler
        self.hotKey = nil
        self.handler = nil
        CarbonHotkeyTeardown.run(hotKey: hotKey, handler: handler)
    }

    private func registerOnMain(_ registration: HotkeyRegistration) throws {
        if let problem = HotkeyChordValidation.problem(with: registration.chord) {
            throw HotkeyRegistrationFailure.reporting(
                registration.chord,
                reason: .invalid(problem)
            )
        }
        if runtime.registration?.chord == registration.chord, hotKey != nil {
            runtime.apply(registration)
            return
        }

        let handlerStatus = installHandlerIfNeeded()
        guard handlerStatus == noErr else {
            throw HotkeyRegistrationFailure.reporting(
                registration.chord,
                reason: .system(handlerStatus)
            )
        }

        var ref: EventHotKeyRef?
        let status = RegisterEventHotKey(
            UInt32(registration.chord.keyCode),
            HotkeyCarbonMapping.modifierFlags(registration.chord.modifiers),
            EventHotKeyID(signature: Self.signature, id: Self.hotKeyID),
            GetApplicationEventTarget(),
            OptionBits(kEventHotKeyNoOptions),
            &ref
        )
        guard status == noErr, let ref else {
            throw HotkeyRegistrationFailure.reporting(
                registration.chord,
                reason: .system(status)
            )
        }

        if let previous = hotKey {
            _ = UnregisterEventHotKey(previous)
        }
        hotKey = ref
        runtime.apply(registration)
    }

    private func unregisterOnMain() {
        if let hotKey {
            _ = UnregisterEventHotKey(hotKey)
            self.hotKey = nil
        }
        runtime.clear()
    }

    private func installHandlerIfNeeded() -> OSStatus {
        if handler != nil {
            return noErr
        }
        var specs = [
            EventTypeSpec(
                eventClass: OSType(kEventClassKeyboard),
                eventKind: UInt32(kEventHotKeyPressed)
            ),
            EventTypeSpec(
                eventClass: OSType(kEventClassKeyboard),
                eventKind: UInt32(kEventHotKeyReleased)
            ),
        ]
        var installed: EventHandlerRef?
        let status = specs.withUnsafeMutableBufferPointer { buffer in
            InstallEventHandler(
                GetApplicationEventTarget(),
                carbonHotkeyEventHandler,
                buffer.count,
                buffer.baseAddress,
                Unmanaged.passUnretained(self).toOpaque(),
                &installed
            )
        }
        if status == noErr {
            handler = installed
        }
        return status
    }

    fileprivate func deliver(_ phase: HotkeyPhase) {
        runtime.receive(phase)
    }

    /// Carbon calls are main-thread only. The closure does not escape `sync`.
    private func onMain<T>(_ body: () throws -> T) rethrows -> T {
        if Thread.isMainThread {
            return try body()
        }
        return try DispatchQueue.main.sync(execute: body)
    }
}

private enum CarbonHotkeyTeardown {
    static func run(hotKey: EventHotKeyRef?, handler: EventHandlerRef?) {
        let body = {
            if let hotKey {
                UnregisterEventHotKey(hotKey)
            }
            if let handler {
                RemoveEventHandler(handler)
            }
        }
        if Thread.isMainThread {
            body()
        } else {
            DispatchQueue.main.sync(execute: body)
        }
    }
}

private func carbonHotkeyEventHandler(
    _: EventHandlerCallRef?,
    _ event: EventRef?,
    _ userData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let event, let userData else {
        return OSStatus(eventNotHandledErr)
    }
    let kind = GetEventKind(event)
    let phase: HotkeyPhase?
    switch kind {
    case UInt32(kEventHotKeyPressed):
        phase = .pressed
    case UInt32(kEventHotKeyReleased):
        phase = .released
    default:
        phase = nil
    }
    guard let phase else {
        return OSStatus(eventNotHandledErr)
    }

    var identifier = EventHotKeyID()
    let read = GetEventParameter(
        event,
        EventParamName(kEventParamDirectObject),
        EventParamType(typeEventHotKeyID),
        nil,
        MemoryLayout<EventHotKeyID>.size,
        nil,
        &identifier
    )
    guard read == noErr, identifier.signature == CarbonHotkeyService.signature,
        identifier.id == CarbonHotkeyService.hotKeyID
    else {
        return OSStatus(eventNotHandledErr)
    }

    let service = Unmanaged<CarbonHotkeyService>.fromOpaque(userData).takeUnretainedValue()
    service.deliver(phase)
    return noErr
}
