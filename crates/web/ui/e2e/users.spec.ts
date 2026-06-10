import { test, expect } from "@playwright/test";
import * as path from "path";
import { fileURLToPath } from "url";
import {
  applyMockApi,
  clearMocks,
  mockPostResponse,
} from "./helpers/mock-api";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const FIXTURES_DIR = path.join(__dirname, "fixtures");

test.describe("Users & Groups", () => {
  test.beforeEach(async ({ page }) => {
    await applyMockApi(page, "single-host");
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    // Navigate to Users & Groups section
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Users & Groups")
      .click();
  });

  test.afterEach(async ({ page }) => {
    await clearMocks(page);
  });

  // ── 1. User cards render ──────────────────────────────────────────
  test("user cards render", async ({ page }) => {
    const cards = page.locator('[data-testid^="user-card-"]');
    await expect(cards.first()).toBeVisible({ timeout: 5000 });

    const count = await cards.count();
    expect(count).toBeGreaterThan(0);

    // Verify the testuser card specifically (from fixture)
    const testUserCard = page.locator('[data-testid="user-card-testuser"]');
    await expect(testUserCard).toBeVisible();

    // Verify card shows user details (use locator("strong") to avoid
    // matching /home/testuser in the detail line)
    await expect(testUserCard.locator("strong")).toContainText("testuser");
    await expect(testUserCard.getByText("UID 1000")).toBeVisible();
  });

  // ── 2. Include toggle changes user strategy ──────────────────────
  test("include toggle — POSTs to /api/user-strategy", async ({ page }) => {
    await mockPostResponse(
      page,
      "/api/user-strategy",
      "post-responses/user-strategy/success.json",
    );

    const card = page.locator('[data-testid="user-card-testuser"]');
    await expect(card).toBeVisible();

    // The checkbox reflects the current strategy (useradd = checked).
    // The fixture has containerfile_strategy: "useradd", so the checkbox
    // starts checked.  Clicking it should POST strategy: "skip".
    const toggle = card.locator(
      'input[type="checkbox"][aria-label="Include testuser"]',
    );
    await expect(toggle).toBeVisible();
    await expect(toggle).toBeChecked();

    // Intercept the POST to verify it fires
    const [request] = await Promise.all([
      page.waitForRequest(
        (req) =>
          req.url().includes("/api/user-strategy") &&
          req.method() === "POST",
      ),
      toggle.click({ force: true }),
    ]);

    const body = request.postDataJSON();
    expect(body.username).toBe("testuser");
    expect(body.strategy).toBe("skip");
  });

  // ── 3. Expand button reveals strategy ────────────────────────────
  test("expand button reveals containerfile strategy", async ({ page }) => {
    const card = page.locator('[data-testid="user-card-testuser"]');
    await expect(card).toBeVisible();

    // Strategy fieldset should not be visible before expanding
    await expect(
      card.getByText("Containerfile strategy"),
    ).not.toBeVisible();

    // Click expand button
    const expandBtn = card.locator(
      'button[aria-label="Expand testuser details"]',
    );
    await expect(expandBtn).toBeVisible();
    await expandBtn.click();

    // Strategy fieldset is now visible
    await expect(card.getByText("Containerfile strategy")).toBeVisible();

    // Verify radio buttons are present
    const skipRadio = card.locator('input[name="strategy-testuser"][value="skip"]');
    const useraddRadio = card.locator(
      'input[name="strategy-testuser"][value="useradd"]',
    );
    await expect(skipRadio).toBeAttached();
    await expect(useraddRadio).toBeAttached();

    // "useradd" should be selected (from fixture)
    await expect(useraddRadio).toBeChecked();

    // "Password options" section should also be present
    await expect(card.getByText("Password options")).toBeVisible();
  });

  // ── 4. Preview artifacts shows tabs ──────────────────────────────
  test("preview artifacts — Kickstart and Blueprint tabs", async ({
    page,
  }) => {
    // Click "Preview Artifacts" button
    const previewBtn = page.getByRole("button", {
      name: "Preview Artifacts",
    });
    await expect(previewBtn).toBeVisible();
    await previewBtn.click();

    // Modal opens (PF6 Modal)
    const modal = page.locator(".pf-v6-c-modal-box");
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Verify modal title
    await expect(modal.getByText("User Artifact Preview")).toBeVisible();

    // Verify tab buttons (use getByRole to avoid matching <pre> content)
    const kickstartTab = modal.getByRole("button", { name: "Kickstart" });
    const blueprintTab = modal.getByRole("button", {
      name: "Blueprint TOML",
    });
    await expect(kickstartTab).toBeVisible();
    await expect(blueprintTab).toBeVisible();

    // Kickstart is the default active tab — content from fixture
    await expect(modal.locator("pre")).toContainText("Kickstart user configuration");

    // Switch to Blueprint tab
    await blueprintTab.click();
    await expect(modal.locator("pre")).toContainText("Blueprint user configuration");
  });

  // ── 5. Redacted preview shows sensitive banner ───────────────────
  test("redacted preview — sensitive banner visible", async ({ page }) => {
    // Re-apply mocks with redacted preview override
    const redactedPath = path.join(
      FIXTURES_DIR,
      "single-host",
      "user-preview-redacted.json",
    );
    await applyMockApi(page, "single-host", {
      "/api/user-preview": redactedPath,
    });
    await page.goto("/");
    await expect(page.locator(".inspectah-statsbar")).toBeVisible();

    // Navigate back to Users & Groups
    await page
      .locator(".inspectah-layout__sidebar")
      .getByText("Users & Groups")
      .click();

    // Open preview
    const previewBtn = page.getByRole("button", {
      name: "Preview Artifacts",
    });
    await expect(previewBtn).toBeVisible();
    await previewBtn.click();

    // Modal opens
    const modal = page.locator(".pf-v6-c-modal-box");
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Sensitive banner (PF6 Alert) should show redaction notice
    const alert = modal.locator(".pf-v6-c-alert");
    await expect(alert).toBeVisible({ timeout: 5000 });
    await expect(alert).toContainText("Sensitive values are redacted");
  });

  // ── 6. Password mismatch shows error ─────────────────────────────
  test("password mismatch — client-side validation error", async ({
    page,
  }) => {
    const card = page.locator('[data-testid="user-card-testuser"]');
    await expect(card).toBeVisible();

    // Expand the card
    const expandBtn = card.locator(
      'button[aria-label="Expand testuser details"]',
    );
    await expandBtn.click();

    // Expand the password options
    await card.getByText("Password options").click();

    // Select "Set new password" radio
    const newPwRadio = card.locator(
      'input[name="password-testuser"][value="new"]',
    );
    await newPwRadio.click();

    // Fill in mismatched passwords
    const pwInput = card.locator("#new-pw-testuser");
    const confirmInput = card.locator("#confirm-pw-testuser");
    await expect(pwInput).toBeVisible();
    await expect(confirmInput).toBeVisible();

    await pwInput.fill("password123");
    await confirmInput.fill("different456");

    // Click "Set password" button
    const setBtn = card.getByRole("button", { name: "Set password" });
    await setBtn.click();

    // Error message should appear
    await expect(card.getByText("Passwords do not match.")).toBeVisible();
  });
});
