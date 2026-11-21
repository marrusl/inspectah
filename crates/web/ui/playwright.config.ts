import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // CI retries a failed test once, only to record a trace of it
  // (trace: "on-first-retry"). failOnFlakyTests then fails the run when the
  // retry passes, so a test that fails and recovers is reported as a failure
  // rather than hidden in a green job.
  retries: process.env.CI ? 1 : 0,
  failOnFlakyTests: !!process.env.CI,
  workers: process.env.CI ? 1 : undefined,
  // The HTML report alongside the github one: it holds the traces and
  // screenshots of a failed or flaky test. The workflow uploads
  // playwright-report/ as an artifact when the step fails, which a flaky
  // test now makes it do.
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
