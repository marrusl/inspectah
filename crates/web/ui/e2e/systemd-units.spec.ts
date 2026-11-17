import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks, mockSequence } from "./helpers/mock-api";
import {
  FILTER_LABELS,
  FILTER_VALUES,
  keyFields,
  outcomeCounts,
  type UnitFields,
} from "../src/components/unitOutcome";

// The Systemd units section in a real browser (spec § 10.3, P1 to P8).
//
// Every test calls setupUnits (§ 10.1): the view and sections bodies were
// recorded from one refine session serving a kitchen-sink scan, and
// `/api/groups` is the recorded body `system_tuning_fixtures_test` holds to
// the real handler. The hand-written preset view is not used: its package
// and config rows have no disposition and crash the app.

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SINGLE_HOST = path.join(HERE, "fixtures", "single-host");
const VIEW_BODIES = {
  "kitchen-sink": path.join(SINGLE_HOST, "view-units-kitchen-sink.json"),
  stock10: path.join(SINGLE_HOST, "view-units-stock10.json"),
} as const;
const SECTIONS_BODY = path.join(SINGLE_HOST, "sections-units-kitchen-sink.json");
const AFTER_EXCLUDE_TMP_MOUNT = path.join(
  HERE,
  "fixtures",
  "sequences",
  "units-exclude-tmp-mount",
  "01-after-exclude.json",
);
const GROUPS_BODY = path.join(
  HERE,
  "..",
  "src",
  "__tests__",
  "fixtures",
  "system-tuning",
  "files-no-tuned.groups.json",
);

/** Bound on Tab and `j` presses while walking to a target, so a miss fails instead of looping. */
const MAX_KEY_PRESSES = 60;

function readView(view: keyof typeof VIEW_BODIES): UnitFields {
  return JSON.parse(fs.readFileSync(VIEW_BODIES[view], "utf-8")) as UnitFields;
}

/** The WCAG tags and impacts `expectNoAxeViolations` checks. */
const AXE_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];
const BLOCKING_IMPACTS = ["critical", "serious"];
const SECTION_SELECTOR = '[data-testid="service-section"]';

/** The viewport `responsive.spec.ts` uses for a narrow layout, and a desktop one. */
const NARROW = { width: 768, height: 1024 };
const DESKTOP = { width: 1280, height: 800 };

/** A long row for the narrow layout: a 200-character unit name and a long reason. */
const LONG_NAME_LENGTH = 200;
const LONG_UNIT = `${"long-unit-name-".repeat(13).slice(0, LONG_NAME_LENGTH - ".service".length)}.service`;
const LONG_DECLINED_TEXT =
  `${"The unit's install section cannot be carried as recorded on the source host. ".repeat(6)}` +
  `/etc/systemd/system/${"nested-directory/".repeat(10)}override.conf`;

