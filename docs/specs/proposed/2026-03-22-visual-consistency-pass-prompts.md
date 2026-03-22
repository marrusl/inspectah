# Visual Consistency Pass — Cursor Prompts

Three prompts covering all 8 tasks from the implementation plan. User creates the feature branch manually before starting.

## Pre-work (manual)

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git checkout -b visual-consistency-pass
```

---

## Prompt 1: Global Spacing, Inline Style Cleanup, Column Consistency

**Cursor: new chat** | **Model: sonnet**

~~~
**Context: Read AGENTS.md First**

Before proceeding, read `/Users/mrussell/Work/bootc-migration/AGENTS.md` to understand:
- Communication style and code quality expectations
- Commit message format and attribution requirements
- This workspace contains TWO separate git repositories: `yoinkc/` and `driftify/`
- The workspace root is NOT a repository — do not run git init or create repos here
- You are acting as Engineering: implement changes but do not commit until review is requested

Project you're working on: yoinkc

---

## Task

Implement Parts A, D, and E of the visual consistency pass spec. Read the spec first:
`docs/specs/proposed/2026-03-22-visual-consistency-pass-design.md`

Also read the implementation plan for detailed steps:
`docs/specs/proposed/2026-03-22-visual-consistency-pass-plan.md`
(Tasks 1, 2, and 3)

## Order of Work: Tests First

### 1. Create test file with ALL failing tests

Create `tests/test_visual_consistency.py`. Write these test classes FIRST, before any implementation:

**TestGlobalSpacing** — Assert that PF6 logical-property CSS variables for table cell padding appear in rendered HTML:
- `--pf-v6-c-table--cell--PaddingBlockStart`
- `--pf-v6-c-table--cell--PaddingBlockEnd`
- `--pf-v6-c-table--cell--PaddingInlineStart`
- `--pf-v6-c-table--cell--PaddingInlineEnd`

Use this render helper (reuse across all test classes in this file):
```python
import tempfile
from pathlib import Path
from yoinkc.renderers import run_all_renderers
from yoinkc.schema import InspectionSnapshot, OsRelease

def _render(refine_mode=False, **snapshot_kwargs) -> str:
    defaults = {
        "meta": {"host_root": "/host"},
        "os_release": OsRelease(name="RHEL", version_id="9", pretty_name="RHEL 9"),
    }
    defaults.update(snapshot_kwargs)
    snapshot = InspectionSnapshot(**defaults)
    with tempfile.TemporaryDirectory() as tmp:
        run_all_renderers(snapshot, Path(tmp), refine_mode=refine_mode)
        return (Path(tmp) / "report.html").read_text()
