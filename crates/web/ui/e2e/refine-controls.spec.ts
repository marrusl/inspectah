import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

const FIXTURES_DIR = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "fixtures",
);

const QUADLET_TRIAGE = {
  triage: { mode: "single_host", baseline: null },
  primary_reason: "quadlet_user_deployed",
  annotations: [],
};

/**
 * The groups fixture with what this spec needs to render: the lists this
 * spec does not use are emptied, and two quadlets are added.
 */
function viewFixture() {
  const view = JSON.parse(
    fs.readFileSync(
      path.join(FIXTURES_DIR, "single-host", "view-with-groups.json"),
      "utf-8",
    ),
  );
  return {
    ...view,
    config_files: [],
    unit_states: [],
    users_groups_decisions: [],
    quadlets: [
      {
        path: "/etc/containers/systemd/myapp.container",
        name: "myapp.container",
        image: "quay.io/org/myapp:latest",
        triage: QUADLET_TRIAGE,
        include: true,
        locked: false,
      },
      {
        path: "/etc/containers/systemd/myapp.network",
        name: "myapp.network",
        image: "",
        triage: QUADLET_TRIAGE,
        include: true,
        locked: false,
      },
    ],
    not_carried_packages: [],
    not_carried_notice: null,
  };
}

test.describe("Refine controls", () => {
  let opBodies: unknown[];

  test.beforeEach(async ({ page }) => {
    opBodies = [];
    await applyMockApi(page, "single-host");
    const view = viewFixture();
    // Registered after applyMockApi, so these routes win.
    // /api/groups narrowed to the two sections this spec visits.
    await page.route("**/api/groups", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify([
          {
            slug: "packages-group",
            label: "Packages",
            sections: [{ id: "packages", label: "Packages", is_triage: true }],
            has_actionable_sections: true,
          },
          {
            slug: "containers-group",
            label: "Containers",
            sections: [
              { id: "containers", label: "Containers", is_triage: true },
            ],
            has_actionable_sections: true,
          },
        ]),
      }),
    );
    await page.route("**/api/view", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(view),
      }),
    );
    await page.route("**/api/op", (route) => {
      opBodies.push(route.request().postDataJSON());
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ ...view, generation: view.generation + 1 }),
      });
    });
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("Packages search keeps the whole word in the box", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();
    await expect(page.getByTestId("package-list")).toBeVisible();
    await page.locator(".inspectah-layout__main h2").click();
    await page.keyboard.press("/");
    const input = page.locator('[data-testid="section-search"] input');
    await expect(input).toBeFocused();

    await page.keyboard.type("mod_ssl");

    await expect(input).toHaveValue("mod_ssl");
    await expect(input).toBeFocused();
    await expect(page.getByTestId("group-member-mod_ssl")).toBeVisible();
    await expect(page.getByTestId("package-row-bash.x86_64")).toHaveCount(0);
  });

  test("j, k, G, g and x work on package rows", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();
    const rows = page.locator('[data-testid^="package-row-"]');
    await expect(rows.first()).toBeVisible();
    const count = await rows.count();
    expect(count).toBeGreaterThan(2);

    await rows.nth(0).locator(".inspectah-package-row__name").click();
    await expect(rows.nth(0)).toBeFocused();
    await page.keyboard.press("j");
    await expect(rows.nth(1)).toBeFocused();
    await page.keyboard.press("k");
    await expect(rows.nth(0)).toBeFocused();
    await page.keyboard.press("G");
    await expect(rows.nth(count - 1)).toBeFocused();
    await page.keyboard.press("g");
    // The first row in the list is the group row above the packages.
    await expect(page.getByTestId("group-row-Web Server")).toBeFocused();

    await rows.nth(1).locator(".inspectah-package-row__name").click();
    const name = (await rows.nth(1).getAttribute("data-testid"))!.replace(
      "package-row-",
      "",
    );
    await page.keyboard.press("x");
    await expect.poll(() => opBodies.length).toBe(1);
    expect(JSON.stringify(opBodies[0])).toContain(
      `"name":"${name.slice(0, name.lastIndexOf("."))}"`,
    );
  });

  test("j moves between container rows", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Containers").click();
    const first = page.getByTestId(
      "quadlet-item-/etc/containers/systemd/myapp.container",
    );
    const second = page.getByTestId(
      "quadlet-item-/etc/containers/systemd/myapp.network",
    );
    await first.locator(".inspectah-decision-row__name").click();
    await first.focus();
    await page.keyboard.press("j");
    await expect(second).toBeFocused();
    await page.keyboard.press("k");
    await expect(first).toBeFocused();
  });

  test("Version Changes opens and closes", async ({ page }) => {
    await page.locator(".inspectah-layout__sidebar").getByText("Packages").click();
    const toggle = page.getByRole("button", { name: "Version Changes" });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(
      page.locator('[data-testid^="context-item-"]').first(),
    ).toBeVisible();
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  test("a stale export says so, and the next export posts the current generation", async ({
    page,
  }) => {
    const view = viewFixture();
    const current = view.generation + 1;
    let changedElsewhere = false;
    const posted: number[] = [];
    // Registered after beforeEach's /api/view route, so this one wins.
    await page.route("**/api/view", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          ...view,
          generation: changedElsewhere ? current : view.generation,
        }),
      }),
    );
    await page.route("**/api/tarball", (route) => {
      // Another tab changes the session before the first export lands.
      changedElsewhere = true;
      const generation = route.request().postDataJSON().generation;
      posted.push(generation);
      if (generation !== current) {
        return route.fulfill({
          status: 409,
          contentType: "application/json",
          body: JSON.stringify({ error: "stale generation" }),
        });
      }
      return route.fulfill({
        status: 200,
        contentType: "application/gzip",
        headers: { "x-export-warnings": "[]" },
        body: "tarball",
      });
    });
    await page.getByRole("button", { name: /export/i }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    const exportButton = dialog.getByRole("button", { name: /^export$/i });
    await exportButton.click();
    await expect(dialog.getByText("Stale state")).toBeVisible();
    await expect(dialog.getByText(/export again/)).toBeVisible();
    await expect(dialog).toContainText(`Generation: ${current}`);

    const download = page.waitForEvent("download");
    await exportButton.click();
    await download;
    expect(posted).toEqual([view.generation, current]);
    await expect(dialog).toBeHidden();
  });
});
