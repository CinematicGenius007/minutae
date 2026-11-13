import Foundation
import Testing

@testable import MinutaeLLM

@Test func ollamaDefaultBaseURLIsTheLocalDaemon() {
    let url = OllamaTransport.defaultBaseURL
    #expect(url.scheme == "http")
    #expect(url.host == "127.0.0.1")
    #expect(url.port == 11434)
}

@Test func ollamaStatusDistinguishesDaemonAndModels() async throws {
    let installed = try FakeBinary()
    defer { installed.remove() }
    let downPort = try FakeOllama.closedPort()
    let down = OllamaTransport(baseURL: loopback(downPort), binaryPath: installed.path)
    let downReport = await down.ollamaStatus()
    #expect(downReport.readiness == .serverDown)
    #expect(downReport.daemonReachable == false)
    #expect(downReport.installed == true)
    #expect(downReport.models.isEmpty)
    #expect(await down.status().daemonReachable == false)
    #expect(await down.status().detail?.contains("not reachable") == true)
    do {
        _ = try await down.run(LLMRequest(prompt: "ping", timeout: .milliseconds(300), purpose: .test))
        Issue.record("Expected the daemon to be down")
    } catch LLMError.unavailable(let message) {
        #expect(message.contains("not reachable"))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }

    let empty = try FakeOllama()
    defer { empty.stop() }
    let emptyTransport = OllamaTransport(baseURL: empty.baseURL, binaryPath: installed.path)
    let emptyReport = await emptyTransport.ollamaStatus()
    #expect(emptyReport.readiness == .noModel)
    #expect(emptyReport.daemonReachable == true)
    #expect(emptyReport.version == "0.0-test")
    #expect(await emptyTransport.status().availableModels.isEmpty)
    #expect(empty.paths().contains("/api/pull") == false)

    let cloud = try FakeOllama()
    defer { cloud.stop() }
    cloud.tagsJSON = """
        {"models":[{"name":"gemma4:31b-cloud","remote_host":"https://ollama.com","remote_model":"gemma4:31b"}]}
        """
    let cloudTransport = OllamaTransport(baseURL: cloud.baseURL, binaryPath: installed.path)
    let cloudReport = await cloudTransport.ollamaStatus()
    #expect(cloudReport.readiness == .cloudOnly)
    #expect(cloudReport.models.map(\.name) == ["gemma4:31b-cloud"])
    #expect(cloudReport.models.allSatisfy { !$0.isLocal })
    #expect(await cloudTransport.status().model == nil)
    #expect(await cloudTransport.status().availableModels == ["gemma4:31b-cloud"])

    let ready = try FakeOllama()
    defer { ready.stop() }
    ready.tagsJSON = """
        {"models":[
            {"name":"gemma4:31b-cloud","remote_host":"https://ollama.com","remote_model":"gemma4:31b"},
            {"name":"llama3.2:1b","digest":"abc"},
            {"name":"suffix-only-cloud"}
        ]}
        """
    let readyTransport = OllamaTransport(baseURL: ready.baseURL, binaryPath: installed.path)
    let readyReport = await readyTransport.ollamaStatus()
    #expect(readyReport.readiness == .ready)
    #expect(readyReport.selectedModel == "llama3.2:1b")
    #expect(readyReport.models.map(\.isLocal) == [false, true, false])
    let listed = await readyTransport.status()
    #expect(listed.availableModels == ["gemma4:31b-cloud", "llama3.2:1b", "suffix-only-cloud"])
    #expect(listed.daemonReachable == true)
    #expect(listed.detail?.contains("llama3.2:1b") == true)
    #expect(listed.binaryPath == installed.resolved)
    #expect(installed.wasExecuted == false)
}

@Test func ollamaStatusNotInstalledWhenBinaryAndDaemonAreMissing() async throws {
    let port = try FakeOllama.closedPort()
    let transport = OllamaTransport(baseURL: loopback(port), binaryPath: "/nonexistent/minutae-ollama")
    let report = await transport.ollamaStatus()
    #expect(report.readiness == .notInstalled)
    #expect(report.installed == false)
    #expect(report.daemonReachable == false)
    #expect(await transport.status().binaryPath == nil)
    do {
        _ = try await transport.run(LLMRequest(prompt: "ping", timeout: .milliseconds(300), purpose: .test))
        Issue.record("Expected not installed")
    } catch LLMError.notInstalled {} catch {
        Issue.record("Unexpected error: \(error)")
    }
}

@Test func ollamaChatPostsNonStreamingJSONToTheLocalModel() async throws {
    let installed = try FakeBinary()
    defer { installed.remove() }
    let server = try FakeOllama()
    defer { server.stop() }
    server.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    let schema = #"{"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"]}"#
    let transport = OllamaTransport(model: "llama3.2:1b", baseURL: server.baseURL, binaryPath: installed.path)
    let response = try await transport.run(
        LLMRequest(
            system: "Be brief.",
            prompt: "Reply with the single word pong.",
            jsonSchema: Data(schema.utf8),
            maxOutputTokens: 16,
            timeout: .seconds(5),
            purpose: .test
        )
    )
    #expect(response.text == "pong")
    #expect(response.transport == .ollama)
    #expect(response.model == "llama3.2:1b")
    #expect(response.usage == Usage(inputTokens: 11, outputTokens: 2))
    let chat = try server.body(for: "/api/chat")
    let object = try jsonObject(chat)
    #expect(jsonBool(object["stream"]) == false)
    #expect(object["model"] as? String == "llama3.2:1b")
    let format = object["format"] as? [String: Any]
    #expect(format?["type"] as? String == "object")
    let options = object["options"] as? [String: Any]
    #expect((options?["num_predict"] as? NSNumber)?.intValue == 16)
    let messages = object["messages"] as? [[String: Any]]
    #expect(messages?.count == 2)
    #expect(messages?.first?["role"] as? String == "system")
    #expect(messages?.last?["content"] as? String == "Reply with the single word pong.")
    #expect(server.paths().contains("/api/pull") == false)
    #expect(installed.wasExecuted == false)
}

@Test func ollamaChatUsesTheStringJSONFormatWhenNoSchemaIsSet() async throws {
    let server = try FakeOllama()
    defer { server.stop() }
    server.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    let transport = OllamaTransport(baseURL: server.baseURL, binaryPath: "/nonexistent/minutae-ollama")
    let response = try await transport.run(LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test))
    #expect(response.text == "pong")
    let object = try jsonObject(try server.body(for: "/api/chat"))
    #expect(object["model"] as? String == "llama3.2:1b")
    #expect(object["format"] as? String == "json")
    #expect(jsonBool(object["stream"]) == false)
}

