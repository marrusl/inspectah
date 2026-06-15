# Package Group Dependency Visibility — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show all group members (including base-image ones de-emphasized), clarify summary labels, fix member list truncation.

**Architecture:** Prerequisite collector fix filters `InstalledGroup.members` to installed-only. Adapter adds `in_base_image` + `added_count` to DTOs. Frontend updates GroupRow (labels, base-image rendering, truncation), PackageList (summary label), and MainContent (ungroup toast/focus).

**Tech Stack:** Rust (collector, adapter), React + PatternFly + vitest (frontend).

**Spec:** `process-docs/specs/proposed/2026-06-13-package-group-dep-visibility.md`

---

### Task 0: Tighten `InstalledGroup.members` to installed-only

**Owner:** Tang (Rust)

**Files:**
- Modify: `crates/collect/src/inspectors/rpm/mod.rs`
- Test: inline tests in same file or `crates/collect/tests/`

**Context:** `parse_group_info_packages()` parses `dnf group info` output and collects Mandatory, Default, and Optional packages into `members`. This includes packages from group metadata that may not be installed on the host. The `in_base_image` derivation in later tasks requires that `members` contains only actually-installed packages.

The RPM inspector already has the full installed RPM name set available — `parse_rpm_qa()` runs early in the inspection and produces the package list. The collector needs to filter `InstalledGroup.members` against this set.

**Approach:** In `collect_installed_groups()` or its call site in the RPM inspector's `inspect()` method, after parsing groups, filter each group's `members` to only names that appear in the installed RPM set. The installed RPM set is available as `PackageEntry` names from the `rpm -qa` parsing that happens earlier in the same `inspect()` call.

- [ ] **Step 1: Write the failing test**

Add a test that constructs `dnf group info` output with members including a package NOT in the `rpm -qa` output. Assert that the resulting `InstalledGroup.members` does NOT contain the uninstalled package.

```rust
#[test]
fn installed_groups_filter_uninstalled_members() {
    // Mock rpm -qa output with only httpd and nginx installed
    // Mock dnf group info with "Web Server" group listing httpd, nginx, and tomcat
    // Assert: InstalledGroup.members contains only ["httpd", "nginx"]
    // Assert: "tomcat" is NOT in members
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-collect -- installed_groups_filter`

Expected: FAIL — tomcat appears in members because no filtering is applied.

- [ ] **Step 3: Implement the filter**

In the RPM inspector's `inspect()` method, after calling `collect_installed_groups()`, build a `HashSet<&str>` of installed package names from the parsed RPM entries. Filter each group's `members` to only names present in the set:

```rust
if let Some(ref mut groups) = installed_groups {
    let installed_names: HashSet<&str> = /* build from rpm -qa parsed entries */;
    for group in groups.iter_mut() {
        group.members.retain(|name| installed_names.contains(name.as_str()));
    }
}
```

The exact location depends on where `installed_groups` is assigned and where the RPM parse results are available. Read the inspector's `inspect()` method to find both.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect`

Expected: PASS. Also update any existing tests that assert uninstalled optional packages in `members`.

Run: `cargo clippy -p inspectah-collect -- -W clippy::all`

- [ ] **Step 5: Commit**

```bash
git add crates/collect/src/inspectors/rpm/mod.rs
git commit -m "fix(collect): filter InstalledGroup.members to installed-only packages"
```

---

### Task 1: Add `in_base_image` + `added_count` to Rust DTOs and adapter

**Owner:** Tang (Rust)

**Files:**
- Modify: `crates/web/src/web_types.rs` — `GroupMemberInfo`, `GroupInfo`
- Modify: `crates/web/src/adapter.rs` — group building code
- Modify: `crates/web/ui/src/api/types.ts` — TypeScript types
- Test: inline in `crates/web/src/adapter.rs`

**Context:** The adapter builds `GroupInfo` and `GroupMemberInfo` from `InstalledGroup` + projected snapshot data. Currently `GroupMemberInfo` has `name`, `locked`, `overlap_groups`. Add `in_base_image: bool`. Add `added_count: usize` to `GroupInfo`. Sort members with new (non-base-image) first.

**Current adapter code** (in `build_web_view()`):

