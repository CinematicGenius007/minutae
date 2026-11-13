import Foundation
import GRDB
import XCTest
@testable import MinutaeCore

final class JobRunnerTests: XCTestCase {
    func testRecoveryAndRetry() async throws {
        let db = try database()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in throw NonRetryableJobError("broken") }], delay: { _ in 0 })
        let id = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        try await db.pool.write { try $0.execute(sql: "UPDATE jobs SET state = 'running', attempts = 1 WHERE id = ?", arguments: [id]) }
        try await runner.start()
        try await waitFor(db, id, "failed")
        let job = try XCTUnwrap(Repository<Job>(db).find(id))
        XCTAssertEqual(job.attempts, 1)
        XCTAssertEqual(job.last_error, "broken")
        await runner.stop()
    }

    func testRetryAndCancelQueued() async throws {
        let db = try database()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in throw Failure() }], delay: { _ in 0 })
        let id = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        let cancelled = try await runner.enqueue(kind: .embed, subjectType: "meeting", subjectID: "a")
        try await runner.cancel(cancelled)
        try await runner.start()
        try await waitFor(db, id, "failed")
        XCTAssertEqual(try Repository<Job>(db).find(id)?.attempts, 2)
        XCTAssertEqual(try Repository<Job>(db).find(cancelled)?.state, "cancelled")
        await runner.stop()
    }

    func testConcurrencyAndRunningCancellation() async throws {
        let db = try database()
        let probe = Probe()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in
            await probe.enter()
            do { try await Task.sleep(for: .seconds(10)) } catch { await probe.leave(); throw error }
            await probe.leave()
        }])
        let first = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        let second = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "b")
        try await runner.start()
        for _ in 0..<100 {
            if await probe.started == 1 { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        try await Task.sleep(for: .milliseconds(300))
        let started = await probe.started
        XCTAssertEqual(started, 1)
        let running = try XCTUnwrap([first, second].first { try Repository<Job>(db).find($0)?.state == "running" })
        try await runner.cancel(running)
        try await waitFor(db, running, "cancelled")
        for _ in 0..<100 {
            if await probe.started == 2 { break }
            try await Task.sleep(for: .milliseconds(50))
        }
        let maximum = await probe.maximum
        XCTAssertEqual(maximum, 1)
        await runner.stop()
    }

    private actor Probe {
        var started = 0
        var active = 0
        var maximum = 0
        func enter() { started += 1; active += 1; maximum = max(maximum, active) }
        func leave() { active -= 1 }
    }

    private func database() throws -> MinutaeDatabase {
        try MinutaeDatabase(path: FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).appendingPathComponent("test.db").path)
    }
    private func waitFor(_ db: MinutaeDatabase, _ id: String, _ state: String) async throws {
        for _ in 0..<100 {
            if try Repository<Job>(db).find(id)?.state == state { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTFail("Timed out waiting for \(state)")
    }
    private struct Failure: Error {}
}
