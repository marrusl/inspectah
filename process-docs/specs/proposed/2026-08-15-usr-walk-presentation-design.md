# Unmanaged /usr Presentation Design

**Status:** Proposed (design note; implementation targets v0.9.0-beta.3)
**Date:** 2026-08-15

## Problem

The /usr walk is a complete detection feature with no output surface.
`scan_unmanaged_files()` builds the full RPM-owned path set, walks /usr,
collapses unowned paths to their shallowest unowned ancestor, and stores
the result in `usr_entries` (`crates/collect/src/inspectors/nonrpm.rs:2125-2133`,
`crates/collect/src/inspectors/nonrpm.rs:2357-2434`). Nothing reads it:

- Aggregate merge discards it (`crates/core/src/aggregate/merge.rs:1819-1824`
  sets `usr_entries: Vec::new()`).
- The web DTOs project only `items`; no /usr field exists
  (`crates/web/src/web_types.rs:52-67`, `crates/web/src/adapter.rs:401-409`).
- The Containerfile unmanaged renderer reads only `items`
  (`crates/pipeline/src/render/unmanaged.rs`).
- The HTML report never references `usr_entries`
  (`crates/pipeline/src/render/report.rs`).

This matters because unmanaged /usr content is a migration blocker in image
mode: /usr ships from the image and is read-only at runtime, so anything
under /usr that no package owns will silently vanish on rebuild unless the
user decides what to do with it.

This note designs the presentation. The implementation plan designs the
types and the wiring.

## Settled product decisions (design within these)

These were decided in the 2026-08-15 product session and are not reopened
here:

1. The /usr walk is a **general inspectah feature**: single-host report,
   aggregate, refine, export. Factor (future) consumes the decision; it
   does not own this surface.
2. Findings get their **own section, "Unmanaged /usr"**, adjacent to
   Unmanaged Files and never folded into it. The existing Unmanaged Files
   surface means "bundleable content you may choose to copy" (scan roots
   /opt, /srv, /usr/local; `crates/collect/src/inspectors/nonrpm.rs:1940-1943`).
   /usr findings mean "this estate is not image-clean." Different user
   problem, different section.
3. **Ordinary Actionable findings, standard include/exclude.** /usr
   entries follow the standard category process: default-include on a
   single host, with the same include/exclude toggle as any other
   Actionable finding. Aggregates use the existing zone machinery: 100
   percent prevalence auto-includes, partial prevalence lands in review
   zones. No bespoke disposition model.
4. **Export behavior matches every other Actionable family.** No
   /usr-specific gating; beta.3 needs none.

## Design tenets

- **The section states a decision problem, not an inventory.** Every
  entry carries the standard include/exclude toggle, default-included,
  and the section reports what is included, not what remains undecided.
- **Density is organized, not reduced.** Entries are already collapsed to
  shallowest unowned ancestors at collection time; the section presents
  that rollup directly, sorted so the largest debt is on top.
- **The user decides; the tool remembers.** Entries default to included,
  matching standard Actionable finding behavior; the user can exclude
  any entry, and export proceeds on whatever the toggle states.
- **Same interaction vocabulary as the rest of refine.** Grid rows, roving
  focus, one status region, batch actions. Nothing novel to learn.

## 1. Section identity and placement

### Section

- **ID:** `unmanaged_usr`. **Label:** "Unmanaged /usr".
- Triage section (`is_triage: true`).

### Refine web UI (single-host and aggregate)

- Joins the existing Software group as a fourth sibling, directly after
  Unmanaged Files: `non_rpm_software`, `language_packages`,
  `unmanaged_files`, `unmanaged_usr`
  (`crates/pipeline/src/section_group.rs:172-188`).
- Group-based number-key navigation (1-8 jumps to groups,
  `crates/web/ui/src/hooks/useKeyboard.ts`) needs no new binding; the
  section is reachable inside the Software group. The legacy flat list
  gains the section after `unmanaged_files`.
- Sidebar badge: whatever convention the sibling Software-group sections
  use for Actionable findings; no /usr-specific counting logic or
  needs-review state to badge against.

### HTML report

- Own section under the "Software & Files" TOC group, after the Non-RPM
  Software content (`crates/pipeline/src/render/report.rs:304-315`).
- TOC count: total entries. Any entries at all trigger the section's
  attention treatment, consistent with how the report styles degraded or
  warning states today (exact styling hook: needs verification at
  implementation time); an empty section gets the clean-state treatment
  instead (see Empty and absent states, below).
- The report is read-only. It renders the framing copy, the entry table
  (path, kind, files, size, included/excluded state), and the remediation
  guidance. A raw scan renders every row at the default include state,
  matching every other Actionable finding family; refine decisions appear
  once the report is rendered from a refined snapshot.

### Section framing copy (all surfaces)

Lead with the blocker and what to do. Draft:

