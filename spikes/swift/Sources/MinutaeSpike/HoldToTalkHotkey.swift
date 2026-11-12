import AppKit
import Carbon
import Foundation

/// Global ⌥Space via Carbon RegisterEventHotKey — does not require Input Monitoring.
/// The system should deliver the hotkey to us instead of inserting a non-breaking space.
final class HoldToTalkHotkey {
    enum Event {
        case press
        case release
    }

    private let handler: (Event) -> Void
    private var hotKeyRef: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private var pressed = false

    init(handler: @escaping (Event) -> Void) {
        self.handler = handler
    }

    deinit {
        unregister()
    }

    func register() {
        let hotKeyID = EventHotKeyID(signature: OSType(0x4D4E5350), id: 1) // 'MNSP'
        // keyCode 49 = Space; optionKey = Carbon option
        let status = RegisterEventHotKey(
            UInt32(kVK_Space),
            UInt32(optionKey),
            hotKeyID,
            GetApplicationEventTarget(),
            0,
            &hotKeyRef
        )
        print("hotkey_register_status: \(status) (0 = noErr)")

        var eventTypes = [
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
            EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased)),
        ]

        let userData = Unmanaged.passUnretained(self).toOpaque()
        InstallEventHandler(
            GetApplicationEventTarget(),
            { (_, event, userData) -> OSStatus in
                guard let userData else { return noErr }
                let this = Unmanaged<HoldToTalkHotkey>.fromOpaque(userData).takeUnretainedValue()
                let kind = GetEventKind(event)
                if kind == UInt32(kEventHotKeyPressed) {
                    if !this.pressed {
                        this.pressed = true
                        this.handler(.press)
                    }
                } else if kind == UInt32(kEventHotKeyReleased) {
                    if this.pressed {
                        this.pressed = false
                        this.handler(.release)
                    }
                }
                return noErr
            },
            2,
            &eventTypes,
            userData,
            &handlerRef
        )
    }

    func unregister() {
        if let hotKeyRef {
            UnregisterEventHotKey(hotKeyRef)
            self.hotKeyRef = nil
        }
        if let handlerRef {
            RemoveEventHandler(handlerRef)
            self.handlerRef = nil
        }
    }
}
