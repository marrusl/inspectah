# Package Group Dependency Visibility

## Summary

Package groups currently hide members that are already in the base image,
making groups like "Minimal" show "0 packages" with nothing to expand.
The summary line ("2 groups (4 packages) · 75 individual packages") uses
ambiguous wording. This spec fixes both: show all group members with
base-image ones de-emphasized, and clarify the summary language.

## Problem

1. **Hidden base-image members:** A group whose members are all in the
   base image shows "0 packages" with an empty expansion. The user
   cannot see what the group contains or verify that those packages are
   already covered by the target image.

2. **Unclear summary line:** "2 groups (4 packages) · 75 individual
   packages" — "4 packages" is ambiguous (added? total? including deps?),
   and "individual" is implementation jargon that doesn't communicate
   "packages not in any group."

## Design

### 1. Show all group members, annotate base-image ones

When a group is expanded, show ALL members from `InstalledGroup.members`,
not just those that appear in `packages_added`.

- **Added members** (in `packages_added`): render as today — normal
  weight, interactive.
- **Base-image members** (in `InstalledGroup.members` but NOT in
  `packages_added`): render de-emphasized — reduced opacity (0.5),
  italic, with a trailing label "(in base image)". These are read-only
  context, not toggleable.

The group header shows the full member breakdown:
- All members in base: "12 packages (all in base image)"
- Mixed: "4 added, 8 in base image"
- All added (no base-image members): "4 packages" (unchanged)

### 2. Clarify summary line

Change the packages summary from:
```
2 groups (4 packages) · 75 individual packages
```
to:
```
2 groups (4 added, 12 in base) · 75 dependencies
```

- The parenthetical shows added vs base-image member counts across all
  groups.
- "individual" → "dependencies" — these are packages not part of any
  DNF group, which are transitive dependencies of the explicitly
  installed packages.
- When all group members are in the base image: "2 groups (all in base)
  · 75 dependencies"
- When there are no groups: "75 dependencies" (no group prefix).

### 3. Data model changes

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
appears in `packages_added`. If it does NOT, set `in_base_image: true`.
The adapter already iterates `InstalledGroup.members` and has access to
`packages_added` via the snapshot.

Currently, `member_count` counts only added members. Keep `member_count`
as the total member count (all members including base-image ones). Add
`added_count: usize` for the number of members that are in
`packages_added`.

**Rust `GroupInfo`** (in `crates/web/src/web_types.rs`):

Add `added_count: usize`:
```rust
pub struct GroupInfo {
    pub name: String,
    pub member_count: usize,      // total members (added + base-image)
    pub added_count: usize,       // members in packages_added only
    pub locked_count: usize,
    pub optional_spillover_count: usize,
    pub render_state: String,
    pub degradation_reason: Option<String>,
    pub members: Vec<GroupMemberInfo>,
}
```

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
  member_count: number;       // total (added + base-image)
  added_count: number;        // added only
  locked_count: number;
  optional_spillover_count: number;
  render_state: "renderable" | "excluded" | "ungrouped" | "degraded";
  degradation_reason: string | null;
  members: GroupMemberInfo[];
}
```

### 4. Frontend changes

**`GroupRow.tsx`:**

- Header label uses `added_count` and `member_count`:
  - `added_count === 0`: "12 packages (all in base image)"
  - `added_count === member_count`: "4 packages" (unchanged)
  - Otherwise: "4 added, 8 in base image"
- Expanded member list includes base-image members:
  - Added members: render as today
  - Base-image members: reduced opacity, italic, "(in base image)"
    label. Not toggleable — they are context, not decisions.
- Base-image members sort after added members within the expanded list.

**`PackageList.tsx` (or `MainContent.tsx` — wherever the summary is):**

- Change "N individual packages" → "N dependencies"
- Change group parenthetical to use `added_count` / `member_count`
  across all groups:
  - Sum `added_count` and `member_count - added_count` across all
    visible groups for the aggregate parenthetical.

**`PackageList.tsx` suppression set:**

- The suppression set currently removes renderable group members from
  the individual (now "dependencies") list. This logic is unchanged —
  base-image members don't appear in `packages_added` so they were
  never in the individual list to begin with.

## Out of Scope

- **RPM transitive dependency trees** — showing what packages a group
  member pulls in via `Requires`. That's a different data pipeline
  problem.
- **Group toggle behavior changes** — including/excluding a group still
  operates on the added members only. Base-image members are context.
- **Base-image member interactivity** — base-image members in the
  expanded group view are read-only labels. They don't have toggles,
  triage badges, or attention indicators.

## Testing Strategy

### Rust adapter tests

- Verify `in_base_image` is set correctly: member in `packages_added`
  → `false`, member not in `packages_added` → `true`.
- Verify `member_count` is total (added + base-image).
- Verify `added_count` matches the count of members in `packages_added`.
- Test edge case: group with all members in base image (Minimal-like).
- Test edge case: group with no base-image members (all added).

### Frontend tests (vitest)

- **GroupRow:** Renders "all in base image" label when `added_count` is 0.
  Renders "4 added, 8 in base image" for mixed groups. Renders base-image
  members with de-emphasized styling and "(in base image)" label.
  Base-image members appear after added members. Base-image members are
  not toggleable.
- **PackageList summary:** Renders "dependencies" instead of "individual
  packages". Renders correct group parenthetical with added/base counts.
- **Contract snapshots:** Update to include `in_base_image` and
  `added_count` fields.