```rust
let members: Vec<GroupMemberInfo> = group.members.iter().map(|member_name| {
    let locked = projected_pkgs.iter().any(|p| p.name == *member_name && p.locked);
    let overlap_groups = /* ... */;
    GroupMemberInfo { name: member_name.clone(), locked, overlap_groups }
}).collect();
```

Add `in_base_image` by checking if `member_name` is NOT in `projected_pkgs`:

```rust
let in_base_image = !projected_pkgs.iter().any(|p| p.name == *member_name);
GroupMemberInfo { name: member_name.clone(), locked, overlap_groups, in_base_image }
```

Sort members: non-base-image first, then base-image, alphabetical within each:

```rust
members.sort_by(|a, b| {
    a.in_base_image.cmp(&b.in_base_image).then(a.name.cmp(&b.name))
});
```

Compute `added_count`:

```rust
let added_count = members.iter().filter(|m| !m.in_base_image).count();
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn group_info_includes_in_base_image_and_added_count() {
    // Build snapshot with packages_added containing ["httpd", "mod_ssl"]
    // Build InstalledGroup with members ["httpd", "mod_ssl", "apr", "apr-util"]
    // (apr and apr-util are installed but from base — not in packages_added)
    // Assert: GroupInfo.added_count == 2
    // Assert: GroupInfo.member_count == 4
    // Assert: members[0].in_base_image == false (httpd — new, sorted first)
    // Assert: members[2].in_base_image == true (apr — from base, sorted after)
    // Assert: members are sorted: new first alphabetically, then base alphabetically
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-web -- group_info_includes`

- [ ] **Step 3: Update `web_types.rs`**

Add `in_base_image: bool` to `GroupMemberInfo`:
```rust
pub struct GroupMemberInfo {
    pub name: String,
    pub locked: bool,
    pub overlap_groups: Vec<String>,
    pub in_base_image: bool,
}
```

Add `added_count: usize` to `GroupInfo`:
```rust
pub struct GroupInfo {
    pub name: String,
    pub member_count: usize,
    pub added_count: usize,
    pub locked_count: usize,
    // ... rest unchanged
}
```

- [ ] **Step 4: Update adapter**

In the `package_groups` mapping in `build_web_view()`:
- Add `in_base_image` check to `GroupMemberInfo` construction
- Sort members (new first, then base-image)
- Compute `added_count`
- Set `member_count` to `members.len()` (total)

- [ ] **Step 5: Update TypeScript types**

In `crates/web/ui/src/api/types.ts`, add `in_base_image: boolean` to `GroupMemberInfo` and `added_count: number` to `GroupInfo`.

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-web && cargo clippy -p inspectah-web -- -W clippy::all`

- [ ] **Step 7: Commit**

```bash
git add crates/web/src/web_types.rs crates/web/src/adapter.rs crates/web/ui/src/api/types.ts
git commit -m "feat(refine): add in_base_image and added_count to group DTOs"
```

---

### Task 2: GroupRow header labels + base-image rendering + truncation fix

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/GroupRow.tsx`
- Test: `crates/web/ui/src/components/__tests__/GroupRow.test.tsx`

**Context:** GroupRow currently shows `"N packages"` in the header using `member_count`. It has a hard `MAX_VISIBLE_MEMBERS = 5` cap with no "show all" option. Base-image members are not visually distinguished.

**Three changes:**

**A. Header labels** using `added_count` and `member_count`:
- `added_count === 0`: `"12 packages (all from base)"`
- `added_count === member_count`: `"4 packages"` (unchanged)
- Otherwise: `"4 new, 8 from base"`

Currently (line 84):
```tsx
group.member_count === 1 ? "1 package" : `${group.member_count} packages`;
```

Replace with:
```tsx
const pkgLabel = useMemo(() => {
  if (group.added_count === 0) {
    return `${group.member_count} packages (all from base)`;
  }
  if (group.added_count === group.member_count) {
    return group.member_count === 1 ? "1 package" : `${group.member_count} packages`;
  }
  const baseCount = group.member_count - group.added_count;
  return `${group.added_count} new, ${baseCount} from base`;
}, [group.member_count, group.added_count]);
```

