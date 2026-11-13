import ApplicationServices
import Foundation
import MinutaeAudio
import MinutaeCore
import MinutaeDesign
import MinutaeDictation
import MinutaeFeatures
import MinutaeLLM
import MinutaeLibrary
import MinutaeMeetings
import MinutaeSpeech

enum AppLaunch {
    static var wantsFakes: Bool {
        let process = ProcessInfo.processInfo
        if process.environment["MINUTAE_FAKES"] == "1" { return true }
        return process.arguments.contains("--fake")
    }

    /// The unit-test host must not open `~/Library/Minutae` or register a hotkey.
    static var isUnderTest: Bool {
        if NSClassFromString("XCTestCase") != nil { return true }
        let environment = ProcessInfo.processInfo.environment
        if environment["XCTestConfigurationFilePath"] != nil { return true }
        if environment["XCTestBundlePath"] != nil { return true }
        return ProcessInfo.processInfo.arguments.contains { argument in
            argument.contains("XCTest") || argument.hasSuffix(".xctest")
        }
    }
}

struct PipelineSmokeReport: Equatable {
    var meetingID: String
    var meetingStatus: String
    var summaryPath: String
    var dictationStatus: String
    var dictationText: String
    var insertMethod: String
    var audioPath: String
}

struct TransportLine: Identifiable, Equatable, Sendable {
    var id: TransportID
    var text: String
}

/// One process composition root. `--fake` or `MINUTAE_FAKES=1` swaps the
/// microphone, speech engine, hotkey, inserter, and LLM transports for fakes.
@MainActor
final class AppEnvironment {
    let usesFakes: Bool
    let root: URL
    let database: MinutaeDatabase
    let settingsStore: any SettingsStore
    let permissions: any PermissionService
    let jobRunner: JobRunner
    let modelManager: ModelManager
    let speech: any SpeechEngine
    let dictation: DictationController
    let meetings: MeetingRecorder
    let recordingsDirectory: URL
    let summariesDirectory: URL
    let dictationsDirectory: URL
    let linkedModules: String
    private(set) var settings: AppSettings
    private(set) var router: LLMRouter
    private(set) var transports: [any LLMTransport]
    private(set) var startupNotes: [String]
    private(set) var hotkeyArmed: Bool
    private(set) var armedHotkeyMode: String?
    private(set) var armedHotkeyCode: UInt16?
    private(set) var armedHotkeyModifiers: UInt?
    private let capture: ExplainingCapture
    private let routerBox: RouterBox
    private let usage: DatabaseUsageObserver
    private let fakeHotkey: FakeHotkeyService?
    private let hotkey: any HotkeyService
    private let dictationRegistration: HotkeyRegistration

    static func makeFromProcess() async throws -> AppEnvironment {
        if AppLaunch.wantsFakes {
            let root = MinutaeDirectories.root.appending(path: "fake", directoryHint: .isDirectory)
            return try await make(useFakes: true, root: root)
        }
        return try await make(useFakes: false, root: MinutaeDirectories.root)
    }

