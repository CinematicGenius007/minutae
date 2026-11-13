import Foundation
import Testing
@testable import MinutaeLLM

private func script(_ body: String) throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: false)
    let file = dir.appendingPathComponent("fake")
    try ("#!/bin/sh\n" + body + "\n").write(to: file, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: file.path)
    return file
}

@Test func deliversStdinAndRemovesPrivateCwd() async throws {
    let file = try script("pwd; /bin/pwd; /bin/cat; /bin/ls -A; /usr/bin/stat -f %Lp . >&2")
    defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
    let prompt = "quotes ' \"\n雪 🐈\n"
    let result = try await SubprocessRunner().run(binary: file.path, prompt: prompt, timeout: .seconds(5))
    let lines = result.stdout.split(separator: "\n", omittingEmptySubsequences: false)
    #expect(result.exitStatus == 0)
    #expect(lines[0] == lines[1])
    #expect(result.stdout.contains(prompt))
    #expect(result.stderr.trimmingCharacters(in: .whitespacesAndNewlines) == "700")
    #expect(!FileManager.default.fileExists(atPath: String(lines[0])))
}

@Test func boundedOutputAndClassification() async throws {
    let file = try script("/usr/bin/head -c 200000 /dev/zero | /usr/bin/tr '\\0' x; echo 'please sign in' >&2; exit 7")
    defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
    let result = try await SubprocessRunner(outputLimit: 1024).run(binary: file.path, prompt: "", timeout: .seconds(5))
    #expect(result.stdout.utf8.count == 1024)
    #expect(result.exitStatus == 7)
    let classifier = ErrorClassifier(patterns: [.init(text: "sign in", error: .notLoggedIn)])
    if case .notLoggedIn = classifier.classify(exitStatus: result.exitStatus, stdout: result.stdout, stderr: result.stderr) {} else { Issue.record("Expected login error") }
}

@Test func missingBinary() async throws {
    do {
        _ = try await SubprocessRunner().run(binary: "/nonexistent/minutae-fake", prompt: "", timeout: .seconds(1))
        Issue.record("Expected error")
    } catch LLMError.notInstalled {} catch { Issue.record("Unexpected error: \(error)") }
}

@Test func timeoutAndCancellation() async throws {
    let file = try script("exec /bin/sleep 30")
    defer { try? FileManager.default.removeItem(at: file.deletingLastPathComponent()) }
    let runner = SubprocessRunner()
    do {
        _ = try await runner.run(binary: file.path, prompt: "", timeout: .milliseconds(100))
        Issue.record("Expected timeout")
    } catch LLMError.timeout {} catch { Issue.record("Unexpected error: \(error)") }
    let task = Task { try await runner.run(binary: file.path, prompt: "", timeout: .seconds(10)) }
    try await Task.sleep(for: .milliseconds(150))
    do {
        _ = try await runner.run(binary: file.path, prompt: "", timeout: .seconds(1))
        Issue.record("Expected single-flight rejection")
    } catch LLMError.unavailable {} catch { Issue.record("Unexpected error: \(error)") }
    task.cancel()
    do { _ = try await task.value; Issue.record("Expected cancellation") }
    catch LLMError.cancelled {} catch { Issue.record("Unexpected error: \(error)") }
}
