# HTML Audit Report Redesign

## Summary

Replace the current `report.html` — a minimal static page missing most audit
sections and dependent on a CDN — with a self-contained, offline-capable HTML
audit report that has full informational parity with `audit-report.md`. The
report uses server-side rendering via minijinja templates, embeds a minimized
report DTO for interactive enhancement, and ships PatternFly 6 CSS inline. It
works for both single-host and fleet-aggregated snapshots.

**Scope:**

- The HTML report renderer in `inspectah-pipeline` (`report.rs`)
- Shared report computation module (`report_data.rs`)
- The markdown audit renderer (`audit.rs`) — scoped to: (a) adding
  Users/Groups section, (b) migrating count/badge logic to `report_data.rs`
- The artifact rename from `report.html` to `audit-report.html` across all
  code surfaces (renderer, tarball, README renderer, Containerfile comments,
  tests)
- Doc updates across the inspectah repo

**Out of scope:** The Refine SPA (`inspectah-web`), any "what to do next" /
actionability affordance (future work), changes to the `Completeness` data
model (the HTML report renders what the current model exposes, nothing more).

## Artifact Naming

Both the markdown and HTML audit reports share a base name with different
extensions:

- `audit-report.md` (unchanged)
- `audit-report.html` (renamed from `report.html`)

### Rename Sweep

The rename from `report.html` to `audit-report.html` is an output-contract
change that touches these code locations:

| File | Current Reference | Change |
|------|------------------|--------|
| `inspectah-pipeline/src/render/mod.rs` | `output_dir.join("report.html")` | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/mod.rs` | doc comment: "2. report.html" | → `"2. audit-report.html"` |
| `inspectah-pipeline/src/render/mod.rs` | test assertions: `"report.html"` | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/readme.rs` | artifact table: `report.html` | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/readme.rs` | footer link: `report.html` | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/readme.rs` | test assertion | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/containerfile.rs` | comment referencing `report.html` | → `"audit-report.html"` |
| `inspectah-pipeline/src/render/tarball.rs` | test fixture: `"report.html"` | → `"audit-report.html"` |
| `inspectah-pipeline/tests/redaction_2c_surfaces_test.rs` | test 11 references `report.html` | → `"audit-report.html"` |
| `README.md` | output tree and references | → `"audit-report.html"` |
| Docs (`getting-started.md`, `architecture.md`, etc.) | Various references | → `"audit-report.html"` |

A `grep -rn 'report\.html' inspectah-pipeline/ inspectah-cli/` must return
zero hits after the rename is complete.

## Section Parity Contract

The spec claims parity with `audit-report.md`. This section defines what
that means precisely, mapped against the current `render_audit()` contract
in `inspectah-pipeline/src/render/audit.rs`.

### Parity Mapping

| # | Current Markdown Heading | HTML Section ID | HTML Display Name | Notes |
|---|-------------------------|----------------|-------------------|-------|
| 1 | `## Fleet Aggregate Summary` | `fleet-summary` | Fleet Aggregate Summary | Conditional: only when `fleet_meta` is present |
| 2 | `## Incomplete Sections` | `incomplete-sections` | Incomplete Sections | Conditional: only when failed or degraded sections exist. See Completeness Contract below. |
| 3 | `## Baseline comparison` | `baseline` | Baseline Comparison | Conditional: only when `target_image` is present |
| 4 | `## Packages` | `packages` | Packages | Includes version changes sub-table when baseline data exists |
| 5 | `## Configuration Files` | `config-files` | Configuration Files | Name matches markdown exactly |
| 6 | `## Service State Changes` | `service-changes` | Service State Changes | Name matches markdown exactly |
| 7 | `## Storage` | `storage` | Storage | Conditional: only when `storage` section exists |
| 8 | `## Kernel & Boot` | `kernel-boot` | Kernel & Boot | Conditional: only when `kernel_boot` section exists |
| 9 | `## Scheduled Tasks` | `scheduled-tasks` | Scheduled Tasks | |
| 10 | `## Security & Access Control` | `security` | Security & Access Control | SELinux mode, booleans, custom modules, fcontext rules. Conditional: only when `selinux` section exists |
| 11 | `## Non-RPM Software` | `nonrpm` | Non-RPM Software | |
| 12 | `## Redactions` | `redactions` | Redactions | Count + pointer only. See Redactions Contract. |
| 13 | `## Warnings` | `warnings` | Warnings | |
| 14 | *(new)* | `users-groups` | Users & Groups | New section. Added to both HTML and markdown renderers. See scope note. |

**Parity rule:** Every `## ` heading in `render_audit()` output has a
corresponding `<details id="...">` section in `render_report()` output for
the same snapshot, with the same data fields. Section #14 (Users & Groups)
is new to both renderers — this is the one intentional contract extension.

**Section names:** HTML display names match the markdown headings exactly
(no renames). This ensures users who read both artifacts see consistent
terminology.

### Section Categories

- **Always rendered:** Packages, Configuration Files, Service State Changes,
  Scheduled Tasks, Non-RPM Software, Users & Groups, Warnings