    static func make(useFakes: Bool, root: URL) async throws -> AppEnvironment {
        var notes: [String] = []
        if useFakes {
            notes.append(
                "Running with fakes. Audio, speech, and AI calls stay on this Mac and use stand-ins."
            )
        }
        let loaded = try await loadSettings(useFakes: useFakes, root: root)
        notes.append(contentsOf: loaded.notes)
        let recordings = URL(fileURLWithPath: loaded.settings.recordingsPath, isDirectory: true)
        let summaries = URL(fileURLWithPath: loaded.settings.summariesPath, isDirectory: true)
        let dictations = URL(fileURLWithPath: loaded.settings.dictationsPath, isDirectory: true)
        let models = root.appending(path: "models", directoryHint: .isDirectory)
        for directory in [root, recordings, summaries, dictations, models] {
            try FileManager.default.createDirectory(
                at: directory, withIntermediateDirectories: true)
        }
        let database = try MinutaeDatabase(path: root.appending(path: "minutae.db").path)
        let permissions: any PermissionService =
            useFakes
            ? FakePermissionService(
                statuses: Dictionary(
                    uniqueKeysWithValues: Permission.allCases.map { ($0, .granted) }))
            : SystemPermissionService()
        let capture = ExplainingCapture(useFakes ? FixtureSpeech.capture() : MicrophoneCapture())
        let speech = await makeSpeech(
            useFakes: useFakes,
            settings: loaded.settings,
            models: models,
            notes: &notes
        )
        let usage = DatabaseUsageObserver(database: database)
        let transports = makeTransports(
            useFakes: useFakes, settings: loaded.settings, notes: &notes)
        let router = LLMRouter(
            chain: chain(from: loaded.settings),
            transports: transports,
            observer: usage
        )
        let box = RouterBox(router)
        let runner = JobRunner(
            database: database,
            handlers: [
                .transcribe: TranscribeJobHandler(database: database, engine: speech).handler,
                .summarise: SummariseJobHandler(
                    database: database,
                    generator: CurrentRouterGenerator(box: box),
                    summariesDirectory: summaries
                ).handler,
            ]
        )
        let meetings = MeetingRecorder(
            database: database,
            recordingsDirectory: recordings,
            capture: capture
        )
        let hotkey: any HotkeyService
        let fakeHotkey: FakeHotkeyService?
        if useFakes {
            let fake = FakeHotkeyService()
            hotkey = fake
            fakeHotkey = fake
        } else {
            hotkey = CarbonHotkeyService()
            fakeHotkey = nil
        }
        let inserter: any DictationInserting
        if useFakes {
            inserter = TextInserterClient(FakeTextInserter())
        } else {
            inserter = TextInserterClient(SystemTextInserter(trusted: { AXIsProcessTrusted() }))
        }
        let frontmost: any FrontmostAppSource =
            useFakes
            ? FixedFrontmostApp()
            : WorkspaceFrontmostAppSource()
        let speechModelName = loaded.settings.whisperModelName
        let dictation = DictationController(
            clients: DictationControllerClients(
                hotkey: hotkey,
                capture: capture,
                speech: speech,
                cleaning: PostProcessorClient(
                    LocalPostProcessor(dictionary: DatabaseDictionaryRepository(database))
                ),
                inserting: inserter
            ),
            environment: DictationControllerEnvironment(
                store: DatabaseDictationStore(database: database),
                frontmost: frontmost,
                cancels: DictationCancelSignal(),
                options: DictationControllerOptions(
                    dictationsDirectory: dictations,
                    speechModelName: { speechModelName })
            )
        )
        let registration = hotkeyRegistration(from: loaded.settings.dictationHotkey)
        let armed = arm(hotkey, registration: registration, notes: &notes)
        await dictation.start()
        do {
            try await meetings.recover()
        } catch {
            notes.append(
                "A previous recording could not be recovered. \(AppMessages.describe(error))")
        }
        do {
            try await runner.start()
        } catch {
            notes.append("Background jobs did not start. \(AppMessages.describe(error))")
        }
        if !useFakes, permissions.status(of: .microphone) == .granted {
            if let message = capture.prepareIfPossible() {
                notes.append(message)
            }
        } else if !useFakes {
            notes.append(
                AppMessages.permissionBlocked(
                    .microphone, status: permissions.status(of: .microphone)))
        }

        return AppEnvironment(
            usesFakes: useFakes,
            root: root,
            database: database,
            settingsStore: loaded.store,
            permissions: permissions,
            jobRunner: runner,
            modelManager: ModelManager(directory: models),
            speech: speech,
            dictation: dictation,
            meetings: meetings,
            recordingsDirectory: recordings,
            summariesDirectory: summaries,
            dictationsDirectory: dictations,
            linkedModules: [
                MinutaeDesignModule.name,
                MinutaeFeaturesModule.name,
                MinutaeLibraryModule.name,
            ].joined(separator: ", "),
            settings: loaded.settings,
            router: router,
            transports: transports,
            startupNotes: notes,
            hotkeyArmed: armed.armed,
            armedHotkeyMode: armed.mode,
            armedHotkeyCode: armed.keyCode,
            armedHotkeyModifiers: armed.modifiers,
            capture: capture,
            routerBox: box,
            usage: usage,
            fakeHotkey: fakeHotkey,
            hotkey: hotkey,
            dictationRegistration: registration
        )
    }

    @discardableResult
    func prepareMicrophone() -> String? {
        capture.prepareIfPossible()
    }

