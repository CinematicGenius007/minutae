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
    #expect(abs(output.count - 1600) <= 320)
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

@Test func wavSurvivesCrashAtCheckpoint() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let writer = try WAVWriter(url: url)
    try writer.append([Float](repeating: 0.25, count: 80_000))
    try writer.append([Float](repeating: 0.5, count: 32_000))
    let handle = try FileHandle(forWritingTo: url)
    try handle.truncate(atOffset: 44 + 80_000 * 4)
    try handle.close()
    let file = try AVAudioFile(forReading: url)
    #expect(file.length == 80_000)
    let buffer = try #require(AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: 80_000))
    try file.read(into: buffer)
    #expect(buffer.frameLength >= 79_800)
    #expect(abs(try #require(buffer.floatChannelData?[0][100]) - 0.25) < 0.001)
    withExtendedLifetime(writer) {}
}

@Test func continuousStereoConversion() throws {
    let format = try #require(AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: 48_000, channels: 2, interleaved: false))
    let converter = try PCMConverter(inputFormat: format)
    var output: [Float] = []
    var boundaries: [Int] = []
    for chunk in 0..<100 {
        let input = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 480))
        input.frameLength = 480
        let channels = try #require(input.floatChannelData)
        for i in 0..<480 {
            let sample = sin(Float(chunk * 480 + i) * 2 * .pi * 440 / 48_000) * 0.5
            channels[0][i] = sample; channels[1][i] = sample
        }
        let next = try converter.convert(input)
        if !output.isEmpty && !next.isEmpty { boundaries.append(output.count) }
        output.append(contentsOf: next)
    }
    #expect(abs(output.count - 16_000) <= 320)
    for boundary in boundaries { #expect(abs(output[boundary] - output[boundary - 1]) < 0.12) }
}

@Test func stopFinishesStreams() async throws {
    let fake = FakeAudioCapture(samples: [0.5])
    let samples = fake.subscribe()
    let events = fake.events()
    try fake.start()
    _ = try fake.stop()
    var sampleIterator = samples.makeAsyncIterator()
    var eventIterator = events.makeAsyncIterator()
    #expect(await sampleIterator.next() == [0.5])
    #expect(await sampleIterator.next() == nil)
    _ = await eventIterator.next()
    #expect(await eventIterator.next() == nil)
}

@Test func captureWithoutRetention() throws {
    let buffer = CaptureBuffer()
    try buffer.begin(retainFullCapture: false)
    try buffer.push([0.5, 0.25])
    let result = try buffer.end()
    #expect(result.full.isEmpty)
    #expect(result.trimmed.isEmpty)
}
