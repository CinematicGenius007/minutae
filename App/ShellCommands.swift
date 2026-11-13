import MinutaeFeatures
import SwiftUI

struct MinutaeCommands: Commands {
    @Environment(\.openWindow) private var openWindow
    var navigation: NavigationStore
    var model: DebugHarnessModel
    @Binding var isSearchPresented: Bool

    var body: some Commands {
        CommandMenu("Go") {
            Button("Meetings") { navigation.select(.meetings) }
                .keyboardShortcut("1", modifiers: .command)
            Button("Library") { navigation.select(.library) }
                .keyboardShortcut("2", modifiers: .command)
            Button("Dictate") { navigation.select(.dictate) }
                .keyboardShortcut("3", modifiers: .command)
            Button("Usage") { navigation.select(.usage) }
                .keyboardShortcut("4", modifiers: .command)
            Button("Settings") { navigation.select(.settings) }
                .keyboardShortcut("5", modifiers: .command)
            Divider()
            Button("Search") { isSearchPresented.toggle() }
                .keyboardShortcut("k", modifiers: .command)
            Button("Toggle Recording") { Task { await model.toggleRecording() } }
                .keyboardShortcut("r", modifiers: [.command, .shift])
        }
        CommandMenu("Debug") {
            Button("Open debug harness") { openWindow(id: "debug-harness") }
            Button("Design gallery") { openWindow(id: "design-gallery") }
        }
    }
}

struct MinutaeStatusMenu: View {
    @Environment(\.openWindow) private var openWindow
    var model: DebugHarnessModel

    var body: some View {
        Button(model.recordingMeetingID == nil ? "Start recording" : "Stop recording") {
            Task { await model.toggleRecording() }
        }
        .disabled(model.environment == nil || model.recordingBusy)
        Button("Open Minutae") { MainWindow.show(openWindow) }
        Button(model.dictationEnabled ? "Dictation off" : "Dictation on") {
            model.toggleDictation()
        }
        .disabled(model.environment == nil)
        Divider()
        Button("Quit") { NSApp.terminate(nil) }
    }
}