    /// The dictation controller has no enable flag. The armed hotkey is the switch.
    func setDictationEnabled(_ enabled: Bool) throws {
        if enabled {
            try hotkey.register(dictationRegistration)
            hotkeyArmed = true
            return
        }
        hotkey.unregister()
        hotkeyArmed = false
    }

    func transportLines() async -> [TransportLine] {
        var lines: [TransportLine] = []
        for transport in transports {
            let status = await transport.status()
            lines.append(TransportLine(id: transport.id, text: AppMessages.transportLine(status)))
        }
        return lines
    }

    func testTransport(_ id: TransportID) async -> String {
        guard transports.contains(where: { $0.id == id }) else {
            return "\(AppMessages.transportName(id)) is not configured."
        }
        let probe = LLMRouter(chain: [id], transports: transports, observer: usage)
        do {
            let response = try await probe.run(
                LLMRequest(
                    prompt: "Reply with the single word pong.",
                    timeout: .seconds(45),
                    purpose: .test
                )
            )
            let text = response.text.trimmingCharacters(in: .whitespacesAndNewlines)
            let clipped = text.count > 160 ? String(text.prefix(160)) + "…" : text
            return "\(AppMessages.transportName(id)) replied: \(clipped)"
        } catch {
            return AppMessages.describe(error)
        }
    }

    func setCodexModel(_ name: String) async throws {
        var updated = try await settingsStore.load()
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        updated.codexModel = trimmed.isEmpty ? nil : trimmed
        try await settingsStore.save(updated)
        settings = updated
        guard !usesFakes else { return }
        var ignored: [String] = []
        transports = Self.makeTransports(useFakes: false, settings: updated, notes: &ignored)
        let replacement = LLMRouter(
            chain: Self.chain(from: updated),
            transports: transports,
            observer: usage
        )
        router = replacement
        routerBox.replace(replacement)
    }

    func modelStatusLine() async -> String {
        if usesFakes {
            return "Fake speech engine is loaded. No Whisper model file is required."
        }
        let name = settings.whisperModelName
        let loaded = await speech.isLoaded
        do {
            if try modelManager.isInstalled(name: name) {
                if loaded { return "\(name) is installed and loaded." }
                return
                    "\(name) is installed but not loaded. Load it before dictation or transcription."
            }
            return AppMessages.missingModel(name)
        } catch {
            return AppMessages.describe(error)
        }
    }

    func ensureWhisperModel(onProgress: @escaping @Sendable (Double) -> Void) async throws {
        if usesFakes {
            throw AppEnvironmentError(message: "Fake mode does not download a Whisper model.")
        }
        let name = settings.whisperModelName
        let url: URL
        if try modelManager.isInstalled(name: name) {
            url = try modelManager.modelURL(name: name)
        } else {
            url = try await modelManager.download(name: name, progress: onProgress)
        }
        try await speech.load(model: url)
    }

    func summaryFile(meetingID: String) -> URL? {
        let needle = "id: \(meetingID)"
        guard
            let files = try? FileManager.default.contentsOfDirectory(
                at: summariesDirectory,
                includingPropertiesForKeys: nil
            )
        else { return nil }
        for file in files where file.pathExtension == "md" {
            guard let handle = try? FileHandle(forReadingFrom: file) else { continue }
            let data = try? handle.read(upToCount: 512)
            try? handle.close()
            if let data, let text = String(data: data, encoding: .utf8), text.contains(needle) {
                return file
            }
        }
        return nil
    }

    func runPipelineSmoke() async throws -> PipelineSmokeReport {
        guard usesFakes, let fakeHotkey else {
            throw AppEnvironmentError(message: "The pipeline smoke run only works with fakes.")
        }
        let meetingID = try await meetings.start(title: "Smoke meeting")
        _ = try await meetings.stop()
        let summary = try await waitForSummary(meetingID)
        try await Task.sleep(for: .milliseconds(50))
        guard fakeHotkey.simulate(.pressed) else {
            throw AppEnvironmentError(message: "The fake hotkey is not armed.")
        }
        try await waitForDictation(status: "listening")
        guard fakeHotkey.simulate(.released) else {
            throw AppEnvironmentError(message: "The fake hotkey is not armed.")
        }
        let row = try await waitForDictation(status: "done")
        let meeting = try Repository<Meeting>(database).find(meetingID)
        return PipelineSmokeReport(
            meetingID: meetingID,
            meetingStatus: meeting?.status ?? "",
            summaryPath: summary.path,
            dictationStatus: row.status,
            dictationText: row.final_text,
            insertMethod: row.insert_method ?? "",
            audioPath: row.audio_path ?? ""
        )
    }

