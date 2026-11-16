import { test, expect, type Page } from "@playwright/test";
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

  test("P2: Needs follow-up leaves 9 rows, All restores 23", async ({ page }) => {
    await setupUnits(page);
    const section = await openUnits(page);
    const live = section.locator('[aria-live="polite"]');

    await filterButtons(page).filter({ hasText: "Needs follow-up" }).click();
    await expect(shownCountedRows(page)).toHaveCount(9);
    expect(
      (await groupHeaders(page)).map((name) => name.replace(/ \d.*$/, "")),
    ).toEqual(["Services", "Other units", "Reported, not carried"]);
    await expect(live).toHaveText("Showing 9 rows that need follow-up.");

    await filterButtons(page).filter({ hasText: "All" }).click();
    await expect(shownCountedRows(page)).toHaveCount(23);
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
});
