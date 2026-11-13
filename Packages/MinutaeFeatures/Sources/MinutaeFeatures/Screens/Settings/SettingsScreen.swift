import MinutaeCore
import MinutaeDesign
import MinutaeLLM
import Observation
import SwiftUI

@MainActor @Observable
public final class SettingsViewModel {
    public var settings = AppSettings()
    public var statuses: [TransportID: TransportStatus] = [:]
    public var message = ""
    public var modelProgress: Double?
    public var modelReady = false
    public var savedKeys: Set<APIKeyAccount> = []
    public var keyAccount: APIKeyAccount = .anthropic
    public var keyDraft = ""
    private let keychain: (any KeychainStore)?
    private let store: any SettingsStore
    private let transports: [any LLMTransport]
    private let download:
        (@Sendable (String, @escaping @Sendable (Double) -> Void) async throws -> Void)?

    public init(
        store: any SettingsStore,
        transports: [any LLMTransport] = [],
        keychain: (any KeychainStore)? = nil,
        download: (@Sendable (String, @escaping @Sendable (Double) -> Void) async throws -> Void)? =
            nil
    ) {
        self.keychain = keychain
        self.store = store
        self.transports = transports
        self.download = download
    }

    public func load() async {
        do { settings = try await store.load() } catch { message = error.localizedDescription }
        if settings.codexModel == nil {
            settings.codexModel = "gpt-6-luna"
            await save()
        }
        for transport in transports { statuses[transport.id] = await transport.status() }
        if let keychain {
            for account in APIKeyAccount.allCases {
                do {
                    if try await keychain.secret(for: account) != nil { savedKeys.insert(account) }
                } catch { message = error.localizedDescription }
            }
        }
    }

    public func save() async {
        do { try await store.save(settings) } catch { message = error.localizedDescription }
    }

    public func saveKey() async {
        guard let keychain else {
            message = "Keychain is not configured."
            return
        }
        do {
            try await keychain.setSecret(keyDraft, for: keyAccount)
            savedKeys.insert(keyAccount)
            keyDraft = ""
        } catch { message = error.localizedDescription }
    }

    public func testCodex() async {
        guard let transport = transports.first(where: { $0.id == .codexCLI }) else {
            message = "Codex is not configured."
            return
        }
        do {
            let response = try await transport.run(
                .init(prompt: "Reply with pong.", timeout: .seconds(45), purpose: .test))
            message = response.text
        } catch { message = error.localizedDescription }
    }

    public func downloadModel() async {
        guard let download else {
            message = "Model download is not configured."
            return
        }
        modelProgress = 0
        do {
            try await download(settings.whisperModelName) { [weak self] progress in
                Task { @MainActor in self?.modelProgress = progress }
            }
            modelReady = true
        } catch { message = error.localizedDescription }
        modelProgress = nil
    }
}

public struct SettingsScreen: View {
    @State private var model: SettingsViewModel
    public init(model: SettingsViewModel) { _model = State(initialValue: model) }

    private static let whisperOptions: [DropdownOption<String>] = [
        .init("tiny", title: "Tiny", detail: "75 MB · fastest"),
        .init("base", title: "Base", detail: "142 MB · balanced"),
        .init("small", title: "Small", detail: "466 MB · accurate"),
        .init("medium", title: "Medium", detail: "1.5 GB · most accurate"),
    ]
    private static let keyOptions: [DropdownOption<APIKeyAccount>] = [
        .init(.anthropic, title: "Anthropic"),
        .init(.openai, title: "OpenAI"),
        .init(.gemini, title: "Gemini"),
    ]

