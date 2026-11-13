import Foundation
import Darwin

public struct SubprocessResult: Sendable {
    public let exitStatus: Int32
    public let stdout: String
    public let stderr: String
}

/// A runner rejects concurrent calls with `unavailable`; each transport may own one runner.
public actor SubprocessRunner {
    private var running = false
    private let outputLimit: Int
    private let environmentKeys: Set<String>

    public init(outputLimit: Int = 1_048_576, environmentKeys: Set<String> = ["PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL"]) {
        self.outputLimit = max(0, outputLimit)
        self.environmentKeys = environmentKeys
    }

    /// Resolve a file directly, without consulting a shell or its functions.
    public static func resolveBinary(_ binary: String, path: String = ProcessInfo.processInfo.environment["PATH"] ?? "") -> String? {
        let candidates: [String]
        if binary.hasPrefix("/") {
            candidates = [binary]
        } else if binary.contains("/") {
            return nil
        } else {
            candidates = path.split(separator: ":").filter { $0.hasPrefix("/") }.map { "\($0)/\(binary)" }
        }
        for candidate in candidates {
            let url = URL(fileURLWithPath: candidate).resolvingSymlinksInPath().standardizedFileURL
            var isDirectory: ObjCBool = false
            if FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory), !isDirectory.boolValue,
               FileManager.default.isExecutableFile(atPath: url.path) { return url.path }
        }
        return nil
    }

    public func run(binary: String, arguments: [String] = [], prompt: String, timeout: Duration) async throws -> SubprocessResult {
        guard !running else { throw LLMError.unavailable("Another subprocess is already running") }
        guard let executable = Self.resolveBinary(binary) else { throw LLMError.notInstalled }
        running = true
        defer { running = false }
        let operation = ProcessOperation(executable: executable, arguments: arguments, prompt: prompt,
                                         timeout: timeout, outputLimit: outputLimit, environmentKeys: environmentKeys)
        return try await withTaskCancellationHandler {
            if Task.isCancelled { throw LLMError.cancelled }
            return try await operation.run()
        } onCancel: {
            operation.stop(.cancelled)
        }
    }
}

private final class ProcessOperation: @unchecked Sendable {
    private let lock = NSLock()
    private var process: Process?
    private var stopped: LLMError?
    private var finished = false
    private let executable: String
    private let arguments: [String]
    private let prompt: String
    private let timeout: Duration
    private let outputLimit: Int
    private let environmentKeys: Set<String>

    init(executable: String, arguments: [String], prompt: String, timeout: Duration, outputLimit: Int, environmentKeys: Set<String>) {
        self.executable = executable; self.arguments = arguments; self.prompt = prompt
        self.timeout = timeout; self.outputLimit = outputLimit; self.environmentKeys = environmentKeys
    }

    func stop(_ reason: LLMError) {
        lock.lock()
        guard stopped == nil, !finished else { lock.unlock(); return }
        stopped = reason
        let child = process
        if let child, child.isRunning { child.terminate() }
        lock.unlock()
        if let child {
            DispatchQueue.global().asyncAfter(deadline: .now() + 0.25) { [self] in
                lock.lock(); defer { lock.unlock() }
                if child.isRunning { kill(child.processIdentifier, SIGKILL) }
            }
        }
    }

    func run() async throws -> SubprocessResult {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global().async { [self] in
                do { continuation.resume(returning: try execute()) }
                catch { continuation.resume(throwing: error) }
            }
        }
    }

    private func execute() throws -> SubprocessResult {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("minutae-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let child = Process()
        child.executableURL = URL(fileURLWithPath: executable)
        child.arguments = arguments
        child.currentDirectoryURL = directory
        child.environment = ProcessInfo.processInfo.environment.filter { environmentKeys.contains($0.key) }
        let input = Pipe(), output = Pipe(), errors = Pipe()
        child.standardInput = input; child.standardOutput = output; child.standardError = errors
        lock.lock()
        if let stopped { lock.unlock(); throw stopped }
        do { try child.run(); process = child; lock.unlock() }
        catch { lock.unlock(); throw LLMError.unavailable("Could not start subprocess") }
        let stdout = Capture(limit: outputLimit), stderr = Capture(limit: outputLimit)
        let readers = DispatchGroup()
        for (pipe, capture) in [(output, stdout), (errors, stderr)] {
            readers.enter()
            DispatchQueue.global().async {
                let fd = pipe.fileHandleForReading.fileDescriptor
                _ = fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK)
                var buffer = [UInt8](repeating: 0, count: 16_384)
                while true {
                    let count = read(fd, &buffer, buffer.count)
                    if count > 0 { capture.append(Data(buffer.prefix(count))); continue }
                    if count == 0 { break }
                    if errno == EINTR { continue }
                    if errno != EAGAIN { break }
                    self.lock.lock(); let done = self.finished; self.lock.unlock()
                    if done { break }
                    usleep(5_000)
                }
                readers.leave()
            }
        }
        DispatchQueue.global().async { [prompt] in
            try? input.fileHandleForWriting.write(contentsOf: Data(prompt.utf8))
            try? input.fileHandleForWriting.close()
        }
        let components = timeout.components
        let seconds = max(0, Double(components.seconds) + Double(components.attoseconds) / 1e18)
        let timer = DispatchWorkItem { [self] in stop(.timeout) }
        DispatchQueue.global().asyncAfter(deadline: .now() + seconds, execute: timer)
        child.waitUntilExit()
        timer.cancel()
        lock.lock(); finished = true; let reason = stopped; process = nil; lock.unlock()
        readers.wait()
        if let reason { throw reason }
        return SubprocessResult(exitStatus: child.terminationStatus, stdout: stdout.text, stderr: stderr.text)
    }
}

private final class Capture: @unchecked Sendable {
    private var data = Data()
    private let limit: Int
    init(limit: Int) { self.limit = limit }
    func append(_ chunk: Data) { data.append(chunk.prefix(max(0, limit - data.count))) }
    var text: String { String(decoding: data, as: UTF8.self) }
}
