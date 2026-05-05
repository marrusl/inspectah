import { defineConfig } from '@playwright/test';

/**
 * Playwright config for inspectah Go port e2e tests.
 *
 * Three server instances are started in globalSetup:
 *   - fleet refine server  (multi-host tarball)   -> REFINE_FLEET_URL
 *   - single refine server (single-host tarball)   -> REFINE_SINGLE_URL
 *   - architect server     (topology directory)    -> ARCHITECT_URL
 *
 * Tests default to REFINE_FLEET_URL as the baseURL. Tests targeting
 * the single-host or architect UIs override baseURL per-test.
 */
export default defineConfig({
  testDir: './tests',
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: process.env.REFINE_FLEET_URL || 'http://localhost:9200',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
  },
  globalSetup: './globalSetup.ts',
  globalTeardown: './globalTeardown.ts',
  projects: [
    {
      name: 'chromium',
      use: { browserName: 'chromium' },
    },
  ],
});
