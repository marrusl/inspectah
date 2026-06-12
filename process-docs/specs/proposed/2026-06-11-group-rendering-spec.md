# Group-Aware Rendering & Refine UI

## Revision History

| Rev | Date       | Summary |
|-----|------------|---------|
| R1  | 2026-06-11 | Initial spec from brainstorm session |
| R2  | 2026-06-11 | Contract-hardening: renderability rule, timeline type, InstalledGroup parsing, UI surface contract |
| R3  | 2026-06-11 | Close R2 blockers: effective replay surface, excluded/degraded state split, persistent optional metadata, badge trim |

## Problem

When Anaconda installs RHEL, it installs packages via DNF groups
(e.g., "Container Management", "Development Tools"). The current
Containerfile renderer emits individual `dnf install` lines for every
package, losing the group-install semantic. This makes the generated
Containerfile longer, harder to read, and less faithful to how the
system was originally provisioned.

### Impact

- **Containerfile readability:** 47 individual `dnf install` lines vs.
  2 `dnf group install` lines for the same packages
- **Migration fidelity:** `dnf group install` pulls in mandatory +
  default + conditional members, matching what Anaconda originally
  installed. Individual lines are a lossy representation.
- **User comprehension in refine:** Users who installed groups see
  individual packages and lose the "why" — they installed a group,
  not 47 unrelated packages

## Design Principle

Group membership is **classification-neutral, projection-owned, and
renderer-consumed.** The Anaconda gap classifier classifies packages
by source_repo, service state, and config data — group membership
does not affect which tier a package lands in. The projection layer
computes group renderability as a derived property. The Containerfile
renderer and refine UI consume that derived property without
re-deriving it.

## Dependencies

- **Anaconda gap classifier spec** must ship first — provides the
  `InstalledGroup` data on the RPM snapshot section and the group
  collection pipeline step
- This spec amends the `InstalledGroup` struct defined in the
  classifier spec (adds `optional_installed` field, renames
  `packages` to `members`)

## Data Model

### InstalledGroup (amended)

```rust
/// Serde alias preserves backward compatibility with existing
/// snapshots that use `packages` as the field name.
pub struct InstalledGroup {
    pub name: String,
    /// Mandatory + default + conditional members (the reproducible
    /// set). Name-only (no arch qualifier). These are the packages
    /// that `dnf group install` will install.
    #[serde(alias = "packages")]
    pub members: Vec<String>,
    /// Optional members that ARE installed on the source system.
    /// NOT reproduced by bare `dnf group install`. Render as
    /// individual packages with provenance annotation.
    /// NOT toggle-bound to the group.
    #[serde(default)]
    pub optional_installed: Vec<String>,
}
```

**Changes from the classifier spec:**
- `packages` renamed to `members` with `#[serde(alias = "packages")]`
  for wire compatibility — existing snapshots with the old field name
  deserialize cleanly without migration
- `optional_installed` added with `#[serde(default)]` — old snapshots
  without this field deserialize with an empty vec (no optional
  spillover, which is correct for pre-amendment data)

**Collector/parser requirements:**

The RPM inspector's `dnf group info` parser must split output by
section header:

| `dnf group info` section | Target field |
|--------------------------|--------------|
| `Mandatory Packages` | `members` |
| `Default Packages` | `members` |
| `Conditional Packages` | `members` (if the condition is met and package is installed) |
| `Optional Packages` | `optional_installed` (only if the package is installed on the host) |

Conditional packages whose condition is NOT met on the source host
are omitted entirely — they are not members and not optional
spillover. The collector cross-references `Optional Packages` against
`packages_added` to determine which optional members are actually
installed.

### Unified Timeline

The session history is a single interleaved timeline of data-plane
ops and view-plane directives:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum TimelineEntry {
    Op(RefinementOp),
    View(ViewDirective),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "directive")]
