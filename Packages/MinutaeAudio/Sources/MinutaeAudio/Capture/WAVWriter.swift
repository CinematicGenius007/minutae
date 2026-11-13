import Foundation

/// Float32 IEEE WAV. Header is checkpointed every five seconds of audio.
public final class WAVWriter {
    private let file: FileHandle
    private var bytes: UInt32 = 0
    private var checkpoint: UInt32 = 0
    public init(url: URL) throws {
        _ = FileManager.default.createFile(atPath: url.path, contents: nil)
        file = try FileHandle(forWritingTo: url)
        try patch()
    }
    public func append(_ samples: [Float]) throws {
        var data = Data(capacity: samples.count * 4)
        for sample in samples { data.appendLE(sample.bitPattern) }
        guard data.count <= Int(UInt32.max - bytes) else { throw CocoaError(.fileWriteOutOfSpace) }
        try file.seekToEnd(); try file.write(contentsOf: data)
        bytes += UInt32(data.count)
        if bytes - checkpoint >= 16_000 * 4 * 5 { try patch() }
    }
    private func patch() throws {
        var header = Data("RIFF".utf8); header.appendLE(bytes + 36)
        header.append(Data("WAVEfmt ".utf8)); header.appendLE(UInt32(16))
        header.appendLE(UInt16(3)); header.appendLE(UInt16(1)); header.appendLE(UInt32(16_000))
        header.appendLE(UInt32(64_000)); header.appendLE(UInt16(4)); header.appendLE(UInt16(32))
        header.append(Data("data".utf8)); header.appendLE(bytes)
        try file.seek(toOffset: 0); try file.write(contentsOf: header); try file.synchronize()
        checkpoint = bytes
    }
    public func close() throws { try patch(); try file.close() }
}
private extension Data {
    mutating func appendLE<T: FixedWidthInteger>(_ value: T) {
        var little = value.littleEndian
        Swift.withUnsafeBytes(of: &little) { append(contentsOf: $0) }
    }
}
