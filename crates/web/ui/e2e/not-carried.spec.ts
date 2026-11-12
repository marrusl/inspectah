import { test, expect } from "@playwright/test";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

// Server recordings (crates/web/tests/system_tuning_fixtures_test.rs), not
// the hand-kept preset, whose package rows predate the disposition field.
const RECORDED = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "__tests__",
  "fixtures",
  "system-tuning",
);

function recorded(name: string): string {
  return path.join(RECORDED, `${name}.json`);
}

// The Not carried group and the Export dialog's pre-download notice, end to
// end in a browser. The view is a recorded server payload with the two
// not-carried fields set per the projection contract in
// process-docs/skills/not-carried-auto-packages.md (Refine projection
// contract).

const FIREWALLD = {
  item_id: { kind: "Package", key: { name: "firewalld", arch: "noarch" } },
  display_name: "firewalld",
  source_repo: "baseos",
  enabled_units: ["firewalld.service"],
  owns_enabled_unit: true,
};
const CRONIE = {
  item_id: { kind: "Package", key: { name: "cronie", arch: "x86_64" } },
  display_name: "cronie",
  source_repo: "baseos",
  enabled_units: ["crond.service"],
  owns_enabled_unit: true,
};
const PYTHON3_FIREWALL = {
  item_id: {
    kind: "Package",
    key: { name: "python3-firewall", arch: "noarch" },
  },
  display_name: "python3-firewall",
  source_repo: "baseos",
  enabled_units: [],
  owns_enabled_unit: false,
};

/** firewalld as the package list shows it once included. */
const FIREWALLD_INCLUDED = {
  entry: {
    aggregate: null,
    arch: "noarch",
    disposition: { include: true, kind: "actionable" },
    epoch: "0",
    name: "firewalld",
    release: "1.el9",
    repo_attribution: { kind: "unrecorded" },
    source_repo: "baseos",
    state: "added",
    version: "1.3.4",
  },
  triage: {
    primary_reason: "package_baseline_match",
    triage: { investigate: null, mode: "single_host" },
  },
};

const NOTICE_BOTH = {
  kind: "notice",
  text: "2 packages that own a unit enabled on the source host are not carried: cronie, firewalld. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.",
};
const NOTICE_CRONIE = {
  kind: "notice",
  text: "1 package that owns a unit enabled on the source host is not carried: cronie. See the Not carried table in audit-report.md and the # Not carried block in the Containerfile.",
};
const FAILURE = {
  kind: "failure",
  text: "epel-release.noarch: staged RPM not verified, not exported",
};

