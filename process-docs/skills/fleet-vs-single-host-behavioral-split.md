---
name: fleet-vs-single-host-behavioral-split
description: Fleet (merged) and single-host snapshots follow different rules for leaf filtering, redaction state, and Containerfile generation. Code that ignores this distinction silently produces wrong output.
---

# Fleet vs. Single-Host Behavioral Split

inspectah operates in two modes determined at snapshot load time by
`fleet_meta` presence in the `InspectionSnapshot`. `RefineMode::Fleet`
vs `RefineMode::SingleHost` is set in `crates/refine/src/session.rs`.
Code that does not check the mode produces silently wrong results.

## Key Behavioral Differences

### 1. Leaf filtering is single-host only (by design)

Leaf-only package filtering (showing only user-installed packages, hiding
auto-dependencies) applies only to single-host snapshots. Fleet snapshots
skip leaf filtering deliberately — fleet mode uses prevalence-based
intersection (strict consensus) to filter packages instead. Per-host leaf
data still exists in each constituent snapshot, but the refine view guard
at `session.rs:1912` disables the leaf filter when `is_fleet_snapshot`.

Guard: `pkg.fleet.is_some()` disables leaf-only filtering for
Containerfile generation. The refine session skips leaf filtering when
`refine_mode` is `Fleet`.

### 2. Redaction state does not propagate through fleet merge

Fleet merges set `merged.redaction_state = None` (per-host state is
dropped). But boolean flags propagate with `any()` semantics:
`sensitive_snapshot`, `preserved_credentials`, `preserved_ssh_keys`,
`preserved_subscription`. Check these booleans, not `redaction_state`,
on fleet snapshots. See `crates/core/src/fleet/mod.rs` line ~125.

### 3. Containerfile rendering diverges

Single-host: `RUN dnf install -y` uses leaf-only filtered packages.
Fleet: Uses the full package set (no leaf filtering) and preserves
non-leaf manual follow-up comments.

The renderer checks `pkg.fleet.is_some()` to decide which path to take.

## Why This Matters

The most common mistake is writing code that works for single-host
snapshots and silently does the wrong thing for fleet, or vice versa.
Specific failure modes:

- **Adding a new snapshot boolean** without updating the fleet merge
  propagation in `crates/core/src/fleet/mod.rs`. The new field
  defaults to `false` in the merged snapshot, hiding host-level truth.
- **Leaf-filtering fleet data** causes migration work to disappear from
  the refine view and Containerfile.
- **Checking `redaction_state` on a fleet snapshot** always returns
  `None`, even if constituent hosts had redaction applied.

## Evidence

The preserve flag consolidation review (2026-06-08) identified that the
spec did not adequately address fleet merge propagation for new
`--no-redaction` behavior. The leaf filter fix (2026-05-17) required
explicit fleet escape hatches in both refine and pipeline rendering to
prevent merged data from being incorrectly leaf-filtered.

## See Also

- `crates/core/src/fleet/mod.rs` -- `merge_snapshots()` propagation
- `crates/refine/src/session.rs` -- `RefineMode` detection
- `crates/pipeline/src/render/containerfile.rs` -- fleet guard
- `package-identity-is-name-dot-arch.md` -- related identity issue
