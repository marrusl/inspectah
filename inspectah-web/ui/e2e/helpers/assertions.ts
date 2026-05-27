// e2e/helpers/assertions.ts
import { Page, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

export async function expectStatsBar(
  page: Page,
  expected: { packages?: string; configs?: string },
): Promise<void> {
  const statsBar = page.locator(".inspectah-statsbar");
  await expect(statsBar).toBeVisible();
  if (expected.packages) await expect(statsBar.getByText("Packages:")).toBeVisible();
  if (expected.configs) await expect(statsBar.getByText("Configs:")).toBeVisible();
}

export async function expectSidebarSection(
  page: Page,
  name: string,
  visible = true,
): Promise<void> {
  const sidebar = page.locator(".inspectah-layout__sidebar");
  const section = sidebar.getByText(name);
  if (visible) {
    await expect(section).toBeVisible();
  } else {
    await expect(section).not.toBeVisible();
  }
}

export async function expectDecisionItem(
  page: Page,
  testId: string,
  included: boolean,
): Promise<void> {
  const item = page.getByTestId(testId);
  await expect(item).toBeVisible();
  const toggle = item.getByRole("switch");
  if (included) {
    await expect(toggle).toBeChecked();
  } else {
    await expect(toggle).not.toBeChecked();
  }
}

export async function expectContainerfileContains(
  page: Page,
  text: string,
): Promise<void> {
  const panel = page.locator(".inspectah-cf-panel--open");
  await expect(panel).toBeVisible();
  await expect(panel.locator(".inspectah-cf-panel__code")).toContainText(text);
}

export async function expectNoAxeViolations(
  page: Page,
  tags: string[] = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"],
): Promise<void> {
  await page.waitForTimeout(500);
  const results = await new AxeBuilder({ page }).withTags(tags).analyze();
  const critical = results.violations.filter(
    (v) => v.impact === "critical" || v.impact === "serious",
  );
  if (critical.length > 0) {
    const summary = critical
      .map((v) => `[${v.impact}] ${v.id}: ${v.description} (${v.nodes.length})`)
      .join("\n");
    expect(critical, `Accessibility violations:\n${summary}`).toEqual([]);
  }
}
