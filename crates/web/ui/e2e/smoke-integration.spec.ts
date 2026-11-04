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
      const response = await fetch("http://localhost:5174/", {
        signal: AbortSignal.timeout(3000)
      });
      viteAvailable = response.ok;
    } catch {
      viteAvailable = false;
    }

    // In CI neither service is optional, so an unreachable one must fail
    // loudly here. This is the only spec that exercises the real refine
    // backend, and the per-test skips below mean it goes *quiet* exactly when
    // the backend is dead — the one circumstance it exists to detect. CI has
    // no excuse for either being down: the workflow starts the refine server,
    // and Playwright starts Vite itself via `webServer` with
    // reuseExistingServer false. Locally a skip remains a convenience.
    if (process.env.CI) {
      const unreachable: string[] = [];
      if (!serverAvailable) {
        unreachable.push("refine backend at http://127.0.0.1:8642/api/health");
      }
      if (!viteAvailable) {
        unreachable.push("Vite dev server at http://localhost:5174/");
      }
      if (unreachable.length > 0) {
        throw new Error(
          `Smoke integration cannot run: ${unreachable.join(" and ")} unreachable. ` +
            `In CI this is a failure, not a skip: it means the service did not come up.`
        );
      }
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
      expect(view.generation).toBeGreaterThanOrEqual(0);
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
      await page.goto("http://localhost:5174/");
      await expect(page.locator(".inspectah-statsbar")).toBeVisible();

      const initialView = await (await page.request.get("http://127.0.0.1:8642/api/view")).json();

      const firstToggle = page.locator("input[type='checkbox']").first();
      await expect(firstToggle).toBeVisible({ timeout: 5000 });
      await firstToggle.click({ force: true });
      await page.waitForResponse((res) => res.url().includes("/api/op"));
      const afterExclude = await (await page.request.get("http://127.0.0.1:8642/api/view")).json();
      expect(afterExclude.generation).toBeGreaterThan(initialView.generation);
      expect(afterExclude.stats.can_undo).toBe(true);

      const undoButton = page.locator(".inspectah-statsbar").getByRole("button", { name: /undo/i });
      await expect(undoButton).toBeVisible();
      await undoButton.click();
      await page.waitForResponse((res) => res.url().includes("/api/undo"));
      const afterUndo = await (await page.request.get("http://127.0.0.1:8642/api/view")).json();
      expect(afterUndo.generation).toBeGreaterThan(afterExclude.generation);
      expect(afterUndo.stats.can_redo).toBe(true);
    });

    test("ops reflects the mutation (cross-endpoint coherence)", async ({ page }) => {
      await page.goto("http://localhost:5174/");
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
      await page.goto("http://localhost:5174/");
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
