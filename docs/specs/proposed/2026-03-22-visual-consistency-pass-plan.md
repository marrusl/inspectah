# Visual Consistency Pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the yoinkc HTML report feel like one cohesive product by normalizing table spacing, cleaning up the config tab, restructuring the packages tab, and fixing column inconsistencies.

**Architecture:** CSS-first changes (global spacing, inline style migration) followed by template restructuring (config columns, pencil reorder, packages card merge). All changes land on a `visual-consistency-pass` feature branch. TDD via rendered-HTML assertions using the existing `run_all_renderers()` pipeline.

**Tech Stack:** Jinja2 templates, PatternFly 6 CSS, vanilla JS, pytest

**Spec:** `docs/specs/proposed/2026-03-22-visual-consistency-pass-design.md`

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `src/yoinkc/templates/report/_css.html.j2` | Modify | Global spacing vars, new CSS classes, remove diff CSS |
| `src/yoinkc/templates/report/_config.html.j2` | Modify | Drop rpm-Va + diff columns, add permissions badge, move pencil, fix colspan/fillers, update inner variant tables |
| `src/yoinkc/templates/report/_services.html.j2` | Modify | Move pencil column left (drop-in overrides table) |
| `src/yoinkc/templates/report/_containers.html.j2` | Modify | Move pencil column left (quadlets table) |
| `src/yoinkc/templates/report/_packages.html.j2` | Modify | Merge repo card into dep tree, expandable repo headers |
| `src/yoinkc/templates/report/_js.html.j2` | Modify | Migrate applyRepoCascade selectors, add recalcTriageCounts call, repo expand/collapse |
| `src/yoinkc/templates/report/_scheduled_jobs.html.j2` | Modify | Add fit-content to timers Schedule column |
| `src/yoinkc/templates/report/_network.html.j2` | Modify | Add fit-content to connections columns |
| `src/yoinkc/renderers/html_report.py` | Modify | Remove `_render_diff_html()`, remove `diff_html` field |
| `tests/test_visual_consistency.py` | Create | All tests for this feature branch |

---

## Task 0: Create Feature Branch

- [ ] **Step 1: Create and switch to feature branch**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git checkout -b visual-consistency-pass
```

- [ ] **Step 2: Verify clean state**

```bash
git status
```

Expected: `On branch visual-consistency-pass`, clean working tree.

---

## Task 1: Part A — Global Table Spacing

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`
- Create: `tests/test_visual_consistency.py`

- [ ] **Step 1: Write failing test — global spacing CSS variables present in rendered HTML**

Create `tests/test_visual_consistency.py`:

```python
"""Tests for the visual consistency pass."""

import tempfile
from pathlib import Path

import pytest

from yoinkc.renderers import run_all_renderers
from yoinkc.schema import (
    ConfigFile,
    ConfigSection,
    InspectionSnapshot,
    OsRelease,
)


def _render(refine_mode=False, **snapshot_kwargs) -> str:
    """Render a report and return the HTML string."""
    defaults = {
        "meta": {"host_root": "/host"},
        "os_release": OsRelease(
            name="RHEL", version_id="9", pretty_name="RHEL 9"
        ),
    }
    defaults.update(snapshot_kwargs)
    snapshot = InspectionSnapshot(**defaults)
    with tempfile.TemporaryDirectory() as tmp:
        run_all_renderers(snapshot, Path(tmp), refine_mode=refine_mode)
        return (Path(tmp) / "report.html").read_text()


class TestGlobalSpacing:
    """Part A: global table spacing via CSS variables."""

    def test_relaxed_padding_block_start(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingBlockStart" in html

    def test_relaxed_padding_block_end(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingBlockEnd" in html

    def test_relaxed_padding_inline_start(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingInlineStart" in html

    def test_relaxed_padding_inline_end(self):
        html = _render()
        assert "--pf-v6-c-table--cell--PaddingInlineEnd" in html
```

- [ ] **Step 2: Run test to verify it fails**

```bash
pytest tests/test_visual_consistency.py::TestGlobalSpacing -v
```

Expected: 4 FAIL — CSS variables not yet in the stylesheet.

- [ ] **Step 3: Add global spacing override to CSS**

In `src/yoinkc/templates/report/_css.html.j2`, after the reduced-motion block (after line 12), add:

