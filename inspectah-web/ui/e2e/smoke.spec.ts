import { test, expect } from "@playwright/test";

test.describe("Smoke tests", () => {
  test("page loads and shows the refine UI", async ({ page }) => {
    await page.goto("/");
    // The page title or main layout should be present
    await expect(page.locator(".inspectah-page")).toBeVisible();
  });

  test("sidebar shows decision sections", async ({ page }) => {
    await page.goto("/");
    const sidebar = page.locator(".inspectah-layout__sidebar");
    await expect(sidebar).toBeVisible();

    // Decision sections: Packages, Config Files
    await expect(sidebar.getByText("Packages")).toBeVisible();
    await expect(sidebar.getByText("Config Files")).toBeVisible();
  });

  test("sidebar shows context sections", async ({ page }) => {
    await page.goto("/");
    const sidebar = page.locator(".inspectah-layout__sidebar");

    // Context sections from the snapshot
    const contextSections = [
      "Services",
      "Containers",
      "Users & Groups",
      "Network",
      "Storage",
      "Scheduled Tasks",
      "Non-RPM Software",
      "Kernel & Boot",
      "SELinux",
    ];

    for (const name of contextSections) {
      await expect(sidebar.getByText(name)).toBeVisible();
    }
  });

  test("stats bar renders package and config counts", async ({ page }) => {
    await page.goto("/");
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar).toBeVisible();

    // Stats bar shows "Packages:" and "Configs:" labels with numbers
    await expect(statsBar.getByText("Packages:")).toBeVisible();
    await expect(statsBar.getByText("Configs:")).toBeVisible();

    // Undo/redo buttons should be present
    await expect(statsBar.getByRole("button", { name: /undo/i })).toBeVisible();
    await expect(statsBar.getByRole("button", { name: /redo/i })).toBeVisible();
  });

  test("health endpoint returns ok", async ({ request }) => {
    const response = await request.get("/api/health");
    expect(response.ok()).toBeTruthy();
    const body = await response.json();
    expect(body.status).toBe("ok");
    expect(body.host).toBeDefined();
    expect(body.host.hostname).toBeTruthy();
  });
});
