# Report Template Split — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development
> (if subagents available) or superpowers:executing-plans to implement this
> plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `templates/report.html.j2` (2,556 lines) into Jinja2 include
partials — one per content section, plus CSS, JS, and shared macros.

**Architecture:** Pure template refactor. Each content section is extracted
into a partial under `templates/report/`. Macros go in a shared file, imported
via `{% from ... import ... %}`. The main template becomes a ~50-line skeleton
of `{% include %}` directives. Zero Python code changes. Rendered HTML output
is byte-identical.

**Tech Stack:** Jinja2, pytest

**Spec:** `docs/specs/2026-03-15-report-template-split-design.md`

---

## Extraction Pattern

Every content partial is a direct extraction — copy the lines from the
original template into a new file. Partials that use macros (`section`,
`fleet_cell`, `data_table`) need an import line at the top:

```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

Only import the macros the partial actually uses. See the macro usage table in
the spec.

## Source Line Map

Reference for `src/yoinkc/templates/report.html.j2`:

| Lines     | Destination               | Macro imports needed           |
|-----------|---------------------------|--------------------------------|
| 17-28     | `_macros.html.j2`         | —                              |
| 29-48     | `_macros.html.j2`         | —                              |
| 49-56     | `_macros.html.j2`         | —                              |
| 65-284    | `_css.html.j2`            | none                           |
| 298-340   | `_sidebar.html.j2`        | none                           |
| 341-380   | `_banner.html.j2`         | none                           |
| 381-417   | `_summary.html.j2`        | `section`                      |
| 418-434   | `_file_browser.html.j2`   | `section`                      |
| 435-603   | `_packages.html.j2`       | `section`, `fleet_cell`        |
| 604-693   | `_services.html.j2`       | `section`, `fleet_cell`        |
| 694-775   | `_config.html.j2`         | `section`, `fleet_cell`        |
| 776-912   | `_non_rpm.html.j2`        | `section`                      |
| 913-1050  | `_containers.html.j2`     | `section`, `fleet_cell`        |
| 1051-1151 | `_users_groups.html.j2`   | `section`                      |
| 1152-1250 | `_scheduled_jobs.html.j2` | `section`, `fleet_cell`        |
| 1251-1340 | `_kernel_boot.html.j2`    | `section`                      |
| 1341-1418 | `_selinux.html.j2`        | `section`                      |
| 1419-1440 | `_secrets.html.j2`        | `section`                      |
| 1441-1566 | `_network.html.j2`        | `section`, `fleet_cell`        |
| 1567-1579 | `_storage.html.j2`        | `section`                      |
| 1580-1606 | `_warnings.html.j2`       | `section`                      |
| 1607-1853 | `_audit_report.html.j2`   | `section`                      |
| 1855-1871 | `_containerfile.html.j2`  | none                           |
| 1877-1898 | `_toolbar.html.j2`        | none                           |
| 1899-2554 | `_js.html.j2`             | none                           |

Lines not listed stay in the skeleton (`report.html.j2`): comment header
(1-16), DOCTYPE/head (57-64), PatternFly CSS embed (64), head close/body
open/page div/masthead (285-297), section/main/page closing tags (1873-1876),
body/html close (2555-2556).

---

## Chunk 1: Create Partials and Skeleton

### Task 1: Create directory and macros

**Files:**
- Create: `src/yoinkc/templates/report/` (directory)
- Create: `src/yoinkc/templates/report/_macros.html.j2`

- [ ] **Step 1: Create the report partials directory**

```bash
mkdir -p src/yoinkc/templates/report
```

- [ ] **Step 2: Create `_macros.html.j2`**

Extract lines 17-56 from `report.html.j2` — the three macros (`section`,
`fleet_cell`, `data_table`). Copy as-is, no modifications needed.

---

### Task 2: Create CSS and JS partials

**Files:**
- Create: `src/yoinkc/templates/report/_css.html.j2`
- Create: `src/yoinkc/templates/report/_js.html.j2`

- [ ] **Step 1: Create `_css.html.j2`**

Extract lines 65-284 (the `<style>` block with custom CSS, 220 lines).
Include the opening `<style>` and closing `</style>` tags.

- [ ] **Step 2: Create `_js.html.j2`**

Extract lines 1899-2554 (the `<script>` block, 656 lines). Include the
opening `<script>` and closing `</script>` tags.

---

### Task 3: Create structural partials (sidebar, banner, summary)

**Files:**
- Create: `src/yoinkc/templates/report/_sidebar.html.j2`
- Create: `src/yoinkc/templates/report/_banner.html.j2`
- Create: `src/yoinkc/templates/report/_summary.html.j2`

- [ ] **Step 1: Create `_sidebar.html.j2`**

Extract lines 298-340. No macro imports needed.

- [ ] **Step 2: Create `_banner.html.j2`**

Extract lines 341-380. No macro imports needed.

- [ ] **Step 3: Create `_summary.html.j2`**

Extract lines 381-417. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

---

### Task 4: Create content section partials (file browser through config)

**Files:**
- Create: `src/yoinkc/templates/report/_file_browser.html.j2`
- Create: `src/yoinkc/templates/report/_packages.html.j2`
- Create: `src/yoinkc/templates/report/_services.html.j2`
- Create: `src/yoinkc/templates/report/_config.html.j2`

- [ ] **Step 1: Create `_file_browser.html.j2`**

Extract lines 418-434. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 2: Create `_packages.html.j2`**

Extract lines 435-603 (169 lines, largest content section). Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

- [ ] **Step 3: Create `_services.html.j2`**

Extract lines 604-693. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

- [ ] **Step 4: Create `_config.html.j2`**

Extract lines 694-775. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

Note: config uses `fleet_cell()` for prevalence bars but builds its tables
by hand (no `data_table`).

---

### Task 5: Create content section partials (non-RPM through scheduled jobs)

**Files:**
- Create: `src/yoinkc/templates/report/_non_rpm.html.j2`
- Create: `src/yoinkc/templates/report/_containers.html.j2`
- Create: `src/yoinkc/templates/report/_users_groups.html.j2`
- Create: `src/yoinkc/templates/report/_scheduled_jobs.html.j2`

- [ ] **Step 1: Create `_non_rpm.html.j2`**

Extract lines 776-912. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 2: Create `_containers.html.j2`**

Extract lines 913-1050. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

- [ ] **Step 3: Create `_users_groups.html.j2`**

Extract lines 1051-1151. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 4: Create `_scheduled_jobs.html.j2`**

Extract lines 1152-1250. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

---

### Task 6: Create content section partials (kernel through warnings)

**Files:**
- Create: `src/yoinkc/templates/report/_kernel_boot.html.j2`
- Create: `src/yoinkc/templates/report/_selinux.html.j2`
- Create: `src/yoinkc/templates/report/_secrets.html.j2`
- Create: `src/yoinkc/templates/report/_network.html.j2`
- Create: `src/yoinkc/templates/report/_storage.html.j2`
- Create: `src/yoinkc/templates/report/_warnings.html.j2`

- [ ] **Step 1: Create `_kernel_boot.html.j2`**

Extract lines 1251-1340. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 2: Create `_selinux.html.j2`**

Extract lines 1341-1418. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 3: Create `_secrets.html.j2`**

Extract lines 1419-1440. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 4: Create `_network.html.j2`**

Extract lines 1441-1566. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section, fleet_cell %}
```

