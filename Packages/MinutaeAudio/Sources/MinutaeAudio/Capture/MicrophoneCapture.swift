import AVFoundation
import Foundation

/// Conversion can also be exercised with synthetic input without microphone access.
public enum PCMConverter {
    public static func convert(_ input: AVAudioPCMBuffer) throws -> [Float] {
        guard let outputFormat = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false),
              let converter = AVAudioConverter(from: input.format, to: outputFormat),
              let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: AVAudioFrameCount(ceil(Double(input.frameLength) * 16_000 / input.format.sampleRate)) + 32) else {
            throw CaptureError.conversionFailed
        }
        var supplied = false
        var error: NSError?
        converter.convert(to: output, error: &error) { _, status in
            if supplied { status.pointee = .endOfStream; return nil }
            supplied = true; status.pointee = .haveData; return input
        }
        if let error { throw error }
        guard let data = output.floatChannelData else { throw CaptureError.conversionFailed }
        return Array(UnsafeBufferPointer(start: data[0], count: Int(output.frameLength)))
    }
}

/// Keep the engine running before dictation to provide pre-roll. Permission is managed by the caller.
public final class MicrophoneCapture: AudioCapture, @unchecked Sendable {
    public let buffer = CaptureBuffer()
    private let engine = AVAudioEngine()
    private var observer: NSObjectProtocol?
    private let queue = DispatchQueue(label: "MinutaeAudio.engine")
    public var wavURL: URL?
    public init() {}
    public func subscribe() -> AsyncStream<[Float]> { buffer.subscribe() }
    public func prepare() throws {
        try queue.sync { try configure() }
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { [weak self] _ in
            self?.queue.async { [weak self] in try? self?.configure() }
        }
    }
    private func configure() throws {
        engine.stop()
        let input = engine.inputNode
        input.removeTap(onBus: 0)
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.conversionFailed }
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [buffer] pcm, _ in
            if let samples = try? PCMConverter.convert(pcm) { try? buffer.push(samples) }
        }
        engine.prepare(); try engine.start()
    }
    public func start() throws { try buffer.begin(wavURL: wavURL) }
    public func stop() throws -> CaptureResult { try buffer.end() }
    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        engine.stop()
    }
}