- **Conditional:** Fleet Aggregate Summary (`fleet_meta`), Baseline
  Comparison (`target_image`), Storage (`storage`), Kernel & Boot
  (`kernel_boot`), Security & Access Control (`selinux`), Incomplete
  Sections (any failed/degraded), Redactions (`redactions` non-empty)

### Precedence Rule

**Failed completeness state overrides data-source absence.** If an
`InspectorId` appears in `failed_sections`, its corresponding report
section renders as "data unavailable" regardless of whether the data source
is present in the snapshot. This is because a failed inspector means the
scan attempted to collect data and could not — the section's absence is not
intentional, it is a failure. The user must see that failure.

For all other states, the data-source presence rules apply: conditional
sections are omitted when their data source is absent.

### Section-Behavior Matrix (authoritative)

This is the single source of truth for how each combination of section
category and state behaves across all report surfaces.

| State | Condition | Page Body | TOC | Summary Card | Filter | DTO |
|-------|-----------|-----------|-----|-------------|--------|-----|
| **Normal** | Data present, not failed/degraded | Renders with data | Anchor link | Count | If filterable and 10+ rows | Included if filterable |
| **Empty** | Always-rendered section, 0 items | Renders, "(0)" badge, "No items detected" body | Anchor link | "0" | No | Excluded (no rows) |
| **Degraded** | `InspectorId` in `degraded_sections` | Renders with partial data, yellow "partial data" pill | Anchor link + "partial" | Count + "partial" | If filterable and 10+ rows | Included if filterable |
| **Failed** | `InspectorId` in `failed_sections` | Renders, "data unavailable" pill, failed body text | Anchor link + "failed" | "n/a" | No | Excluded |
| **Absent** | Conditional section, data source missing, NOT in `failed_sections` | Not rendered | Not present | Not present | No | Excluded |

**Filterable sections:** Only Packages, Configuration Files, Service State
Changes, Scheduled Tasks, and Users & Groups are filterable (they have DTO
support). All other sections are never filterable regardless of row count.

**Warnings exclusion:** Warnings is always rendered in the page body and
TOC but is explicitly excluded from summary cards. Warnings are
meta-commentary about the scan itself (e.g., "SELinux getenforce failed"),
not a findings category like Packages or Services. They do not belong in
the summary grid alongside migration-surface counts.

**Key distinctions:**

- **Empty vs Failed:** Empty means "the scan ran and found nothing" (badge
  shows "(0)"). Failed means "the scan could not collect data" (badge shows
  "data unavailable"). These must never be conflated.
- **Empty vs Absent:** Empty applies only to always-rendered sections (they
  always appear). Absent applies to conditional sections whose data source
  is missing (they are omitted).
- **Failed conditional:** A conditional section that is in `failed_sections`
  renders as failed (per the precedence rule), not absent. Example: if the
  Storage inspector failed, the Storage section renders with "data
  unavailable" even though `snap.storage` is `None`.

### Redactions Contract

The HTML report matches the current markdown behavior exactly: a count of
redacted items plus a pointer to `secrets-review.md`. No structured redaction
findings (paths, patterns, remediation) appear in the HTML report.

This preserves the existing artifact boundary: the general audit report stays
summary-level for redaction data, while `secrets-review.md` is the dedicated
artifact for secret-adjacent detail.

The Redactions section body:

```
N item(s) redacted. See secrets-review.md for details.
```

Rendered in monospace font, muted color — visually distinct from actionable
data.

### Users & Groups Safe-Field Whitelist

The `UserGroupDecision` type carries sensitive fields (`password_hash`,
`ssh_keys` with full key content, `shadow_entries`, `gshadow_entries`).
The general audit artifacts (both HTML and markdown) render only
summary-safe fields. This whitelist is the contract for what appears in
both renderers and in the DTO.

**Allowed fields (rendered in audit report and included in DTO):**

| Field | Type | Notes |
|-------|------|-------|
| `name` | String | User/group name |
| `uid` | u64 | User ID |
| `gid` | u64 | Group ID |
| `shell` | String | Login shell |
| `home` | String | Home directory |
| `include` | bool | Whether included in migration |
| `classification` | String | "interactive", "system", etc. |
| `has_sudo` | bool | Whether user has sudo access |
| `has_subuid` | bool | Whether user has subuid mapping |
| `ssh_key_count` | u64 | Number of SSH keys (count only, not content) |
| `supplementary_groups` | Vec | Group memberships |
| `password_status` | String | "password_set", "locked", "disabled" (status, not hash) |

**Excluded fields (never in audit report, never in DTO):**

| Field | Reason |
|-------|--------|
| `password_hash` | Secret material |
| `ssh_keys` | Full SSH key content is sensitive |
| `classification_rationale` | May reference sensitive context |

**Also excluded from both renderers (raw `UserGroupSection` fields):**

| Field | Reason |
|-------|--------|
| `passwd_entries` | Raw `/etc/passwd` lines |
| `shadow_entries` | Raw `/etc/shadow` lines (contain password hashes) |
| `gshadow_entries` | Raw `/etc/gshadow` lines |
| `sudoers_rules` | Raw sudoers rules (sensitive access config) |
| `ssh_authorized_keys_refs` | Raw key references |
| `subuid_entries` / `subgid_entries` | Raw mapping lines |

