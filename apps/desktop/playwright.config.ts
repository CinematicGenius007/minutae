// Screenshot + accessibility suite for the desktop UI, run against `vite` with the in-browser
// mock backend (src/lib/mock.ts). Chromium stands in for WebView2 (Windows) and WebKit for
// WKWebView (macOS) / WebKitGTK (Linux).
//
//   bun run test:e2e            compare against baselines
//   bun run test:e2e:update     regenerate baselines (review the PNG diff before committing)
import { defineConfig, devices } from "@playwright/test";

// Worktrees each run their own vite: set E2E_PORT so a server from another checkout is never reused.
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
      // Font hinting differs slightly between machines; layout or token regressions do not hide under this.
      maxDiffPixelRatio: 0.002,
      threshold: 0.2,
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
