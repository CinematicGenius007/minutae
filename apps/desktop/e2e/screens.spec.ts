// Screenshot suite: every route and state, light and dark, against the mock backend.
// Baselines live in e2e/__screenshots__/<browser>/. Regenerate with `bun run test:e2e:update`.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page, test } from "@playwright/test";
import type { HudState } from "../src/lib/types";

const MEETING = "00000000-0000-4000-8000-000000000001";
const NOW = new Date("2026-11-20T16:30:00Z");
const THEMES = ["light", "dark"] as const;
type Theme = (typeof THEMES)[number];

interface Shot {
  name: string;
  /** Route inside the hash router. */
  route: string;
  /** Mock switches (query string before the hash). */
  query?: string;
  /** Window size for overlay windows (default: the desktop viewport). */
  viewport?: { width: number; height: number };
  /** Runs after the page settles, before the screenshot. */
  act?: (page: Page) => Promise<void>;
  /** Regions that change every frame (timers, live waveform). */
  mask?: (page: Page) => Locator[];
  /** Waits for this text before capturing. */
  ready?: string;
}

/** Runs `then`, then scrolls the Import section into view (it sits below the fold on Storage). */
const showImport = (then?: (page: Page) => Promise<void>) => async (page: Page) => {
  if (then) await then(page);
  await page.locator('section[aria-labelledby="g-import"]').evaluate((el) => el.scrollIntoView({ block: "end" }));
  await page.evaluate(() => document.fonts.ready);
};

const liveMask = (page: Page) => [page.locator("canvas"), page.locator("[role=timer]"), page.locator("button[aria-pressed='true']")];

const TRAY_VIEWPORT = { width: 320, height: 520 };

const retentionSelect = (page: Page) => page.getByRole("combobox", { name: "Keep dictations for" });
const retentionDialog = (page: Page) => page.getByRole("dialog", { name: "Shorten dictation retention?" });

async function chooseRetention(page: Page, name: string) {
  await retentionSelect(page).click();
  await page.getByRole("option", { name, exact: true }).click();
}

const SHOTS: Shot[] = [
  { name: "meetings", route: "/", ready: "Q2 Design Sync" },
  { name: "meetings-empty", route: "/", query: "state=empty" },
  { name: "live", route: "/live", query: "state=recording", mask: liveMask, ready: "Great, I'll move it on the roadmap today." },
  { name: "live-captions-waiting", route: "/live", query: "state=recording&captions=none", mask: liveMask, ready: "Captions appear here" },
  { name: "live-captions-no-model", route: "/live", query: "state=recording&captions=noModel", mask: liveMask, ready: "Get a model" },
  { name: "detail", route: `/meetings/${MEETING}`, ready: "Share updated mockups" },
  { name: "detail-processing", route: `/meetings/${MEETING}`, query: "state=processing", ready: "Hiring loop" },
  { name: "detail-failed", route: `/meetings/${MEETING}`, query: "state=failed", ready: "Vendor call" },
  { name: "dictate", route: "/dictate", ready: "Running ten minutes late" },
  { name: "dictate-empty", route: "/dictate", query: "state=empty" },
  { name: "dictate-error", route: "/dictate", query: "dictations=error", ready: "Couldn't load your dictations." },
  { name: "dictate-wayland", route: "/dictate", query: "os=linux&session=wayland", ready: "Global Esc cancellation is unavailable on Wayland." },
  {
    name: "dictate-retention",
    route: "/settings/dictation",
    query: "oldDictations",
    act: async (page) => {
      await chooseRetention(page, "7 days");
      await retentionDialog(page).getByRole("button", { name: "Shorten retention" }).click();
      await expect(retentionSelect(page)).toHaveText("7 days");
      await page.getByRole("link", { name: "Dictate", exact: true }).click();
      await settle(page, "Keep for 7 days");
      await expect(page.getByText("Standup: finished the model")).toHaveCount(0);
      await expect(page.getByText("Running ten minutes late")).toBeVisible();
    },
  },
  {
    name: "settings-dictation-retention-menu",
    route: "/settings/dictation",
    act: async (page) => {
      await retentionSelect(page).click();
      await expect(page.getByRole("option", { name: "Forever" })).toBeVisible();
      await expect(page.getByRole("option", { name: "90 days" })).toBeVisible();
    },
  },
  {
    name: "settings-dictation-retention-confirm",
    route: "/settings/dictation",
    act: async (page) => {
      await chooseRetention(page, "7 days");
      await expect(retentionDialog(page)).toHaveAccessibleDescription(
        "Minutae will permanently delete dictations older than 7 days, along with any saved audio. Text already typed into other apps stays.",
      );
      await expect(retentionDialog(page).getByRole("button", { name: "Cancel" })).toBeFocused();
    },
  },
  {
    name: "settings-dictation-delete-confirm",
    route: "/settings/dictation",
    act: async (page) => {
      await page.getByRole("button", { name: "Delete all dictations…" }).click();
      await expect(page.getByRole("dialog", { name: "Delete all dictations?" })).toBeVisible();
    },
  },
  { name: "library", route: "/library" },
  {
    name: "library-answer",
    // Route params live inside the hash: #/library?q=…
    route: `/library?q=${encodeURIComponent("When did we move the launch to May")}`,
    act: async (page) => {
      const input = page.getByRole("searchbox").first();
      await expect(input).toHaveValue(/launch/);
      await input.press("Enter");
      await expect(page.getByText("Carol proposed May")).toBeVisible();
      await settle(page);
    },
  },
  ...(["general", "transcription", "summaries", "dictation", "storage", "usage"] as const).map((s) => ({
    name: `settings-${s}`,
    route: `/settings/${s}`,
  })),
  // Linux Wayland: shortcuts through the GlobalShortcuts portal, without key releases, and with no portal.
  { name: "settings-dictation-wayland", route: "/settings/dictation", query: "os=linux&session=wayland", ready: "Change in system settings" },
  {
    name: "settings-dictation-wayland-press",
    route: "/settings/dictation",
    query: "os=linux&session=wayland&portal=press",
    ready: "so each press starts or stops dictation",
  },
  {
    name: "settings-dictation-wayland-noportal",
    route: "/settings/dictation",
    query: "os=linux&session=wayland&portal=none",
    ready: "minutae --toggle-dictation",
  },
  { name: "settings-general-wayland", route: "/settings/general", query: "os=linux&session=wayland", ready: "Ctrl+Alt+R" },
  // Settings → Storage → Import from older Minutae: found, empty, scan error, running, done.
  { name: "settings-import", route: "/settings/storage", ready: "Minutae for Mac", act: showImport() },
  { name: "settings-import-empty", route: "/settings/storage", query: "import=none", ready: "Nothing to import", act: showImport() },
  { name: "settings-import-error", route: "/settings/storage", query: "import=error", ready: "Couldn't look for older", act: showImport() },
  {
    name: "settings-import-running",
    route: "/settings/storage",
    query: "import=running",
    ready: "Minutae for Mac",
    act: showImport(async (page) => {
      await page.getByRole("button", { name: "Import", exact: true }).click();
      await page.mouse.move(0, 0);
      await expect(page.getByText("21 of 54")).toBeVisible();
    }),
  },
  {
    name: "settings-import-done",
    route: "/settings/storage",
    ready: "Minutae for Mac",
    act: showImport(async (page) => {
      await page.getByRole("button", { name: "Import", exact: true }).click();
      await page.mouse.move(0, 0);
      await expect(page.getByText("Imported 41 meetings and 12 dictations")).toBeVisible();
      await expect(page.getByText("53 already imported")).toBeVisible();
    }),
  },
  // Settings → General "Updates" row: loading (checking), update found, error.
  ...(["checking", "available", "error"] as const).map((u) => ({
    name: `settings-general-update-${u}`,
    route: "/settings/general",
    query: `update=${u}`,
    ready: "Updates",
  })),
  // A Scoop copy can't install itself: the row shows its update command instead.
  { name: "settings-general-update-scoop", route: "/settings/general", query: "update=available&channel=scoop", ready: "Updates" },
  { name: "onboarding", route: "/onboarding", ready: "Microphone" },
  { name: "prompt", route: "/prompt" },
  // macOS menu-bar panel: a 320 px window; the timer and level meters move every frame.
  { name: "tray", route: "/tray", viewport: TRAY_VIEWPORT, ready: "Hiring loop" },
  { name: "tray-empty", route: "/tray", query: "state=empty", viewport: TRAY_VIEWPORT },
  {
    name: "tray-recording",
    route: "/tray",
    query: "state=recording",
    viewport: TRAY_VIEWPORT,
    ready: "Stop",
    mask: (page) => [page.locator("[data-tray-timer]"), page.locator("[data-tray-levels]")],
  },
  { name: "gallery", route: "/gallery" },
];

