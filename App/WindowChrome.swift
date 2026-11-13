import AppKit
import MinutaeDesign
import SwiftUI

enum MainWindow {
    static let id = "main"

    @MainActor
    static func show(_ openWindow: OpenWindowAction) {
        NSApp.activate()
        if let window = NSApp.windows.first(where: { $0.identifier == WindowChrome.identifier }) {
            window.deminiaturize(nil)
            window.makeKeyAndOrderFront(nil)
            return
        }
        openWindow(id: id)
    }
}

struct WindowChrome: NSViewRepresentable {
    static let identifier = NSUserInterfaceItemIdentifier("minutae.main")

    func makeNSView(context: Context) -> NSView {
        ChromeView()
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

private final class ChromeView: NSView {
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard let window else { return }
        window.identifier = WindowChrome.identifier
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.titlebarSeparatorStyle = .none
        window.styleMask.insert(.fullSizeContentView)
        window.isMovableByWindowBackground = true
        window.backgroundColor = NSColor(Color.minutae.bg)
    }
}
