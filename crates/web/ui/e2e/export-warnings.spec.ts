import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

const FIXTURES_DIR = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "fixtures",
);

// The export's warnings travel in the x-export-warnings response header
// (process-docs/skills/refine-export-warnings-header.md), so the tarball
// route is fulfilled inline here: mockPostResponse sets no custom headers.
test.describe("Export warnings", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    // /api/groups has no fixture in the single-host preset. Without a
    // route it falls through to the dev proxy and the app shows
    // "Failed to load"; an empty list is enough for the export path.
    await page.route("**/api/groups", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: "[]",
      }),
    );
    // The preset's package and config rows predate the disposition field
    // and crash MainContent (see process-docs/skills/
    // e2e-fixtures-are-hand-maintained.md). The export path needs only the
    // stats bar, so serve the preset view with its item lists emptied.
    // Registered after applyMockApi, so this route wins.
    const view = JSON.parse(
      fs.readFileSync(
        path.join(FIXTURES_DIR, "single-host", "view.json"),
        "utf-8",
      ),
    );
    await page.route("**/api/view", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ...view,
          packages: [],
          config_files: [],
          repo_groups: [],
          version_changes: [],
          unit_states: [],
          users_groups_decisions: [],
        }),
      }),
    );
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  async function exportWith(
    page: import("@playwright/test").Page,
    header: string,
  ) {
    await page.route("**/api/tarball", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/gzip",
        headers: { "x-export-warnings": header },
        body: Buffer.from([0x1f, 0x8b, 0x08, 0x00]),
      }),
    );
    await page.getByRole("button", { name: /export/i }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible({ timeout: 3000 });
    const downloadPromise = page.waitForEvent("download");
    await dialog.getByRole("button", { name: /export/i }).first().click();
    await downloadPromise;
    return dialog;
  }

  test("downloads and keeps the dialog open listing the warnings", async ({
    page,
  }) => {
    const dialog = await exportWith(
      page,
      JSON.stringify([
        {
          kind: "notice",
          text: "1 package that owns a unit enabled on the source host is not carried: cronie.",
        },
        {
          kind: "failure",
          text: "epel-release.noarch: staged RPM not verified, not exported",
        },
      ]),
    );

    await expect(dialog.getByText("Downloaded, with warnings")).toBeVisible();
    await expect(dialog.locator(".pf-v6-c-alert.pf-m-info")).toContainText(
      "cronie",
    );
    await expect(dialog.locator(".pf-v6-c-alert.pf-m-warning")).toContainText(
      "staged RPM not verified",
    );

    // The footer button; the modal's X is also labelled "Close".
    await dialog
      .getByRole("button", { name: "Close" })
      .filter({ hasText: "Close" })
      .click();
    await expect(dialog).toBeHidden();
  });

  test("downloads and closes when there is nothing to say", async ({
    page,
  }) => {
    const dialog = await exportWith(page, "[]");
    await expect(dialog).toBeHidden();
  });
});
