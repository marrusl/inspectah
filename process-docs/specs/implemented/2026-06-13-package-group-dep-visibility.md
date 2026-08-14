# Package Group Dependency Visibility

## Summary

Package groups currently hide members that are already in the base image,
making groups like "Minimal" show "0 packages" with nothing to expand.
The summary line ("2 groups (4 packages) · 75 individual packages") uses
ambiguous wording. This spec fixes both: show all group members with
base-image ones labeled and de-emphasized, clarify the summary language,
and fix the member list truncation.

## Problem

1. **Hidden base-image members:** A group whose members are all in the
   base image shows "0 packages" with an empty expansion. The user
   cannot see what the group contains or verify that those packages are
   already covered by the target image.

2. **Unclear summary line:** "2 groups (4 packages) · 75 individual
   packages" — "4 packages" is ambiguous (added? total?), and
   "individual" is implementation jargon.

3. **Member list truncation:** `GroupRow.tsx` has a hard cap
   `MAX_VISIBLE_MEMBERS = 5` with no way to see remaining members.
   A group like "Minimal" with 12 base-image members shows 5 and
   silently hides 7.

## Design

### 0. Prerequisite: tighten `InstalledGroup.members` collector contract

**Problem:** `InstalledGroup.members` is currently populated by
`parse_group_info_packages()` in `crates/collect/src/inspectors/rpm/mod.rs`,
which parses `dnf group info` output and collects Mandatory, Default,
and Optional packages into the same `members` list. This includes
packages from the group metadata that may not be installed on the host.
The `in_base_image` derivation below requires that `members` contains
only actually-installed packages.

**Fix:** In the collector, after parsing `dnf group info`, filter
`InstalledGroup.members` against the host's installed RPM list (already
available at collection time from `rpm -qa`). Remove any member name
that does not appear in the installed RPM set.

`optional_installed` is a separate field and is already scoped to
installed optional packages — no change needed there.

**Files changed:**
- `crates/collect/src/inspectors/rpm/mod.rs` — filter `members` against
  installed RPM names in `collect_installed_groups()` or the call site
- Update existing collector tests that assert uninstalled optional
  packages in `members`

**Schema impact:** None. The `InstalledGroup` serialization format is
unchanged — `members` is still `Vec<String>`. The semantic tightening
is backward-compatible (fewer entries, not different shape). Existing
tarballs with unfiltered members will show those packages as "from base"
which is slightly inaccurate but harmless — future scans will be
correct.

### 1. Show all group members, annotate base-image ones

With the collector contract tightened (prerequisite above), every entry
in `InstalledGroup.members` is an installed package. Show ALL members
when a group is expanded, not just those in `packages_added`.

- **New members** (in `packages_added`): render as today — normal
  weight, interactive.
- **Base-image members** (installed on host but NOT in
  `packages_added`): render de-emphasized with a trailing label
  "(from base)". These are read-only context, not toggleable.

**`in_base_image` derivation:** With the tightened collector, every
member in `InstalledGroup.members` is confirmed installed on the host.
A member absent from `packages_added` is therefore definitionally in
the base image — `packages_added` is computed as
`host_packages - base_packages`. This is a closed-world determination
contingent on the prerequisite collector fix.

**De-emphasis treatment:**
- Reduced opacity (0.5) with italic text
- "(from base)" label after the package name
- Minimum contrast ratio must meet WCAG 2.2 AA (4.5:1 for normal text).
  If 0.5 opacity on the current text color falls below this, use a
  named muted color token instead of raw opacity
- Base-image members are not focusable via keyboard navigation (they
  are not decision targets). Screen readers should still announce them
  — use `aria-label="{name} (from base image, no action needed)"`
  rather than `aria-hidden`

**Sort order within expanded list:** New members first (sorted
alphabetically), then base-image members (sorted alphabetically).

The group header shows the full member breakdown:
- All members from base: "12 packages (all from base)"
- Mixed: "4 new, 8 from base"
- All new (no base-image members): "4 packages" (unchanged)

### 2. Fix member list truncation

Replace the hard `MAX_VISIBLE_MEMBERS = 5` cap with a progressive
disclosure pattern:

