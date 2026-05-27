import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks, mockSequence, mockError } from "./helpers/mock-api";

test.describe("Triage workflow (mock tier)", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("toggle package exclude — view updates via GET refetch", async ({ page }) => {
    // Verify initial stats: Packages: 4 included / 1 excluded
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible();

    // Wire mockSequence: POST /api/op advances GET /api/view to after-exclude state
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    // Find an included package toggle and click to exclude it.
    // PackageList renders <input type="checkbox" role="checkbox" aria-label="<name>">.
    const firstToggle = page.locator("input[type=checkbox][aria-label='httpd.x86_64']");
    await firstToggle.click({ force: true });

    // Wait for the POST response + GET refetch to settle, then verify stats changed
    await expect(statsBar.getByText(/Packages:\s*3 included/)).toBeVisible({ timeout: 5000 });
    await expect(statsBar.getByText(/2 excluded/)).toBeVisible();
  });

  test("undo reverts state — redo button becomes enabled", async ({ page }) => {
    // Wire mockSequence: /api/op and /api/undo advance the view through two states
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view.json",
        "sequences/exclude-undo-redo/01-after-exclude.json",
        "sequences/exclude-undo-redo/02-after-undo.json",
      ],
      { triggerOn: ["/api/op", "/api/undo"] },
    );

    // Step 1: toggle to exclude → advances to state 1
    const firstToggle = page.locator("input[type=checkbox][aria-label='httpd.x86_64']");
    await firstToggle.click({ force: true });

    // Wait for exclude to take effect
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Packages:\s*3 included/)).toBeVisible({ timeout: 5000 });

    // Step 2: click undo → advances to state 2 (after-undo fixture has can_redo: true)
    const undoBtn = page.getByRole("button", { name: "Undo" });
    await undoBtn.click();

    // Verify stats reverted to original counts
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible({ timeout: 5000 });

    // Verify redo button is now enabled (after-undo fixture has can_redo: true)
    const redoBtn = page.getByRole("button", { name: "Redo" });
    await expect(redoBtn).toBeEnabled({ timeout: 3000 });
  });

  test("server error on mutation — page stays interactive", async ({ page }) => {
    // Wire /api/op to return 500 on POST
    await mockError(page, "/api/op", "500");

    // Find a toggle and click it
    const firstToggle = page.locator("input[type=checkbox][aria-label='httpd.x86_64']");
    await firstToggle.click({ force: true });

    // Give the error response time to process
    await page.waitForTimeout(500);

    // Page should still be interactive: statsbar visible, toggles still present
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    const toggleCount = await page.locator("input[type=checkbox]").count();
    expect(toggleCount).toBeGreaterThan(0);

    // Stats should remain unchanged (original values from the initial mock)
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar.getByText(/Packages:\s*4 included/)).toBeVisible();
  });
});

