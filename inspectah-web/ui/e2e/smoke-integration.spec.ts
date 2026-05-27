import { test, expect } from "@playwright/test";

test.describe("Smoke integration (real server)", () => {
  test.describe.configure({ mode: "serial" });

  let serverAvailable = false;
  let viteAvailable = false;

  test.beforeAll(async () => {
    // Check server
    try {
      const response = await fetch("http://127.0.0.1:8642/api/health", {
        signal: AbortSignal.timeout(3000)
      });
      serverAvailable = response.ok;
    } catch {
      serverAvailable = false;
    }

    // Check Vite
    try {
      const response = await fetch("http://127.0.0.1:5174/", {
        signal: AbortSignal.timeout(3000)
      });
      viteAvailable = response.ok;
    } catch {
      viteAvailable = false;
    }
  });

  test.describe("API endpoints", () => {
    test.beforeEach(async () => {
      if (!serverAvailable) {
        test.skip(true, "Refine server not running on port 8642");
      }
    });

    test("health endpoint returns valid response", async ({ page }) => {
      const response = await page.request.get("http://127.0.0.1:8642/api/health");
      expect(response.ok()).toBeTruthy();
      const body = await response.json();
      expect(body.status).toBe("ok");
      expect(body.host.hostname).toBeTruthy();
      expect(typeof body.session_is_sensitive).toBe("boolean");
    });

    test("view returns valid initial state", async ({ page }) => {
      const response = await page.request.get("http://127.0.0.1:8642/api/view");
      expect(response.ok()).toBeTruthy();
      const view = await response.json();
      expect(view.generation).toBeGreaterThanOrEqual(1);
      expect(view.packages.length).toBeGreaterThan(0);
      expect(typeof view.containerfile_preview).toBe("string");
    });

    test("tarball export produces gzip response", async ({ page }) => {
      const view = await (await page.request.get("http://127.0.0.1:8642/api/view")).json();
      const response = await page.request.post("http://127.0.0.1:8642/api/tarball", {
        data: { generation: view.generation },
        headers: { "Content-Type": "application/json" },
      });
      expect(response.ok()).toBeTruthy();
      expect(response.headers()["content-type"]).toContain("application/gzip");
      const body = await response.body();
      expect(body.length).toBeGreaterThan(0);
    });
  });

  test.describe("UI interactions", () => {
    test.beforeEach(async () => {
      if (!serverAvailable) {
        test.skip(true, "Refine server not running on port 8642");
      }
      if (!viteAvailable) {
        test.skip(true, "Vite dev server not running on port 5174 — start with 'npm run dev'");
      }
    });

    test("exclude → containerfile changes → undo", async ({ page }) => {
      await page.goto("http://127.0.0.1:5174/");
      await expect(page.locator(".inspectah-statsbar")).toBeVisible();

      const initialView = await (await page.request.get("http://127.0.0.1:8642/api/view")).json();
      const initialGen = initialView.generation;

      // Find first checkbox toggle
      const firstToggle = page.locator("input[type='checkbox']").first();
      await expect(firstToggle).toBeVisible({ timeout: 5000 });
      const opResp = page.waitForResponse((res) => res.url().includes("/api/op"));
      await firstToggle.click({ force: true });
      const afterExclude = await (await opResp).json();
      expect(afterExclude.generation).toBe(initialGen + 1);
      expect(afterExclude.can_undo).toBe(true);
      expect(afterExclude.containerfile_preview).not.toBe(initialView.containerfile_preview);

      const undoButton = page.locator(".inspectah-statsbar").getByRole("button", { name: /undo/i });
      await expect(undoButton).toBeVisible();
      const undoResp = page.waitForResponse((res) => res.url().includes("/api/undo"));
      await undoButton.click();
      const afterUndo = await (await undoResp).json();
      expect(afterUndo.generation).toBe(initialGen);
      expect(afterUndo.can_redo).toBe(true);
    });

    test("ops reflects the mutation (cross-endpoint coherence)", async ({ page }) => {
      await page.goto("http://127.0.0.1:5174/");
      await expect(page.locator(".inspectah-statsbar")).toBeVisible();

      const opsBefore = await (await page.request.get("http://127.0.0.1:8642/api/ops")).json();
      const countBefore = opsBefore.length;

      const firstToggle = page.locator("input[type='checkbox']").first();
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/op"));

      const opsAfter = await (await page.request.get("http://127.0.0.1:8642/api/ops")).json();
      expect(opsAfter.length).toBe(countBefore + 1);
    });

    test("viewed persistence across reload", async ({ page }) => {
      await page.goto("http://127.0.0.1:5174/");
      await expect(page.locator(".inspectah-statsbar")).toBeVisible();

      await page.request.post("http://127.0.0.1:8642/api/viewed", {
        data: { id: "packages:test-pkg" },
        headers: { "Content-Type": "application/json" },
      });

      const viewed = await (await page.request.get("http://127.0.0.1:8642/api/viewed")).json();
      expect(viewed.ids).toContain("packages:test-pkg");

      await page.reload();
      await expect(page.locator(".inspectah-statsbar")).toBeVisible();
      const viewedAfterReload = await (await page.request.get("http://127.0.0.1:8642/api/viewed")).json();
      expect(viewedAfterReload.ids).toContain("packages:test-pkg");
    });
  });
});