```css
/* Global table spacing — relaxed density */
.pf-v6-c-table {
  --pf-v6-c-table--cell--PaddingBlockStart: var(--pf-t--global--spacer--sm);
  --pf-v6-c-table--cell--PaddingBlockEnd: var(--pf-t--global--spacer--sm);
  --pf-v6-c-table--cell--PaddingInlineStart: var(--pf-t--global--spacer--md);
  --pf-v6-c-table--cell--PaddingInlineEnd: var(--pf-t--global--spacer--md);
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
pytest tests/test_visual_consistency.py::TestGlobalSpacing -v
```

Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_css.html.j2 tests/test_visual_consistency.py
git commit -m "style: add relaxed global table spacing via PF6 CSS variables"
```

---

## Task 2: Part E — Inline Style Cleanup

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`
- Modify: templates containing inline styles (found via grep)
- Test: `tests/test_visual_consistency.py`

- [ ] **Step 1: Grep to find exact inline style locations**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
grep -rn 'style="margin-left: 8px; cursor: pointer;"' src/yoinkc/templates/report/
grep -rn 'style="margin: 0;"' src/yoinkc/templates/report/
grep -rn 'style="color: var(--pf-v6-global--Color--200);"' src/yoinkc/templates/report/
```

Record exact files and line numbers.

- [ ] **Step 2: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
class TestInlineStyleCleanup:
    """Part E: inline styles migrated to CSS classes."""

    def test_css_classes_defined(self):
        html = _render()
        assert ".fleet-variant-toggle" in html
        assert ".fleet-variant-table" in html or ".fleet-prevalence" in html
        assert ".variant-index" in html

    def test_no_inline_margin_left_cursor(self):
        # Grep the raw template files — inline styles live in templates,
        # not in rendered output (which may lack fleet data in test fixtures).
        import glob
        content_templates = glob.glob(
            "src/yoinkc/templates/report/_*.html.j2"
        )
        for path in content_templates:
            if path.endswith("_css.html.j2") or path.endswith("_js.html.j2"):
                continue
            text = open(path).read()
            assert 'style="margin-left: 8px; cursor: pointer;"' not in text, (
                f"inline margin-left/cursor still in {path}"
            )

    def test_no_inline_margin_zero(self):
        import glob
        content_templates = glob.glob(
            "src/yoinkc/templates/report/_*.html.j2"
        )
        for path in content_templates:
            if path.endswith("_css.html.j2") or path.endswith("_js.html.j2"):
                continue
            text = open(path).read()
            assert 'style="margin: 0;"' not in text, (
                f"inline margin:0 still in {path}"
            )

    def test_no_inline_variant_color(self):
        import glob
        content_templates = glob.glob(
            "src/yoinkc/templates/report/_*.html.j2"
        )
        for path in content_templates:
            if path.endswith("_css.html.j2") or path.endswith("_js.html.j2"):
                continue
            text = open(path).read()
            assert 'style="color: var(--pf-v6-global--Color--200);"' not in text, (
                f"inline variant color still in {path}"
            )
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestInlineStyleCleanup -v
```

Expected: FAIL — CSS classes not yet defined, inline styles still in template files.

- [ ] **Step 4: Add CSS classes to `_css.html.j2`**

Add near the fleet styling section:

```css
/* Fleet variant styling (migrated from inline styles) */
.fleet-variant-toggle { margin-left: 8px; cursor: pointer; }
.fleet-variant-table { margin: 0; }
.variant-index { color: var(--pf-v6-global--Color--200); }
```

- [ ] **Step 5: Replace inline styles with classes in templates**

For each file found in step 1:
- Replace `style="margin-left: 8px; cursor: pointer;"` with `class="fleet-variant-toggle"`
- Replace `style="margin: 0;"` with `class="fleet-variant-table"`
- Replace `style="color: var(--pf-v6-global--Color--200);"` with `class="variant-index"`

Preserve any existing classes — merge if needed (e.g., `class="existing fleet-variant-toggle"`).

