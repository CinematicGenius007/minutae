import Foundation
import Testing
@testable import MinutaeLLM

@Test func codexFakeCLI() async throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent("codex")
    let script = """
    #!/bin/sh
    case "$*" in *"--ignore-user-config"*"--ignore-rules"*"--model test-model -"*) ;; *) exit 8;; esac
    [ "$(cat)" = "hello" ] || exit 9
    echo '{"type":"item.completed","item":{"type":"agent_message","text":"pong"}}'
    echo '{"type":"turn.completed","usage":{"input_tokens":2,"output_tokens":1}}'
    """
    try script.write(to: file, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: file.path)
    let transport = CodexTransport(codexModel: "test-model", binaryPath: file.path)
    let response = try await transport.run(LLMRequest(prompt: "hello", timeout: .seconds(5), purpose: .test))
    #expect(response.text == "pong")
    #expect(response.usage?.inputTokens == 2)
    let empty = CodexTransport(codexModel: " ", binaryPath: file.path)
    #expect(await empty.status().detail != nil)
    do { _ = try await empty.run(LLMRequest(prompt: "hello", timeout: .seconds(5), purpose: .test)); Issue.record("Expected model rejection") }
    catch LLMError.modelRejected {} 
}