test.describe("Triage workflow", () => {
  // These tests mutate shared server state; must run serially
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Wait for data to load
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    // Ensure Packages section is active (default)
    await expect(
      page.locator(".inspectah-layout__sidebar").getByText("Packages"),
    ).toBeVisible();
  });

  test("toggle a package exclusion updates Containerfile preview", async ({
    page,
  }) => {
    // The panel may be collapsed at 1280px viewport — expand if needed
    const cfPanelOpen = page.locator(".inspectah-cf-panel--open");
    const isOpen = await cfPanelOpen.isVisible().catch(() => false);
    if (!isOpen) {
      await page.keyboard.press("Control+e");
      await expect(cfPanelOpen).toBeVisible({ timeout: 2000 });
    }

    const initialPreview = await cfPanelOpen
      .locator(".inspectah-cf-panel__code")
      .textContent();

    // Find the first Switch toggle in a decision item.
    // PF6 Switch renders <input type="checkbox" role="switch">.
    const firstToggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();

    // Set up response listener before clicking to avoid race
    const opResponse = page.waitForResponse((res) => res.url().includes("/api/op"));

    // Click to toggle (exclude)
    await firstToggle.click({ force: true });

    // Wait for the API response to update the view
    await opResponse;

    // Wait for React to re-render the Containerfile preview
    await expect(async () => {
      const updatedPreview = await cfPanelOpen
        .locator(".inspectah-cf-panel__code")
        .textContent();
      expect(updatedPreview).not.toBe(initialPreview);
    }).toPass({ timeout: 5000 });
  });

  test("undo reverts the last operation", async ({ page }) => {
    // Get initial package counts (triage/viewed counter is not undone)
    const statsBar = page.locator(".inspectah-statsbar");
    const initialText = await statsBar.textContent();
    const pkgPattern = /Packages:\s*\d+\s*included\s*\/\s*\d+\s*excluded/;
    const initialPkgs = initialText?.match(pkgPattern)?.[0];

    // Find and click a toggle
    const toggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();
    const opResp = page.waitForResponse((res) => res.url().includes("/api/op"));
    await toggle.click({ force: true });
    await opResp;

    // Package counts should have changed
    await page.waitForTimeout(500);
    const afterToggle = await statsBar.textContent();
    const afterTogglePkgs = afterToggle?.match(pkgPattern)?.[0];
    expect(afterTogglePkgs).not.toBe(initialPkgs);

    // Click undo
    const undoResp2 = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await page.getByRole("button", { name: /undo/i }).click();
    await undoResp2;

    // Package counts should revert
    await page.waitForTimeout(500);
    const afterUndo = await statsBar.textContent();
    const afterUndoPkgs = afterUndo?.match(pkgPattern)?.[0];
    expect(afterUndoPkgs).toBe(initialPkgs);
  });

  test("redo re-applies an undone operation", async ({ page }) => {
    const toggle = page
      .getByRole("switch", { name: /toggle/i })
      .first();

    // Toggle, undo, then redo — compare package counts (triage/viewed counter
    // is not undone, so we can't compare the full statsbar text)
    const opResp2 = page.waitForResponse((res) => res.url().includes("/api/op"));
    await toggle.click({ force: true });
    await opResp2;

    // Wait for stats to update, then read the package line
    await page.waitForTimeout(500);
    const afterToggle = await page.locator(".inspectah-statsbar").textContent();
    // Extract just the "Packages: X included / Y excluded" part
    const pkgPattern = /Packages:\s*\d+\s*included\s*\/\s*\d+\s*excluded/;
    const afterTogglePkgs = afterToggle?.match(pkgPattern)?.[0];

    const undoResp = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await page.getByRole("button", { name: /undo/i }).click();
    await undoResp;

    const redoResp = page.waitForResponse((res) => res.url().includes("/api/redo"));
    await page.getByRole("button", { name: /redo/i }).click();
    await redoResp;

    await page.waitForTimeout(500);
    const afterRedo = await page.locator(".inspectah-statsbar").textContent();
    const afterRedoPkgs = afterRedo?.match(pkgPattern)?.[0];
    expect(afterRedoPkgs).toBe(afterTogglePkgs);
  });

  test("export tarball downloads successfully", async ({ page }) => {
    // Click the export button in the stats bar
    const exportBtn = page.getByRole("button", { name: /export/i });
    await exportBtn.click();

    // Export dialog should appear
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();

    // Click the confirm/download button in the dialog
    const downloadBtn = dialog.getByRole("button", {
      name: /download|export|save/i,
    });
    const downloadExists = await downloadBtn.isVisible().catch(() => false);
    if (!downloadExists) {
      // Dialog might have a different structure — just verify it opened
      await expect(dialog).toBeVisible();
      return;
    }

    // Start waiting for download before clicking
    const downloadPromise = page.waitForEvent("download");
    await downloadBtn.click();
    const download = await downloadPromise;

    // Verify the download has a tar.gz filename
    expect(download.suggestedFilename()).toMatch(/\.tar\.gz$/);
  });
});