The `UserGroupDecision` struct provides the safe projection. The renderer
iterates `snap.users_groups.users`, deserializes each `serde_json::Value`
into `UserGroupDecision`, and renders only the whitelisted fields. The raw
`UserGroupSection` collection fields are never accessed by either renderer.

## Completeness Contract

The HTML report renders completeness information using only what the current
`Completeness` data model exposes. No per-section reason text is promised.

### Current Data Model

```rust
enum Completeness {
    Complete,
    Partial {
        degraded_sections: Vec<InspectorId>,
        reason: String,  // ONE global reason
    },
    Incomplete {
        failed_sections: Vec<InspectorId>,
        degraded_sections: Vec<InspectorId>,
        reason: String,  // ONE global reason
    },
}
```

### Three Render States

Each data section can be in one of three states:

| State | Condition | Badge | Body | Visual Treatment |
|-------|-----------|-------|------|-----------------|
| **Normal** | Section has data, not in failed/degraded lists | Item count | Data tables | Default |
| **Degraded** | `InspectorId` appears in `degraded_sections` | Item count + yellow "partial data" pill | Data tables (partial) | Yellow left border accent |
| **Failed** | `InspectorId` appears in `failed_sections` | "data unavailable" (no count) | "Data collection failed for this section." | Red left border accent, muted text |

**Critical distinction:** Failed sections render as "data unavailable," NOT
as "(0)" or "No items detected." A failed inspector means the scan could not
collect data — that is fundamentally different from "the scan ran and found
nothing." Conflating them is an audit truthfulness bug.

### Completeness Banner

The banner at the top of the report:

- Lists each degraded section by name (as an anchor link to the section)
- Lists each failed section by name (as an anchor link to the section)
- Shows the single global reason string
- Does NOT promise per-section reasons (the data model does not carry them)

Example:

```
Warning: This report was generated from an incomplete inspection.
Failed: config. Degraded: services.
Reason: permission denied on /etc/shadow
```

### Section-Level Indicators

- **Degraded:** Yellow "partial data" pill in the `<summary>` element.
  No per-section reason text — only the global reason appears in the
  completeness banner.
- **Failed:** Red "data unavailable" pill in the `<summary>` element.
  Section body shows a single message: "Data collection failed for this
  section. See the completeness warning above for details."

### TOC and Summary Card Behavior

- **Normal sections:** Anchor link in TOC, count in summary card.
- **Degraded sections:** Anchor link in TOC (with "partial" indicator),
  count in summary card (with "partial" indicator).
- **Failed sections:** Anchor link in TOC (with "failed" indicator), no
  count in summary card (show "n/a" instead of a number).

### InspectorId to Section Mapping

The `Completeness` model uses `InspectorId` variants. The mapping to report
sections:

| InspectorId | Report Section |
|-------------|---------------|
| `Rpm` | Packages |
| `Config` | Configuration Files |
| `Services` | Service State Changes |
| `Storage` | Storage |
| `KernelBoot` | Kernel & Boot |
| `ScheduledTasks` | Scheduled Tasks |
| `Selinux` | Security & Access Control |
| `NonRpmSoftware` | Non-RPM Software |
| `UsersGroups` | Users & Groups |
| `Containers` | *(not rendered in audit — container workloads go to quadlet/)* |
| `Subscription` | *(not rendered in audit — subscription goes to subscription/)* |
| `Network` | *(not rendered in audit)* |
| `Hardware` | *(not rendered in audit — Phase 2)* |
| `Ostree` | *(not rendered in audit — Phase 2)* |
| `OsRelease` | *(not rendered in audit — used for source info header)* |

## Architecture

### Rendering Strategy: Hybrid B+C

The HTML report uses **server-side rendering** via minijinja templates for all
content, with a **minimized report DTO** embedded as a `<script
type="application/json">` block for interactive enhancement.

**Why this hybrid:**

- **Minijinja templates (Approach B):** HTML lives in `.html` template files
  with proper syntax highlighting and tooling. Templates are type-safe via
  context objects. The report renders complete, semantic HTML that works with
  JS disabled. Compile-time embedding for release builds, runtime loading for
  development iteration.
- **Embedded report DTO (from Approach C):** The inline JS reads the
  embedded DTO to power table filtering and result counts. JS never builds
  DOM from scratch — it only enhances the server-rendered HTML. This keeps
  the JS footprint at ~100 lines.
- **Why not pure format!() (Approach A):** The current 300-line `format!()`
  approach is workable for partial coverage but unmaintainable at full
  audit parity (~1500-2000 lines of HTML in Rust string literals).
- **Why not pure client-side rendering (Approach C):** Moves rendering logic
  to untyped vanilla JS, errors surface at open-time not build-time,
  accessibility and print are harder to guarantee.

### Template Engine: minijinja

Minijinja over askama for three reasons:

