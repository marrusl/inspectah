# Editor Link Redesign Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace "View & edit in editor →" text links with a pencil icon in a dedicated column across config, services, and containers tables.

**Architecture:** Template-only changes (Jinja2 + CSS). No JS changes — `navigateToEditor()` is reused as-is.

**Tech Stack:** Jinja2 templates, PF6 CSS, CSS filter for greyscale effect

**Spec:** `docs/specs/proposed/2026-03-16-editor-link-redesign-design.md`

---

## Task 1: Add CSS for pencil icon

**Files:**
- Modify: `src/yoinkc/templates/report/_css.html.j2`

- [ ] **Step 1: Add editor-icon styles**

Add to the existing stylesheet:

```css
.editor-icon {
  font-size: 14px;
  cursor: pointer;
  filter: grayscale(1);
  transition: filter 0.15s;
  background: none;
  border: none;
  padding: 2px;
}
.editor-icon:hover {
  filter: grayscale(0);
}
```

- [ ] **Step 2: Commit**

```bash
git add src/yoinkc/templates/report/_css.html.j2
git commit -m "feat: add editor-icon CSS for pencil link"
```

## Task 2: Update config table

**Files:**
- Modify: `src/yoinkc/templates/report/_config.html.j2`

- [ ] **Step 1: Add icon column header**

In the `<thead>` (line 5), add a new `<th>` inside the refine_mode guard as the last column before the closing `</tr>`:

```jinja2
{% if refine_mode %}<th class="pf-m-fit-content" scope="col"></th>{% endif %}
```

- [ ] **Step 2: Replace text links with icon — non-fleet single rows (line 84)**

Replace the inline `<a>` text link with a pencil icon `<td>`:

Remove the `<a>` from the Path `<td>` (line 84) so it just has `<code>{{ item.path }}</code>`.

Add a new `<td>` at the end of the row (before `{{ fleet_cell(item) }}` or after it, matching the `<th>` position):

```jinja2
{% if refine_mode %}<td><button class="pf-v6-c-button pf-m-plain editor-icon" title="Edit in editor" data-section="config" data-list="files" data-index="{{ item.snap_index }}" data-path="{{ item.path }}" onclick="navigateToEditor(this)">✏️</button></td>{% endif %}
```

- [ ] **Step 3: Replace text links with icon — fleet single rows (line 71)**

Same change as Step 2 but for fleet single-variant rows. Remove the `<a>` from the Path `<td>`, add icon `<td>` at the matching column position.

- [ ] **Step 4: Add icon column to variant parent row (line 28)**

The parent row has `<td></td><td></td><td></td><td></td>` for empty columns. Add one more empty `<td>` if in refine_mode:

```jinja2
<td></td><td></td><td></td><td></td>{% if refine_mode %}<td></td>{% endif %}
```

- [ ] **Step 5: Add icon to variant child rows**

In the variant children table (lines 34–62), add the pencil icon `<td>` to each variant row. Use the appropriate tooltip based on selection state:

```jinja2
{% if refine_mode %}
<td><button class="pf-v6-c-button pf-m-plain editor-icon" title="{% if v.item.include %}Edit in editor{% else %}View in editor (read-only){% endif %}" data-section="config" data-list="files" data-index="{{ v.item.snap_index }}" data-path="{{ path }}" onclick="navigateToEditor(this)">✏️</button></td>
{% endif %}
```

- [ ] **Step 6: Verify column alignment**

Check that all row types (single, fleet single, variant parent, variant child) have consistent column counts. Count the `<td>` elements in each row type and ensure they match the `<th>` count.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/templates/report/_config.html.j2
git commit -m "feat: replace editor text links with pencil icon in config table"
```

## Task 3: Update services table

**Files:**
- Modify: `src/yoinkc/templates/report/_services.html.j2`

- [ ] **Step 1: Read the file and identify all "View & edit in editor" links**

- [ ] **Step 2: Add icon column `<th>` in refine_mode guard**

- [ ] **Step 3: Replace each text link with pencil icon `<td>`**

Same pattern as config: remove inline `<a>` from content cells, add icon `<td>` at consistent column position. Use appropriate tooltip for selected vs non-selected variants.

- [ ] **Step 4: Ensure column alignment across all row types**

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_services.html.j2
git commit -m "feat: replace editor text links with pencil icon in services table"
```

## Task 4: Update containers table

**Files:**
- Modify: `src/yoinkc/templates/report/_containers.html.j2`

- [ ] **Step 1: Read the file and identify all "View & edit in editor" links**

- [ ] **Step 2: Add icon column `<th>` in refine_mode guard**

- [ ] **Step 3: Replace each text link with pencil icon `<td>`**

Same pattern as config and services.

- [ ] **Step 4: Ensure column alignment across all row types**

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/templates/report/_containers.html.j2
git commit -m "feat: replace editor text links with pencil icon in containers table"
```

## Task 5: Cleanup and verification

- [ ] **Step 1: Search for any remaining "View & edit in editor" text**

```bash
grep -r "View.*edit.*editor\|edit-in-editor-link" src/yoinkc/templates/report/
```

There should be zero matches. If any remain, remove them.

- [ ] **Step 2: Run tests**

```bash
python -m pytest tests/ -x -q
```

- [ ] **Step 3: Visual verification**

Open a report in refine mode. Verify:
- Pencil icons appear on every editable row in config, services, containers
- Icons are greyscale by default, full-color pencil on hover
- Click navigates to editor and selects correct file
- Tooltips show correct text (edit vs read-only)
- No icon column appears in non-refine mode
- Column alignment is consistent (especially config variant rows)
- No "View & edit in editor →" text anywhere
