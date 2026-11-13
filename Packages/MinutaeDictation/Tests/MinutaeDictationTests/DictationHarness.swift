import Foundation
import MinutaeCore
import MinutaeSpeech
import Testing

@testable import MinutaeDictation

struct DictationPaths {
    var root: URL
    var audio: URL
    var database: MinutaeDatabase

    static func make() throws -> DictationPaths {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("minutae-dic06-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let audio = root.appendingPathComponent("dictations", isDirectory: true)
        let database = try MinutaeDatabase(path: root.appendingPathComponent("db.sqlite").path)
        return DictationPaths(root: root, audio: audio, database: database)
    }
}

func dictationClock() throws -> Date {
    let formatter = ISO8601DateFormatter()
    formatter.formatOptions = [.withInternetDateTime]
    formatter.timeZone = TimeZone(secondsFromGMT: 0)
    guard let date = formatter.date(from: "2026-04-11T15:00:00Z") else {
        struct BadClock: Error {}
        throw BadClock()
    }
    return date
}

struct DictationGates {
    var speech: Gate
    var clean: Gate
    var insert: Gate

    init(_ plan: DictationHarnessPlan) {
        speech = Gate(opened: plan.speechOpened)
        clean = Gate(opened: plan.cleanOpened)
        insert = Gate(opened: plan.insertOpened)
    }
}

@MainActor
struct DictationAssembly {
    var plan: DictationHarnessPlan
    var paths: DictationPaths
    var clock: Date
    var hotkey: FakeHotkeyService
    var capture: ScriptedCapture
    var speech: GatedSpeech
    var gates: DictationGates
    var inserter: GatedInserter
    var frontmost: ScriptedFrontmost
    var signal: DictationCancelSignal
    var ids = SequenceID()

    func controller() -> DictationController {
        let clock = clock
        let ids = ids
        let cleaning = GatedCleaner(
            gate: gates.clean,
            replacement: plan.cleaned,
            fails: plan.cleanFails
        )
        let clients = DictationControllerClients(
            hotkey: hotkey,
            capture: capture,
            speech: speech,
            cleaning: cleaning,
            inserting: TextInserterClient(inserter)
        )
        let options = DictationControllerOptions(
            dictationsDirectory: paths.audio,
            now: { clock },
            makeID: { ids.next() }
        )
        let environment = DictationControllerEnvironment(
            store: DatabaseDictationStore(database: paths.database),
            frontmost: frontmost,
            cancels: signal,
            options: options
        )
        return DictationController(clients: clients, environment: environment)
    }
}

@MainActor
final class DictationHarness {
    let root: URL
    let audioDirectory: URL
    let database: MinutaeDatabase
    let hotkey: FakeHotkeyService
    let speech: GatedSpeech
    let speechGate: Gate
    let cleanGate: Gate
    let inserter: GatedInserter
    let insertGate: Gate
    let frontmost: ScriptedFrontmost
    let signal: DictationCancelSignal
    let controller: DictationController
    let log: EventLog

    static func open(_ plan: DictationHarnessPlan = .init()) async throws -> DictationHarness {
        let assembly = try makeAssembly(plan)
        let controller = assembly.controller()
        let log = EventLog(await controller.events())
        await controller.start()
        return DictationHarness(assembly: assembly, controller: controller, log: log)
    }

    private static func makeAssembly(_ plan: DictationHarnessPlan) throws -> DictationAssembly {
        let paths = try DictationPaths.make()
        let hotkey = FakeHotkeyService()
        try hotkey.register(HotkeyRegistration(mode: plan.mode, chord: .optionSpace))
        let capture = ScriptedCapture(samples: plan.samples)
        capture.startError = plan.startError
        capture.stopError = plan.stopError
        let gates = DictationGates(plan)
        let speech = GatedSpeech(
            gate: gates.speech, transcript: echoTranscript(), fails: plan.speechFails)
        return DictationAssembly(
            plan: plan,
            paths: paths,
            clock: try dictationClock(),
            hotkey: hotkey,
            capture: capture,
            speech: speech,
            gates: gates,
            inserter: GatedInserter(gate: gates.insert, result: plan.insertion),
            frontmost: ScriptedFrontmost(plan.target),
            signal: DictationCancelSignal()
        )
    }

    private init(assembly: DictationAssembly, controller: DictationController, log: EventLog) {
        root = assembly.paths.root
        audioDirectory = assembly.paths.audio
        database = assembly.paths.database
        hotkey = assembly.hotkey
        speech = assembly.speech
        speechGate = assembly.gates.speech
        cleanGate = assembly.gates.clean
        inserter = assembly.inserter
        insertGate = assembly.gates.insert
        frontmost = assembly.frontmost
        signal = assembly.signal
        self.controller = controller
        self.log = log
    }

    func press() { hotkey.simulate(.pressed) }

    func release() { hotkey.simulate(.released) }

    func dictations() throws -> [Dictation] {
        try Repository<Dictation>(database).all().sorted { $0.id < $1.id }
    }

    func onlyRow() throws -> Dictation {
        let rows = try dictations()
        try #require(rows.count == 1)
        return rows[0]
    }

    func waitUntil(_ label: String, _ condition: () -> Bool) async -> Bool {
        let deadline = ContinuousClock().now.advanced(by: .seconds(3))
        while ContinuousClock().now < deadline {
            if condition() { return true }
            try? await Task.sleep(for: .milliseconds(5))
        }
        Issue.record("timed out: \(label)")
        return false
    }

    func waitStatus(_ status: String) async -> Bool {
        await waitUntil("status \(status)") {
            ((try? self.dictations()) ?? []).contains { $0.status == status }
        }
    }

    func waitIgnored(_ count: Int = 1) async -> Bool {
        await waitUntil("ignored \(count)") { log.ignored() >= count }
    }

    func phases() -> [DictationPhase] {
        collapsedPhases(log.snapshot())
    }

    func lastHUD() -> DictationHUDState? {
        for event in log.snapshot().reversed() {
            if case .state(let state) = event { return state }
        }
        return nil
    }

    func releaseGates() {
        speechGate.open()
        cleanGate.open()
        insertGate.open()
    }

    func cleanup() {
        releaseGates()
        try? FileManager.default.removeItem(at: root)
    }
}

private func echoTranscript() -> MinutaeSpeech.Transcript {
    MinutaeSpeech.Transcript(
        segments: [
            MinutaeSpeech.Transcript.Segment(
                text: "hello world",
                startMilliseconds: 0,
                endMilliseconds: 1_000
            )
        ],
        language: "en"
    )
}