const HUD_STATES: { name: string; payload: HudState }[] = [
  { name: "listening", payload: { state: "listening", level: 0.7, handsFree: false, app: "Slack", shape: "chatMessage", shortcutPresets: ["chatMessage"] } },
  { name: "listening-handsfree", payload: { state: "listening", level: 0.4, handsFree: true, app: null, shape: "tidy", shortcutPresets: [] } },
  { name: "processing", payload: { state: "processing" } },
  { name: "inserted", payload: { state: "inserted", words: 23, undoAvailable: false, resultId: null } },
  { name: "copied", payload: { state: "copied", words: 12, reason: "No text field · copied instead", reasonCode: "noTextField" } },
  { name: "error", payload: { state: "error", message: "Microphone is off. Allow it in Settings.", fixAction: "microphoneSettings" } },
];

function url(route: string, query?: string) {
  return `/${query ? `?${query}` : ""}#${route}`;
}

async function open(page: Page, theme: Theme, route: string, query?: string) {
  await page.clock.setFixedTime(NOW);
  await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
  await page.goto(url(route, query));
  await page.evaluate(() => document.fonts.ready);
  await expect(page.locator("#root > *").first()).toBeAttached();
}

/**
 * Waits for the screen's own "data loaded" marker: a `[data-screen]` root with
 * `data-ready="true"` and no component on the page still reporting `data-ready="false"`.
 */
async function settle(page: Page, ready?: string) {
  await expect(page.locator('[data-screen][data-ready="true"]').first()).toBeAttached();
  await expect(page.locator('[data-ready="false"]')).toHaveCount(0);
  if (ready) await expect(page.getByText(ready).first()).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
}