```

**TestInlineStyleCleanup** — Tests that:
- CSS classes `.fleet-variant-toggle`, `.fleet-variant-table`, `.variant-index` are defined in the rendered HTML
- The three specific inline styles are gone from content template FILES (not rendered output — grep the template source files directly since test fixtures may not produce fleet variant content):
  - `style="margin-left: 8px; cursor: pointer;"` gone from `src/yoinkc/templates/report/_*.html.j2` (excluding `_css.html.j2` and `_js.html.j2`)
  - `style="margin: 0;"` gone from same
  - `style="color: var(--pf-v6-global--Color--200);"` gone from same

**TestColumnConsistency** — Tests that:
- Scheduled timers table (first table in `id="section-scheduled-jobs"`) contains `pf-m-fit-content`
- Network connections table (first table in `id="section-network"`) contains `pf-m-fit-content`

### 2. Run tests to verify they fail

```bash
pytest tests/test_visual_consistency.py -v
```

### 3. Implement Part A: Global Table Spacing

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

### 4. Implement Part E: Inline Style Cleanup

First, grep to find every occurrence:
```bash
grep -rn 'style="margin-left: 8px; cursor: pointer;"' src/yoinkc/templates/report/
grep -rn 'style="margin: 0;"' src/yoinkc/templates/report/
grep -rn 'style="color: var(--pf-v6-global--Color--200);"' src/yoinkc/templates/report/
```

Add CSS classes to `_css.html.j2`:
```css
/* Fleet variant styling (migrated from inline styles) */
.fleet-variant-toggle { margin-left: 8px; cursor: pointer; }
.fleet-variant-table { margin: 0; }
.variant-index { color: var(--pf-v6-global--Color--200); }
```

Replace each inline style with the corresponding class in the template files. Preserve existing classes (merge with space).

### 5. Implement Part D: Column Consistency

In `src/yoinkc/templates/report/_scheduled_jobs.html.j2`, add `class="pf-m-fit-content"` to the `Schedule` column `<th>` in the timers table.

In `src/yoinkc/templates/report/_network.html.j2`, add `class="pf-m-fit-content"` to the `Method`, `Type`, and `Deployment` column `<th>` tags in the Connections table.

### 6. Run all tests

```bash
pytest tests/test_visual_consistency.py -v
pytest tests/ -v --tb=short
```

All should pass including the full suite (no regressions).

## Commits

Make 3 separate commits (one per part: A, E, D). Each commit should represent one logical change.

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~

---

## Prompt 2: Config Tab Cleanup (Drop Columns, Permissions Badge, Pencil Reorder)

**Cursor: continue current chat** | **Model: sonnet**

~~~
## Task

Implement Parts B1-B5 of the visual consistency pass. This covers:
- B1: Drop `rpm -Va flags` column
- B2: Drop diff preview column + backend cleanup
- B3: Add "permissions changed" badge
- B4: Move pencil icon to between checkbox and path (all 3 editor-enabled tabs)
- B5: Update variant row structure (colspan, filler cells, inner variant tables)

Read the spec sections carefully:
`docs/specs/proposed/2026-03-22-visual-consistency-pass-design.md` (Part B)

And the plan:
`docs/specs/proposed/2026-03-22-visual-consistency-pass-plan.md` (Tasks 4, 5, 6)

## Order of Work: Tests First

### 1. Write ALL failing tests first

Add these test classes to `tests/test_visual_consistency.py`:

**TestConfigColumnCleanup** — Tests that:
- `rpm -Va` text and raw flag strings like `S.5.....` are NOT present in the config section
- `diff-view` and `diff-add` CSS class references are NOT in the config section
- `.diff-view`, `.diff-hdr`, `.diff-hunk`, `.diff-add`, `.diff-del` CSS class definitions are removed from the full HTML
- `_render_diff_html` function no longer exists on the `html_report` module (`not hasattr(html_report, "_render_diff_html")`)

Use a helper that renders with a config file:
```python
from yoinkc.schema import ConfigFile, ConfigSection

def _render_with_config(flags="S.5.....", diff="--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new", refine_mode=False):
    return _render(
        refine_mode=refine_mode,
        config=ConfigSection(files=[
            ConfigFile(path="/etc/test.conf", rpm_va_flags=flags,
                       diff_against_rpm=diff, kind="test",
                       category="system", include=True),
        ]),
    )
