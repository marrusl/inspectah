---
name: aggregate-vs-single-host-behavioral-split
description: Aggregate (merged) and single-host snapshots follow different rules for leaf filtering, redaction state, and Containerfile generation. Code that ignores this distinction silently produces wrong output.
---

# Aggregate vs. Single-Host Behavioral Split

inspectah operates in two modes determined at snapshot load time by
`aggregate_meta` presence in the `InspectionSnapshot`. `RefineMode::Aggregate`
vs `RefineMode::SingleHost` is set in `crates/refine/src/session.rs`.
Code that does not check the mode produces silently wrong results.

## Key Behavioral Differences

### 1. Leaf filtering is single-host only (by design)

Leaf-only package filtering (showing only user-installed packages, hiding
auto-dependencies) applies only to single-host snapshots. Aggregate snapshots
skip leaf filtering deliberately — aggregate mode uses prevalence-based
intersection (strict consensus) to filter packages instead. Per-host leaf
data still exists in each constituent snapshot, but the refine view guard
at `session.rs:1912` disables the leaf filter when `is_aggregate_snapshot`.

Guard: `pkg.aggregate.is_some()` disables leaf-only filtering for
Containerfile generation. The refine session skips leaf filtering when
`refine_mode` is `Aggregate`.

### 2. Redaction state does not propagate through aggregate merge

Aggregate merges set `merged.redaction_state = None` (per-host state is
dropped). But boolean flags propagate with `any()` semantics:
`sensitive_snapshot`, `preserved_credentials`, `preserved_ssh_keys`,
`preserved_subscription`. Check these booleans, not `redaction_state`,
on aggregate snapshots. See `crates/core/src/aggregate/mod.rs` line ~125.

### 3. Containerfile rendering diverges

Single-host: `RUN dnf install -y` uses leaf-only filtered packages.
Aggregate: Uses the full package set (no leaf filtering) and preserves
non-leaf manual follow-up comments.

The renderer checks `pkg.aggregate.is_some()` to decide which path to take.

### 4. Cross-inspector invariants established at collection do not survive the merge

Anything one inspector pass reconciles across sections holds per host and
**stops holding in the merged snapshot**. `merge_snapshots()` calls each
section merger independently (`crates/core/src/aggregate/mod.rs:240-255`);
no pass reconciles a config entry against the kernel, service, or container
section afterwards. Three properties combine to break the invariant:

- Config variant identity hashes **content**, not disposition
  (`crates/core/src/aggregate/merge.rs:119-125`). Two hosts with byte-identical
  files land in one variant group however they were dispositioned.
- A single-content group clones the **lowest-host-index** entry and calls
  `set_include(true)` on it (`merge.rs:622-635`). `FindingKind::with_include`
  preserves `Inventory` and `Advisory` but rewrites `Actionable`
  (`crates/core/src/types/finding.rs:57-65`), so the merged disposition is
  whichever kind the first host happened to contribute.
- The other host's section entry survives in full alongside it, including
  fields the renderer keys off.

Worked example: System Tuning, a section that exists in both modes with
mode-specific content. The sysctl explanation (effective values, runtime
drift, vendor files replaced) is true of one host's files and one host's
kernel. An earlier per-key sysctl merge picked one host's view as the
fleet's whole-struct winner, so the aggregate presented one host's
explanation as the fleet's. Now `merge_kernelboot_sections` sets
`kernel_boot.sysctl` to `None` (`crates/core/src/aggregate/merge.rs`),
and `render::sysctl_report::sysctl_report` checks `aggregate_meta`
**before** it looks at the section, returning `SysctlReport::Aggregate`.
The audit, the HTML report and the refine `SysctlPanel` all format that
one result, so on an aggregate each says the one fleet line
(`AGGREGATE_LINE`) and never "unavailable". The section itself stays
reachable in both modes: single-host refine renders the tuned rows and
the panel through `SystemTuningSection`, aggregate refine renders the
aggregate tuned rows and the same panel (`AggregateApp.tsx`). The sysctl
files themselves need nothing from the kernel section to merge: they are
config entries, merged by path with a variant per differing content.

The pattern: when a section's content is a per-host fact, do not merge
it into a winner. Drop it in the merge and branch on the mode once, in
the model every surface reads, before testing the section for absence;
otherwise an aggregate reads as a host whose collection failed.

Before relying on a cross-section invariant in a renderer, ask whether it was
established in `finalize_cross_inspector` (collection boundary, per host) or
after the merge. Only the latter holds for aggregate output.

## Why This Matters

The most common mistake is writing code that works for single-host
snapshots and silently does the wrong thing for aggregate, or vice versa.
Specific failure modes:

- **Adding a new snapshot boolean** without updating the aggregate merge
  propagation in `crates/core/src/aggregate/mod.rs`. The new field
  defaults to `false` in the merged snapshot, hiding host-level truth.
- **Leaf-filtering aggregate data** causes migration work to disappear from
  the refine view and Containerfile.
- **Checking `redaction_state` on a aggregate snapshot** always returns
  `None`, even if constituent hosts had redaction applied.
- **Trusting a collection-time cross-section invariant in a renderer.** It
  holds per host and does not survive independent section merges.

## Evidence

The preserve flag consolidation review (2026-06-08) identified that the
spec did not adequately address aggregate merge propagation for new
`--no-redaction` behavior. The leaf filter fix (2026-05-17) required
explicit aggregate escape hatches in both refine and pipeline rendering to
prevent merged data from being incorrectly leaf-filtered.

## See Also

- `crates/core/src/aggregate/mod.rs` -- `merge_snapshots()` propagation
- `crates/refine/src/session.rs` -- `RefineMode` detection
- `crates/pipeline/src/render/containerfile.rs` -- aggregate guard
- `package-identity-is-name-dot-arch.md` -- related identity issue