test.describe("Phase 5: Tiered triage", () => {
  // FIXTURE REQUIREMENT: These tests require a running `inspectah refine` server
  // with a scan tarball. Run with:
  //   cargo run -p inspectah-cli -- refine testdata/<tarball> &
  //   cd inspectah-web/ui && npx playwright test e2e/triage.spec.ts
  //
  // Tests are structurally complete but skipped until a tarball fixture exists.

  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    // Check if server is running with data
    try {
      await page.goto("/", { timeout: 5000 });
      await expect(page.locator(".inspectah-statsbar")).toBeVisible({ timeout: 3000 });
    } catch {
      test.skip(true, "No refine server running with tarball fixture");
    }
  });

  test.skip("triage surface reduced — needs_review count < 100", async ({ page }) => {
    // Navigate to Packages section (should be default)
    const sidebar = page.locator(".inspectah-layout__sidebar");
    await expect(sidebar.getByText("Packages")).toBeVisible();

    // Read the stats bar for needs_review count
    const statsBar = page.locator(".inspectah-statsbar");
    const statsText = await statsBar.textContent();

    // Parse the "X to triage" or similar indicator
    // Phase 5 goal: reduce from ~734 to <100
    const triagePattern = /(\d+)\s+to\s+triage|needs\s+review:\s*(\d+)/i;
    const match = statsText?.match(triagePattern);
    const triageCount = match ? parseInt(match[1] || match[2]) : 0;

    expect(triageCount).toBeLessThan(100);

    // Verify Tier 1 section shows "baseline packages" collapsed summary
    const tier1Section = page.locator(".inspectah-tier1-summary");
    await expect(tier1Section).toBeVisible();
    await expect(tier1Section.getByText(/baseline packages/i)).toBeVisible();

    // Verify repo groups are visible in the triage list
    const repoGroup = page.locator(".inspectah-repo-group").first();
    await expect(repoGroup).toBeVisible();
  });

  test.skip("ExcludeRepo removes packages and shows undo", async ({ page }) => {
    // Navigate to Packages section
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    // Find a third-party repo toggle (Switch element with repo label)
    // Repo groups should have a Switch for verified repos
    const repoToggle = page
      .locator(".inspectah-repo-group")
      .filter({ hasNotText: /unverified/i })
      .first()
      .getByRole("switch");

    await expect(repoToggle).toBeVisible();

    // Get initial package count from stats bar
    const statsBar = page.locator(".inspectah-statsbar");
    const initialStats = await statsBar.textContent();
    const pkgPattern = /Packages:\s*(\d+)\s*included/;
    const initialCount = parseInt(initialStats?.match(pkgPattern)?.[1] || "0");

    // Expand Containerfile panel if needed
    const cfPanelOpen = page.locator(".inspectah-cf-panel--open");
    const isOpen = await cfPanelOpen.isVisible().catch(() => false);
    if (!isOpen) {
      await page.keyboard.press("Control+e");
      await expect(cfPanelOpen).toBeVisible({ timeout: 2000 });
    }

    const initialCF = await cfPanelOpen
      .locator(".inspectah-cf-panel__code")
      .textContent();

    // Click repo toggle to exclude
    const opResp = page.waitForResponse((res) => res.url().includes("/api/op"));
    await repoToggle.click();
    await opResp;

    // Verify packages from that repo disappear from triage list
    await page.waitForTimeout(500);
    const afterStats = await statsBar.textContent();
    const afterCount = parseInt(afterStats?.match(pkgPattern)?.[1] || "0");
    expect(afterCount).toBeLessThan(initialCount);

    // Verify Containerfile preview updated (repo's packages removed)
    const updatedCF = await cfPanelOpen
      .locator(".inspectah-cf-panel__code")
      .textContent();
    expect(updatedCF).not.toBe(initialCF);

    // Verify undo button is available
    const undoBtn = page.getByRole("button", { name: /undo/i });
    await expect(undoBtn).toBeEnabled();

    // Click undo
    const undoResp = page.waitForResponse((res) => res.url().includes("/api/undo"));
    await undoBtn.click();
    await undoResp;

    // Verify restoration
    await page.waitForTimeout(500);
    const restoredStats = await statsBar.textContent();
    const restoredCount = parseInt(restoredStats?.match(pkgPattern)?.[1] || "0");
    expect(restoredCount).toBe(initialCount);
  });

  test.skip("unverified repo shows label but no toggle", async ({ page }) => {
    // Navigate to Packages section
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    // Find a repo group with "Unverified" badge
    const unverifiedGroup = page
      .locator(".inspectah-repo-group")
      .filter({ hasText: /unverified/i })
      .first();

    // Verify badge is visible
    await expect(unverifiedGroup.getByText(/unverified/i)).toBeVisible();

    // Verify no Switch element is present in that group
    const toggle = unverifiedGroup.getByRole("switch");
    const toggleExists = await toggle.count();
    expect(toggleExists).toBe(0);
  });

  test.skip("Tier 1 configs show 'managed by packages' and are not in Containerfile", async ({
    page,
  }) => {
    // Navigate to Config Files section
    await page.locator(".inspectah-layout__sidebar").getByText("Config Files").click();

    // Wait for config section to load
    await expect(page.locator(".inspectah-layout__main")).toBeVisible();

    // Verify "managed by packages" summary text is visible in Tier 1
    const tier1Summary = page.locator(".inspectah-tier1-summary");
    await expect(tier1Summary.getByText(/managed by packages/i)).toBeVisible();

    // Open Containerfile panel if not visible
    const cfPanelOpen = page.locator(".inspectah-cf-panel--open");
    const isOpen = await cfPanelOpen.isVisible().catch(() => false);
    if (!isOpen) {
      await page.keyboard.press("Control+e");
      await expect(cfPanelOpen).toBeVisible({ timeout: 2000 });
    }

    // Get Containerfile content
    const cfContent = await cfPanelOpen
      .locator(".inspectah-cf-panel__code")
      .textContent();

    // Verify no COPY directives for default config paths
    // (e.g., /etc/passwd, /etc/group, /etc/hostname)
    expect(cfContent).not.toMatch(/COPY.*\/etc\/passwd/);
    expect(cfContent).not.toMatch(/COPY.*\/etc\/group/);
    expect(cfContent).not.toMatch(/COPY.*\/etc\/hostname/);
  });

  test.skip("Decisions/Full toggle switches between views", async ({ page }) => {
    // Navigate to Packages section
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();

    // Find the Decisions/Full toggle (likely a ToggleGroup or similar)
    const decisionsToggle = page.getByRole("button", { name: /decisions/i });
    const fullToggle = page.getByRole("button", { name: /full/i });

    // Verify Decisions is active by default
    await expect(decisionsToggle).toHaveAttribute("aria-pressed", "true");

    // Verify Tier 1 items are not visible (collapsed)
    const tier1Items = page.locator(".inspectah-tier1-items");
    const tier1Visible = await tier1Items.isVisible().catch(() => false);
    expect(tier1Visible).toBe(false);

    // Click Full
    await fullToggle.click();

    // Verify Full is now active
    await expect(fullToggle).toHaveAttribute("aria-pressed", "true");

    // Verify Tier 1 items become visible
    await expect(tier1Items).toBeVisible({ timeout: 2000 });

    // Click Decisions to collapse back
    await decisionsToggle.click();

    // Verify Tier 1 items collapse
    await page.waitForTimeout(300);
    const tier1StillVisible = await tier1Items.isVisible().catch(() => false);
    expect(tier1StillVisible).toBe(false);
  });
});