1. **No proc macros.** Askama adds proc-macro compile overhead. Minijinja is
   a pure library crate (~30K lines). inspectah's workspace currently only
   has `serde` derive as proc-macro overhead.
2. **Dev iteration speed.** Minijinja supports runtime template loading during
   development (edit HTML, re-run binary, no recompile). Askama recompiles on
   every template change.
3. **Simpler custom filters.** Register a Rust closure for badge rendering,
   conflict counting, degraded CSS class selection.

Dependency: `minijinja = "2"` with `builtins` feature. Autoescaping enabled
by default — retires hand-rolled `html_escape()` for template contexts (keep
for non-template uses in the markdown renderer).

### PatternFly CSS

Vendor a minified copy of PatternFly 6 CSS in
`inspectah-pipeline/assets/patternfly.min.css`. Embed via `include_str!()` at
compile time. No CDN, no network dependency at build time. The CSS file is
tracked in git so the shipped version is explicit.

Size: ~400KB. Acceptable for a one-off report file.

### Shared Computation

Extract fleet conflict-counting, badge computation, and section-state logic
from `audit.rs` into a shared `report_data.rs` module. Both the markdown
renderer (`audit.rs`) and the HTML template context builder (`report.rs`) use
the same functions. This eliminates logic duplication and ensures both reports
produce consistent counts.

## Embedded Report DTO

### Why a DTO, Not the Full Snapshot

The `InspectionSnapshot` is the complete scan output. Embedding it verbatim
in the HTML report would:

1. Expose more data than the browser needs (the JS only does filtering and
   counting)
2. Broaden the browser-visible surface to include fields that may carry
   sensitive-adjacent data (config file contents, redaction metadata)
3. Inflate the HTML file size unnecessarily

Instead, the Rust renderer builds a **`ReportFilterData`** struct containing
only the fields the JS needs for table filtering:

```rust
#[derive(Serialize)]
struct ReportFilterData {
    packages: Vec<FilterablePackage>,     // name, version, release, arch, repo
    config_files: Vec<FilterableConfig>,  // path, kind
    services: Vec<FilterableService>,     // name, state
    scheduled: Vec<FilterableScheduled>,  // name/path, type
    users: Vec<FilterableUser>,           // name, uid
}

#[derive(Serialize)]
struct FilterablePackage {
    name: String,
    version: String,
    release: String,
    arch: String,
    repo: String,
}
// ... similar flat structs for other filterable sections
```

Sections that are never filterable (Storage, Kernel/Boot, Non-RPM, Warnings,
Redactions, Security) are excluded from the DTO entirely.

### Script-Safe Serialization

The DTO is embedded inside `<script type="application/json">`. Raw
`serde_json::to_string()` output is not safe for this context because JSON
values containing `</script>` or U+2028/U+2029 can break the HTML document.

**Required serialization rule:** After `serde_json::to_string()`, replace
these characters with their JSON unicode escape equivalents. All replacements
are valid JSON (RFC 8259 section 7) and `JSON.parse()` decodes them
transparently back to the original characters.

- `<` (U+003C) to `\u003c` --- prevents `</script>` and `<!--` from being parsed as HTML
- `>` (U+003E) to `\u003e` --- belt-and-suspenders: prevents close-tag interpretation
- U+2028 to `\u2028` --- line separator: valid JSON but breaks some JS contexts
- U+2029 to `\u2029` --- paragraph separator: same issue

This is the approach used by Django (`escapejs`), Rails (`json_escape`), and
Go (`template/html`). No ad-hoc string transforms. Every replacement is a
standard JSON unicode escape.

The function lives in `report.rs` as `fn script_safe_json(json: &str) ->
String` and is tested with adversarial inputs (see Verification Plan, proof
#6).

### Trusted Insertion Boundary

The DTO serialization and script-safe encoding happen entirely in Rust
(`report.rs`). The result is a `String` containing valid, script-safe JSON.

This string is passed to the minijinja template context as a pre-escaped
value. The template inserts it using minijinja's `|safe` filter (which
disables autoescaping for that value):

```html
<script type="application/json" id="report-filter-data">
{{ filter_data_json|safe }}
</script>
```

**Why `|safe` is correct here:** Minijinja's default autoescaping is for
HTML content (it would turn `"` into `&quot;`, breaking `JSON.parse()`).
The `|safe` filter bypasses HTML escaping because the value is already
encoded for its target context (a script block, not HTML content).

**The trust boundary:** `report.rs::script_safe_json()` is the single
function responsible for making the DTO safe for script-block embedding.
The template trusts its output via `|safe`. No other code path produces
script-block content. If a future change needs to embed additional JSON,
it must go through the same function.

### Content Security

- The DTO contains only display-safe fields (names, versions, paths). No
  config file contents, no redaction detail, no secret-adjacent data.
- The DTO is built from the post-redaction snapshot (same as all rendered
  content).
- The existing `redaction_2c_surfaces_test.rs` test suite (tests 7-11)
  already covers `report.html` as a redaction surface. Those tests will be
  updated for the new filename and must continue to pass, proving that
  planted secrets do not appear in either the rendered HTML or the embedded
  DTO.

### CSP Target