- Show up to 5 members by default (preserving compact initial state)
- When more exist, render a clickable "Show all N members" link where
  the static "{N} more" text currently appears
- Clicking expands to the full list
- Clicking again collapses back to 5
- The 5-item cap applies to the combined list (new + base-image),
  with new members taking priority in the initial view

### 3. Clarify summary line

Change the packages summary wording:

**When groups exist:**
```
2 groups (4 new, 12 from base) · 75 other packages
```

- Parenthetical shows new vs base-image member counts summed across
  all visible groups (unique packages, not membership slots — if a
  package belongs to two groups, count it once)
- "individual packages" → "other packages" — neutral label for packages
  not in any DNF group
- Variants:
  - All group members from base: "2 groups (all from base) · 75 other
    packages"
  - All group members new: "2 groups (4 packages) · 75 other packages"
    (no "from base" qualifier needed)

**When no groups detected:**
```
75 packages
```
No "other" label — there's no contrast to draw. Just the count.

### 4. Data model changes

**Rust `GroupMemberInfo`** (in `crates/web/src/web_types.rs`):

Add `in_base_image: bool` field:
```rust
pub struct GroupMemberInfo {
    pub name: String,
    pub locked: bool,
    pub overlap_groups: Vec<String>,
    pub in_base_image: bool,
}
```

**Rust adapter** (in `crates/web/src/adapter.rs`):

When building `GroupMemberInfo` entries, check whether each member name
appears in `packages_added`. If it does NOT and IS installed on the
host, set `in_base_image: true`. The adapter already iterates
`InstalledGroup.members` and has access to `packages_added` via the
snapshot.

**Rust `GroupInfo`** (in `crates/web/src/web_types.rs`):

Add `added_count: usize`:
```rust
pub struct GroupInfo {
    pub name: String,
    pub member_count: usize,      // total members (new + base-image)
    pub added_count: usize,       // members NOT in base image
    pub locked_count: usize,
    pub optional_spillover_count: usize,
    pub render_state: String,
    pub degradation_reason: Option<String>,
    pub members: Vec<GroupMemberInfo>,
}
```

`member_count` is the total count of all members (new + base-image).
`added_count` is members present in `packages_added` only.

**TypeScript types** (in `crates/web/ui/src/api/types.ts`):

```typescript
export interface GroupMemberInfo {
  name: string;
  locked: boolean;
  overlap_groups: string[];
  in_base_image: boolean;
}

export interface GroupInfo {
  name: string;
  member_count: number;       // total (new + base-image)
  added_count: number;        // new members only
  locked_count: number;
  optional_spillover_count: number;
  render_state: "renderable" | "excluded" | "ungrouped" | "degraded";
  degradation_reason: string | null;
  members: GroupMemberInfo[];
}
```

### 5. Frontend changes

**`GroupRow.tsx`:**

- Header label uses `added_count` and `member_count`:
  - `added_count === 0`: "12 packages (all from base)"
  - `added_count === member_count`: "4 packages" (unchanged)
  - Otherwise: "4 new, 8 from base"
- Expanded member list includes base-image members:
  - New members: render as today, sorted alphabetically
  - Base-image members: reduced opacity, italic, "(from base)" label,
    not toggleable, sorted alphabetically after new members
  - Screen reader: `aria-label` on base-image rows (see accessibility
    treatment above)
- Replace `MAX_VISIBLE_MEMBERS = 5` truncation with progressive
  disclosure: "Show all N members" / "Show less" toggle when list
  exceeds 5 items

**`PackageList.tsx` summary label:**

- When groups exist: "N other packages" instead of "N individual
  packages"
- When no groups exist: "N packages" (no qualifier)
- Group parenthetical: sum `added_count` and
  `member_count - added_count` across all visible groups, using unique
  package counts (deduplicate across overlapping groups)

**`PackageList.tsx` suppression set:**

Unchanged. Base-image members don't appear in `packages_added` so they
were never in the individual/other packages list to begin with.

**`MainContent.tsx` ungroup follow-ons:**

`MainContent.tsx` uses `group.member_count` and `group.members[0]` for
ungroup behavior. With `member_count` now including base-image members
and `members` containing non-renderable base-image entries, two edits
are required:

- **Ungroup success toast:** Currently shows
  `"Group ungrouped into {member_count} packages"`. Change to use
  `added_count` instead: `"Group ungrouped into {added_count} packages"`.
  Base-image members don't become individually rendered rows after
  ungroup, so the toast should not count them.
- **Post-ungroup focus target:** Currently targets `members[0]` as the
  first focus candidate. Change to target the first member where
  `in_base_image === false`. With the spec's sort order (new members
  first), this is still `members[0]` IF the sort is applied to the
  `GroupInfo.members` array at the adapter level. The spec requires
  the adapter to sort members with new members first — this ensures
  `members[0]` is always a valid focus target when `added_count > 0`.
  When `added_count === 0` (all-from-base group), ungroup dissolves
  nothing visible — the toast should say
  `"Group ungrouped (all packages from base)"` and focus stays on the
  next group row or falls through to the first package row.

### 6. Count deduplication

Groups can share members. When computing the aggregate parenthetical
counts ("4 new, 12 from base"), count unique packages, not membership
slots. Build a `Set<string>` of new member names and a `Set<string>`
of base-image member names across all visible groups. A package that
appears in two groups counts once. If a package is new in one group
and base-image in another (shouldn't happen, but defensive), count it
as new.

Degraded and excluded groups still contribute to the aggregate counts
— their members exist on the host regardless of render state.

## Out of Scope

- **RPM transitive dependency trees** — showing what packages a group
  member pulls in via `Requires`. Different data pipeline.
- **Group toggle behavior changes** — including/excluding a group still
  operates on new members only. Base-image members are context.
- **Base-image member interactivity** — base-image members in the
  expanded group view are read-only labels. No toggles, triage badges,
  or attention indicators.

## Testing Strategy

### Rust collector tests

- Verify `InstalledGroup.members` only contains packages present in the
  installed RPM set (tightened contract).
- Test: group metadata lists 10 members, only 7 are installed → members
  has 7 entries.
- Update existing tests that assert uninstalled optional packages in
  `members`.

### Rust adapter tests

- Verify `in_base_image` is set correctly: member in `packages_added`
  → `false`, member not in `packages_added` → `true`.
- Verify `member_count` is total (new + base-image).
- Verify `added_count` matches the count of members in `packages_added`.
- Verify members are sorted: new members first, then base-image.
- Test: group with all members in base image (Minimal-like) —
  `added_count` is 0, all members have `in_base_image: true`.
- Test: group with no base-image members — `added_count` equals
  `member_count`, no members have `in_base_image: true`.
- Test: group with mixed members.

### Grouped `/api/view` contract fixture

Add a grouped contract fixture in `crates/web/tests/contract_snapshots.rs`
that exercises:
- A snapshot with at least one `InstalledGroup` containing both
  `packages_added` members and base-image-only members.
- Assert `GroupInfo` in the serialized response includes `added_count`,
  `member_count`, and `members` with correct `in_base_image` values.
- This fixture protects the wire format for consumers of the `/api/view`
  endpoint.

### Frontend tests (vitest)

- **GroupRow header labels:** Renders "all from base" when
  `added_count` is 0. Renders "4 new, 8 from base" for mixed groups.
  Renders "4 packages" when all are new.
- **GroupRow expansion:** Base-image members render with "(from base)"
  label. Base-image members appear after new members. Base-image
  members are not toggleable.
- **GroupRow truncation:** Shows "Show all N members" when list exceeds
  5. Clicking expands to full list. Clicking again collapses.
- **GroupRow accessibility:** Base-image members have appropriate
  `aria-label`. Contrast ratio meets WCAG 2.2 AA.
- **PackageList summary:** Renders "other packages" when groups exist.
  Renders "packages" (no qualifier) when no groups. Renders correct
  group parenthetical with new/base counts. Deduplicates across
  overlapping groups.
- **MainContent ungroup toast:** Uses `added_count` not `member_count`.
  All-from-base group shows "(all packages from base)" toast.
- **MainContent ungroup focus:** Post-ungroup focus targets the first
  new (non-base-image) member. All-from-base group does not crash or
  focus a nonexistent row.
- **Contract snapshots:** Update to include `in_base_image` and
  `added_count` fields.