enum ViewDirective {
    UngroupGroup { group_name: String },
}
```

The session stores `Vec<TimelineEntry>` with one cursor. This
replaces the current `Vec<RefinementOp>`.

**Backward compatibility:** Old autosaved sessions store
`ops: Vec<RefinementOp>`. The session loader detects schema_version
and migrates: wrap each `RefinementOp` in `TimelineEntry::Op(...)`.
Schema version bumps from 2 to 3.

**Extended ItemId:**

```rust
enum ItemId {
    // existing...
    Package { name: String, arch: String },
    Service { unit: String },
    // new:
    Group { name: String },
}
```

`ItemId::Group.name` must match an `InstalledGroup.name` exactly
(case-sensitive). `validate_target()` rejects unknown group names.

### Why ViewDirective is separate from RefinementOp

`RefinementOp` variants mutate projected snapshot state (flip
`include` flags, change variant assignments). `UngroupGroup` does
neither — it changes how packages are rendered but does not modify
any `PackageEntry`. The separation makes the data-plane / view-plane
boundary explicit at the type level. `project_snapshot()` only
processes `TimelineEntry::Op` entries; view directives are collected
separately into derived rendering state.

### Render Context

View-derived rendering state lives in a dedicated `RenderContext`,
never on `InspectionSnapshot`:

```rust
pub struct RenderContext {
    /// Per-group render state, keyed by group name. Only groups
    /// present in `InstalledGroup` data appear here.
    pub group_states: HashMap<String, GroupRenderState>,
}

/// Explicit state machine for group rendering. `Excluded` is a
/// user action (SetInclude false). `Degraded` is a system-detected
/// conflict. `Ungrouped` is a user action (ViewDirective).
/// These are distinct states with different UI treatments.
enum GroupRenderState {
    /// All conditions met. Renders as `dnf group install`.
    Renderable,
    /// User explicitly excluded via SetInclude(Group, false).
    /// All reproducible members are include: false.
    /// UI: group row visible but toggled off, ungroup available.
    Excluded,
    /// User explicitly ungrouped via ViewDirective. Renders as
    /// individual packages with provenance.
    /// UI: group row removed, members in individual zone.
    Ungrouped,
    /// Failed renderability check. Renders as individual packages
    /// with degradation indicator.
    /// UI: group row visible but dimmed, controls disabled.
    Degraded { reason: DegradationReason },
}

enum DegradationReason {
    MemberExcluded,
    MemberOverridden,
    MultilibConflict,
}
```

The Containerfile renderer, refine view builder, and export path all
consume `(InspectionSnapshot, RenderContext)`. The snapshot stays
pure host truth; the render context carries session-derived view
state. `RenderContext` is NOT serialized into
`inspection-snapshot.json`.

## Session Behavior

### Group-Level Include/Exclude

`SetInclude { item_id: ItemId::Group { name }, include }` toggles
all reproducible members of a group.

**Fan-out happens in `project_snapshot()`, not in `apply()`.**
The session records one atomic group-level op. During projection
replay, the op expands to set `include` on each member package
(matching all arches in `packages_added` for each member name).

- One undo = one group toggle (not N individual package undos)
- `InstalledGroup` data must be available during replay (it is —
  it's on the snapshot)
- The ops list stays compact

**Locked members:** `SetInclude(Group, true)` includes the group
but locked members stay locked (silent no-op per existing session
behavior). The UI surfaces a toast: "N packages locked (platform)
— cannot be included." (Locked platform_plumbing members are
excluded by default; the lock prevents re-inclusion.)

**Optional-installed members are NOT affected** by group-level
`SetInclude`. They are independent individual packages. When a
parent group is excluded, optional spillover packages stay at their
current toggle state. The UI surfaces: "N optional packages from
this group remain included individually" with a scroll-to link.

### Ungroup

`ViewDirective::UngroupGroup { group_name }` converts a group into
individual package rows with their own toggles.

**Effects:**
- Group row disappears from the UI
- Member packages appear as individual rows in the packages zone
- Containerfile switches from `dnf group install` to individual
  `dnf install` lines with comment: `# Ungrouped from "Group Name"`