    private func waitForSummary(_ meetingID: String) async throws -> URL {
        let deadline = Date().addingTimeInterval(20)
        while Date() < deadline {
            if let meeting = try Repository<Meeting>(database).find(meetingID) {
                if meeting.status == "failed" {
                    throw AppEnvironmentError(
                        message: "The meeting failed. \(jobDetail(meetingID))")
                }
                if meeting.status == "done", let file = summaryFile(meetingID: meetingID) {
                    return file
                }
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw AppEnvironmentError(
            message: "Timed out waiting for the summary. \(jobDetail(meetingID))")
    }

    private func waitForDictation(status: String) async throws -> Dictation {
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            let rows = try Repository<Dictation>(database).all()
            if status != "failed", let failed = rows.first(where: { $0.status == "failed" }) {
                throw AppEnvironmentError(message: failed.error ?? "Dictation failed.")
            }
            if let match = rows.first(where: { $0.status == status }) {
                return match
            }
            try await Task.sleep(for: .milliseconds(50))
        }
        let rows = try Repository<Dictation>(database).all()
        let detail = rows.map { "\($0.status) \($0.error ?? "")" }.joined(separator: "; ")
        throw AppEnvironmentError(
            message: "Timed out waiting for dictation status \(status). \(detail)")
    }

    private func jobDetail(_ meetingID: String) -> String {
        let jobs = (try? Repository<Job>(database).all()) ?? []
        return jobs.filter { $0.subject_id == meetingID }.map { job in
            "\(job.kind)=\(job.state) \(job.last_error ?? "")"
        }.joined(separator: "; ")
    }

    private struct LoadedSettings {
        var store: any SettingsStore
        var settings: AppSettings
        var notes: [String]
    }

    private static func loadSettings(useFakes: Bool, root: URL) async throws -> LoadedSettings {
        if useFakes {
            var seeded = AppSettings()
            seeded.recordingsPath = root.appending(path: "recordings").path
            seeded.summariesPath = root.appending(path: "summaries").path
            seeded.dictationsPath = root.appending(path: "dictations").path
            let store = try InMemorySettingsStore(settings: seeded)
            return LoadedSettings(store: store, settings: seeded, notes: [])
        }
        // The suite name equals the app's bundle id, which Foundation rejects (nil); the standard
        // domain for this app is that same domain.
        let defaults: UserDefaults =
            Bundle.main.bundleIdentifier == UserDefaultsSettingsStore.suiteName
            ? .standard
            : (UserDefaults(suiteName: UserDefaultsSettingsStore.suiteName) ?? .standard)
        let store = UserDefaultsSettingsStore(userDefaults: defaults)
        do {
            let settings = try await store.load()
            return LoadedSettings(store: store, settings: settings, notes: [])
        } catch {
            return LoadedSettings(
                store: store,
                settings: .defaults,
                notes: [
                    "Settings could not be read. Defaults are in use. \(AppMessages.describe(error))"
                ]
            )
        }
    }

    private static func makeSpeech(
        useFakes: Bool,
        settings: AppSettings,
        models: URL,
        notes: inout [String]
    ) async -> ReportingSpeechEngine {
        let name = settings.whisperModelName
        if useFakes {
            let fake = FakeSpeechEngine(result: FixtureSpeech.transcript)
            await fake.load(model: models)
            return ReportingSpeechEngine(engine: fake, modelName: name)
        }
        let engine = ReportingSpeechEngine(engine: WhisperCppEngine(), modelName: name)
        let manager = ModelManager(directory: models)
        do {
            guard try manager.isInstalled(name: name) else {
                notes.append(AppMessages.missingModel(name))
                return engine
            }
            try await engine.load(model: try manager.modelURL(name: name))
        } catch {
            notes.append(AppMessages.describe(error))
        }
        return engine
    }

    private static func makeTransports(
        useFakes: Bool,
        settings: AppSettings,
        notes: inout [String]
    ) -> [any LLMTransport] {
        if settings.apiTransportEnabled || settings.transportFallbackOrder.contains(.api) {
            notes.append(
                "The API transport is not in this build. Codex, Grok, and Ollama are used.")
        }
        if useFakes {
            return TransportID.defaultFallbackOrder.map { id in
                FixtureTransport(id: id, reply: FixtureSpeech.summaryJSON)
            }
        }
        return [
            CodexTransport(codexModel: settings.codexModel ?? ""),
            GrokTransport(),
            OllamaTransport(),
        ]
    }

    private static func chain(from settings: AppSettings) -> [TransportID] {
        let available: Set<TransportID> = [.codexCLI, .grokCLI, .ollama]
        let filtered = settings.transportFallbackOrder.filter { available.contains($0) }
        return filtered.isEmpty ? TransportID.defaultFallbackOrder : filtered
    }

    private struct ArmedHotkey {
        var armed: Bool
        var mode: String?
        var keyCode: UInt16?
        var modifiers: UInt?
    }

    private static func arm(
        _ hotkey: any HotkeyService,
        registration: HotkeyRegistration,
        notes: inout [String]
    ) -> ArmedHotkey {
        do {
            try hotkey.register(registration)
            return ArmedHotkey(
                armed: true,
                mode: registration.mode.rawValue,
                keyCode: registration.chord.keyCode,
                modifiers: registration.chord.modifiers.rawValue
            )
        } catch {
            notes.append(AppMessages.describe(error))
            return ArmedHotkey(armed: false, mode: nil, keyCode: nil, modifiers: nil)
        }
    }

    private static func hotkeyRegistration(from hotkey: DictationHotkey) -> HotkeyRegistration {
        let mode: HotkeyMode = hotkey.mode == .hold ? .hold : .toggle
        let modifiers = MinutaeDictation.HotkeyModifier(rawValue: hotkey.chord.modifiers.rawValue)
        let chord = MinutaeDictation.HotkeyChord(
            keyCode: hotkey.chord.keyCode, modifiers: modifiers)
        return HotkeyRegistration(mode: mode, chord: chord)
    }

    private init(
        usesFakes: Bool,
        root: URL,
        database: MinutaeDatabase,
        settingsStore: any SettingsStore,
        permissions: any PermissionService,
        jobRunner: JobRunner,
        modelManager: ModelManager,
        speech: any SpeechEngine,
        dictation: DictationController,
        meetings: MeetingRecorder,
        recordingsDirectory: URL,
        summariesDirectory: URL,
        dictationsDirectory: URL,
        linkedModules: String,
        settings: AppSettings,
        router: LLMRouter,
        transports: [any LLMTransport],
        startupNotes: [String],
        hotkeyArmed: Bool,
        armedHotkeyMode: String?,
        armedHotkeyCode: UInt16?,
        armedHotkeyModifiers: UInt?,
        capture: ExplainingCapture,
        routerBox: RouterBox,
        usage: DatabaseUsageObserver,
        fakeHotkey: FakeHotkeyService?,
        hotkey: any HotkeyService,
        dictationRegistration: HotkeyRegistration
    ) {
        self.usesFakes = usesFakes
        self.root = root
        self.database = database
        self.settingsStore = settingsStore
        self.permissions = permissions
        self.jobRunner = jobRunner
        self.modelManager = modelManager
        self.speech = speech
        self.dictation = dictation
        self.meetings = meetings
        self.recordingsDirectory = recordingsDirectory
        self.summariesDirectory = summariesDirectory
        self.dictationsDirectory = dictationsDirectory
        self.linkedModules = linkedModules
        self.settings = settings
        self.router = router
        self.transports = transports
        self.startupNotes = startupNotes
        self.hotkeyArmed = hotkeyArmed
        self.armedHotkeyMode = armedHotkeyMode
        self.armedHotkeyCode = armedHotkeyCode
        self.armedHotkeyModifiers = armedHotkeyModifiers
        self.capture = capture
        self.routerBox = routerBox
        self.usage = usage
        self.fakeHotkey = fakeHotkey
        self.hotkey = hotkey
        self.dictationRegistration = dictationRegistration
    }
}

final class RouterBox: @unchecked Sendable {
    private let lock = NSLock()
    private var router: LLMRouter

