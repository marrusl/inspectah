import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // One retry, not two. At two, a spec can fail twice and pass on the third
  // attempt and the job still reports green, which hides the known
  // order-dependent test rather than surfacing it. CI runs workers: 1, so
  // ordering differs from local runs and order-dependence is more likely to
  // bite there. One retry still absorbs genuine browser flake; a test that
  // fails deterministically fails both attempts.
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 1 : undefined,
  // The HTML report alongside the github one so a retry is visible at all:
  // "github" annotates failures but does not surface flake counts, so a test
  // that passed only on its retry looks identical to one that passed first
  // time. The workflow uploads playwright-report/ as an artifact on failure.
  reporter: process.env.CI
    ? [["github"], ["html", { open: "never" }]]
    : "list",
  use: {
    baseURL: "http://localhost:5174",
    screenshot: "only-on-failure",
    trace: "on-first-retry",
  },
  webServer: {
    command: "npx vite --port 5174",
    url: "http://localhost:5174",
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