```

**TestPermissionsBadge** — Tests that:
- Badge containing "permissions" appears when flags contain `M` (e.g., `"SM5....."`)
- Badge appears for `U` flag (e.g., `"..5..U.."`)
- Badge appears for `G` flag (e.g., `"..5...G."`)
- Badge does NOT appear for content-only flags (e.g., `"S.5....."`)
- Badge does NOT appear for empty flags (`""`)

**TestPencilReorder** — Tests that:
- In refine mode, `editor-icon` appears BEFORE the path text (`/etc/test.conf`) in DOM order in the config section
- In read-only mode, `editor-icon` does NOT appear in the config section

### 2. Run tests to verify they fail

```bash
pytest tests/test_visual_consistency.py::TestConfigColumnCleanup tests/test_visual_consistency.py::TestPermissionsBadge tests/test_visual_consistency.py::TestPencilReorder -v
```

### 3. Implement B1-B2: Remove columns and backend

**Backend (`src/yoinkc/renderers/html_report.py`):**
- Delete the `_render_diff_html()` function entirely
- Remove the `"diff_html": _render_diff_html(...)` line from `_prepare_config_files()`

**CSS (`src/yoinkc/templates/report/_css.html.j2`):**
- Remove the `.diff-view`, `.diff-add`, `.diff-del`, `.diff-hdr`, `.diff-hunk` CSS rules

**Template (`src/yoinkc/templates/report/_config.html.j2`):**
- Remove `<th class="pf-m-fit-content" scope="col">rpm -Va flags</th>` from the header
- Remove `<th scope="col">Diff</th>` from the header
- Remove corresponding `<td>` cells from ALL row types (normal rows, variant parent rows, variant child rows in inner compact tables)
- Reduce filler `<td></td>` cells on parent/group rows (remove the fillers that padded for rpm-Va and Diff — keep fillers for Kind and Category only)
- Update `colspan="10"` on fleet variant group rows to reflect new column count: base 4 (checkbox + path + kind + category) + conditionals (fleet, pencil)

### 4. Implement B3: Permissions badge

In `_config.html.j2`, in the Kind `<td>` cell, after the existing kind badge, add:

```jinja2
{%- if f.flags and ('M' in f.flags or 'U' in f.flags or 'G' in f.flags) %}
<span class="pf-v6-c-label pf-m-compact pf-m-gold"><span class="pf-v6-c-label__content">permissions</span></span>
{%- endif %}
```

Apply the same logic in fleet variant child rows (inner compact variant tables), using the variant item's `flags` field. The template receives this field as `flags` (not `rpm_va_flags`).

### 5. Implement B4-B5: Pencil icon reorder

Move the pencil `<th>` and `<td>` from the last column to position 2 (between checkbox and path) in ALL of these tables:

**`_config.html.j2`:** New order: Checkbox | Pencil | Path | Kind | Category | Fleet

**`_services.html.j2` (Drop-in Overrides table ONLY, NOT State Changes):** New order: Checkbox | Pencil | Parent unit | Drop-in path | Content | Fleet

**`_containers.html.j2` (Quadlets table ONLY, NOT Compose Services or Running Containers):** New order: Checkbox | Pencil | Unit | Image | Path | Content | Fleet

For each template:
- Move `<th>` in `<thead>`
- Move `<td>` in each `<tbody>` row (normal, variant parent, variant child)
- Pencil column only renders when `refine_mode` is true (keep the existing conditional)
- Update any filler cells and colspan values that assumed the old column order

### 6. Run all tests

```bash
pytest tests/test_visual_consistency.py -v
pytest tests/ -v --tb=short
```

## Commits

Make 3 separate commits:
1. B1-B2: Remove rpm-Va and diff columns + backend cleanup
2. B3: Add permissions badge
3. B4-B5: Pencil icon reorder

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~

---

## Prompt 3: Packages Tab Restructure

**Cursor: new chat** | **Model: opus-high**

~~~
**Context: Read AGENTS.md First**

Before proceeding, read `/Users/mrussell/Work/bootc-migration/AGENTS.md` to understand:
- Communication style and code quality expectations
- Commit message format and attribution requirements
- This workspace contains TWO separate git repositories: `yoinkc/` and `driftify/`
- The workspace root is NOT a repository — do not run git init or create repos here
- You are acting as Engineering: implement changes but do not commit until review is requested

Project you're working on: yoinkc

---

## Task

Implement Part C of the visual consistency pass: merge the separate Repositories card into the dependency tree as expandable group headers with include/exclude toggles.

Read the spec carefully (especially Part C with its cascade scope, mixed-state, default repo, and expand/collapse sections):
`docs/specs/proposed/2026-03-22-visual-consistency-pass-design.md`

And the plan (Task 7):
`docs/specs/proposed/2026-03-22-visual-consistency-pass-plan.md`

Also read the current implementation to understand the existing structure:
- `src/yoinkc/templates/report/_packages.html.j2` — current Repositories card and dep tree
- `src/yoinkc/templates/report/_js.html.j2` — search for `applyRepoCascade` and `repo-cb` to understand current cascade logic
- `src/yoinkc/renderers/html_report.py` — search for `repo_groups` and `repo_files` to understand the template context

## Order of Work: Tests First

### 1. Write ALL failing tests first

Add `TestPackagesRestructure` to `tests/test_visual_consistency.py`:

```python
from yoinkc.schema import RepoFile, PackageEntry, RpmSection

