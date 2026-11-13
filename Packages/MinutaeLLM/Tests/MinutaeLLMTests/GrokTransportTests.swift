import Foundation
import Testing

@testable import MinutaeLLM

@Test func grokKnownLocationsAreInstallPathsOnly() {
    #expect(
        GrokTransport.knownLocations(home: "/Users/example") == [
            "/Users/example/.grok/bin/grok",
            "/Users/example/.local/bin/grok",
            "/opt/homebrew/bin/grok",
            "/usr/local/bin/grok",
        ]
    )
}

@Test func grokPathExecutableBeatsKnownLocation() throws {
    let root = try tempRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let pathBinary = root.appendingPathComponent("path/grok")
    let homeBinary = root.appendingPathComponent("home/.grok/bin/grok")
    try writeExecutable(pathBinary)
    try writeExecutable(homeBinary)
    let resolved = GrokTransport.resolveBinary(
        path: pathBinary.deletingLastPathComponent().path,
        home: root.appendingPathComponent("home").path)
    #expect(resolved == realPath(pathBinary))
}

@Test func grokSymlinkResolvesToTheFile() throws {
    let root = try tempRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let bin = root.appendingPathComponent("path")
    let real = bin.appendingPathComponent("real-grok")
    try writeExecutable(real)
    try FileManager.default.createSymbolicLink(
        at: bin.appendingPathComponent("grok"), withDestinationURL: real)
    let resolved = GrokTransport.resolveBinary(
        path: bin.path, home: root.appendingPathComponent("empty").path)
    #expect(resolved == realPath(real))
}

@Test func grokKnownLocationWhenPathHasNoExecutable() throws {
    let root = try tempRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let home = root.appendingPathComponent("home")
    let binary = home.appendingPathComponent(".local/bin/grok")
    try writeExecutable(binary)
    let resolved = GrokTransport.resolveBinary(path: "/nonexistent-minutae-path", home: home.path)
    #expect(resolved == realPath(binary))
}

@Test func grokSkipsNonExecutablePathEntry() throws {
    let root = try tempRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let pathDir = root.appendingPathComponent("path")
    try FileManager.default.createDirectory(at: pathDir, withIntermediateDirectories: true)
    let decoy = pathDir.appendingPathComponent("grok")
    try "#!/bin/sh\necho SHOULD_NOT_RUN\n".write(to: decoy, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: decoy.path)
    try FileManager.default.createDirectory(
        at: pathDir.appendingPathComponent("dir-grok"), withIntermediateDirectories: true)
    let home = root.appendingPathComponent("home")
    let binary = home.appendingPathComponent(".grok/bin/grok")
    try writeExecutable(binary)
    let resolved = GrokTransport.resolveBinary(path: pathDir.path, home: home.path)
    #expect(resolved == realPath(binary))
}

@Test func grokOverrideDoesNotFallThrough() throws {
    let root = try tempRoot()
    defer { try? FileManager.default.removeItem(at: root) }
    let binary = root.appendingPathComponent("path/grok")
    try writeExecutable(binary)
    let path = binary.deletingLastPathComponent().path
    #expect(
        GrokTransport.resolveBinary(path: path, home: root.path, override: binary.path)
            == realPath(binary))
    #expect(
        GrokTransport.resolveBinary(
            path: path, home: root.path, override: "/nonexistent/minutae-grok") == nil)
}

@Test func grokFakeCLIInvocation() async throws {
    let prompt = "quotes ' \"\n雪 🐈\n"
    let json = """
        {"text":"pong","stopReason":"end_turn","sessionId":"secret-session","requestId":"req","num_turns":1,\
        "usage":{"input_tokens":15,"output_tokens":1},"modelUsage":{"grok-4.7-build":{"inputTokens":15,"costUSD":0.02}},\
        "total_cost_usd":0.02}
        """
    let fake = try installFakeCLI(validatingScript(json: json))
    defer { fake.remove() }
    let transport = GrokTransport(model: "test-model", binaryPath: fake.binary.path)
    let response = try await transport.run(sample(prompt))
    #expect(response.text == "pong")
    #expect(response.transport == .grokCLI)
    #expect(response.model == "grok-4.7-build")
    #expect(response.usage == Usage(inputTokens: 15, outputTokens: 1))
    #expect(response.duration >= .zero)
    let argv = try arguments(at: fake.directory.appendingPathComponent("args"))
    #expect(try text(at: fake.directory.appendingPathComponent("stdin")) == prompt)
    #expect(try text(at: fake.directory.appendingPathComponent("prompt")) == prompt)
    expectFlags(argv, prompt: prompt, model: "test-model", schema: nil)
    let cwd = try text(at: fake.directory.appendingPathComponent("cwdpath"))
    #expect(!FileManager.default.fileExists(atPath: cwd))
    #expect(
        !FileManager.default.fileExists(
            atPath: URL(fileURLWithPath: cwd).deletingLastPathComponent().path))
}