for (const theme of THEMES) {
  test.describe(`${theme}`, () => {
    for (const shot of SHOTS) {
      test(`${shot.name}`, async ({ page }) => {
        if (shot.viewport) await page.setViewportSize(shot.viewport);
        await open(page, theme, shot.route, shot.query);
        await settle(page, shot.ready);
        if (shot.act) {
          await shot.act(page);
          await page.mouse.move(0, 0);
        }
        await expect(page).toHaveScreenshot(`${shot.name}-${theme}.png`, {
          fullPage: shot.name === "gallery",
          mask: shot.mask?.(page),
        });
      });
    }

    for (const h of HUD_STATES) {
      test(`hud ${h.name}`, async ({ page }) => {
        await page.setViewportSize({ width: 480, height: 120 });
        await open(page, theme, "/hud", h.name === "error" ? "os=macos" : undefined);
        await page.waitForFunction(() => "__minutaeMockEmit" in window);
        await settle(page); // lazy route + hud subscription
        await page.evaluate((p) => {
          const emit = (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit;
          // Listening: a few level frames so the bars have a shape.
          if ((p as { state: string }).state === "listening") {
            for (const level of [0.3, 0.8, 1, 0.5, 0.75]) emit("dictation://hud", { ...(p as object), level });
          }
          emit("dictation://hud", p);
        }, h.payload);
        await expect(page.getByRole(h.name === "error" ? "alert" : "status")).toBeVisible();
        if (h.name === "error") await expect(page.getByRole("button", { name: "Dismiss" })).toBeVisible();
        await expect(page).toHaveScreenshot(`hud-${h.name}-${theme}.png`);
      });
    }

    test("hud error on Wayland", async ({ page }) => {
      await page.setViewportSize({ width: 480, height: 120 });
      await open(page, theme, "/hud", "os=linux&session=wayland&hud=error");
      await settle(page);
      await expect(page.getByRole("alert")).toHaveText("Microphone is off");
      await expect(page.getByRole("button", { name: "Dismiss" })).toHaveCount(0);
      await expect(page).toHaveScreenshot(`hud-error-wayland-${theme}.png`);
    });

    for (const [name, route, ready] of [
      ["meetings", "/", "Q2 Design Sync"],
      ["detail", `/meetings/${MEETING}`, "Share updated mockups"],
      ["dictate", "/dictate", "Running ten minutes late"],
    ] as const) {
      test(`${name} narrow`, async ({ page }) => {
        await page.setViewportSize({ width: 820, height: 900 });
        await open(page, theme, route);
        await settle(page, ready);
        await expect(page).toHaveScreenshot(`${name}-narrow-${theme}.png`);
      });
    }
  });
}

test("first run opens onboarding, finishing returns to Meetings", async ({ page }) => {
  await page.goto(url("/", "onboarding=0"));
  await expect(page).toHaveURL(/#\/onboarding$/);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByText("Q2 Design Sync").first()).toBeVisible();
});

test("library highlights match the query words", async ({ page }) => {
  await page.goto(url("/library?q=mockups%20Friday"));
  const marks = page.locator("mark");
  await expect(marks.first()).toBeVisible();
  const texts = await marks.allTextContents();
  expect(texts.map((t) => t.toLowerCase()).sort()).toEqual(["friday", "mockups"]);
});

test("⌘K on Dictate opens Library search", async ({ page, browserName }) => {
  await page.goto(url("/dictate"));
  await expect(page.getByText("Running ten minutes late")).toBeVisible();
  // The mock reports the OS from the user agent: Desktop Safari → macOS (⌘), Desktop Chrome → Windows (Ctrl).
  await page.keyboard.press(browserName === "webkit" ? "Meta+k" : "Control+k");
  await expect(page).toHaveURL(/#\/library/);
  await expect(page.getByRole("searchbox").first()).toBeFocused();
});

test("cancel and Escape keep dictation retention and return focus to the select", async ({ page }) => {
  await open(page, "light", "/settings/dictation", "oldDictations");
  await chooseRetention(page, "7 days");
  await retentionDialog(page).getByRole("button", { name: "Cancel" }).click();
  await expect(retentionDialog(page)).toHaveCount(0);
  await expect(retentionSelect(page)).toHaveText("Forever");
  await expect(retentionSelect(page)).toBeFocused();

  await chooseRetention(page, "7 days");
  await expect(retentionDialog(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(retentionDialog(page)).toHaveCount(0);
  await expect(retentionSelect(page)).toHaveText("Forever");
  await expect(retentionSelect(page)).toBeFocused();

  await page.getByRole("link", { name: "Dictate", exact: true }).click();
  await expect(page.getByText("Standup: finished the model")).toBeVisible();
});

test("lengthening dictation retention saves immediately", async ({ page }) => {
  await open(page, "light", "/settings/dictation");
  await chooseRetention(page, "30 days");
  await retentionDialog(page).getByRole("button", { name: "Shorten retention" }).click();
  await expect(retentionSelect(page)).toHaveText("30 days");

  await chooseRetention(page, "7 days");
  await expect(retentionDialog(page)).toBeVisible();
  await retentionDialog(page).getByRole("button", { name: "Cancel" }).click();
  await expect(retentionSelect(page)).toHaveText("30 days");

  for (const longer of ["90 days", "Forever"]) {
    await chooseRetention(page, longer);
    await expect(retentionSelect(page)).toHaveText(longer);
    await expect(retentionDialog(page)).toHaveCount(0);
  }
});

test("deleting dictations updates history after confirmation", async ({ page }) => {
  await page.goto(url("/dictate"));
  const dictate = page.getByRole("link", { name: "Dictate", exact: true });
  const settings = page.getByRole("link", { name: "Settings", exact: true });
  await expect(page.getByText("Running ten minutes late")).toBeVisible();
  await expect(page.getByText(/(?:Keep for|Kept) \d+ days/)).toHaveCount(0);

  const firstRow = page.getByRole("listitem").filter({ hasText: "Thanks for the notes" });
  await firstRow.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Delete dictation" }).click();
  await expect(firstRow).toHaveCount(0);

  await settings.click();
  await page.getByRole("link", { name: "Dictation", exact: true }).click();
  const remove = page.getByRole("button", { name: "Delete all dictations…" });
  const dialog = page.getByRole("dialog", { name: "Delete all dictations?" });
  await remove.click();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await dictate.click();
  await expect(page.getByText("Running ten minutes late")).toBeVisible();
  await settings.click();
  await page.getByRole("link", { name: "Dictation", exact: true }).click();
  await remove.click();
  await dialog.getByRole("button", { name: "Delete all dictations", exact: true }).click();
  await expect(page.getByText("Dictations deleted", { exact: true })).toBeVisible();
  await dictate.click();
  await expect(page.getByText("No dictations yet.")).toBeVisible();
});

test("keyboard: Meetings → Detail → Settings, visible focus, esc closes menus", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "WebKit skips links on Tab by default (Safari setting)");
  const ring = () =>
    page.evaluate(() => {
      const el = document.activeElement as HTMLElement;
      // The ring is on the element, a child (tab label) or the row behind it (lane rows).
      const drawn = [el, ...Array.from(el.querySelectorAll<HTMLElement>("*"))].some((n) => getComputedStyle(n).boxShadow !== "none");
      const row = el.closest("[class*='interactive']");
      return drawn || (row ? getComputedStyle(row, "::before").boxShadow !== "none" : false);
    });
  await page.goto(url("/"));
  await expect(page.getByText("Q2 Design Sync").first()).toBeVisible();
  // Tab from the search field reaches the first row.
  await page.getByRole("searchbox").focus();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Q2 Design Sync" })).toBeFocused();
  expect(await ring()).toBe(true);
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`#/meetings/${MEETING}$`));
  // Detail: tabs are reachable and show the ring.
  await page.getByRole("tab", { name: "Summary" }).focus();
  expect(await ring()).toBe(true);
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Transcript" })).toBeFocused();
  // Settings: a select opens with the keyboard and esc closes it.
  await page.goto(url("/settings/general"));
  const select = page.getByRole("combobox").first();
  await select.focus();
  expect(await ring()).toBe(true);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("listbox")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("listbox")).toHaveCount(0);
  await expect(select).toBeFocused();
});

// ── Accessibility smoke: fail on serious or critical axe violations ──────────
const A11Y_ROUTES: [string, string, string?][] = [
  ["meetings", "/"],
  ["live", "/live", "state=recording"],
  ["live-captions-off", "/live", "state=recording&captions=off"],
  ["detail", `/meetings/${MEETING}`],
  ["detail-failed", `/meetings/${MEETING}`, "state=failed"],
  ["dictate", "/dictate"],
  ["dictate-empty", "/dictate", "state=empty"],
  ["dictate-error", "/dictate", "dictations=error"],
  ["library", "/library?q=launch"],
  ["settings-general", "/settings/general"],
  ["settings-transcription", "/settings/transcription"],
  ["settings-summaries", "/settings/summaries"],
  ["settings-dictation", "/settings/dictation"],
  ["settings-dictation-wayland", "/settings/dictation", "os=linux&session=wayland&portal=press"],
  ["settings-dictation-wayland-noportal", "/settings/dictation", "os=linux&session=wayland&portal=none"],
  ["settings-storage", "/settings/storage"],
  ["settings-usage", "/settings/usage"],
  ["onboarding", "/onboarding"],
  ["prompt", "/prompt"],
  ["tray", "/tray"],
];

for (const theme of THEMES) {
  for (const [name, route, query] of A11Y_ROUTES) {
    test(`a11y ${name} ${theme}`, async ({ page }) => {
      await open(page, theme, route, query);
      await settle(page);
      const results = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        .analyze();
      const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
      const report = bad.map((v) => `${v.id} (${v.impact}): ${v.help}\n  ${v.nodes.map((n) => n.target.join(" ")).slice(0, 6).join("\n  ")}`);
      expect(report, report.join("\n")).toEqual([]);
    });
  }
}

// ── Behaviour ───────────────────────────────────────────────────────────────

const mod = (browserName: string) => (browserName === "webkit" ? "Meta" : "Control");

test("updates: check → up to date; available → install shows progress", async ({ page }) => {
  await page.goto(url("/settings/general"));
  await settle(page, "Updates");
  await page.getByRole("button", { name: "Check for updates" }).click();
  await expect(page.getByText("Checking…")).toBeVisible();
  await expect(page.getByText("Version 0.4.0 is the latest.")).toBeVisible();

  await page.goto(url("/settings/general", "update=available"));
  await settle(page, "Updates");
  await expect(page.getByText("Version 0.5.0 is available. You have 0.4.0.")).toBeVisible();
  await page.getByRole("button", { name: "Install and restart" }).click();
  await expect(page.getByRole("progressbar", { name: "Update download" })).toBeVisible();
});

test("updates: a Scoop copy shows its command instead of installing", async ({ page }) => {
  await page.goto(url("/settings/general", "update=available&channel=scoop"));
  await settle(page, "Updates");
  await expect(page.getByText("scoop update minutae", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Install and restart" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Copy command" })).toBeVisible();
});