- Optional spillover packages merge back into the flat list
  (they're now individual rows like everything else)

**API contract:**

| Endpoint | UngroupGroup behavior |
|----------|-----------------------|
| `/api/op` | Accepts `TimelineEntry::View(UngroupGroup { ... })`. Returns updated view with group dissolved. |
| `/api/ops` | History includes `View` entries alongside `Op` entries. |
| `/api/view` | Ungrouped group's members appear as individual package rows. |
| `/api/changes` | `is_dirty = true` after `UngroupGroup` — it changes Containerfile/export output shape. |
| Undo/redo | `UngroupGroup` participates in the unified cursor. Undo re-groups. |
| Autosave | `TimelineEntry::View` entries persisted in the v3 schema timeline. |
| Export | Ungrouped state reflected in exported Containerfile. NOT in `inspection-snapshot.json`. |

**Idempotency:** `UngroupGroup` on an already-ungrouped group is a
no-op — not recorded, cursor unchanged. `is_directive_noop()` checks
the current `group_states` map (checks for `Ungrouped` entry).

**Undo:** Linear undo for v1. Undoing `UngroupGroup` re-groups the
packages. Subsequent per-package toggles made after ungrouping are
also undone (they're later in the timeline). Non-linear undo
(preserving per-package toggles through regroup) is deferred to v2
if users report pain.

### Initial State

All detected groups start grouped. No op/directive = grouped.
`UngroupGroup` is the only way to un-group (besides auto-degradation
from renderability failure — see below).

All groups form regardless of member state — no minimum member
count threshold. A group with 2 members is still a group.

### Overlapping Groups

A package can belong to multiple installed groups. Both groups
appear in the UI. DNF handles idempotent installs.

**Last-writer-wins for ops:** Later ops in the timeline take
precedence during projection replay. A
`SetInclude(Package("podman"), true)` after
`SetInclude(Group("Container Management"), false)` results in
podman being included.

**Overlap annotations:** In the expanded group view, shared members
show "also in @Headless Management." In the audit report, flag
overlapping memberships: "vim-enhanced appears in both 'Development
Tools' and 'Minimal Install' — DNF handles this correctly, no
action needed."

## Group Renderability Contract

A group may render as `dnf group install "Name"` **only when the
following conditions are all true** after projection:

1. **Not ungrouped:** Group's state is not `Ungrouped` in
   `RenderContext.group_states`.
2. **All reproducible members included on the effective surface:**
   Every package in `InstalledGroup.members` that survives into the
   effective projected render surface — the same post-classification,
   post-baseline-suppression package set that refine, preview, and
   export treat as truth — has `include: true` in the projected
   state. Raw `packages_added` membership is NOT the truth source;
   the projection's filtered, classified output is. A member that
   is present in the snapshot but filtered out by classification
   (e.g., baseline-suppressed as platform_plumbing) does not count
   toward or against renderability — it is invisible to the group
   contract.
3. **No member-level divergence:** No individual `SetInclude(Package)`
   op in the timeline has overridden any reproducible member's
   effective include state to differ from the group-level state.
4. **No multi-arch conflict:** No reproducible member requires
   arch-qualified replay. If the same package name appears on the
   effective surface with multiple arches (e.g., `glibc.x86_64` and
   `glibc.i686`) and both survive classification, bare
   `dnf group install` cannot faithfully reproduce the arch mix —
   it replays current comps/basearch behavior, not the scanned
   host's package set.

**Locked members and degradation:** Locked platform_plumbing
members are an expected baseline state and do NOT trigger
degradation. A group with 12 reproducible members where 2 are
locked (excluded, cannot be re-included) is still `Renderable`
as long as the remaining 10 are all included. The locked members
are filtered out of the effective render surface by classification
and do not participate in the renderability check. Degradation
triggers only when a non-locked reproducible member's include
state diverges from the group-level expectation.

**When any condition fails:** The group is **auto-degraded**. Its
state is set to `Degraded { reason }` in `RenderContext`. The renderer emits
individual `dnf install` lines with a comment:
`# "Group Name" degraded — members rendered individually`

The UI shows the group row with a degraded indicator (dimmed group
name, "rendered as individual packages" subtitle). The group toggle
and ungroup button are disabled on a degraded group — the user sees
individual package rows in the individual packages zone with full
per-package control.

**Auto-degradation is re-evaluated on every projection.** If the
user undoes the op that caused divergence, the group may become
renderable again and automatically upgrades back to grouped state.

The `GroupRenderState` enum (defined in the Render Context section
above) is the canonical type for this state machine.

## View Projection

During `project_snapshot()`, the projection layer:

1. Replays `TimelineEntry::Op` entries with member-level fan-out
   for `SetInclude(Group(...))` (matching all arches per member name)
2. Collects `TimelineEntry::View` entries (respecting undo cursor)
3. For each group, derives `GroupRenderState`:
   - If `UngroupGroup` directive present → `Ungrouped`
   - If `SetInclude(Group, false)` and all members excluded → `Excluded`
   - If renderability conditions fail → `Degraded { reason }`
   - Otherwise → `Renderable`
4. Packages `RenderContext { group_states }` alongside the
   projected snapshot

**Caching:** `RenderContext` is materialized once during projection
and cached alongside `cached_view`. Recomputed on any mutation.

**is_op_noop for groups:**
- `SetInclude(Group, false)`: check that ALL reproducible member
  packages are already excluded. Short-circuit on first mismatch.
- `SetInclude(Group, true)`: check that ALL non-locked reproducible
  members are already included. Short-circuit on first mismatch.

**Counting rules:**
- Summary bar counts are **unique packages**, not visible rows.
  A package in two groups counts once.
- "N groups (M packages)" counts unique packages across all
  renderable groups.
- Search result counts follow the same unique-package rule.

**Placement precedence for overlapping packages:**
- A package that is a reproducible member in one group and
  optional-installed in another: the reproducible membership wins.
  The package stays inside the renderable group; it does NOT also
  appear as optional spillover.
- A package in a renderable group and an ungrouped group: the
  package stays inside the renderable group. It does NOT appear as
  an individual row from the ungrouped group.
- A package in both a degraded and a renderable group: the
  renderable group wins. The package stays inside the renderable
  group; it does NOT appear as an individual from the degraded group.
- A package in two renderable groups: appears in both groups'
  expanded member lists. The Containerfile emits both group names;
  DNF handles idempotency.

## Containerfile Rendering

### Section Order

Within the packages area of the Containerfile:

1. **Repo-enabling packages** — own RUN (existing behavior, unchanged)
2. **Package groups** — new RUN (renderable groups only)
3. **Individual packages** — own RUN (includes optional spillover,
   ungrouped members, and degraded group members)

### Output Format

**Groups (renderable):**

```dockerfile
# === Package Groups (2) ===
RUN dnf group install -y \
    "Container Management" \
    "Development Tools" \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
```

Group names are alphabetically sorted on continuation lines.

**Individual packages with annotations:**

```dockerfile
# === Packages (14) ===
# Optional members (not installed by dnf group install):
#   python3-pytest — from "Development Tools"
#   python3-tox — from "Development Tools"
# Ungrouped from "System Administration":
#   cockpit, cockpit-ws
# "Security Tools" degraded — members rendered individually:
#   aide, nmap
RUN dnf install -y \
    aide \
    cockpit \
    cockpit-ws \
    htop \
    nmap \
    python3-pytest \
    python3-tox \
    tmux \
    && dnf clean all \
    && rm -rf \
        /var/cache/dnf \
        /var/lib/dnf/history* \
        /var/log/dnf* \
        /var/log/hawkey.log \
        /var/log/rhsm
```

Comment annotations appear ABOVE the RUN statement, not inline.
Packages within the RUN are alphabetically sorted regardless of
provenance.

### Rendering Rules

- Only renderable groups (passed all four conditions) emit
  `dnf group install` lines
- Degraded and ungrouped groups' members fold into the individual
  packages section with distinct comment headers
- Optional spillover packages listed with their source group name
- Excluded groups emit nothing (no comment, no packages)
- The renderer consumes `(InspectionSnapshot, RenderContext)` —
  it does not access session state directly

### Renderer Partitioning

The group-aware renderer partitions packages into four buckets:

1. **Grouped:** packages whose group is `Renderable` → emit via
   `dnf group install`
2. **Ungrouped members:** packages from `Ungrouped` groups → emit
   as individual `dnf install` with `# Ungrouped from` comment
3. **Degraded members:** packages from `Degraded` groups → emit
   as individual `dnf install` with `# degraded` comment
4. **Non-group packages** (+ optional spillover): everything else
   → emit as individual `dnf install`

## Refine UI (Web)

### Page Layout (v1)

The packages section follows this fixed layout:

```
┌─────────────────────────────────────┐
│ Summary Bar                         │
│ "2 groups (24 pkgs) · 14 individual │
│  · 2 optional from groups"          │
├─────────────────────────────────────┤
│ GROUPS ZONE                         │
│  ▶ Container Management  12 pkgs [⊙]│
│  ▼ Development Tools    12 pkgs [⊙] │
│    ├ autoconf                       │
│    ├ automake                       │
│    ├ binutils               🔒     │
│    └ + 9 more                       │
├─────────────────────────────────────┤
│ INDIVIDUAL PACKAGES divider         │
│  htop               3.3.0   [⊙]    │
│  python3-pytest      7.4.0   [⊙]   │
│    ↳ optional from "Dev Tools"      │
│  tmux               3.4     [⊙]    │
├─────────────────────────────────────┤
│ EXCLUDED ZONE (existing)            │
│  (packages with include: false)     │
└─────────────────────────────────────┘
```

- Groups zone is always above the individual packages zone
- Repo state (RepoBar) is preserved as existing row metadata, not
  replaced — groups are an overlay on the current package surface,
  not a replacement
- Degraded groups appear in the groups zone with a dimmed indicator
  and disabled controls; their members appear in the individual zone

### Group Row (Collapsed)

- Left border accent (purple) to visually distinguish from
  individual packages
- Chevron (▶) for expand/collapse (read-only member view)
- Group name (semibold)
- Package count: "12 packages"
- Locked count (when present): "2 locked" in amber
- Optional leftover count (when present and group is excluded):
  "2 optional still included" in muted purple — persistent row
  metadata, not just a transient toast. Clickable to scroll to
  the spillover packages in the individual zone.
- Degraded indicator (when present): "rendered individually" in
  muted text, toggle and ungroup disabled
- **Ungroup button:** Ghost-style, low prominence. Label:
  "ungroup". Tooltip: "Show as individual packages with separate
  include/exclude controls"
- Include/exclude toggle (single, atomic)
- Groups sorted alphabetically within the groups zone

### Group Row (Expanded)

Clicking the chevron expands a read-only member list:

- Members listed alphabetically with name only
- No member-role badges (mandatory / default) in v1.
  `InstalledGroup.members: Vec<String>` does not carry role
  metadata, and the distinction is not user-relevant for migration
  triage. All non-locked members display identically. If role
  badges are desired in a future version, `members` must be
  enriched to `Vec<GroupMember>` with a role field — that is
  explicitly out of scope for this spec.
- Locked members: lock icon + amber text + dimmed name
- Overlap annotation on shared members: "also in @Headless
  Management"
- Truncation: first 5 members + "+ N more" link. Clicking the
  link reveals the full list (no re-truncation until collapse).
  Search disables truncation entirely — all members visible.
- No per-member toggles — the group is the unit of decision

### Individual Packages Zone

Below the groups zone, separated by an "Individual Packages"
divider:

- Regular individual packages with their own toggles
- Optional spillover packages with provenance badge:
  `optional from "Development Tools"` — states host fact (this
  package IS an optional member of this group) plus replay reason
  (not installed by bare `dnf group install`). NOT inferred user
  intent.
- Optional spillover packages have independent toggle state (not
  bound to parent group)
- Ungrouped and degraded group members appear here with their
  respective provenance annotations

### Search

**Owner:** Global search (`Ctrl+K`). Section-level search (`/`)
is not extended for v1 — groups participate in the existing global
search surface only.

**Match behavior:**

| Query matches... | UI behavior |
|------------------|-------------|
| Group name | Group row highlighted in groups zone |
| Package inside a collapsed group | Group auto-expands, truncation disabled, matching member highlighted |
| Package inside an expanded group | Matching member highlighted |
| Optional spillover package | Individual row highlighted in individual zone |
| Package in ungrouped/degraded group | Individual row highlighted in individual zone |

**Focus target:** Search lands focus on the first matching row
(group row if the match is a group name, member row if the match
is a package inside a group). Keyboard navigation (`↑`/`↓`)
moves between matches.

**Re-collapse:** Groups that were auto-expanded by search
re-collapse when the query is cleared. Groups the user manually
expanded before search stay expanded.

**Summary bar during search:** Updates to filtered count:
"1 group (match inside) · 3 individual packages"

### Keyboard & Accessibility

**Tab order within a group row:**
1. Chevron (expand/collapse) — `Enter` or `Space` to toggle
2. Ungroup button — `Enter` to activate
3. Include/exclude toggle — `Space` to toggle

`Enter` on the group row itself (when row has focus, not a child
control) expands/collapses — same as chevron.

**Focus restoration:**
- After ungroup: focus moves to the first individual package row
  that was formerly the first member of the group
- After undo of ungroup: focus moves to the restored group row
- After scroll-to optional spillover: focus lands on the first
  spillover package from that group

**Live regions (ARIA):**

| Event | `aria-live` | Announced text |
|-------|-------------|----------------|
| Ungroup | `polite` | "Group ungrouped into N packages. Press Ctrl+Z to undo." |
| Group exclude with locked members | `polite` | "N packages locked as platform. Cannot be included." |
| Group exclude with optional orphans | `polite` | "N optional packages from this group remain included." |
| Auto-degradation | `polite` | "Group rendered as individual packages due to member conflict." |

Toasts are visual + `aria-live`. Visible for 5 seconds or until
next user action.

**Persistent indicators:** Locked-member count and degradation
state are persistent row metadata (always visible on the group
row), not transient toast-only. The toast fires once on the
triggering action; the row indicator persists.

### Containerfile Preview

- Updates live as group state changes
- Degraded and ungrouped groups show in the individual packages
  section with distinct comment headers
- Mixed state renders correctly

## TUI

Same data contract as web. Group rows with expand/collapse,
ungroup keybinding, atomic toggle. TUI-specific interaction design
deferred to the TUI refine spec (already in progress with Tang) —
this spec defines the data contract the TUI implements against.

## Testing

### Collector Tests

- Parser splits `dnf group info` output correctly: mandatory +
  default + conditional → `members`, installed optional →
  `optional_installed`
- Conditional packages whose condition is not met are omitted
- Optional packages not installed on the host are omitted
- Snapshot round-trip: old format `{ packages: [...] }` loads via
  serde alias, new format `{ members: [...] }` loads directly
- Snapshot round-trip: missing `optional_installed` defaults to `[]`

### Unit Tests (refine crate)

- `SetInclude(Group, false)` excludes all non-locked reproducible
  members
- `SetInclude(Group, true)` includes all non-locked reproducible
  members; locked members stay locked
- `SetInclude(Group)` is a no-op when all members already match
- `UngroupGroup` causes members to render as individual rows
- `UngroupGroup` on already-ungrouped group is a no-op
- Undo of `UngroupGroup` re-groups packages
- Undo of `SetInclude(Group)` restores previous member states
- Fan-out handles multi-arch members (e.g., glibc.x86_64 +
  glibc.i686)
- Optional-installed members are NOT affected by group-level ops
- Overlapping groups: individual op after group op takes precedence
- `is_op_noop` correctly checks all members for group ops
- `is_directive_noop` correctly detects already-ungrouped groups
- Autosave round-trip with `TimelineEntry` (v3 schema)
- Autosave migration from v2 schema (wrap ops in TimelineEntry::Op)
- Dirty state: `UngroupGroup` sets `is_dirty = true`

### Renderability Tests

- Group with all members included → `Renderable`
- Group with one member excluded by individual op → `Degraded`
- Group with overlapping member excluded by other group op →
  `Degraded`
- Group with multilib member (two arches survive) → `Degraded`
- Group with locked members but all non-locked members included →
  `Renderable` (locked members do NOT trigger degradation)
- Group with all members included after undo of divergent op →
  `Renderable` (auto-upgrades back)
- Degraded group emits individual `dnf install` with comment
- Renderable group emits `dnf group install`

### Overlap Precedence Tests

- Package in both a degraded and a renderable group → stays in
  the renderable group, not emitted as individual from degraded
- Package in both an excluded and a renderable group → stays in
  the renderable group

### Overlap Tests

- Shared package excluded by group A, group B still renderable →
  group B degraded (member not included)
- Shared package re-included individually after group exclude →
  both groups re-evaluated for renderability
- Package is optional in group A, reproducible in group B →
  appears inside group B, not as spillover
- Package in ungrouped group A, renderable group B → stays in
  group B, does not appear as individual from A
- Summary counts use unique packages, not visible rows
- Containerfile does not duplicate packages across group and
  individual sections

### Integration Tests (web)

- `/api/op` accepts `TimelineEntry::View(UngroupGroup)`
- `/api/ops` returns interleaved timeline
- `/api/view` includes group rows with render state and member data
- `/api/changes` reports `is_dirty = true` after `UngroupGroup`
- Undo/redo cycle preserves group state and render context
- Containerfile preview reflects current group + render state
- Export omits render context from `inspection-snapshot.json`

### Playwright Tests (e2e)

- Group row expand/collapse with chevron
- Group toggle include/exclude
- Ungroup button → packages appear as individual rows
- Search highlights group with matching member, auto-expands
- Search clears → auto-expanded groups re-collapse
- Toast appears on ungroup with correct package count
- Optional spillover packages have provenance badge
- Locked member indicator visible in expanded view
- Degraded group shows indicator, controls disabled
- Keyboard navigation through group controls (chevron → ungroup
  → toggle)
- Focus restoration after ungroup and undo

## Scope

### In Scope

- `InstalledGroup` struct amendment (members + optional_installed)
  with serde alias for wire compatibility
- `TimelineEntry` unified timeline type (schema v3)
- `ViewDirective::UngroupGroup` session extension
- `RenderContext` separate from `InspectionSnapshot`
- Group renderability contract with auto-degradation
- `ItemId::Group` and group-level `SetInclude`
- Containerfile group rendering (dnf group install)
- Web UI group rows (collapsed, expanded, ungroup, degraded)
- Search, toast, locked feedback, optional orphan feedback
- Keyboard and accessibility contract
- Overlap annotations (UI + audit report)
- Autosave v2→v3 migration

### Out of Scope

- Fleet merge with groups (follow-up spec — flag for Collins
  review before implementation)
- Non-linear undo for ungroup (deferred to v2 if users report pain)
- TUI-specific interaction design (deferred to TUI refine spec)
- Group-aware fleet consensus (how groups behave in fleet context)
- Comps metadata caching or refresh
- Environment group display (top-level Anaconda environment like
  "Server with GUI" — only constituent groups are rendered)
- Section-level search (`/`) for groups (v1 uses global search only)

## Known Limitations

- **No intent detection for optional members.** Cannot distinguish
  whether optional packages were installed via `--with-optional`
  or individually. Both render as individual packages with
  provenance stating host fact + replay reason, not inferred intent.
- **Group membership is snapshot-time.** If comps metadata changes
  between scan and Containerfile execution, `dnf group install`
  may install a different set of packages than the source system
  had. This is inherent to `dnf group install` and not specific
  to inspectah.
- **Baseline suppression eats most overlap.** Many packages that
  appear in multiple groups (sudo, openssh-server, chrony) are
  Tier 1 / platform_plumbing and filtered before group rendering.
  The overlap annotation logic only fires for packages that
  survive classification.
- **Auto-degradation is conservative.** Any single divergent member
  degrades the entire group to individual rendering. This preserves
  Containerfile truthfulness at the cost of losing group rendering
  in edge cases that might have been safe. The tradeoff favors
  correctness over presentation.