@Test func grokOmitsModelFlagWhenUnset() async throws {
    let json = #"{"text":"pong","stopReason":"end_turn"}"#
    let fake = try installFakeCLI(recordingScript(json: json))
    defer { fake.remove() }
    let response = try await GrokTransport(model: "  \n", binaryPath: fake.binary.path).run(
        sample("hello"))
    #expect(response.model == nil)
    #expect(response.usage == nil)
    let argv = try arguments(at: fake.directory.appendingPathComponent("args"))
    #expect(!argv.contains("--model"))
    #expect(try text(at: fake.directory.appendingPathComponent("stdin")) == "hello")
}

@Test func grokSystemStaysOffArgvAndSchemaIsPassed() async throws {
    let json =
        #"{"text":"pong","stopReason":"EndTurn","model":"reported","modelUsage":{"other":{}}}"#
    let fake = try installFakeCLI(recordingScript(json: json))
    defer { fake.remove() }
    let schema = Data("{\"type\":\"object\"}".utf8)
    let response = try await GrokTransport(model: "alias", binaryPath: fake.binary.path).run(
        sample("hello", system: "Be brief", schema: schema)
    )
    #expect(response.text == "pong")
    #expect(response.model == "reported")
    let composed = "Be brief\n\nhello"
    #expect(try text(at: fake.directory.appendingPathComponent("stdin")) == composed)
    let argv = try arguments(at: fake.directory.appendingPathComponent("args"))
    expectFlags(argv, prompt: composed, model: "alias", schema: "{\"type\":\"object\"}")
    #expect(!argv.contains("Be brief"))
}

@Test func grokClassifiesLoginAndRateLimit() async throws {
    try await expectError(
        .notLoggedIn, script: "echo 'Not signed in. Run `grok login` to authenticate' >&2\nexit 1\n"
    )
    try await expectError(
        .rateLimited(retryAfter: nil),
        script: "echo 'You have hit the rate limit for your plan.' >&2\nexit 1\n")
    try await expectError(
        .rateLimited(retryAfter: nil), script: "echo '429 Too Many Requests' >&2\nexit 1\n")
    try await expectError(.notLoggedIn, script: "echo 'no xAI credential available' >&2\nexit 1\n")
}

@Test func grokClassifiesJSONStopReasons() async throws {
    try await expectError(
        .cancelled, script: "printf '%s\\n' '{\"text\":\"partial\",\"stopReason\":\"cancelled\"}'\n"
    )
    try await expectError(
        .rateLimited(retryAfter: nil),
        script: "printf '%s\\n' '{\"text\":\"\",\"stopReason\":\"RATE_LIMITED\"}'\n")
    try await expectError(
        .invalidOutput("Grok returned malformed JSON."), script: "printf '%s\\n' 'not-json'\n")
    try await expectError(
        .invalidOutput("Grok returned no text."),
        script: "printf '%s\\n' '{\"text\":\"  \",\"stopReason\":\"end_turn\"}'\n")
    try await expectError(
        .unavailable("Process exited with status 2"),
        script: "echo 'error: unexpected argument' >&2\nexit 2\n")
}

@Test func grokSuccessIgnoresLoginTextOnStderr() async throws {
    let script =
        "echo 'not signed in' >&2\nprintf '%s\\n' '{\"text\":\"pong\",\"stopReason\":\"end_turn\"}'\n"
    let fake = try installFakeCLI("cat > /dev/null\n" + script)
    defer { fake.remove() }
    let response = try await GrokTransport(binaryPath: fake.binary.path).run(sample("hello"))
    #expect(response.text == "pong")
}

