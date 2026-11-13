import AVFoundation
import CoreAudio
import Foundation

/// Stateful conversion preserves the resampler's history across input buffers.
public final class PCMConverter {
    private let converter: AVAudioConverter
    private let outputFormat: AVAudioFormat
    public init(inputFormat: AVAudioFormat) throws {
        guard let format = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 16_000, channels: 1, interleaved: false),
              let converter = AVAudioConverter(from: inputFormat, to: format) else { throw CaptureError.conversionFailed }
        self.outputFormat = format
        self.converter = converter
    }
    public func convert(_ input: AVAudioPCMBuffer) throws -> [Float] {
        let capacity = AVAudioFrameCount(ceil(Double(input.frameLength) * 16_000 / input.format.sampleRate)) + 64
        guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity) else { throw CaptureError.conversionFailed }
        var supplied = false
        var error: NSError?
        let result = converter.convert(to: output, error: &error) { _, status in
            if supplied { status.pointee = .noDataNow; return nil }
            supplied = true; status.pointee = .haveData; return input
        }
        if let error { throw error }
        guard result != .error, let data = output.floatChannelData else { throw CaptureError.conversionFailed }
        return Array(UnsafeBufferPointer(start: data[0], count: Int(output.frameLength)))
    }
    public static func convert(_ input: AVAudioPCMBuffer) throws -> [Float] {
        try PCMConverter(inputFormat: input.format).convert(input)
    }
}

/// Call prepare before dictation to keep the pre-roll buffer warm. The caller owns permission.
public final class MicrophoneCapture: AudioCapture, @unchecked Sendable {
    public let buffer = CaptureBuffer()
    private let engine = AVAudioEngine()
    private var observer: NSObjectProtocol?
    private let queue = DispatchQueue(label: "MinutaeAudio.engine")
    private var listener: AudioObjectPropertyListenerBlock?
    public var wavURL: URL?
    public init() {}
    public func subscribe() -> AsyncStream<[Float]> { buffer.subscribe() }
    public func events() -> AsyncStream<CaptureEvent> { buffer.events() }
    public func prepare() throws {
        try queue.sync { try configure() }
        observer = NotificationCenter.default.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil) { [weak self] _ in self?.scheduleRecovery() }
        var address = Self.defaultInputAddress
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in self?.scheduleRecovery() }
        let status = AudioObjectAddPropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &address, queue, block)
        guard status == noErr else { throw CaptureFailure(message: "Default input listener failed: \(status)") }
        listener = block
    }
    private static var defaultInputAddress: AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(mSelector: kAudioHardwarePropertyDefaultInputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    }
    private func scheduleRecovery() {
        queue.async { [weak self] in
            guard let self else { return }
            do { try self.configure() } catch { self.buffer.fail(error) }
        }
    }
    private func configure() throws {
        engine.stop()
        let input = engine.inputNode
        input.removeTap(onBus: 0)
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else { throw CaptureError.conversionFailed }
        let converter = try PCMConverter(inputFormat: format)
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [buffer] pcm, _ in
            do { try buffer.push(converter.convert(pcm)) } catch { buffer.fail(error) }
        }
        engine.prepare(); try engine.start()
    }
    public func start(retainFullCapture: Bool) throws { try buffer.begin(wavURL: wavURL, retainFullCapture: retainFullCapture) }
    public func stop() throws -> CaptureResult { try buffer.end() }
    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
        if let listener {
            var address = Self.defaultInputAddress
            AudioObjectRemovePropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &address, queue, listener)
        }
        engine.stop()
    }
}