test("record: pill → live → pause / resume → mark moment → stop lands on the meeting", async ({ page }) => {
  await page.goto(url("/"));
  await settle(page);
  await page.getByRole("button", { name: /^Record/ }).first().click();
  await expect(page).toHaveURL(/#\/live$/);
  await expect(page.getByText("On air", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
  await expect(page.getByText("Paused", { exact: true }).first()).toBeVisible();
  await page.getByRole("button", { name: "Resume" }).click();
  await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();

  await page.getByRole("button", { name: /Mark moment/ }).click();
  await expect(page.getByText(/Moment marked at/).first()).toBeVisible();

  await page.getByRole("button", { name: /Stop & summarize/ }).click();
  // The mock takes ~600 ms in phase "stopping" before the meeting is saved.
  await expect(page.getByText("Saving", { exact: true })).toBeVisible();
  await expect(page).toHaveURL(/#\/meetings\/00000000-0000-4000-8000-000000000009$/);
  await settle(page);
});

test("live captions: partials become finals, empty finals retract, scrolling up stops following", async ({ page }) => {
  await page.goto(url("/live", "state=recording"));
  await settle(page, "Great, I'll move it on the roadmap today.");
  const feed = page.getByTestId("captions");
  const lines = feed.locator("[data-final]");
  const emit = (c: Record<string, unknown>) =>
    page.evaluate(
      (c) => (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit("recording://caption", c),
      { meetingId: "00000000-0000-4000-8000-000000000009", track: "mic", speaker: "You", endMs: 0, ...c },
    );
  const testing = lines.filter({ hasText: "Testing one" });

  await emit({ id: 9001, atMs: 900000, text: "Testing one", final: false });
  await expect(testing).toHaveAttribute("data-final", "false");
  await emit({ id: 9001, atMs: 900000, text: "Testing one two three.", final: true });
  await expect(testing).toHaveCount(1);
  await expect(testing).toHaveAttribute("data-final", "true");
  await expect(testing).toContainText("Testing one two three.");

  await emit({ id: 9002, atMs: 901000, text: "Um, so", final: false });
  await expect(lines.filter({ hasText: "Um, so" })).toHaveCount(1);
  await emit({ id: 9002, atMs: 901000, text: "", final: true });
  await expect(lines.filter({ hasText: "Um, so" })).toHaveCount(0);

  // Enough lines to scroll; scrolling up shows "Jump to latest" and stops following.
  for (let i = 0; i < 30; i++) await emit({ id: 9100 + i, atMs: 902000 + i * 1000, text: `Line ${i} of a long call.`, final: true });
  await expect(feed).toContainText("Line 29 of a long call.");
  await feed.evaluate((el) => el.scrollTo({ top: 0 }));
  const jump = page.getByRole("button", { name: "Jump to latest" });
  await expect(jump).toBeVisible();
  await emit({ id: 9200, atMs: 990000, text: "Newest line.", final: true });
  expect(await feed.evaluate((el) => el.scrollTop)).toBe(0);
  await jump.click();
  await expect(jump).toBeHidden();
  await expect(feed.getByText("Newest line.")).toBeInViewport();
});

test("live captions off: the pane says so and can turn them on", async ({ page }) => {
  await page.goto(url("/live", "state=recording&captions=off"));
  await settle(page, "Live captions are off.");
  await page.getByRole("button", { name: "Turn on captions" }).click();
  await expect(page.getByText("Captions appear here as people talk.")).toBeVisible();
});

test("onboarding on macOS finishes even when system audio stays notDetermined", async ({ page }) => {
  await page.goto(url("/", "os=macos&onboarding=0"));
  await expect(page).toHaveURL(/#\/onboarding$/);
  await settle(page);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Three switches, then you're done.");
  await expect(page.getByText("You can skip for now.")).toBeVisible();

  // System audio is next: asking settles the row although the backend still says notDetermined.
  await page.getByRole("button", { name: "Allow…" }).click();
  await expect(page.getByText(/^Asked\. If it's still off/)).toBeVisible();
  // The state machine moves on to Accessibility (optional): Later finishes the list.
  await page.getByRole("button", { name: "Later" }).click();
  await expect(page.getByText("You can skip for now.")).toHaveCount(0);

  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByText("Q2 Design Sync").first()).toBeVisible();
});

test("onboarding with nothing to ask (all notRequired) goes straight to Meetings", async ({ page }) => {
  await page.goto(url("/onboarding", "perms=none"));
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.getByText("Q2 Design Sync").first()).toBeVisible();
});

test("dictation shortcut recorder rejects a bare key and a duplicate, inline", async ({ page, browserName }) => {
  await page.goto(url("/settings/dictation"));
  await settle(page);
  const cap = page.getByRole("button", { name: /Dictation shortcut/ });
  const before = await cap.textContent();

  await cap.click();
  await page.keyboard.press("a");
  await expect(page.getByRole("alert")).toContainText("A on its own would fire while you type");
  await expect(cap).toHaveAttribute("aria-invalid", "true");
  await expect(cap).toHaveText(before ?? "");

  // Same combination as the Record shortcut (CommandOrControl+Alt+R).
  await cap.click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await page.keyboard.press(`${mod(browserName)}+Alt+KeyR`);
  await expect(page.getByRole("alert")).toContainText("already the Record shortcut");
  await expect(cap).toHaveText(before ?? "");

  // The backend refuses one the UI allows (owned by the OS): its message shows inline, value kept.
  await cap.click();
  await page.keyboard.press("Alt+F4");
  await expect(page.getByRole("alert")).toContainText("The system already uses it");
  await expect(cap).toHaveText(before ?? "");

  // A valid combination saves and clears the error.
  await cap.click();
  await page.keyboard.press("Control+Shift+KeyD");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(cap).not.toHaveText(before ?? "");
});

test("library ask: only the latest question's answer shows", async ({ page }) => {
  await page.goto(url("/library"));
  await settle(page);
  const input = page.getByRole("searchbox").first();
  // The mock answers this one slowly (1.6 s)…
  await input.fill("Who owns the dashboard budget");
  await input.press("Enter");
  // …and this one faster (0.9 s), so the first answer would land last.
  await input.fill("When did we move the launch to May");
  await input.press("Enter");
  await expect(page.getByText("Carol proposed May")).toBeVisible();
  await page.waitForTimeout(1200); // past the slow answer's arrival
  await expect(page.getByText("Carol proposed May")).toBeVisible();
  await expect(page.getByText("Bob owns the dashboard performance budget")).toHaveCount(0);
});

test("audio player keeps its speed when switching meetings", async ({ page }) => {
  // Record every <audio> the player creates so we can read the real playbackRate.
  await page.addInitScript(() => {
    const made: HTMLAudioElement[] = [];
    (window as unknown as { __audios: HTMLAudioElement[] }).__audios = made;
    const Native = window.Audio;
    window.Audio = function (src?: string) {
      const a = new Native(src);
      made.push(a);
      return a;
    } as unknown as typeof Audio;
  });
  await page.goto(url(`/meetings/${MEETING}`, "audio=1"));
  await settle(page);
  const speed = page.getByRole("button", { name: /Playback speed/ });
  await expect(speed).toBeEnabled();
  await speed.click(); // 1 → 1.25
  await speed.click(); // → 1.5
  await expect(speed).toHaveAccessibleName(/1\.5/);

  // Same Detail screen instance, another meeting: the player builds new <audio> elements.
  const before = await page.evaluate(() => (window as unknown as { __audios: HTMLAudioElement[] }).__audios.length);
  await page.evaluate(() => {
    window.location.hash = "#/meetings/00000000-0000-4000-8000-000000000003";
  });
  const fresh = () =>
    page.evaluate((n) => (window as unknown as { __audios: HTMLAudioElement[] }).__audios.slice(n).map((a) => a.playbackRate), before);
  await expect.poll(async () => (await fresh()).length).toBeGreaterThan(0);
  expect(await fresh()).toEqual((await fresh()).map(() => 1.5));
  await expect(speed).toBeEnabled();
  await expect(speed).toHaveAccessibleName(/1\.5/);
});

test("search params live in the hash and don't come back from the sidebar", async ({ page, browserName }) => {
  await page.goto(url("/"));
  await settle(page);
  const search = page.getByRole("searchbox").first();
  await search.fill("launch");
  await search.press("Enter");
  await expect(page).toHaveURL(/#\/library\?focus=1&q=launch$/);
  expect(await page.evaluate(() => window.location.search)).toBe("");
  await expect(page.getByRole("searchbox", { name: "Search or ask" })).toHaveValue("launch");

  await page.getByRole("link", { name: /Meetings/ }).first().click();
  await expect(page).toHaveURL(/#\/$/);
  await expect(page.locator('[data-screen="meetings"]')).toBeAttached(); // Library has unmounted
  await page.keyboard.press(`${mod(browserName)}+3`);
  await expect(page).toHaveURL(/#\/library$/);
  await expect(page.getByRole("searchbox", { name: "Search or ask" })).toHaveValue("");
});

test("checklist changes save and say when the Markdown file was left unchanged", async ({ page }) => {
  await page.goto('/?checklist=unchanged#/meetings/' + MEETING);
  const result = await page.evaluate(async (id) => {
    const entrypoint = "/src/lib/api.ts";
    const { api } = await import(entrypoint) as typeof import("../src/lib/api");
    const before = await api.summaryMarkdown(id);
    const added = await api.addActionItem(id, { text: "Send the handoff" });
    const edited = await api.editActionItem(id, added.item.id, { text: "Send the notes" });
    const ticked = await api.setActionDone(added.item.id, true);
    const saved = (await api.getMeeting(id)).actionItems.find((a) => a.id === added.item.id);
    const deleted = await api.deleteActionItem(id, added.item.id);
    const after = await api.summaryMarkdown(id);
    await api.deleteMeetingParts(id, ["markdown"]);
    const withoutExport = await api.addActionItem(id, { text: "No file to update" });
    return { before, after, saved, withoutExport, notices: [added, edited, ticked, deleted].map((r) => r.markdownNotice) };
  }, MEETING);

  expect(result.saved).toMatchObject({ text: "Send the notes", done: true });
  for (const notice of result.notices) expect(notice).toContain("left unchanged");
  expect(result.after).toBe(result.before);
  expect(result.withoutExport.markdownNotice).toBeNull();
});

test("meeting cleanup reports an absent Markdown export separately from deletions", async ({ page }) => {
  await page.goto('/#/meetings/' + MEETING);
  const result = await page.evaluate(async (id) => {
    const entrypoint = "/src/lib/api.ts";
    const { api } = await import(entrypoint) as typeof import("../src/lib/api");
    const first = await api.deleteMeetingParts(id, ["markdown"]);
    const contents = await api.meetingContents(id);
    const second = await api.deleteMeetingParts(id, ["markdown"]);
    return { first, contents, second };
  }, MEETING);

  expect(result.first.deleted).toEqual(["markdown"]);
  expect(result.first.markdownNotice).toBeNull();
  expect(result.contents.markdownPath).toBeNull();
  expect(result.contents.markdownNotice).toContain("No Markdown export was found");
  expect(result.second.deleted).toEqual([]);
  expect(result.second.failures).toEqual([]);
  expect(result.second.markdownNotice).toBe(result.contents.markdownNotice);
});


// ── Accessibility pass: cloud planned + forced-colors focus visibility ──────

test("Settings → Transcription: Cloud is visibly disabled, planned, unreachable, and exposes its reason", async ({ page }) => {
  await page.goto(url("/settings/transcription"));
  await settle(page, "Where it runs");

  await expect(page.getByText(/Runs on this (Mac|PC|computer), free, private and offline\. Cloud transcription is planned for a future release\./)).toBeVisible();

  const group = page.getByRole("radiogroup", { name: "Where transcription runs" });
  const local = group.getByRole("radio", { name: /On this (Mac|PC|computer)/ });
  const cloud = group.getByRole("radio", { name: /Cloud.*Planned/i });

  await expect(local).toBeVisible();
  await expect(local).toBeEnabled();
  await expect(local).toHaveAttribute("aria-checked", "true");

  await expect(cloud).toBeVisible();
  await expect(cloud).toBeDisabled();
  await expect(cloud).toHaveAttribute("aria-checked", "false");
  await expect(cloud).not.toContainText("·");

  // Reason is exposed to screen readers via aria-describedby (accessible description).
  await expect(cloud).toHaveAccessibleDescription(/Cloud transcription is planned for a future release/);

  // Mouse click (even forced) and keyboard arrows cannot select Cloud.
  await cloud.click({ force: true });
  await expect(local).toHaveAttribute("aria-checked", "true");
  await expect(cloud).toHaveAttribute("aria-checked", "false");

  await local.focus();
  await page.keyboard.press("ArrowRight");
  await expect(local).toBeFocused();
  await expect(local).toHaveAttribute("aria-checked", "true");
  await expect(cloud).toHaveAttribute("aria-checked", "false");

  // AI summary providers (Codex, Grok, Ollama, API keys) remain enabled and usable.
  await page.goto(url("/settings/summaries"));
  await settle(page, "Summarize with");
  await expect(page.getByRole("button", { name: "Add key" }).first()).toBeEnabled();
  await expect(page.getByRole("switch", { name: "Use API keys as last resort" })).toBeEnabled();
});

test("forced-colors: focus outline survives on buttons, tabs, selects, segments, fields and rows", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "WebKit does not emulate Windows forced-colors");
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });

  const hasOutline = () =>
    page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) return false;
      const check = (node: Element | null) => {
        if (!node) return false;
        const cs = getComputedStyle(node);
        return cs.outlineStyle !== "none" && Number.parseFloat(cs.outlineWidth) >= 1;
      };
      // Inputs draw their focus outline on their containing .box / form field wrapper.
      return check(el) || check(el.parentElement) || check(el.closest("form"));
    });

  // Meetings: search field and lane row button
  await page.goto(url("/"));
  await settle(page, "Q2 Design Sync");
  await page.getByRole("searchbox").focus();
  expect(await hasOutline(), "SearchField focus outline in forced-colors").toBe(true);
  await page.keyboard.press("Tab");
  await expect(page.getByRole("button", { name: "Q2 Design Sync" })).toBeFocused();
  expect(await hasOutline(), "LaneRow button focus outline in forced-colors").toBe(true);

  // Detail: tabs
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(new RegExp(`#/meetings/${MEETING}$`));
  await settle(page, "Share updated mockups");
  await page.getByRole("tab", { name: "Summary" }).focus();
  expect(await hasOutline(), "TabsTrigger focus outline in forced-colors").toBe(true);

  // Settings: select, toggle, segmented control
  await page.goto(url("/settings/transcription"));
  await settle(page, "Where it runs");
  await page.getByRole("radio", { name: /On this (Mac|PC|computer)/ }).focus();
  expect(await hasOutline(), "Segmented item focus outline in forced-colors").toBe(true);
  await page.getByRole("combobox", { name: "Language" }).focus();
  expect(await hasOutline(), "Select trigger focus outline in forced-colors").toBe(true);
  await page.getByRole("switch", { name: "Label speakers" }).focus();
  expect(await hasOutline(), "Toggle focus outline in forced-colors").toBe(true);
});

test("live captions partial differs from final by more than colour, and provider handles do not trap Tab", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "WebKit skips non-form controls on Tab by default");
  await page.goto(url("/live", "state=recording"));
  await settle(page, "Great, I'll move it on the roadmap today.");
  await page.evaluate(() => {
    const emit = (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit;
    emit("recording://caption", {
      meetingId: "00000000-0000-4000-8000-000000000009",
      id: 9501,
      track: "mic",
      speaker: "You",
      atMs: 910000,
      endMs: 0,
      text: "Partial thought in progress",
      final: false,
    });
  });
  const partialP = page.locator('[data-final="false"] p').last();
  const finalP = page.locator('[data-final="true"] p').first();
  await expect(partialP).toContainText("Partial thought in progress…");
  expect(await partialP.evaluate((el) => getComputedStyle(el).fontStyle)).toBe("italic");
  expect(await finalP.evaluate((el) => getComputedStyle(el).fontStyle)).toBe("normal");
  // Verify a bundled Geist italic @font-face actually loaded (base.css sets font-synthesis: none).
  expect(
    await page.evaluate(() =>
      Array.from(document.fonts).some((f) => f.family.replace(/^"|"$/g, "") === "Geist" && f.style === "italic" && f.status === "loaded"),
    ),
  ).toBe(true);

  // Summaries: Tab leaves the reorder handle instead of trapping focus.
  await page.goto(url("/settings/summaries"));
  await settle(page, "Summarize with");
  const firstHandle = page.locator('button[id^="handle-"]').first();
  await firstHandle.focus();
  await expect(firstHandle).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(firstHandle).not.toBeFocused();
});

// ── Deterministic computed design-token colours (0-tolerance RGB check) ─────

const EXPECTED_TOKENS: Record<"light" | "dark", Record<string, string>> = {
  light: {
    "--bg": "rgb(255, 255, 255)",
    "--ink": "rgb(17, 17, 17)",
    "--muted": "rgb(107, 107, 107)",
    "--faint": "rgb(181, 181, 176)",
    "--surface": "rgb(244, 244, 242)",
    "--raised": "rgb(255, 255, 255)",
    "--line": "rgb(230, 230, 227)",
    "--line-strong": "rgb(217, 217, 213)",
    "--control-edge": "rgb(138, 138, 133)",
    "--on-ink": "rgb(255, 255, 255)",
    "--onair": "rgb(255, 59, 31)",
    "--onair-fill": "rgb(219, 47, 23)",
    "--on-onair": "rgb(255, 255, 255)",
    "--destructive": "rgb(200, 50, 31)",
    "--ok": "rgb(43, 182, 115)",
    "--hud": "rgb(17, 17, 17)",
  },
  dark: {
    "--bg": "rgb(13, 13, 12)",
    "--ink": "rgb(237, 237, 232)",
    "--muted": "rgb(154, 154, 148)",
    "--faint": "rgb(74, 74, 71)",
    "--surface": "rgb(24, 24, 23)",
    "--raised": "rgb(42, 42, 40)",
    "--line": "rgb(38, 38, 36)",
    "--line-strong": "rgb(58, 58, 56)",
    "--control-edge": "rgb(110, 110, 105)",
    "--on-ink": "rgb(13, 13, 12)",
    "--onair": "rgb(255, 74, 46)",
    "--onair-fill": "rgb(219, 47, 23)",
    "--on-onair": "rgb(255, 255, 255)",
    "--destructive": "rgb(240, 98, 77)",
    "--ok": "rgb(43, 182, 115)",
    "--hud": "rgb(38, 38, 36)",
  },
};

const EXPECTED_CONTRAST_MORE: Record<"light" | "dark", Record<string, string>> = {
  light: {
    "--muted": "rgb(74, 74, 71)",
    "--line": "rgb(138, 138, 133)",
    "--line-strong": "rgb(107, 107, 107)",
    "--control-edge": "rgb(89, 89, 86)",
    "--destructive": "rgb(166, 34, 18)",
  },
  dark: {
    "--muted": "rgb(189, 189, 183)",
    "--line": "rgb(110, 110, 105)",
    "--line-strong": "rgb(142, 142, 136)",
    "--control-edge": "rgb(154, 154, 148)",
    "--destructive": "rgb(255, 122, 102)",
  },
};

for (const theme of THEMES) {
  test(`computed design-token colours match DESIGN-LANGUAGE.md (${theme})`, async ({ page }) => {
    await open(page, theme, "/dictate");
    await settle(page, "Running ten minutes late");

    const resolveTokens = (names: string[]) =>
      page.evaluate((keys) => {
        const probe = document.createElement("div");
        probe.style.display = "none";
        document.body.appendChild(probe);
        const out: Record<string, string> = {};
        for (const key of keys) {
          probe.style.color = `var(${key})`;
          out[key] = getComputedStyle(probe).color;
        }
        probe.remove();
        return out;
      }, names);

    expect(await resolveTokens(Object.keys(EXPECTED_TOKENS[theme]))).toEqual(EXPECTED_TOKENS[theme]);

    // Verify rendered elements pick up the resolved token colours directly.
    const rendered = await page.evaluate(() => {
      const time = document.querySelector("time");
      return {
        bg: getComputedStyle(document.body).backgroundColor,
        ink: getComputedStyle(document.body).color,
        muted: time ? getComputedStyle(time).color : null,
      };
    });
    expect(rendered).toEqual({
      bg: EXPECTED_TOKENS[theme]["--bg"],
      ink: EXPECTED_TOKENS[theme]["--ink"],
      muted: EXPECTED_TOKENS[theme]["--muted"],
    });

    // prefers-contrast: more strengthens --muted, hairlines, --control-edge and --destructive.
    await page.emulateMedia({ colorScheme: theme, contrast: "more", reducedMotion: "reduce" });
    expect(await resolveTokens(Object.keys(EXPECTED_CONTRAST_MORE[theme]))).toEqual(EXPECTED_CONTRAST_MORE[theme]);
  });
}

// ── Dictate screen accessibility audit tests ────────────────────────────────

test("dictate a11y: heading hierarchy, screen-reader names, spoken shortcut and live search announcements", async ({ page, browserName }) => {
  await open(page, "light", "/dictate");
  await settle(page, "Running ten minutes late");

  // Headings: h1 "Dictate" followed by h2 "Recent" with no skipped heading levels.
  const headings = await page.locator("h1, h2, h3, h4, h5, h6").evaluateAll((els) =>
    els.map((el) => ({ tag: el.tagName, text: el.textContent?.trim() })),
  );
  expect(headings[0]).toEqual({ tag: "H1", text: "Dictate" });
  expect(headings[1]).toEqual({ tag: "H2", text: "Recent" });
  expect(headings.some((h) => h.tag === "H3")).toBe(false);

  // Spoken shortcut in How-to strip avoids raw unicode symbols and hides the duplicate decorative keycap.
  const howto = page.getByRole("region", { name: "How to dictate" });
  if (browserName === "webkit") {
    await expect(howto).toContainText("Option Space");
  } else {
    await expect(howto).toContainText("Alt + Space");
  }
  await expect(howto.locator("[aria-hidden='true'], [aria-hidden='']").first()).toBeAttached();

  // Every row's Copy and More actions buttons have unique accessible names referencing the dictation.
  const copyNames = await page.getByRole("button", { name: /^Copy “/ }).evaluateAll((els) =>
    els.map((el) => el.getAttribute("aria-label")),
  );
  const moreNames = await page.getByRole("button", { name: /^More actions for “/ }).evaluateAll((els) =>
    els.map((el) => el.getAttribute("aria-label")),
  );
  expect(copyNames.length).toBe(4);
  expect(new Set(copyNames).size).toBe(4);
  expect(moreNames.length).toBe(4);
  expect(new Set(moreNames).size).toBe(4);

  // "/" shortcut focuses search; live status announces match counts; Clear search returns focus to search.
  const search = page.getByRole("searchbox", { name: "Search dictations" });
  await page.keyboard.press("/");
  await expect(search).toBeFocused();

  const status = page.locator('[data-screen="dictate"] [role="status"]').first();
  await search.fill("Messages");
  await expect(status).toHaveText("1 dictation matches “Messages”.");

  await search.fill("nonexistent-query-xyz");
  await expect(status).toHaveText("No dictations match “nonexistent-query-xyz”.");
  await page.getByRole("button", { name: "Clear search" }).click();
  await expect(search).toHaveValue("");
  await expect(search).toBeFocused();
});

test("dictate a11y: keyboard order, focus rings, and focus restoration after cancel and delete", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "WebKit skips non-form controls on Tab by default");
  await open(page, "light", "/dictate");
  await settle(page, "Running ten minutes late");

  const hasRing = () =>
    page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) return false;
      const check = (n: Element | null) => {
        if (!n) return false;
        const cs = getComputedStyle(n);
        return cs.boxShadow !== "none" || (cs.outlineStyle !== "none" && Number.parseFloat(cs.outlineWidth) >= 1);
      };
      return check(el) || check(el.parentElement);
    });

  // Tab order: Search -> row 1 Copy -> row 1 More actions -> row 2 Copy.
  const search = page.getByRole("searchbox", { name: "Search dictations" });
  await search.focus();
  expect(await hasRing()).toBe(true);

  await page.keyboard.press("Tab");
  const firstCopy = page.getByRole("button", { name: /^Copy “Thanks for the notes/ });
  await expect(firstCopy).toBeFocused();
  expect(await hasRing()).toBe(true);

  await page.keyboard.press("Tab");
  const firstMore = page.getByRole("button", { name: /^More actions for “Thanks for the notes/ });
  await expect(firstMore).toBeFocused();
  expect(await hasRing()).toBe(true);
  await expect(firstMore).toHaveCSS("opacity", "1");

  // Open menu -> Delete… -> Cancel returns focus to the triggering More actions button.
  await page.keyboard.press("Enter");
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  const dialog = page.getByRole("dialog", { name: "Delete this dictation?" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(firstMore).toBeFocused();

  // Open menu -> Delete… -> Escape also returns focus to the triggering More actions button.
  await firstMore.click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(firstMore).toBeFocused();

  // Confirm delete moves focus to the next row's Copy button (not document.body).
  await firstMore.click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
  await dialog.getByRole("button", { name: "Delete dictation" }).click();
  await expect(page.getByRole("button", { name: /^Copy “Refactor the recorder/ })).toBeFocused();
});

