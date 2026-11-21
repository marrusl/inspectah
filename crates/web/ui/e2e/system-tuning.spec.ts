import { test, expect, type Page } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks, mockSequence } from "./helpers/mock-api";

// System Tuning in a real browser. The sidebar entry is a focusable link
// that Enter activates, and the read-only sysctl panel renders from the
// view, before and after a view refresh.
//
// The bodies are the ones `crates/web/tests/system_tuning_fixtures_test.rs`
// records from the real handlers, not the hand-written preset views, so
// the page renders what the server sends.

const RECORDED = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "__tests__",
  "fixtures",
  "system-tuning",
);

function recorded(name: string): string {
  return path.join(RECORDED, `${name}.json`);
}

/** Serve the `/api/groups` body recorded with this layout's session. */
async function serveGroups(page: Page, layout: string) {
  const body = fs.readFileSync(recorded(`${layout}.groups`), "utf-8");
  await page.route("**/api/groups", (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body }),
  );
}

/** Focus a sidebar link and press Enter, as a keyboard user would. */
async function activateFromKeyboard(page: Page, entry: ReturnType<Page["getByRole"]>) {
  await entry.focus();
  await expect(entry).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(entry).toHaveAttribute("aria-current", "page");
  // The click is handled in place; the hash is never followed.
  expect(new URL(page.url()).hash).toBe("");
}

test.describe("System Tuning", () => {
  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("single host: opens from the keyboard and survives a view refresh", async ({
    page,
  }) => {
    await applyMockApi(page, "single-host", {
      "/api/health": recorded("files-no-tuned.health"),
      "/api/view": recorded("files-no-tuned.view"),
    });
    await serveGroups(page, "files-no-tuned");
    await page.goto("/");

    const nav = page.getByRole("navigation", { name: "Sections" });
    const entry = nav.getByRole("link", { name: /^System Tuning / });
    // Tuned rows only: this host has sysctl files and no tuned selection.
    await expect(entry.locator(".pf-v6-c-badge")).toHaveText("0");
    await activateFromKeyboard(page, entry);

    const main = page.locator(".inspectah-layout__main");
    const heading = main.getByRole("heading", { level: 2, name: "System Tuning" });
    const panel = main.getByRole("region", { name: "Sysctl" });
    const settingsRows = panel
      .getByRole("table", { name: "Sysctl settings" })
      .getByRole("row");
    await expect(heading).toBeVisible();
    await expect(panel).toBeVisible();
    await expect(settingsRows).toHaveCount(3); // header + 2 keys
    await expect(panel.getByText("Runtime drift")).toBeVisible();
    await expect(panel.getByRole("button")).toHaveCount(0);
    await expect(panel.getByRole("checkbox")).toHaveCount(0);

    // A view refresh: undo answers with the next view and the app refetches
    // it. The next view is the recorded mask-only host, so the panel's
    // content changing proves the refreshed view was rendered.
    await mockSequence(
      page,
      "/api/view",
      [recorded("files-no-tuned.view"), recorded("mask-only.view")],
      { triggerOn: "/api/undo" },
    );
    await page.keyboard.press("Control+z");

    await expect(panel.getByText("Vendor files replaced")).toBeVisible({
      timeout: 5000,
    });
    await expect(heading).toBeVisible();
    await expect(settingsRows).toHaveCount(1); // header only
    await expect(panel.getByText("Runtime drift")).toHaveCount(0);
  });

  test("aggregate: opens from the keyboard and shows the aggregate line", async ({
    page,
  }) => {
    await applyMockApi(page, "aggregate-3", {
      "/api/health": recorded("aggregate.health"),
      "/api/aggregate/view": recorded("aggregate.aggregate-view"),
    });
    await page.goto("/");

    const nav = page.getByRole("navigation", { name: "Aggregate sections" });
    const entry = nav.getByRole("link", { name: /^System Tuning / });
    await expect(entry.locator(".pf-v6-c-badge")).toHaveText("0 / 0");
    await activateFromKeyboard(page, entry);

    const panel = page.getByRole("region", { name: "Sysctl" });
    await expect(panel).toBeVisible();
    await expect(
      panel.getByText(
        "Sysctl explanation is per host and is not available across a fleet yet; each host's own scan has it. Sysctl files are carried as Configuration Files, with a variant per differing content.",
      ),
    ).toBeVisible();
    await expect(panel.getByRole("button")).toHaveCount(0);
  });
});