**B. Base-image member rendering** in the expanded list:
- Base-image members: reduced opacity, italic, "(from base)" label, not toggleable
- Added (new) members: render as today
- Members are already sorted by the adapter (new first)

For each member in the expanded list, check `member.in_base_image`:
```tsx
<span
  className={`inspectah-group-row__member-name ${member.in_base_image ? "inspectah-group-row__member--from-base" : ""}`}
  aria-label={member.in_base_image ? `${member.name} (from base image, no action needed)` : undefined}
>
  {member.name}
  {member.in_base_image && (
    <span className="inspectah-group-row__from-base-label"> (from base)</span>
  )}
</span>
```

Add CSS:
```css
.inspectah-group-row__member--from-base {
  opacity: 0.5;
  font-style: italic;
}
.inspectah-group-row__from-base-label {
  font-size: var(--pf-t--global--font--size--xs);
}
```

**C. Truncation fix** — replace hard cap with progressive disclosure:

Replace:
```tsx
const visibleMembers = sortedMembers.slice(0, MAX_VISIBLE_MEMBERS);
const remainingCount = sortedMembers.length - MAX_VISIBLE_MEMBERS;
```

With:
```tsx
const [showAll, setShowAll] = useState(false);
const visibleMembers = showAll ? sortedMembers : sortedMembers.slice(0, 5);
const remainingCount = sortedMembers.length - 5;
```

Replace the static `{remainingCount} more` text with a clickable toggle:
```tsx
{!showAll && remainingCount > 0 && (
  <button
    className="inspectah-group-row__show-all"
    onClick={(e) => { e.stopPropagation(); setShowAll(true); }}
  >
    Show all {sortedMembers.length} members
  </button>
)}
{showAll && sortedMembers.length > 5 && (
  <button
    className="inspectah-group-row__show-all"
    onClick={(e) => { e.stopPropagation(); setShowAll(false); }}
  >
    Show less
  </button>
)}
```

- [ ] **Step 1: Write the failing tests**

```typescript
describe("GroupRow header labels", () => {
  it("shows 'all from base' when added_count is 0", () => {
    const group = makeGroup({ member_count: 12, added_count: 0 });
    render(<GroupRow {...defaultProps} group={group} />);
    expect(screen.getByText(/12 packages \(all from base\)/)).toBeInTheDocument();
  });

  it("shows 'N new, M from base' for mixed groups", () => {
    const group = makeGroup({ member_count: 12, added_count: 4 });
    render(<GroupRow {...defaultProps} group={group} />);
    expect(screen.getByText(/4 new, 8 from base/)).toBeInTheDocument();
  });

  it("shows 'N packages' when all are new", () => {
    const group = makeGroup({ member_count: 4, added_count: 4 });
    render(<GroupRow {...defaultProps} group={group} />);
    expect(screen.getByText(/4 packages/)).toBeInTheDocument();
    expect(screen.queryByText(/from base/)).not.toBeInTheDocument();
  });
});

describe("GroupRow base-image members", () => {
  it("renders base-image members with (from base) label", () => {
    const group = makeGroup({
      members: [
        { name: "httpd", locked: false, overlap_groups: [], in_base_image: false },
        { name: "apr", locked: false, overlap_groups: [], in_base_image: true },
      ],
      member_count: 2,
      added_count: 1,
    });
    // Expand the group, then check
    render(<GroupRow {...defaultProps} group={group} />);
    // click to expand...
    expect(screen.getByText("(from base)")).toBeInTheDocument();
  });
});

describe("GroupRow truncation", () => {
  it("shows 'Show all N members' when list exceeds 5", () => {
    const members = Array.from({ length: 8 }, (_, i) => ({
      name: `pkg${i}`, locked: false, overlap_groups: [], in_base_image: false,
    }));
    const group = makeGroup({ members, member_count: 8, added_count: 8 });
    render(<GroupRow {...defaultProps} group={group} />);
    // expand...
    expect(screen.getByText(/Show all 8 members/)).toBeInTheDocument();
  });
});
```

Adjust `makeGroup` helper to include `added_count` and `in_base_image` fields. Read existing test patterns in `GroupRow.test.tsx` first.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/GroupRow.test.tsx`

- [ ] **Step 3: Implement all three changes** (header labels, base-image rendering, truncation fix)

- [ ] **Step 4: Add CSS** for `.inspectah-group-row__member--from-base` and `.inspectah-group-row__show-all`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/GroupRow.test.tsx`