for (const theme of THEMES) {
  test(`dictate a11y: loading state (${theme})`, async ({ page }) => {
    await open(page, theme, "/dictate", "dictations=loading");
    const loading = page.getByRole("status").filter({ hasText: "Loading dictations…" });
    await expect(loading).toBeVisible();
    await expect(page.locator('section[aria-labelledby="recent-label"]')).toHaveAttribute("aria-busy", "true");
    // Reduced motion stops the spinner animation.
    const anim = await loading.locator("span").first().evaluate((el) => getComputedStyle(el).animationName);
    expect(anim).toBe("none");

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(bad, JSON.stringify(bad)).toEqual([]);
  });
}

test("dictate a11y: error state announces alert and recovers on retry", async ({ page }) => {
  await open(page, "light", "/dictate", "dictations=errorOnce");
  await settle(page, "Couldn't load your dictations.");
  const alert = page.locator('[data-screen="dictate"] [role="alert"]');
  await expect(alert).toContainText("Couldn't load your dictations.");

  // Keyboard retry moves focus from the unmounted button to the Recent heading (not document.body).
  const retry = alert.getByRole("button", { name: "Try again" });
  await retry.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByText("Running ten minutes late")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Recent", level: 2 })).toBeFocused();

  // Mouse retry recovers without leaving a stray focus ring on any control or heading.
  await page.reload();
  await settle(page, "Couldn't load your dictations.");
  await alert.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByText("Running ten minutes late")).toBeVisible();
  const hasStrayFocusRing = await page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null;
    if (!el || el === document.body) return false;
    return el.matches(":focus-visible") || el.tagName === "INPUT";
  });
  expect(hasStrayFocusRing).toBe(false);
});

