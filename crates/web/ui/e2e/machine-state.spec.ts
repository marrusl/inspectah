import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import * as fs from "fs";
import * as path from "path";
import { fileURLToPath } from "url";
import { applyMockApi, clearMocks } from "./helpers/mock-api";

// The machine-state block and the Export dialog's machine-state notices,
// end to end in a browser, against server recordings
// (crates/web/tests/system_tuning_fixtures_test.rs): a host with one class
// file per override level, before and after the IQN and hostname are
// included.
const RECORDED = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "src",
  "__tests__",
  "fixtures",
  "system-tuning",
);

function recorded(name: string): string {
  return fs.readFileSync(path.join(RECORDED, `${name}.json`), "utf-8");
}

const IQN = "/etc/iscsi/initiatorname.iscsi";
const HOSTNAME = "/etc/hostname";
const KEYTAB = "/etc/krb5.keytab";

test.describe("Machine state", () => {
  let current: string;
  let opBodies: unknown[];
  // The reply to the next op: the included view, or a refusal.
  let nextOp: { status: number; body: string };

  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host", {});
    const groups = recorded("machine-state.groups");
    const health = recorded("machine-state.health");
    current = recorded("machine-state.view");
    opBodies = [];
    nextOp = { status: 200, body: recorded("machine-state-included.view") };
    await page.route("**/api/groups", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: groups }),
    );
    await page.route("**/api/health", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: health }),
    );
    await page.route("**/api/view", (route) =>
      route.fulfill({ status: 200, contentType: "application/json", body: current }),
    );
    await page.route("**/api/op", (route) => {
      opBodies.push(route.request().postDataJSON());
      if (nextOp.status === 200) current = nextOp.body;
      return route.fulfill({
        status: nextOp.status,
        contentType: "application/json",
        body: nextOp.body,
      });
    });
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();
    await page
      .getByRole("navigation", { name: "Sections" })
      .getByRole("link", { name: /^Configuration Files / })
      .click();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  async function expandBlock(page: import("@playwright/test").Page) {
    const block = page.getByTestId("machine-state-group");
    await block.getByRole("button", { name: /^Machine state \(6\)/ }).click();
    return block;
  }

  test("shows the counts collapsed and the rows in three sub-lists", async ({
    page,
  }) => {
    const block = page.getByTestId("machine-state-group");
    await expect(
      block.getByRole("button", { name: /^Machine state/ }),
    ).toHaveText(
      "Machine state (6) · 2 host credentials · 2 provisioning steps",
    );
    await expandBlock(page);
    await expect(block.getByRole("heading", { level: 3 })).toHaveText([
      "Host credentials",
      "Host identity",
      "Generated",
    ]);
    const keytab = block.getByTestId(`machine-state-row-${KEYTAB}`);
    await expect(keytab).toContainText("Not carried");
    await expect(keytab).toContainText(
      "At provisioning: ipa-client-install, realm join or adcli join",
    );
    await expect(keytab.getByRole("button")).toHaveCount(0);
    await expect(keytab.getByRole("checkbox")).toHaveCount(0);
  });

  test("includes a warn row through the dialog, and focus lands on Exclude", async ({
    page,
  }) => {
    const block = await expandBlock(page);
    const iqn = block.getByTestId(`machine-state-row-${IQN}`);
    await iqn.getByRole("button", { name: `Include ${IQN}…` }).click();

    const dialog = page.getByRole("dialog", {
      name: "Include this host's identity in the image?",
    });
    await expect(dialog.getByRole("button", { name: "Cancel" })).toBeFocused();
    await dialog.getByRole("button", { name: "Include in image" }).click();

    await expect(dialog).toBeHidden();
    expect(opBodies).toEqual([
      {
        kind: "Op",
        op: "SetInclude",
        target: {
          item_id: { kind: "Config", key: { path: IQN } },
          include: true,
        },
      },
    ]);
    await expect(iqn).toContainText("Included by you");
    await expect(
      iqn.getByRole("button", { name: `Exclude ${IQN}` }),
    ).toBeFocused();
  });

  test("shows a not-carried refusal on the row", async ({ page }) => {
    nextOp = {
      status: 422,
      body: JSON.stringify({ error: `not carried: ${HOSTNAME} is withheld` }),
    };
    const block = await expandBlock(page);
    const row = block.getByTestId(`machine-state-row-${HOSTNAME}`);
    await row.getByRole("button", { name: `Include ${HOSTNAME}` }).click();

    await expect(row.getByTestId(`machine-state-failure-${HOSTNAME}`)).toContainText(
      `inspectah does not carry this file: ${HOSTNAME} is withheld. Nothing was changed.`,
    );
    await expect(
      row.getByRole("button", { name: `Include ${HOSTNAME}` }),
    ).toBeEnabled();
    await expect(row).not.toContainText("Included by you");
  });

  test("the Export dialog lists the steps and links back to the block", async ({
    page,
  }) => {
    await page
      .getByRole("navigation", { name: "Sections" })
      .getByRole("link", { name: /^System Tuning / })
      .click();
    await page.getByRole("button", { name: /export/i }).click();
    const dialog = page.getByRole("dialog");
    const notice = dialog.getByTestId("machine-state-notice");
    await expect(notice).toContainText(
      "6 machine-state files left out · 2 provisioning steps",
    );
    await notice.getByRole("button", { name: "Show machine-state details" }).click();
    await expect(notice.getByTestId("machine-state-step")).toHaveCount(2);

    await dialog.getByRole("button", { name: "Review machine state" }).click();
    await expect(dialog).toBeHidden();
    await expect(
      page.getByTestId("machine-state-group").getByRole("button", {
        name: /^Machine state/,
      }),
    ).toHaveAttribute("aria-expanded", "true");
  });

  test("has no axe violations and no horizontal scroll at a narrow desktop width", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await expandBlock(page);
    // Let the expand transition finish: mid-fade text reads as low contrast.
    await page.waitForTimeout(500);
    const results = await new AxeBuilder({ page })
      .include("#machine-state-group")
      .analyze();
    expect(results.violations).toEqual([]);
    const overflow = await page
      .getByTestId("machine-state-group")
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });
});
