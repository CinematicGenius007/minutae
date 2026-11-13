import Foundation
import whisper

public struct Transcript: Sendable, Equatable {
    public struct Segment: Sendable, Equatable {
        public let text: String
        public let startMilliseconds: Int64
        public let endMilliseconds: Int64
        public init(text: String, startMilliseconds: Int64, endMilliseconds: Int64) {
            self.text = text; self.startMilliseconds = startMilliseconds; self.endMilliseconds = endMilliseconds
        }
    }
    public let segments: [Segment]
    public let language: String?
    public init(segments: [Segment], language: String?) { self.segments = segments; self.language = language }
}
public struct TranscriptionOptions: Sendable {
    public var language: String?
    public init(language: String? = nil) { self.language = language }
}
public protocol SpeechEngine: Sendable {
    var isLoaded: Bool { get async }
    func load(model: URL) async throws
    func transcribe(samples: [Float], options: TranscriptionOptions) async throws -> Transcript
    func unload() async
}
public enum SpeechError: Error { case modelLoadFailed, notLoaded, transcriptionFailed, invalidLanguage }

public actor FakeSpeechEngine: SpeechEngine {
    public private(set) var isLoaded = false
    public var result: Transcript
    public init(result: Transcript = Transcript(segments: [], language: nil)) { self.result = result }
    public func load(model: URL) { isLoaded = true }
    public func unload() { isLoaded = false }
    public func transcribe(samples: [Float], options: TranscriptionOptions = .init()) throws -> Transcript {
        guard isLoaded else { throw SpeechError.notLoaded }; return result
    }
}

private final class WhisperContext: @unchecked Sendable {
    let pointer: OpaquePointer
    init(_ pointer: OpaquePointer) { self.pointer = pointer }
    deinit { whisper_free(pointer) }
}

public actor WhisperCppEngine: SpeechEngine {
    private var context: WhisperContext?
    private let idleDuration: Duration
    private var idleTask: Task<Void, Never>?
    public var isLoaded: Bool { context != nil }
    public init(idleDuration: Duration = .seconds(300)) { self.idleDuration = idleDuration }
    deinit { idleTask?.cancel() }
    public func load(model: URL) throws {
        unload()
        var params = whisper_context_default_params()
        params.use_gpu = true
        guard let loaded = model.path.withCString({ whisper_init_from_file_with_params($0, params) }) else { throw SpeechError.modelLoadFailed }
        context = WhisperContext(loaded)
        scheduleUnload()
    }
    public func unload() { idleTask?.cancel(); idleTask = nil; context = nil }
    private func scheduleUnload() {
        idleTask?.cancel()
        let duration = idleDuration
        idleTask = Task { [weak self] in
            do { try await Task.sleep(for: duration) } catch { return }
            guard !Task.isCancelled else { return }
            await self?.unload()
        }
    }
    public func transcribe(samples: [Float], options: TranscriptionOptions = .init()) throws -> Transcript {
        guard let context = context?.pointer else { throw SpeechError.notLoaded }
        guard samples.count <= Int(Int32.max) else { throw SpeechError.transcriptionFailed }
        idleTask?.cancel()
        defer { scheduleUnload() }
        var params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY)
        params.print_progress = false; params.print_realtime = false; params.print_timestamps = false; params.print_special = false
        let language = options.language ?? "auto"
        guard !language.utf8.contains(0), language == "auto" || language.withCString({ whisper_lang_id($0) }) >= 0 else { throw SpeechError.invalidLanguage }
        let result = language.withCString { code in
            params.language = code
            return samples.withUnsafeBufferPointer { whisper_full(context, params, $0.baseAddress, Int32($0.count)) }
        }
        guard result == 0 else { throw SpeechError.transcriptionFailed }
        let segments = (0..<whisper_full_n_segments(context)).map { index in
            Transcript.Segment(text: String(cString: whisper_full_get_segment_text(context, index)), startMilliseconds: whisper_full_get_segment_t0(context, index) * 10, endMilliseconds: whisper_full_get_segment_t1(context, index) * 10)
        }
        let lang = whisper_lang_str(whisper_full_lang_id(context)).map { String(cString: $0) }
        return Transcript(segments: segments, language: lang)
    }
}
