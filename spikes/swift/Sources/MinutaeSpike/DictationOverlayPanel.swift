import AppKit

/// Non-activating HUD panel: floats over full-screen apps, transparent, ignores mouse.
final class DictationOverlayPanel {
    private var panel: NSPanel?
    private var label: NSTextField?

    func show(state: OverlayState) {
        if panel == nil {
            let panel = NSPanel(
                contentRect: NSRect(x: 0, y: 0, width: 220, height: 56),
                styleMask: [.borderless, .nonactivatingPanel],
                backing: .buffered,
                defer: false
            )
            panel.level = .statusBar
            panel.isOpaque = false
            panel.backgroundColor = .clear
            panel.hasShadow = true
            panel.ignoresMouseEvents = true
            panel.becomesKeyOnlyIfNeeded = true
            panel.hidesOnDeactivate = false
            panel.collectionBehavior = [
                .canJoinAllSpaces,
                .fullScreenAuxiliary,
                .stationary,
            ]
            // Critical: do not activate the app when showing
            panel.isFloatingPanel = true

            let container = NSView(frame: panel.contentView!.bounds)
            container.wantsLayer = true
            container.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.72).cgColor
            container.layer?.cornerRadius = 14

            let label = NSTextField(labelWithString: "")
            label.textColor = .white
            label.font = .systemFont(ofSize: 15, weight: .medium)
            label.alignment = .center
            label.frame = container.bounds.insetBy(dx: 8, dy: 16)
            container.addSubview(label)

            panel.contentView = container
            self.panel = panel
            self.label = label
        }

        label?.stringValue = state == .listening ? "listening" : "processing"
        positionNearTop()
        panel?.orderFrontRegardless()
        // Never call NSApp.activate
        print("overlay: show \(state.rawValue); app_active=\(NSApp.isActive) key=\(panel?.isKeyWindow ?? false)")
    }

    func hide() {
        panel?.orderOut(nil)
        print("overlay: hide")
    }

    private func positionNearTop() {
        guard let panel, let screen = NSScreen.main else { return }
        let frame = screen.visibleFrame
        let size = panel.frame.size
        let x = frame.midX - size.width / 2
        let y = frame.maxY - size.height - 48
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }
}