> In image mode, /usr ships from the container image and stays read-only
> at runtime. The files below live under /usr on this host but belong to
> no RPM package, so a rebuilt image will not carry them unless you
> include them in the export. For content that should be package-managed,
> building an RPM that owns it is the durable fix; use include only for
> what genuinely needs to travel with the image as-is.

### Empty and absent states

The /usr walk runs inside `scan_unmanaged_files()`, which only runs with
`--include-unmanaged` (`crates/cli/src/commands/scan.rs:695-729`). Three
states:

1. **Section absent** (`unmanaged_files: None`): the walk did not run.
   Refine and report show the section with a not-scanned state: "This
   snapshot was collected without --include-unmanaged, so /usr was not
   checked. Re-scan with --include-unmanaged to check it."
2. **Section present, `usr_entries` empty:** the walk ran and found
   nothing. This is the image-clean signal and deserves positive copy:
   "Every file under /usr on this host is owned by an RPM package. This
   host's /usr is image-clean." Older snapshots predating the walk also
   deserialize to empty; per the repo convention there is no old-tarball
   compatibility, re-scan instead, so the clean reading stands.
3. **Entries present:** the full section.

## 2. The entry row

Rows follow the existing decision-grid idiom (`role="grid"`, `role="row"`,
`role="gridcell"`, roving tabindex, `aria-rowindex`), matching
`crates/web/ui/src/components/DecisionItem.tsx:276-397` and the flat
focus-index model in `DecisionList.tsx`. This section is a decision
surface, so it uses the grid idiom rather than the list/checkbox idiom of
`UnmanagedFileList.tsx`.

### Row content (cells, in order)

1. **Selection checkbox** (for batch actions).
2. **Path**, monospace, truncated middle on overflow with full path in
   the accessible name and title.
3. **Kind badge:** "Directory" with rolled-up count ("Directory, 214
   files") or the single-file type ("ELF binary", "Script", "Symlink",
   "File"). Kind comes from an explicit entry-kind field, not inference
   (see § 6; today single-file vs collapsed-directory is only inferable
   from `file_type != Other`, which fails for unclassifiable single
   files, `crates/collect/src/inspectors/nonrpm.rs:2417-2423`).
4. **Total size**, human units.
5. **Host prevalence** (aggregate mode only): "14/20 hosts".
6. **Include/exclude toggle** (§ 3), the standard Actionable-finding
   control.

No expand affordance in beta.3: entries carry no child path list, so
there is nothing to drill into. A representative child sample is a future
improvement (§ 7).

### Ordering

Default sort: total size descending, then path ascending. The largest
vendoring debt surfaces first; ties read alphabetically. No sort controls
in beta.3.

### Keyboard interaction

- The grid is one tab stop. Arrow Up/Down move row focus (roving
  tabindex); Home/End jump to first/last row.
- **Space** on a focused row toggles its selection checkbox.
- **Enter** on a focused row toggles its include/exclude state, the same
  interaction the sibling Actionable sections use.
- Shift+click on a row checkbox extends selection from the last selected
  row (range select). A select-all checkbox lives in the grid header.
- Section-level shortcuts (/, ?, number keys, Ctrl chords) keep their
  existing global behavior; none are shadowed by the grid.

### Screen reader semantics

- Grid label: "Unmanaged /usr entries".
- Row accessible name is composed from the cells: path, kind and count,
  size, prevalence (aggregate), current include/exclude state. Example:
  "/usr/lib/custom-agent, directory, 214 files, 38 megabytes, 14 of 20
  hosts, included."
- One `role="status"` `aria-live="polite"` region per section (matching
  the existing pattern in `UnmanagedFileList.tsx`) announces
  include/exclude changes and batch results with a single contextual
  message, for example: "/usr/lib/custom-agent excluded. 12 entries
  excluded, 202 included." Never announce a bare count.
- All interactive targets meet a 44 px minimum hit size; focus is always
  visibly ringed.

## 3. Include and exclude

/usr entries are ordinary Actionable findings
(`crates/core/src/types/finding.rs:5-17`): default-included on a single
host, with the same include/exclude toggle and interaction idiom every
other Actionable section in refine already uses. Aggregates use the
existing zone machinery: 100 percent prevalence auto-includes, partial
prevalence lands in a review zone, exactly like every other artifact
family in aggregate refine.

Batch include and batch exclude follow the standard toolbar pattern used
elsewhere in refine.

## 4. Export behavior

### What an included entry produces

- Included entries render as COPY directives into the image, in a
  dedicated Containerfile block headed `=== Unmanaged /usr ===`, following
  the existing Tier 2 unmanaged pattern (warning comment block plus COPY
  lines, `crates/pipeline/src/render/unmanaged.rs`). A collapsed
  directory entry copies the whole subtree; a single-file entry copies
  the file.
- The warning comment states the contract plainly: this content is owned
  by no package, updates are the user's responsibility, and building an
  RPM that owns it is the durable fix.