test("dictate a11y: hit targets (>= 28x28 desktop, >= 44x44 narrow), narrow width and 200% zoom reflow", async ({ page }) => {
  await open(page, "light", "/dictate");
  await settle(page, "Running ten minutes late");

  // Desktop hit targets: every interactive control inside Dictate is >= 28x28.
  const desktopBoxes = await page.locator('[data-screen="dictate"] button, [data-screen="dictate"] input').evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { label: el.getAttribute("aria-label") ?? el.textContent?.trim(), w: Math.round(r.width), h: Math.round(r.height) };
    }),
  );
  expect(desktopBoxes.length).toBeGreaterThan(0);
  for (const b of desktopBoxes) {
    expect(b.w, `${b.label} width on desktop`).toBeGreaterThanOrEqual(28);
    expect(b.h, `${b.label} height on desktop`).toBeGreaterThanOrEqual(28);
  }

  // Narrow (820px) and 200% zoom widths (640px for 1280@200%, 480px for 960@200%): no horizontal overflow,
  // More actions stays visible (no hover needed), and row buttons are >= 44x44.
  for (const vp of [
    { width: 820, height: 900 },
    { width: 640, height: 450 },
    { width: 480, height: 450 },
  ]) {
    await page.setViewportSize(vp);
    await expect(page.locator('[data-screen="dictate"] button[aria-label^="More actions"]').first()).toHaveCSS("opacity", "1");
    const metrics = await page.evaluate(() => {
      const main = document.getElementById("main");
      const pageEl = document.querySelector('[data-screen="dictate"]');
      const copy = document.querySelector('[data-screen="dictate"] button[aria-label^="Copy "]');
      const more = document.querySelector('[data-screen="dictate"] button[aria-label^="More actions"]');
      const cr = copy?.getBoundingClientRect();
      const mr = more?.getBoundingClientRect();
      return {
        docOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        mainOverflow: main ? main.scrollWidth - main.clientWidth : 0,
        pageOverflow: pageEl ? pageEl.scrollWidth - pageEl.clientWidth : 0,
        copyW: cr ? Math.round(cr.width) : 0,
        copyH: cr ? Math.round(cr.height) : 0,
        moreW: mr ? Math.round(mr.width) : 0,
        moreH: mr ? Math.round(mr.height) : 0,
      };
    });
    expect(metrics.docOverflow, `document overflow at ${vp.width}px`).toBeLessThanOrEqual(0);
    expect(metrics.mainOverflow, `main overflow at ${vp.width}px`).toBeLessThanOrEqual(0);
    expect(metrics.pageOverflow, `dictate overflow at ${vp.width}px`).toBeLessThanOrEqual(0);
    expect(metrics.copyW, `Copy width at ${vp.width}px`).toBeGreaterThanOrEqual(44);
    expect(metrics.copyH, `Copy height at ${vp.width}px`).toBeGreaterThanOrEqual(44);
    expect(metrics.moreW, `More width at ${vp.width}px`).toBeGreaterThanOrEqual(44);
    expect(metrics.moreH, `More height at ${vp.width}px`).toBeGreaterThanOrEqual(44);
  }
});

