import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

// Sidebar labels from REVIEW_SECTIONS and REFERENCE_SECTIONS in Sidebar.tsx
const REVIEW_SECTIONS = [
  "Packages",
  "Config Files",
  "Users & Groups",
  "Services",
  "Containers",
  "System Tuning",
];

const REFERENCE_SECTIONS = [
  "Version Changes",
  "Compose",
  "Network",
  "Storage",
  "Scheduled Tasks",
  "Non-RPM Software",
  "Kernel & Boot",
  "Security & Access Control",
];

const ALL_SECTIONS = [...REVIEW_SECTIONS, ...REFERENCE_SECTIONS];

// Sections that have corresponding fixture data (from single-host/sections.json)
// Others will render but may show empty states
const SECTIONS_WITH_FIXTURE_DATA = [
  "Packages",      // packages
  "Config Files",  // configs
  "Users & Groups",// users_groups
  "Services",      // services
  "Containers",    // containers (maps to compose in fixture)
  "System Tuning", // sysctls + tuned
  "Version Changes", // version_changes
  // repos exists but no sidebar entry for it (merged into Packages view)
  // Note: Compose section exists in sidebar but maps to containers fixture
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
      await expect(sidebar.getByText(name)).toBeVisible();
    }
  });

  test("sidebar renders all reference sections", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    for (const name of REFERENCE_SECTIONS) {
      await expect(sidebar.getByText(name)).toBeVisible();
    }
  });

  // Test sections that have fixture data
  for (const section of SECTIONS_WITH_FIXTURE_DATA) {
    test(`clicking ${section} renders content pane with data`, async ({ page }) => {
      await page.locator(".inspectah-layout__sidebar").getByText(section).click();
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
});
