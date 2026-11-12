import AVFoundation
import Foundation
import WhisperKit

enum SpeechProbe {
    static func run(args: [String]) async throws {
        let mode = args.first ?? "whisper-cpp"
        var seconds: Double = 10
        if let idx = args.firstIndex(of: "--seconds"), args.indices.contains(idx + 1) {
            seconds = Double(args[idx + 1]) ?? 10
        }

        switch mode {
        case "record":
            let url = try await recordMic(seconds: seconds)
            print("recording: \(url.path)")
            print("note: delete after measuring — do not commit")
        case "whisper-cpp":
            try await runWhisperCpp(seconds: seconds)
        case "whisperkit":
            try await runWhisperKit(seconds: seconds)
        case "from-file":
            guard let path = args.dropFirst().first else {
                throw ProbeFailure(code: .invalidOutput, detail: "usage: speech from-file <wav>")
            }
            try await transcribeExisting(path: path)
        default:
            throw ProbeFailure(
                code: .invalidOutput,
                detail: "usage: speech record|whisper-cpp|whisperkit|from-file"
            )
        }
    }

    private static func runWhisperCpp(seconds: Double) async throws {
        let wav = try await prepareAudio(seconds: seconds)
        defer { try? FileManager.default.removeItem(at: wav) }

        let model = ProcessInfo.processInfo.environment["SPIKE_WHISPER_MODEL"]
            ?? "\(NSHomeDirectory())/Library/Application Support/com.nebula.minutae/models/ggml-tiny.bin"
        guard FileManager.default.fileExists(atPath: model) else {
            throw ProbeFailure(code: .notInstalled, detail: "ggml model missing at \(model)")
        }
        let binary = ProcessInfo.processInfo.environment["SPIKE_WHISPER_CLI"]
            ?? "/opt/homebrew/bin/whisper-cli"
        guard FileManager.default.isExecutableFile(atPath: binary) else {
            throw ProbeFailure(code: .notInstalled, detail: "whisper-cli missing at \(binary)")
        }

        let outDir = try emptyTempDir(prefix: "minutae-spike-whisper")
        defer { try? FileManager.default.removeItem(at: outDir) }
        let outPrefix = outDir.appendingPathComponent("out")

        let memBefore = currentMemoryBytes()
        let t0 = Date()
        let result = try await runProcess(
            executable: binary,
            arguments: [
                "-m", model,
                "-f", wav.path,
                "-otxt",
                "-of", outPrefix.path,
                "-np",
                "-nt",
            ],
            stdin: nil,
            cwd: outDir,
            timeout: 120
        )
        let total = Date().timeIntervalSince(t0)
        let memAfter = currentMemoryBytes()
        let txtPath = outPrefix.appendingPathExtension("txt")
        let text = (try? String(contentsOf: txtPath, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        // whisper-cli does not stream first token easily; approximate TTFT as total for short clips
        print("=== whisper.cpp via Swift Process ===")
        print("chip: \(chipName())")
        print("model: \(model)")
        print("binary: \(binary)")
        print("audio_seconds: \(seconds)")
        print("exit: \(result.exitCode)")
        print("time_to_first_text_sec: \(String(format: "%.3f", total))  # no token stream; equals total")
        print("total_sec: \(String(format: "%.3f", total))")
        print("rss_delta_mb: \(String(format: "%.1f", Double(max(0, memAfter - memBefore)) / 1_048_576))")
        print("transcript_chars: \(text.count)")
        print("transcript_redacted: <omitted — do not commit>")
        if result.exitCode != 0 {
            print("--- stderr ---")
            print(truncate(result.stderr, limit: 1500))
        }
    }

    private static func runWhisperKit(seconds: Double) async throws {
        let allowDownload = ProcessInfo.processInfo.environment["SPIKE_ALLOW_MODEL_DOWNLOAD"] == "1"
        let modelFolder = ProcessInfo.processInfo.environment["SPIKE_WHISPERKIT_MODEL_FOLDER"]
        if modelFolder == nil && !allowDownload {
            print("=== WhisperKit ===")
            print("result: fails")
            print("why: no local CoreML model folder; SPIKE_ALLOW_MODEL_DOWNLOAD not set")
            print("note: spike rules forbid non-Ollama network. Set SPIKE_WHISPERKIT_MODEL_FOLDER to a pre-fetched model to measure.")
            print("chip: \(chipName())")
            return
        }

        let wav = try await prepareAudio(seconds: seconds)
        defer { try? FileManager.default.removeItem(at: wav) }

        let memBefore = currentMemoryBytes()
        let loadStart = Date()
        let kit: WhisperKit
        if let modelFolder {
            kit = try await WhisperKit(modelFolder: modelFolder, verbose: false, logLevel: .error)
        } else {
            // tiny for speed when explicitly allowed
            kit = try await WhisperKit(model: "tiny", verbose: false, logLevel: .error)
        }
        let loadSec = Date().timeIntervalSince(loadStart)
        print("whisperkit_load_sec: \(String(format: "%.3f", loadSec))")

        // Warm pass (discard)
        _ = try await kit.transcribe(audioPath: wav.path)
        let warmMem = currentMemoryBytes()

        let t0 = Date()
        var firstTextAt: Date?
        let results = try await kit.transcribe(audioPath: wav.path)
        let text = results.map(\.text).joined(separator: " ")
        if firstTextAt == nil, !text.isEmpty { firstTextAt = Date() }
        let total = Date().timeIntervalSince(t0)
        let ttft = firstTextAt.map { $0.timeIntervalSince(t0) } ?? total
        let memAfter = currentMemoryBytes()

        print("=== WhisperKit ===")
        print("chip: \(chipName())")
        print("model: \(modelFolder ?? "tiny (downloaded)")")
        print("audio_seconds: \(seconds)")
        print("time_to_first_text_sec: \(String(format: "%.3f", ttft))  # batch API; equals total unless streaming used")
        print("total_sec: \(String(format: "%.3f", total))")
        print("rss_delta_mb_warm: \(String(format: "%.1f", Double(max(0, memAfter - warmMem)) / 1_048_576))")
        print("rss_delta_mb_including_load: \(String(format: "%.1f", Double(max(0, memAfter - memBefore)) / 1_048_576))")
        print("transcript_chars: \(text.trimmingCharacters(in: .whitespacesAndNewlines).count)")
        print("transcript_redacted: <omitted — do not commit>")
    }

    private static func transcribeExisting(path: String) async throws {
        try await runWhisperCppOnFile(path: path)
    }

    private static func runWhisperCppOnFile(path: String) async throws {
        let model = ProcessInfo.processInfo.environment["SPIKE_WHISPER_MODEL"]
            ?? "\(NSHomeDirectory())/Library/Application Support/com.nebula.minutae/models/ggml-tiny.bin"
        let binary = ProcessInfo.processInfo.environment["SPIKE_WHISPER_CLI"]
            ?? "/opt/homebrew/bin/whisper-cli"
        let outDir = try emptyTempDir(prefix: "minutae-spike-whisper")
        let outPrefix = outDir.appendingPathComponent("out")
        let t0 = Date()
        let result = try await runProcess(
            executable: binary,
            arguments: ["-m", model, "-f", path, "-otxt", "-of", outPrefix.path, "-np", "-nt"],
            stdin: nil,
            cwd: outDir,
            timeout: 120
        )
        print("total_sec: \(String(format: "%.3f", Date().timeIntervalSince(t0)))")
        print("exit: \(result.exitCode)")
    }

    /// Prefer a spoken sample via `say` (deterministic, no live mic needed in CI-like runs),
    /// fall back to mic if SPIKE_USE_MIC=1.
    private static func prepareAudio(seconds: Double) async throws -> URL {
        if ProcessInfo.processInfo.environment["SPIKE_USE_MIC"] == "1" {
            return try await recordMic(seconds: seconds)
        }
        return try synthesizeSpeechWav(seconds: seconds)
    }

    private static func synthesizeSpeechWav(seconds: Double) throws -> URL {
        let dir = try emptyTempDir(prefix: "minutae-spike-audio")
        let aiff = dir.appendingPathComponent("speech.aiff")
        let wav = dir.appendingPathComponent("speech.wav")
        // ~10s of spoken filler
        let phrase = Array(repeating: "Testing one two three four five.", count: max(1, Int(seconds / 2))).joined(separator: " ")
        let say = Process()
        say.executableURL = URL(fileURLWithPath: "/usr/bin/say")
        say.arguments = ["-o", aiff.path, phrase]
        try say.run()
        say.waitUntilExit()
        if say.terminationStatus != 0 {
            throw ProbeFailure(code: .unknown, detail: "say failed")
        }
        let af = Process()
        af.executableURL = URL(fileURLWithPath: "/usr/bin/afconvert")
        af.arguments = ["-f", "WAVE", "-d", "LEI16@16000", aiff.path, wav.path]
        try af.run()
        af.waitUntilExit()
        if af.terminationStatus != 0 {
            throw ProbeFailure(code: .unknown, detail: "afconvert failed")
        }
        try? FileManager.default.removeItem(at: aiff)
        print("audio_source: say+afconvert (not live mic; set SPIKE_USE_MIC=1 for mic)")
        return wav
    }

    private static func recordMic(seconds: Double) async throws -> URL {
        let session = AVAudioApplication.shared
        let granted = await AVAudioApplication.requestRecordPermission()
        guard granted else {
            throw ProbeFailure(code: .unknown, detail: "microphone permission denied")
        }
        _ = session

        let dir = try emptyTempDir(prefix: "minutae-spike-mic")
        let url = dir.appendingPathComponent("mic.wav")
        let settings: [String: Any] = [
            AVFormatIDKey: Int(kAudioFormatLinearPCM),
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
        ]
        let recorder = try AVAudioRecorder(url: url, settings: settings)
        recorder.prepareToRecord()
        guard recorder.record() else {
            throw ProbeFailure(code: .unknown, detail: "AVAudioRecorder.record() failed")
        }
        try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
        recorder.stop()
        print("audio_source: microphone \(seconds)s")
        print("mic_permission: granted")
        return url
    }

    private static func chipName() -> String {
        var size = 0
        sysctlbyname("machdep.cpu.brand_string", nil, &size, nil, 0)
        var buf = [CChar](repeating: 0, count: size)
        sysctlbyname("machdep.cpu.brand_string", &buf, &size, nil, 0)
        return String(cString: buf)
    }

    private static func currentMemoryBytes() -> UInt64 {
        var info = mach_task_basic_info()
        var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size) / 4
        let kr = withUnsafeMutablePointer(to: &info) { ptr in
            ptr.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { intPtr in
                task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), intPtr, &count)
            }
        }
        guard kr == KERN_SUCCESS else { return 0 }
        return info.resident_size
    }
}