- [ ] **Step 6: Run full frontend suite**

Run: `cd crates/web/ui && npx vitest run`

Fix any broken tests from type changes (`in_base_image` and `added_count` now required on test fixtures).

- [ ] **Step 7: Commit**

```bash
git add crates/web/ui/src/components/GroupRow.tsx \
       crates/web/ui/src/components/__tests__/GroupRow.test.tsx \
       crates/web/ui/src/App.css
git commit -m "feat(refine): group row header labels, base-image members, progressive disclosure"
```

---

### Task 3: PackageList summary label changes

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

**Context:** The summary line currently reads: `"2 groups (4 packages) · 75 individual packages"`. Three changes:

1. "individual packages" → "other packages" (when groups exist) or just "packages" (no groups)
2. Group parenthetical: `"(4 packages)"` → `"(4 new, 12 from base)"` with deduplication
3. Existing `groupPackageCount` (line 323) counts all members — split into new vs base

**Current summary rendering** (around line 439):
```tsx
{visibleGroups.length} groups ({groupPackageCount} packages) · {individualPackages.length} individual packages
```

Replace with:
```tsx
{visibleGroups.length} {visibleGroups.length === 1 ? "group" : "groups"} ({groupSummaryLabel}) · {displayPackages.length} other {displayPackages.length === 1 ? "package" : "packages"}
```

Where `groupSummaryLabel` is computed from deduplicated counts:

```tsx
const { newCount, baseCount } = useMemo(() => {
  const newSet = new Set<string>();
  const baseSet = new Set<string>();
  for (const group of visibleGroups) {
    for (const member of group.members) {
      if (member.in_base_image) {
        baseSet.add(member.name);
      } else {
        newSet.add(member.name);
      }
    }
  }
  // If a package is new in any group, count as new (defensive)
  for (const name of newSet) baseSet.delete(name);
  return { newCount: newSet.size, baseCount: baseSet.size };
}, [visibleGroups]);

const groupSummaryLabel = useMemo(() => {
  if (newCount === 0) return "all from base";
  if (baseCount === 0) return `${newCount} ${newCount === 1 ? "package" : "packages"}`;
  return `${newCount} new, ${baseCount} from base`;
}, [newCount, baseCount]);
```

When no groups exist, the entire summary `<div>` is already hidden by `{hasGroups && ...}`. The "other" label only shows when groups exist.

- [ ] **Step 1: Write the failing tests**

```typescript
it("shows 'other packages' instead of 'individual packages'", () => {
  // Render PackageList with groups + individual packages
  // Assert: text contains "other packages"
  // Assert: text does NOT contain "individual packages"
});

it("shows 'N new, M from base' in group parenthetical", () => {
  // Render with groups that have mixed in_base_image members
  // Assert: parenthetical shows correct new/base counts
});

it("deduplicates overlapping group members", () => {
  // Two groups sharing a member
  // Assert: count is unique, not double-counted
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 3: Implement the changes**

- [ ] **Step 4: Run tests**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 5: Commit**

```bash
git add crates/web/ui/src/components/PackageList.tsx \
       crates/web/ui/src/components/__tests__/PackageList.test.tsx
git commit -m "feat(refine): update package summary labels for group dep visibility"
```

---

### Task 4: MainContent ungroup toast/focus follow-ons

**Owner:** Kit (frontend)

**Files:**
- Modify: `crates/web/ui/src/components/MainContent.tsx`
- Test: `crates/web/ui/src/components/__tests__/MainContent.ungroup.test.tsx` (or new file)

**Context:** `handleGroupUngroup` in MainContent uses `group.member_count` for the toast and `group.members[0]` for focus. With `member_count` now including base-image members, two edits are needed.

**Current code** (around line 237):
```tsx
const memberCount = group?.member_count ?? 0;
const firstMember = group?.members?.[0];
const message = `Group ungrouped into ${memberCount} package${memberCount !== 1 ? "s" : ""}. Ctrl+Z to undo.`;
```

**Change 1 — Toast uses `added_count`:**
```tsx
const addedCount = group?.added_count ?? 0;
```

Toast message:
```tsx
const message = addedCount === 0
  ? `Group ungrouped (all packages from base). Ctrl+Z to undo.`
  : `Group ungrouped into ${addedCount} package${addedCount !== 1 ? "s" : ""}. Ctrl+Z to undo.`;
