import AppKit
import MinutaeDesign
import MinutaeDictation
import SwiftUI

/// Shows the dictation HUD in a floating panel near the top centre of the screen in use.
///
/// The panel is non-activating, never key or main, ignores the mouse, and joins every Space,
/// so the app you are dictating into keeps focus. Wire it once:
///
///     DictationHUDPresenter.attach(to: environment.dictation)
@MainActor
public final class DictationHUDPresenter {
    public let model: DictationHUDModel
    private let panel: DictationHUDWindow
    private var task: Task<Void, Never>?

    public init(model: DictationHUDModel = DictationHUDModel()) {
        self.model = model
        panel = DictationHUDWindow(model: model)
        model.onChange = { [panel] display in
            panel.update(visible: display != nil)
        }
    }

    /// Starts a presenter that follows `controller` for the life of its event stream.
    @discardableResult
    public static func attach(to controller: some DictationControlling) -> DictationHUDPresenter {
        let presenter = DictationHUDPresenter()
        presenter.start(observing: controller)
        return presenter
    }

    /// The task keeps the presenter alive until the stream ends or `stop()` is called.
    public func start(observing controller: some DictationControlling) {
        task?.cancel()
        task = Task {
            let events = await controller.events()
            await self.model.run(events)
        }
    }

    public func stop() {
        task?.cancel()
        task = nil
        panel.update(visible: false)
    }
}

/// Never key, never main: clicks and keys stay with the frontmost app.
final class DictationHUDPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class DictationHUDWindow {
    static let size = NSSize(width: 640, height: 60)
    static let topInset: CGFloat = 12

    private let panel: DictationHUDPanel
    private var hideGeneration = 0

    init(model: DictationHUDModel) {
        panel = DictationHUDPanel(
            contentRect: NSRect(origin: .zero, size: Self.size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: true
        )
        panel.level = .statusBar
        panel.isFloatingPanel = true
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.ignoresMouseEvents = true
        panel.isReleasedWhenClosed = false
        panel.animationBehavior = .none
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
        panel.collectionBehavior.insert(.ignoresCycle)
        let hosting = NSHostingView(
            rootView: DictationHUDContent(model: model)
                .frame(width: Self.size.width, height: Self.size.height, alignment: .top)
        )
        hosting.sizingOptions = []
        hosting.frame = NSRect(origin: .zero, size: Self.size)
        panel.contentView = hosting
        panel.setAccessibilityLabel("Dictation status")
    }

    func update(visible: Bool) {
        hideGeneration += 1
        if visible {
            position()
            panel.alphaValue = 1
            panel.orderFrontRegardless()
            return
        }
        guard panel.isVisible else { return }
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            panel.orderOut(nil)
            return
        }
        // Let the SwiftUI fade finish, then drop the window if nothing came back.
        let token = hideGeneration
        Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            guard let self, self.hideGeneration == token else { return }
            self.panel.orderOut(nil)
        }
    }

    private func position() {
        let mouse = NSEvent.mouseLocation
        let screen =
            NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) }
            ?? NSScreen.main ?? NSScreen.screens.first
        guard let visible = screen?.visibleFrame else { return }
        let origin = NSPoint(
            x: visible.midX - Self.size.width / 2,
            y: visible.maxY - Self.size.height - Self.topInset
        )
        panel.setFrame(NSRect(origin: origin, size: Self.size), display: false)
    }
}
