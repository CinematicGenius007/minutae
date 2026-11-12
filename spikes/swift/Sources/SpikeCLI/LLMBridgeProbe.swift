import Foundation

enum LLMBridgeProbe {
    static func run(args: [String]) async throws {
        guard let transport = args.first else {
            throw ProbeFailure(code: .invalidOutput, detail: "usage: llm codex|grok")
        }
        var timeoutSec: TimeInterval = 60
        if let idx = args.firstIndex(of: "--timeout"), args.indices.contains(idx + 1) {
            timeoutSec = TimeInterval(args[idx + 1]) ?? 60
        }

        switch transport {
        case "codex":
            try await runCodex(timeout: timeoutSec)
        case "grok":
            try await runGrok(timeout: timeoutSec)
        case "cancel-demo":
            try await runCancelDemo()
        default:
            throw ProbeFailure(code: .invalidOutput, detail: "unknown transport \(transport)")
        }
    }

    static func runCodex(timeout: TimeInterval) async throws {
        let model = ProcessInfo.processInfo.environment["MINUTAE_CODEX_MODEL"]
            ?? ProcessInfo.processInfo.environment["CODEX_MODEL"]
        guard let model, !model.isEmpty else {
            throw ProbeFailure(
                code: .invalidOutput,
                detail: "set MINUTAE_CODEX_MODEL (or CODEX_MODEL) — model is not hardcoded"
            )
        }

        let codex = resolveBinary("codex")
        guard FileManager.default.isExecutableFile(atPath: codex) else {
            throw ProbeFailure(code: .notInstalled, detail: "codex not found at \(codex)")
        }

        let cwd = try emptyTempDir(prefix: "minutae-spike-codex")
        let lastMessage = cwd.appendingPathComponent("last.txt")
        FileManager.default.createFile(atPath: lastMessage.path, contents: nil)

        let result = try await runProcess(
            executable: codex,
            arguments: [
                "exec",
                "--skip-git-repo-check",
                "--sandbox", "read-only",
                "--ephemeral",
                "--ignore-user-config",
                "--ignore-rules",
                "--json",
                "--color", "never",
                "--model", model,
                "-C", cwd.path,
                "-o", lastMessage.path,
                "-",
            ],
            stdin: "Reply with the single word pong.",
            cwd: cwd,
            timeout: timeout
        )
        print("=== codex ===")
        print("binary: \(codex)")
        print("model: \(model)")
        print("exit: \(result.exitCode)")
        print("classified: \(classifyCodex(result).rawValue)")
        print("--- stdout (truncated) ---")
        print(truncate(result.stdout, limit: 2000))
        print("--- stderr (truncated) ---")
        print(truncate(result.stderr, limit: 2000))
        let last = (try? String(contentsOf: lastMessage, encoding: .utf8)) ?? ""
        print("--- last-message ---")
        print(truncate(last, limit: 500))
    }

    static func runGrok(timeout: TimeInterval) async throws {
        let grok = resolveBinary("grok")
        guard FileManager.default.isExecutableFile(atPath: grok) else {
            throw ProbeFailure(code: .notInstalled, detail: "grok not found at \(grok)")
        }

        let cwd = try emptyTempDir(prefix: "minutae-spike-grok")
        let promptFile = cwd.appendingPathComponent("prompt.txt")
        try "Reply with the single word pong.".write(to: promptFile, atomically: true, encoding: .utf8)

        let result = try await runProcess(
            executable: grok,
            arguments: [
                "--prompt-file", promptFile.path,
                "--output-format", "json",
                "--tools", "",
                "--disable-web-search",
                "--no-subagents",
                "--max-turns", "1",
                "--cwd", cwd.path,
                "--verbatim",
            ],
            stdin: nil,
            cwd: cwd,
            timeout: timeout
        )
        print("=== grok ===")
        print("binary: \(grok)")
        print("exit: \(result.exitCode)")
        print("classified: \(classifyGrok(result).rawValue)")
        print("--- stdout (truncated) ---")
        print(truncate(result.stdout, limit: 2000))
        print("--- stderr (truncated) ---")
        print(truncate(result.stderr, limit: 2000))
    }

