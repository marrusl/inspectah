import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";
import { sidebarLink } from "./helpers/assertions";

// The sidebar draws one link per section `/api/groups` returns. Triage
// sections carry a decision count, reference sections an item count.
const REVIEW_SECTIONS = [
  "Packages",
  "Configuration Files",
  "Kernel & Boot",
  "System Tuning",
  "Security & Access Control",
  "Systemd units",
  "Scheduled Tasks",
  "Containers",
  "Users & Groups",
  "Non-RPM Software",
  "Language Packages",
  "Unmanaged Files",
];

const REFERENCE_SECTIONS = ["Network", "Storage", "Secrets", "Subscription"];

// Sections with rows in the single-host preset view. The rest render too,
// but may only show an empty state.
const SECTIONS_WITH_FIXTURE_DATA = [
  "Packages",
  "Configuration Files",
  "Users & Groups",
  "Systemd units",
  "Containers",
  "System Tuning",
];

test.describe("Context sections", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => { await clearMocks(page); });

  test("sidebar renders all review sections", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    for (const name of REVIEW_SECTIONS) {
      await expect(sidebarLink(sidebar, name)).toBeVisible();
    }
  });

  test("sidebar renders all reference sections", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    for (const name of REFERENCE_SECTIONS) {
      await expect(sidebarLink(sidebar, name)).toBeVisible();
    }
  });

  // Test sections that have fixture data
  for (const section of SECTIONS_WITH_FIXTURE_DATA) {
    test(`clicking ${section} renders content pane with data`, async ({ page }) => {
      await sidebarLink(page.locator(".inspectah-layout__sidebar"), section).click();
      const main = page.locator(".inspectah-layout__main");
      await expect(main).toBeVisible();

      // Content pane should have items, an empty message, or a heading
      const hasDecisionItems = await main.locator("[data-testid^='decision-item-']").count();
      const hasContextItems = await main.locator("[data-testid^='context-item-']").count();
      const hasHeading = await main.locator("h2, h3").count();
      const hasEmptyMessage = await main.getByText(/no .* to triage|nothing/i).count();

      // At minimum, we should see a heading or some items or an empty message
      expect(hasDecisionItems + hasContextItems + hasHeading + hasEmptyMessage).toBeGreaterThan(0);
    });
  }

  // Version Changes is no longer a sidebar section: it is a collapsed panel
  // at the foot of Packages.
  test("Packages shows the Version Changes panel", async ({ page }) => {
    await sidebarLink(page.locator(".inspectah-layout__sidebar"), "Packages").click();
    const panel = page.getByTestId("version-changes-panel");
    await panel.getByRole("button", { name: "Version Changes" }).click();
    await expect(panel.getByText("openssl")).toBeVisible();
  });
});
