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
   aggregate, refine, export. Factor (future) consumes resolved state; it
   does not own this surface.
2. Findings get their **own section, "Unmanaged /usr"**, adjacent to
   Unmanaged Files and never folded into it. The existing Unmanaged Files
   surface means "bundleable content you may choose to copy" (scan roots
   /opt, /srv, /usr/local; `crates/collect/src/inspectors/nonrpm.rs:1940-1943`).
   /usr findings mean "this estate is not image-clean." Different user
   problem, different section.
3. **Per-entry dispositions, four states:** include-in-export (COPY into
   the image), package-it-properly, remove, approved exception.
   Include-in-export is first-class; it preserves the vendoring migration
   path for users who want to carry every unexpected binary and script
   into the image deliberately. A bare include/exclude toggle undersells
   how blocking this content is.
4. Full resolve-before-export gating arrives later with factor. Beta.3
   ships the section and dispositions; unresolved entries surface at
   export as advisory, never as a gate.

## Design tenets

- **The section states a decision problem, not an inventory.** Every entry
  carries a disposition control and the section reports how many entries
  still need one.
- **Density is organized, not reduced.** Entries are already collapsed to
  shallowest unowned ancestors at collection time; the section presents
  that rollup directly, sorted so the largest debt is on top.
- **The user decides; the tool remembers.** No disposition is preselected.
  Export proceeds regardless, and undecided entries are reported, not
  silently resolved.
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
- Sidebar badge: count of entries that still need review (not total
  entries). A section with 40 entries all dispositioned shows 0 and reads
  as done.

### HTML report

- Own section under the "Software & Files" TOC group, after the Non-RPM
  Software content (`crates/pipeline/src/render/report.rs:304-315`).
- TOC count: total entries. When any entry lacks a disposition, the
  section renders an attention treatment consistent with how the report
  styles degraded or warning states today (exact styling hook: needs
  verification at implementation time).
- The report is read-only. It renders the framing copy, the entry table
  (path, kind, files, size, disposition when set), and the remediation
  guidance. Dispositions appear when the report is rendered from a refined
  snapshot; a raw scan renders the table with a "needs review" state for
  every row.

### Section framing copy (all surfaces)

Lead with the blocker and what to do. Draft:

> In image mode, /usr ships from the container image and stays read-only
> at runtime. The files below live under /usr on this host but belong to
> no RPM package, so a rebuilt image will not carry them. Decide what
> happens to each entry: include it in the export so it is copied into
> the image, repackage it properly, remove it, or record it as an
> approved exception.

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
6. **Disposition control** (§ 3).

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
- **Enter** on a focused row opens its disposition menu.
- Shift+click on a row checkbox extends selection from the last selected
  row (range select). A select-all checkbox lives in the grid header.
- Section-level shortcuts (/, ?, number keys, Ctrl chords) keep their
  existing global behavior; none are shadowed by the grid.

### Screen reader semantics

- Grid label: "Unmanaged /usr entries".
- Row accessible name is composed from the cells: path, kind and count,
  size, prevalence (aggregate), current disposition. Example: "/usr/lib/
  custom-agent, directory, 214 files, 38 megabytes, 14 of 20 hosts,
  needs review."
- One `role="status"` `aria-live="polite"` region per section (matching
  the existing pattern in `UnmanagedFileList.tsx`) announces disposition
  changes and batch results with a single contextual message, for
  example: "/usr/lib/custom-agent set to include in export. 12 entries
  still need review." Never announce a bare count.
- All interactive targets meet a 44 px minimum hit size; focus is always
  visibly ringed.

## 3. The disposition interaction

### The control

A per-row **menu button** showing the current state as a compact pill.
Four commands plus the initial state:

| State | Pill label | Menu item copy |
|---|---|---|
| (default) | Needs review | not a menu item; the unset state |
| include-in-export | Include | Include in export: COPY this into the image |
| package-it-properly | Package | Package it properly: build an RPM that owns it |
| remove | Remove | Remove: drop it from the migrated system |
| approved exception | Exception | Approved exception: accepted as-is, keep a record |

Behavior:

- Button carries `aria-haspopup="menu"` and an accessible name of
  "Disposition: <current state>". Enter/Space opens the menu; Arrow keys
  move between items (`role="menuitemradio"`, `aria-checked` on the
  current state); Enter selects; Escape closes without change. Focus
  returns to the button on close.
