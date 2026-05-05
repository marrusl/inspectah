/**
 * Shared helpers for inspectah Go port e2e tests.
 */
import { Page, Locator, expect } from '@playwright/test';
import { readFileSync, existsSync } from 'fs';
import { join } from 'path';

const PID_FILE = join(__dirname, '..', '.server-pids');

// Server name → PID file index mapping (matches globalSetup SERVERS order)
const SERVER_PID_INDEX: Record<string, number> = {
  'fleet-refine': 0,
  'single-refine': 1,
  'architect': 2,
};

// Resolve a base URL to a server name for PID lookup
function serverNameFromURL(url: string): string {
  if (url.includes('9201')) return 'single-refine';
  if (url.includes('9202')) return 'architect';
  return 'fleet-refine';
}

/**
 * Fast server liveness check. Verifies the server PID is alive and its
 * health endpoint responds. Fails immediately with a diagnostic message
 * instead of letting Playwright hit a 30-second timeout on a dead server.
 *
 * Called automatically by waitForBoot() and waitForArchitectBoot()
 * before any DOM assertions.
 */
export async function checkServerLiveness(baseURL?: string): Promise<void> {
  const url = baseURL || process.env.REFINE_FLEET_URL || 'http://localhost:9200';
  const serverName = serverNameFromURL(url);
  const pidIndex = SERVER_PID_INDEX[serverName] ?? 0;

  // Check PID file exists and the target server PID is alive
  if (existsSync(PID_FILE)) {
    const pids = readFileSync(PID_FILE, 'utf-8').trim().split('\n').filter(Boolean).map(Number);
    const pid = pids[pidIndex];
    if (pid) {
      try {
        process.kill(pid, 0); // signal 0 = existence check
      } catch {
        throw new Error(
          `${serverName} server (pid=${pid}) is no longer running — ` +
          `likely crashed during a previous test. Remaining tests will fail ` +
          `with ERR_CONNECTION_REFUSED. Check server stderr for panic details.`
        );
      }
    }
  }

  // Quick HTTP health check with fast timeout (2s instead of 30s)
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 2_000);
  try {
    const resp = await fetch(`${url}/api/health`, { signal: controller.signal });
    if (!resp.ok) {
      throw new Error(
        `${serverName} server health check returned ${resp.status} — ` +
        `server may be in a degraded state.`
      );
    }
  } catch (err: unknown) {
    if (err instanceof Error && err.message.includes('server')) {
      throw err; // re-throw our own diagnostic errors
    }
    throw new Error(
      `${serverName} server is not responding at ${url}/api/health — ` +
      `likely crashed during a previous test. Check server stderr for details.`
    );
  } finally {
    clearTimeout(timeout);
  }
}

/** Wait for the report SPA to fully boot (sidebar + sections rendered). */
export async function waitForBoot(page: Page): Promise<void> {
  // Fast liveness check before waiting for DOM — catches dead servers
  // immediately instead of timing out after 30 seconds.
  await checkServerLiveness();

  // The boot() function renders the sidebar and navigates to 'overview'.
  // Wait for the sidebar nav to have links and the overview heading to appear.
  await page.waitForSelector('#sidebar .pf-v6-c-nav__link', { timeout: 10_000 });
  await page.waitForSelector('#heading-overview', { timeout: 10_000 });
}

/**
 * Wait for the report SPA to boot AND refine mode to be fully established.
 * Refine mode is async (XHR to /api/health, then /api/snapshot), so the
 * rebuild bar becoming active is the definitive signal.
 */
export async function waitForRefineBoot(page: Page): Promise<void> {
  await waitForBoot(page);
  // The rebuild bar gets class 'active' when enableRefineMode() fires.
  // This is the definitive signal that refine mode is ready.
  await page.waitForSelector('#rebuild-bar.active', { timeout: 10_000 });
}

/** Navigate to a section via sidebar link click. */
export async function navigateToSection(page: Page, sectionId: string): Promise<void> {
  await page.click(`[data-section="${sectionId}"]`);
  await page.waitForSelector(`#heading-${sectionId}`, { timeout: 5_000 });
}

/**
 * Get all sidebar section IDs currently rendered.
 * Returns the data-section attribute values.
 */
export async function getSidebarSections(page: Page): Promise<string[]> {
  return page.$$eval('[data-section]', (els) =>
    els.map((el) => el.getAttribute('data-section') || '')
  );
}

/** Check if the page is in refine mode (rebuild bar visible). */
export async function isRefineMode(page: Page): Promise<boolean> {
  const bar = page.locator('#rebuild-bar');
  // The rebuild bar exists in the DOM always but may be hidden in static mode
  const display = await bar.evaluate((el) => getComputedStyle(el).display);
  return display !== 'none';
}

/** Get the current theme (dark or light). */
export async function getTheme(page: Page): Promise<'dark' | 'light'> {
  const isDark = await page.evaluate(() =>
    document.body.classList.contains('pf-v6-theme-dark')
  );
  return isDark ? 'dark' : 'light';
}

/**
 * Find the first interactive toggle (role="switch") in a section.
 * Uses semantic selectors: tries accordion-toggle first, then item-toggle.
 * Returns null if no toggles exist.
 */
export async function findToggleInSection(
  page: Page,
  sectionId: string
): Promise<Locator | null> {
  // item-toggle buttons use role="switch" with aria-checked
  const toggle = page.locator(`#section-${sectionId} button[role="switch"]`).first();
  if ((await toggle.count()) > 0) return toggle;

  // Fallback: accordion toggles
  const accordion = page.locator(`#section-${sectionId} .accordion-toggle`).first();
  if ((await accordion.count()) > 0) return accordion;

  return null;
}

/** Wait for the architect page to be fully rendered (fleet sidebar + layer tree). */
export async function waitForArchitectBoot(page: Page): Promise<void> {
  // Fast liveness check for the architect server
  const archURL = process.env.ARCHITECT_URL || 'http://localhost:9202';
  await checkServerLiveness(archURL);

  await page.waitForSelector('#fleet-list', { timeout: 10_000 });
  await page.waitForSelector('#layer-tree', { timeout: 10_000 });
}

/** Get the architect URL from env, with fallback. */
export function architectURL(): string {
  return process.env.ARCHITECT_URL || 'http://localhost:9202';
}
