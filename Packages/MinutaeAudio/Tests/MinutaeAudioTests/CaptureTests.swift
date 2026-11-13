import AVFoundation
import Foundation
import Testing
@testable import MinutaeAudio

@Test func meterAndTrim() {
    #expect(AudioMath.rms([1, -1]) == 1)
    let signal = [Float](repeating: 0, count: 320) + [Float](repeating: 0.5, count: 320) + [Float](repeating: 0, count: 640) + [Float](repeating: 0.5, count: 320) + [Float](repeating: 0, count: 6400)
    #expect(AudioMath.trim(signal).count == 6080)
}

@Test func conversion() throws {
    let format = try #require(AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 1, interleaved: false))
    let input = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 4800))
    input.frameLength = 4800
    let channel = try #require(input.floatChannelData?[0])
    for i in 0..<4800 { channel[i] = sin(Float(i) * 2 * .pi * 440 / 48_000) }
    let output = try PCMConverter.convert(input)
    #expect(abs(output.count - 1600) <= 32)
    #expect(AudioMath.rms(output) > 0.6)
}

@Test func fakeAndSubscribers() async throws {
    let fake = FakeAudioCapture(samples: [0.5, 0.25])
    let a = fake.subscribe(); let b = fake.subscribe()
    try fake.start()
    var ai = a.makeAsyncIterator(); var bi = b.makeAsyncIterator()
    #expect(await ai.next() == [0.5, 0.25])
    #expect(await bi.next() == [0.5, 0.25])
    #expect(try fake.stop().full == [0.5, 0.25])
}

@Test func wavCheckpoint() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let writer = try WAVWriter(url: url)
    try writer.append([Float](repeating: 0.25, count: 80_000))
    let audio = try AVAudioFile(forReading: url)
    #expect(audio.length == 80_000)
    try writer.close()
}