test.describe("Not carried", () => {
  // The single-host view the server would send, with only the not-carried
  // fields varying. Stateful: an include changes what GET /api/view returns,
  // since the packages view refetches after an op.
  let current: Record<string, unknown>;
  let opBodies: unknown[];

  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host", {
      "/api/health": recorded("files-no-tuned.health"),
    });
    const groups = fs.readFileSync(recorded("files-no-tuned.groups"), "utf-8");
    await page.route("**/api/groups", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: groups,
      }),
    );
    const base = JSON.parse(
      fs.readFileSync(recorded("files-no-tuned.view"), "utf-8"),
    );
    // Server order: unit owners first, then the rest; each by name, then arch.
    current = {
      ...base,
      not_carried_packages: [CRONIE, FIREWALLD, PYTHON3_FIREWALL],
      not_carried_notice: NOTICE_BOTH,
    };
    opBodies = [];
    await page.route("**/api/view", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(current),
      }),
    );
    await page.route("**/api/op", (route) => {
      opBodies.push(route.request().postDataJSON());
      // firewalld included: its row leaves, python3-firewall stays (the
      // known limit), the notice drops its name, and firewalld joins the
      // package list as an included decision in a new generation.
      const view = current as {
        packages: unknown[];
        generation: number;
      };
      current = {
        ...current,
        packages: [...view.packages, FIREWALLD_INCLUDED],
        generation: view.generation + 1,
        not_carried_packages: [CRONIE, PYTHON3_FIREWALL],
        not_carried_notice: NOTICE_CRONIE,
      };
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(current),
      });
    });
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  test("lists the rows in server order and includes one with a click", async ({
    page,
  }) => {
    const group = page.getByTestId("not-carried-group");
    await group.getByRole("button", { name: /not carried \(3\)/i }).click();

    await expect(group.getByTestId(/^not-carried-row-/)).toHaveCount(3);
    const ids = await group
      .getByTestId(/^not-carried-row-/)
      .evaluateAll((els) => els.map((e) => e.getAttribute("data-testid")));
    expect(ids).toEqual([
      "not-carried-row-cronie.x86_64",
      "not-carried-row-firewalld.noarch",
      "not-carried-row-python3-firewall.noarch",
    ]);
    await expect(
      group.getByTestId("not-carried-row-firewalld.noarch"),
    ).toHaveClass(/inspectah-not-carried__row--owner/);
    await expect(
      group.getByTestId("not-carried-row-python3-firewall.noarch"),
    ).not.toHaveClass(/inspectah-not-carried__row--owner/);

    await group.getByRole("button", { name: "Include firewalld" }).click();

    await expect(
      group.getByTestId("not-carried-row-firewalld.noarch"),
    ).toHaveCount(0);
    expect(opBodies).toEqual([
      {
        kind: "Op",
        op: "SetInclude",
        target: {
          item_id: {
            kind: "Package",
            key: { name: "firewalld", arch: "noarch" },
          },
          include: true,
        },
      },
    ]);
    await expect(
      group.getByTestId("not-carried-row-python3-firewall.noarch"),
    ).toBeVisible();
    await expect(
      page.getByRole("checkbox", { name: "firewalld.noarch" }),
    ).toBeChecked();
  });

  async function exportWith(
    page: import("@playwright/test").Page,
    header: unknown[],
  ) {
    await page.route("**/api/tarball", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/gzip",
        headers: { "x-export-warnings": JSON.stringify(header) },
        body: Buffer.from([0x1f, 0x8b, 0x08, 0x00]),
      }),
    );
    await page.getByRole("button", { name: /export/i }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible({ timeout: 3000 });
    await expect(dialog.getByTestId("not-carried-notice")).toBeVisible();
    const downloadPromise = page.waitForEvent("download");
    await dialog.getByRole("button", { name: /^export$/i }).click();
    await downloadPromise;
    return dialog;
  }

  test("the Export dialog shows a compact info notice before download", async ({
    page,
  }) => {
    await page.getByRole("button", { name: /export/i }).click();
    const notice = page.getByRole("dialog").getByTestId("not-carried-notice");

    await expect(notice).toHaveClass(/pf-m-info/);
    await expect(notice).toContainText(
      "2 packages that own enabled units will not be carried",
    );
    await expect(notice.getByText("crond.service")).toHaveCount(0);

    await notice
      .getByRole("button", { name: "Show not-carried details" })
      .click();
    await expect(notice.getByText("crond.service")).toBeVisible();
  });

  test("the review link closes the dialog and shows the group", async ({
    page,
  }) => {
    // Short enough that the group starts below the fold, so the link has to
    // scroll to it.
    await page.setViewportSize({ width: 1280, height: 280 });
    await expect(page.getByTestId("not-carried-group")).not.toBeInViewport();
    // Start elsewhere, so the link has to switch sections too.
    await page
      .getByRole("navigation", { name: "Sections" })
      .getByRole("link", { name: /^System Tuning / })
      .click();
    await expect(page.getByTestId("not-carried-group")).toHaveCount(0);

    await page.getByRole("button", { name: /export/i }).click();
    const dialog = page.getByRole("dialog");
    await dialog
      .getByRole("button", { name: "Review not-carried packages" })
      .click();

    await expect(dialog).toBeHidden();
    const group = page.getByTestId("not-carried-group");
    const toggle = group.getByRole("button", { name: /not carried \(3\)/i });
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(group).toBeInViewport();
    await expect(toggle).toBeFocused();
    await expect(
      group.getByRole("button", { name: "Include firewalld" }),
    ).toBeVisible();
  });

  test("after download, the notice already shown is not repeated", async ({
    page,
  }) => {
    const dialog = await exportWith(page, [NOTICE_BOTH, FAILURE]);
    await expect(dialog.getByText("Downloaded, with warnings")).toBeVisible();
    await expect(dialog.getByTestId("export-warning-0")).toContainText(
      "staged RPM not verified",
    );
    await expect(dialog.getByTestId("export-warning-1")).toHaveCount(0);
  });

  test("after download, the dialog closes when only the shown notice came back", async ({
    page,
  }) => {
    const dialog = await exportWith(page, [NOTICE_BOTH]);
    await expect(dialog).toBeHidden();
  });

  test("long names, repos and units wrap clear of the Include button", async ({
    page,
  }) => {
    // Host strings longer than the row. The package name and the repo's tail
    // are unbroken tokens, with no hyphen for the browser to wrap at, so only
    // an explicit breaking rule keeps them clear of the button.
    const longName = "python3-" + "averyveryverylongpackagename".repeat(3);
    const LONG = {
      item_id: { kind: "Package", key: { name: longName, arch: "x86_64" } },
      display_name: longName,
      source_repo: "rhel-9-for-x86_64-appstream-eus-rpms-" + "mirror".repeat(8),
      enabled_units: [
        `${"long-running-agent-".repeat(4)}worker@primary.service`,
      ],
      owns_enabled_unit: true,
    };
    current = { ...current, not_carried_packages: [LONG, PYTHON3_FIREWALL] };
    await page.setViewportSize({ width: 480, height: 900 });
    await page.reload();
    const group = page.getByTestId("not-carried-group");
    await group.getByRole("button", { name: /not carried \(2\)/i }).click();

    const row = group.getByTestId(`not-carried-row-${longName}.x86_64`);
    const button = row.getByRole("button", { name: `Include ${longName}` });
    await expect(button).toBeVisible();

    const rowBox = (await row.boundingBox())!;
    const buttonBox = (await button.boundingBox())!;
    // The button keeps its natural width and stays inside the row.
    expect(buttonBox.x + buttonBox.width).toBeLessThanOrEqual(
      rowBox.x + rowBox.width + 0.5,
    );
    expect(
      await button.evaluate((el) => el.scrollWidth <= el.clientWidth),
    ).toBe(true);
    // No text box reaches into the button.
    for (const text of [
      row.locator(".inspectah-not-carried__name"),
      row.locator(".inspectah-not-carried__repo"),
      row.locator(".inspectah-not-carried__units code"),
    ]) {
      const box = (await text.boundingBox())!;
      expect(box.x + box.width).toBeLessThanOrEqual(buttonBox.x);
    }
    expect(await row.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(
      true,
    );
  });
});