- [ ] **Step 6: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestInlineStyleCleanup -v
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: migrate 3 inline styles to CSS classes"
```

---

## Task 3: Part D — Column Consistency

**Files:**
- Modify: `src/yoinkc/templates/report/_scheduled_jobs.html.j2`
- Modify: `src/yoinkc/templates/report/_network.html.j2`
- Test: `tests/test_visual_consistency.py`

- [ ] **Step 1: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
class TestColumnConsistency:
    """Part D: fit-content on narrow columns."""

    def test_timers_schedule_has_fit_content(self):
        html = _render()
        # The timers table currently has no fit-content on any column.
        # After fix, Schedule header should have pf-m-fit-content.
        # Find the timers section and check its table headers.
        import re
        # Timers table is the first table in the scheduled-jobs section
        sched_start = html.find('id="section-scheduled-jobs"')
        if sched_start == -1:
            pytest.skip("no scheduled-jobs section in fixture")
        sched_html = html[sched_start:sched_start + 5000]
        # First table in scheduled jobs = timers
        first_table = sched_html[sched_html.find("<table"):sched_html.find("</table>") + 8]
        assert "pf-m-fit-content" in first_table

    def test_connections_method_has_fit_content(self):
        html = _render()
        net_start = html.find('id="section-network"')
        if net_start == -1:
            pytest.skip("no network section in fixture")
        net_html = html[net_start:net_start + 5000]
        # Connections is the first table in network section
        first_table = net_html[net_html.find("<table"):net_html.find("</table>") + 8]
        assert "pf-m-fit-content" in first_table
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestColumnConsistency -v
```

Expected: FAIL (or skip if fixture lacks data — in that case, the implementation is still correct and tests can be adjusted after).

- [ ] **Step 3: Add fit-content to timers Schedule column**

In `src/yoinkc/templates/report/_scheduled_jobs.html.j2`, find the timers table header row. Change:

```html
<th scope="col">Schedule</th>
```

to:

```html
<th class="pf-m-fit-content" scope="col">Schedule</th>
```

- [ ] **Step 4: Add fit-content to connections columns**

In `src/yoinkc/templates/report/_network.html.j2`, find the Connections table header row. Change:

```html
<th scope="col">Method</th><th scope="col">Type</th><th scope="col">Deployment</th>
```

to:

```html
<th class="pf-m-fit-content" scope="col">Method</th><th class="pf-m-fit-content" scope="col">Type</th><th class="pf-m-fit-content" scope="col">Deployment</th>
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestColumnConsistency -v
```

Expected: PASS.

- [ ] **Step 6: Run full test suite to check for regressions**

```bash
pytest tests/ -v --tb=short
```

Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_scheduled_jobs.html.j2 src/yoinkc/templates/report/_network.html.j2 tests/test_visual_consistency.py
git commit -m "style: add pf-m-fit-content to timers and connections columns"
```

---

## Task 4: Part B1-B2 — Drop rpm-Va and Diff Columns + Backend Cleanup

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` — remove `_render_diff_html()` and `diff_html` field
- Modify: `src/yoinkc/templates/report/_config.html.j2` — remove columns
- Modify: `src/yoinkc/templates/report/_css.html.j2` — remove diff CSS classes
- Test: `tests/test_visual_consistency.py`

- [ ] **Step 1: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
from yoinkc.schema import RpmSection


class TestConfigColumnCleanup:
    """Part B1-B2: rpm-Va and diff columns removed."""

    def _render_with_config(self, refine_mode=False) -> str:
        return _render(
            refine_mode=refine_mode,
            config=ConfigSection(
                files=[
                    ConfigFile(
                        path="/etc/sshd/sshd_config",
                        rpm_va_flags="S.5.....",
                        diff_against_rpm="--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new",
                        kind="sshd",
                        category="security",
                        include=True,
                    ),
                ]
            ),
        )

    def test_rpm_va_column_removed(self):
        html = self._render_with_config()
        # The header "rpm -Va flags" or "rpm -Va" should not be present
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "rpm -Va" not in config_html
        assert "S.5....." not in config_html

    def test_diff_column_removed(self):
        html = self._render_with_config()
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "diff-view" not in config_html
        assert "diff-add" not in config_html

    def test_diff_css_classes_removed(self):
        html = self._render_with_config()
        assert ".diff-view" not in html
        assert ".diff-hdr" not in html
        assert ".diff-hunk" not in html
        assert ".diff-add" not in html
        assert ".diff-del" not in html

    def test_render_diff_html_function_removed(self):
        """The _render_diff_html helper should no longer exist."""
        from yoinkc.renderers import html_report
        assert not hasattr(html_report, "_render_diff_html")
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestConfigColumnCleanup -v
```

Expected: FAIL — columns and function still exist.

- [ ] **Step 3: Remove `_render_diff_html()` from `html_report.py`**

Delete the `_render_diff_html()` function (approximately lines 347-366). Also remove the line in `_prepare_config_files()` that calls it — find the line like:

```python
"diff_html": _render_diff_html(f.diff_against_rpm or ""),
```

and remove it entirely.

- [ ] **Step 4: Remove diff CSS classes from `_css.html.j2`**

Delete lines 80-84 (the `.diff-view`, `.diff-add`, `.diff-del`, `.diff-hdr`, `.diff-hunk` rules).

- [ ] **Step 5: Remove rpm-Va and Diff columns from `_config.html.j2`**

In the main table header (`<thead>`), remove:
- `<th class="pf-m-fit-content" scope="col">rpm -Va flags</th>`
- `<th scope="col">Diff</th>`

In each data row (`<tbody>`), remove the corresponding `<td>` cells:
- The cell that renders `{{ f.flags }}` or `{{ v.item.flags }}`
- The cell that renders `{{ v.item.diff_html|safe }}` or similar

**For variant/fleet rows:** Also remove the rpm-Va and Diff columns from the inner compact variant tables. Remove corresponding filler `<td></td>` cells from parent/group rows (reduce from 4 fillers to 2 — keep Kind and Category fillers only).

**Update colspan:** Find `colspan="10"` and recalculate. New base column count = checkbox(1) + path(1) + kind(1) + category(1) = 4. With fleet: +1. With refine: +1. Update to match.

- [ ] **Step 6: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestConfigColumnCleanup -v
```

