// Screenshot suite: every route and state, light and dark, against the mock backend.
// Baselines live in e2e/__screenshots__/<browser>/. Regenerate with `bun run test:e2e:update`.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Locator, type Page, test } from "@playwright/test";

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
  /** Runs after the page settles, before the screenshot. */
  act?: (page: Page) => Promise<void>;
  /** Regions that change every frame (timers, live waveform). */
  mask?: (page: Page) => Locator[];
  /** Waits for this text before capturing. */
  ready?: string;
}

const liveMask = (page: Page) => [page.locator("canvas"), page.locator("[role=timer]"), page.locator("button[aria-pressed='true']")];

const SHOTS: Shot[] = [
  { name: "meetings", route: "/", ready: "Q2 Design Sync" },
  { name: "meetings-empty", route: "/", query: "state=empty" },
  { name: "live", route: "/live", query: "state=recording", mask: liveMask },
  { name: "detail", route: `/meetings/${MEETING}`, ready: "Share updated mockups" },
  { name: "detail-processing", route: `/meetings/${MEETING}`, query: "state=processing", ready: "Hiring loop" },
  { name: "detail-failed", route: `/meetings/${MEETING}`, query: "state=failed", ready: "Vendor call" },
  { name: "dictate", route: "/dictate", ready: "Running ten minutes late" },
  { name: "dictate-empty", route: "/dictate", query: "state=empty" },
  { name: "library", route: "/library" },
  {
    name: "library-answer",
    route: "/library",
    // wouter's hash navigate() keeps the query before the hash: /?q=…#/library
    query: `q=${encodeURIComponent("When did we move the launch to May")}`,
    act: async (page) => {
      const input = page.getByRole("searchbox").first();
      await expect(input).toHaveValue(/launch/);
      await page.waitForTimeout(400); // debounce + mock search
      await input.press("Enter");
      await expect(page.getByText("Carol proposed May")).toBeVisible();
    },
  },
  ...(["general", "transcription", "summaries", "dictation", "storage", "usage"] as const).map((s) => ({
    name: `settings-${s}`,
    route: `/settings/${s}`,
  })),
  { name: "onboarding", route: "/onboarding", ready: "Microphone" },
  { name: "prompt", route: "/prompt" },
  { name: "gallery", route: "/gallery" },
];

const HUD_STATES: { name: string; payload: unknown }[] = [
  { name: "listening", payload: { state: "listening", level: 0.7, handsFree: false } },
  { name: "listening-handsfree", payload: { state: "listening", level: 0.4, handsFree: true } },
  { name: "processing", payload: { state: "processing" } },
  { name: "inserted", payload: { state: "inserted", words: 23 } },
  { name: "copied", payload: { state: "copied", words: 12, reason: "No text field · copied instead" } },
  { name: "error", payload: { state: "error", message: "Microphone is off. Allow it in Settings." } },
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

/** Mock calls resolve after ~120 ms; let lazy routes, data and fonts settle. */
async function settle(page: Page, ready?: string) {
  if (ready) await expect(page.getByText(ready).first()).toBeVisible();
  await page.waitForTimeout(500);
  await page.evaluate(() => document.fonts.ready);
}

for (const theme of THEMES) {
  test.describe(`${theme}`, () => {
    for (const shot of SHOTS) {
      test(`${shot.name}`, async ({ page }) => {
        await open(page, theme, shot.route, shot.query);
        await settle(page, shot.ready);
        if (shot.act) await shot.act(page);
        await expect(page).toHaveScreenshot(`${shot.name}-${theme}.png`, {
          fullPage: shot.name === "gallery",
          mask: shot.mask?.(page),
        });
      });
    }

    for (const h of HUD_STATES) {
      test(`hud ${h.name}`, async ({ page }) => {
        await page.setViewportSize({ width: 480, height: 120 });
        await open(page, theme, "/hud");
        await page.waitForFunction(() => "__minutaeMockEmit" in window);
        await page.waitForTimeout(300); // lazy route + subscription
        await page.evaluate((p) => {
          const emit = (window as unknown as { __minutaeMockEmit: (e: string, p: unknown) => void }).__minutaeMockEmit;
          // Listening: a few level frames so the bars have a shape.
          if ((p as { state: string }).state === "listening") {
            for (const level of [0.3, 0.8, 1, 0.5, 0.75]) emit("dictation://hud", { ...(p as object), level });
          }
          emit("dictation://hud", p);
        }, h.payload);
        await expect(page.getByRole(h.name === "error" ? "alert" : "status")).toBeVisible();
        await expect(page).toHaveScreenshot(`hud-${h.name}-${theme}.png`);
      });
    }

    for (const [name, route, ready] of [
      ["meetings", "/", "Q2 Design Sync"],
      ["detail", `/meetings/${MEETING}`, "Share updated mockups"],
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
  await page.goto(url("/library", "q=mockups%20Friday"));
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
  ["detail", `/meetings/${MEETING}`],
  ["detail-failed", `/meetings/${MEETING}`, "state=failed"],
  ["dictate", "/dictate"],
  ["library", "/library", "q=launch"],
  ["settings-general", "/settings/general"],
  ["settings-transcription", "/settings/transcription"],
  ["settings-summaries", "/settings/summaries"],
  ["settings-dictation", "/settings/dictation"],
  ["settings-storage", "/settings/storage"],
  ["settings-usage", "/settings/usage"],
  ["onboarding", "/onboarding"],
  ["prompt", "/prompt"],
];

for (const theme of THEMES) {
  for (const [name, route, query] of A11Y_ROUTES) {
    test(`a11y ${name} ${theme}`, async ({ page }) => {
      await open(page, theme, route, query);
      await settle(page);
      const results = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        // Known design exception: white on --onair (3.6:1) in the live record pill. Red is the
        // spec'd "live" fill (DESIGN-LANGUAGE §2/§7); tracked as a design ask, not silenced elsewhere.
        .exclude("button[aria-pressed='true']")
        .analyze();
      const bad = results.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
      const report = bad.map((v) => `${v.id} (${v.impact}): ${v.help}\n  ${v.nodes.map((n) => n.target.join(" ")).slice(0, 6).join("\n  ")}`);
      expect(report, report.join("\n")).toEqual([]);
    });
  }
}
