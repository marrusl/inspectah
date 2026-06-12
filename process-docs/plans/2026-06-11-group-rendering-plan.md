# Group-Aware Rendering & Refine UI — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render Anaconda-installed DNF groups as `dnf group install` lines in the Containerfile and as collapsible, atomic group rows in the refine UI.

**Architecture:** Data model changes in `crates/core`, session + projection mechanics in `crates/refine`, Containerfile rendering in `crates/pipeline`, web API + UI in `crates/web`. The session timeline evolves from `Vec<RefinementOp>` to `Vec<TimelineEntry>` (interleaved ops + view directives). A new `RenderContext` carries group rendering state alongside the projected snapshot, keeping `InspectionSnapshot` pure. The Containerfile renderer partitions packages into grouped / ungrouped / degraded / individual buckets.

**Tech Stack:** Rust (serde, thiserror), TypeScript/React (web UI), Playwright (e2e tests)

**Spec:** `process-docs/specs/proposed/2026-06-11-group-rendering-spec.md` (R3, approved)

**Dependency:** The Anaconda gap classifier spec must ship first — it provides `InstalledGroup` collection in the RPM inspector. This plan assumes group data is available on the snapshot.

**Owner assignments:** Tang (Rust backend: data model, session, autosave, renderability, renderer, web API), Kit (frontend: web UI components, search, a11y, e2e). Thorn checkpoints after each risk boundary (see checkpoint markers in body).

**Migration note:** The `/api/op` endpoint switches directly to `TimelineEntry`. No backward-compat envelope — the frontend and backend land together.

---

## Phase 1: Data Model Foundation

### Task 1: Amend InstalledGroup struct

**Files:**
- Modify: `crates/core/src/types/rpm.rs` (InstalledGroup struct, ~line 167)
- Test: `crates/core/src/types/rpm.rs` (inline #[cfg(test)] module)

- [ ] **Step 1: Write the failing test — serde round-trip with new fields**

```rust
#[test]
fn installed_group_new_fields_round_trip() {
    let group = InstalledGroup {
        name: "Container Management".into(),
        members: vec!["podman".into(), "buildah".into()],
        optional_installed: vec!["python3-podman".into()],
    };
    let json = serde_json::to_string(&group).unwrap();
    let back: InstalledGroup = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "Container Management");
    assert_eq!(back.members, vec!["podman", "buildah"]);
    assert_eq!(back.optional_installed, vec!["python3-podman"]);
}

#[test]
fn installed_group_old_format_loads_via_alias() {
    // Old snapshots use "packages" instead of "members"
    let json = r#"{"name":"Dev Tools","packages":["gcc","make"]}"#;
    let group: InstalledGroup = serde_json::from_str(json).unwrap();
    assert_eq!(group.members, vec!["gcc", "make"]);
    assert!(group.optional_installed.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-core installed_group -- --nocapture`
Expected: FAIL — `members` field does not exist yet

- [ ] **Step 3: Amend the struct**

Replace the current `InstalledGroup` definition:

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InstalledGroup {
    #[serde(default)]
    pub name: String,
    #[serde(default, alias = "packages")]
    pub members: Vec<String>,
    #[serde(default)]
    pub optional_installed: Vec<String>,
}
```

- [ ] **Step 4: Fix any compilation errors from the rename**

Search for all usages of `.packages` on `InstalledGroup` across the codebase:
```bash
grep -rn 'InstalledGroup' crates/ --include='*.rs' | grep -v target
grep -rn '\.packages' crates/ --include='*.rs' | grep InstalledGroup
```
Update all references from `.packages` to `.members`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p inspectah-core installed_group -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/types/rpm.rs
git commit -m "feat(core): amend InstalledGroup with members alias and optional_installed"
```

---

### Task 2: Add TimelineEntry and ViewDirective types

**Files:**
- Modify: `crates/refine/src/types.rs` (add new enums after RefinementOp)
- Test: `crates/refine/src/types.rs` (inline tests)

- [ ] **Step 1: Write the failing test — TimelineEntry serde round-trip**

```rust
#[test]
fn timeline_entry_op_round_trip() {
    let entry = TimelineEntry::Op(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "httpd".into(),
            arch: "x86_64".into(),
        },
        include: false,
    });
    let json = serde_json::to_string(&entry).unwrap();
    let back: TimelineEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}

#[test]
fn timeline_entry_view_round_trip() {
    let entry = TimelineEntry::View(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    });
    let json = serde_json::to_string(&entry).unwrap();
    let back: TimelineEntry = serde_json::from_str(&json).unwrap();
    assert_eq!(entry, back);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine timeline_entry -- --nocapture`
Expected: FAIL — types don't exist

- [ ] **Step 3: Add the types**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TimelineEntry {
    Op(RefinementOp),
    View(ViewDirective),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "directive")]
pub enum ViewDirective {
    UngroupGroup { group_name: String },
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine timeline_entry -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/types.rs
git commit -m "feat(refine): add TimelineEntry and ViewDirective types"
```

---

### Task 3: Add ItemId::Group variant

**Files:**
- Modify: `crates/refine/src/types.rs` (ItemId enum, add Group variant)
- Modify: `crates/web/ui/src/api/types.ts` (add ItemIdGroup)
- Test: `crates/refine/src/types.rs` (inline tests)

- [ ] **Step 1: Write the failing test — ItemId::Group serde**

```rust
#[test]
fn item_id_group_round_trip() {
    let id = ItemId::Group {
        name: "Development Tools".into(),
    };
    let json = serde_json::to_string(&id).unwrap();
    assert!(json.contains("Group"));
    let back: ItemId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p inspectah-refine item_id_group -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add the variant to ItemId**

Add to the `ItemId` enum in `crates/refine/src/types.rs`:

```rust
// Group section (new — group-aware rendering)
Group {
    name: String,
},
```

- [ ] **Step 4: Fix exhaustive match arms**

Search for `match` on `ItemId` across the codebase and add `ItemId::Group { .. }` arms. Key locations:
- `session.rs`: `validate_target()`, `is_item_locked()`, `is_op_noop()`
- `projection/decisions.rs`: any match on ItemId

For now, add stub arms that return `Err(RefineError::BadRequest("group ops not yet implemented"))` or equivalent. These will be implemented in Phase 2.

- [ ] **Step 5: Add TypeScript type**

In `crates/web/ui/src/api/types.ts`, add:

```typescript
export interface ItemIdGroup {
  kind: "Group";
  key: { name: string };
}
```

Add `| ItemIdGroup` to the `ItemId` union type.

- [ ] **Step 6: Run full test suite**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: PASS (stubs compile, existing tests unaffected)

- [ ] **Step 7: Commit**

```bash
git add crates/refine/src/types.rs crates/web/ui/src/api/types.ts
git commit -m "feat(refine): add ItemId::Group variant"
```

---

### Task 4: Add GroupRenderState, DegradationReason, and RenderContext types

These types live in `crates/core` (not `crates/refine`) so both `crates/refine` (produces RenderContext) and `crates/pipeline` (consumes it for rendering) can depend on them without a crate cycle.

**Files:**
- Create: `crates/core/src/types/group_render.rs`
- Modify: `crates/core/src/types/mod.rs` (add module)
- Test: `crates/core/src/types/group_render.rs` (inline tests)

- [ ] **Step 1: Write the tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_context_default_is_empty() {
        let ctx = RenderContext::default();
        assert!(ctx.group_states.is_empty());
    }

    #[test]
    fn group_render_state_serde_round_trip() {
        let state = GroupRenderState::Degraded {
            reason: DegradationReason::MultilibConflict,
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: GroupRenderState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }

    #[test]
    fn render_context_is_renderable_helper() {
        let mut ctx = RenderContext::default();
        ctx.group_states.insert(
            "Dev Tools".into(),
            GroupRenderState::Renderable,
        );
        ctx.group_states.insert(
            "Container Management".into(),
            GroupRenderState::Excluded,
        );
        assert!(ctx.is_renderable("Dev Tools"));
        assert!(!ctx.is_renderable("Container Management"));
        assert!(!ctx.is_renderable("Nonexistent"));
    }
}
```

- [ ] **Step 2: Create the module**

Create `crates/core/src/types/group_render.rs`:

```rust
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state")]
pub enum GroupRenderState {
    Renderable,
    Excluded,
    Ungrouped,
    Degraded { reason: DegradationReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradationReason {
    MemberExcluded,
    MemberOverridden,
    MultilibConflict,
}

#[derive(Debug, Clone, Default)]
pub struct RenderContext {
    pub group_states: HashMap<String, GroupRenderState>,
}

impl RenderContext {
    pub fn is_renderable(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Renderable)
        )
    }

    pub fn is_excluded(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Excluded)
        )
    }