Expected: PASS.

- [ ] **Step 7: Run full test suite**

```bash
pytest tests/ -v --tb=short
```

Fix any regressions (other tests may assert on diff_html presence).

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: remove rpm-Va and diff preview columns from config tab

The editor tab is the canonical surface for viewing and editing diffs.
Removes _render_diff_html() and associated CSS classes."
```

---

## Task 5: Part B3 — Permissions Changed Badge

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`
- Test: `tests/test_visual_consistency.py`

- [ ] **Step 1: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
class TestPermissionsBadge:
    """Part B3: permissions badge when rpm-Va flags contain M, U, or G."""

    def _render_config(self, flags: str) -> str:
        return _render(
            config=ConfigSection(
                files=[
                    ConfigFile(
                        path="/etc/test.conf",
                        rpm_va_flags=flags,
                        kind="test",
                        category="system",
                        include=True,
                    ),
                ]
            ),
        )

    def test_badge_shown_for_mode_change(self):
        html = self._render_config("SM5.....")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_shown_for_user_change(self):
        html = self._render_config("..5..U..")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_shown_for_group_change(self):
        html = self._render_config("..5...G.")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" in config_html.lower()

    def test_badge_not_shown_for_content_only(self):
        html = self._render_config("S.5.....")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" not in config_html.lower()

    def test_badge_not_shown_for_empty_flags(self):
        html = self._render_config("")
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "permissions" not in config_html.lower()
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestPermissionsBadge -v
```

Expected: FAIL — badge not yet rendered.

- [ ] **Step 3: Add permissions badge to config template**

In `_config.html.j2`, in the Kind `<td>` cell (where the kind badge is rendered), add after the existing kind badge:

```jinja2
{%- if f.flags and ('M' in f.flags or 'U' in f.flags or 'G' in f.flags) %}
<span class="pf-v6-c-label pf-m-compact pf-m-gold"><span class="pf-v6-c-label__content">permissions</span></span>
{%- endif %}
```

Do the same in fleet variant child rows (the inner compact variant table), using the variant item's flags field.

- [ ] **Step 4: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestPermissionsBadge -v
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 tests/test_visual_consistency.py
git commit -m "feat: add permissions-changed badge on config rows

Shown when rpm-Va flags contain M (mode), U (user), or G (group).
Surfaces permission/ownership changes without a dedicated column."
```

---

