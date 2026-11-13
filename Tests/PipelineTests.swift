import MinutaeCore
import MinutaeLLM
import MinutaeSpeech
import XCTest

@testable import Minutae

final class PipelineTests: XCTestCase {
    @MainActor
    func testCompositionRootBuildsWithFakes() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("minutae-composition-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let environment = try await AppEnvironment.make(useFakes: true, root: root)
        XCTAssertTrue(environment.usesFakes)
        XCTAssertTrue(environment.hotkeyArmed)
        XCTAssertEqual(environment.armedHotkeyMode, "hold")
        XCTAssertEqual(environment.armedHotkeyCode, 49)
        XCTAssertEqual(environment.armedHotkeyModifiers, 1 << 1)
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appending(path: "minutae.db").path))
        let model = await environment.modelStatusLine()
        XCTAssertTrue(model.contains("Fake speech engine"))
    }

    @MainActor
    func testFakePipelineWritesSummaryAndDictation() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("minutae-pipeline-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let environment = try await AppEnvironment.make(useFakes: true, root: root)
        let report = try await environment.runPipelineSmoke()
        XCTAssertEqual(report.meetingStatus, "done")
        XCTAssertTrue(FileManager.default.fileExists(atPath: report.summaryPath))
        let markdown = try String(contentsOfFile: report.summaryPath, encoding: .utf8)
        XCTAssertTrue(markdown.contains("id: \(report.meetingID)"))
        XCTAssertTrue(markdown.contains("Smoke summary bullet"))
        XCTAssertEqual(report.dictationStatus, "done")
        XCTAssertTrue(report.dictationText.localizedCaseInsensitiveContains("fake engine"))
        XCTAssertEqual(report.insertMethod, "pasteboard")
        XCTAssertFalse(report.audioPath.isEmpty)
        XCTAssertTrue(FileManager.default.fileExists(atPath: report.audioPath))
    }

    func testMissingModelAndDeniedPermissionStayReadable() async {
        let engine = ReportingSpeechEngine(engine: FakeSpeechEngine(), modelName: "base")
        do {
            _ = try await engine.transcribe(samples: [0.1], options: .init())
            XCTFail("An unloaded model should fail with a message.")
        } catch {
            let text = AppMessages.describe(error)
            XCTAssertTrue(text.contains("base"))
            XCTAssertTrue(text.contains("not installed"))
        }
        let denied = AppMessages.permissionBlocked(.microphone, status: .denied)
        XCTAssertTrue(denied.contains("Microphone"))
        XCTAssertTrue(denied.contains("denied"))
        let missingCLI = AppMessages.describe(LLMError.notInstalled)
        XCTAssertTrue(missingCLI.contains("not installed"))
    }
}
