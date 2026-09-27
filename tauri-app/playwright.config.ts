import { defineConfig, devices } from "@playwright/test";

/**
 * Browser tests for the webview.
 *
 * The interface is plain web code, so it runs in Chromium against the Vite
 * dev server with a stub Tauri runtime injected before the application loads.
 * That means these tests drive the real components, the real store, and the
 * real event reducers; only the boundary is replaced.
 */
export default defineConfig({
  testDir: "./tests",
  testMatch: /.*\.spec\.ts/,
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  reporter: process.env.CI ? "line" : [["list"]],
  use: {
    baseURL: "http://127.0.0.1:1420",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
  ],
  webServer: {
    // Bound to IPv4 explicitly: the dev server otherwise listens on whatever
    // `localhost` resolves to first, which is not always the loopback address
    // the test runner polls.
    command: "npm run dev -- --port 1420 --strictPort --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