@Test func grokUsesInjectedModelWhenJSONOmitsOne() async throws {
    let script =
        "printf '%s\\n' '{\"text\":\"pong\",\"stopReason\":\"end_turn\",\"modelUsage\":{\"a\":{},\"b\":{}}}'\n"
    let fake = try installFakeCLI("cat > /dev/null\n" + script)
    defer { fake.remove() }
    let response = try await GrokTransport(model: "chosen", binaryPath: fake.binary.path).run(
        sample("hello"))
    #expect(response.model == "chosen")
}

@Test func grokStatusAndMissingBinary() async throws {
    let missing = GrokTransport(model: "  ", binaryPath: "/nonexistent/minutae-grok")
    #expect(
        await missing.status()
            == TransportStatus(
                installed: false,
                signedIn: nil,
                model: nil,
                detail: GrokTransport.notInstalledDetail
            )
    )
    await expectRun(missing, .notInstalled)
    let fake = try installFakeCLI(
        "if [ \"${1:-}\" = \"--version\" ]; then echo 'grok 1.0.77 (test) [stable]'; exit 0; fi\nexit 9\n"
    )
    defer { fake.remove() }
    let ready = GrokTransport(model: "chosen", binaryPath: fake.binary.path)
    #expect(
        await ready.status()
            == TransportStatus(
                installed: true,
                binaryPath: realPath(fake.binary),
                signedIn: nil,
                model: "chosen",
                version: "grok 1.0.77 (test) [stable]"
            )
    )
    let broken = try installFakeCLI("echo 'nope' >&2\nexit 1\n")
    defer { broken.remove() }
    let status = await GrokTransport(binaryPath: broken.binary.path).status()
    #expect(status.installed)
    #expect(status.signedIn == nil)
    #expect(status.version == nil)
}

@Test func grokPatternsRegisterAdditively() {
    let base = ErrorClassifier(patterns: [.init(text: "limit", error: .unavailable("base"))])
    let combined = base.registering(GrokTransport.stderrPatterns)
    if case .unavailable(let message) = combined.classify(
        exitStatus: 1, stdout: "", stderr: "rate limit")
    {
        #expect(message == "base")
    } else {
        Issue.record("Expected the earlier pattern to win")
    }
    if case .notLoggedIn = combined.classify(
        exitStatus: 1, stdout: "", stderr: "Authentication required")
    {
    } else {
        Issue.record("Expected the registered login pattern")
    }
    if case .unavailable = base.classify(exitStatus: 1, stdout: "", stderr: "not signed in") {
    } else {
        Issue.record("Registering must not mutate the original classifier")
    }
    #expect(combined.classify(exitStatus: 0, stdout: "", stderr: "rate limit") == nil)
}

@Test func grokRejectsNonUTF8Schema() async throws {
    let fake = try installFakeCLI("exit 0\n")
    defer { fake.remove() }
    let schema = Data([0xFF, 0xFE])
    await expectRun(
        GrokTransport(binaryPath: fake.binary.path), .invalidOutput("JSON schema is not UTF-8."),
        schema: schema)
}

private func sample(_ prompt: String, system: String? = nil, schema: Data? = nil) -> LLMRequest {
    LLMRequest(
        system: system, prompt: prompt, jsonSchema: schema, timeout: .seconds(5), purpose: .test)
}

private func expectError(_ expected: LLMError, script: String) async throws {
    let fake = try installFakeCLI("cat > /dev/null\n" + script)
    defer { fake.remove() }
    await expectRun(GrokTransport(binaryPath: fake.binary.path), expected)
}

private func expectRun(_ transport: GrokTransport, _ expected: LLMError, schema: Data? = nil) async
{
    do {
        _ = try await transport.run(sample("hello", schema: schema))
        Issue.record("Expected \(expected)")
    } catch let error as LLMError {
        #expect(sameError(error, expected))
    } catch {
        Issue.record("Unexpected error: \(error)")
    }
}