- Selecting any state replaces the previous one; re-selecting the current
  state closes the menu unchanged. Returning an entry to "needs review"
  is done with undo (Ctrl+Z), which must cover disposition ops like every
  other refine decision.
- "Needs review" pills get the section's attention styling; the four set
  states get neutral pill styling with distinct labels. Color alone never
  carries the distinction.

### Default state: needs review (recommendation, with rationale)

**Strong recommendation:** no disposition is preselected. The persisted
disposition field defaults to an explicit unreviewed state, rendered as
"Needs review".

Rationale:

- The section exists because this content blocks a trustworthy image
  until a human looks at it. Any preselected disposition makes that
  decision silently. Defaulting to include-in-export (what the current
  type comment implies, `crates/core/src/types/nonrpm.rs:192-194`)
  silently vendors unowned content into the image; defaulting to
  package-it-properly fabricates work items nobody confirmed.
- "Needs review" respects blocker-ness without blocking anything else:
  an unreviewed entry produces no COPY output and no export gate. The
  rest of the export proceeds untouched, and the unreviewed count rides
  along as an advisory (§ 4).
- It makes progress measurable. The sidebar badge, the section header,
  and export readiness all count the same thing: entries still at the
  default.

Alternative considered: defaulting to include-in-export, so that "export
everything" is zero-click. Rejected as the default because it converts
the blocker signal into silent behavior, but the same outcome stays one
action away via select-all plus batch Include.

### Batch actions

- Selecting one or more rows reveals a toolbar (`role="toolbar"`, label
  "Batch disposition") above the grid: the four disposition buttons, the
  selected count, and Clear selection.
- Applying a batch disposition sets every selected entry, clears the
  selection, and announces once: "9 entries set to include in export.
  3 entries still need review."
- Batch application is a single undo step.

### Mapping onto the existing finding model

`FindingKind` is Actionable/Advisory/Inventory
(`crates/core/src/types/finding.rs:5-17`). It cannot express four
dispositions plus an unreviewed default, and the non-toggleable rule for
Advisory/Inventory (`with_include` passes them through unchanged) is the
wrong contract for a control with four live states. Semantically:

- **include-in-export** behaves like Actionable include=true: it is the
  only state that produces build output.
- **package-it-properly** and **remove** behave like Actionable
  include=false for rendering, but carry distinct intent that the audit
  report and (later) factor must see.
- **approved exception** is the analog of an acknowledged advisory:
  no output, recorded rationale, done.
- **needs review** is a state the current model has no word for.

Beta.3 therefore needs a dedicated disposition type on `UnmanagedUsrEntry`
(five values: the four states plus unreviewed as the serde-named default)
instead of the current `FindingKind` field. This note flags the need; the
implementation plan designs the type. Constraints the plan inherits:

- Name the serde default explicitly (per the
  finding-disposition-serde-defaults skill).
- The web DTO carries the disposition as a tagged union, never collapsed
  to a bool (per the web-disposition-contract skill: hand-built DTOs that
  collapse with `is_included()` are exactly the trap this section would
  fall into).
- Snapshot schema version bumps with the type change.
- A new refine op kind covers disposition changes, integrated with
  undo/redo and autosave replay.

## 4. Export behavior

### What include-in-export produces

- Included entries render as COPY directives into the image, in a
  dedicated Containerfile block headed `=== Unmanaged /usr ===`, following
  the existing Tier 2 unmanaged pattern (warning comment block plus COPY
  lines, `crates/pipeline/src/render/unmanaged.rs`). A collapsed
  directory entry copies the whole subtree; a single-file entry copies
  the file.
- The warning comment states the contract plainly: this content is owned
  by no package, updates are the user's responsibility, and packaging it
  properly is the durable fix.
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
  any other section, so toggling dispositions visibly adds and removes
  COPY lines.
- The section header shows the vendoring cost: "6 entries included,
  212 MB copied into the image." Users should see three gigabytes coming
  before the build does.

### Export readiness (advisory in beta.3)

- Unresolved means: entries at needs-review. Package-it-properly and
  remove entries are resolved decisions that produce no output; they are
  work items, not blockers of the export itself.
