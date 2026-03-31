import { test, expect } from '@playwright/test';
import { FLEET_URL } from './helpers';

test.describe('Config Editor / File Browser', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(FLEET_URL);
    // Wait for the helper script to activate (enables interactive features)
    await page.locator('.helper-active').waitFor({ state: 'attached', timeout: 10_000 });
  });

  test('editor tab opens with file tree and viewer pane', async ({ page }) => {
    await page.click('a[data-tab="output_files"]');
    const section = page.locator('#section-output_files');
    await expect(section).toBeVisible();

    // Verify the editor drawer structure is present
    const editorTab = page.locator('#editor-tab');
    await expect(editorTab).toBeVisible();

    // Verify the file tree pane exists
    const tree = page.locator('#editor-tree');
    await expect(tree).toBeVisible();

    // Verify the viewer/content pane exists
    const content = page.locator('#editor-content');
    await expect(content).toBeVisible();
  });

  test('clicking a file in tree loads it in viewer', async ({ page }) => {
    await page.click('a[data-tab="output_files"]');
    await expect(page.locator('#editor-tab')).toBeVisible();

    // Click a non-variant file entry (e.g., nginx.conf or motd)
    const fileEntry = page.locator('.editor-single-file').first();
    await expect(fileEntry).toBeVisible();
    await fileEntry.click();

    // After clicking, the file path should be displayed in the toolbar
    const pathEl = page.locator('#editor-file-path');
    await expect(pathEl).not.toBeEmpty();

    // The clicked entry should get the pf-m-current class
    await expect(fileEntry).toHaveClass(/pf-m-current/);

    // The viewer pane should show content (readonly pre or CodeMirror)
    const readonlyContent = page.locator('#editor-readonly-content');
    await expect(readonlyContent).toBeVisible();
  });

  test('editing and saving a file marks state as dirty', async ({ page }) => {
    await page.click('a[data-tab="output_files"]');
    await expect(page.locator('#editor-tab')).toBeVisible();

    // Click an INCLUDED file (nginx.conf is included; motd is excluded).
    // Excluded files show "Switch to this variant" instead of "Edit".
    const fileEntry = page.locator('.editor-single-file[data-path="/etc/nginx/nginx.conf"]');
    await expect(fileEntry).toBeVisible();
    await fileEntry.click();

    // Wait for the Edit button to appear (only for included files)
    const editBtn = page.locator('#btn-edit');
    await expect(editBtn).toBeVisible();

    // Click Edit to enter edit mode (shows CodeMirror)
    await editBtn.click();

    // The CodeMirror container should become visible
    const cmContainer = page.locator('#editor-cm-container');
    await expect(cmContainer).toBeVisible();

    // The Save button should appear
    const saveBtn = page.locator('#btn-save');
    await expect(saveBtn).toBeVisible();

    // Type something into the CodeMirror editor
    const cmContent = page.locator('.cm-content[contenteditable="true"]');
    await cmContent.click();
    await page.keyboard.type('# test change');

    // Click Save
    await saveBtn.click();

    // After saving, the re-render button in the toolbar should be enabled
    // (the editor save makes the snapshot dirty)
    const rerender = page.locator('#btn-re-render');
    await expect(rerender).toBeEnabled();
  });
});