private func sameError(_ lhs: LLMError, _ rhs: LLMError) -> Bool {
    switch (lhs, rhs) {
    case (.notInstalled, .notInstalled), (.notLoggedIn, .notLoggedIn), (.timeout, .timeout),
        (.cancelled, .cancelled):
        return true
    case (.rateLimited(let left), .rateLimited(let right)):
        return left == right
    case (.invalidOutput(let left), .invalidOutput(let right)),
        (.modelRejected(let left), .modelRejected(let right)),
        (.unavailable(let left), .unavailable(let right)):
        return left == right
    default:
        return false
    }
}

private func expectFlags(_ argv: [String], prompt: String, model: String?, schema: String?) {
    #expect(pair(argv, "--output-format") == "json")
    #expect(pair(argv, "--tools") == "")
    #expect(pair(argv, "--max-turns") == "1")
    #expect(argv.contains("--disable-web-search"))
    #expect(argv.contains("--no-subagents"))
    #expect(argv.contains("--verbatim"))
    #expect(pair(argv, "--prompt-file") != nil)
    #expect(pair(argv, "--cwd") != nil)
    #expect(pair(argv, "--model") == model)
    #expect(pair(argv, "--json-schema") == schema)
    let forbidden = [
        "--always-approve", "--continue", "--resume", "--session-id", "--permission-mode", "-p",
        "--single", "bypassPermissions",
    ]
    #expect(forbidden.allSatisfy { !argv.contains($0) })
    for line in prompt.split(separator: "\n", omittingEmptySubsequences: true) {
        #expect(!argv.contains(String(line)))
    }
}

private func pair(_ argv: [String], _ flag: String) -> String? {
    guard let index = argv.firstIndex(of: flag), argv.index(after: index) < argv.endIndex else {
        return nil
    }
    return argv[argv.index(after: index)]
}

private func validatingScript(json: String) -> String {
    """
    dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    printf '%s\\n' "$@" > "$dir/args"
    cat > "$dir/stdin"
    prompt_file=""
    cwd=""
    prev=""
    for arg in "$@"; do
      if [ "$prev" = "--prompt-file" ]; then prompt_file=$arg; fi
      if [ "$prev" = "--cwd" ]; then cwd=$arg; fi
      prev=$arg
    done
    if [ -z "$prompt_file" ] || [ -z "$cwd" ]; then echo "missing flags" >&2; exit 2; fi
    printf '%s' "$cwd" > "$dir/cwdpath"
    mode=$(/usr/bin/stat -f %Lp "$cwd") || exit 4
    if [ "$mode" != "700" ]; then echo "cwd mode $mode" >&2; exit 4; fi
    if [ -n "$(/bin/ls -A "$cwd")" ]; then echo "cwd not empty" >&2; exit 5; fi
    pmode=$(/usr/bin/stat -f %Lp "$prompt_file") || exit 6
    if [ "$pmode" != "600" ]; then echo "prompt mode $pmode" >&2; exit 6; fi
    cat "$prompt_file" > "$dir/prompt"
    cat <<'GROK_JSON'
    \(json)
    GROK_JSON
    """
}

private func recordingScript(json: String) -> String {
    """
    dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    printf '%s\\n' "$@" > "$dir/args"
    cat > "$dir/stdin"
    cat <<'GROK_JSON'
    \(json)
    GROK_JSON
    """
}

private struct FakeCLI {
    let directory: URL
    let binary: URL
    func remove() { try? FileManager.default.removeItem(at: directory) }
}

private func installFakeCLI(_ body: String) throws -> FakeCLI {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
        "minutae-grok-test-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    let binary = directory.appendingPathComponent("grok")
    try ("#!/bin/sh\n" + body + "\n").write(to: binary, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: binary.path)
    return FakeCLI(directory: directory, binary: binary)
}

private func tempRoot() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(
        "minutae-grok-resolve-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
    return url
}

private func writeExecutable(_ url: URL) throws {
    try FileManager.default.createDirectory(
        at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    try "#!/bin/sh\nexit 0\n".write(to: url, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
}

private func realPath(_ url: URL) -> String {
    url.resolvingSymlinksInPath().standardizedFileURL.path
}

private func text(at url: URL) throws -> String {
    try String(contentsOf: url, encoding: .utf8)
}

private func arguments(at url: URL) throws -> [String] {
    let raw = try text(at: url)
    var lines = raw.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
    if raw.hasSuffix("\n"), lines.last == "" { lines.removeLast() }
    return lines
}