    pub fn is_ungrouped(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Ungrouped)
        )
    }

    pub fn is_degraded(&self, group_name: &str) -> bool {
        matches!(
            self.group_states.get(group_name),
            Some(GroupRenderState::Degraded { .. })
        )
    }
}
```

- [ ] **Step 3: Register the module**

Add `pub mod group_render;` to `crates/core/src/types/mod.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-core group_render -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/types/group_render.rs crates/core/src/types/mod.rs
git commit -m "feat(core): add GroupRenderState, DegradationReason, RenderContext types"
```

---

## Phase 2: Session Mechanics

### Task 5: Migrate RefineSession to Vec<TimelineEntry>

**Files:**
- Modify: `crates/refine/src/session.rs` (internal ops field, apply, undo, redo)
- Test: existing session tests must still pass

This is the largest single task. The session currently stores `ops: Vec<RefinementOp>` and `cursor: usize`. It must now store `timeline: Vec<TimelineEntry>` with the same cursor.

- [ ] **Step 1: Write a migration test**

```rust
#[test]
fn session_timeline_migration_preserves_existing_ops() {
    let mut snap = test_snapshot();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "httpd".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }).unwrap();
    assert_eq!(session.timeline_len(), 1);
    assert!(session.can_undo());
}
```

- [ ] **Step 2: Rename internal field from `ops` to `timeline`**

In `session.rs`, change:
- `ops: Vec<RefinementOp>` → `timeline: Vec<TimelineEntry>`
- `apply()`: wrap op in `TimelineEntry::Op(op)` before pushing
- `undo()` / `redo()`: adjust to work with `TimelineEntry`
- `recompute_view()`: extract `RefinementOp` entries from timeline for projection
- Add `pub fn timeline_len(&self) -> usize` helper

The key insight: `project_snapshot()` only processes `TimelineEntry::Op` entries. `TimelineEntry::View` entries are collected separately for `RenderContext`.

- [ ] **Step 3: Update all internal references**

Search for `self.ops` in session.rs and update:
- `self.ops.truncate(self.cursor)` → `self.timeline.truncate(self.cursor)`
- `self.ops.push(op)` → `self.timeline.push(TimelineEntry::Op(op))`
- `self.ops.len()` → `self.timeline.len()`
- Projection replay: filter `self.timeline[..self.cursor]` for `TimelineEntry::Op` entries

- [ ] **Step 4: Run full session test suite**

Run: `cargo test -p inspectah-refine session -- --nocapture`
Expected: ALL PASS — existing behavior preserved

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "refactor(refine): migrate session from Vec<RefinementOp> to Vec<TimelineEntry>"
```

---

### Task 6: Add apply_directive() for UngroupGroup

**Files:**
- Modify: `crates/refine/src/session.rs`
- Test: `crates/refine/src/session.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn ungroup_adds_view_directive_to_timeline() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    assert_eq!(session.timeline_len(), 1);
    assert!(session.can_undo());
}

#[test]
fn ungroup_idempotent_on_already_ungrouped() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    let len_after_first = session.timeline_len();
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    assert_eq!(session.timeline_len(), len_after_first, "idempotent");
}

#[test]
fn ungroup_unknown_group_returns_error() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    let result = session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Nonexistent Group".into(),
    });
    assert!(result.is_err());
}

#[test]
fn undo_ungroup_regroups() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    session.undo().unwrap();
    assert_eq!(session.timeline_len(), 1);
    assert_eq!(session.cursor(), 0);
}
```

Add a `test_snapshot_with_groups()` helper that creates a snapshot with `installed_groups` populated (Container Management with podman, buildah, skopeo as members).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine ungroup -- --nocapture`
Expected: FAIL — `apply_directive` does not exist

- [ ] **Step 3: Implement apply_directive()**

```rust
pub fn apply_directive(
    &mut self,
    directive: ViewDirective,
) -> Result<(), RefineError> {
    // Validate: group must exist in installed_groups
    match &directive {
        ViewDirective::UngroupGroup { group_name } => {
            let groups = self.installed_groups();
            if !groups.iter().any(|g| g.name == *group_name) {
                return Err(RefineError::BadRequest(
                    format!("unknown group: {group_name}")
                ));
            }
        }
    }

    // Check idempotency
    if self.is_directive_noop(&directive) {
        return Ok(());
    }

    // Truncate redo history at cursor
    self.timeline.truncate(self.cursor);
    self.timeline.push(TimelineEntry::View(directive));
    self.cursor += 1;
    self.generation += 1;
    self.cached_view = None;
    self.cached_decisions = None;
    self.recompute_view();
    self.try_autosave();
    Ok(())
}

fn is_directive_noop(&self, directive: &ViewDirective) -> bool {
    match directive {
        ViewDirective::UngroupGroup { group_name } => {
            self.render_context().is_ungrouped(group_name)
        }
    }
}
```

Also add a helper `installed_groups()` that reads from `self.original.rpm.installed_groups`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p inspectah-refine ungroup -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "feat(refine): add apply_directive() for UngroupGroup with idempotency"
```

---

### Task 7: Add group-level SetInclude fan-out in projection