@Test func ollamaChatRefusesCloudMissingAndEmptyModelsWithoutPosting() async throws {
    let cloud = try FakeOllama()
    defer { cloud.stop() }
    cloud.tagsJSON = """
        {"models":[{"name":"gemma4:31b-cloud","remote_host":"https://ollama.com","remote_model":"gemma4:31b"}]}
        """
    let cloudTransport = OllamaTransport(
        model: "gemma4:31b-cloud",
        baseURL: cloud.baseURL,
        binaryPath: "/nonexistent/minutae-ollama"
    )
    do {
        _ = try await cloudTransport.run(LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test))
        Issue.record("Expected a cloud-model rejection")
    } catch LLMError.modelRejected(let message) {
        #expect(message.contains("cloud"))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }
    #expect(cloud.paths().contains("/api/chat") == false)

    let missing = try FakeOllama()
    defer { missing.stop() }
    missing.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    let missingTransport = OllamaTransport(model: "missing:latest", baseURL: missing.baseURL, binaryPath: "/nonexistent/minutae-ollama")
    do {
        _ = try await missingTransport.run(LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test))
        Issue.record("Expected a missing-model rejection")
    } catch LLMError.modelRejected(let message) {
        #expect(message.contains("not installed"))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }
    #expect(missing.paths().contains("/api/chat") == false)

    let none = try FakeOllama()
    defer { none.stop() }
    do {
        _ = try await OllamaTransport(baseURL: none.baseURL, binaryPath: "/nonexistent/minutae-ollama").run(
            LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test)
        )
        Issue.record("Expected no-model failure")
    } catch LLMError.unavailable(let message) {
        #expect(message.contains("no model"))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }
    #expect(none.paths().contains("/api/chat") == false)
    #expect(none.paths().contains("/api/pull") == false)
}

@Test func ollamaChatClassifiesInvalidOutputAndHTTPErrors() async throws {
    let invalid = try FakeOllama()
    defer { invalid.stop() }
    invalid.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    invalid.chatJSON = "not-json"
    let invalidTransport = OllamaTransport(baseURL: invalid.baseURL, binaryPath: "/nonexistent/minutae-ollama")
    do {
        _ = try await invalidTransport.run(LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test))
        Issue.record("Expected invalid output")
    } catch LLMError.invalidOutput {} catch {
        Issue.record("Unexpected error: \(error)")
    }

    let missing = try FakeOllama()
    defer { missing.stop() }
    missing.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    missing.chatStatus = 404
    missing.chatJSON = #"{"error":"model 'llama3.2:1b' not found"}"#
    do {
        _ = try await OllamaTransport(baseURL: missing.baseURL, binaryPath: "/nonexistent/minutae-ollama").run(
            LLMRequest(prompt: "ping", timeout: .seconds(5), purpose: .test)
        )
        Issue.record("Expected model rejection")
    } catch LLMError.modelRejected {} catch {
        Issue.record("Unexpected error: \(error)")
    }
}

