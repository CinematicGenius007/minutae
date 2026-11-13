import AVFoundation
import Foundation

/// Samples are mono, normalized Float32 PCM at 16 kHz.
public protocol AudioCapture: AnyObject {
    func subscribe() -> AsyncStream<[Float]>
    func start() throws
    func stop() throws -> CaptureResult
}

public struct CaptureResult: Sendable {
    public let full: [Float]
    public let trimmed: [Float]
}

public enum AudioMath {
    public static func rms(_ samples: [Float]) -> Float {
        guard !samples.isEmpty else { return 0 }
        return sqrt(samples.reduce(Float.zero) { $0 + $1 * $1 } / Float(samples.count))
    }

    /// Trims only the edges, retaining a hangover after the last voiced frame.
    public static func trim(_ samples: [Float], thresholdDB: Float = -45, hangover: Int = 4_800) -> [Float] {
        let frame = 320
        let threshold = pow(10, thresholdDB / 20)
        var first: Int?
        var last = 0
        for start in stride(from: 0, to: samples.count, by: frame) {
            let end = min(start + frame, samples.count)
            if rms(Array(samples[start..<end])) > threshold {
                if first == nil { first = start }
                last = end
            }
        }
        guard let first else { return [] }
        return Array(samples[first..<min(samples.count, last + hangover)])
    }
}

/// Serializes fan-out and keeps pre-roll even while no dictation is active.
public final class CaptureBuffer: @unchecked Sendable {
    private let lock = NSLock()
    private var subscribers: [UUID: AsyncStream<[Float]>.Continuation] = [:]
    private var ring: [Float] = []
    private var captured: [Float]?
    private var writer: WAVWriter?
    public init() {}
    public func subscribe() -> AsyncStream<[Float]> {
        let id = UUID()
        return AsyncStream { continuation in
            lock.lock(); subscribers[id] = continuation; lock.unlock()
            continuation.onTermination = { [weak self] _ in self?.remove(id) }
        }
    }
    private func remove(_ id: UUID) { lock.lock(); subscribers.removeValue(forKey: id); lock.unlock() }
    public func begin(wavURL: URL? = nil) throws {
        lock.lock(); defer { lock.unlock() }
        guard captured == nil else { throw CaptureError.alreadyRecording }
        let newWriter = try wavURL.map { try WAVWriter(url: $0) }
        try newWriter?.append(ring)
        writer = newWriter
        captured = ring
    }
    public func push(_ samples: [Float]) throws {
        lock.lock(); defer { lock.unlock() }
        ring.append(contentsOf: samples)
        if ring.count > 4_800 { ring.removeFirst(ring.count - 4_800) }
        if captured != nil { try writer?.append(samples); captured?.append(contentsOf: samples) }
        for continuation in subscribers.values { continuation.yield(samples) }
    }
    public func end(thresholdDB: Float = -45) throws -> CaptureResult {
        lock.lock(); defer { lock.unlock() }
        guard let full = captured else { throw CaptureError.notRecording }
        try writer?.close(); writer = nil; captured = nil
        return CaptureResult(full: full, trimmed: AudioMath.trim(full, thresholdDB: thresholdDB))
    }
}
public enum CaptureError: Error { case alreadyRecording, notRecording, conversionFailed }

public final class FakeAudioCapture: AudioCapture {
    public let buffer = CaptureBuffer()
    private let samples: [Float]
    public init(samples: [Float]) { self.samples = samples }
    public func subscribe() -> AsyncStream<[Float]> { buffer.subscribe() }
    public func start() throws { try buffer.begin(); try buffer.push(samples) }
    public func stop() throws -> CaptureResult { try buffer.end() }
}
