import { test, expect } from "@playwright/test";
import {
  applyMockApi,
  clearMocks,
  mockSequence,
} from "./helpers/mock-api";
import * as path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const FIXTURES = path.join(__dirname, "fixtures");

/**
 * Apply the single-host preset with group-aware view fixture overridden.
 * This loads the standard routes (health, sections, etc.) from the
 * single-host preset but swaps /api/view for view-with-groups.json.
 */
async function applyGroupMock(page: import("@playwright/test").Page) {
  await applyMockApi(page, "single-host", {
    "/api/view": path.join(
      FIXTURES,
      "single-host",
      "view-with-groups.json",
    ),
  });
}

test.describe("Group rendering", () => {
  test.beforeEach(async ({ page }) => {
    await applyGroupMock(page);
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  // ── 1. Group row expand/collapse ──────────────────────────────────
  test("group row expand/collapse", async ({ page }) => {
    const groupRow = page.getByTestId("group-row-Web Server");
    await expect(groupRow).toBeVisible();

    // Group should start collapsed — members not visible
    const memberList = groupRow.locator('[role="list"]');
    await expect(memberList).not.toBeVisible();

    // Click chevron to expand
    const chevron = groupRow.locator(".inspectah-group-row__chevron");
    await chevron.click();

    // Members should now be visible
    await expect(memberList).toBeVisible();
    await expect(page.getByTestId("group-member-httpd")).toBeVisible();
    await expect(page.getByTestId("group-member-mod_ssl")).toBeVisible();

    // Click chevron again to collapse
    await chevron.click();

    // Members should be hidden again
    await expect(memberList).not.toBeVisible();
  });

  // ── 2. Ungroup converts to individual rows ────────────────────────
  test("ungroup converts to individual rows", async ({ page }) => {
    const groupRow = page.getByTestId("group-row-Web Server");
    await expect(groupRow).toBeVisible();

    // Set up sequence: initial view → after-ungroup (group removed, members individual)
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view-with-groups.json",
        "sequences/group-ungroup/01-after-ungroup.json",
      ],
      { triggerOn: "/api/op" },
    );

    // Click the ungroup button
    const ungroupBtn = groupRow.getByRole("button", { name: /ungroup/i });
    await ungroupBtn.click();

    // Group row should disappear
    await expect(groupRow).not.toBeVisible({ timeout: 5000 });

    // Former member packages should appear as individual rows (name.arch format)
    const individualZone = page.getByTestId("individual-packages-zone");
    await expect(
      individualZone.getByTestId("package-row-httpd.x86_64"),
    ).toBeVisible({ timeout: 5000 });
    await expect(
      individualZone.getByTestId("package-row-mod_ssl.x86_64"),
    ).toBeVisible({ timeout: 5000 });
  });

  // ── 3. Search highlights group member ─────────────────────────────
  test("search highlights group member", async ({ page }) => {
    // Open section search with /
    await page.locator(".inspectah-layout__main").click();
    await page.keyboard.press("/");
    const searchInput = page.locator('[data-testid="section-search"] input');
    await expect(searchInput).toBeVisible({ timeout: 2000 });

    // Type a member name that exists inside the group
    await searchInput.fill("mod_ssl");

    // The group should auto-expand and the member should be visible
    await expect(page.getByTestId("group-member-mod_ssl")).toBeVisible({
      timeout: 5000,
    });

    // The member row should have data-search-match attribute
    const memberRow = page.getByTestId("group-member-mod_ssl");
    await expect(memberRow).toHaveAttribute("data-search-match", "true");
  });

  // ── 4. Optional spillover shows provenance ────────────────────────
  test("optional spillover shows provenance", async ({ page }) => {
    // The fixture has optional_spillover_count: 1 on the "Web Server" group.
    // The summary line should reflect this.
    const summary = page.getByTestId("package-list-summary");
    await expect(summary).toBeVisible();
    await expect(summary).toContainText("1 optional from groups");

    // The spillover package (custom-agent) should appear in the individual zone
    const individualZone = page.getByTestId("individual-packages-zone");
    const customAgentRow = individualZone.getByTestId(
      "package-row-custom-agent.x86_64",
    );
    await expect(customAgentRow).toBeVisible();
  });

  // ── 5. Excluded-group optional spillover remains visible ──────────
  test("excluded-group optional spillover remains visible and toggleable", async ({
    page,
  }) => {
    // Set up sequence: initial view → after excluding the group
    await mockSequence(
      page,
      "/api/view",
      [
        "single-host/view-with-groups.json",
        "sequences/group-exclude/01-after-group-exclude.json",
      ],
      { triggerOn: "/api/op" },
    );

    // Exclude the group by toggling it off
    const groupRow = page.getByTestId("group-row-Web Server");
    const groupToggle = groupRow.locator(
      `#group-toggle-Web\\ Server`,
    );
    await groupToggle.click({ force: true });

    // After excluding the group, the group members (httpd, mod_ssl) become
    // excluded, but the optional spillover package (custom-agent) should
    // remain visible and independently toggleable in the individual zone.
    const individualZone = page.getByTestId("individual-packages-zone");
    const customAgentRow = individualZone.getByTestId(
      "package-row-custom-agent.x86_64",
    );
    await expect(customAgentRow).toBeVisible({ timeout: 5000 });

    // The custom-agent package should still have a working checkbox and be included
    const checkbox = customAgentRow.locator("input[type=checkbox]");
    await expect(checkbox).toBeVisible();
    await expect(checkbox).toBeChecked();
  });

  // ── 6. Native tab traversal follows chevron → ungroup → toggle ────
  test("native tab traversal follows chevron → ungroup → toggle order", async ({
    page,
  }) => {
    const groupRow = page.getByTestId("group-row-Web Server");
    await expect(groupRow).toBeVisible();

    // Focus the chevron button first
    const chevron = groupRow.locator(".inspectah-group-row__chevron");
    await chevron.focus();
    await expect(chevron).toBeFocused();

    // Tab to ungroup button
    await page.keyboard.press("Tab");
    const ungroupBtn = groupRow.getByRole("button", { name: /ungroup/i });
    await expect(ungroupBtn).toBeFocused();

    // Tab to toggle switch
    await page.keyboard.press("Tab");
    // The Switch component renders an input inside the toggle container
    const toggleInput = groupRow.locator(
      `#group-toggle-Web\\ Server`,
    );
    await expect(toggleInput).toBeFocused();
  });
});
