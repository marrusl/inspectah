# Toggle Switches Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace all PF6 checkboxes with toggle switches in the HTML report, and change excluded-item styling from strikethrough + opacity 0.45 to opacity 0.6.

**Architecture:** Direct 1:1 swap of `pf-v6-c-check` → `pf-v6-c-switch` markup across 10 templates, CSS rule updates in `_css.html.j2`, and class selector renames in `_js.html.j2`. No Python changes. No schema changes.

**Tech Stack:** Jinja2 templates, PatternFly 6 CSS (v6.4.0, bundled), vanilla JavaScript.

**Spec:** `docs/specs/proposed/2026-03-15-toggle-switches-design.md`

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src/yoinkc/templates/report/_css.html.j2` | Excluded-item styling, toggle visibility |
| Modify | `src/yoinkc/templates/report/_js.html.j2` | Event handlers, refine-mode activation |
| Modify | `src/yoinkc/templates/report/_containers.html.j2` | 5 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_config.html.j2` | 4 checkbox → switch swaps (2 multi-line, 2 single-line) |
| Modify | `src/yoinkc/templates/report/_services.html.j2` | 5 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_packages.html.j2` | 5 checkbox → switch swaps (preserving `leaf-cb`, `repo-cb`, `disabled`) |
| Modify | `src/yoinkc/templates/report/_kernel_boot.html.j2` | 1 checkbox → switch swap |
| Modify | `src/yoinkc/templates/report/_users_groups.html.j2` | 2 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_network.html.j2` | 2 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_scheduled_jobs.html.j2` | 2 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_non_rpm.html.j2` | 2 checkbox → switch swaps |
| Modify | `src/yoinkc/templates/report/_toolbar.html.j2` | Comment references only |

## Chunk 1: CSS and JavaScript Foundation

These changes establish the new styling and event wiring. Templates won't render correctly until they're updated in Chunk 2, but the CSS/JS is independent and can be committed first.

### Task 1: Update CSS Rules

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`

- [ ] **Step 1: Read `_css.html.j2`** to find the exact current rules.

- [ ] **Step 2: Update excluded-item styling.** Replace:

```css
tr.excluded td { text-decoration: line-through; opacity: 0.45; }
tr.excluded td:first-child { text-decoration: none; opacity: 1; }
div.excluded { text-decoration: line-through; opacity: 0.45; }
```

With:

```css
tr.excluded td { opacity: 0.6; }
tr.excluded td:first-child { opacity: 1; }
div.excluded { opacity: 0.6; }
```

- [ ] **Step 3: Update toggle visibility rule.** Replace:

```css
.pf-v6-c-check.pf-m-standalone.include-cb-wrap { display: none; }
```

With:

```css
.pf-v6-c-switch.include-toggle-wrap { display: none; }
```

- [ ] **Step 4: Commit.**

```bash
git add src/yoinkc/templates/report/_css.html.j2
git commit -m "style: update excluded-item styling and toggle visibility

Remove strikethrough, change opacity 0.45 to 0.6. Rename checkbox
class selectors to toggle equivalents.

Assisted-by: Claude Code"
```

### Task 2: Update JavaScript Selectors

**Files:**
- Modify: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Read `_js.html.j2`** to find all occurrences of `.include-cb` and `.include-cb-wrap`. There are 6+ occurrences of `.include-cb` and at least 1 of `.include-cb-wrap`. Grep to confirm the exact count.

- [ ] **Step 2: Rename all `.include-cb` → `.include-toggle`** across the file. This includes:
  - The main change event listener selector (~line 200)
  - Toolbar counting selectors
  - Reset button selectors
  - Any other `querySelector`/`querySelectorAll` calls

Use find-and-replace: `.include-cb` → `.include-toggle` (but NOT `.include-cb-wrap` — that gets its own rename).

**Important:** The `.leaf-cb` and `.repo-cb` selectors must NOT be renamed. They are separate classes on the same `<input>` elements. Only `.include-cb` changes.

- [ ] **Step 3: Rename all `.include-cb-wrap` → `.include-toggle-wrap`** across the file. This is the refine-mode activation selector.

- [ ] **Step 4: Verify the refine-mode activation display value.** The current code sets `el.style.display = 'inline-grid'` when activating `.include-cb-wrap` elements. PF6 `pf-v6-c-switch` also uses `display: inline-grid`, so this value remains correct. Confirm no change is needed.

- [ ] **Step 5: Commit.**

```bash
git add src/yoinkc/templates/report/_js.html.j2
git commit -m "refactor: rename checkbox selectors to toggle equivalents