The rendered HTML includes a `<meta>` CSP header:

```html
<meta http-equiv="Content-Security-Policy"
      content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
```

This declares the report's security posture: no external resources, no
network access, only inline styles and scripts. `unsafe-inline` is required
because the CSS and JS are embedded in the document — there is no external
file to hash against. This is the correct tradeoff for a self-contained
offline artifact.

## Report Structure

### Layout Zones (top to bottom)

1. **Header bar** — dark background. "inspectah Migration Audit Report" +
   generation timestamp. Persistent warning badge if warnings > 0 (e.g.,
   "3 warnings" pill, visible without scrolling).

2. **Completeness warning** (conditional) — yellow banner with left border
   accent. Lists failed and degraded sections by name (as anchor links) with
   the single global reason. See Completeness Contract above.

3. **Source info** — hostname large and prominent. Second line: OS pretty
   name, arch, SELinux mode. Third line: baseline image reference + digest
   (if baseline comparison was run). For fleet snapshots, the fleet aggregate
   summary is visually grouped with source info (shared container/background),
   showing host count, section coverage table, baseline status
   (unanimous/provisional), and variant conflict count.

4. **Summary cards** — responsive grid. Cards for all "always rendered"
   sections (including empty ones showing "0") and present conditional
   sections, EXCEPT Warnings. Warnings are meta-commentary about the scan
   itself, not a findings category — they are surfaced in the header bar
   badge instead. Failed sections show "n/a" instead of a count. Absent
   sections are not in cards. Semantic markup: `<dl>` with `<dt>` for label,
   `<dd>` for count, so screen readers announce "Packages: 47" not "47" then
   "Packages" separately.