    init(_ router: LLMRouter) {
        self.router = router
    }

    func current() -> LLMRouter {
        lock.lock()
        defer { lock.unlock() }
        return router
    }

    func replace(_ router: LLMRouter) {
        lock.lock()
        self.router = router
        lock.unlock()
    }
}

struct CurrentRouterGenerator: SummaryGenerating {
    let box: RouterBox

    func generate(transcript: String) async throws -> SummaryGeneration {
        try await LLMSummaryGenerator(router: box.current()).generate(transcript: transcript)
    }
}

struct ReportingSpeechEngine: SpeechEngine {
    private let engine: any SpeechEngine
    private let modelName: String

    init(engine: any SpeechEngine, modelName: String) {
        self.engine = engine
        self.modelName = modelName
    }

    var isLoaded: Bool {
        get async { await engine.isLoaded }
    }

    func load(model: URL) async throws {
        do {
            try await engine.load(model: model)
        } catch {
            throw SpeechLoadFailure(
                message:
                    "The Whisper model “\(modelName)” could not be loaded. \(AppMessages.describe(error))"
            )
        }
    }

    func transcribe(samples: [Float], options: TranscriptionOptions) async throws
        -> MinutaeSpeech.Transcript
    {
        guard await engine.isLoaded else {
            throw SpeechLoadFailure(message: AppMessages.missingModel(modelName))
        }
        do {
            return try await engine.transcribe(samples: samples, options: options)
        } catch SpeechError.notLoaded {
            throw SpeechLoadFailure(message: AppMessages.missingModel(modelName))
        } catch SpeechError.modelLoadFailed {
            throw SpeechLoadFailure(
                message: "The Whisper model “\(modelName)” could not be loaded.")
        }
    }