Rename .include-cb to .include-toggle and .include-cb-wrap to
.include-toggle-wrap across all JS selectors. No logic changes.

Assisted-by: Claude Code"
```

## Chunk 2: Template Swaps

Each task swaps checkbox markup to switch markup in one template file. The pattern is the same for all:

**From (standard):**
```html
<span class="pf-v6-c-check pf-m-standalone include-cb-wrap">
  <input type="checkbox" class="pf-v6-c-check__input include-cb" {{ 'checked' if ITEM.include else '' }}/>
</span>
```

**To (standard):**
```html
<label class="pf-v6-c-switch include-toggle-wrap">
  <input type="checkbox" class="pf-v6-c-switch__input include-toggle" {{ 'checked' if ITEM.include else '' }}/>
  <span class="pf-v6-c-switch__toggle"></span>
</label>
```

Where `ITEM` varies by template (e.g., `svc`, `pkg`, `container`, `user`, etc.).

### Task 3: Swap `_packages.html.j2` (Most Complex)

**Files:**
- Modify: `src/yoinkc/templates/report/_packages.html.j2`

This template is the most complex because it has secondary classes (`leaf-cb`, `repo-cb`) and `disabled` attributes that must be preserved.

- [ ] **Step 1: Read `_packages.html.j2`** to identify all 5 checkbox locations and their secondary classes. There are: 1 repo checkbox (line 26), 2 leaf checkboxes (lines 60, 95), and 2 standard checkboxes (lines 143, 159).

- [ ] **Step 2: Swap each checkbox to a switch.** For each occurrence:

**Repo checkbox** (line 26 — has `repo-cb` class, conditional `disabled`):
```html
<label class="pf-v6-c-switch include-toggle-wrap">
  <input type="checkbox" class="pf-v6-c-switch__input include-toggle repo-cb" {{ 'checked' if rf.include else '' }}{{ ' disabled title="Default distribution repository — cannot be excluded"' if rf.is_default_repo else '' }}/>
  <span class="pf-v6-c-switch__toggle"></span>
</label>
```

Note: the variable is `rf` (not `repo`), the attribute is `rf.is_default_repo` (not `is_default`), and the title uses an em dash (—).

**Leaf package checkboxes** (lines 60, 95 — have `leaf-cb` class):
```html
<label class="pf-v6-c-switch include-toggle-wrap">
  <input type="checkbox" class="pf-v6-c-switch__input include-toggle leaf-cb" {{ 'checked' if ITEM.include else '' }}/>
  <span class="pf-v6-c-switch__toggle"></span>
</label>
```

Where `ITEM` is `entry` (line 60) or uses a complex expression (line 95). Preserve the exact `checked` conditional from each line.

**Standard package checkboxes** (lines 143, 159 — no secondary class):
```html
<label class="pf-v6-c-switch include-toggle-wrap">
  <input type="checkbox" class="pf-v6-c-switch__input include-toggle" {{ 'checked' if p.include else '' }}/>
  <span class="pf-v6-c-switch__toggle"></span>
</label>
```

- [ ] **Step 3: Verify no `leaf-cb` or `repo-cb` references were lost** by grepping the modified file.

- [ ] **Step 4: Commit.**

```bash
git add src/yoinkc/templates/report/_packages.html.j2
git commit -m "feat(packages): swap checkboxes for PF6 toggle switches

Preserve leaf-cb, repo-cb secondary classes and disabled attribute
for default distribution repos.

Assisted-by: Claude Code"
```

### Task 4: Swap `_containers.html.j2`

**Files:**
- Modify: `src/yoinkc/templates/report/_containers.html.j2`

- [ ] **Step 1: Read the file** to find all 5 checkbox locations.

- [ ] **Step 2: Swap all 5 checkboxes** using the standard pattern. The item variable names will vary (`primary.item`, `v.item`, `u`, `c`) — preserve whatever the template uses.

**Note:** Line 89 has a non-standard layout — the checkbox is wrapped in a bare `<label>` outside a table cell (unlike all other occurrences which are inside `<td class="pf-v6-c-table__check">`). Since the switch wrapper is already a `<label>`, the outer `<label>` wrapper at line 89 should be removed to avoid nesting labels.

- [ ] **Step 3: Commit.**

```bash
git add src/yoinkc/templates/report/_containers.html.j2
git commit -m "feat(containers): swap checkboxes for PF6 toggle switches

