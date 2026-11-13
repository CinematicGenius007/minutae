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

    public var body: some View {
        @Bindable var model = model
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.s20) {
                Text("Settings").font(.minutae.display(size: 36))
                section("TRANSCRIPTION")
                SettingsRow("Whisper model", description: "Choose the local transcription model.") {
                    Picker("Whisper model", selection: $model.settings.whisperModelName) {
                        Text("Tiny · 75 MB").tag("tiny")
                        Text("Base · 142 MB").tag("base")
                        Text("Small · 466 MB").tag("small")
                        Text("Medium · 1.5 GB").tag("medium")
                    }.labelsHidden().onChange(of: model.settings.whisperModelName) { _, _ in
                        model.modelReady = false
                        Task { await model.save() }
                    }
                }
                SettingsRow(
                    "Model status",
                    description: model.modelReady
                        ? "Ready" : "Download the selected model to transcribe locally."
                ) {
                    if let progress = model.modelProgress {
                        ProgressTrack(value: progress).frame(width: 100)
                    }
                    SecondaryButton("Download selected model") {
                        Task { await model.downloadModel() }
                    }
                }
                section("SUMMARIES · NO API KEY NEEDED")
                ForEach(model.settings.transportFallbackOrder, id: \.self) { id in
                    provider(id)
                }
                SettingsRow(
                    "Advanced · API key",
                    description:
                        "Optional Anthropic, OpenAI or Gemini keys are stored in macOS Keychain."
                ) {
                    VStack {
                        Picker("Provider", selection: $model.keyAccount) {
                            Text("Anthropic").tag(APIKeyAccount.anthropic)
                            Text("OpenAI").tag(APIKeyAccount.openai)
                            Text("Gemini").tag(APIKeyAccount.gemini)
                        }.labelsHidden()
                        Text(model.savedKeys.contains(model.keyAccount) ? "✓ saved" : "not set")
                        SecureField("API key", text: $model.keyDraft).frame(width: 140)
                        SecondaryButton("Save") { Task { await model.saveKey() } }
                    }
                }
                section("DETECTION & DICTATION")
                SettingsRow(
                    "Ask when a meeting starts",
                    description: "Prompt when microphone activity is detected."
                ) {
                    Toggle("Ask when a meeting starts", isOn: $model.settings.autoDetectMeetings)
                        .labelsHidden().toggleStyle(.minutaePine)
                        .onChange(of: model.settings.autoDetectMeetings) { _, _ in
                            Task { await model.save() }
                        }
                }
                SettingsRow(
                    "Keep dictation audio", description: "Save dictation audio on this Mac."
                ) {
                    Toggle("Keep dictation audio", isOn: $model.settings.keepDictationAudio)
                        .labelsHidden().toggleStyle(.minutaePine)
                        .onChange(of: model.settings.keepDictationAudio) { _, _ in
                            Task { await model.save() }
                        }
                }
                SettingsRow("Dictation hotkey", description: "Hold to talk.") {
                    KeyCap("⌥Space", accessibilityText: "Option Space")
                }
                if !model.message.isEmpty {
                    Text(model.message).font(.minutae.ui(size: 13)).foregroundStyle(
                        Color.minutae.muted)
                }
            }.frame(maxWidth: 680, alignment: .leading).padding(Spacing.s28)
        }.foregroundStyle(Color.minutae.ink).background(Color.minutae.bg).task {
            await model.load()
        }
    }

    private func section(_ title: String) -> some View {
        Text(title).font(.minutae.mono(size: 12, weight: .medium)).foregroundStyle(
            Color.minutae.muted)
    }

    @ViewBuilder private func provider(_ id: TransportID) -> some View {
        switch id {
        case .codexCLI:
            SettingsRow(
                "Codex",
                description:
                    "Used first. Signed in with your ChatGPT account. Only the transcript text is sent."
            ) {
                Picker(
                    "Codex model",
                    selection: Binding(
                        get: { model.settings.codexModel ?? "gpt-6-luna" },
                        set: {
                            model.settings.codexModel = $0
                            Task { await model.save() }
                        })
                ) { Text("gpt-6-luna").tag("gpt-6-luna") }.labelsHidden()
                SecondaryButton("Test") { Task { await model.testCodex() } }
            }
        case .grokCLI:
            SettingsRow("Grok", description: model.statuses[id]?.detail ?? "grok-4.7") {
                status(id)
            }
        case .ollama:
            SettingsRow(
                "Ollama",
                description: "Runs fully on this Mac. "
                    + (model.statuses[id]?.availableModels.joined(separator: ", ").isEmpty == false
                        ? model.statuses[id]!.availableModels.joined(separator: ", ")
                        : "No local model.")
            ) { status(id) }
        case .api: EmptyView()
        }
    }

    private func status(_ id: TransportID) -> some View {
        Text(
            model.statuses[id].map {
                $0.installed && $0.signedIn != false && $0.daemonReachable != false
                    && (id != .ollama || !$0.availableModels.isEmpty)
            } == true ? "✓ ready" : "set up"
        )
        .font(.minutae.ui(size: 13)).foregroundStyle(Color.minutae.muted)
    }
}

#Preview {
    SettingsScreen(model: SettingsViewModel(store: InMemorySettingsStore()))
}
