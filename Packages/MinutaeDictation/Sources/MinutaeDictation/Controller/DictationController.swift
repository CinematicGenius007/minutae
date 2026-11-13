import Foundation
import MinutaeAudio
import MinutaeCore
import MinutaeSpeech

/// One dictation from hotkey to inserted text.
///
/// The run is idle, then listening, processing, and inserting, then idle.
/// A failure is one `error` event and then idle. Cancel comes from
/// `DictationCancelSource` (Esc is injected there). A press that neither
/// starts nor stops the current dictation emits `ignoredWhileBusy` and is
/// not queued.
///
/// The database status while the microphone is open is `listening`.
/// Schema v2 has no `recording` status.
public actor DictationController {
    private let hotkey: any HotkeyService
    private let capture: OwnedCapture
    private let speech: any SpeechEngine
    let cleaning: any DictationCleaning
    let inserting: any DictationInserting
    let store: any DictationStore
    private let frontmost: any FrontmostAppSource
    private let cancels: any DictationCancelSource
    private let directory: URL
    private let now: @Sendable () -> Date
    private let makeID: @Sendable () -> String
    private let speechModelName: @Sendable () -> String?
    private let hub = DictationEventHub()

    var activity: Activity = .idle
    var row: Dictation?
    private var startedAt: Date?
    var level: Float = 0
    var banner: String?
    var outcome: DictationOutcome?
    var arming = false
    var stopClaimed = false
    var cancelRequested = false
    /// Chord armed when listening began. Session `.ended` stops capture only
    /// when this no longer matches, which is a mode switch or unregister.
    /// Press and release stop the dictation themselves.
    var activeRegistration: HotkeyRegistration?
    private var tasks: [Task<Void, Never>] = []
    var captureTask: Task<Void, Never>?
    /// Transcription through insertion. Detached so a press during that work
    /// is observed and ignored instead of sitting in the hotkey stream.
    var pipeline: Task<Void, Never>?
    public private(set) var hud: DictationHUDState

    public init(clients: DictationControllerClients, environment: DictationControllerEnvironment) {
        hotkey = clients.hotkey
        capture = OwnedCapture(clients.capture)
        speech = clients.speech
        cleaning = clients.cleaning
        inserting = clients.inserting
        store = environment.store
        frontmost = environment.frontmost
        cancels = environment.cancels
        directory = environment.options.dictationsDirectory
        now = environment.options.now
        makeID = environment.options.makeID
        speechModelName = environment.options.speechModelName
        hud = DictationHUDState(
            phase: .idle,
            dictationID: nil,
            targetBundleID: nil,
            targetAppName: nil,
            level: 0,
            banner: nil
        )
    }

    deinit {
        for task in tasks {
            task.cancel()
        }
        captureTask?.cancel()
    }

    public func events() -> AsyncStream<DictationControllerEvent> {
        hub.stream()
    }

    /// Subscribes to the hotkey, its session edges, and the cancel source.
    public func start() {
        guard tasks.isEmpty else { return }
        let keyEvents = hotkey.events
        let sessionEvents = hotkey.session
        let cancelEvents = cancels.cancels
        tasks.append(
            watch(keyEvents) { controller, event in
                await controller.handleKey(event)
            })
        tasks.append(
            watch(sessionEvents) { controller, edge in
                await controller.handleSession(edge)
            })
        tasks.append(
            watch(cancelEvents) { controller, _ in
                await controller.cancel()
            })
        emit(.idle, banner: nil)
    }

    public func cancel() async {
        if activity == .listening {
            cancelRequested = true
            await stopListening(trigger: .cancel)
            return
        }
        if activity == .processing || activity == .inserting || arming {
            cancelRequested = true
        }
    }

    /// Starts a dictation from inside the app, or stops the one that is listening.
    ///
    /// This is the in-app equivalent of a toggle-mode hotkey press. It is ignored while a
    /// dictation is transcribing or inserting.
    public func toggleFromApp() async {
        if activity == .listening, !stopClaimed {
            await stopListening(trigger: .release)
            return
        }
        if activity == .idle, !arming {
            await begin()
            return
        }
        hub.emit(.ignoredWhileBusy)
    }
}

extension DictationController {
    private func watch<Event: Sendable>(
        _ stream: AsyncStream<Event>,
        body: @escaping @Sendable (DictationController, Event) async -> Void
    ) -> Task<Void, Never> {
        Task { [weak self] in
            for await event in stream {
                guard let self else { return }
                await body(self, event)
            }
        }
    }

    private func handleSession(_ edge: HotkeySessionEdge) async {
        guard edge == .ended else { return }
        guard hotkey.registration != activeRegistration else { return }
        await stopListening(trigger: .release)
    }

    private func handleKey(_ event: HotkeyEvent) async {
        let mode = hotkey.registration?.mode ?? .hold
        if isStop(mode: mode, phase: event.phase) {
            await stopListening(trigger: .release)
            return
        }
        if isStart(event.phase) {
            await begin()
            return
        }
        if event.phase == .pressed {
            hub.emit(.ignoredWhileBusy)
        }
    }

    private func isStart(_ phase: HotkeyPhase) -> Bool {
        phase == .pressed && activity == .idle && !arming
    }

    private func isStop(mode: HotkeyMode, phase: HotkeyPhase) -> Bool {
        guard activity == .listening, !stopClaimed else { return false }
        switch mode {
        case .hold:
            return phase == .released
        case .toggle:
            return phase == .pressed
        }
    }