```

**Change 2 — Focus targets first new member:**
```tsx
const firstNewMember = group?.members?.find(m => !m.in_base_image);
const firstMemberName = firstNewMember?.name ?? null;
```

With the adapter's sort order (new members first), `members[0]` would work IF the adapter sort is reliable. But explicitly filtering is safer and doesn't depend on sort order.

- [ ] **Step 1: Write the failing tests**

```typescript
it("ungroup toast uses added_count not member_count", () => {
  // Group with member_count: 8, added_count: 3
  // Trigger ungroup
  // Assert: toast says "3 packages" not "8 packages"
});

it("all-from-base group shows special toast", () => {
  // Group with added_count: 0
  // Trigger ungroup
  // Assert: toast says "all packages from base"
});

it("post-ungroup focus targets first new member", () => {
  // Group with members: [{ name: "apr", in_base_image: true }, { name: "httpd", in_base_image: false }]
  // Trigger ungroup
  // Assert: pendingFocusTarget is "httpd" not "apr"
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd crates/web/ui && npx vitest run src/components/__tests__/MainContent.ungroup.test.tsx`

- [ ] **Step 3: Implement the changes**

- [ ] **Step 4: Run tests**

Run: `cd crates/web/ui && npx vitest run`

- [ ] **Step 5: Commit**

```bash
git add crates/web/ui/src/components/MainContent.tsx \
       crates/web/ui/src/components/__tests__/MainContent.ungroup.test.tsx
git commit -m "fix(refine): ungroup toast and focus respect added_count and base-image members"
```

---

### Task 5: Grouped `/api/view` contract fixture + final verification

**Owner:** Tang (Rust) + Kit (frontend)

**Files:**
- Modify: `crates/web/tests/contract_snapshots.rs` — add grouped fixture
- Update: `crates/web/tests/snapshots/` — new/updated snapshot files
- Verify: full workspace

**Context:** The contract snapshot tests currently build snapshots with no installed groups. Add a fixture with at least one `InstalledGroup` that contains both `packages_added` members and base-image-only members. This protects the wire format.

- [ ] **Step 1: Add grouped contract fixture**

In `crates/web/tests/contract_snapshots.rs`, add a test that builds a snapshot with:
- `packages_added` containing `["httpd", "mod_ssl"]`
- `InstalledGroup { name: "Web Server", members: ["httpd", "mod_ssl", "apr", "apr-util"] }`
- (apr and apr-util are not in `packages_added` → from base)

Assert the serialized `ViewResponse`:
- `package_groups[0].added_count == 2`
- `package_groups[0].member_count == 4`
- `package_groups[0].members` contains entries with correct `in_base_image` values
- Members are sorted (new first)

- [ ] **Step 2: Run and accept snapshot**

```bash
cargo test -p inspectah-web --test contract_snapshots
cargo insta accept
```

- [ ] **Step 3: Update any broken tests**

Fix test fixtures across `crates/web/tests/api_test.rs` and frontend tests that now need `in_base_image` and `added_count` fields.

- [ ] **Step 4: Run full Rust test suite**

```bash
cargo test --workspace
```

Expected: exit 0.

- [ ] **Step 5: Run full frontend test suite**

```bash
cd crates/web/ui && npx vitest run --reporter=verbose
```

Expected: exit 0.

- [ ] **Step 6: Run clippy and tsc**

```bash
cargo clippy --workspace -- -W clippy::all
cd crates/web/ui && npx tsc --noEmit
```

Expected: zero warnings, zero errors.

- [ ] **Step 7: Commit**

Stage only exact files:

```bash
git add crates/web/tests/contract_snapshots.rs \
       crates/web/tests/snapshots/contract_snapshots__contract_view.snap \
       crates/web/tests/snapshots/contract_snapshots__grouped_view.snap
git commit -m "test(refine): add grouped contract fixture for package group dep visibility"
```

If additional files changed (api_test.rs, frontend test fixtures), add them by name.
