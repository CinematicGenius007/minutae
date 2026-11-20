import { describe, expect, test } from "bun:test";
import { detectPlatform, formatShortcut, localPhrase, shortcutKeys } from "./platform";

const MAC_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15";
const WIN_UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";
const LINUX_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";

describe("detectPlatform", () => {
  test("recognises each OS", () => {
    expect(detectPlatform(MAC_UA)).toBe("mac");
    expect(detectPlatform(WIN_UA)).toBe("windows");
    expect(detectPlatform(LINUX_UA)).toBe("linux");
  });
});

describe("shortcutKeys", () => {
  test("maps mod to ⌘ on macOS and Ctrl elsewhere", () => {
    expect(shortcutKeys("mod+k", "mac")).toEqual(["⌘", "K"]);
    expect(shortcutKeys("mod+k", "windows")).toEqual(["Ctrl", "K"]);
    expect(shortcutKeys("mod+k", "linux")).toEqual(["Ctrl", "K"]);
  });
  test("maps alt/option and shift", () => {
    expect(shortcutKeys("alt+space", "mac")).toEqual(["⌥", "Space"]);
    expect(shortcutKeys("option+shift+r", "windows")).toEqual(["Alt", "Shift", "R"]);
  });
  test("keeps punctuation and named keys", () => {
    expect(shortcutKeys("mod+,", "mac")).toEqual(["⌘", ","]);
    expect(shortcutKeys("enter", "mac")).toEqual(["⏎"]);
    expect(shortcutKeys("esc", "linux")).toEqual(["esc"]);
  });
});

describe("formatShortcut", () => {
  test("runs glyphs together on macOS, joins words with + elsewhere", () => {
    expect(formatShortcut("mod+r", "mac")).toBe("⌘R");
    expect(formatShortcut("mod+r", "windows")).toBe("Ctrl+R");
    expect(formatShortcut("mod+shift+k", "linux")).toBe("Ctrl+Shift+K");
  });
});

describe("localPhrase", () => {
  test("names the device per platform", () => {
    expect(localPhrase("mac")).toBe("on this Mac");
    expect(localPhrase("windows")).toBe("on this PC");
    expect(localPhrase("linux")).toBe("on this computer");
  });
});