    public var body: some View {
        @Bindable var model = model
        PageScaffold("Settings", spacing: 0) {
            section("TRANSCRIPTION")
            SettingsRow("Whisper model", description: "Runs on this Mac. Bigger is slower and more accurate.") {
                Dropdown(
                    "Whisper model",
                    selection: Binding(
                        get: { model.settings.whisperModelName },
                        set: {
                            model.settings.whisperModelName = $0
                            model.modelReady = false
                            Task { await model.save() }
                        }),
                    options: Self.whisperOptions, width: 160)
            }
            SettingsRow(
                "Model status",
                description: model.modelProgress != nil
                    ? "Downloading…" : (model.modelReady ? "Downloaded and ready." : "Not downloaded yet.")
            ) {
                if let progress = model.modelProgress {
                    ProgressTrack(value: progress).frame(width: 120)
                    Text("\(Int(progress * 100))%")
                        .font(.minutae.mono(size: 12)).foregroundStyle(Color.minutae.muted)
                } else if model.modelReady {
                    StatusPill("Ready", tone: .mint)
                } else {
                    SecondaryButton("Download") { Task { await model.downloadModel() } }
                }
            }
            section("SUMMARIES · NO API KEY NEEDED")
            ForEach(model.settings.transportFallbackOrder.filter { $0 != .api }, id: \.self) { id in
                provider(id)
            }
            SettingsRow(
                "Advanced · API key",
                description: "Optional. Stored in the macOS Keychain, never in a file."
            ) {
                Dropdown("Provider", selection: $model.keyAccount, options: Self.keyOptions, width: 130)
            }
            HStack(spacing: Spacing.s12) {
                MinutaeField(
                    model.savedKeys.contains(model.keyAccount) ? "Saved · paste to replace" : "Paste API key",
                    text: $model.keyDraft, secure: true)
                SecondaryButton("Save") { Task { await model.saveKey() } }
                    .disabled(model.keyDraft.isEmpty)
            }
            .padding(.top, Spacing.s12)
            section("DETECTION & DICTATION")
            SettingsRow("Ask when a meeting starts", description: "Prompt when microphone activity is detected.") {
                Toggle("Ask when a meeting starts", isOn: $model.settings.autoDetectMeetings)
                    .labelsHidden().toggleStyle(.minutaePine)
                    .onChange(of: model.settings.autoDetectMeetings) { _, _ in Task { await model.save() } }
            }
            SettingsRow("Keep dictation audio", description: "Save dictation audio on this Mac.") {
                Toggle("Keep dictation audio", isOn: $model.settings.keepDictationAudio)
                    .labelsHidden().toggleStyle(.minutaePine)
                    .onChange(of: model.settings.keepDictationAudio) { _, _ in Task { await model.save() } }
            }
            SettingsRow("Dictation hotkey", description: "Hold to talk, release to insert.") {
                KeyCap("⌥Space", accessibilityText: "Option Space")
            }
            if !model.message.isEmpty {
                Text(model.message)
                    .font(.minutae.ui(size: 13)).foregroundStyle(Color.minutae.muted)
                    .textSelection(.enabled)
            }
        }
        .task { await model.load() }
    }

    private func section(_ title: String) -> some View {
        Text(title)
            .font(.minutae.mono(size: 12, weight: .medium))
            .foregroundStyle(Color.minutae.muted)
            .padding(.top, Spacing.s28)
            .padding(.bottom, Spacing.s4)
    }

    private func isReady(_ id: TransportID) -> Bool {
        guard let s = model.statuses[id] else { return false }
        return s.installed && s.signedIn != false && s.daemonReachable != false
            && (id != .ollama || !s.availableModels.isEmpty)
    }

    @ViewBuilder private func provider(_ id: TransportID) -> some View {
        switch id {
        case .codexCLI:
            SettingsRow(
                "Codex",
                description: "Used first. Signed in with your ChatGPT account. Only transcript text is sent."
            ) {
                Dropdown(
                    "Codex model",
                    selection: Binding(
                        get: { model.settings.codexModel ?? "gpt-6-luna" },
                        set: {
                            model.settings.codexModel = $0
                            Task { await model.save() }
                        }),
                    options: [.init("gpt-6-luna", title: "gpt-6-luna")], width: 150)
                providerStatus(id)
                TextAction("Test") { Task { await model.testCodex() } }
            }
        case .grokCLI:
            SettingsRow("Grok", description: "Fallback when Codex is unavailable.") { providerStatus(id) }
        case .ollama:
            let models = model.statuses[id]?.availableModels ?? []
            let local = models.filter { !$0.contains("cloud") }
            SettingsRow(
                "Ollama",
                description: models.isEmpty
                    ? "No model installed. Run `ollama pull llama3.2:3b`."
                    : (local.isEmpty
                        ? "Only cloud models found, so text leaves this Mac."
                        : "Runs fully on this Mac · \(local.joined(separator: ", "))")
            ) { providerStatus(id) }
        case .api: EmptyView()
        }
    }

    private func providerStatus(_ id: TransportID) -> some View {
        isReady(id) ? StatusPill("Ready", tone: .mint) : StatusPill("Set up", tone: .butter)
    }
}

#Preview {
    SettingsScreen(model: SettingsViewModel(store: InMemorySettingsStore()))
}
