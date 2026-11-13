import MinutaeCore
import MinutaeDesign
import MinutaeFeatures
import SwiftUI

@main
struct MinutaeApp: App {
    static let windowTitle = MinutaeFeaturesModule.windowTitle
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var model: DebugHarnessModel
    @State private var navigation: NavigationStore
    @State private var isSearchPresented: Bool
    @State private var meetingsModel: MeetingsViewModel?
    @State private var settingsModel: SettingsViewModel?
    @State private var usageModel: UsageViewModel?

    init() {
        MinutaeDesign.registerFonts()
        _model = State(initialValue: DebugHarnessModel())
        _navigation = State(initialValue: NavigationStore())
        _isSearchPresented = State(initialValue: false)
        _meetingsModel = State(initialValue: nil)
        _settingsModel = State(initialValue: nil)
        _usageModel = State(initialValue: nil)
    }

    private var screens: ScreenRegistry {
        var registry = ScreenRegistry.placeholders
        if let meetingsModel {
            registry = registry.replacing(.meetings) {
                AnyView(MeetingsScreen(model: meetingsModel))
            }
        }
        if let settingsModel {
            registry = registry.replacing(.settings) {
                AnyView(SettingsScreen(model: settingsModel))
            }
        }
        if let usageModel {
            registry = registry.replacing(.usage) {
                AnyView(UsageScreen(model: usageModel))
            }
        }
        return registry
    }

    private var screenEpoch: Int {
        (meetingsModel == nil ? 0 : 1) + (settingsModel == nil ? 0 : 2)
            + (usageModel == nil ? 0 : 4)
    }

    var body: some Scene {
        WindowGroup(Self.windowTitle, id: MainWindow.id) {
            AppShellView(
                navigation: navigation,
                screens: screens,
                isRecording: model.recordingMeetingID != nil,
                canRecord: model.environment != nil && !model.recordingBusy,
                actionMessage: model.actionMessage,
                bootError: model.bootError,
                onToggleRecording: { Task { await model.toggleRecording() } },
                onSearch: { isSearchPresented = true },
                isSearchPresented: $isSearchPresented,
                screenEpoch: screenEpoch
            )
            .background(WindowChrome())
            .task { await installLiveScreens() }
        }
        .defaultSize(width: Layout.windowWidth, height: Layout.windowHeight)
        .windowStyle(.hiddenTitleBar)
        .commands {
            MinutaeCommands(
                navigation: navigation,
                model: model,
                isSearchPresented: $isSearchPresented
            )
        }

        Window("Debug harness", id: "debug-harness") {
            DebugHarnessView(model: model)
        }
        .defaultSize(width: 760, height: 720)

        Window("Design gallery", id: "design-gallery") {
            DesignGalleryView()
        }
        .defaultSize(width: Layout.windowWidth, height: Layout.windowHeight)

        MenuBarExtra {
            MinutaeStatusMenu(model: model)
        } label: {
            Image(
                systemName: model.recordingMeetingID == nil
                    ? "waveform.circle" : "record.circle.fill"
            )
        }
        .menuBarExtraStyle(.menu)
    }

    /// Settings and Usage keep one model for the process. The download closure
    /// captures the sendable model manager, not the main-actor environment.
    private func installLiveScreens() async {
        await model.bootIfNeeded()
        guard let environment = model.environment else { return }
        if meetingsModel == nil {
            meetingsModel = MeetingsViewModel(
                database: environment.database,
                runner: environment.jobRunner,
                summariesDirectory: environment.summariesDirectory
            )
        }
        if settingsModel == nil {
            let manager = environment.modelManager
            let keychain = try? SecurityKeychainStore()
            settingsModel = SettingsViewModel(
                store: environment.settingsStore,
                transports: environment.transports,
                keychain: keychain,
                download: { name, progress in
                    _ = try await manager.download(name: name, progress: progress)
                }
            )
        }
        if usageModel == nil {
            usageModel = UsageViewModel(database: environment.database)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }
}