- At export, the audit report gains an Unmanaged /usr section: counts by
  disposition, the unreviewed list, and the included-bytes total. The
  Containerfile carries a one-line comment when unreviewed entries exist:
  `# NOTE: N unmanaged /usr entries were not reviewed; see the audit report.`
- If the export flow surfaces pre-export warnings in the UI today, the
  unreviewed count joins them (needs verification of which surface
  exists). No gating: the export button never disables over /usr state
  in beta.3. Resolution gating is factor-era (§ 7).

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
- One disposition per path, applied fleet-wide, exactly like other
  aggregate decisions. The row's accessible name includes prevalence so
  the scope of the decision is audible.
- Default sort stays size descending; prevalence is visible, not the
  sort key. An entry on 2 of 200 hosts at 3 GB still belongs on top.
- Mode-divergence rule: every new behavior here must be checked in both
  `RefineMode::SingleHost` and `RefineMode::Aggregate` (per the
  aggregate-vs-single-host-behavioral-split skill).

## 6. Data-model gaps flagged for implementation

Flagged only; the implementation plan owns the type design.

1. **Disposition type:** five-state enum on `UnmanagedUsrEntry` replacing
   `FindingKind` (§ 3). Named serde default; schema version bump.
2. **Entry kind:** explicit single-file vs collapsed-directory field.
   Today it is inferred from `file_type != Other`, which is ambiguous
   for single files that classify as Other.
3. **Aggregate merge:** stop discarding `usr_entries`; path-keyed union
   with prevalence, max-plus-varies for counts and sizes (§ 5).
4. **Web DTOs:** new /usr section DTO carrying path, kind, count, size,
   prevalence, and the tagged disposition. Single-host adapter and
   aggregate handlers both project it.
5. **Refine ops:** disposition-set op (single and batch) with undo/redo,
   autosave replay, and the same non-regression discipline the
   web-disposition-contract skill documents for advisory toggles.
6. **Projection and export:** `usr_entries` with dispositions must
   survive refine projection into the exported snapshot; factor later
   consumes resolved state from there. Whether the current projection
   passes the section through untouched needs verification.
7. **Renderers:** Containerfile block (§ 4), audit report section,
   HTML report section.

## 7. Must have vs future improvement

### Must have (beta.3)

- Aggregate merge preserves `usr_entries` with path prevalence.
- Schema bump; entry-kind field; five-state disposition model.
- Refine web section in single-host and aggregate: grid rows, disposition
  menu, batch toolbar, keyboard and screen reader contract, empty and
  not-scanned states, needs-review badge.
- HTML report section with framing copy and entry table.
- Containerfile COPY block for included entries plus warning comments;
  included-bytes total in the section header and preview.
- Audit report counts and unreviewed list; advisory comment in the
  Containerfile when unreviewed entries remain.

### Future improvement (explicitly not beta.3)

- **Resolution gating at export** (factor era): export blocked or
  acknowledged-only while entries need review.
- **Subtree digests** and change tracking for collapsed directories;
  variant detection across hosts.
- **Representative child samples** per collapsed directory, enabling
  row drill-down.
- **Exception rationale notes:** a free-text note on approved exceptions,
  surfaced in the audit report.
- **Per-host variance display** for counts and sizes in aggregate mode;
  sort controls.
- **TUI parity:** the TUI single-host screen gains the section following
  the web semantics.
- **Scan gating revisit:** the /usr walk currently runs only with
  `--include-unmanaged`. Running it unconditionally (it is a blocker
  signal, independent of Tier 2 bundling) is a product call to make
  separately.
- Factor consumption of resolved /usr state as a structural signal.

## Needs verification at implementation time

- Tier 2 unmanaged bundling mechanics, and whether /usr content can ride
  the same path into the build context (§ 4).
- Which UI surface, if any, shows pre-export warnings today (§ 4).
- Whether refine projection passes `usr_entries` through to the exported
  snapshot untouched (§ 6).
- The HTML report's existing attention-state styling hooks (§ 1).

## Related

- Backlog: gap 3 of the extended-findings enumeration; split out of the
  beta.2 correctness run so presentation could be designed deliberately.
- Skills consulted: web-disposition-contract,
  finding-disposition-serde-defaults,
  aggregate-vs-single-host-behavioral-split, snapshot-schema-versioning,
  codebase-layout.
