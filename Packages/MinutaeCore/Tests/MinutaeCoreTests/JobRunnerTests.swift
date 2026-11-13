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

    func testRunAfterAndBackoffUseClock() async throws {
        let db = try database()
        let clock = TestClock()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in throw Failure() }], clock: { clock.now }, delay: { _ in 30 })
        let id = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        let formatter = ISO8601DateFormatter()
        let future = formatter.string(from: clock.now.addingTimeInterval(60))
        try await db.pool.write { try $0.execute(sql: "UPDATE jobs SET run_after = ? WHERE id = ?", arguments: [future, id]) }
        try await runner.start()
        try await Task.sleep(for: .milliseconds(350))
        XCTAssertEqual(try Repository<Job>(db).find(id)?.attempts, 0)
        clock.advance(60)
        for _ in 0..<100 {
            if try Repository<Job>(db).find(id)?.attempts == 1,
               try Repository<Job>(db).find(id)?.state == "queued" { break }
            try await Task.sleep(for: .milliseconds(20))
        }
        let retry = try XCTUnwrap(Repository<Job>(db).find(id))
        XCTAssertEqual(retry.run_after, formatter.string(from: clock.now.addingTimeInterval(30)))
        clock.advance(29)
        try await Task.sleep(for: .milliseconds(250))
        XCTAssertEqual(try Repository<Job>(db).find(id)?.attempts, 1)
        clock.advance(1)
        try await waitFor(db, id, "failed")
        XCTAssertEqual(try Repository<Job>(db).find(id)?.attempts, 2)
        await runner.stop()
    }

    func testDatabaseFailureIsObservableAndCompletionRecovers() async throws {
        let db = try database()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in }])
        let id = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        try await db.pool.write { try $0.execute(sql: "CREATE TRIGGER reject_finish BEFORE UPDATE ON jobs WHEN NEW.state = 'succeeded' BEGIN SELECT RAISE(FAIL, 'finish blocked'); END") }
        let stream = await runner.databaseErrors()
        let errorTask = Task { for await message in stream { return message }; return "" }
        try await runner.start()
        let message = await errorTask.value
        XCTAssertTrue(message.contains("finish blocked"))
        XCTAssertEqual(try Repository<Job>(db).find(id)?.state, "running")
        try await db.pool.write { try $0.execute(sql: "DROP TRIGGER reject_finish") }
        try await waitFor(db, id, "succeeded")
        await runner.stop()
    }

    func testDefaultBackoffHasPolicyBounds() async throws {
        let db = try database()
        let clock = TestClock()
        let runner = JobRunner(database: db, handlers: [.summarise: { _ in throw Failure() }], clock: { clock.now })
        let id = try await runner.enqueue(kind: .summarise, subjectType: "meeting", subjectID: "a")
        try await runner.start()
        for attempt in 1...2 {
            for _ in 0..<100 {
                if let job = try Repository<Job>(db).find(id), job.attempts == attempt, job.state == "queued", job.run_after != nil { break }
                try await Task.sleep(for: .milliseconds(20))
            }
            let job = try XCTUnwrap(Repository<Job>(db).find(id))
            let date = try XCTUnwrap(ISO8601DateFormatter().date(from: try XCTUnwrap(job.run_after)))
            let expected = 30.0 * pow(2, Double(attempt - 1))
            let interval = date.timeIntervalSince(clock.now)
            XCTAssertGreaterThanOrEqual(interval, expected * 0.9 - 1)
            XCTAssertLessThanOrEqual(interval, expected * 1.1)
            clock.advance(100)
        }
        await runner.stop()
    }

    func testClaimDatabaseFailureIsObservableAndRecovers() async throws {
        let db = try database()
        let runner = JobRunner(database: db, handlers: [.transcribe: { _ in }])
        let id = try await runner.enqueue(kind: .transcribe, subjectType: "meeting", subjectID: "a")
        try await db.pool.write { try $0.execute(sql: "CREATE TRIGGER reject_claim BEFORE UPDATE ON jobs WHEN NEW.state = 'running' BEGIN SELECT RAISE(FAIL, 'claim blocked'); END") }
        let stream = await runner.databaseErrors()
        let errorTask = Task { for await message in stream { return message }; return "" }
        try await runner.start()
        let message = await errorTask.value
        XCTAssertTrue(message.contains("claim blocked"))
        XCTAssertEqual(try Repository<Job>(db).find(id)?.state, "queued")
        try await db.pool.write { try $0.execute(sql: "DROP TRIGGER reject_claim") }
        try await waitFor(db, id, "succeeded")
        await runner.stop()
    }

    private final class TestClock: @unchecked Sendable {
        private let lock = NSLock()
        private var value = Date(timeIntervalSince1970: 1_700_000_000)
        var now: Date { lock.lock(); defer { lock.unlock() }; return value }
        func advance(_ seconds: TimeInterval) { lock.lock(); defer { lock.unlock() }; value.addTimeInterval(seconds) }
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