@Test func ollamaChatTimesOutAndCancels() async throws {
    let slow = try FakeOllama()
    defer { slow.stop() }
    slow.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    slow.holdChat = true
    let transport = OllamaTransport(baseURL: slow.baseURL, binaryPath: "/nonexistent/minutae-ollama")
    do {
        _ = try await transport.run(LLMRequest(prompt: "ping", timeout: .milliseconds(200), purpose: .test))
        Issue.record("Expected timeout")
    } catch LLMError.timeout {} catch {
        Issue.record("Unexpected error: \(error)")
    }

    let held = try FakeOllama()
    defer { held.stop() }
    held.tagsJSON = #"{"models":[{"name":"llama3.2:1b"}]}"#
    held.holdChat = true
    let cancellable = OllamaTransport(baseURL: held.baseURL, binaryPath: "/nonexistent/minutae-ollama")
    let task = Task {
        try await cancellable.run(LLMRequest(prompt: "ping", timeout: .seconds(10), purpose: .test))
    }
    try await Task.sleep(for: .milliseconds(200))
    task.cancel()
    do {
        _ = try await task.value
        Issue.record("Expected cancellation")
    } catch LLMError.cancelled {} catch {
        Issue.record("Unexpected error: \(error)")
    }
}

@Test func ollamaPullReportsProgressForTheInjectedNameOnly() async throws {
    let installed = try FakeBinary()
    defer { installed.remove() }
    let server = try FakeOllama()
    defer { server.stop() }
    let transport = OllamaTransport(baseURL: server.baseURL, binaryPath: installed.path)
    _ = await transport.status()
    do {
        try await transport.pullModel("  ") { _ in Issue.record("Empty name must not report progress") }
        Issue.record("Expected rejection")
    } catch LLMError.modelRejected {} catch {
        Issue.record("Unexpected error: \(error)")
    }
    #expect(server.paths().contains("/api/pull") == false)
    let seen = ProgressLog()
    try await transport.pullModel("tiny-test:latest") { seen.append($0) }
    #expect(seen.values.map(\.status) == ["pulling manifest", "downloading", "success"])
    #expect(seen.values[1].completed == 40)
    #expect(seen.values[1].total == 100)
    let body = try jsonObject(try server.body(for: "/api/pull"))
    #expect(body["model"] as? String == "tiny-test:latest")
    #expect(jsonBool(body["stream"]) == true)
    #expect(installed.wasExecuted == false)
}