**Files:**
- Modify: `crates/refine/src/session.rs` (projection replay)
- Test: `crates/refine/src/session.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn set_include_group_false_excludes_all_members() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Container Management".into(),
        },
        include: false,
    }).unwrap();
    let view = session.view();
    for pkg in &view.packages {
        if ["podman", "buildah", "skopeo"].contains(&pkg.entry.name.as_str()) {
            assert!(!pkg.entry.include, "{} should be excluded", pkg.entry.name);
        }
    }
}

#[test]
fn set_include_group_true_includes_non_locked_members() {
    let snap = test_snapshot_with_groups_and_locked();
    let mut session = RefineSession::new(snap);
    // First exclude the group
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Development Tools".into(),
        },
        include: false,
    }).unwrap();
    // Then re-include
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Development Tools".into(),
        },
        include: true,
    }).unwrap();
    let view = session.view();
    for pkg in &view.packages {
        if pkg.entry.name == "binutils" && pkg.entry.locked {
            assert!(!pkg.entry.include, "locked member stays excluded");
        } else if pkg.entry.name == "gcc" {
            assert!(pkg.entry.include, "non-locked member re-included");
        }
    }
}

#[test]
fn set_include_group_fan_out_handles_multiarch() {
    // glibc appears as both x86_64 and i686
    let snap = test_snapshot_with_multiarch_group();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Development Tools".into(),
        },
        include: false,
    }).unwrap();
    let view = session.view();
    let glibcs: Vec<_> = view.packages.iter()
        .filter(|p| p.entry.name == "glibc")
        .collect();
    assert!(glibcs.len() >= 2, "both arches present");
    for g in &glibcs {
        assert!(!g.entry.include, "both arches excluded");
    }
}

#[test]
fn undo_group_exclude_restores_all_members() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Container Management".into(),
        },
        include: false,
    }).unwrap();
    session.undo().unwrap();
    let view = session.view();
    for pkg in &view.packages {
        if ["podman", "buildah", "skopeo"].contains(&pkg.entry.name.as_str()) {
            assert!(pkg.entry.include, "{} should be restored", pkg.entry.name);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine set_include_group -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement fan-out in projection**

In the projection replay loop within `recompute_view()` / `project_snapshot()`, when encountering `SetInclude { item_id: ItemId::Group { name }, include }`:

1. Look up `InstalledGroup` by name from the snapshot
2. For each member name in `group.members`, find ALL matching `PackageEntry` records in `packages_added` (match by name, any arch)
3. Set `include` on each match, respecting lock (skip locked items when trying to include)

```rust
ItemId::Group { name } => {
    if let Some(groups) = &self.original.rpm.as_ref()
        .and_then(|r| r.installed_groups.as_ref())
    {
        if let Some(group) = groups.iter().find(|g| g.name == *name) {
            for member_name in &group.members {
                for pkg in projected.rpm.as_mut()
                    .map(|r| r.packages_added.iter_mut())
                    .into_iter().flatten()
                {
                    if pkg.name == *member_name {
                        if *include && pkg.locked {
                            continue; // locked stays excluded
                        }
                        pkg.include = *include;
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Add validate_target for Group**

In `validate_target()`, add:
```rust
ItemId::Group { name } => {
    let exists = self.installed_groups()
        .iter()
        .any(|g| g.name == *name);
    if !exists {
        return Err(RefineError::BadRequest(
            format!("unknown group: {name}")
        ));
    }
    Ok(())
}
```

- [ ] **Step 5: Add is_op_noop for Group**

```rust
ItemId::Group { name } => {
    if let Some(group) = self.find_group(name) {
        let projected = self.snapshot_projected();
        for member_name in &group.members {
            for pkg in projected.rpm_packages() {
                if pkg.name == *member_name && !pkg.locked {
                    if pkg.include != *include {
                        return false; // at least one differs
                    }
                }
            }
        }
        true // all match already
    } else {
        true // unknown group, treat as noop
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-refine set_include_group -- --nocapture`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "feat(refine): group-level SetInclude with member fan-out in projection"
```

---

### Task 8: Add optional-installed independence tests

**Files:**
- Modify: `crates/refine/src/session.rs` (tests only)

- [ ] **Step 1: Write tests**

```rust
#[test]
fn optional_installed_not_affected_by_group_exclude() {
    let snap = test_snapshot_with_optional_members();
    let mut session = RefineSession::new(snap);
    // Record the optional package's include state
    let opt_before = session.view().packages.iter()
        .find(|p| p.entry.name == "python3-podman")
        .unwrap().entry.include;
    // Exclude the group
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Container Management".into(),
        },
        include: false,
    }).unwrap();
    let opt_after = session.view().packages.iter()
        .find(|p| p.entry.name == "python3-podman")
        .unwrap().entry.include;
    assert_eq!(opt_before, opt_after, "optional member unchanged");
}
```

- [ ] **Step 2: Run and verify**

Run: `cargo test -p inspectah-refine optional_installed -- --nocapture`
Expected: PASS (optional members are in `optional_installed`, not `members`, so fan-out skips them)

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "test(refine): verify optional-installed packages are independent of group toggle"
```

---

### Task 9: Add overlap and last-writer-wins tests

**Files:**
- Modify: `crates/refine/src/session.rs` (tests only)

- [ ] **Step 1: Write tests**

```rust
#[test]
fn individual_op_after_group_op_takes_precedence() {
    let snap = test_snapshot_with_overlapping_groups();
    let mut session = RefineSession::new(snap);
    // Exclude group A (which contains podman)
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Group {
            name: "Container Management".into(),
        },
        include: false,
    }).unwrap();
    // Re-include podman individually
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "podman".into(),
            arch: "x86_64".into(),
        },
        include: true,
    }).unwrap();
    let podman = session.view().packages.iter()
        .find(|p| p.entry.name == "podman")
        .unwrap();
    assert!(podman.entry.include, "individual op wins");
}
```

- [ ] **Step 2: Run and verify**

Run: `cargo test -p inspectah-refine individual_op_after_group -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "test(refine): verify last-writer-wins for group vs individual ops"
```

---

> **THORN CHECKPOINT 1:** Review Tasks 1–9 (data model + session mechanics). Focus on: type safety, serde contracts, fan-out correctness, undo behavior, and whether illegal states are representable.

---

## Phase 3: Autosave Migration

### Task 10: Bump autosave to schema v3

**Files:**
- Modify: `crates/refine/src/autosave.rs`
- Test: `crates/refine/src/autosave.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn v3_session_round_trips_with_timeline_entries() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    write_dummy_tarball(&tarball);
    let hash = compute_tarball_hash(&tarball).unwrap();

    let state = SessionState {
        schema_version: 3,
        tarball_path: tarball.clone(),
        tarball_hash: hash.clone(),
        timeline: vec![
            TimelineEntry::Op(RefinementOp::SetInclude {
                item_id: ItemId::Package {
                    name: "vim".into(),
                    arch: "x86_64".into(),
                },
                include: false,
            }),
            TimelineEntry::View(ViewDirective::UngroupGroup {
                group_name: "Dev Tools".into(),
            }),
        ],
        cursor: 2,
        saved_at: "100s".into(),
    };
    save_session(&state, &tarball).unwrap();
    let loaded = load_session(&tarball).unwrap().unwrap();
    assert_eq!(loaded.schema_version, 3);
    assert_eq!(loaded.timeline.len(), 2);
    assert_eq!(loaded.cursor, 2);
}

#[test]
fn v2_session_migrates_to_v3_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let tarball = dir.path().join("test.tar.gz");
    write_dummy_tarball(&tarball);
    let hash = compute_tarball_hash(&tarball).unwrap();

    let v2_json = serde_json::json!({
        "schema_version": 2,
        "tarball_path": tarball.to_string_lossy(),
        "tarball_hash": hash.as_str(),
        "ops": [{
            "op": "SetInclude",
            "target": {
                "item_id": {"kind": "Package", "key": {"name": "vim", "arch": "x86_64"}},
                "include": false
            }
        }],
        "cursor": 1,
        "saved_at": "200s"
    });
    let session_path = session_file_path(&tarball);
    std::fs::write(&session_path, serde_json::to_string_pretty(&v2_json).unwrap()).unwrap();

    let loaded = load_session(&tarball).unwrap().unwrap();
    assert_eq!(loaded.schema_version, 3);
    assert_eq!(loaded.timeline.len(), 1);
    match &loaded.timeline[0] {
        TimelineEntry::Op(RefinementOp::SetInclude { .. }) => {}
        other => panic!("expected Op(SetInclude), got {other:?}"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine v3_session -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Update SessionState struct**

```rust
pub struct SessionState {
    pub schema_version: u32,
    pub tarball_path: PathBuf,
    pub tarball_hash: ContentHash,
    pub timeline: Vec<TimelineEntry>,
    pub cursor: usize,
    pub saved_at: String,
}
```

- [ ] **Step 4: Update load_session to handle v2→v3 migration**

In `load_session()`, after loading:
- If `schema_version == 2`: deserialize `ops: Vec<RefinementOp>`, wrap each in `TimelineEntry::Op(...)`, set `schema_version = 3`
- If `schema_version == 3`: deserialize `timeline: Vec<TimelineEntry>` directly
- Otherwise: error

- [ ] **Step 5: Update save_session to write v3 format**

Change `save_session()` to serialize using `timeline` field name.

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-refine -- --nocapture`
Expected: ALL PASS

- [ ] **Step 7: Commit**

```bash
git add crates/refine/src/autosave.rs
git commit -m "feat(refine): bump autosave to schema v3 with TimelineEntry migration"
```

---

## Phase 4: Group Renderability

### Task 11: Implement full group state derivation

The state derivation function must produce all four `GroupRenderState` variants. It needs access to:
- The group definition (`InstalledGroup`)
- The effective projected packages (post-classification, post-baseline-suppression)
- The ungrouped set (from `ViewDirective` entries)
- Whether the group was explicitly excluded via `SetInclude(Group, false)` — derived from the timeline
- Whether any individual `SetInclude(Package)` op overrides a group member AFTER a group-level op — this is what makes `MemberOverridden` reachable

**State derivation logic:**
1. If ungrouped → `Ungrouped`
2. If group-level SetInclude(false) is the most recent group-level op AND all non-locked members are excluded AND no individual op re-includes any member after the group op → `Excluded`
3. If any non-locked member has include state that differs from what `dnf group install` would produce:
   - If the divergence is from an individual op AFTER a group-level op → `Degraded { MemberOverridden }`
   - If multilib conflict → `Degraded { MultilibConflict }`
   - Otherwise → `Degraded { MemberExcluded }`
4. Otherwise → `Renderable`

**Files:**
- Create: `crates/refine/src/group_state.rs` (derivation logic, separate from types in core)
- Modify: `crates/refine/src/lib.rs` (add module)
- Test: `crates/refine/src/group_state.rs` (inline tests)

- [ ] **Step 1: Write failing tests for all four states**

```rust
#[test]
fn all_members_included_no_overrides_is_renderable() {
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_all_included(&["gcc", "make"]),
        ungrouped: false,
        group_excluded: false,
        divergent_overrides: &HashSet::new(),
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
}

#[test]
fn group_level_exclude_with_all_members_off_is_excluded() {
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_all_excluded(&["gcc", "make"]),
        ungrouped: false,
        group_excluded: true,
        divergent_overrides: &HashSet::new(),
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Excluded);
}

#[test]
fn group_excluded_but_member_reincluded_individually_is_degraded_overridden() {
    // gcc was re-included individually AFTER SetInclude(Group, false)
    // and that diverges from the group op's intent → MemberOverridden
    let mut divergent = HashSet::new();
    divergent.insert("gcc".into());
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_with_overrides(&[("gcc", true), ("make", false)]),
        ungrouped: false,
        group_excluded: true,
        divergent_overrides: &divergent,
    };
    assert!(matches!(
        derive_group_state(&ctx),
        GroupRenderState::Degraded { reason: DegradationReason::MemberOverridden }
    ));
}

#[test]
fn reaffirming_member_op_does_not_degrade() {
    // SetInclude(Group, false) then SetInclude(Package("make"), false)
    // The individual op matches group intent → no divergence → Excluded
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_all_excluded(&["gcc", "make"]),
        ungrouped: false,
        group_excluded: true,
        divergent_overrides: &HashSet::new(), // no divergence
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Excluded);
}

#[test]
fn shared_member_op_for_other_group_does_not_degrade_this_group() {
    // Groups A={x,y} and B={y,z}. Individual op on y after group-B
    // exclude should NOT degrade group A if A has no group-level op.
    let ctx = GroupEvalContext {
        group: &group("Group A", &["x", "y"]),
        effective_packages: &packages_all_included(&["x", "y"]),
        ungrouped: false,
        group_excluded: false,
        divergent_overrides: &HashSet::new(), // y's op was for group B
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
}

#[test]
fn member_excluded_without_group_op_is_degraded_member_excluded() {
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_with_overrides(&[("gcc", true), ("make", false)]),
        ungrouped: false,
        group_excluded: false,
        divergent_overrides: &HashSet::new(),
    };
    assert!(matches!(
        derive_group_state(&ctx),
        GroupRenderState::Degraded { reason: DegradationReason::MemberExcluded }
    ));
}

#[test]
fn multilib_member_is_degraded() {
    let pkgs = vec![
        pkg("glibc", "x86_64", true, false),
        pkg("glibc", "i686", true, false),
        pkg("gcc", "x86_64", true, false),
    ];
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["glibc", "gcc"]),
        effective_packages: &pkgs,
        ungrouped: false,
        group_excluded: false,
        divergent_overrides: &HashSet::new(),
    };
    assert!(matches!(
        derive_group_state(&ctx),
        GroupRenderState::Degraded { reason: DegradationReason::MultilibConflict }
    ));
}

#[test]
fn locked_members_do_not_trigger_degradation() {
    let pkgs = vec![
        pkg("gcc", "x86_64", true, false),
        pkg("binutils", "x86_64", false, true), // locked, excluded
    ];
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "binutils"]),
        effective_packages: &pkgs,
        ungrouped: false,
        group_excluded: false,
        divergent_overrides: &HashSet::new(),
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Renderable);
}

#[test]
fn ungrouped_group_is_ungrouped() {
    let ctx = GroupEvalContext {
        group: &group("Dev Tools", &["gcc", "make"]),
        effective_packages: &packages_all_included(&["gcc", "make"]),
        ungrouped: true,
        group_excluded: false,
        divergent_overrides: &HashSet::new(),
    };
    assert_eq!(derive_group_state(&ctx), GroupRenderState::Ungrouped);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine derive_group_state -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement derive_group_state()**

```rust
pub struct GroupEvalContext<'a> {
    pub group: &'a InstalledGroup,
    pub effective_packages: &'a [PackageEntry],
    pub ungrouped: bool,
    pub group_excluded: bool,
    /// Per-group override set: package names that have an individual
    /// SetInclude(Package) op AFTER the most recent group-level op
    /// for THIS group, AND whose resulting include state diverges
    /// from what the group-level op would have produced.
    /// Built per-group during timeline scanning — NOT a global set.
    /// A reaffirming op (individual op that matches the group op's
    /// intent) does NOT count as an override.
    /// A member shared with another group is only flagged if the
    /// diverging individual op references this member AND appears
    /// after this group's most recent group-level op.
    pub divergent_overrides: &'a HashSet<String>,
}

pub fn derive_group_state(ctx: &GroupEvalContext) -> GroupRenderState {
    // Priority 1: ungrouped
    if ctx.ungrouped {
        return GroupRenderState::Ungrouped;
    }

    // Check member states on the effective surface
    let mut any_non_locked_excluded = false;
    let mut all_non_locked_excluded = true;
    let mut has_non_locked = false;

    for member_name in &ctx.group.members {
        let matching: Vec<_> = ctx.effective_packages.iter()
            .filter(|p| p.name == *member_name)
            .collect();

        if matching.is_empty() {
            continue; // not on effective surface
        }

        // Multi-arch conflict
        if matching.len() > 1 {
            return GroupRenderState::Degraded {
                reason: DegradationReason::MultilibConflict,
            };
        }

        let pkg = matching[0];
        if pkg.locked {
            continue; // expected state, not degradation
        }

        has_non_locked = true;

        // Check for divergent individual override after group op
        if ctx.divergent_overrides.contains(&pkg.name) {
            return GroupRenderState::Degraded {
                reason: DegradationReason::MemberOverridden,
            };
        }

        if !pkg.include {
            any_non_locked_excluded = true;
        } else {
            all_non_locked_excluded = false;
        }
    }

    // Priority 2: explicit group exclude with all non-locked
    // members confirmed off and no divergent overrides
    if ctx.group_excluded && has_non_locked && all_non_locked_excluded {
        return GroupRenderState::Excluded;
    }

    // Priority 3: non-locked member excluded without group op
    if any_non_locked_excluded {
        return GroupRenderState::Degraded {
            reason: DegradationReason::MemberExcluded,
        };
    }

    GroupRenderState::Renderable
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine derive_group_state -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/group_state.rs crates/refine/src/lib.rs
git commit -m "feat(refine): implement full group state derivation with all four states"
```

---

### Task 12: Build RenderContext during view computation

**Files:**
- Modify: `crates/refine/src/session.rs` (recompute_view)
- Test: `crates/refine/src/session.rs` (inline tests)

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn render_context_built_during_view_computation() {
    let snap = test_snapshot_with_groups();
    let session = RefineSession::new(snap);
    let ctx = session.render_context();
    // All groups start renderable
    assert!(ctx.is_renderable("Container Management"));
}

#[test]
fn render_context_reflects_ungroup() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    let ctx = session.render_context();
    assert!(ctx.is_ungrouped("Container Management"));
}

#[test]
fn render_context_reflects_degradation() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    // Exclude one member individually to trigger degradation
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "podman".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }).unwrap();
    let ctx = session.render_context();
    assert!(ctx.is_degraded("Container Management"));
}

#[test]
fn render_context_auto_upgrades_after_undo() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "podman".into(),
            arch: "x86_64".into(),
        },
        include: false,
    }).unwrap();
    assert!(session.render_context().is_degraded("Container Management"));
    session.undo().unwrap();
    assert!(session.render_context().is_renderable("Container Management"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p inspectah-refine render_context -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Add render_context field and computation**

Add `cached_render_context: Option<RenderContext>` to `RefineSession`. In `recompute_view()`:

1. Scan timeline for `ViewDirective::UngroupGroup` entries → `ungrouped_set: HashSet<String>`
2. For each installed group, scan timeline to find the most recent `SetInclude(Group { name })` op (if any) → determines `group_excluded` flag
3. For each installed group, scan timeline for individual `SetInclude(Package)` ops on group members that appear AFTER that group's most recent group-level op, AND whose resulting include state diverges from the group op's direction → `divergent_overrides: HashSet<String>` (per-group, not global)
   - A reaffirming op (e.g., `SetInclude(Package("make"), false)` after `SetInclude(Group, false)`) does NOT count — it matches the group's intent
   - An op on a shared member only counts as a divergent override for the group whose most recent group-level op it post-dates
4. Build `GroupEvalContext` per group and call `derive_group_state()`
5. Store results in `RenderContext { group_states }`
6. Cache alongside `cached_view`

The per-group timeline analysis is what makes `Excluded` and `MemberOverridden` precise. A global override set would mis-degrade groups that share members with other groups.

Add `pub fn render_context(&self) -> &RenderContext` accessor.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine render_context -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "feat(refine): build RenderContext during view computation"
```

---

### Task 13: Thread RenderContext through renderer signature

This task ONLY changes the function signature. No rendering behavior changes — those come in Phase 5. This ensures the phase is independently compilable and reviewable.

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs` (add `Option<&RenderContext>` parameter)
- Modify: `crates/refine/src/session.rs` (pass `Some(&render_ctx)` at call site)

- [ ] **Step 1: Update renderer signature**

Add `render_ctx: Option<&RenderContext>` as the last parameter to `render_containerfile_with_originals()`. The function ignores it for now.

Import `RenderContext` from `inspectah_core::types::group_render`. This works because both `crates/pipeline` and `crates/refine` depend on `crates/core`.

- [ ] **Step 2: Update all call sites**

In `crates/refine/src/session.rs`, pass `Some(&self.cached_render_context.as_ref().unwrap_or(&RenderContext::default()))` at the call site in `recompute_view()`. In test helpers and other callers, pass `None`.

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p inspectah-refine -- --nocapture && cargo test -p inspectah-pipeline -- --nocapture`
Expected: ALL PASS (no behavior change, just plumbing)

- [ ] **Step 4: Commit**

```bash
git add crates/pipeline/src/render/containerfile.rs crates/refine/src/session.rs
git commit -m "refactor(pipeline): thread RenderContext through containerfile renderer signature"
```

---

> **THORN CHECKPOINT 2:** Review Tasks 10–13 (autosave, renderability, RenderContext). Focus on: schema migration correctness, renderability contract edge cases, and whether the projection boundary stays clean.

---

## Phase 5: Containerfile Renderer

### Task 14: Add group install section rendering

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Test: `crates/pipeline/tests/` (new test file or extend existing)

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn test_containerfile_renders_group_install_section() {
    let snap = snapshot_with_two_groups();
    let ctx = render_context_all_renderable();
    let output = render_containerfile_with_originals(
        &snap, None, &HashMap::new(), Some(&ctx),
    );
    assert!(output.contains("# === Package Groups (2) ==="));
    assert!(output.contains("dnf group install -y"));
    assert!(output.contains("\"Container Management\""));
    assert!(output.contains("\"Development Tools\""));
}
```

- [ ] **Step 2: Implement group install section**

In `packages_section_lines()`, before the existing package emission:

1. Read `installed_groups` from snapshot
2. Read `RenderContext` to determine which groups are renderable
3. Collect renderable group names, sort alphabetically
4. Build group members set (all members of renderable groups) for exclusion from individual lines
5. Emit `# === Package Groups (N) ===` section with `RUN dnf group install -y \` lines
6. Filter the remaining `install_names` to exclude packages covered by renderable groups

- [ ] **Step 3: Run test**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/pipeline/src/render/containerfile.rs crates/pipeline/tests/
git commit -m "feat(pipeline): render dnf group install section in Containerfile"
```

---

### Task 15: Add comment annotations for ungrouped, degraded, and optional

**Files:**
- Modify: `crates/pipeline/src/render/containerfile.rs`
- Test: `crates/pipeline/tests/`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn test_containerfile_ungrouped_comment() {
    let snap = snapshot_with_groups();
    let ctx = render_context_with_ungrouped("Dev Tools");
    let output = render_containerfile_with_originals(
        &snap, None, &HashMap::new(), Some(&ctx),
    );
    assert!(output.contains("# Ungrouped from \"Dev Tools\""));
    assert!(!output.contains("dnf group install"));
}

#[test]
fn test_containerfile_degraded_comment() {
    let snap = snapshot_with_groups();
    let ctx = render_context_with_degraded("Dev Tools");
    let output = render_containerfile_with_originals(
        &snap, None, &HashMap::new(), Some(&ctx),
    );
    assert!(output.contains("# \"Dev Tools\" degraded"));
}

#[test]
fn test_containerfile_optional_provenance_comment() {
    let snap = snapshot_with_optional_members();
    let ctx = render_context_all_renderable();
    let output = render_containerfile_with_originals(
        &snap, None, &HashMap::new(), Some(&ctx),
    );
    assert!(output.contains("# Optional members"));
    assert!(output.contains("python3-pytest"));
}
```

- [ ] **Step 2: Implement comment annotations**

In the individual packages section, before the `RUN dnf install -y` block, emit comment headers:

1. Collect optional spillover packages (from `InstalledGroup.optional_installed` for ALL groups regardless of render state — optional members stay independent even when the parent group is excluded, per spec)
2. Collect ungrouped group members
3. Collect degraded group members
4. Emit comment blocks above the RUN statement

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Run full renderer test suite**

Run: `cargo test -p inspectah-pipeline -- --nocapture`
Expected: ALL PASS (no regressions)

- [ ] **Step 5: Commit**

```bash
git add crates/pipeline/src/render/containerfile.rs crates/pipeline/tests/
git commit -m "feat(pipeline): add provenance comments for ungrouped, degraded, optional packages"
```

---

### Task 16: Add excluded group renders nothing test

**Files:**
- Modify: `crates/pipeline/tests/` (test file)

- [ ] **Step 1: Write test**

```rust
#[test]
fn test_containerfile_excluded_group_emits_nothing() {
    let snap = snapshot_with_groups();
    let ctx = render_context_with_excluded("Dev Tools");
    let output = render_containerfile_with_originals(
        &snap, None, &HashMap::new(), Some(&ctx),
    );
    assert!(!output.contains("Dev Tools"));
    assert!(!output.contains("gcc")); // excluded member
}
```

- [ ] **Step 2: Verify the test passes**

The existing rendering logic should already handle this — excluded packages have `include: false` and are filtered out.

- [ ] **Step 3: Commit**

```bash
git add crates/pipeline/tests/
git commit -m "test(pipeline): verify excluded groups emit nothing in Containerfile"
```

---

### Task 16a: Preview/export parity proof

Preview and export must use the same `RenderContext` and produce identical Containerfile output for the same session state.

**Files:**
- Test: `crates/refine/src/session.rs` (inline tests)

- [ ] **Step 1: Write parity test**

```rust
#[test]
fn preview_and_export_produce_same_containerfile() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();

    let preview = session.view().containerfile_preview.clone();

    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    session.export_tarball(&export_path, session.view().generation).unwrap();
    let exported_containerfile = read_containerfile_from_tarball(&export_path);

    assert_eq!(preview, exported_containerfile,
        "preview and export Containerfile must match");
}
```

- [ ] **Step 2: Verify both code paths pass the same RenderContext**

Check that `export_tarball()` passes the same `RenderContext` to `render_containerfile_with_originals()` as `recompute_view()` does. If `export_tarball()` reconstructs its own RenderContext, verify it uses the same derivation logic.

- [ ] **Step 3: Run and verify**

Run: `cargo test -p inspectah-refine preview_and_export -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "test(refine): prove preview/export Containerfile parity with RenderContext"
```

---

### Task 16b: Overlap-precedence proof matrix

Explicit tests for every spec-required precedence case across both preview and export output.

**Files:**
- Test: `crates/refine/src/session.rs` (inline tests)
- Test: `crates/pipeline/tests/` (renderer tests)

- [ ] **Step 1: Write precedence tests**

```rust
// Session-level precedence (state derivation)
#[test]
fn precedence_renderable_wins_over_degraded_for_shared_member() {
    // Package in both renderable group A and degraded group B
    // → package stays in group A, not emitted as individual from B
}

#[test]
fn precedence_renderable_wins_over_excluded_for_shared_member() {
    // Package in renderable group A and excluded group B
    // → package stays in group A
}

#[test]
fn precedence_renderable_wins_over_ungrouped_for_shared_member() {
    // Package in renderable group A and ungrouped group B
    // → package stays in group A, not emitted as individual from B
}

#[test]
fn precedence_reproducible_wins_over_optional_spillover() {
    // Package is optional in group A and reproducible in group B
    // → appears inside group B, not as optional spillover
}

// Renderer-level precedence (no duplicates)
#[test]
fn no_duplicate_across_group_and_individual_sections() {
    // Package covered by renderable group must NOT also appear
    // in the individual packages RUN statement
}

#[test]
fn precedence_preview_matches_export_for_all_cases() {
    // Run each precedence case through both preview and export
    // and assert identical Containerfile output
}
```

- [ ] **Step 2: Run and verify**

Run: `cargo test -p inspectah-refine precedence -- --nocapture && cargo test -p inspectah-pipeline precedence -- --nocapture`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/session.rs crates/pipeline/tests/
git commit -m "test(refine,pipeline): overlap-precedence proof matrix for all spec cases"
```

---

> **THORN CHECKPOINT 3:** Review Tasks 14–16b (Containerfile renderer + parity + precedence). Focus on: output format matches spec, partitioning is correct, no duplicate packages between group and individual sections, preview/export parity, all precedence cases proven.

---

## Phase 6: Web API

### Task 17: Update /api/op to accept TimelineEntry

**Files:**
- Modify: `crates/web/src/handlers.rs`
- Test: `crates/web/tests/api_test.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn api_op_accepts_ungroup_directive() {
    let json = serde_json::json!({
        "kind": "View",
        "directive": "UngroupGroup",
        "group_name": "Container Management"
    });
    // Test that the handler accepts this shape and returns a valid view
}
```

- [ ] **Step 2: Update the handler**

Switch `/api/op` to accept `TimelineEntry` directly. No backward-compat envelope — frontend and backend land together.

```rust
pub async fn apply_op(...) -> Result<...> {
    let entry: TimelineEntry = serde_json::from_slice(&body)
        .map_err(|e| AppError(RefineError::BadRequest(format!("invalid: {e}"))))?;
    let mut session = state.session.lock().unwrap();
    match entry {
        TimelineEntry::Op(op) => session.apply(op).map_err(AppError)?,
        TimelineEntry::View(dir) => session.apply_directive(dir).map_err(AppError)?,
    }
    Ok(Json(serde_json::to_value(build_web_view(&session)).unwrap()))
}
```

- [ ] **Step 2a: Replace `applyOp` with `applyTimelineEntry` in client**

In `crates/web/ui/src/api/client.ts`:

1. Rename `applyOp` → `applyTimelineEntry` with new signature:

```typescript
export async function applyTimelineEntry(
  entry: TimelineEntry
): Promise<ViewResponse> {
  return post("/api/op", entry);
}

// Convenience wrapper for existing callers that still send bare ops
export async function applyOp(op: RefinementOp): Promise<ViewResponse> {
  return applyTimelineEntry({ kind: "Op", ...op });
}

// New: send a ViewDirective
export async function applyDirective(
  directive: ViewDirective
): Promise<ViewResponse> {
  return applyTimelineEntry({ kind: "View", ...directive });
}
```

2. Add `TimelineEntry` and `ViewDirective` to `types.ts`:

```typescript
export type ViewDirective =
  | { directive: "UngroupGroup"; group_name: string };

export type TimelineEntry =
  | { kind: "Op" } & RefinementOp
  | { kind: "View" } & ViewDirective;
```

3. Add `AnnotatedTimelineEntry` for `/api/ops` history responses. Uses **flat** shape (matching Rust `#[serde(flatten)]`):

```typescript
// Flat: TimelineEntry fields spread alongside `active`
// Matches Rust #[serde(flatten)] on the entry field
export type AnnotatedTimelineEntry =
  | { kind: "Op"; active: boolean } & RefinementOp
  | { kind: "View"; active: boolean } & ViewDirective;
```

This replaces `AnnotatedOp`. The Rust struct uses `#[serde(flatten)]` so the JSON is flat (e.g., `{"kind":"Op","op":"SetInclude","target":{...},"active":true}`), NOT nested (`{"entry":{...},"active":true}`).

- [ ] **Step 2b: Update all existing `applyOp` callers**

Search `crates/web/ui/src/` for `applyOp(` calls. Each must send `TimelineEntry` format. Existing callers that send `SetInclude` etc. use the convenience `applyOp` wrapper (which wraps in `{ kind: "Op" }`). New group callers use `applyDirective`.

- [ ] **Step 3: Run API tests**

Run: `cargo test -p inspectah-web -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/handlers.rs crates/web/tests/ crates/web/ui/src/api/client.ts crates/web/ui/src/api/types.ts
git commit -m "feat(web): accept TimelineEntry in /api/op, add applyTimelineEntry/applyDirective client helpers"
```

---

### Task 18: Add group data and PackageProvenance to ViewResponse

**Files:**
- Modify: `crates/web/src/web_types.rs` (add GroupInfo DTO, add PackageProvenance DTO, add provenance field to package DTO)
- Modify: `crates/web/src/adapter.rs` (populate group data AND per-package provenance for all three kinds: optional_spillover, ungrouped_member, degraded_member)
- Modify: `crates/web/ui/src/api/types.ts` (add TypeScript types for GroupInfo, PackageProvenance)
- Test: `crates/web/tests/api_test.rs` (verify provenance populated for all three cases)

- [ ] **Step 1: Define GroupInfo DTO**

```rust
// web_types.rs
#[derive(Serialize, Clone, Debug)]
pub struct GroupInfo {
    pub name: String,
    pub member_count: usize,
    pub locked_count: usize,
    pub optional_spillover_count: usize,
    pub render_state: String, // "renderable", "excluded", "ungrouped", "degraded"
    pub degradation_reason: Option<String>,
    pub members: Vec<GroupMemberInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct GroupMemberInfo {
    pub name: String,
    pub locked: bool,
    pub overlap_groups: Vec<String>,
}
```

Add `pub package_groups: Vec<GroupInfo>` to `ViewResponse`.

- [ ] **Step 2: Build GroupInfo in adapter**

In `build_web_view()`, read `session.render_context()` and `installed_groups` to populate `package_groups`.

- [ ] **Step 3: Add TypeScript types**

```typescript
export interface GroupMemberInfo {
  name: string;
  locked: boolean;
  overlap_groups: string[];
}

export interface GroupInfo {
  name: string;
  member_count: number;
  locked_count: number;
  optional_spillover_count: number;
  render_state: "renderable" | "excluded" | "ungrouped" | "degraded";
  degradation_reason: string | null;
  members: GroupMemberInfo[];
}
```

Add `package_groups: GroupInfo[]` to `ViewResponse`.

- [ ] **Step 4: Define PackageProvenance DTO (Rust + TypeScript)**

```rust
// web_types.rs — alongside GroupInfo
#[derive(Serialize, Clone, Debug)]
pub struct PackageProvenance {
    pub kind: String,    // "optional_spillover", "ungrouped_member", "degraded_member"
    pub group_name: String,
}
```

Add `pub provenance: Option<PackageProvenance>` to the existing package DTO that `ViewResponse` already carries (the struct that wraps `RefinedPackage` for the wire).

```typescript
// types.ts — alongside GroupInfo
export interface PackageProvenance {
  kind: "optional_spillover" | "ungrouped_member" | "degraded_member";
  group_name: string;
}
```

Add `provenance: PackageProvenance | null` to the existing `RefinedPackage` TypeScript interface.

- [ ] **Step 5: Populate provenance in adapter**

In `build_web_view()`, after building the package list, cross-reference each individual-zone package against `InstalledGroup` data and `RenderContext.group_states`:
- Package is in `optional_installed` of ANY group (regardless of render state) → `{ kind: "optional_spillover", group_name }`
- Package is a member of an `Ungrouped` group → `{ kind: "ungrouped_member", group_name }`
- Package is a member of a `Degraded` group → `{ kind: "degraded_member", group_name }`
- Package covered by a `Renderable` group → not in the individual zone (no provenance needed)
- Package not in any group → `provenance: None`

- [ ] **Step 6: Write API proof tests for provenance**

```rust
#[test]
fn api_view_populates_optional_spillover_provenance() {
    // Snapshot with group having optional_installed members
    // → view response packages include provenance for those members
}

#[test]
fn api_view_populates_ungrouped_member_provenance() {
    // Session with UngroupGroup applied
    // → former members have provenance { kind: "ungrouped_member" }
}

#[test]
fn api_view_populates_degraded_member_provenance() {
    // Session with individual override causing degradation
    // → members have provenance { kind: "degraded_member" }
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p inspectah-web -- --nocapture`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add crates/web/src/web_types.rs crates/web/src/adapter.rs crates/web/ui/src/api/types.ts crates/web/tests/
git commit -m "feat(web): add GroupInfo and PackageProvenance to ViewResponse with adapter population"
```

---

### Task 19: Update /api/changes for directive dirty-state

**Files:**
- Modify: `crates/refine/src/session.rs` (changes_summary)
- Test: `crates/refine/src/session.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn ungroup_sets_dirty_state() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    assert!(!session.changes_summary().is_dirty);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();
    assert!(session.changes_summary().is_dirty);
}
```

- [ ] **Step 2: Update is_dirty computation**

In `changes_summary()`, include `TimelineEntry::View` entries in the dirty check. A session with any timeline entries (ops or directives) that differ from the original state is dirty.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-refine ungroup_sets_dirty -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "feat(refine): UngroupGroup sets is_dirty in changes_summary"
```

---

### Task 20: Update /api/ops to return TimelineEntry history

**Files:**
- Modify: `crates/web/src/handlers.rs` (ops endpoint)
- Test: `crates/web/tests/api_test.rs`

- [ ] **Step 1: Update ops handler**

The existing `/api/ops` endpoint returns `Vec<AnnotatedOp>`. Replace with `Vec<AnnotatedTimelineEntry>` — each entry wraps a `TimelineEntry` with an `active: bool` flag (based on cursor position).

```rust
#[derive(Serialize)]
pub struct AnnotatedTimelineEntry {
    #[serde(flatten)]
    pub entry: TimelineEntry,
    pub active: bool,
}
```

- [ ] **Step 2: Migrate frontend history consumers**

Update all consumers of `/api/ops` (undo/redo UI, history panel if any) to use `AnnotatedTimelineEntry` instead of `AnnotatedOp`. The `AnnotatedTimelineEntry` TypeScript type was already defined in Task 17 Step 2a. Remove or deprecate `AnnotatedOp` from `types.ts`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-web -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/src/handlers.rs crates/web/tests/ crates/web/ui/src/api/types.ts
git commit -m "feat(web): return TimelineEntry history from /api/ops"
```

---

> After Phase 6 (Tasks 17–20): no standalone checkpoint. API changes are proven by Phase 7 integration. Thorn reviews API contract as part of Checkpoint 4 alongside UI.

---

## Phase 7: Web UI

### Task 21: Add GroupRow component

**Files:**
- Create: `crates/web/ui/src/components/GroupRow.tsx`
- Test: `crates/web/ui/src/components/__tests__/GroupRow.test.tsx`

- [ ] **Step 1: Write component test**

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { GroupRow } from "../GroupRow";

const mockGroup: GroupInfo = {
  name: "Container Management",
  member_count: 12,
  locked_count: 0,
  optional_spillover_count: 0,
  render_state: "renderable",
  degradation_reason: null,
  members: [
    { name: "podman", locked: false, overlap_groups: [] },
    { name: "buildah", locked: false, overlap_groups: [] },
  ],
};

test("renders group name and package count", () => {
  render(<GroupRow group={mockGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  expect(screen.getByText("Container Management")).toBeInTheDocument();
  expect(screen.getByText(/12 packages/)).toBeInTheDocument();
});

test("expand/collapse shows member list", () => {
  render(<GroupRow group={mockGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: /expand/i }));
  expect(screen.getByText("podman")).toBeInTheDocument();
  expect(screen.getByText("buildah")).toBeInTheDocument();
});

test("ungroup button calls onUngroup", () => {
  const onUngroup = jest.fn();
  render(<GroupRow group={mockGroup} onToggle={jest.fn()} onUngroup={onUngroup} />);
  fireEvent.click(screen.getByText("ungroup"));
  expect(onUngroup).toHaveBeenCalledWith("Container Management");
});
```

- [ ] **Step 2: Implement GroupRow component**

Build `GroupRow.tsx` with:
- Collapsed state: chevron, group name (semibold), "N packages", locked count if present, ungroup button, toggle
- Expanded state: alphabetical member list, locked indicator, overlap annotation
- Left purple border accent
- Truncation: first 5 + "N more" link

- [ ] **Step 3: Run tests**

Run: `cd crates/web/ui && npm test -- --grep GroupRow`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/GroupRow.tsx crates/web/ui/src/components/__tests__/GroupRow.test.tsx
git commit -m "feat(web-ui): add GroupRow component with expand, toggle, and ungroup"
```

---

### Task 22: Update PackageList to render groups zone

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write test**

```typescript
test("renders groups zone above individual packages zone", () => {
  const { container } = render(
    <PackageList
      mode="single"
      packages={mockPackages}
      repoGroups={[]}
      packageGroups={mockGroups}
      onToggle={jest.fn()}
    />
  );
  const zones = container.querySelectorAll("[data-zone]");
  expect(zones[0].getAttribute("data-zone")).toBe("groups");
  expect(zones[1].getAttribute("data-zone")).toBe("individual");
});

test("summary bar shows group and individual counts", () => {
  render(
    <PackageList
      mode="single"
      packages={mockPackages}
      repoGroups={[]}
      packageGroups={mockGroups}
      onToggle={jest.fn()}
    />
  );
  expect(screen.getByText(/2 groups/)).toBeInTheDocument();
  expect(screen.getByText(/14 individual/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Implement zones in PackageList**

Add `packageGroups: GroupInfo[]` prop to `PackageListProps`. Update rendering:
1. Summary bar with group + individual + optional counts
2. Groups zone with `GroupRow` for each renderable/excluded/degraded group
3. "Individual Packages" divider
4. Individual packages zone (existing package rows, minus those covered by renderable groups)
5. Optional spillover packages with provenance badge

- [ ] **Step 3: Run tests**

Run: `cd crates/web/ui && npm test -- --grep PackageList`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/PackageList.tsx crates/web/ui/src/components/__tests__/PackageList.test.tsx
git commit -m "feat(web-ui): render groups zone and individual zone in PackageList"
```

---

### Task 23: Wire ungroup action through API

**Files:**
- Modify: `crates/web/ui/src/api/client.ts`
- Modify: `crates/web/ui/src/components/PackageList.tsx`

- [ ] **Step 1: Add ungroupGroup API call**

```typescript
// client.ts — uses applyDirective from Task 17
export async function ungroupGroup(groupName: string): Promise<ViewResponse> {
  return applyDirective({
    directive: "UngroupGroup",
    group_name: groupName,
  });
}
```

- [ ] **Step 2: Wire into PackageList**

Pass an `onUngroup` handler from the app shell through PackageList to GroupRow that calls `ungroupGroup()` and updates view state.

- [ ] **Step 3: Add toast notification**

After successful ungroup, show toast: "Group ungrouped into N packages. Ctrl+Z to undo." Use the existing toast/notification pattern from the codebase.

- [ ] **Step 4: Run tests**

Run: `cd crates/web/ui && npm test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/web/ui/src/api/client.ts crates/web/ui/src/components/
git commit -m "feat(web-ui): wire ungroup action with toast notification"
```

---

### Task 24: Add optional spillover provenance badge

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx` (or new SpilloverBadge component)
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write test**

```typescript
test("optional spillover packages show provenance badge", () => {
  render(
    <PackageList
      mode="single"
      packages={mockPackagesWithOptionalSpillover}
      repoGroups={[]}
      packageGroups={mockGroups}
      onToggle={jest.fn()}
    />
  );
  expect(screen.getByText(/optional from "Development Tools"/)).toBeInTheDocument();
});
```

- [ ] **Step 2: Implement provenance badge (UI only — consumes Task 18's contract)**

Task 18 owns the `PackageProvenance` DTO definition, adapter population, and API proof for all three provenance kinds. This task ONLY renders the badge in the UI by reading the `provenance` field that Task 18 already populates on each package.

In the individual packages zone, for any package where `provenance` is non-null, render a styled chip below the package name based on `provenance.kind`:
- `optional_spillover` → `"optional from \"Dev Tools\""`
- `ungrouped_member` → `"ungrouped from \"Dev Tools\""`
- `degraded_member` → `"from \"Dev Tools\" (rendered individually)"`

No DTO definitions, no adapter logic, no `web_types.rs` changes — those all belong to Task 18.

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/
git commit -m "feat(web-ui): add provenance badge for optional spillover packages"
```

---

### Task 25: Wire search to auto-expand groups

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Modify: `crates/web/ui/src/components/GlobalSearch.tsx` (if needed)
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write test**

```typescript
test("searching for member package auto-expands the group", () => {
  render(
    <PackageList
      mode="single"
      packages={mockPackages}
      repoGroups={[]}
      packageGroups={mockGroups}
      onToggle={jest.fn()}
      searchQuery="podman"
    />
  );
  expect(screen.getByText("podman")).toBeVisible();
});

test("searching for group name highlights the group row", () => {
  render(
    <PackageList ... searchQuery="Container" />
  );
  const groupRow = screen.getByTestId("group-row-Container Management");
  expect(groupRow).toHaveAttribute("data-search-match", "true");
});

test("searching highlights optional spillover packages", () => {
  render(
    <PackageList ... searchQuery="python3-pytest"
      packageGroups={mockGroupsWithOptional} />
  );
  const spilloverRow = screen.getByTestId("package-row-python3-pytest");
  expect(spilloverRow).toBeVisible();
  expect(spilloverRow).toHaveAttribute("data-search-match", "true");
});

test("filtered summary shows unique-package counts during search", () => {
  render(
    <PackageList ... searchQuery="podman" />
  );
  expect(screen.getByText(/1 group \(match inside\)/)).toBeInTheDocument();
});

test("clearing search re-collapses auto-expanded groups", () => {
  const { rerender } = render(
    <PackageList ... searchQuery="podman" />
  );
  rerender(<PackageList ... searchQuery="" />);
  // Group should be collapsed again (podman not visible in member list)
});
```

- [ ] **Step 2: Implement search-driven expansion**

Track `autoExpandedGroups: Set<string>` in component state. When `searchQuery` matches a member of a collapsed group:
1. Add group to `autoExpandedGroups`
2. Disable truncation for auto-expanded groups
3. Highlight matching member

On search clear: remove all entries from `autoExpandedGroups`. Groups the user manually expanded stay expanded.

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/
git commit -m "feat(web-ui): search auto-expands groups with matching members"
```

---

### Task 26: Add keyboard and ARIA for group rows

**Files:**
- Modify: `crates/web/ui/src/components/GroupRow.tsx`
- Test: `crates/web/ui/src/components/__tests__/GroupRow.test.tsx`

- [ ] **Step 1: Write keyboard tests**

```typescript
test("tab order: chevron → ungroup → toggle", () => {
  render(<GroupRow group={mockGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  const chevron = screen.getByRole("button", { name: /expand/i });
  const ungroup = screen.getByText("ungroup");
  const toggle = screen.getByRole("checkbox");
  chevron.focus();
  fireEvent.keyDown(document, { key: "Tab" });
  expect(document.activeElement).toBe(ungroup);
  fireEvent.keyDown(document, { key: "Tab" });
  expect(document.activeElement).toBe(toggle);
});

test("Enter on group row expands", () => {
  render(<GroupRow group={mockGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  const row = screen.getByRole("group");
  fireEvent.keyDown(row, { key: "Enter" });
  expect(screen.getByText("podman")).toBeVisible();
});
```

- [ ] **Step 2: Implement ARIA**

- Add `role="group"` to group row container with `aria-label="Container Management, 12 packages"` (matches Task 27c semantic container model). The container itself is NOT a tab stop (`tabIndex` omitted) — tab order goes chevron → ungroup → toggle per the spec. For **tab traversal**, focus follows the natural child order. For **programmatic focus** (search landing on group-name match, undo/regroup focus restoration), the container gets a temporary `tabIndex={-1}` and receives `focus()` directly — this makes the group row itself the focus target for programmatic placement without inserting it into the tab order. This matches the spec: undo → focus on restored group row; group-name search → focus on group row.
- Expanded member list uses `role="list"` disclosure region; members are `role="listitem"`
- Add `aria-expanded` to chevron button
- Add `aria-label` to ungroup button: "Ungroup Container Management"
- Add `aria-live="polite"` region for toasts
- Focus restoration: after ungroup, focus the first resulting individual package row

**Note on tab traversal testing:** The `fireEvent.keyDown(Tab)` assertions in Step 1 verify logical focus order as a unit-test proxy. Real browser tab traversal (native focus ring, skip-link behavior) is proven in Task 27's Playwright e2e tests where `page.keyboard.press("Tab")` exercises the actual browser focus engine.

- [ ] **Step 3: Run tests**

Run: `cd crates/web/ui && npm test -- --grep GroupRow`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/GroupRow.tsx crates/web/ui/src/components/__tests__/GroupRow.test.tsx
git commit -m "feat(web-ui): keyboard navigation and ARIA for group rows"
```

---

### Task 27: Add Playwright e2e tests for group rendering

**Files:**
- Create: `crates/web/ui/e2e/groups.spec.ts`

- [ ] **Step 1: Write e2e tests**

```typescript
import { test, expect } from "@playwright/test";
import { mockApi } from "./helpers/mock-api";

test.describe("Group rendering", () => {
  test("group row expand/collapse", async ({ page }) => {
    await mockApi(page, { withGroups: true });
    await page.goto("/");
    await page.click('[data-testid="group-row-Container Management"] button[aria-label*="expand"]');
    await expect(page.locator("text=podman")).toBeVisible();
    await page.click('[data-testid="group-row-Container Management"] button[aria-label*="collapse"]');
    await expect(page.locator("text=podman")).not.toBeVisible();
  });

  test("ungroup converts to individual rows", async ({ page }) => {
    await mockApi(page, { withGroups: true });
    await page.goto("/");
    await page.click("text=ungroup");
    await expect(page.locator('[data-testid="group-row-Container Management"]')).not.toBeVisible();
    await expect(page.locator('[data-testid="package-row-podman"]')).toBeVisible();
  });

  test("search highlights group member", async ({ page }) => {
    await mockApi(page, { withGroups: true });
    await page.goto("/");
    await page.keyboard.press("Control+k");
    await page.fill('[data-testid="global-search-input"]', "podman");
    await expect(page.locator("text=podman")).toBeVisible();
  });

  test("optional spillover shows provenance", async ({ page }) => {
    await mockApi(page, { withGroups: true, withOptionalSpillover: true });
    await page.goto("/");
    await expect(page.locator('text=optional from "Development Tools"')).toBeVisible();
  });

  test("excluded-group optional spillover remains visible and independent", async ({ page }) => {
    await mockApi(page, { withGroups: true, withOptionalSpillover: true });
    await page.goto("/");
    // Exclude the parent group
    await page.click('[data-testid="group-row-Development Tools"] input[type="checkbox"]');
    // Optional spillover package must still be visible and independently toggleable
    const spilloverRow = page.locator('[data-testid="package-row-python3-pytest"]');
    await expect(spilloverRow).toBeVisible();
    await expect(spilloverRow.locator('input[type="checkbox"]')).toBeChecked();
    await expect(page.locator('text=optional from "Development Tools"')).toBeVisible();
    // Prove independent toggleability: uncheck the spillover package
    await spilloverRow.locator('input[type="checkbox"]').click();
    await expect(spilloverRow.locator('input[type="checkbox"]')).not.toBeChecked();
    // Re-check to confirm it's fully independent of the excluded parent
    await spilloverRow.locator('input[type="checkbox"]').click();
    await expect(spilloverRow.locator('input[type="checkbox"]')).toBeChecked();
  });

  test("native tab traversal follows chevron → ungroup → toggle order", async ({ page }) => {
    await mockApi(page, { withGroups: true });
    await page.goto("/");
    // Tab into the first group row
    await page.keyboard.press("Tab"); // skip to first focusable in groups zone
    // First stop: chevron button
    const chevron = page.locator('[data-testid="group-row-Container Management"] button[aria-label*="expand"]');
    await expect(chevron).toBeFocused();
    // Second stop: ungroup button
    await page.keyboard.press("Tab");
    const ungroup = page.locator('[data-testid="group-row-Container Management"] button:has-text("ungroup")');
    await expect(ungroup).toBeFocused();
    // Third stop: toggle checkbox
    await page.keyboard.press("Tab");
    const toggle = page.locator('[data-testid="group-row-Container Management"] input[type="checkbox"]');
    await expect(toggle).toBeFocused();
  });
});
```

- [ ] **Step 2: Add mock API fixtures**

Extend `helpers/mock-api.ts` with group-aware fixtures.

- [ ] **Step 3: Run e2e tests**

Run: `cd crates/web/ui && npx playwright test e2e/groups.spec.ts`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/e2e/groups.spec.ts crates/web/ui/e2e/helpers/
git commit -m "test(e2e): add Playwright tests for group rendering"
```

---

### Task 27a: Undo focus restoration

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write test**

```typescript
test("after ungroup, focus moves to first individual package from group", () => {
  // ungroup "Container Management" → focus lands on the podman row
});

test("after undo of ungroup, focus moves to restored group row", () => {
  // undo → focus lands on the Container Management group row
});

test("after scroll-to optional spillover, focus lands on first spillover package", () => {
  // click the "2 optional still included" link → focus on first spillover row
});
```

- [ ] **Step 2: Implement focus restoration**

Track `pendingFocusTarget` in component state. After ungroup API call resolves, set focus target to first member package. After undo resolves, set focus target to restored group row. Use `useEffect` to apply focus after re-render.

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/
git commit -m "feat(web-ui): undo focus restoration for group actions"
```

---

### Task 27b: Locked, optional, degraded feedback and disabled controls

**Files:**
- Modify: `crates/web/ui/src/components/GroupRow.tsx`
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/GroupRow.test.tsx`

- [ ] **Step 1: Write tests**

```typescript
test("degraded group row has disabled toggle and ungroup button", () => {
  const degradedGroup = { ...mockGroup, render_state: "degraded" as const };
  render(<GroupRow group={degradedGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  expect(screen.getByRole("checkbox")).toBeDisabled();
  expect(screen.getByText("ungroup")).toBeDisabled();
});

test("excluded group with optional leftovers shows persistent count", () => {
  const excludedGroup = {
    ...mockGroup,
    render_state: "excluded" as const,
    optional_spillover_count: 2,
  };
  render(<GroupRow group={excludedGroup} onToggle={jest.fn()} onUngroup={jest.fn()} />);
  expect(screen.getByText(/2 optional still included/)).toBeInTheDocument();
});

test("locked member toast announces via aria-live", () => {
  // Toggle a group with locked members → toast + aria-live polite
});
```

- [ ] **Step 2: Implement**

- Degraded groups: toggle and ungroup buttons get `disabled` attribute, row gets dimmed styling and "rendered individually" subtitle
- Excluded groups with optional spillover: persistent "N optional still included" text with clickable scroll-to link
- Locked member feedback: toast with `aria-live="polite"` region
- Optional orphan feedback: toast with `aria-live="polite"` region
- Degradation announcement: toast with `aria-live="polite"` region

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/
git commit -m "feat(web-ui): locked/optional/degraded feedback, disabled controls, ARIA announcements"
```

---

### Task 27c: Search focus, count, and re-collapse behavior

**Files:**
- Modify: `crates/web/ui/src/components/PackageList.tsx`
- Test: `crates/web/ui/src/components/__tests__/PackageList.test.tsx`

- [ ] **Step 1: Write tests**

```typescript
test("search for member name lands focus on matching member row", () => {
  render(<PackageList ... searchQuery="podman" />);
  // Focus should be on the podman member row inside the auto-expanded group
});

test("search for group name lands focus on the group row itself", () => {
  render(<PackageList ... searchQuery="Container Management" />);
  // Focus should be on the group row, NOT auto-expanded
  const groupRow = screen.getByTestId("group-row-Container Management");
  expect(document.activeElement).toBe(groupRow);
  expect(groupRow).toHaveAttribute("data-search-match", "true");
});

test("arrow keys navigate between search matches", () => {
  render(<PackageList ... searchQuery="pod" />);
  // Down arrow moves to next match
});

test("summary bar shows unique package count, not visible row count", () => {
  // Package in two groups counts once
  render(<PackageList ... packageGroups={overlappingGroups} />);
  expect(screen.getByText(/24 packages/)).toBeInTheDocument(); // unique count
});

test("auto-expanded groups re-collapse on search clear, manual stays open", () => {
  // Manually expand group A, search triggers auto-expand of group B
  // Clear search → B re-collapses, A stays expanded
});

test("excluded vs ungrouped vs degraded rows render differently", () => {
  render(<PackageList ... packageGroups={mixedStateGroups} />);
  // Excluded: visible row, toggle off, ungroup enabled
  // Ungrouped: no row in groups zone, members in individual zone
  // Degraded: visible row, dimmed, controls disabled
});
```

- [ ] **Step 2: Implement**

- Search focus: member-name match → focus on member row inside auto-expanded group. Group-name match → focus on group row itself (do NOT auto-expand).
- Group row semantic container: `role="group"` with `aria-label="Container Management, 12 packages"`. Member rows inside are `role="listitem"` within a `role="list"` disclosure region.
- Arrow key navigation: `↑`/`↓` moves between search matches across groups and individual zone
- Summary counting: compute unique packages across all renderable groups (deduplicate overlaps)
- Re-collapse: track `autoExpandedGroups` vs `userExpandedGroups` separately
- State-specific rendering: check `render_state` on each GroupInfo to determine row behavior

- [ ] **Step 3: Run tests**

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/web/ui/src/components/
git commit -m "feat(web-ui): search focus/count behavior, state-specific row rendering"
```

---

> **THORN CHECKPOINT 4:** Review Tasks 17–27c (web API + UI). Focus on: TimelineEntry cutover coherence across request/history/client helpers, component structure, accessibility compliance, search interaction correctness (group-name/member/optional-spillover highlighting, filtered counts), focus restoration, feedback behaviors, state-specific rendering, test coverage against approved spec UX contract.

---

## Phase 8: Audit Report & Export Guard

### Task 28: Add overlap annotations to audit report

**Files:**
- Modify: `crates/pipeline/src/render/audit.rs`
- Test: `crates/pipeline/tests/`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn audit_report_flags_overlapping_group_members() {
    let snap = snapshot_with_overlapping_groups();
    let output = render_audit_report(&snap);
    assert!(output.contains("vim-enhanced appears in both"));
    assert!(output.contains("DNF handles this correctly"));
}
```

- [ ] **Step 2: Implement overlap annotations**

In the audit report's package section, after listing packages, check `installed_groups` for any package that appears in multiple groups. Emit a note: `"<package> appears in both '<Group A>' and '<Group B>' — DNF handles this correctly, no action needed."`

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-pipeline audit -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/pipeline/src/render/audit.rs crates/pipeline/tests/
git commit -m "feat(pipeline): add overlap annotations to audit report"
```

---

### Task 29: Add export guard — RenderContext not in snapshot

**Files:**
- Modify: `crates/refine/src/session.rs` (export_tarball test)
- Test: `crates/refine/src/session.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn export_snapshot_does_not_contain_render_context() {
    let snap = test_snapshot_with_groups();
    let mut session = RefineSession::new(snap);
    session.apply_directive(ViewDirective::UngroupGroup {
        group_name: "Container Management".into(),
    }).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let export_path = dir.path().join("export.tar.gz");
    session.export_tarball(&export_path, session.view().generation).unwrap();

    // Read the exported snapshot JSON from the tarball
    let exported_json = read_snapshot_from_tarball(&export_path);
    assert!(!exported_json.contains("ungrouped"));
    assert!(!exported_json.contains("group_states"));
    assert!(!exported_json.contains("render_context"));
}
```

- [ ] **Step 2: Verify test passes**

The export path serializes `InspectionSnapshot` which does not have `RenderContext` fields. This test is a regression guard.

Run: `cargo test -p inspectah-refine export_snapshot_does_not_contain -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/refine/src/session.rs
git commit -m "test(refine): guard that export snapshot never contains RenderContext"
```

---

> **Note on collector/parser:** The `dnf group info` parser update (splitting mandatory/default/conditional → `members`, installed optional → `optional_installed`) is the Anaconda gap classifier spec's responsibility. This plan's Task 1 amends the struct definition; the classifier implementation must produce the correct split. If implementing both specs in sequence, verify the parser before starting Phase 2.

---

## Summary

| Phase | Tasks | Owner | Focus |
|-------|-------|-------|-------|
| 1 — Data Model | 1–4 | Tang | Types, serde, wire compat |
| 2 — Session | 5–9 | Tang | Timeline, fan-out, undo |
| 3 — Autosave | 10 | Tang | Schema v3, v2→v3 migration |
| 4 — Renderability | 11–13 | Tang | Full state derivation, RenderContext |
| 5 — Renderer | 14–16b | Tang | Containerfile output, parity, precedence proof |
| 6 — Web API | 17–20 | Tang + Kit | Endpoint updates, TypeScript types |
| 7 — Web UI | 21–27c | Kit | Components, search, a11y, feedback, focus |
| 8 — Audit/Export | 28–29 | Tang | Overlap report, export guard |

**Thorn checkpoints (4, aligned to risk boundaries):**
1. After Phase 2 — state machine / session contract closure
2. After Phase 4 — renderability / RenderContext closure
3. After Phase 5 — renderer / preview-export parity closure
4. After Phase 7 — UI / a11y / search proof closure

**Total:** 34 tasks (29 original + 16a, 16b, 27a, 27b, 27c), 4 Thorn checkpoints. Tang owns Phases 1–6 + 8 (Rust). Kit owns Phase 7 (frontend). Tasks 17–20 are shared (Tang: Rust handlers, Kit: TypeScript types + client helpers).