    private func begin() async {
        guard activity == .idle, !arming else {
            hub.emit(.ignoredWhileBusy)
            return
        }
        arming = true
        cancelRequested = false
        let target = await frontmost.frontmostApp()
        guard activity == .idle else {
            arming = false
            return
        }
        if cancelRequested {
            clearArming()
            return
        }
        do {
            try insertListeningRow(target: target)
        } catch {
            await fail(message: "store: \(dictationStageDetail(error))")
            return
        }
        activity = .listening
        arming = false
        activeRegistration = hotkey.registration
        level = 0
        outcome = nil
        emit(.listening, banner: nil)
        watchCapture()
        do {
            try capture.capture.start(retainFullCapture: true)
        } catch {
            await fail(message: "capture start: \(dictationStageDetail(error))")
            return
        }
        if cancelRequested {
            await stopListening(trigger: .cancel)
        }
    }

    private func insertListeningRow(target: DictationTarget) throws {
        let started = now()
        let stamp = DictationAudioFile.timestamp(started)
        let created = Dictation(
            id: makeID(),
            started_at: stamp,
            duration_ms: nil,
            target_app_bundle_id: target.bundleIdentifier,
            target_app_name: target.name,
            raw_text: "",
            final_text: "",
            language: nil,
            insert_method: nil,
            status: DictationRowStatus.listening,
            audio_path: nil,
            error: nil,
            created_at: stamp,
            speech_model: speechModelName()
        )
        try store.insert(created)
        row = created
        startedAt = started
    }

    private func clearArming() {
        arming = false
        cancelRequested = false
    }

    private func watchCapture() {
        captureTask?.cancel()
        let stream = capture.capture.events()
        let task = Task { [weak self] in
            for await event in stream {
                await self?.note(event)
            }
        }
        captureTask = task
        tasks.append(task)
    }

    private func note(_ event: CaptureEvent) async {
        guard activity == .listening, !stopClaimed else { return }
        switch event {
        case .samples(let samples):
            guard !samples.isEmpty else { return }
            level = AudioMath.rms(samples)
            emit(.listening, banner: banner)
        case .failure(let failure):
            await stopListening(trigger: .captureFailed(failure.message))
        }
    }

    private func stopListening(trigger: StopTrigger) async {
        guard activity == .listening, !stopClaimed else { return }
        stopClaimed = true
        activity = .processing
        emit(.processing, banner: nil)
        let saved = saveCapture()
        if cancelRequested || trigger == .cancel {
            await completeCancel(error: saved.failure)
            return
        }
        if let failure = saved.failure {
            await fail(message: failure)
            return
        }
        if case .captureFailed(let message) = trigger {
            await fail(message: "capture: \(message)")
            return
        }
        pipeline = Task { [weak self] in
            await self?.transcribeAndContinue(saved)
        }
    }

    private func saveCapture() -> SavedAudio {
        let stopped: CaptureResult
        do {
            stopped = try capture.capture.stop()
        } catch {
            return SavedAudio(failure: "capture stop: \(dictationStageDetail(error))")
        }
        let duration = DictationAudioFile.durationMilliseconds(sampleCount: stopped.full.count)
        let url: URL
        do {
            url = try DictationAudioFile.write(
                samples: stopped.full,
                directory: directory,
                started: startedAt ?? now(),
                id: row?.id ?? makeID()
            )
        } catch {
            try? writeRow { current in
                current.duration_ms = duration
                current.status = DictationRowStatus.processing
            }
            return SavedAudio(
                duration: duration,
                failure: "wav: \(dictationStageDetail(error))"
            )
        }
        do {
            try writeRow { current in
                current.audio_path = url.path
                current.duration_ms = duration
                current.status = DictationRowStatus.processing
            }
        } catch {
            return SavedAudio(
                duration: duration,
                path: url.path,
                failure: "store: \(dictationStageDetail(error))"
            )
        }
        return SavedAudio(trimmed: stopped.trimmed, duration: duration, path: url.path)
    }

    private func transcribeAndContinue(_ saved: SavedAudio) async {
        let transcript: MinutaeSpeech.Transcript
        do {
            transcript = try await speech.transcribe(
                samples: saved.trimmed,
                options: TranscriptionOptions()
            )
        } catch {
            if cancelRequested {
                await completeCancel(error: nil)
                return
            }
            await fail(message: "transcribe: \(dictationStageDetail(error))")
            return
        }
        if cancelRequested {
            await completeCancel(error: nil)
            return
        }
        let raw = joinedTranscript(transcript)
        await cleanAndInsert(raw: raw, language: transcript.language)
    }

    func emit(_ phase: DictationPhase, banner: String?) {
        self.banner = banner
        hud = DictationHUDState(
            phase: phase,
            dictationID: row?.id,
            targetBundleID: row?.target_app_bundle_id,
            targetAppName: row?.target_app_name,
            level: level,
            banner: banner,
            outcome: outcome
        )
        hub.emit(.state(hud))
    }
}

extension DictationController {
    enum Activity {
        case idle
        case listening
        case processing
        case inserting
    }

    enum StopTrigger: Equatable {
        case release
        case cancel
        case captureFailed(String)
    }

    struct SavedAudio: Sendable {
        var trimmed: [Float] = []
        var duration: Int?
        var path: String?
        var failure: String?
    }
}
