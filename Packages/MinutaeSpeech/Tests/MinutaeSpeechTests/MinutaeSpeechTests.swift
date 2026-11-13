import Foundation
import Testing
@testable import MinutaeSpeech

private struct FakeDownload: ModelDownloading {
    let file: URL
    func download(from url: URL, progress: @escaping @Sendable (Double) -> Void) async throws -> URL {
        #expect(url.host == "huggingface.co")
        #expect(url.path == "/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin")
        progress(1)
        return file
    }
}
@Test func rejectsUnsafeModelNames() throws {
    let manager = ModelManager(directory: URL(fileURLWithPath: "/tmp/models"))
    for name in ["", ".", "..", "../tiny", "tiny/../../x", "/tiny", "tiny.bin", "tiny\\x", "TINY", "tiny ", " tiny", "tiny\n", "tiny%2f", "tiny\0x", "é", "large", "tiny-en"] {
        #expect(throws: ModelError.self) { try manager.modelURL(name: name) }
    }
    #expect(try manager.modelURL(name: "tiny").lastPathComponent == "ggml-tiny.bin")
    #expect(try !manager.isInstalled(name: "tiny"))
}
@Test func fakeEngineLifecycle() async throws {
    let result = Transcript(segments: [.init(text: "hello", startMilliseconds: 0, endMilliseconds: 100)], language: "en")
    let engine = FakeSpeechEngine(result: result)
    await #expect(throws: SpeechError.self) { try await engine.transcribe(samples: [], options: .init()) }
    await engine.load(model: URL(fileURLWithPath: "/model"))
    #expect(await engine.isLoaded)
    #expect(try await engine.transcribe(samples: [0], options: .init()) == result)
    await engine.unload()
    #expect(await !engine.isLoaded)
}
@Test func downloadRejectsInvalidSizeAndPreservesExistingModel() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("download")
    let existing = root.appendingPathComponent("ggml-tiny.bin")
    try Data("bad".utf8).write(to: file)
    try Data("old".utf8).write(to: existing)
    let manager = ModelManager(directory: root, downloader: FakeDownload(file: file))
    await #expect(throws: ModelError.self) { try await manager.download(name: "tiny") }
    #expect(try Data(contentsOf: existing) == Data("old".utf8))
    #expect(!FileManager.default.fileExists(atPath: file.path))
}
@Test func downloadRejectsWrongHash() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appendingPathComponent("download")
    _ = FileManager.default.createFile(atPath: file.path, contents: nil)
    let handle = try FileHandle(forWritingTo: file)
    try handle.truncate(atOffset: 77691713)
    try handle.close()
    let manager = ModelManager(directory: root, downloader: FakeDownload(file: file))
    await #expect(throws: ModelError.self) { try await manager.download(name: "tiny") }
    #expect(try !manager.isInstalled(name: "tiny"))
}
@Test(.enabled(if: !(ProcessInfo.processInfo.environment["MINUTAE_WHISPER_MODEL"] ?? "").isEmpty, "Set MINUTAE_WHISPER_MODEL to run")) func latencyBenchmark() async throws {
    guard let path = ProcessInfo.processInfo.environment["MINUTAE_WHISPER_MODEL"], !path.isEmpty else {
        return
    }
    let engine = WhisperCppEngine()
    try await engine.load(model: URL(fileURLWithPath: path))
    let samples = [Float](repeating: 0, count: 160_000)
    let clock = ContinuousClock()
    let start = clock.now
    _ = try await engine.transcribe(samples: samples, options: .init())
    print("Whisper 10-second generated fixture: \(start.duration(to: clock.now))")
    await engine.unload()
}
