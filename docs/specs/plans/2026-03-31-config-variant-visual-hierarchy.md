# Config Variant Visual Hierarchy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three-tier visual hierarchy (tied/auto-selected/clean) to config variant groups with chevron affordance and tie pre-expansion.

**Architecture:** Template-only changes to `_config.html.j2`, `_css.html.j2`, and `_js.html.j2`. No Python/renderer changes — the template context already provides variant group state (tied vs winner via `_group_variants()` and `_build_context()` in `html_report.py`). Chevron via CSS `::before` pseudo-element. Auto-selected badge as a PF6-styled inline label.

**Tech Stack:** Jinja2 templates, CSS custom properties, vanilla JS, PatternFly 6 tokens

**Spec:** `docs/specs/proposed/2026-03-31-config-variant-visual-hierarchy-design.md`

---

### Task 1: Auto-Selected Badge CSS + Chevron CSS

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`

- [ ] **Step 1: Add auto-selected badge CSS**

In `_css.html.j2`, after the existing `.variant-tie-badge` styles (~line 365), add:

```css
/* Auto-selected variant badge (Tier 2 — informational) */
.variant-auto-badge {
  background: rgba(43, 154, 243, 0.12);
  color: var(--pf-t--global--color--status--info--default, #2b9af3);
  padding: 0.15rem 0.5rem;
  border-radius: 3px;
  font-size: 0.75rem;
}

html:not(.pf-v6-theme-dark) .variant-auto-badge {
  background: rgba(43, 154, 243, 0.08);
  color: #0066cc;
}
```

- [ ] **Step 2: Add chevron CSS**

In `_css.html.j2`, after the `.fleet-variant-toggle` styles (~line 338), add:

```css
/* Chevron affordance for variant group expand/collapse */
.fleet-variant-toggle::before {
  content: "▶";
  display: inline-block;
  font-size: 0.6rem;
  margin-right: 0.4rem;
  transition: transform 150ms ease;
  color: var(--pf-t--global--text--color--subtle, #8a8d90);
}

.fleet-variant-toggle.expanded::before {
  transform: rotate(90deg);
}
```

- [ ] **Step 3: Verify existing tests still pass**

```bash
uv run --extra dev pytest -q
```

Expected: all tests pass (no functional changes yet, just CSS).

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_css.html.j2
git commit -m "feat(refine): Add auto-selected badge and chevron CSS

Tier 2 info-blue badge for auto-selected variant groups.
CSS ::before chevron on variant toggle with rotation on expand.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Template Changes — Badge + Chevron Rendering

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`

Before making changes, read the file to understand the current structure. Key elements:
- Line ~16: `.fleet-variant-group` parent row with `.variant-tied` class for ties
- Line ~29: `.fleet-variant-toggle` button inside the group header
- Line ~31: existing `.variant-tie-badge` for tied groups
- Line ~40: `.fleet-variant-children` row containing the child variants

- [ ] **Step 1: Add auto-selected badge to template**

In `_config.html.j2`, find where the `.variant-tie-badge` is rendered (inside the `.fleet-variant-toggle` button). After the tie badge conditional, add the auto-selected badge:

The tie badge is shown when the group is tied (all variants have `include=false`). The auto-selected badge should show when the group is NOT tied and has more than one variant (a winner was auto-selected).

Look at how the template determines if a group is tied — it likely checks if any variant in the group has `include=true`. If none do, it's a tie. If exactly one does, it's auto-selected.

Add after the existing tie badge block:

```jinja2
{% if not is_tied and variant_count > 1 %}
<span class="variant-auto-badge">{{ variant_count }} variants · auto-selected</span>
{% endif %}
```

**Note:** The exact variable names (`is_tied`, `variant_count`) depend on what the template context provides. Read the template to find the actual variables used in the tie badge conditional and mirror that pattern.

- [ ] **Step 2: Add `aria-expanded` to the toggle button**

On the `.fleet-variant-toggle` button element, add:

```
aria-expanded="false"
```

This will be toggled by JS when expanding/collapsing.

- [ ] **Step 3: Verify tests pass**

```bash
uv run --extra dev pytest -q
```

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2
git commit -m "feat(refine): Render auto-selected badge and aria-expanded on variant groups

Show 'N variants · auto-selected' badge on non-tied variant groups.
Add aria-expanded attribute for accessibility.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: JS — Chevron Toggle + Tie Pre-Expansion

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Update the variant toggle click handler**

Find the variant toggle click handler (~line 1641). It currently toggles the children row's `display` style. Update it to also:
1. Toggle the `.expanded` class on the toggle button
2. Update the `aria-expanded` attribute

The current handler looks approximately like:
```javascript
document.addEventListener('click', function(e) {
  var toggle = e.target.closest('.fleet-variant-toggle');
  if (!toggle) return;
  var row = toggle.closest('tr');
  var next = row.nextElementSibling;
  if (next && next.classList.contains('fleet-variant-children')) {
    if (next.style.display === 'table-row') {
      next.style.display = '';
      // ADD: toggle.classList.remove('expanded');
      // ADD: toggle.setAttribute('aria-expanded', 'false');
    } else {
      next.style.display = 'table-row';
      // ADD: toggle.classList.add('expanded');
      // ADD: toggle.setAttribute('aria-expanded', 'true');
    }
  }
});
```

Add the `.expanded` class toggle and `aria-expanded` update to both branches.

- [ ] **Step 2: Add tie pre-expansion on page load**

After the variant toggle handler, add a `DOMContentLoaded` block (or add to an existing one) that pre-expands all tied variant groups:

```javascript
// Pre-expand tied variant groups on page load
document.querySelectorAll('.variant-tied .fleet-variant-toggle').forEach(function(toggle) {
  var row = toggle.closest('tr');
  var next = row.nextElementSibling;
  if (next && next.classList.contains('fleet-variant-children')) {
    next.style.display = 'table-row';
    toggle.classList.add('expanded');
    toggle.setAttribute('aria-expanded', 'true');
  }
});
```

This fires once on load. It does NOT re-fire after prevalence slider changes (those are preview-only — ties are recomputed on re-render).

- [ ] **Step 3: Verify tests pass**

```bash
uv run --extra dev pytest -q
```

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "feat(refine): Add chevron toggle behavior and pre-expand tied groups

Toggle .expanded class and aria-expanded on variant expand/collapse.
Pre-expand all tied variant groups on page load so users see
blocking items immediately.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Renderer Verification

**Files:**
- Read only: `src/yoinkc/renderers/html_report.py`

This is a verification task, not a code change. The spec requires confirming the renderer provides correct variant state.

- [ ] **Step 1: Verify variant state in template context**

Read `html_report.py`, specifically `_build_context()` (~line 667) and `_group_variants()` (~line 422).

Confirm:
1. The template receives variant group data with `include` state for each variant
2. Tie detection (all variants `include=false`) is computed correctly
3. The winner (one variant with `include=true`) is identifiable from template data

- [ ] **Step 2: Verify post-re-render consistency**

Start refine on the fleet fixture and test:
```bash
uv run python tests/e2e/generate-fixtures.py
uv run yoinkc refine tests/e2e/fixtures/fleet-3host.tar.gz --no-browser --port 9200 &
```

Open http://localhost:9200, make a change, re-render, verify that:
- Tied groups still show as tied after re-render
- Auto-selected groups still show the correct winner after re-render

Kill the server after verification.

- [ ] **Step 3: Document findings**

If everything checks out, no changes needed — move to Task 5.

If any issues found, report as BLOCKED with specifics.

---

### Task 5: Revert Tie Styling to Gold

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`

The UI overhaul changed tie styling from gold (`#cc8800`) to red/danger (`#c9190b`). The spec requires reverting to gold.

- [ ] **Step 1: Find and update tie badge colors**

In `_css.html.j2`, find `.variant-tie-badge` and `.variant-tied` styles. Replace red/danger token references with gold:

```css
.variant-tie-badge {
  background: rgba(204, 136, 0, 0.12);
  color: #cc8800;
  /* ... keep existing padding, border-radius, font-size, font-weight */
  border: 1px solid rgba(204, 136, 0, 0.3);
}

html:not(.pf-v6-theme-dark) .variant-tie-badge {
  background: rgba(204, 136, 0, 0.08);
  color: #995500;
}
```

Also update `.variant-tied` row styling if it uses red:

```css
.variant-tied {
  border-top: 2px solid rgba(204, 136, 0, 0.4);
  background: rgba(204, 136, 0, 0.04);
}
```

- [ ] **Step 2: Update tie badge text**

In `_config.html.j2`, find the `.variant-tie-badge` text. Change from "tied — choose one" to "tied — compare & choose" if not already using that text.

- [ ] **Step 3: Update summary callout colors if needed**

In `_summary.html.j2`, if the `.summary-ties-callout` uses red/danger styling, update to gold to match:

```css
.summary-ties-callout {
  color: #cc8800;
  /* ... keep existing layout */
}
```

- [ ] **Step 4: Verify tests pass**

```bash
uv run --extra dev pytest -q
```

Some tests may reference specific colors or badge text — update assertions if needed.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_css.html.j2 src/yoinkc/templates/report/_config.html.j2 src/yoinkc/templates/report/_summary.html.j2
git commit -m "fix(refine): Revert tie styling from red to gold

Ties are decision points, not errors. Gold (#cc8800) matches the
Mar 22 auto-selection spec. Badge text: 'tied — compare & choose'.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: E2E Tests

**Files:**
- Modify: `tests/e2e/tests/variant-selection.spec.ts`

- [ ] **Step 1: Add tier hierarchy tests**

Add these tests to the existing `variant-selection.spec.ts`:

```typescript
test('tied groups are pre-expanded on load', async ({ page }) => {
  await page.goto(FLEET_URL);
  await page.locator('.pf-v6-c-nav__link[data-tab="config"]').click();

  // Find a tied variant group (has .variant-tied class)
  const tiedGroup = page.locator('.variant-tied .fleet-variant-toggle').first();
  await expect(tiedGroup).toBeVisible();
  await expect(tiedGroup).toHaveClass(/expanded/);

  // Children should be visible
  const tiedRow = tiedGroup.locator('xpath=ancestor::tr[1]');
  const children = tiedRow.locator('+ .fleet-variant-children');
  await expect(children).toBeVisible();
});

test('auto-selected groups show blue badge', async ({ page }) => {
  await page.goto(FLEET_URL);
  await page.locator('.pf-v6-c-nav__link[data-tab="config"]').click();

  const autoBadge = page.locator('.variant-auto-badge').first();
  await expect(autoBadge).toBeVisible();
  await expect(autoBadge).toContainText('auto-selected');
});

test('clean files have no variant badges', async ({ page }) => {
  await page.goto(FLEET_URL);
  await page.locator('.pf-v6-c-nav__link[data-tab="config"]').click();

  // Files without variant groups should have no badges
  // Count total config rows vs rows with badges
  const allRows = page.locator('#section-config tr[data-snap-section="config"]');
  const badgeRows = page.locator('#section-config .variant-auto-badge, #section-config .variant-tie-badge');
  const totalRows = await allRows.count();
  const badgeCount = await badgeRows.count();
  expect(badgeCount).toBeLessThan(totalRows);
});

test('chevron reflects expand/collapse state', async ({ page }) => {
  await page.goto(FLEET_URL);
  await page.locator('.pf-v6-c-nav__link[data-tab="config"]').click();

  // Find a collapsed auto-selected group
  const toggle = page.locator('.fleet-variant-toggle:not(.expanded)').first();
  await expect(toggle).toBeVisible();

  // Click to expand
  await toggle.click();
  await expect(toggle).toHaveClass(/expanded/);

  // Children should be visible
  const row = toggle.locator('xpath=ancestor::tr[1]');
  const children = row.locator('+ .fleet-variant-children');
  await expect(children).toBeVisible();

  // Click again to collapse
  await toggle.click();
  await expect(toggle).not.toHaveClass(/expanded/);
  await expect(children).not.toBeVisible();
});

test('auto badge readable in light mode', async ({ page }) => {
  await page.goto(FLEET_URL);

  // Ensure light mode
  const html = page.locator('html');
  const isDark = await html.evaluate(el => el.classList.contains('pf-v6-theme-dark'));
  if (isDark) {
    await page.locator('#theme-toggle').click();
  }

  await page.locator('.pf-v6-c-nav__link[data-tab="config"]').click();

  const badge = page.locator('.variant-auto-badge').first();
  await expect(badge).toBeVisible();

  const contrast = await badge.evaluate(el => {
    const style = window.getComputedStyle(el);
    return { color: style.color, bg: style.backgroundColor };
  });
  expect(contrast.color).not.toEqual(contrast.bg);
});
```

- [ ] **Step 2: Run E2E tests**

```bash
cd tests/e2e && npx playwright test tests/variant-selection.spec.ts
```

- [ ] **Step 3: Add Python render tests**

In `tests/test_fleet_output.py`, add to `TestVariantTieResolution`:

```python
def test_auto_selected_group_renders_auto_badge(self, tmp_path):
    """Auto-selected variant group must render .variant-auto-badge."""
    env = Environment(autoescape=True)
    # Use a snapshot where one variant has higher count (auto-selected)
    snap = InspectionSnapshot(
        schema_version=SCHEMA_VERSION,
        os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel", platform_id="platform:el9"),
        meta={"fleet": {"source_hosts": ["h1", "h2", "h3"], "total_hosts": 3, "min_prevalence": 1}},
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED,
                          content="winner", include=True,
                          fleet=FleetPrevalence(count=2, total=3, hosts=["h1", "h2"])),
            ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED,
                          content="loser", include=False,
                          fleet=FleetPrevalence(count=1, total=3, hosts=["h3"])),
        ]),
    )
    html_report.render(snap, env, tmp_path, refine_mode=True)
    html = (tmp_path / "report.html").read_text()
    assert "variant-auto-badge" in html
    assert "auto-selected" in html

def test_tied_group_has_no_auto_badge(self, tmp_path):
    """Tied variant group must NOT render .variant-auto-badge."""
    env = Environment(autoescape=True)
    snap = self._make_tied_snapshot(resolved=False)
    html_report.render(snap, env, tmp_path, refine_mode=True)
    html = (tmp_path / "report.html").read_text()
    assert "variant-auto-badge" not in html
```

- [ ] **Step 4: Run all tests**

```bash
uv run --extra dev pytest -q
cd tests/e2e && npx playwright test
```

- [ ] **Step 5: Commit**

```bash
git add tests/e2e/tests/variant-selection.spec.ts tests/test_fleet_output.py
git commit -m "test(refine): Add three-tier variant hierarchy E2E and Python tests

Tied pre-expanded, auto-selected badge visible, clean files no badge,
chevron expand/collapse, light mode contrast check. Python: auto badge
renders for winner groups, not for tied groups.

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Self-Review Checklist

- [x] **Spec coverage:** All spec requirements have corresponding tasks — auto-selected badge (T1+T2), chevron (T1+T3), pre-expand ties (T3), gold tie revert (T5), E2E tests (T6), Python tests (T6), renderer verification (T4), aria-expanded (T2+T3)
- [x] **No placeholders:** Every task has concrete code
- [x] **Type consistency:** `.fleet-variant-toggle`, `.expanded`, `.variant-auto-badge`, `.variant-tie-badge`, `.fleet-variant-children`, `.variant-tied` used consistently throughout
- [x] **Selector contract:** Class names match the spec's test table exactly
- [x] **Scope:** Config-only, no editor/services/containers changes
- [x] **Gold tie colors:** Task 5 explicitly reverts red→gold