for (const theme of THEMES) {
  test(`dictate a11y: prefers-contrast: more (${theme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme, contrast: "more", reducedMotion: "reduce" });
    await page.clock.setFixedTime(NOW);
    await page.goto(url("/dictate"));
    await settle(page, "Running ten minutes late");

    const howtoBorder = await page.getByRole("region", { name: "How to dictate" }).evaluate((el) => {
      const cs = getComputedStyle(el);
      return { style: cs.borderTopStyle, width: Number.parseFloat(cs.borderTopWidth) };
    });
    expect(howtoBorder.style).toBe("solid");
    expect(howtoBorder.width).toBeGreaterThanOrEqual(1);

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
    expect(bad, JSON.stringify(bad)).toEqual([]);
  });
}

test("dictate a11y: forced-colors: active focus outlines and boundaries", async ({ page, browserName }) => {
  test.skip(browserName === "webkit", "WebKit does not emulate Windows forced-colors");
  await page.emulateMedia({ forcedColors: "active", reducedMotion: "reduce" });
  await page.clock.setFixedTime(NOW);
  await page.goto(url("/dictate"));
  await settle(page, "Running ten minutes late");

  const hasOutline = () =>
    page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el) return false;
      const check = (n: Element | null) => {
        if (!n) return false;
        const cs = getComputedStyle(n);
        return cs.outlineStyle !== "none" && Number.parseFloat(cs.outlineWidth) >= 1;
      };
      return check(el) || check(el.parentElement);
    });

  await page.getByRole("searchbox", { name: "Search dictations" }).focus();
  expect(await hasOutline(), "SearchField focus outline in forced-colors").toBe(true);
  await page.getByRole("button", { name: /^Copy “/ }).first().focus();
  expect(await hasOutline(), "Copy button focus outline in forced-colors").toBe(true);
  await page.getByRole("button", { name: /^More actions for “/ }).first().focus();
  expect(await hasOutline(), "More actions focus outline in forced-colors").toBe(true);
});

test("interactive HUD dismisses a persistent error through the backend", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 64 });
  await page.clock.install();
  await open(page, "light", "/hud", "os=macos&hud=error");
  await settle(page);
  await page.clock.fastForward(7000);
  await expect(page.getByRole("alert")).toHaveText("Microphone is off");
  const dismiss = page.getByRole("button", { name: "Dismiss", exact: true });
  await dismiss.focus();
  await expect(dismiss).toBeFocused();
  await dismiss.click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  const state = await page.evaluate(async () => {
    const path = "/src/lib/api.ts";
    const { api } = await import(path);
    return (await api.getDictationHud()).state;
  });
  expect(state).toBe("idle");
});

for (const query of ["os=windows", "os=linux&session=x11", "os=linux&session=wayland"]) {
  test(`passive HUD has no controls (${query})`, async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 64 });
    await open(page, "light", "/hud", `${query}&hud=error`);
    await settle(page);
    await expect(page.getByRole("alert")).toHaveText("Microphone is off");
    await expect(page.getByRole("button")).toHaveCount(0);
  });
}

test("HUD Dismiss racing a new dictation leaves it running", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 64 });
  await open(page, "light", "/hud", "os=macos&hud=error");
  await settle(page);
  await expect(page.getByRole("button", { name: "Dismiss", exact: true })).toBeVisible();
  const state = await page.evaluate(async () => {
    const path = "/src/lib/api.ts";
    const { api } = await import(path);
    const button = document.querySelector("button");
    // Same task: the backend has moved on, but the error button is still on screen.
    (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit("dictation://hud", {
      state: "listening", level: 0.5, handsFree: false, app: "Slack", shape: "tidy", shortcutPresets: [],
    });
    button?.click();
    await new Promise((done) => setTimeout(done, 400));
    return (await api.getDictationHud()).state;
  });
  expect(state).toBe("listening");
});

test("HUD interaction does not hold a later result open", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 64 });
  await page.clock.install();
  await open(page, "light", "/hud", "os=macos&hud=inserted");
  await settle(page);
  await page.evaluate(async () => {
    const path = "/src/lib/api.ts";
    const { api } = await import(path);
    const emit = (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit;
    const hovered = api.setDictationHudInteraction(true, false);
    await Promise.resolve();
    emit("dictation://hud", { state: "processing" });
    void hovered;
  });
  await page.clock.fastForward(1000);
  await page.evaluate(() => {
    (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit("dictation://hud", {
      state: "inserted", words: 2, undoAvailable: false, resultId: null,
    });
  });
  await expect(page.getByRole("status")).toContainText("Inserted");
  await page.clock.fastForward(7000);
  await expect(page.getByRole("status")).toHaveCount(0);
});

test("settings patches drop edits for identifier-preserving shapes", async ({ page }) => {
  await open(page, "light", "/dictate");
  const keys = await page.evaluate(async () => {
    const path = "/src/lib/api.ts";
    const { api } = await import(path);
    const edits = { capitals: "sentence", dropFinalPeriod: true };
    await api.updateSettings({ dictationShapeEdits: { verbatim: edits, codeAndCommit: edits, email: edits } });
    return Object.keys((await api.getSettings()).dictationShapeEdits);
  });
  expect(keys).toEqual(["email"]);
});

test("Dictate describes cursor insertion on X11", async ({ page }) => {
  await open(page, "light", "/dictate", "os=linux&session=x11");
  await settle(page);
  await expect(page.getByText("Speak anywhere · Types where your cursor is")).toBeVisible();
});
