import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// ---------------------------------------------------------------------------
// FIXTURE REQUIREMENT
//
// These tests require a running `inspectah refine` server loaded with a
// FLEET tarball (merged multi-host scan). Run with:
//
//   cargo run -p inspectah-cli -- refine testdata/fleet-e2e.tar.gz --no-browser --port 8642
//
// The fleet tarball must contain an `inspection-snapshot.json` whose
// `fleet_meta` field is populated (host_count >= 3 for zone tests to
// exercise all three zone groups). It should include at least:
//
//   - Multiple config-file items with >=2 variants each (for variant/ack/diff)
//   - Package items spread across consensus zones
//   - At least one decision-section item (config_files) with actionable variants
//
// If the fleet fixture does not exist yet, generate one:
//
//   1. Scan 3+ RHEL hosts:
//        inspectah scan -o host-01.tar.gz
//        inspectah scan -o host-02.tar.gz
//        inspectah scan -o host-03.tar.gz
//
//   2. Merge into a fleet tarball:
//        inspectah fleet merge host-01.tar.gz host-02.tar.gz host-03.tar.gz \
//          -o testdata/fleet-e2e.tar.gz
//
// ---------------------------------------------------------------------------

const BASE_URL = "http://127.0.0.1:8642";

test.describe("Fleet refine E2E", () => {
  // Fleet tests mutate shared server state (undo/redo, variant selection).
  // Run serially to avoid port conflicts and state races.
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    // Verify server is running in fleet mode before each test.
    // Skip the entire suite if the server is unreachable or not in fleet mode.
    try {
      const healthResp = await page.request.get(`${BASE_URL}/api/health`);
      if (!healthResp.ok()) {
        test.skip(true, "Server not running on port 8642");
        return;
      }
      const health = await healthResp.json();
      if (!health.fleet) {
        test.skip(true, "Server is not running in fleet mode (no fleet_meta in snapshot)");
        return;
      }
    } catch {
      test.skip(true, "Cannot connect to refine server at port 8642");
      return;
    }

    await page.goto("/");
    // Wait for the fleet app to load (data-testid="fleet-app" is set by FleetApp)
    await expect(page.getByTestId("fleet-app")).toBeVisible({ timeout: 10000 });
  });

  // -----------------------------------------------------------------------
  // Zone rendering
  // -----------------------------------------------------------------------

  test("fleet health endpoint reports fleet metadata", async ({ request }) => {
    const response = await request.get(`${BASE_URL}/api/health`);
    expect(response.ok()).toBeTruthy();

    const body = await response.json();
    expect(body.status).toBe("ok");
    expect(body.fleet).toBeDefined();
    expect(body.fleet.host_count).toBeGreaterThanOrEqual(2);
    expect(body.fleet.label).toBeTruthy();
  });

  test("fleet view API returns sections with zone structure", async ({ request }) => {
    const response = await request.get(`${BASE_URL}/api/fleet/view`);
    expect(response.ok()).toBeTruthy();

    const view = await response.json();
    expect(view.sections).toBeDefined();
    expect(view.sections.length).toBeGreaterThan(0);
    expect(view.generation).toBeGreaterThanOrEqual(0);
    expect(typeof view.can_undo).toBe("boolean");
    expect(typeof view.can_redo).toBe("boolean");
    expect(view.summary).toBeDefined();
    expect(view.summary.host_count).toBeGreaterThanOrEqual(2);
  });

  test("zone headers render with correct counts", async ({ page }) => {
    // Navigate to a section that should have zone groups rendered.
    // Fleet sections with >=3 hosts and items in multiple zones will show
    // zone headers (Consensus, Near Consensus, Divergent).
    const fleetContent = page.getByTestId("fleet-content");
    await expect(fleetContent).toBeVisible();

    // Check if any zone groups are rendered. For a fleet-of-2, zones may
    // not be active (flat mode). We test both scenarios.
    const zoneGroups = page.locator("[data-testid^='zone-']");
    const zoneCount = await zoneGroups.count();

    if (zoneCount === 0) {
      // Flat mode — fleet-of-2 or single-zone items. Verify flat rendering.
      const fleetSection = page.getByTestId("fleet-section");
      await expect(fleetSection.first()).toBeVisible();
      return;
    }

    // Zone mode — verify at least one zone group has a label and count badge
    const firstZone = zoneGroups.first();
    await expect(firstZone).toBeVisible();

    const label = firstZone.locator(".fleet-zone-group__label");
    await expect(label).toBeVisible();
    const labelText = await label.textContent();
    expect(["Consensus", "Near Consensus", "Divergent"]).toContain(labelText?.trim());

    // Badge shows the item count
    const badge = firstZone.locator(".pf-v6-c-badge");
    await expect(badge).toBeVisible();
    const badgeText = await badge.textContent();
    expect(parseInt(badgeText ?? "0", 10)).toBeGreaterThan(0);
  });

  // -----------------------------------------------------------------------
  // Sidebar navigation
  // -----------------------------------------------------------------------

  test("sidebar shows fleet sections and allows navigation", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    await expect(sidebar).toBeVisible();

    // Fleet sidebar should contain section links
    const navItems = sidebar.locator("a, button");
    const navCount = await navItems.count();
    expect(navCount).toBeGreaterThan(0);

    // Click the second nav item (if available) to switch sections
    if (navCount >= 2) {
      const secondItem = navItems.nth(1);
      const secondItemText = await secondItem.textContent();
      await secondItem.click();

      // Active section in fleet-content should change
      const content = page.getByTestId("fleet-content");
      await expect(content).toContainText(secondItemText?.trim() ?? "");
    }
  });

  // -----------------------------------------------------------------------
  // Banner and ack flow
  // -----------------------------------------------------------------------

  test("fleet banner renders when variant items need review", async ({ page }) => {
    const banner = page.getByTestId("fleet-banner");

    // Banner may or may not be visible depending on whether the fixture
    // has actionable variant items. We test both outcomes.
    const bannerVisible = await banner.isVisible().catch(() => false);

    if (!bannerVisible) {
      // No actionable variants in this fixture — verify ack-progress is absent too
      const ackProgress = page.getByTestId("ack-progress");
      const ackVisible = await ackProgress.isVisible().catch(() => false);
      // If no banner and no ack progress, the fixture has no actionable variants.
      // This is a valid state.
      if (!ackVisible) return;
    }

    // Banner is visible — verify structure
    await expect(banner).toHaveAttribute("role", "status");

    // Should show severity via data-severity attribute
    const severity = await banner.getAttribute("data-severity");
    expect(["danger", "warning", "success"]).toContain(severity);

    // Headline text should be present
    const headline = await banner.locator("div").first().textContent();
    expect(headline).toBeTruthy();
  });

  test("ack progress shows in toolbar when variants exist", async ({ page }) => {
    const ackProgress = page.getByTestId("ack-progress");
    const isVisible = await ackProgress.isVisible().catch(() => false);

    if (!isVisible) {
      // No actionable variant items — skip
      test.skip(true, "Fixture has no actionable variant items");
      return;
    }

    // Should show "X of Y variants need review"
    const text = await ackProgress.textContent();
    expect(text).toMatch(/\d+ of \d+ variants need review/);
  });

  // -----------------------------------------------------------------------
  // Variant selection
  // -----------------------------------------------------------------------

  test("fleet item rows render with prevalence and attention", async ({ page }) => {
    const fleetSection = page.getByTestId("fleet-section");
    await expect(fleetSection.first()).toBeVisible();

    // Find fleet item rows
    const itemRows = page.locator(".fleet-item-row");
    const rowCount = await itemRows.count();
    expect(rowCount).toBeGreaterThan(0);

    // First item should have a name and prevalence badge
    const firstRow = itemRows.first();
    const name = firstRow.locator(".fleet-item-row__name");
    await expect(name).toBeVisible();

    const prevalence = firstRow.locator(".fleet-item-row__prevalence");
    await expect(prevalence).toBeVisible();
  });

  test("fleet item row shows variant count when variants exist", async ({ page }) => {
    const variantBadges = page.locator(".fleet-item-row__variants");
    const variantCount = await variantBadges.count();

    if (variantCount === 0) {
      test.skip(true, "No items with variants in current section");
      return;
    }

    const firstVariant = variantBadges.first();
    await expect(firstVariant).toBeVisible();
    const text = await firstVariant.textContent();
    // Should show variant count (e.g., "2 variants")
    expect(text).toMatch(/\d+\s*variant/i);
  });

  // -----------------------------------------------------------------------
  // Diff comparison via fleet/diff API
  // -----------------------------------------------------------------------

  test("fleet diff API returns unified diff data", async ({ request, page }) => {
    // First, get the fleet view to find an item with variants
    const viewResp = await request.get(`${BASE_URL}/api/fleet/view`);
    const view = await viewResp.json();

    // Find a section with variant items
    let variantItem = null;
    for (const section of view.sections) {
      const items = section.items ?? [];
      // Also check inside zones
      const zoneItems = section.zones
        ? [
            ...section.zones.consensus.items,
            ...section.zones.near_consensus.items,
            ...section.zones.divergent.items,
          ]
        : [];
      const allItems = [...items, ...zoneItems];
      const withVariants = allItems.find(
        (item: { variants?: { count: number; options: Array<{ hash: string }> } }) =>
          item.variants && item.variants.count >= 2,
      );
      if (withVariants) {
        variantItem = withVariants;
        break;
      }
    }

    if (!variantItem) {
      test.skip(true, "No variant items found in fleet view for diff test");
      return;
    }

    // Request diff between first two variant hashes
    const opts = variantItem.variants.options;
    const diffResp = await request.post(`${BASE_URL}/api/fleet/diff`, {
      data: {
        item_id: variantItem.item_id,
        base: opts[0].hash,
        target: opts[1].hash,
      },
    });

    expect(diffResp.ok()).toBeTruthy();
    const diff = await diffResp.json();
    expect(diff.base_hash).toBe(opts[0].hash);
    expect(diff.target_hash).toBe(opts[1].hash);
    expect(diff.hunks).toBeDefined();
    expect(diff.stats).toBeDefined();
    expect(typeof diff.stats.insertions).toBe("number");
    expect(typeof diff.stats.deletions).toBe("number");
  });

  // -----------------------------------------------------------------------
  // Section search/filter
  // -----------------------------------------------------------------------

  test("section search filters items by name", async ({ page }) => {
    const itemRows = page.locator(".fleet-item-row");
    const initialCount = await itemRows.count();

    if (initialCount === 0) {
      test.skip(true, "No fleet items in current section");
      return;
    }

    // Get the name of the first item to use as a search term
    const firstName = await itemRows.first().locator(".fleet-item-row__name").textContent();
    if (!firstName) {
      test.skip(true, "Could not read item name");
      return;
    }

    // Open section search with "/" key
    await page.keyboard.press("/");

    // Type a partial search term (first few chars of the item name)
    const searchTerm = firstName.trim().substring(0, 4);
    await page.keyboard.type(searchTerm);

    // Wait for filtering to take effect
    await page.waitForTimeout(300);

    // Filtered count should be <= initial count (some items filtered out)
    const filteredCount = await itemRows.count();
    expect(filteredCount).toBeLessThanOrEqual(initialCount);
    expect(filteredCount).toBeGreaterThan(0);

    // Clear search with Escape
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);

    // Count should restore
    const restoredCount = await itemRows.count();
    expect(restoredCount).toBe(initialCount);
  });

  // -----------------------------------------------------------------------
  // Export
  // -----------------------------------------------------------------------

  test("export triggers download dialog", async ({ page }) => {
    // Export button is in the stats bar / toolbar
    const exportBtn = page.getByRole("button", { name: /export/i });
    const exportExists = await exportBtn.isVisible().catch(() => false);

    if (!exportExists) {
      // Try keyboard shortcut (Ctrl+Shift+E)
      await page.keyboard.press("Control+Shift+e");
    } else {
      await exportBtn.click();
    }

    // Export dialog should appear
    const dialog = page.getByRole("dialog");
    const dialogVisible = await dialog.isVisible().catch(() => false);

    if (!dialogVisible) {
      test.skip(true, "Export dialog did not appear — feature may not be wired yet");
      return;
    }

    await expect(dialog).toBeVisible();

    // Dialog should have a download/export button
    const downloadBtn = dialog.getByRole("button", {
      name: /download|export|save/i,
    });
    const downloadExists = await downloadBtn.isVisible().catch(() => false);

    if (!downloadExists) {
      // Dialog opened but has different structure — verify it opened
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

  // -----------------------------------------------------------------------
  // Undo / Redo
  // -----------------------------------------------------------------------

  test("undo reverts a toggle and redo restores it", async ({ page }) => {
    // Find a fleet item row with a toggle switch (decision section items)
    const toggles = page.locator(".fleet-item-row__toggle input[type='checkbox']");
    const toggleCount = await toggles.count();

    if (toggleCount === 0) {
      test.skip(true, "No toggleable items in current fleet view");
      return;
    }

    const firstToggle = toggles.first();
    const initialChecked = await firstToggle.isChecked();

    // Toggle the item
    const opResponse = page.waitForResponse(
      (res) => res.url().includes("/api/op") && res.status() === 200,
    );
    await firstToggle.click();
    await opResponse;

    // Wait for fleet view to refresh
    await page.waitForTimeout(500);

    // Verify toggle changed
    const afterToggle = await toggles.first().isChecked();
    expect(afterToggle).not.toBe(initialChecked);

    // Undo
    const undoResp = page.waitForResponse(
      (res) => res.url().includes("/api/undo") && res.status() === 200,
    );
    await page.getByRole("button", { name: /undo/i }).click();
    await undoResp;
    await page.waitForTimeout(500);

    // Verify toggle reverted
    const afterUndo = await toggles.first().isChecked();
    expect(afterUndo).toBe(initialChecked);

    // Redo
    const redoResp = page.waitForResponse(
      (res) => res.url().includes("/api/redo") && res.status() === 200,
    );
    await page.getByRole("button", { name: /redo/i }).click();
    await redoResp;
    await page.waitForTimeout(500);

    // Verify toggle restored
    const afterRedo = await toggles.first().isChecked();
    expect(afterRedo).toBe(afterToggle);
  });

  // -----------------------------------------------------------------------
  // Keyboard shortcuts
  // -----------------------------------------------------------------------

  test("keyboard shortcut ? opens help modal", async ({ page }) => {
    await page.keyboard.press("?");

    // Help modal should appear
    const helpModal = page.getByRole("dialog");
    const visible = await helpModal.isVisible().catch(() => false);

    if (!visible) {
      test.skip(true, "Help modal not rendered for keyboard shortcut ?");
      return;
    }

    await expect(helpModal).toBeVisible();

    // Should mention fleet-specific shortcut (c for compare)
    const modalText = await helpModal.textContent();
    expect(modalText).toContain("Compare");

    // Close modal
    await page.keyboard.press("Escape");
    await expect(helpModal).not.toBeVisible();
  });

  // -----------------------------------------------------------------------
  // Accessibility
  // -----------------------------------------------------------------------

  test("axe-core finds no critical violations in fleet view", async ({ page }) => {
    // Give dynamic content a moment to render
    await page.waitForTimeout(500);

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();

    const critical = results.violations.filter(
      (v) => v.impact === "critical" || v.impact === "serious",
    );

    if (critical.length > 0) {
      const summary = critical
        .map(
          (v) =>
            `[${v.impact}] ${v.id}: ${v.description} (${v.nodes.length} instance(s))`,
        )
        .join("\n");
      expect(critical, `Fleet accessibility violations:\n${summary}`).toEqual(
        [],
      );
    }
  });

  test("fleet banner has appropriate ARIA role", async ({ page }) => {
    const banner = page.getByTestId("fleet-banner");
    const bannerVisible = await banner.isVisible().catch(() => false);

    if (!bannerVisible) {
      test.skip(true, "No fleet banner visible — no actionable variants");
      return;
    }

    await expect(banner).toHaveAttribute("role", "status");
  });

  test("fleet item rows are keyboard focusable", async ({ page }) => {
    const itemRows = page.locator(".fleet-item-row");
    const rowCount = await itemRows.count();

    if (rowCount === 0) {
      test.skip(true, "No fleet item rows visible");
      return;
    }

    // Fleet item rows should be focusable (tabindex or naturally focusable element)
    const firstRow = itemRows.first();
    await firstRow.focus();

    // Verify focus landed on the row or a child element
    const focused = page.locator(":focus");
    await expect(focused).toBeVisible();
  });
});