@Test func ollamaRefusesANonLoopbackURLWithoutConnecting() async throws {
    let installed = try FakeBinary()
    defer { installed.remove() }
    let transport = OllamaTransport(
        baseURL: URL(string: "http://203.0.113.1:11434") ?? URL(fileURLWithPath: "/"),
        binaryPath: installed.path
    )
    let report = await transport.ollamaStatus()
    #expect(report.daemonReachable == false)
    #expect(report.detail.contains("loopback"))
    do {
        _ = try await transport.run(LLMRequest(prompt: "ping", timeout: .milliseconds(200), purpose: .test))
        Issue.record("Expected refusal")
    } catch LLMError.unavailable(let message) {
        #expect(message.contains("loopback"))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }
    #expect(installed.wasExecuted == false)
}

private func loopback(_ port: UInt16) -> URL {
    URL(string: "http://127.0.0.1:\(port)") ?? URL(fileURLWithPath: "/")
}

private func jsonObject(_ data: Data) throws -> [String: Any] {
    try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private func jsonBool(_ value: Any?) -> Bool? {
    (value as? NSNumber)?.boolValue
}

private struct FakeBinary {
    let path: String
    let resolved: String
    private let directory: URL
    private let marker: URL

    init() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
            "ollama-bin-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
        let marker = directory.appendingPathComponent("ran")
        let file = directory.appendingPathComponent("ollama")
        let script = "#!/bin/sh\n/usr/bin/touch '\(marker.path)'\n"
        try script.write(to: file, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
        path = file.path
        resolved = URL(fileURLWithPath: file.path).resolvingSymlinksInPath().path
        self.directory = directory
        self.marker = marker
    }

    var wasExecuted: Bool { FileManager.default.fileExists(atPath: marker.path) }

    func remove() { try? FileManager.default.removeItem(at: directory) }
}

private final class FakeOllama: @unchecked Sendable {
    private var storedTagsJSON = #"{"models":[]}"#
    private var storedVersionJSON = #"{"version":"0.0-test"}"#
    private var storedChatJSON =
        #"{"model":"llama3.2:1b","message":{"role":"assistant","content":"pong"},"done":true,"prompt_eval_count":11,"eval_count":2}"#
    private var storedChatStatus = 200
    private var storedHoldChat = false
    private let lock = NSLock()

    var tagsJSON: String {
        get { lock.lock(); defer { lock.unlock() }; return storedTagsJSON }
        set { lock.lock(); storedTagsJSON = newValue; lock.unlock() }
    }
    var versionJSON: String {
        get { lock.lock(); defer { lock.unlock() }; return storedVersionJSON }
        set { lock.lock(); storedVersionJSON = newValue; lock.unlock() }
    }
    var chatJSON: String {
        get { lock.lock(); defer { lock.unlock() }; return storedChatJSON }
        set { lock.lock(); storedChatJSON = newValue; lock.unlock() }
    }
    var chatStatus: Int {
        get { lock.lock(); defer { lock.unlock() }; return storedChatStatus }
        set { lock.lock(); storedChatStatus = newValue; lock.unlock() }
    }
    var holdChat: Bool {
        get { lock.lock(); defer { lock.unlock() }; return storedHoldChat }
        set { lock.lock(); storedHoldChat = newValue; lock.unlock() }
    }
    let pullBody = """
        {"status":"pulling manifest"}
        {"status":"downloading","digest":"sha256:abc","total":100,"completed":40}
        {"status":"success"}
        """
    private let finished = DispatchSemaphore(value: 0)
    private var listenFD: Int32 = -1
    private var running = false
    private var stopped = false
    private var recorded: [(path: String, body: Data)] = []
    private(set) var baseURL = URL(fileURLWithPath: "/")

    func paths() -> [String] {
        lock.lock()
        defer { lock.unlock() }
        return recorded.map(\.path)
    }

    func body(for path: String) throws -> Data {
        lock.lock()
        defer { lock.unlock() }
        guard let body = recorded.last(where: { $0.path == path })?.body else {
            throw TestFailure("No request for \(path)")
        }
        return body
    }

    static func closedPort() throws -> UInt16 {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw TestFailure("socket") }
        defer { Darwin.close(fd) }
        let address = try Self.loopbackAddress(port: 0)
        guard Self.bind(fd, address) == 0, let port = Self.port(of: fd) else { throw TestFailure("bind") }
        return port
    }

    init() throws {
        let fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw TestFailure("socket") }
        var yes: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, socklen_t(MemoryLayout.size(ofValue: yes)))
        let address = try Self.loopbackAddress(port: 0)
        guard Self.bind(fd, address) == 0, let port = Self.port(of: fd) else {
            Darwin.close(fd)
            throw TestFailure("bind")
        }
        guard listen(fd, 16) == 0 else {
            Darwin.close(fd)
            throw TestFailure("listen")
        }
        listenFD = fd
        baseURL = URL(string: "http://127.0.0.1:\(port)") ?? URL(fileURLWithPath: "/")
        running = true
        DispatchQueue.global(qos: .userInitiated).async { [self] in
            self.serveLoop()
        }
    }

    func stop() {
        lock.lock()
        let shouldWait = running
        running = false
        stopped = true
        let fd = listenFD
        listenFD = -1
        lock.unlock()
        if fd >= 0 { Darwin.close(fd) }
        if shouldWait { finished.wait() }
    }

    private func serveLoop() {
        defer { finished.signal() }
        while true {
            lock.lock()
            let stop = stopped
            let fd = listenFD
            lock.unlock()
            if stop || fd < 0 { return }
            var storage = sockaddr_in()
            var length = socklen_t(MemoryLayout<sockaddr_in>.size)
            let client = withUnsafeMutablePointer(to: &storage) { pointer -> Int32 in
                pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                    Darwin.accept(fd, raw, &length)
                }
            }
            if client < 0 { return }
            var noPipe: Int32 = 1
            setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &noPipe, socklen_t(MemoryLayout.size(ofValue: noPipe)))
            var timeout = timeval(tv_sec: 1, tv_usec: 0)
            setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
            handle(client)
            Darwin.close(client)
        }
    }

    private func handle(_ client: Int32) {
        guard let request = readRequest(client) else { return }
        record(request)
        if request.path == "/api/chat" && holdChat && !pause(seconds: 5) { return }
        let response = response(for: request.path)
        writeAll(client, response)
    }

    private func record(_ request: (method: String, path: String, body: Data)) {
        lock.lock()
        recorded.append((request.path, request.body))
        lock.unlock()
    }

    private func response(for path: String) -> Data {
        switch path {
        case "/api/version":
            return http(status: 200, type: "application/json", body: Data(versionJSON.utf8))
        case "/api/tags":
            return http(status: 200, type: "application/json", body: Data(tagsJSON.utf8))
        case "/api/chat":
            let type = chatJSON.first == "{" ? "application/json" : "text/plain"
            return http(status: chatStatus, type: type, body: Data(chatJSON.utf8))
        case "/api/pull":
            return http(status: 200, type: "application/x-ndjson", body: Data(pullBody.utf8))
        default:
            return http(status: 404, type: "application/json", body: Data(#"{"error":"not found"}"#.utf8))
        }
    }

    private func pause(seconds: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            lock.lock()
            let stop = stopped
            lock.unlock()
            if stop { return false }
            Thread.sleep(forTimeInterval: 0.02)
        }
        return true
    }

    private func readRequest(_ fd: Int32) -> (method: String, path: String, body: Data)? {
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 8_192)
        let headerMark = Data("\r\n\r\n".utf8)
        while data.range(of: headerMark) == nil {
            let count = Darwin.read(fd, &buffer, buffer.count)
            if count <= 0 { return nil }
            data.append(buffer, count: count)
            if data.count > 1_048_576 { return nil }
        }
        guard let headerEnd = data.range(of: headerMark) else { return nil }
        let header = String(decoding: data[..<headerEnd.lowerBound], as: UTF8.self)
        let lines = header.split(separator: "\r\n", omittingEmptySubsequences: false)
        guard let requestLine = lines.first else { return nil }
        let parts = requestLine.split(separator: " ")
        guard parts.count >= 2 else { return nil }
        var body = Data(data[headerEnd.upperBound...])
        let length = contentLength(header)
        while body.count < length {
            let count = Darwin.read(fd, &buffer, buffer.count)
            if count <= 0 { break }
            body.append(buffer, count: count)
        }
        if body.count > length { body = Data(body.prefix(length)) }
        let path = String(parts[1]).split(separator: "?", maxSplits: 1).first.map(String.init) ?? "/"
        return (String(parts[0]), path, body)
    }

    private func contentLength(_ header: String) -> Int {
        for line in header.split(separator: "\r\n") {
            let pieces = line.split(separator: ":", maxSplits: 1)
            guard pieces.count == 2, pieces[0].trimmingCharacters(in: .whitespaces).lowercased() == "content-length" else {
                continue
            }
            return Int(pieces[1].trimmingCharacters(in: .whitespaces)) ?? 0
        }
        return 0
    }

    private func http(status: Int, type: String, body: Data) -> Data {
        let reason = status == 200 ? "OK" : "Error"
        var data = Data(
            """
            HTTP/1.1 \(status) \(reason)\r\n\
            Content-Type: \(type)\r\n\
            Content-Length: \(body.count)\r\n\
            Connection: close\r\n\
            \r\n
            """.utf8
        )
        data.append(body)
        return data
    }

    private func writeAll(_ fd: Int32, _ data: Data) {
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            var sent = 0
            while sent < data.count {
                let count = Darwin.write(fd, base.advanced(by: sent), data.count - sent)
                if count <= 0 { return }
                sent += count
            }
        }
    }

    private static func loopbackAddress(port: UInt16) throws -> sockaddr_in {
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        guard "127.0.0.1".withCString({ inet_pton(AF_INET, $0, &address.sin_addr) }) == 1 else {
            throw TestFailure("inet_pton")
        }
        return address
    }

    private static func bind(_ fd: Int32, _ address: sockaddr_in) -> Int32 {
        var address = address
        return withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                Darwin.bind(fd, raw, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
    }

    private static func port(of fd: Int32) -> UInt16? {
        var address = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let result = withUnsafeMutablePointer(to: &address) { pointer -> Int32 in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { raw in
                getsockname(fd, raw, &length)
            }
        }
        guard result == 0 else { return nil }
        return UInt16(bigEndian: address.sin_port)
    }
}

private final class ProgressLog: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [OllamaPullProgress] = []
    func append(_ progress: OllamaPullProgress) {
        lock.lock()
        items.append(progress)
        lock.unlock()
    }
    var values: [OllamaPullProgress] {
        lock.lock()
        defer { lock.unlock() }
        return items
    }
}

private struct TestFailure: Error, CustomStringConvertible {
    var description: String
    init(_ description: String) { self.description = description }
}
