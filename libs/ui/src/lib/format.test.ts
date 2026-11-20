import { describe, expect, test } from "bun:test";
import { clamp01, formatBytes, formatClock, formatDuration, formatRelativeDate } from "./format";

describe("formatDuration", () => {
  test("mm:ss under an hour", () => {
    expect(formatDuration(32 * 60_000 + 14_000)).toBe("32:14");
    expect(formatDuration(5_000)).toBe("00:05");
    expect(formatDuration(0)).toBe("00:00");
  });
  test("h:mm:ss at an hour or more", () => {
    expect(formatDuration(3_600_000 + 12 * 60_000 + 14_000)).toBe("1:12:14");
  });
  test("floors partial seconds and guards bad input", () => {
    expect(formatDuration(59_999)).toBe("00:59");
    expect(formatDuration(-10)).toBe("00:00");
    expect(formatDuration(Number.NaN)).toBe("00:00");
  });
});

describe("formatClock", () => {
  const d = new Date(2026, 10, 20, 14, 30);
  test("24-hour", () => {
    expect(formatClock(d)).toBe("14:30");
    expect(formatClock(new Date(2026, 0, 1, 9, 5))).toBe("09:05");
  });
  test("12-hour", () => {
    expect(formatClock(d, true)).toBe("2:30 PM");
    expect(formatClock(new Date(2026, 0, 1, 0, 0), true)).toBe("12:00 AM");
  });
});

describe("formatRelativeDate", () => {
  const now = new Date(2026, 10, 20, 18, 0); // Fri 20 Nov 2026
  test("today shows the time", () => {
    expect(formatRelativeDate(new Date(2026, 10, 20, 14, 30), now)).toBe("14:30");
  });
  test("this week shows the weekday", () => {
    expect(formatRelativeDate(new Date(2026, 10, 18, 9, 0), now)).toBe("Wed");
    expect(formatRelativeDate(new Date(2026, 10, 16, 9, 0), now)).toBe("Mon");
  });
  test("older shows day and month", () => {
    expect(formatRelativeDate(new Date(2026, 10, 12, 9, 0), now)).toBe("12 Nov");
  });
});

describe("formatBytes / clamp01", () => {
  test("sizes", () => {
    expect(formatBytes(466_000_000)).toBe("466 MB");
    expect(formatBytes(1_500_000)).toBe("1.5 MB");
    expect(formatBytes(512)).toBe("512 B");
  });
  test("clamp", () => {
    expect(clamp01(1.4)).toBe(1);
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(0.62)).toBe(0.62);
  });
});
