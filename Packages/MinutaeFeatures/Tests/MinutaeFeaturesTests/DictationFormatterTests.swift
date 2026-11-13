import Foundation
import MinutaeCore
import Testing

@testable import MinutaeFeatures

struct DictationFormatterTests {
    let formatter = Fixtures.formatter
    let now = Fixtures.date("2026-11-13T16:00:00Z")

    @Test func timeDayAndDuration() {
        #expect(formatter.time(Fixtures.date("2026-11-13T14:41:00Z")) == "2:41 PM")
        #expect(formatter.day(Fixtures.date("2026-11-13T01:00:00Z"), now: now) == "Today")
        #expect(formatter.day(Fixtures.date("2026-11-12T23:00:00Z"), now: now) == "Yesterday")
        #expect(formatter.day(Fixtures.date("2026-11-02T10:00:00Z"), now: now) == "Nov 2")
        #expect(formatter.day(Fixtures.date("2025-11-02T10:00:00Z"), now: now) == "Nov 2, 2025")
        #expect(formatter.duration(milliseconds: 6_000) == "6 s")
        #expect(formatter.duration(milliseconds: 6_499) == "6 s")
        #expect(formatter.duration(milliseconds: 72_000) == "1 m 12 s")
        #expect(formatter.duration(milliseconds: 120_000) == "2 m")
        #expect(formatter.clock(seconds: 2.9) == "0:02")
        #expect(formatter.clock(seconds: 75) == "1:15")
    }

    @Test func rowMetaMatchesTheDesign() {
        #expect(formatter.rowMeta(Fixtures.row("a"), now: now) == "2:41 PM · 6 s")
        let older = Fixtures.row("b", started: "2026-11-12T13:12:00Z", durationMs: 8_000)
        #expect(formatter.rowMeta(older, now: now) == "Yesterday, 1:12 PM · 8 s")
        let unknown = Fixtures.row("c", durationMs: nil)
        #expect(formatter.rowMeta(unknown, now: now) == "2:41 PM")
    }

    @Test func detailTitleAndMeta() {
        let row = Fixtures.row("a")
        #expect(formatter.detailTitle(row) == "Dictation · 2:41 PM")
        #expect(
            formatter.detailMeta(row, now: now)
                == "Today · 6 s · pasted into Slack · whisper small")
        var copied = row
        copied.insert_method = "clipboardOnly"
        copied.speech_model = nil
        #expect(formatter.detailMeta(copied, now: now) == "Today · 6 s · copied to the clipboard")
        var typed = row
        typed.insert_method = "typing"
        typed.target_app_name = nil
        #expect(DictationFormatter.destination(typed) == "typed")
        var failed = row
        failed.insert_method = nil
        failed.status = "failed"
        #expect(DictationFormatter.destination(failed) == "failed")
    }
}
