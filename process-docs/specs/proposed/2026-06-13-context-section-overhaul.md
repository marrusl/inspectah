# Context Section Layout Overhaul

## Summary

Three reference sections — Version Changes, Networking, and Kernel & Boot —
currently render through the generic `ContextItem` flat list. This spec
replaces them with purpose-built layouts that match how sysadmins actually
scan migration data.

**Scope:** Adapter restructuring (Rust) + one new frontend component
(Version Changes table) + sidebar count fix for subsection-only sections.

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
- **Empty groups:** If only upgrades exist, omit the Downgrades header
  entirely (don't show "Downgrades (0)"). Same for the reverse.

**EVR formatting:** The component must replicate the adapter's current
pairwise epoch-display logic from `format_evr_pair()` in
`crates/web/src/adapter.rs`. The rule is pairwise, not per-side: show
the `epoch:` prefix on BOTH sides when EITHER side has a non-empty,
non-`"0"` epoch. This preserves the visual delta for epoch-change rows
(e.g., `1:2.4.57 → 0:2.4.51`). When neither side has a meaningful
epoch, display version alone on both sides. Do not apply per-side
epoch rendering — that would drop the base-side `0:` on epoch-delta
rows, misrepresenting the version relationship.

**Empty states:** The `version_changes` reference section currently has
two reason-specific empty states handled in `MainContent.tsx`:
- `data_unavailable` → "Baseline data is required..."
- `zero_drift` → "All packages match the target baseline versions."
- Default (no items, no reason) → standard EmptyState

The new component must preserve all three. Read `empty_reason` from the
reference section (still emitted by the adapter) and render the
appropriate message. Do not drop this contract.

**Search/navigation contract:** The current search and focus system must
be preserved:

- `GlobalSearch.tsx` indexes reference section items via
  `searchable_text`. The version changes reference section must continue
  to emit ContextItems so GlobalSearch can index them. The adapter
  continues emitting the `version_changes` reference section as before
  (flat ContextItems) — the frontend simply renders a different component
  for this section instead of `ContextList`. GlobalSearch reads the
  section data; the visual component reads `viewData.version_changes`.
- **DOM/focus contract:** Each data row in the table must carry
  `data-testid="context-item-{name}.{arch}"` — the exact selector the
  app uses for programmatic focus. The row element must be focusable
  (`tabIndex={-1}` or a natively focusable element). When
  `revealItemId` targets a version change item, focus must land on the
  **matching data row**, not a group header or table header. On plain
  section entry (no `revealItemId`), focus must land on the **first
  data row**, not a group header — group headers must not use
  `role="row"` or must be excluded from the `[role="row"]` query
  that `App.tsx` uses for initial focus. This matches the existing
  `ContextItem.tsx` contract where `data-testid="context-item-${item.id}"`
  is the focus target.
- **Section search is out of scope.** `MainContent.tsx` does not
  currently expose section-level search for context sections (only
  decision sections have it). This spec does not add it. GlobalSearch
  covers version change item discovery.

**Accessibility:**
- Group headers: `role="row"` with `aria-label="N downgrades"`.
- Use PatternFly composable `Table` / `Thead` / `Tbody`. Add
  `@patternfly/react-table` to `package.json` dependencies if not
  already present.
- Column headers in `Thead` for screen reader column identification.

**Files changed:**
- `crates/web/src/adapter.rs` — `web_version_changes_section()` continues
  to emit ContextItems into the reference section (preserves search
  indexing). No structural change to the adapter.
- `crates/web/ui/src/components/VersionChangesTable.tsx` — NEW. Receives
  `VersionChangeEntry[]` from `viewData.version_changes` plus
  `empty_reason` from the reference section. Renders the grouped table
  with EVR formatting, empty states, search highlight, and focus anchors.
- `crates/web/ui/src/components/MainContent.tsx` — The `version_changes`
  section case currently renders `<ContextList>`. Replace with
  `<VersionChangesTable>` while still passing the reference section for
  empty_reason and search compatibility.
- `crates/web/ui/src/components/__tests__/VersionChangesTable.test.tsx` —
  NEW.
- `crates/web/ui/package.json` — add `@patternfly/react-table` if not
  already a dependency.

### 2. Networking → Subsections by Type

**Problem:** NM connections, firewall zones, firewall direct rules, static
routes, IP routes, IP rules, DNS resolution, hosts additions, proxy
environment, and hostname are mixed into one flat ContextItem list with no
categorization.

**Design:** Group items into labeled subsections using the existing
`ContextSubsection` mechanism.

**Exhaustive field mapping against `RefNetwork`:**

| RefNetwork field | Subsection | Current adapter behavior |
|---|---|---|
| `connections: Vec<RefNMConnection>` | Connections | ContextItem per connection |
| `firewall_zones: Vec<RefFirewallZone>` | Firewall | ContextItem per zone (detail = zone content) |
| `firewall_direct_rules: Vec<RefFirewallDirectRule>` | Firewall | ContextItem per rule (detail = args) |
| `static_routes: Vec<RefStaticRoute>` | Routes & Rules | ContextItem per route file |
| `ip_routes: Vec<String>` | Routes & Rules | ContextItem per route |
| `ip_rules: Vec<String>` | Routes & Rules | ContextItem per rule |
| `resolv_provenance: String` | DNS & Hosts | ContextItem (single) |
| `hosts_additions: Vec<String>` | DNS & Hosts | ContextItem per line |
| `proxy_env: Vec<RefProxyEnv>` | Proxy | ContextItem per entry |

**Subsection groups (5):**
- **Connections** — NM connection profiles (`connections`)
- **Firewall** — firewalld zones + direct rules (`firewall_zones`,
  `firewall_direct_rules`). Zones already have expandable `detail`
  (zone content) — preserved as-is.
- **Routes & Rules** — static route files, ip routes, ip rules
  (`static_routes`, `ip_routes`, `ip_rules`)
- **DNS & Hosts** — resolver provenance + hosts additions
  (`resolv_provenance`, `hosts_additions`)
- **Proxy** — proxy environment entries (`proxy_env`)

**Note:** `hostname` is NOT in `RefNetwork` — it does not appear in the
current adapter output for this section. Do not add it. If hostname
surfaces later, it belongs in a future scope.

**Empty subsections:** Omit any subsection with zero items.

**Files changed:**
- `crates/web/src/adapter.rs` — `web_network_section()` restructured to
  emit items into subsections instead of a flat `items` vec. Uses
  `ContextSubsection` for each group. Top-level `items` remains empty
  (see sidebar fix below).

### 3. Kernel & Boot → Customizations vs Defaults/Context

**Problem:** cmdline, GRUB defaults, tuned profile, locale, timezone,
sysctl overrides, kernel modules, modprobe.d/modules-load.d/dracut
snippets, alternatives, and custom tuned profiles are all dumped into one
flat list.

**Design:** Split into two subsections based on migration relevance,
not domain taxonomy.

**Subsection groups:**
- **Customizations** — items the user deliberately changed:
  - Active tuned profile
  - Sysctl overrides (non-default values)
  - Non-default kernel modules
  - modules-load.d snippets
  - modprobe.d snippets
  - dracut.conf.d snippets
  - Custom tuned profiles
- **Defaults / Context** — reference information for awareness:
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
  Top-level `items` remains empty (see sidebar fix below).

## Cross-Cutting: Sidebar Count Fix

**Problem:** `Sidebar.tsx` counts reference section items via
`sec.items.length`. When networking and kernel/boot move all items into
subsections (top-level `items` empty), the sidebar will show `0` for
these sections.

**Fix:** Update the `sectionCount()` function in `Sidebar.tsx` to sum
items across subsections when top-level items is empty:

```typescript
function contextCount(sections, id) {
  const sec = sections.find(s => s.id === lookupId);
  if (!sec) return "0";
  const topLevel = sec.items.length;
  if (topLevel > 0) return String(topLevel);
  // Fall through to subsection sum
  const subTotal = (sec.subsections ?? [])
    .reduce((sum, sub) => sum + sub.items.length, 0);
  return String(subTotal);
}
```

This is backward-compatible — sections that still use top-level items
(Storage, Scheduled Tasks, etc.) are unaffected.

**Files changed:**
- `crates/web/ui/src/components/Sidebar.tsx` — update `sectionCount()`.

## Cross-Cutting: Subsection Accessibility

**Problem:** `ContextList.tsx` currently renders subsection labels as a
plain `<div>` with class `inspectah-context-subsection__label`. This
works for visual hierarchy but provides no semantic structure for screen
readers.

**Fix:** Upgrade subsection labels to use heading + region semantics:

```html
<section aria-labelledby="subsection-{id}">
  <h4 id="subsection-{id}" class="inspectah-context-subsection__label">
    {sub.display_name}
  </h4>
  <div role="list">...</div>
</section>
```

Use `<h4>` since the section heading is `<h2>` and the main content area
doesn't use `<h3>` for subsection-level content.

**Files changed:**
- `crates/web/ui/src/components/ContextList.tsx` — update subsection
  rendering.

## Out of Scope

- **Config file diff from default** — separate spec (pipeline collector
  changes required).
- **Package group dependency visibility** — separate spec (RPM dependency
  data not currently in the snapshot).
- **Package group summary wording** ("2 groups (4 packages) · 75
  individual packages") — include in the group deps spec.
- **Sortable table columns** for version changes — nice-to-have for a
  follow-up, not launch-critical.

## Implementation Notes

- **Networking and Kernel & Boot are adapter changes + sidebar fix.** The
  Rust adapters restructure output to use `ContextSubsection`. The
  frontend `ContextList` handles subsection rendering. The sidebar count
  fix is a small change to `sectionCount()` in `Sidebar.tsx`.
- **Version Changes requires a new React component.** The data is already
  in `ViewResponse.version_changes`. The adapter continues to emit the
  flat reference section for search indexing — the frontend renders the
  table component instead of `ContextList` for this section.
- **The generic ContextItem component is not being removed.** Other
  sections still use it.
- **TDD required** for all changes.

## Testing Strategy

### Rust adapter tests

- **Networking:** Verify every `RefNetwork` field maps to exactly one
  subsection. Test with all fields populated: assert 5 subsections with
  correct item counts. Test with only connections populated: assert 1
  subsection. Test with empty `RefNetwork`: assert empty section.
- **Kernel & Boot:** Verify customization items (tuned, sysctls, modules,
  snippets) land in "Customizations" subsection. Verify context items
  (cmdline, GRUB, locale, timezone, alternatives) land in
  "Defaults / Context". Test with no customizations: assert only
  "Defaults / Context" subsection emitted.
- **Contract snapshot tests:** Update existing snapshots to reflect
  subsection structure.

### Frontend tests (vitest)

- **VersionChangesTable:**
  - Renders downgrades before upgrades with correct group headers
  - Shows counts in group headers
  - Applies danger/success styling to group headers
  - Handles pairwise EVR formatting: both sides show epoch when either
    has a non-zero epoch; both sides omit epoch when neither does
  - Handles empty groups (upgrades only, downgrades only)
  - Handles fully empty with `data_unavailable` reason
  - Handles fully empty with `zero_drift` reason
  - Handles fully empty with no reason (default EmptyState)
  - Search highlight: row with matching name gets highlight class
  - Focus anchor: `data-testid="context-item-{name}.{arch}"` on each
    data row, element is focusable, focus lands on row not header
- **Sidebar:** Verify `sectionCount()` sums subsection items when
  top-level items is empty. Verify existing sections unaffected.
- **ContextList accessibility:** Verify subsection labels render as
  `<h4>` inside `<section>` with `aria-labelledby`.

### End-to-end contract

- Verify `GlobalSearch` still indexes version change items (reads
  reference section ContextItems, not the table component).
- Verify focus restore navigates to a version change row in the table.
