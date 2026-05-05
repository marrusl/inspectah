/**
 * Section navigation tests for the refine report UI.
 * Validates sidebar links navigate to correct sections and show proper headings.
 */
import { test, expect } from '@playwright/test';
import { waitForBoot, navigateToSection, getSidebarSections } from './helpers';

test.describe('Section navigation', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/');
    await waitForBoot(page);
  });

  // Sections that are always present regardless of fixture data.
  // nonrpm and version-changes are conditionally rendered by the SPA
  // based on snapshot content, so they are tested separately.
  const sections = [
    { id: 'overview', label: 'Overview' },
    { id: 'packages', label: 'Packages' },
    { id: 'config', label: 'Configuration' },
    { id: 'runtime', label: 'Runtime' },
    { id: 'containers', label: 'Containers' },
    { id: 'identity', label: 'Identity' },
    { id: 'system', label: 'System & Security' },
    { id: 'secrets', label: 'Secrets' },
    { id: 'editor', label: 'Edit Files' },
  ];

  for (const section of sections) {
    test(`navigates to ${section.label} section`, async ({ page }) => {
      await navigateToSection(page, section.id);

      // Section heading should be visible
      const heading = page.locator(`#heading-${section.id}`);
      await expect(heading).toBeVisible();

      // Active sidebar link uses aria-current="page" (PatternFly pattern)
      const navLink = page.locator(`[data-section="${section.id}"]`);
      await expect(navLink).toHaveAttribute('aria-current', 'page');
    });
  }

  test('only one section is active at a time', async ({ page }) => {
    await navigateToSection(page, 'packages');

    const activeLinks = page.locator('[data-section][aria-current="page"]');
    await expect(activeLinks).toHaveCount(1);
    await expect(activeLinks.first()).toHaveAttribute('data-section', 'packages');
  });

  test('section containers use proper IDs', async ({ page }) => {
    for (const section of sections) {
      await navigateToSection(page, section.id);
      const container = page.locator(`#section-${section.id}`);
      await expect(container).toBeAttached();
    }
  });

  test('conditional sections only appear when data exists', async ({ page }) => {
    const sidebarSections = await getSidebarSections(page);

    // version-changes only shows when snapshot has rpm.version_changes
    // nonrpm only shows when snapshot has non_rpm_software.items
    // These are data-dependent; just verify they don't break the sidebar
    const conditionalIds = ['nonrpm', 'version-changes'];
    for (const id of conditionalIds) {
      if (sidebarSections.includes(id)) {
        await navigateToSection(page, id);
        const heading = page.locator(`#heading-${id}`);
        await expect(heading).toBeVisible();
      }
    }
  });
});
