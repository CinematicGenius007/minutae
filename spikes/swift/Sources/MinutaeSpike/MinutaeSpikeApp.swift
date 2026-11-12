import AppKit
import SwiftUI

@main
struct MinutaeSpikeApp: App {
    @NSApplicationDelegateAdaptor(SpikeAppDelegate.self) var appDelegate

    private var controllerLabel: String {
        appDelegate.controller.isRecording ? "Minutae Spike · rec" : "Minutae Spike"
    }

    private var controllerIcon: String {
        appDelegate.controller.isRecording ? "record.circle.fill" : "waveform.circle"
    }

    var body: some Scene {
        // Template SF Symbol: idle vs recording swapped via label text; true dual-icon
        // needs an NSStatusItem + template PDFs (see RESULTS.md limitations).
        MenuBarExtra(controllerLabel, systemImage: controllerIcon) {
            MenuBarContent(controller: appDelegate.controller)
        }
        .menuBarExtraStyle(.menu)

        // Hidden settings window so the process stays a proper LSUIElement-capable app
        Settings {
            VStack(alignment: .leading, spacing: 8) {
                Text("Minutae SPIKE-01")
                    .font(.headline)
                Text("Throwaway native probes. See spikes/swift/RESULTS.md.")
                    .foregroundStyle(.secondary)
                Button("Show overlay: listening") {
                    appDelegate.controller.showOverlay(state: .listening)
                }
                Button("Show overlay: processing") {
                    appDelegate.controller.showOverlay(state: .processing)
                }
                Button("Hide overlay") {
                    appDelegate.controller.hideOverlay()
                }
            }
            .padding()
            .frame(width: 360)
        }
    }
}

@MainActor
final class SpikeAppDelegate: NSObject, NSApplicationDelegate {
    let controller = SpikeController()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        controller.start()
        print("spike: launched. Hold ⌥Space for hold-to-talk. Menu bar icon should appear.")
    }
}

enum OverlayState: String {
    case listening
    case processing
}

@MainActor
final class SpikeController: ObservableObject {
    @Published var isRecording = false
    @Published var lastHotkeyEvent = "none"
    @Published var nbspDetected = false

    private var hotkey: HoldToTalkHotkey?
    private var overlay: DictationOverlayPanel?

    func start() {
        overlay = DictationOverlayPanel()
        hotkey = HoldToTalkHotkey { [weak self] event in
            Task { @MainActor in
                self?.handleHotkey(event)
            }
        }
        hotkey?.register()
        print("spike: hotkey registered for ⌥Space (Carbon RegisterEventHotKey)")
        print("spike: permissions expected — none for Carbon hotkey; Accessibility only if testing insertion")
        if ProcessInfo.processInfo.environment["SPIKE_AUTO_DEMO"] == "1" {
            print("spike: auto-demo — showing overlay without activating")
            showOverlay(state: .listening)
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
                self?.showOverlay(state: .processing)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { [weak self] in
                self?.hideOverlay()
                print("spike: auto-demo done; isActive=\(NSApp.isActive)")
            }
        }
    }

    private func handleHotkey(_ event: HoldToTalkHotkey.Event) {
        switch event {
        case .press:
            isRecording = true
            lastHotkeyEvent = "press \(ISO8601DateFormatter().string(from: Date()))"
            showOverlay(state: .listening)
            print("hotkey: press — overlay listening; focused app should keep keyboard focus")
        case .release:
            isRecording = false
            lastHotkeyEvent = "release \(ISO8601DateFormatter().string(from: Date()))"
            showOverlay(state: .processing)
            print("hotkey: release — overlay processing")
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { [weak self] in
                self?.hideOverlay()
            }
        }
    }

    func showOverlay(state: OverlayState) {
        overlay?.show(state: state)
    }

    func hideOverlay() {
        overlay?.hide()
    }
}

struct MenuBarContent: View {
    @ObservedObject var controller: SpikeController

    var body: some View {
        // Paper design uses a popover-style HUD. SwiftUI MenuBarExtra `.menu` is a system menu;
        // `.window` is closer to a popover but still not an arbitrary NSPanel design.
        Text(controller.isRecording ? "Recording…" : "Idle")
        Text("Last hotkey: \(controller.lastHotkeyEvent)")
            .font(.caption)
        Divider()
        Button("Show listening overlay") { controller.showOverlay(state: .listening) }
        Button("Show processing overlay") { controller.showOverlay(state: .processing) }
        Button("Hide overlay") { controller.hideOverlay() }
        Divider()
        Button("Quit") { NSApp.terminate(nil) }
    }
}