/** Axe over the given parts of the page; fails on any critical or serious violation. */
async function expectNoBlockingViolations(
  page: Page,
  include: string[],
  rules?: string[],
) {
  let builder = new AxeBuilder({ page });
  for (const selector of include) builder = builder.include(selector);
  builder = rules ? builder.withRules(rules) : builder.withTags(AXE_TAGS);
  const results = await builder.analyze();
  const blocking = results.violations.filter((v) => BLOCKING_IMPACTS.includes(v.impact ?? ""));
  const summary = blocking
    .map((v) => `[${v.impact}] ${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)
    .join("\n");
  expect(blocking, `Accessibility violations:\n${summary}`).toEqual([]);
  return results;
}

/** The section's horizontal overflow: the page scrolls sideways when this is true. */
function pageScrollsSideways(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const root = document.scrollingElement!;
    return root.scrollWidth > root.clientWidth;
  });
}

/** The distinct `top` values of the bar's buttons: one per line they occupy. */
async function barLineCount(page: Page): Promise<number> {
  const tops = await filterButtons(page).evaluateAll((els) =>
    els.map((el) => Math.round(el.getBoundingClientRect().top)),
  );
  return new Set(tops).size;
}

/** Whether every bar button lies within the section's left and right edges. */
function barButtonsInsideSection(page: Page): Promise<boolean> {
  return page.evaluate((selector) => {
    const section = document.querySelector(selector)!.getBoundingClientRect();
    const buttons = document.querySelectorAll(`${selector} [aria-label="Show"] button`);
    return (
      buttons.length > 0 &&
      Array.from(buttons).every((b) => {
        const r = b.getBoundingClientRect();
        return r.left >= section.left && r.right <= section.right;
      })
    );
  }, SECTION_SELECTOR);
}

/** Uncaught page errors of the running test, asserted empty after it. */
let pageErrors: string[] = [];

async function setupUnits(page: Page, view: keyof typeof VIEW_BODIES = "kitchen-sink") {
  await applyMockApi(page, "single-host", {
    "/api/view": VIEW_BODIES[view],
    "/api/snapshot/sections": SECTIONS_BODY,
  });
  // `/api/groups` is not in the preset route map; serve the recorded body.
  const groups = fs.readFileSync(GROUPS_BODY, "utf-8");
  await page.route("**/api/groups", (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: groups }),
  );
  page.on("pageerror", (err) => pageErrors.push(err.message));
}

function sidebarEntry(page: Page, name: RegExp) {
  return page.getByRole("navigation", { name: "Sections" }).getByRole("link", { name });
}

/** Load the app and open the units section from the sidebar. */
async function openUnits(page: Page) {
  await page.goto("/");
  await sidebarEntry(page, /^Systemd units /).click();
  const section = page.getByTestId("service-section");
  await expect(section).toBeVisible();
  return section;
}

/** The filter bar's buttons, each read as "<label> <count>". */
function filterButtons(page: Page) {
  return page.getByRole("group", { name: "Show" }).getByRole("button");
}

async function filterTexts(page: Page): Promise<string[]> {
  const texts = await filterButtons(page).allInnerTexts();
  return texts.map((t) => t.replace(/\s+/g, " ").trim());
}

/** The shown group headers' accessible names, in section order. */
async function groupHeaders(page: Page): Promise<string[]> {
  return page
    .getByTestId("service-section")
    .locator(".inspectah-unit-group > button")
    .evaluateAll((els) => els.map((el) => el.getAttribute("aria-label") ?? ""));
}

/**
 * Shown rows other than drop-ins: unit rows, the default target and the
 * reported rows. These are what the bar's counts count.
 */
function shownCountedRows(page: Page) {
  return page
    .getByTestId("service-section")
    .locator('[role="row"]:visible:not([data-testid^="dropin-item-"])');
}

/** Shown orphan drop-in rows: shown rows with no outcome, which the bar does not count. */
function shownOrphanRows(page: Page) {
  return page.getByTestId("service-section").locator('[data-testid="dropin-orphans"] [role="row"]:visible');
}

test.describe("Systemd units section", () => {
  test.beforeEach(() => {
    pageErrors = [];
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
    expect(pageErrors).toEqual([]);
  });

  test("P1: renders the groups in order, the bar's counts and the sidebar entry", async ({
    page,
  }) => {
    await setupUnits(page);
    await page.goto("/");

    const entry = sidebarEntry(page, /^Systemd units /);
    await expect(entry.locator(".pf-v6-c-badge")).toHaveText("24");

    await openUnits(page);
    await expect(
      page.locator(".inspectah-layout__main").getByRole("heading", { level: 2, name: "Systemd units" }),
    ).toBeVisible();
    expect(await groupHeaders(page)).toEqual([
      "Default target",
      "Services 11, 5 need follow-up",
      "Sockets 6",
      "Timers 1",
      "Other units 1, 1 needs follow-up",
      "Reported, not carried 3",
    ]);
    expect(await filterTexts(page)).toEqual([
      "All 23",
      "Needs follow-up 9",
      "Carried 14",
      "Excluded 0",
    ]);
    await expect(filterButtons(page).filter({ hasText: "Excluded" })).toBeDisabled();
  });

  test("P2: Needs follow-up leaves 9 rows, All restores 24 beside the bar's All 23", async ({ page }) => {
    await setupUnits(page);
    const section = await openUnits(page);
    const live = section.locator('[aria-live="polite"]');

    await filterButtons(page).filter({ hasText: "Needs follow-up" }).click();
    await expect(shownCountedRows(page)).toHaveCount(9);
    await expect(shownOrphanRows(page)).toHaveCount(0);
    expect(
      (await groupHeaders(page)).map((name) => name.replace(/ \d.*$/, "")),
    ).toEqual(["Services", "Other units", "Reported, not carried"]);
    await expect(live).toHaveText("Showing 9 rows that need follow-up.");

    await filterButtons(page).filter({ hasText: "All" }).click();
    // 24 rows shown: the 23 with an outcome and sshd.service's orphan drop-in.
    await expect(shownCountedRows(page)).toHaveCount(23);
    await expect(shownOrphanRows(page)).toHaveCount(1);
    await expect(
      page.getByTestId("service-section").locator('[data-testid="dropin-orphan-group-sshd.service"] [role="row"]'),
    ).toBeVisible();
    await expect(filterButtons(page).filter({ hasText: "All" })).toHaveText("All 23");
    expect(await groupHeaders(page)).toHaveLength(6);
    await expect(live).toHaveText("Showing all 24 rows.");
  });

  test("P3: keyboard only, a toggled row stays in place with focus", async ({ page }) => {
    await setupUnits(page);
    await mockSequence(
      page,
      "/api/view",
      [VIEW_BODIES["kitchen-sink"], AFTER_EXCLUDE_TMP_MOUNT],
      { triggerOn: "/api/op" },
    );
    await page.goto("/");
    const focusedTestId = () =>
      page.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? null);

    // Open the section from its sidebar entry with Enter. Entering a section
    // focuses its first row (App's section-entry focus), which sits below
    // the bar, so the bar is reached with Shift+Tab.
    const entry = sidebarEntry(page, /^Systemd units /);
    await entry.focus();
    await page.keyboard.press("Enter");
    const section = page.getByTestId("service-section");
    await expect(page.getByTestId("default-target")).toBeFocused();

    const followUp = filterButtons(page).filter({ hasText: "Needs follow-up" });
    const isFollowUpFocused = () => followUp.evaluate((el) => el === document.activeElement);
    for (let i = 0; i < MAX_KEY_PRESSES && !(await isFollowUpFocused()); i++) {
      await page.keyboard.press("Shift+Tab");
    }
    await expect(followUp).toBeFocused();
    await page.keyboard.press("Space");
    await expect(followUp).toHaveAttribute("aria-pressed", "true");

    // Tab past the remaining bar buttons and the group headers into the rows.
    const isRowFocused = () =>
      page.evaluate(() => document.activeElement?.getAttribute("role") === "row");
    for (let i = 0; i < MAX_KEY_PRESSES && !(await isRowFocused()); i++) {
      await page.keyboard.press("Tab");
    }
    expect(await isRowFocused()).toBe(true);

    const tmpMount = page.getByTestId("service-item-tmp.mount");
    for (let i = 0; i < MAX_KEY_PRESSES && (await focusedTestId()) !== "service-item-tmp.mount"; i++) {
      await page.keyboard.press("j");
    }
    await expect(tmpMount).toBeFocused();

    const shownRowIds = () =>
      section
        .locator('[role="row"]:visible')
        .evaluateAll((els) => els.map((el) => el.getAttribute("data-testid")));
    const before = await shownRowIds();
    const at = before.indexOf("service-item-tmp.mount");
    const neighbours = [before[at - 1], before[at + 1]];

    const opSent = page.waitForRequest((r) => r.url().endsWith("/api/op") && r.method() === "POST");
    await page.keyboard.press("x");
    await opSent;

    // The post-exclude body's counts, by the same rules the bar uses.
    const afterBody = JSON.parse(fs.readFileSync(AFTER_EXCLUDE_TMP_MOUNT, "utf-8")) as UnitFields;
    const counts = outcomeCounts(keyFields(afterBody));
    await expect
      .poll(() => filterTexts(page))
      .toEqual(FILTER_VALUES.map((value) => `${FILTER_LABELS[value]} ${counts[value]}`));
    expect(counts).not.toEqual(outcomeCounts(keyFields(readView("kitchen-sink"))));

    await expect(tmpMount).toBeFocused();
    await expect(tmpMount.getByRole("checkbox")).not.toBeChecked();
    const after = await shownRowIds();
    const atAfter = after.indexOf("service-item-tmp.mount");
    expect(atAfter).toBe(at);
    expect([after[atAfter - 1], after[atAfter + 1]]).toEqual(neighbours);
  });

  test("P4: a global search result for a unit lands on its row", async ({ page }) => {
    await setupUnits(page);
    await page.goto("/");
    await expect(sidebarEntry(page, /^Systemd units /)).toBeVisible();

    await page.locator('[data-testid="global-search-input"] input').fill("wg0");
    const results = page.getByRole("listbox", { name: "Search results" }).getByRole("option");
    const unitResult = results.filter({ hasText: "wg-quick@wg0.service" });
    await expect(unitResult).toHaveCount(1);
    await expect(unitResult).toContainText("Systemd units");
    // The units reference section is not indexed, so nothing reads "Services".
    await expect(results.filter({ hasText: /^Services/ })).toHaveCount(0);

    await unitResult.click();
    await expect(page.getByTestId("service-item-wg-quick@wg0.service")).toBeFocused();
  });

  test("P5: a Scheduled Tasks timer row reaches its unit row", async ({ page }) => {
    // The recorded sections body must hold a timer with a unit row, or
    // this test would pass on nothing.
    const sections = JSON.parse(fs.readFileSync(SECTIONS_BODY, "utf-8")) as {
      id: string;
      items: { id: string; unit?: string; unit_row?: boolean }[];
    }[];
    const timer = sections
      .find((s) => s.id === "scheduled_tasks")
      ?.items.find((item) => item.unit_row === true);
    expect(timer, "a scheduled_tasks timer row with unit_row: true").toBeDefined();

    await setupUnits(page);
    await page.goto("/");
    await sidebarEntry(page, /^Scheduled Tasks /).click();
    await page.getByTestId(`context-item-unit-${timer!.id}`).click();

    await expect(page.getByTestId("service-section")).toBeVisible();
    await expect(page.getByTestId(`service-item-${timer!.unit}`)).toBeFocused();
  });
  test("P6: no critical or serious axe violations in the section", async ({ page }) => {
    await setupUnits(page);
    await openUnits(page);
    await expectNoBlockingViolations(page, [SECTION_SELECTOR]);

    await filterButtons(page).filter({ hasText: "Needs follow-up" }).click();
    await expect(shownCountedRows(page)).toHaveCount(9);
    await expectNoBlockingViolations(page, [SECTION_SELECTOR]);
  });

  test("P6: no critical or serious axe violations on a target-only host", async ({ page }) => {
    // This body keeps the kitchen-sink Containerfile preview and stats, so
    // only the units section is read.
    await setupUnits(page, "stock10");
    const section = await openUnits(page);
    await expect(page.getByTestId("default-target")).toBeVisible();
    await expect(section.getByRole("group", { name: "Show" })).toHaveCount(0);
    await expectNoBlockingViolations(page, [SECTION_SELECTOR]);
  });

  test("P7: no horizontal scroll at 768 x 1024, and the bar on one line at 1280 x 800", async ({
    page,
  }) => {
    await setupUnits(page);
    await page.setViewportSize(DESKTOP);
    await openUnits(page);
    expect(await barLineCount(page)).toBe(1);
    expect(await pageScrollsSideways(page)).toBe(false);

    await page.setViewportSize(NARROW);
    await expect.poll(() => pageScrollsSideways(page)).toBe(false);
    expect(await barButtonsInsideSection(page)).toBe(true);
    // The spec's "the bar wraps at 768" is not asserted: in the recorded
    // view the bar fits on one line at this width. Measured: the section is
    // 700 px wide at 768 x 1024 (660 px at 1280 x 800, beside the sidebar
    // and Containerfile panel) and the bar's buttons span 425 px.
  });

  test("P7: a long unit name and reason cause no horizontal scroll at 768 x 1024", async ({
    page,
  }) => {
    await setupUnits(page);
    const view = JSON.parse(fs.readFileSync(VIEW_BODIES["kitchen-sink"], "utf-8"));
    const row = view.unit_states.find((u: { declined: unknown }) => u.declined);
    row.unit = LONG_UNIT;
    row.declined.text = LONG_DECLINED_TEXT;
    // Registered last, so it answers before the preset route.
    await page.route("**/api/view", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(view) }),
    );
    await page.setViewportSize(DESKTOP);
    await openUnits(page);
    await page.setViewportSize(NARROW);

    const longRow = page.getByTestId(`service-item-${LONG_UNIT}`);
    await expect(longRow).toBeVisible();
    await expect(longRow).toContainText(LONG_DECLINED_TEXT);
    await expect.poll(() => pageScrollsSideways(page)).toBe(false);
  });

  test("P8: dark theme keeps contrast on outcome labels, the follow-up flag and the preset", async ({
    page,
  }) => {
    // The theme main.tsx applies at load from the stored choice.
    await page.addInitScript(() => localStorage.setItem("inspectah-theme", "dark"));
    await setupUnits(page);
    await openUnits(page);
    await expect(page.locator("html")).toHaveClass(/pf-v6-theme-dark/);

    const parts = {
      outcomeLabels: `${SECTION_SELECTOR} [role="row"] .inspectah-decision-row__badge .pf-v6-c-label`,
      followUpFlag: `${SECTION_SELECTOR} .inspectah-unit-group > button .pf-v6-c-label`,
      presetText: `${SECTION_SELECTOR} [data-testid^="service-state-"]`,
    };
    for (const selector of Object.values(parts)) {
      await expect(page.locator(selector).first()).toBeVisible();
    }
    const results = await expectNoBlockingViolations(page, Object.values(parts), ["color-contrast"]);
    // Axe checked those nodes rather than finding nothing to check.
    expect(results.passes.find((r) => r.id === "color-contrast")?.nodes.length ?? 0).toBeGreaterThan(0);
  });
});