5. **TOC bar** — gray background, inline anchor links with counts. Section
   names match the section headers. Warning count highlighted in red. Failed
   sections marked with "failed" indicator. Empty sections (count 0) rendered
   as anchor links (they DO scroll to a rendered section with "No items
   detected" body). Absent sections not in TOC.

6. **Sections** — each wrapped in a `<details>`/`<summary>` block inside a
   bordered container. See Section Design below.

7. **Footer** — inspectah version. "See audit-report.md for the full report
   in Markdown format."

### Section Design

Every audit section follows a consistent pattern:

```
+--[ Section Header (summary element) ]------------------+
|  > Section Name (count badge) [state indicator]        |
+--------------------------------------------------------+
|  Filter: [________________] Showing X of Y items       |
|                                                         |
|  | Col 1 | Col 2 | Col 3 | Col 4 |                    |
|  |-------|-------|-------|--------|                    |
|  | data  | data  | data  | data   |                    |
+--------------------------------------------------------+
```

**Count badges** in the `<summary>` element:

- Simple: "Packages (47)"
- Rich: "Service State Changes (12 enabled, 3 masked)"
- Fleet conflicts: "Configuration Files (47, 3 conflicts)"
- Degraded: "Configuration Files (23)" + yellow "partial data" pill
- Failed: "Configuration Files" + red "data unavailable" pill (no count)
- Empty: "Non-RPM Software (0)" — grayed out text

**Warning section:** Red left border accent on the section container.

**Filter inputs** (tables with 10+ rows only):

- Visible `<label>` element: "Filter [section name]" — not placeholder only.
- `<input>` with `input` event handler filtering rows via the embedded DTO.
- Result count: "Showing X of Y [items]" updated on each keystroke.
- No-results state: visible "No matching items" message with
  `aria-live="polite"` for screen reader announcement.

### Sections Covered

| # | Section | Badge Format | Filter | Summary Card | Fleet |
|---|---------|-------------|--------|-------------|-------|
| 1 | Fleet Aggregate Summary | (N hosts) | No | No | Yes |
| 2 | Incomplete Sections | — | No | No | No |
| 3 | Baseline Comparison | — | No | No | No |
| 4 | Packages | (N) + version changes | Yes (10+) | Yes | No |
| 5 | Configuration Files | (N, K conflicts) | Yes (10+) | Yes | Variant sel. |
| 6 | Service State Changes | (N enabled, M masked) | Yes (10+) | Yes | Variant sel. |
| 7 | Storage | (N entries) | No | Yes | No |
| 8 | Kernel & Boot | (N items) | No | Yes | No |
| 9 | Scheduled Tasks | (N cron, M timers) | Yes (10+) | Yes | No |
| 10 | Security & Access Control | (mode) | No | Yes | No |
| 11 | Non-RPM Software | (N) | No | Yes | No |
| 12 | Redactions | (N redacted) | No | Yes | No |
| 13 | Warnings | (N) | No | **No** (header badge) | No |
| 14 | Users & Groups | (N) | Yes (10+) | Yes | No |

"Yes (10+)" = filter appears if the section has 10+ rows at render time.
Only the five filterable sections (Packages, Configuration Files, Service
State Changes, Scheduled Tasks, Users & Groups) have DTO support for
filtering. All other sections are never filterable.

## Interactive Features

All interactivity is **progressive enhancement** — the report is fully
readable with JS disabled.

### Collapse/Expand

Native `<details>`/`<summary>` elements. No JS needed for the toggle itself.
Free keyboard support (Enter/Space) and screen reader semantics.

Default state: all sections collapsed on initial load. The user scans count
badges and opens what's relevant.

### Table Filtering

Vanilla JS, ~30 lines. Each filterable table has an `<input>` above it. On
`input` events, the JS reads the corresponding section from the embedded
`ReportFilterData` DTO, matches against the query string, and toggles
`display:none` on non-matching `<tr>` elements. Updates the "Showing X of Y"
count.

### TOC Navigation

Anchor links in the TOC bar. On `hashchange`, the JS:

1. Finds the target `<details>` element
2. Opens it if collapsed (`element.open = true`)
3. Scrolls to it
4. Moves focus to the `<summary>` element of the target section

This ensures clicking a TOC link to a collapsed section actually reveals
content. The same handler also runs on initial page load if the URL
contains a hash fragment (e.g., `audit-report.html#packages`), so direct
links to sections work correctly. ~10 lines of JS.

### Print Support

A `beforeprint` event handler opens all `<details>` elements and saves their
prior state. An `afterprint` handler restores the original open/closed state.

`@media print` CSS block (~15 lines):

- Hides filter inputs, TOC bar, and header warning badge
- Sets `page-break-inside: avoid` on section containers
- Removes dark header background for ink-friendly printing
- Forces summary cards to a single column

The print handler does NOT use `@media print` to force `<details>` open
(browser support is inconsistent). It uses the JS event handlers.

### Embedded Report DTO

See the Embedded Report DTO section above for the data contract.

The DTO `<script>` block (uses `|safe` per the Trusted Insertion Boundary
contract):

```html
<script type="application/json" id="report-filter-data">
{{ filter_data_json|safe }}
</script>
```

The inline JS parses it once on load:

```javascript
const data = JSON.parse(
  document.getElementById('report-filter-data').textContent
);
```

## Responsive Behavior

### Breakpoints

The report uses two breakpoints, defined as CSS custom properties:

- `--report-bp-narrow: 600px` — below this, single-column layout
- `--report-bp-medium: 900px` — below this, reduced columns

### Component Behavior by Viewport

| Component | Wide (>900px) | Medium (600-900px) | Narrow (<600px) |
|-----------|--------------|-------------------|----------------|
| Summary cards | 5-column grid | 3-column grid | 2-column grid |
| TOC bar | Single-line, horizontal | Wraps to 2 lines | Hidden (sections are close enough to scroll) |
| Header bar | Logo left, timestamp right | Same | Stack vertically |
| Fleet summary | Side-by-side with source info | Stacked below source info | Stacked below source info |
| Filter inputs | Inline with result count | Stack: input above count | Stack: input above count |
| Data tables | Full width | Horizontal scroll (`overflow-x: auto`) | Horizontal scroll |
| Section badges | Inline after title | Inline after title | Wrap below title |

### TOC Overflow

At wide viewports, the TOC bar is a single horizontal line of anchor links.
When links exceed the container width (many sections), the TOC bar wraps
naturally — `flex-wrap: wrap` with `gap: 0.5rem`. No horizontal scroll on
the TOC itself.

At narrow viewports (<600px), the TOC bar is hidden entirely via
`display: none`. The collapsed section headers are visible enough for
direct scrolling at mobile widths.

## PatternFly Usage

### What Is Used

The vendored PatternFly 6 CSS provides:

- **CSS custom properties (design tokens):** `--pf-t--global--font--*`,
  `--pf-t--global--color--*`, `--pf-t--global--spacer--*`,
  `--pf-t--global--border--*`. These are the authoritative source for
  colors, fonts, spacing, and borders throughout the report.
- **Base typography and reset:** PatternFly's global reset and body styles.

### What Is Custom

The report uses custom CSS classes for components that don't map to
PatternFly components (PatternFly is a React component library — its CSS
classes assume React-rendered markup):

- `.report-section` — the `<details>` wrapper with border and badge styling
- `.report-header` — the dark header bar
- `.report-toc` — the TOC bar
- `.report-cards` — the summary card grid (uses PF spacing tokens)
- `.report-filter` — the filter input container
- `.badge`, `.badge-degraded`, `.badge-failed` — state indicator pills

**Rule:** All custom classes reference PF design tokens for colors, fonts,
and spacing. No hardcoded color values (`#f0ab00`), font stacks, or pixel
spacing in custom CSS. The vendored PF CSS is the design-system source of
truth.

### Custom CSS Location

The report's custom CSS (~80-100 lines) is authored in
`inspectah-pipeline/assets/report.css` and embedded via `include_str!()` in
the base template, inside a `<style>` block after the PF CSS.

## Accessibility

- **Keyboard:** `<details>`/`<summary>` provide native Enter/Space toggle.
  Filter `<input>` elements are standard form controls. TOC links are
  standard `<a>` elements. Focus management on TOC navigation moves focus
  to the target `<summary>` element.
- **Screen readers:** `<summary>` elements announce section name and count
  badge. Filter inputs have visible `<label>` elements. Filter no-results
  uses `aria-live="polite"`. Summary cards use `<dl>`/`<dt>`/`<dd>` for
  semantic count announcement.
- **Color:** All color-coded indicators (degraded yellow, warning red, failed
  red) also have text labels — color is never the sole signifier.
- **Responsive:** See Responsive Behavior section above.

## Template File Structure

```
inspectah-pipeline/
  assets/
    patternfly.min.css          # Vendored PF6 (~400KB)
    report.css                  # Custom report styles (~100 lines)
    report.js                   # Interactive enhancement (~100 lines)

  templates/
    report/
      base.html                 # DOCTYPE, head, inlined CSS/JS, body wrapper
      header.html               # Dark header bar + warning badge
      completeness.html         # Completeness warning banner (conditional)
      source-info.html          # Hostname, OS, baseline, fleet summary
      summary-cards.html        # Summary card grid
      toc.html                  # TOC bar
      section.html              # Reusable section macro (details/summary/badge)
      packages.html             # Packages table + version changes
      config.html               # Configuration Files table + conflict badges
      services.html             # Service State Changes table
      storage.html              # Storage/fstab table
      kernel.html               # Kernel & Boot params, sysctl, modules
      scheduled.html            # Scheduled Tasks (cron/timers/at)
      security.html             # Security & Access Control (SELinux)
      nonrpm.html               # Non-RPM Software
      users.html                # Users & Groups
      redactions.html           # Redactions (count + pointer)
      warnings.html             # Warnings list
      fleet-summary.html        # Fleet aggregate summary (conditional)

  src/render/
    report.rs                   # View-model construction + render_report()
    report_data.rs              # Shared computation (counts, badges, conflicts)
    audit.rs                    # Markdown renderer (updated to use report_data,
                                #   + Users & Groups section added)
    mod.rs                      # Updated: render audit-report.html
```

### Section Template Macro

```html
{% macro section(id, title, count, state="normal",
                 conflict_count=0, extra_badge="") %}
<div class="report-section
  {%- if state == 'failed' %} report-section--failed
  {%- elif state == 'degraded' %} report-section--degraded
  {%- elif id == 'warnings' %} report-section--warning
  {%- endif %}">
<details id="{{ id }}">
  <summary>
    {{ title }}
    {% if state == "failed" %}
      <span class="badge-failed">data unavailable</span>
    {% elif state == "degraded" %}
      <span class="badge">({{ count }}{% if conflict_count %},
        {{ conflict_count }} conflicts{% endif %})</span>
      <span class="badge-degraded">partial data</span>
    {% else %}
      <span class="badge">({{ count }}{% if conflict_count %},
        {{ conflict_count }} conflicts{% endif %})</span>
    {% endif %}
    {% if extra_badge %}
      <span class="badge-extra">{{ extra_badge }}</span>
    {% endif %}
  </summary>
  {% if state == "failed" %}
    <p class="failed-body">Data collection failed for this section.
    See the completeness warning above for details.</p>
  {% else %}
    {{ caller() }}
  {% endif %}
</details>
</div>
{% endmacro %}
```

## Rust Implementation

### View-Model

The `render_report()` function in `report.rs` builds a minijinja context
from the `InspectionSnapshot`:

1. Compute counts, badges, conflict tallies, section states via
   `report_data.rs`
2. Build the `ReportFilterData` DTO and serialize it with script-safe
   encoding
3. Build the template context with all section data + serialized DTO
4. Render via minijinja `Environment`
5. Return the complete HTML string

The function signature remains `pub fn render_report(snap:
&InspectionSnapshot, context: &RenderContext) -> String` — no API change.

### Shared Report Data

`report_data.rs` provides:

- `SectionState` enum: `Normal`, `Degraded`, `Failed`
- `section_state(id: InspectorId, completeness: &Completeness) -> SectionState`
- Package count and version change count
- Config file count with fleet conflict count (shared with `audit.rs`)
- Service count with enabled/disabled/masked breakdown
- Storage entry count, kernel/boot item count
- Scheduled task count with type breakdown
- Non-RPM item count, user/group count
- Warning count, redaction count
- Fleet metadata (host count, section coverage, baseline status)

### Build Integration

PatternFly CSS, report CSS, and report JS are vendored in `assets/` and
embedded via `include_str!()`. No `build.rs` needed — these are static
assets tracked in git.

## Verification Plan

### Proof Matrix

| # | What Is Proved | Test Type | Mechanism |
|---|---------------|-----------|-----------|
| 1 | **Section parity** | Unit | For a fully-populated snapshot, extract all `## ` headings from `render_audit()` output and all `<details id="...">` IDs from `render_report()` output. Assert they map 1:1 per the parity table. |
| 2 | **Empty section rendering** | Unit | Render a snapshot with zero packages. Assert the Packages section has `(0)` badge and "No items detected" body. |
| 3 | **Failed section rendering** | Unit | Render a snapshot with `Completeness::Incomplete { failed_sections: [Config] }`. Assert the Configuration Files section has "data unavailable" badge and failed body text. Assert it does NOT show "(0)". |
| 4 | **Degraded section rendering** | Unit | Render a snapshot with `Completeness::Partial { degraded_sections: [Services] }`. Assert the Service State Changes section has "partial data" pill and still shows its data. |
| 5 | **Failed vs empty distinction** | Unit | Same snapshot: Config is failed, Non-RPM has zero items. Assert Config shows "data unavailable" and Non-RPM shows "(0)". |
| 6 | **Script-safe JSON embedding** | Unit | Plant `</script>`, `<!--`, U+2028, U+2029 in snapshot values. Assert the rendered HTML is well-formed XML/HTML (no premature script close). Assert the dangerous strings do not appear literally in the output. |
| 7 | **Redaction surface** | Integration | Extend `redaction_2c_surfaces_test.rs` test 11 for the new filename. Plant secrets in snapshot, run redaction, render HTML. Assert no planted secret appears in rendered output (HTML or embedded DTO). |
| 8 | **DTO minimization** | Unit | Serialize `ReportFilterData` from a snapshot with redaction findings. Assert the DTO JSON does not contain redaction paths, patterns, or remediation text. |
| 9 | **Offline / no-CDN** | Unit | Render a report. Assert the output contains zero `http://` or `https://` URLs. (The vendored PF CSS and inline JS have no external references.) |
| 10 | **Filename rename** | Integration | After `render_all()`, assert `audit-report.html` exists and `report.html` does not. Run `grep -r 'report\.html'` on the pipeline crate and assert zero hits. |
| 11 | **HTML structure snapshot** | Snapshot (insta) | Render a known snapshot. Snapshot the HTML body structure (exclude vendored CSS and DTO JSON to keep the snapshot reviewable). |
| 12 | **Fleet rendering** | Unit | Render a fleet snapshot. Assert fleet summary section, variant conflict badges, per-section host counts are present. |
| 13 | **Escaping** | Unit | Plant `<script>alert(1)</script>` in snapshot values. Assert the literal string does not appear in rendered HTML. Assert the escaped version does. |
| 14 | **Users & Groups in markdown** | Unit | Render a snapshot with user/group data through `render_audit()`. Assert a `## Users & Groups` section appears. |
| 15 | **Users & Groups safe-field whitelist** | Unit | Render a snapshot with a `UserGroupDecision` containing `password_hash` and `ssh_keys`. Assert neither value appears in the rendered HTML, the rendered markdown, or the embedded DTO. Assert whitelisted fields (name, uid, shell) DO appear. |
| 16 | **Failed conditional section** | Unit | Render a snapshot where Storage inspector failed (`failed_sections: [Storage]`) but `snap.storage` is `None`. Assert the Storage section renders as "data unavailable" (not absent). |

### Snapshot Test Strategy

Full-document `insta` snapshots are impractical with ~400KB of vendored CSS
and variable-length embedded JSON. Instead:

- **Structure snapshot:** Snapshot the HTML body only (content between
  `<body>` and `</body>`), with the embedded DTO replaced by a
  `[FILTER_DATA_PLACEHOLDER]` marker and the vendored CSS replaced by
  `[PATTERNFLY_CSS_PLACEHOLDER]`. This keeps the snapshot focused on
  structure and reviewable.
- **Targeted assertions:** Separate unit tests for the vendored CSS
  (present, non-empty), the DTO (valid JSON, correct fields), and the
  offline contract (no external URLs).

## Migration Path

Incremental, confidence-building:

1. **Add minijinja dependency + template skeleton + vendored CSS.** Create
   `templates/` directory, vendor PatternFly CSS, add `report_data.rs`
   stub. Wire up minijinja `Environment` in `report.rs`. Existing tests
   still pass (output is identical).

2. **Port existing output to templates.** Move the current HTML output from
   `format!()` into `base.html` + section templates. Regression tests verify
   output hasn't changed (modulo whitespace). This is the confidence step.

3. **Add new sections one at a time.** Each section gets its own template
   file and test coverage. Section order matches the parity table.

4. **Add interactive features.** Inline JS for filtering, TOC navigation,
   print support. The JS reads the embedded report DTO.

5. **Add Users & Groups to markdown renderer.** Update `audit.rs` to render
   a `## Users & Groups` section, using `report_data.rs` for counts.

6. **Rename artifact.** Execute the rename sweep (see Rename Sweep table).
   Verify with `grep -rn 'report\.html'`.

7. **Doc updates.** Update README, getting-started, architecture, output
   artifacts, tutorials, how-to guides. Replace "HTML dashboard" with
   "HTML audit report" everywhere.

## Future Work (not in scope)

- **"What to do next" affordance:** Connect findings to actions — which are
  blockers vs. informational. Requires upstream data model work.
- **PatternFly CSS optimization:** Strip unused CSS rules to reduce the
  ~400KB payload. Only worth doing if file size becomes a concern.
- **Sortable table columns:** Click column headers to sort. Not needed for
  v1 — filter covers the primary use case.
- **Per-section completeness reasons:** Requires extending the
  `Completeness` enum to carry per-`InspectorId` reason strings. Would
  enable section-level degradation reason pills. Tracked as a data model
  change, not a rendering change.