## Task 6: Part B4-B5 — Pencil Icon Reorder

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`
- Modify: `src/yoinkc/templates/report/_services.html.j2`
- Modify: `src/yoinkc/templates/report/_containers.html.j2`
- Test: `tests/test_visual_consistency.py`

- [ ] **Step 1: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
import re


class TestPencilReorder:
    """Part B4-B5: pencil icon between checkbox and path."""

    def _render_refine_config(self) -> str:
        return _render(
            refine_mode=True,
            config=ConfigSection(
                files=[
                    ConfigFile(
                        path="/etc/test.conf",
                        rpm_va_flags="S.5.....",
                        kind="test",
                        category="system",
                        include=True,
                    ),
                ]
            ),
        )

    def test_config_pencil_before_path(self):
        html = self._render_refine_config()
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        # Find first data row: checkbox td, then pencil td, then path td
        # The pencil (editor-icon) should appear before the path text
        pencil_pos = config_html.find("editor-icon")
        path_pos = config_html.find("/etc/test.conf")
        assert pencil_pos != -1, "pencil icon not found in config section"
        assert path_pos != -1, "path not found in config section"
        assert pencil_pos < path_pos, "pencil should appear before path in DOM order"

    def test_pencil_not_in_readonly_mode(self):
        html = _render(
            refine_mode=False,
            config=ConfigSection(
                files=[
                    ConfigFile(
                        path="/etc/test.conf",
                        rpm_va_flags="S.5.....",
                        kind="test",
                        category="system",
                        include=True,
                    ),
                ]
            ),
        )
        config_start = html.find('id="section-config"')
        config_html = html[config_start:config_start + 5000]
        assert "editor-icon" not in config_html
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestPencilReorder -v
```

Expected: `test_config_pencil_before_path` FAIL (pencil is after path in current DOM order).

- [ ] **Step 3: Reorder pencil column in `_config.html.j2`**

In the `<thead>`, move the pencil `<th>` from last position to position 2 (after checkbox, before Path):

```html
<th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th>
{%- if refine_mode %}<th class="pf-m-fit-content" scope="col"></th>{%- endif %}
<th scope="col">Path</th>
<th class="pf-m-fit-content" scope="col">Kind</th>
<th class="pf-m-fit-content" scope="col">Category</th>
{%- if fleet_meta %}<th scope="col">Fleet</th>{%- endif %}
```

In each `<tbody>` row, move the pencil `<td>` to the same position. Update filler cells and colspan values accordingly.

In the inner variant tables, apply the same column reorder.

- [ ] **Step 4: Reorder pencil column in `_services.html.j2`**

Drop-in Overrides table only (State Changes table has no pencil). Move the pencil `<th>` and `<td>` from last position to position 2:

```html
<th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th>
{%- if refine_mode %}<th class="pf-m-fit-content" scope="col"></th>{%- endif %}
<th scope="col">Parent unit</th>
<th scope="col">Drop-in path</th>
<th scope="col">Content</th>
{%- if fleet_meta %}<th scope="col">Fleet</th>{%- endif %}
```

- [ ] **Step 5: Reorder pencil column in `_containers.html.j2`**

Quadlets table only (Compose Services and Running Containers tables have no pencil). Move the pencil `<th>` and `<td>` from last position to position 2:

```html
<th class="pf-v6-c-table__check pf-m-fit-content" scope="col"></th>
{%- if refine_mode %}<th class="pf-m-fit-content" scope="col"></th>{%- endif %}
<th scope="col">Unit</th>
<th scope="col">Image</th>
<th scope="col">Path</th>
<th scope="col">Content</th>
{%- if fleet_meta %}<th scope="col">Fleet</th>{%- endif %}
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestPencilReorder -v
```

Expected: PASS.

- [ ] **Step 7: Run full test suite**

```bash
pytest tests/ -v --tb=short
```

Fix any regressions (editor tests may assert on pencil position).

- [ ] **Step 8: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2 src/yoinkc/templates/report/_services.html.j2 src/yoinkc/templates/report/_containers.html.j2 tests/test_visual_consistency.py
git commit -m "style: move pencil icon between checkbox and path

Groups interactive controls (toggle + edit) on the left side of the row.
Applied to config, services drop-in, and containers quadlet tables."
```

---

## Task 7: Part C — Packages Tab Restructure

**Files:**
- Modify: `src/yoinkc/renderers/html_report.py` — prepare `repo_file_lookup` for template
- Modify: `src/yoinkc/templates/report/_packages.html.j2`
- Modify: `src/yoinkc/templates/report/_js.html.j2`
- Test: `tests/test_visual_consistency.py`

This is the most complex task. The work is:
1. Prepare `repo_file_lookup` dict in the renderer
2. Remove the separate Repositories card
3. Promote repo group headers to expandable/collapsible rows with toggles
4. Migrate `applyRepoCascade()` selectors to new DOM structure
5. Add `recalcTriageCounts()` call after cascade
6. Preserve default-repo disabled toggle behavior

- [ ] **Step 1: Write failing tests**

Add to `tests/test_visual_consistency.py`:

```python
from yoinkc.schema import RepoFile, PackageEntry