    func unload() async {
        await engine.unload()
    }
}

final class ExplainingCapture: AudioCapture, @unchecked Sendable {
    private let inner: any AudioCapture
    private let prepareLock = NSLock()
    private var didPrepare = false

    init(_ inner: any AudioCapture) {
        self.inner = inner
    }

    func prepareIfPossible() -> String? {
        guard let microphone = inner as? MicrophoneCapture else { return nil }
        prepareLock.lock()
        if didPrepare {
            prepareLock.unlock()
            return nil
        }
        didPrepare = true
        prepareLock.unlock()
        do {
            try microphone.prepare()
            return nil
        } catch {
            prepareLock.lock()
            didPrepare = false
            prepareLock.unlock()
            return "The microphone is not ready. \(AppMessages.describe(error))"
        }
    }

    func events() -> AsyncStream<CaptureEvent> { inner.events() }

    func subscribe() -> AsyncStream<[Float]> { inner.subscribe() }

    func start(retainFullCapture: Bool) throws {
        do {
            try inner.start(retainFullCapture: retainFullCapture)
        } catch let error as CaptureError {
            throw CaptureFailure(message: AppMessages.describe(error))
        }
    }

    func stop() throws -> CaptureResult {
        do {
            return try inner.stop()
        } catch let error as CaptureError {
            throw CaptureFailure(message: AppMessages.describe(error))
        }
    }
}

private struct FixtureTransport: LLMTransport {
    let id: TransportID
    let reply: String

    func status() async -> TransportStatus {
        TransportStatus(
            installed: true,
            signedIn: true,
            model: "fake",
            detail: "Fake transport for smoke runs."
        )
    }

    func run(_ request: LLMRequest) async throws -> LLMResponse {
        LLMResponse(text: reply, transport: id, model: "fake", duration: .milliseconds(1))
    }
}

private enum FixtureSpeech {
    static let transcript = MinutaeSpeech.Transcript(
        segments: [
            .init(text: "hello from the fake engine", startMilliseconds: 0, endMilliseconds: 1_000)
        ],
        language: "en"
    )

    static let summaryJSON = """
        {"title":"Smoke meeting","summary":["Smoke summary bullet"],"keyDecisions":[],"actionItems":[],"participants":[]}
        """

    static func capture() -> FakeAudioCapture {
        FakeAudioCapture(samples: Array(repeating: 0.2, count: 16_000))
    }
}

private struct FixedFrontmostApp: FrontmostAppSource {
    func frontmostApp() async -> DictationTarget {
        DictationTarget(bundleIdentifier: "com.apple.TextEdit", name: "TextEdit")
    }
}
