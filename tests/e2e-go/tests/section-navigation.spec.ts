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
  // Core sections that are always present regardless of fixture data.
  // Editor is conditionally rendered (only when fixture has config/drop-in/quadlet files).
  const coreSections = [
    { id: 'overview', label: 'Overview' },
    { id: 'packages', label: 'Packages' },
    { id: 'config', label: 'Configuration' },
    { id: 'runtime', label: 'Runtime' },
    { id: 'containers', label: 'Containers' },
    { id: 'identity', label: 'Identity' },
    { id: 'system', label: 'System & Security' },
    { id: 'secrets', label: 'Secrets' },
  ];

  // Sections that may or may not be visible depending on fixture data
  const conditionalSections = [
    { id: 'editor', label: 'Edit Files' },
  ];

  const sections = [...coreSections, ...conditionalSections];

  for (const section of coreSections) {
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

  for (const section of conditionalSections) {
    test(`navigates to ${section.label} section`, async ({ page }) => {
      const navLink = page.locator(`[data-section="${section.id}"]`);
      const isVisible = (await navLink.count()) > 0 &&
        await navLink.evaluate((el) => {
          const li = el.closest('li');
          return li ? getComputedStyle(li).display !== 'none' : true;
        });

      if (!isVisible) {
        test.skip(true, `${section.label} section not present in fixture`);
        return;
      }

      await navigateToSection(page, section.id);
      const heading = page.locator(`#heading-${section.id}`);
      await expect(heading).toBeVisible();
    });
  }

  test('only one section is active at a time', async ({ page }) => {
    await navigateToSection(page, 'packages');

    const activeLinks = page.locator('[data-section][aria-current="page"]');
    await expect(activeLinks).toHaveCount(1);
    await expect(activeLinks.first()).toHaveAttribute('data-section', 'packages');
  });

  test('section containers use proper IDs', async ({ page }) => {
    for (const section of coreSections) {
      await navigateToSection(page, section.id);
      const container = page.locator(`#section-${section.id}`);
      await expect(container).toBeAttached();
    }
    // Also check conditional sections if visible
    for (const section of conditionalSections) {
      const navLink = page.locator(`[data-section="${section.id}"]`);
      const isVisible = (await navLink.count()) > 0 &&
        await navLink.evaluate((el) => {
          const li = el.closest('li');
          return li ? getComputedStyle(li).display !== 'none' : true;
        });
      if (isVisible) {
        await navigateToSection(page, section.id);
        const container = page.locator(`#section-${section.id}`);
        await expect(container).toBeAttached();
      }
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
