// Screenshot + accessibility suite for the desktop UI, run against `vite` with the in-browser
// mock backend (src/lib/mock.ts). Chromium stands in for WebView2 (Windows) and WebKit for
// WKWebView (macOS) / WebKitGTK (Linux).
//
//   bun run test:e2e            compare against baselines
//   bun run test:e2e:update     regenerate baselines (review the PNG diff before committing)
import { defineConfig, devices } from "@playwright/test";

// Each checkout (git worktree) can run its own server: E2E_PORT=1431 bun run test:e2e.
// reuseExistingServer would otherwise silently test whichever checkout already holds the port.
const port = Number(process.env.E2E_PORT ?? 1420);

export default defineConfig({
  testDir: "./e2e",
  outputDir: "./e2e/.results",
  snapshotPathTemplate: "{testDir}/__screenshots__/{projectName}/{arg}{ext}",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never", outputFolder: "./e2e/.report" }]] : "list",
  timeout: 30_000,
  expect: {
    toHaveScreenshot: {
      // pixelmatch uses includeAA: false (anti-aliased glyph edges are already excluded) and
      // compares pixels in YIQ space with maxDelta = 35215 * threshold^2. The old defaults
      // (threshold: 0.2 -> ±53 RGB, maxDiffPixelRatio: 0.002 -> 2,592 px at 1440x900) let
      // real token colour changes (e.g. dark --muted #8E8E88 -> #9A9A94, Δ=12, 299..4,445 px),
      // --onair -> --onair-fill, font swaps and even a missing settings row pass silently.
      // threshold: 0.03 flags neutral-grey shifts > ~8 RGB (~1.5x margin on Δ=12) while ignoring
      // minor ±7 RGB CoreText gamma/hinting drift across macOS releases; maxDiffPixelRatio: 0.00015
      // (194 px at 1440x900, 25 px on 320x520 tray) catches even sparse token changes like
      // dictate-light's single search placeholder (231 px) and prompt-dark's --muted (299 px),
      // while leaving small non-AA font-hinting headroom between local macOS and CI's macos-latest.
      // Exact 0-tolerance RGB token fidelity is additionally enforced by the deterministic
      // computed-token test in e2e/screens.spec.ts.
      maxDiffPixelRatio: 0.00015,
      threshold: 0.03,
      animations: "disabled",
      caret: "hide",
      scale: "css",
    },
  },
  use: {
    baseURL: `http://localhost:${port}`,
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
    locale: "en-GB",
    timezoneId: "Europe/London",
    reducedMotion: "reduce",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } },
    { name: "webkit", use: { ...devices["Desktop Safari"], viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } },
  ],
  webServer: {
    command: `bunx vite --port ${port} --strictPort`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