class TestPackagesRestructure:
    """Part C: merge repo card into dep tree."""

    def _render_packages(self, refine_mode=False) -> str:
        return _render(
            refine_mode=refine_mode,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(
                        name="httpd",
                        version="2.4.57",
                        release="8.el9",
                        arch="x86_64",
                        source_repo="appstream",
                        include=True,
                    ),
                ],
                repo_files=[
                    RepoFile(
                        path="/etc/yum.repos.d/redhat.repo",
                        include=True,
                        is_default_repo=True,
                    ),
                    RepoFile(
                        path="/etc/yum.repos.d/epel.repo",
                        include=True,
                        is_default_repo=False,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )

    def test_separate_repo_card_removed(self):
        html = self._render_packages()
        assert "card-pkg-repos" not in html

    def test_repo_headers_in_dep_tree(self):
        html = self._render_packages()
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 10000]
        # Repo names should appear as group headers in the dep tree
        assert "table-group-header" in pkg_html

    def test_default_repo_toggle_disabled(self):
        html = self._render_packages(refine_mode=True)
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 10000]
        # Default repo toggle should be disabled
        assert "Default distribution repository" in pkg_html
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
pytest tests/test_visual_consistency.py::TestPackagesRestructure -v
```

Expected: `test_separate_repo_card_removed` FAIL (card still present).

- [ ] **Step 3: Prepare `repo_file_lookup` in `html_report.py`**

In the renderer function that prepares the packages template context (where `repo_groups` is built), add a `repo_file_lookup` dict mapping repo group names to their corresponding `RepoFile` entries with original indices:

```python
repo_file_lookup = {}
if snapshot.rpm and snapshot.rpm.repo_files:
    for idx, rf in enumerate(snapshot.rpm.repo_files):
        # Match repo group names to repo files by checking if the repo name
        # appears in the file path (e.g., "epel" in "/etc/yum.repos.d/epel.repo")
        for repo_name in repo_groups:
            if repo_name.lower() in rf.path.lower():
                rf_copy = rf  # or a dict with the fields needed
                repo_file_lookup[repo_name] = {"file": rf, "index": idx}
```

Pass `repo_file_lookup` to the template context alongside `repo_groups`.

- [ ] **Step 4: Remove the Repositories card from `_packages.html.j2`**

Delete the entire `card-pkg-repos` block (approximately lines 47-76 in current template). This removes the separate Repositories card with its `#pkg-repo-table`.

- [ ] **Step 5: Add repo toggle to dep tree group headers**

In `_packages.html.j2`, find the repo group header row (the `<tr>` with `table-group-header` class, approximately line 90). Currently:

```html
<tr><th colspan="{{ 6 if fleet_meta else 5 }}" class="table-group-header{{ ' table-group-header-first' if loop.first else '' }}">{{ repo_name }} ({{ entries|length }} package{{ 's' if entries|length != 1 else '' }})</th></tr>
```

Replace with a row that includes the repo toggle. The toggle should:
- Target `data-snap-section="rpm" data-snap-list="repo_files"` using the corresponding repo_file's index
- Be disabled with title for default repos (`is_default_repo`)
- Include expand/collapse chevron

You'll need to match `repo_name` back to the corresponding `repo_files` entry. Add a lookup: iterate `snapshot.rpm.repo_files` to find the matching entry by checking if the repo name appears in the repo file path or content.

The exact structure for the new header row:

```html
<tr class="table-group-header{{ ' table-group-header-first' if loop.first else '' }}" data-repo-group="{{ repo_name }}">
  <td class="pf-v6-c-table__check">
    {%- set repo_file = repo_file_lookup.get(repo_name) %}
    {%- if repo_file is not none %}
    <label class="pf-v6-c-switch include-toggle-wrap">
      <input type="checkbox" class="pf-v6-c-switch__input include-toggle repo-cb"
             {{ 'checked' if repo_file.include else '' }}
             {{ ' disabled title="Default distribution repository — cannot be excluded"' if repo_file.is_default_repo else '' }}
             data-snap-section="rpm" data-snap-list="repo_files" data-snap-index="{{ repo_file._index }}"/>
      <span class="pf-v6-c-switch__toggle"></span>
    </label>
    {%- endif %}
  </td>
  <th colspan="{{ (5 if fleet_meta else 4) }}" class="table-group-header-text">
    <button class="pf-v6-c-button pf-m-plain repo-collapse-btn" aria-expanded="true" data-repo="{{ repo_name }}">
      <span class="pf-v6-c-button__icon">&#9660;</span>
    </button>
    {{ repo_name }} ({{ entries|length }} package{{ 's' if entries|length != 1 else '' }})
  </th>
</tr>
```

