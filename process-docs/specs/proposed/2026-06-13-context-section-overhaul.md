# Context Section Layout Overhaul

## Summary

Three reference sections — Version Changes, Networking, and Kernel & Boot —
currently render through the generic `ContextItem` flat list. This spec
replaces them with purpose-built layouts that match how sysadmins actually
scan migration data.

**Scope:** Adapter restructuring (Rust) + one new frontend component
(Version Changes table). Networking and Kernel & Boot use the existing
`ContextList` subsection rendering — adapter-only changes.

## Sections

### 1. Version Changes → Grouped Table

**Problem:** Upgrades and downgrades are interleaved in a flat list. Each
renders as a ContextItem with title `"▼ httpd.x86_64"` and subtitle
`"2.4.57 → 2.4.51 (downgrade)"`. No columnar alignment, no visual
distinction between risk levels.

**Design:** Replace the ContextItem list with a grouped table component.

- **Two groups:** Downgrades first (they're the actionable risk), then
  Upgrades. Each group has a header row with count and color accent.
- **Columns:** Package (name.arch), Host Version, Target Version. No
  explicit "Direction" column — the group header communicates that.
- **Downgrade group:** Red accent (PatternFly `pf-m-danger` left border
  or subtle background tint `--pf-t--global--color--status--danger--default`
  at 5-8% opacity). Header: `"▼ Downgrades (N)"`.
- **Upgrade group:** Green accent. Header: `"▲ Upgrades (N)"`.
- **Row styling:** Compact — 6px vertical padding. Monospace for version
  strings. Package name in medium weight.
- **Empty states:** If only upgrades exist, omit the Downgrades header
  entirely (don't show "Downgrades (0)"). Same for the reverse.
- **Existing empty reasons preserved:** The adapter already handles
  `data_unavailable` and `zero_drift` empty states — the new component
  must pass these through to `ContextList`'s EmptyState or render its own.

**Accessibility:**
- Group headers: `role="row"` with `aria-label="N downgrades"`.
- Table uses PatternFly `Table` with `Thead`/`Tbody` grouping.
- Column headers in `Thead` for screen reader column identification.

**Files changed:**
- `crates/web/src/adapter.rs` — `web_version_changes_section()` emits
  two `ContextSubsection`s (downgrades, upgrades) instead of flat items.
  OR: the adapter emits structured data and the frontend renders a
  dedicated component. Preferred: **dedicated component** since the table
  layout doesn't fit the ContextItem model.
- `crates/web/ui/src/components/VersionChangesTable.tsx` — NEW. Receives
  `VersionChangeEntry[]` from `ViewResponse` (already available — the
  adapter maps these in `build_web_view()`). Renders the grouped table.
- `crates/web/ui/src/components/MainContent.tsx` — The `version_changes`
  section case currently renders `<ContextList>`. Replace with
  `<VersionChangesTable>` using `viewData.version_changes`.
- `crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx` — NEW.

**Data flow:** No Rust adapter change needed for version changes. The
`ViewResponse` already includes `version_changes: Vec<VersionChangeEntry>`
with `name`, `arch`, `host_version`, `base_version`, `host_epoch`,
`base_epoch`, `direction` fields. The frontend currently ignores this and
reads from the reference section's ContextItems instead. The new component
reads directly from `version_changes`.

### 2. Networking → Subsections by Type

**Problem:** NM connections, firewall zones, DNS config, and hostname are
mixed into one flat ContextItem list with no categorization.

**Design:** Group items into labeled subsections using the existing
`ContextSubsection` mechanism. No new frontend component needed —
`ContextList` already renders subsections with labels.

**Subsection groups:**
- **Connections** — NM connection profiles (name, type, method)
- **Firewall Zones** — firewalld zones (name, services, ports). These
  already have expandable `detail` (zone content) — preserved as-is.
- **DNS & Resolution** — resolv.conf entries
- **Identity** — hostname

**Empty subsections:** Omit any subsection with zero items. If the host
has no firewall zones, the "Firewall Zones" header doesn't appear.

**Files changed:**
- `crates/web/src/adapter.rs` — `web_network_section()` restructured to
  emit items into subsections instead of a flat `items` vec. Uses
  `ContextSubsection` for each group. Top-level `items` remains empty.
- No frontend changes. `ContextList` already handles this.

### 3. Kernel & Boot → Customizations vs Defaults/Context

**Problem:** cmdline, GRUB defaults, tuned profile, locale, timezone,
sysctl overrides, kernel modules, modprobe.d/modules-load.d/dracut
snippets, and alternatives are all dumped into one flat list.

**Design:** Split into two subsections based on migration relevance,
not domain taxonomy.

**Subsection groups:**
- **Customizations** — items the user deliberately changed. These are
  the things that matter for migration carry-forward:
  - Active tuned profile
  - Sysctl overrides (non-default values)
  - Non-default kernel modules
  - modules-load.d snippets
  - modprobe.d snippets
  - dracut.conf.d snippets
  - Custom tuned profiles
- **Defaults / Context** — reference information for awareness, not
  typically requiring action:
  - Kernel cmdline
  - GRUB defaults
  - Locale
  - Timezone
  - Alternatives

**Empty subsections:** Omit any subsection with zero items. A system
with no customizations shows only "Defaults / Context".

**Files changed:**
- `crates/web/src/adapter.rs` — `web_kernel_boot_section()` restructured
  to emit items into two `ContextSubsection`s instead of a flat vec.
  Top-level `items` remains empty.
- No frontend changes. `ContextList` already handles this.

## Out of Scope

- **Config file diff from default** — separate spec (pipeline collector
  changes required).
- **Package group dependency visibility** — separate spec (RPM dependency
  data not currently in the snapshot).
- **Package group summary wording** ("2 groups (4 packages) · 75
  individual packages") — include in the group deps spec since it's the
  same underlying clarity problem.
- **Sortable table columns** for version changes — nice-to-have for a
  follow-up, not launch-critical. The grouping (downgrades first) does
  the heavy lifting.

## Implementation Notes

- **Networking and Kernel & Boot are adapter-only changes.** The Rust
  `web_network_section()` and `web_kernel_boot_section()` functions
  restructure their output to use `ContextSubsection` instead of flat
  `items`. The frontend `ContextList` component already renders
  subsections with labels. No new React components needed.
- **Version Changes requires a new React component** since the table
  layout doesn't fit the ContextItem/ContextList pattern. The data is
  already available in `ViewResponse.version_changes` — the component
  reads it directly instead of going through the reference section.
- **The generic ContextItem component is not being removed.** Other
  sections (Storage, Scheduled Tasks, Non-RPM Software, SELinux) still
  use it. This spec replaces its use in three specific sections.
- **Existing search behavior:** The reference sections support
  `searchable_text` for global search. Networking and Kernel & Boot
  preserve this since they still use ContextItem within subsections.
  Version Changes needs search integration in the new table component
  (highlight matching rows).
- **TDD required** for all changes.

## Testing Strategy

- **Rust adapter tests:** Verify subsection structure for networking and
  kernel/boot. Verify subsection counts, item placement, and empty
  subsection omission.
- **Frontend tests (vitest):** VersionChangesTable component tests —
  renders downgrades before upgrades, shows counts, applies color
  styling, handles empty groups, handles fully empty (zero_drift /
  data_unavailable).
- **Contract snapshot tests:** Update existing contract snapshots to
  reflect subsection structure changes.