Assisted-by: Claude Code"
```

### Task 5: Swap `_config.html.j2`

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`

- [ ] **Step 1: Read the file** to find all 4 checkbox locations. There are 2 multi-line checkboxes (lines 14-16, 37-39) and 2 single-line checkboxes (lines 57, 69). The multi-line ones span 2-3 lines with indentation.

- [ ] **Step 2: Swap all 4 checkboxes** using the standard pattern. For multi-line checkboxes, replace the full span of lines. Match the exact whitespace/indentation of each location.

- [ ] **Step 3: Commit.**

```bash
git add src/yoinkc/templates/report/_config.html.j2
git commit -m "feat(config): swap checkboxes for PF6 toggle switches

Assisted-by: Claude Code"
```

### Task 6: Swap `_services.html.j2`

**Files:**
- Modify: `src/yoinkc/templates/report/_services.html.j2`

- [ ] **Step 1: Read the file** to find all 5 checkbox locations.
- [ ] **Step 2: Swap all 5 checkboxes** using the standard pattern.
- [ ] **Step 3: Commit.**

```bash
git add src/yoinkc/templates/report/_services.html.j2
git commit -m "feat(services): swap checkboxes for PF6 toggle switches

Assisted-by: Claude Code"
```

### Task 7: Swap remaining templates (batch)

**Files:**
- Modify: `src/yoinkc/templates/report/_kernel_boot.html.j2` (1 checkbox)
- Modify: `src/yoinkc/templates/report/_users_groups.html.j2` (2 checkboxes)
- Modify: `src/yoinkc/templates/report/_network.html.j2` (2 checkboxes)
- Modify: `src/yoinkc/templates/report/_scheduled_jobs.html.j2` (2 checkboxes)
- Modify: `src/yoinkc/templates/report/_non_rpm.html.j2` (2 checkboxes)

- [ ] **Step 1: Read each file** and locate all checkbox occurrences.

- [ ] **Step 2: Swap all checkboxes** in each file using the standard pattern.

- [ ] **Step 3: Commit all 5 files together** (they are simple, uniform changes).

```bash
git add src/yoinkc/templates/report/_kernel_boot.html.j2 \
        src/yoinkc/templates/report/_users_groups.html.j2 \
        src/yoinkc/templates/report/_network.html.j2 \
        src/yoinkc/templates/report/_scheduled_jobs.html.j2 \
        src/yoinkc/templates/report/_non_rpm.html.j2
git commit -m "feat: swap checkboxes for PF6 toggle switches in remaining tabs

kernel_boot, users_groups, network, scheduled_jobs, non_rpm.

Assisted-by: Claude Code"
```

### Task 8: Update `_toolbar.html.j2` Comments

**Files:**
- Modify: `src/yoinkc/templates/report/_toolbar.html.j2`

- [ ] **Step 1: Read the file** and find comment references to `.include-cb` / `.include-cb-wrap`.

- [ ] **Step 2: Update comment text** to reference `.include-toggle` / `.include-toggle-wrap`.

- [ ] **Step 3: Commit.**

```bash
git add src/yoinkc/templates/report/_toolbar.html.j2
git commit -m "docs: update toolbar comments for toggle class names

Assisted-by: Claude Code"
```

## Chunk 3: Verification

### Task 9: Full Verification

- [ ] **Step 1: Grep for any remaining `include-cb` references** across the entire `src/` directory. There should be zero matches.

```bash
grep -r "include-cb" src/yoinkc/templates/
```

Expected: no output.

- [ ] **Step 2: Grep for any remaining `pf-v6-c-check` references** in report templates. There should be zero matches (other templates outside `report/` are not in scope).

```bash
grep -r "pf-v6-c-check" src/yoinkc/templates/report/
```

Expected: no output.

- [ ] **Step 3: Run the existing test suite** to confirm nothing is broken.

```bash
pytest tests/ -v
```

Expected: all tests pass. Tests use data attributes and snapshot assertions, not CSS class names, so they should be unaffected.

- [ ] **Step 4: Visual spot-check.** Generate a report from a test snapshot and open it in a browser. Verify:
  - Toggles are hidden by default (not in refine mode)
  - No strikethrough styling on any excluded items in the HTML source
  - The `excluded` class still applies opacity 0.6
  - `pf-v6-c-table__check` cells accommodate the wider switch (~36px vs ~16px checkbox) without layout issues

- [ ] **Step 5: If tests pass and visual check is good, the implementation is complete.** No further commits needed.