- [ ] **Step 5: Create `_storage.html.j2`**

Extract lines 1567-1579. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

- [ ] **Step 6: Create `_warnings.html.j2`**

Extract lines 1580-1606. Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

---

### Task 7: Create remaining partials (audit report, containerfile, toolbar)

**Files:**
- Create: `src/yoinkc/templates/report/_audit_report.html.j2`
- Create: `src/yoinkc/templates/report/_containerfile.html.j2`
- Create: `src/yoinkc/templates/report/_toolbar.html.j2`

- [ ] **Step 1: Create `_audit_report.html.j2`**

Extract lines 1607-1853 (270 lines, all 13 subsections). Add at top:
```jinja2
{% from "report/_macros.html.j2" import section %}
```

The audit report uses `{% call section("audit", "Audit report") %}` at L1608
and `{% endcall %}` at L1853 — both must be included in the partial.

- [ ] **Step 2: Create `_containerfile.html.j2`**

Extract lines 1855-1871 (~17 lines). No macro imports needed — this section
uses raw `<div>` markup, not the `section` macro.

- [ ] **Step 3: Create `_toolbar.html.j2`**

Extract lines 1877-1898 (toolbar markup + toast div, before the `<script>`
block).

---

### Task 8: Replace main template with skeleton

**Files:**
- Modify: `src/yoinkc/templates/report.html.j2`

- [ ] **Step 1: Read the original template**

Before replacing, read the full original to understand the exact HTML nesting.
Pay attention to:
- The masthead `<header>` block (L289-297)
- The `<section class="pf-v6-c-page__main-section">` wrapper
- The closing tags at L1873-1876

- [ ] **Step 2: Replace `report.html.j2` with the skeleton**

Replace the entire 2,556-line file with a skeleton that:
1. Keeps the comment header (updated to reference partials)
2. Keeps DOCTYPE, html, head with PatternFly CSS embed
3. Includes `report/_css.html.j2` for custom CSS
4. Keeps the masthead header markup inline (small, structural)
5. Includes all content partials in order
6. Keeps the structural closing tags (`</section>`, `</main>`, `</div>`)
7. Includes toolbar and JS partials
8. Closes body and html

The skeleton should look like the example in the spec's "Skeleton Structure"
section, but the implementer MUST verify the exact HTML element nesting
against the original template. The spec's example is illustrative.

**Critical:** The skeleton must preserve every structural HTML element from
the original. Missing a `<section>` or `</div>` will break the layout.

---

### Task 9: Verify and commit

- [ ] **Step 1: Run the full test suite**

```bash
cd /path/to/yoinkc
python -m pytest tests/ -v
```

Expected: all tests pass. The HTML report tests in
`test_renderer_outputs.py::TestHtmlReport` are the primary correctness check.

- [ ] **Step 2: Spot-check rendered output**

Generate a report and visually verify it renders correctly:

```bash
python -m pytest tests/test_renderer_outputs.py::TestHtmlReport -v
```

If all tests pass, the output is byte-identical to the original.

- [ ] **Step 3: Verify partial count**

```bash
ls src/yoinkc/templates/report/ | wc -l
```

Expected: 24 files (macros + CSS + JS + sidebar + banner + summary +
file_browser + packages + services + config + non_rpm + containers +
users_groups + scheduled_jobs + kernel_boot + selinux + secrets + network +
storage + warnings + audit_report + containerfile + toolbar = 23, plus
verify against spec's file layout).

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/templates/report/ src/yoinkc/templates/report.html.j2
git commit -m "refactor(template): split report.html.j2 into include partials

Split templates/report.html.j2 (2,556 lines) into 24 Jinja2 include partials
under templates/report/. Macros shared via {% import %}. Main template is now
a ~50-line skeleton of {% include %} directives.

No behavioral changes — rendered HTML output is byte-identical.

Assisted-by: Claude <noreply@anthropic.com>"
```