    static func runCancelDemo() async throws {
        let cwd = try emptyTempDir(prefix: "minutae-spike-cancel")
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sleep")
        process.arguments = ["30"]
        process.currentDirectoryURL = cwd
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        try await Task.sleep(nanoseconds: 200_000_000)
        process.terminate()
        process.waitUntilExit()
        print("=== cancel-demo ===")
        print("exit: \(process.terminationStatus)")
        print("classified: \(ClassifiedError.cancelled.rawValue)")
        print("detail: Process.terminate() after 200ms — maps to cancelled")
    }

    private static func classifyCodex(_ result: ProcessResult) -> ClassifiedError {
        if result.timedOut { return .timeout }
        if result.cancelled { return .cancelled }
        let blob = (result.stdout + "\n" + result.stderr).lowercased()
        if result.exitCode == 127 || blob.contains("no such file") { return .notInstalled }
        if blob.contains("not logged") || blob.contains("auth") || blob.contains("sign in")
            || blob.contains("unauthorized")
        {
            return .notLoggedIn
        }
        if blob.contains("rate limit") || blob.contains("429") { return .rateLimited }
        if blob.contains("model is not supported") || blob.contains("turn.failed") {
            return .invalidOutput
        }
        if result.exitCode == 0 { return .unknown } // success — caller inspects stdout/last-message
        return .unknown
    }

    private static func classifyGrok(_ result: ProcessResult) -> ClassifiedError {
        if result.timedOut { return .timeout }
        if result.cancelled { return .cancelled }
        let blob = (result.stdout + "\n" + result.stderr).lowercased()
        if result.exitCode == 127 { return .notInstalled }
        if blob.contains("auth") || blob.contains("login") || blob.contains("unauthorized") {
            return .notLoggedIn
        }
        if blob.contains("rate limit") || blob.contains("429") { return .rateLimited }
        if result.exitCode != 0 { return .unknown }
        return .unknown
    }

    private static func resolveBinary(_ name: String) -> String {
        let envKey = "MINUTAE_\(name.uppercased())_PATH"
        if let override = ProcessInfo.processInfo.environment[envKey], !override.isEmpty {
            return override
        }
        let candidates: [String]
        switch name {
        case "codex":
            candidates = ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"]
        case "grok":
            candidates = [
                "\(NSHomeDirectory())/.grok/bin/grok",
                "\(NSHomeDirectory())/.local/bin/grok",
                "/opt/homebrew/bin/grok",
            ]
        default:
            candidates = ["/usr/bin/\(name)"]
        }
        for path in candidates where FileManager.default.isExecutableFile(atPath: path) {
            return path
        }
        return candidates[0]
    }
}

struct ProcessResult {
    var exitCode: Int32
    var stdout: String
    var stderr: String
    var timedOut: Bool
    var cancelled: Bool
}

func runProcess(
    executable: String,
    arguments: [String],
    stdin: String?,
    cwd: URL,
    timeout: TimeInterval
) async throws -> ProcessResult {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: executable)
    process.arguments = arguments
    process.currentDirectoryURL = cwd

    let out = Pipe()
    let err = Pipe()
    process.standardOutput = out
    process.standardError = err

    let inPipe = Pipe()
    if stdin != nil {
        process.standardInput = inPipe
    } else {
        process.standardInput = FileHandle.nullDevice
    }

    try process.run()
    if let stdin {
        inPipe.fileHandleForWriting.write(Data(stdin.utf8))
        try inPipe.fileHandleForWriting.close()
    }

    let timedOut = await withTaskGroup(of: Bool.self) { group in
        group.addTask {
            process.waitUntilExit()
            return false
        }
        group.addTask {
            try? await Task.sleep(nanoseconds: UInt64(timeout * 1_000_000_000))
            if process.isRunning {
                process.terminate()
                return true
            }
            return false
        }
        let first = await group.next() ?? false
        group.cancelAll()
        return first
    }

    let stdout = String(data: out.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    let stderr = String(data: err.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
    return ProcessResult(
        exitCode: process.terminationStatus,
        stdout: stdout,
        stderr: stderr,
        timedOut: timedOut,
        cancelled: timedOut == false && process.terminationReason == .uncaughtSignal
    )
}

func emptyTempDir(prefix: String) throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("\(prefix)-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: url.path)
    return url
}

func truncate(_ text: String, limit: Int) -> String {
    if text.count <= limit { return text }
    return String(text.prefix(limit)) + "\n…(truncated)"
}
