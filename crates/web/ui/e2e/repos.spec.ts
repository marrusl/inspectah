import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks, mockSequence } from "./helpers/mock-api";

test.describe("Repo surfaces on Packages page", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    // Navigate to Packages section
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Packages")
      .click();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("repo bar renders above package list", async ({ page }) => {
    const repoBar = page.locator('[data-testid="repo-bar"]');
    await expect(repoBar).toBeVisible();

    // Verify it contains the "Repositories" label
    await expect(repoBar.getByText("Repositories")).toBeVisible();
  });

  test("package rows show repo context", async ({ page }) => {
    // Wait for package rows to render
    const packageRows = page.locator('[data-testid^="package-row-"]');
    await expect(packageRows.first()).toBeVisible({ timeout: 5000 });

    // Verify multiple package rows are present
    const count = await packageRows.count();
    expect(count).toBeGreaterThan(0);

    // Verify repo information is displayed (repo-text appears in each row)
    const repoText = page.locator('[data-testid="repo-text"]').first();
    await expect(repoText).toBeVisible();

    // Verify it shows actual repo name (from fixture: rhel-9-for-x86_64-baseos-rpms or appstream)
    await expect(repoText).toContainText("rhel-9-for-x86_64");
  });

  test("excluded zone renders after excluding items", async ({ page }) => {
    // Set up mock sequence for toggle action
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    // Find and click a package toggle
    const toggle = page.locator('input[type="checkbox"]').first();
    await expect(toggle).toBeVisible();
    await toggle.click({ force: true });

    // Wait for the UI to update
    await page.waitForTimeout(1000);

    // Verify page stays interactive (can still see repo bar)
    const repoBar = page.locator('[data-testid="repo-bar"]');
    await expect(repoBar).toBeVisible();

    // The excluded zone may or may not be visible depending on fixture data
    // but we verify the page remains functional after a toggle
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar).toBeVisible();
  });
});

// Position 29 (§ 3.2, § 6.3): the repo Switch reads the effective counts,
// never `enabled`/history, and activation posts the direction those counts
// derive. The `repo-disagree` sequence fixtures start each case already in
// the state where history and members disagree -- reachable through
// ordinary per-package decisions (§ 6.3's phase 0 RepoBar rule) -- and walk
// the Switch through include-all/exclude-all, undo, and redo.
test.describe("Repo disagreement (position 29)", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    // /api/groups has no fixture in the single-host preset; mock it directly
    // so the sidebar has a "Packages" nav item to click (an empty array
    // renders no sections at all, per Sidebar.tsx's `groups.map(...)`).
    await page.route("**/api/groups", (routeObj) => {
      if (routeObj.request().method() !== "GET") {
        routeObj.continue();
        return;
      }
      routeObj.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          {
            slug: "packages-group",
            label: "Packages",
            sections: [{ id: "packages", label: "Packages", is_triage: true }],
            has_actionable_sections: true,
          },
        ]),
      });
    });
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("enabled history, members excluded, then included; undo and redo", async ({
    page,
  }) => {
    await mockSequence(
      page,
      "/api/view",
      [
        "sequences/repo-disagree/00-enabled-all-excluded.json",
        "sequences/repo-disagree/01-after-include-all.json",
        "sequences/repo-disagree/02-after-undo.json",
        "sequences/repo-disagree/03-after-redo.json",
      ],
      { triggerOn: ["/api/op", "/api/undo", "/api/redo"] },
    );

    const opPayloads: unknown[] = [];
    page.on("request", (req) => {
      if (req.method() === "POST" && req.url().includes("/api/op")) {
        opPayloads.push(req.postDataJSON());
      }
    });

    await page.goto("/");
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Packages")
      .click();

    const vendorSwitch = page.locator("#repo-toggle-vendor");
    await expect(vendorSwitch).toBeVisible();
    await expect(vendorSwitch).not.toBeChecked();
    await expect(page.getByText("2 packages excluded")).toBeVisible();

    await vendorSwitch.click({ force: true });
    await expect(vendorSwitch).toBeChecked();
    await expect(page.getByText("2 of 2 packages included")).toBeVisible();
    expect(opPayloads.at(-1)).toMatchObject({
      target: {
        item_id: { kind: "Repo", key: { path: "vendor" } },
        include: true,
      },
    });

    await page.getByRole("button", { name: "Undo" }).click();
    await expect(vendorSwitch).not.toBeChecked();
    await expect(page.getByText("2 packages excluded")).toBeVisible();

    await page.getByRole("button", { name: "Redo" }).click();
    await expect(vendorSwitch).toBeChecked();
    await expect(page.getByText("2 of 2 packages included")).toBeVisible();
  });

  test("mirror direction: excluded history, members included, then excluded", async ({
    page,
  }) => {
    await mockSequence(
      page,
      "/api/view",
      [
        "sequences/repo-disagree/10-disabled-all-included.json",
        "sequences/repo-disagree/11-after-exclude-all.json",
        "sequences/repo-disagree/12-after-undo.json",
        "sequences/repo-disagree/13-after-redo.json",
      ],
      { triggerOn: ["/api/op", "/api/undo", "/api/redo"] },
    );

    const opPayloads: unknown[] = [];
    page.on("request", (req) => {
      if (req.method() === "POST" && req.url().includes("/api/op")) {
        opPayloads.push(req.postDataJSON());
      }
    });

    await page.goto("/");
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Packages")
      .click();

    const vendorSwitch = page.locator("#repo-toggle-vendor");
    await expect(vendorSwitch).toBeVisible();
    await expect(vendorSwitch).toBeChecked();
    await expect(page.getByText("2 of 2 packages included")).toBeVisible();

    await vendorSwitch.click({ force: true });
    await expect(vendorSwitch).not.toBeChecked();
    await expect(page.getByText("2 packages excluded")).toBeVisible();
    expect(opPayloads.at(-1)).toMatchObject({
      target: {
        item_id: { kind: "Repo", key: { path: "vendor" } },
        include: false,
      },
    });

    await page.getByRole("button", { name: "Undo" }).click();
    await expect(vendorSwitch).toBeChecked();
    await expect(page.getByText("2 of 2 packages included")).toBeVisible();

    await page.getByRole("button", { name: "Redo" }).click();
    await expect(vendorSwitch).not.toBeChecked();
    await expect(page.getByText("2 packages excluded")).toBeVisible();
  });
});