class TestPackagesRestructure:
    """Part C: merge repo card into dep tree."""

    def _render_packages(self, refine_mode=False) -> str:
        return _render(
            refine_mode=refine_mode,
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", version="2.4.57", release="8.el9",
                                 arch="x86_64", source_repo="appstream", include=True),
                ],
                repo_files=[
                    RepoFile(path="/etc/yum.repos.d/redhat.repo",
                             include=True, is_default_repo=True),
                    RepoFile(path="/etc/yum.repos.d/epel.repo",
                             include=True, is_default_repo=False),
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
        assert "table-group-header" in pkg_html

    def test_default_repo_toggle_disabled(self):
        html = self._render_packages(refine_mode=True)
        pkg_start = html.find('id="section-packages"')
        pkg_html = html[pkg_start:pkg_start + 10000]
        assert "Default distribution repository" in pkg_html
```

Check that the test fixture schema fields (`RepoFile`, `PackageEntry`, `RpmSection`) accept these keyword arguments. Adjust field names if needed to match the actual schema in `src/yoinkc/schema.py`.

### 2. Run tests to verify they fail

```bash
pytest tests/test_visual_consistency.py::TestPackagesRestructure -v
```

### 3. Prepare `repo_file_lookup` in `html_report.py`

In the renderer function that prepares the packages template context (where `repo_groups` is built), create a `repo_file_lookup` dict mapping repo group names to their `RepoFile` entries with original indices. Pass it to the template context.

The matching logic: for each repo_file, check if the repo group name appears in the file path (case-insensitive). This is heuristic but matches how the repo groups are named today.

### 4. Remove the Repositories card from `_packages.html.j2`

Delete the entire `card-pkg-repos` block — the `<div>` with `id="card-pkg-repos"` containing `#pkg-repo-table`. This is approximately lines 47-76 in the current template.

### 5. Add repo toggle to dep tree group headers

Replace the current repo group `<tr>` header (which is just a `<th colspan>` with the repo name) with a new row that includes:
- A checkbox toggle in a `<td class="pf-v6-c-table__check">` targeting `data-snap-section="rpm" data-snap-list="repo_files"` with the correct index from `repo_file_lookup`
- Disabled toggle with title "Default distribution repository — cannot be excluded" for repos where `is_default_repo` is true
- An expand/collapse chevron button (`aria-expanded="true"`, data-repo attribute)
- The repo name and package count in a `<th colspan>` spanning remaining columns

Repos with no matching `repo_file_lookup` entry should render the header without a toggle (just the name + count).

### 6. Add expand/collapse JS in `_js.html.j2`

Add a click handler for the collapse button that:
- Toggles `aria-expanded` between "true"/"false"
- Swaps the chevron character (down arrow `\u25BC` when expanded, right arrow `\u25B6` when collapsed)
- Hides/shows all subsequent sibling `<tr>` elements until the next `table-group-header` row
- Does NOT change any include/exclude state

Default state: expanded (all repos visible on load).

### 7. Migrate `applyRepoCascade()` selectors in `_js.html.j2`

The current `applyRepoCascade()` queries `#pkg-repo-table .repo-cb`. Update selectors to match the new DOM (repo checkboxes are now `.repo-cb` inside the dep tree table, not in `#pkg-repo-table` which no longer exists).

The cascade logic itself (walking `data-leaf` rows) should remain unchanged.

Update the event listener registration (currently `document.querySelectorAll('#pkg-repo-table .repo-cb')`) to the new selector.

**Required new work:** Add `recalcTriageCounts()` to the repo-toggle change handler, AFTER `applyRepoCascade(this)`. The current handler does not call it, so sidebar triage counts go stale after repo cascade. The handler should look like:

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

### 8. Run all tests

```bash
pytest tests/test_visual_consistency.py::TestPackagesRestructure -v
pytest tests/ -v --tb=short
```

Fix any regressions — existing tests may assert on `card-pkg-repos` or `#pkg-repo-table` DOM presence.

## Key Constraints from the Spec

- **Cascade scope:** Dep-tree leaf rows (`data-leaf`) only. Does NOT touch unclassified packages or the simple non-leaf `packages_added[:100]` list.
- **Mixed state:** No indeterminate/tri-state. Repo toggle is a bulk override: OFF forces all leaf rows excluded, ON forces all included. Individual package toggles don't affect repo toggle.
- **Default repos:** `is_default_repo` toggles are disabled with title text, same as current behavior.
- **Unmatchable repos:** If a repo file doesn't match any `repo_groups` key, cascade no-ops on packages (acceptable).

Do not commit yet. When asked, review your own changes in this chat before committing. Do not use subagents for review.
~~~

---

## Post-Implementation: Smoke Test (Manual)

After all 3 prompts are complete and committed:

```bash
cd /Users/mrussell/Work/bootc-migration
./driftify/driftify.py --profile web-server
./yoinkc/run-yoinkc.sh
```

Open the generated report and verify:
- Relaxed table spacing across all tabs
- Config tab: no rpm-Va, no diff, permissions badge when appropriate
- Pencil icon between checkbox and path in config/services/containers
- Packages: no separate Repositories card, repos are expandable headers
- Timers: Schedule column tighter
- Connections: Method/Type/Deployment columns tighter
- No stray inline styles in page source