- Content sourcing: COPY needs the bytes in the build context. Tier 2
  unmanaged files already have a scan-time bundling flow
  (`crates/cli/src/commands/scan.rs:741-745`); whether /usr content rides
  the same bundling mechanism or the block lists paths the user must
  supply in the build context is an implementation-plan decision (needs
  verification of the Tier 2 bundling mechanics). The design requirement
  is only: when the archive lacks the content, the block says so per
  path, and the export readiness summary repeats it.

### Export preview

- The Containerfile panel shows the `=== Unmanaged /usr ===` block like
  any other section, so toggling include/exclude visibly adds and
  removes COPY lines.
- The section header shows the vendoring cost: "6 entries included,
  212 MB copied into the image." Users should see three gigabytes coming
  before the build does.

### Export readiness

The audit report gains an Unmanaged /usr section: counts included and
excluded, and the included-bytes total. Export readiness follows the
same behavior as every other Actionable finding family; beta.3 needs no
/usr-specific export gate.

## 5. Aggregate treatment

### What beta.3 minimally needs

Aggregate merge currently discards `usr_entries`
(`crates/core/src/aggregate/merge.rs:1819-1824`), so aggregate and
factor see nothing. Verified. Beta.3 minimum:

- **Union by path with prevalence.** Merge entries across hosts keyed by
  path, attaching the same `AggregatePrevalence` (count, total, hosts)
  the other merged families carry. Path is the stable identity for
  beta.3.
- **Representative counts and sizes.** Per-host `file_count` and
  `total_size_bytes` can differ for the same path. Carry the maximum and
  a varies flag; render as "up to 214 files, up to 38 MB" when hosts
  disagree. Full per-host variance display is future work.
- No content hashes exist for /usr entries, so variant detection (same
  path, different content) is explicitly out of scope until subtree
  digests arrive (§ 7).

### Aggregate UI

- Same section, same rows, plus the prevalence cell ("14/20 hosts").
- One include/exclude decision per path, applied fleet-wide, exactly
  like other aggregate decisions. The row's accessible name includes
  prevalence so the scope of the decision is audible.
- Default sort stays size descending; prevalence is visible, not the
  sort key. An entry on 2 of 200 hosts at 3 GB still belongs on top.
- Mode-divergence rule: every new behavior here must be checked in both
  `RefineMode::SingleHost` and `RefineMode::Aggregate` (per the
  aggregate-vs-single-host-behavioral-split skill).

## 6. Data-model gaps flagged for implementation

Flagged only; the implementation plan owns the type design.

1. **Entry kind:** explicit single-file vs collapsed-directory field.
   Today it is inferred from `file_type != Other`, which is ambiguous
   for single files that classify as Other.
2. **Aggregate merge:** stop discarding `usr_entries`; path-keyed union
   with prevalence, max-plus-varies for counts and sizes (§ 5).
3. **Web DTOs:** new /usr section DTO carrying path, kind, count, size,
   prevalence, and the include/exclude state, matching how other
   Actionable-finding DTOs already carry it. Single-host adapter and
   aggregate handlers both project it.
4. **Projection and export:** `usr_entries` must survive refine
   projection into the exported snapshot; factor later consumes the
   ordinary decisions from there. Whether the current projection passes
   the section through untouched needs verification.
5. **Renderers:** Containerfile block (§ 4), audit report section,
   HTML report section.

## 7. Must have vs future improvement

### Must have (beta.3)

- Aggregate merge preserves `usr_entries` with path prevalence.
- Schema bump for the entry-kind field.
- Refine web section in single-host and aggregate: grid rows, the
  standard include/exclude toggle, batch toolbar, keyboard and screen
  reader contract, empty and not-scanned states.
- HTML report section with framing copy and entry table.
- Containerfile COPY block for included entries plus warning comments;
  included-bytes total in the section header and preview.
- Audit report counts (included and excluded) and included-bytes total.

### Future improvement (explicitly not beta.3)

- **Subtree digests** and change tracking for collapsed directories;
  variant detection across hosts.
- **Representative child samples** per collapsed directory, enabling
  row drill-down.
- **Entry notes:** an optional free-text note on any entry, surfaced in
  the audit report.
- **Per-host variance display** for counts and sizes in aggregate mode;
  sort controls.
- **TUI parity:** the TUI single-host screen gains the section following
  the web semantics.
- **Scan gating revisit:** the /usr walk currently runs only with
  `--include-unmanaged`. Running it unconditionally (it is a blocker
  signal, independent of Tier 2 bundling) is a product call to make
  separately.
- Factor consumption of these decisions as a structural signal.

## Needs verification at implementation time

- Tier 2 unmanaged bundling mechanics, and whether /usr content can ride
  the same path into the build context (§ 4).
- Whether refine projection passes `usr_entries` through to the exported
  snapshot untouched (§ 6).
- The HTML report's existing attention-state styling hooks (§ 1).

## Related

- Backlog: gap 3 of the extended-findings enumeration; split out of the
  beta.2 correctness run so presentation could be designed deliberately.
- Skills consulted: aggregate-vs-single-host-behavioral-split,
  snapshot-schema-versioning, codebase-layout.
