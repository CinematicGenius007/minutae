import ApplicationServices
import AppKit
import Carbon
import Foundation

enum InsertionProbe {
    static func run(args: [String]) throws {
        let method = args.first ?? "clipboard"
        var text = "minutae-spike-\(Int(Date().timeIntervalSince1970))"
        if let idx = args.firstIndex(of: "--text"), args.indices.contains(idx + 1) {
            text = args[idx + 1]
        }

        if IsSecureEventInputEnabled() {
            print("secure_event_input: true")
            print("classified: refused")
            print("detail: IsSecureEventInputEnabled() is true — insertion skipped")
            return
        }
        print("secure_event_input: false")

        switch method {
        case "clipboard":
            try insertViaClipboard(text)
        case "ax":
            try insertViaAccessibility(text)
        default:
            throw ProbeFailure(code: .invalidOutput, detail: "usage: insertion clipboard|ax")
        }
    }

    static func printSecureInputState() {
        print("IsSecureEventInputEnabled: \(IsSecureEventInputEnabled())")
    }

    private static func insertViaClipboard(_ text: String) throws {
        let pb = NSPasteboard.general
        let saved: [[String: Any]] = pb.pasteboardItems?.compactMap { item in
            var dict: [String: Any] = [:]
            for type in item.types {
                if let data = item.data(forType: type) {
                    dict[type.rawValue] = data
                }
            }
            return dict.isEmpty ? nil : dict
        } ?? []

        pb.clearContents()
        pb.setString(text, forType: .string)

        // Cmd+V via CGEvent
        let source = CGEventSource(stateID: .hidSystemState)
        let keyV: CGKeyCode = 9
        guard
            let down = CGEvent(keyboardEventSource: source, virtualKey: keyV, keyDown: true),
            let up = CGEvent(keyboardEventSource: source, virtualKey: keyV, keyDown: false)
        else {
            throw ProbeFailure(code: .unknown, detail: "could not create CGEvent for Cmd+V")
        }
        down.flags = .maskCommand
        up.flags = .maskCommand
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)

        // Restore pasteboard shortly after paste
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
            pb.clearContents()
            for dict in saved {
                let item = NSPasteboardItem()
                for (raw, value) in dict {
                    if let data = value as? Data {
                        item.setData(data, forType: NSPasteboard.PasteboardType(raw))
                    }
                }
                pb.writeObjects([item])
            }
        }
        // Keep process alive long enough for restore
        Thread.sleep(forTimeInterval: 0.3)
        print("method: clipboard+CmdV")
        print("inserted_length: \(text.count)")
        print("pasteboard_restored: true")
        print("note: requires Accessibility (or Input Monitoring) for CGEvent post in some contexts; target app must be focused")
    }

    private static func insertViaAccessibility(_ text: String) throws {
        let trusted = AXIsProcessTrusted()
        print("ax_trusted: \(trusted)")
        if !trusted {
            throw ProbeFailure(
                code: .unknown,
                detail: "Accessibility not granted. Enable this binary in System Settings → Privacy → Accessibility"
            )
        }

        let system = AXUIElementCreateSystemWide()
        var focusedRef: CFTypeRef?
        let focusedStatus = AXUIElementCopyAttributeValue(
            system,
            kAXFocusedUIElementAttribute as CFString,
            &focusedRef
        )
        guard focusedStatus == .success, let focused = focusedRef else {
            throw ProbeFailure(code: .unknown, detail: "no focused UI element (AX error \(focusedStatus.rawValue))")
        }
        let element = focused as! AXUIElement

        var roleRef: CFTypeRef?
        AXUIElementCopyAttributeValue(element, kAXRoleAttribute as CFString, &roleRef)
        let role = roleRef as? String ?? "?"
        print("focused_role: \(role)")

        // Prefer selected-text replace / value set
        var setStatus = AXUIElementSetAttributeValue(
            element,
            kAXSelectedTextAttribute as CFString,
            text as CFTypeRef
        )
        if setStatus != .success {
            // Fallback: append to AXValue when possible
            var valueRef: CFTypeRef?
            AXUIElementCopyAttributeValue(element, kAXValueAttribute as CFString, &valueRef)
            let existing = valueRef as? String ?? ""
            setStatus = AXUIElementSetAttributeValue(
                element,
                kAXValueAttribute as CFString,
                (existing + text) as CFTypeRef
            )
        }
        print("method: accessibility")
        print("ax_status: \(setStatus.rawValue)")
        if setStatus != .success {
            throw ProbeFailure(code: .unknown, detail: "AX set failed with \(setStatus.rawValue)")
        }
        print("inserted_length: \(text.count)")
    }
}