**Note:** Uses the `repo_file_lookup` dict prepared in Step 3.

- [ ] **Step 6: Add expand/collapse JS for repo groups**

In `_js.html.j2`, add a click handler for `.repo-collapse-btn`:

```javascript
document.querySelectorAll('.repo-collapse-btn').forEach(function(btn) {
  btn.addEventListener('click', function() {
    var repo = this.getAttribute('data-repo');
    var expanded = this.getAttribute('aria-expanded') === 'true';
    this.setAttribute('aria-expanded', String(!expanded));
    this.querySelector('.pf-v6-c-button__icon').textContent = expanded ? '\u25B6' : '\u25BC';
    // Toggle visibility of all rows in this repo group
    var row = this.closest('tr');
    var next = row.nextElementSibling;
    while (next && !next.classList.contains('table-group-header')) {
      next.style.display = expanded ? 'none' : '';
      next = next.nextElementSibling;
    }
  });
});
```

- [ ] **Step 7: Migrate `applyRepoCascade()` selectors**

In `_js.html.j2`, find `applyRepoCascade()` and update it to work with the new DOM structure. The function currently queries `#pkg-repo-table .repo-cb` — update to query `.repo-cb` within the dep tree card. The cascade logic (walking `data-leaf` rows) should remain unchanged.

Also update the repo-toggle event listener registration (currently `document.querySelectorAll('#pkg-repo-table .repo-cb')`) to target the new selector.

**Add `recalcTriageCounts()`** to the repo-toggle handler after `applyRepoCascade(this)`:

```javascript
document.querySelectorAll('.repo-cb').forEach(function(cb) {
  cb.addEventListener('change', function() {
    applyRepoCascade(this);
    recalcTriageCounts();
    updateToolbar();
    setDirty(!isSnapshotClean());
  });
});
```

- [ ] **Step 8: Run tests to verify they pass**

```bash
pytest tests/test_visual_consistency.py::TestPackagesRestructure -v
```

Expected: PASS.

- [ ] **Step 9: Run full test suite**

```bash
pytest tests/ -v --tb=short
```

Fix any regressions (existing package-related tests may assert on the old repo card DOM).

- [ ] **Step 10: Commit**

```bash
git add src/yoinkc/templates/report/_packages.html.j2 src/yoinkc/templates/report/_js.html.j2 src/yoinkc/renderers/html_report.py tests/test_visual_consistency.py
git commit -m "feat: merge Repositories card into dependency tree

Repo names become expandable group headers with include/exclude toggles.
Default repos remain non-toggleable. Cascade via applyRepoCascade()
migrated to new DOM structure. recalcTriageCounts() added after cascade."
```

---

## Task 8: Visual Smoke Test

- [ ] **Step 1: Run full test suite one final time**

```bash
pytest tests/ -v --tb=short
```

Expected: All pass.

- [ ] **Step 2: Generate a test report and visually inspect**

```bash
cd /Users/mrussell/Work/bootc-migration
./driftify/driftify.py --profile web-server
./yoinkc/run-yoinkc.sh
```

Open the generated report in a browser and verify:
- [ ] Relaxed table spacing is visible across all tabs
- [ ] Config tab: no rpm-Va column, no diff column, permissions badge shows when appropriate
- [ ] Config/Services/Containers: pencil icon is between checkbox and path
- [ ] Packages: no separate Repositories card, repos are expandable headers in dep tree
- [ ] Scheduled jobs timers: Schedule column has tighter fit
- [ ] Network connections: Method/Type/Deployment columns have tighter fit
- [ ] No inline `style="margin-left: 8px"` or `style="margin: 0"` in page source

- [ ] **Step 3: Commit any visual polish fixes discovered during inspection**

```bash
git add -A
git commit -m "fix: visual polish from smoke test"
```

(Skip this step if no fixes are needed.)
