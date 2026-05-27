import { test, expect } from "@playwright/test";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

test.describe("Smoke tests", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-page")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("page loads and shows the refine UI", async ({ page }) => {
    await expect(page.locator(".inspectah-page")).toBeVisible();
  });

  test("sidebar shows decision sections", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    await expect(sidebar).toBeVisible();
    await expect(sidebar.getByText("Packages")).toBeVisible();
    await expect(sidebar.getByText("Config Files")).toBeVisible();
  });

  test("sidebar shows reference sections", async ({ page }) => {
    const sidebar = page.locator(".inspectah-layout__sidebar");
    await expect(sidebar.getByText("Services")).toBeVisible();
    await expect(sidebar.getByText("Users & Groups")).toBeVisible();
  });

  test("stats bar renders package and config counts", async ({ page }) => {
    const statsBar = page.locator(".inspectah-statsbar");
    await expect(statsBar).toBeVisible();
    await expect(statsBar.getByText("Packages:")).toBeVisible();
    await expect(statsBar.getByText("Configs:")).toBeVisible();
    await expect(statsBar.getByRole("button", { name: /undo/i })).toBeVisible();
    await expect(statsBar.getByRole("button", { name: /redo/i })).toBeVisible();
  });

  test("health endpoint returns valid data", async ({ page }) => {
    const response = await page.request.get("/api/health");
    expect(response.ok()).toBeTruthy();
    const body = await response.json();
    expect(body.status).toBe("ok");
    expect(body.host).toBeDefined();
    expect(body.host.hostname).toBeTruthy();
  });
});
