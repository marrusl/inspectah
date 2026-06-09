# Feature: Fleet leaf-only package aggregation

## What

Filter fleet `packages_added` to leaf packages at aggregation time in
`merge_rpm_sections`. The merged fleet snapshot should only contain
packages classified as leaf (user-installed) on every authoritative
host. Auto packages are excluded from `packages_added` and remain
accessible via `leaf_dep_tree` for drill-down.

This is a data model change, not a UI change. Both web refine and TUI
get the correct view for free — no per-frontend leaf filtering logic.

## Why

The current `first_host_option` approach picks one arbitrary host's
leaf data (first alphabetically by hostname) and passes it through.
This creates two problems:

1. **Fragile selection.** The fleet Containerfile's `dnf install` line
   depends on which host sorts first. Different hostnames → different
   output for the same fleet.
2. **Wrong abstraction level.** Fleet `packages_added` contains raw
   state (every package on every host) rather than intent (what the
   user installed). Config files already filter to relevant items — the
   user doesn't see every config file, just the ones that matter.
   Packages should work the same way.

**Concrete example:** If `git` + 64 deps are on every host, the
fleet's `packages_added` contains only `git`. The Containerfile gets
`dnf install git`. The refine view shows `git`. Auto deps like
`perl-libs` are accessible via the dep tree but don't appear as
standalone triage items.

If `htop` is leaf on 8/10 hosts but auto on 2, it falls out of the
intersection — the conservative, correct behavior for fleet consensus.

## Data model: the full leaf triplet

The leaf classification has three coupled fields. All three must be
specified together to maintain contract coherence.

### `leaf_packages: Option<Vec<String>>`

**Fleet semantics:** Intersection of all authoritative hosts' leaf
sets. A package appears in the fleet's `leaf_packages` only if it is
classified as leaf on every host that has authoritative leaf data.

**`Some([])` vs `None`:** These are distinct states.
- `Some([])` — authoritative empty: leaf classification ran on all
  contributing hosts and no packages survived the intersection. This
  participates in downstream filtering (result: no leaf packages →
  Containerfile has no `dnf install` line).
- `None` — unavailable: leaf classification could not run or all hosts
  degraded. Downstream consumers must treat as "leaf truth unknown" and
  fall back to showing/installing all included packages.

### `auto_packages: Option<Vec<String>>`

**Fleet semantics:** `None`. Auto packages are not independently
meaningful in fleet mode — they are derivable from "everything in host
snapshots' `packages_added` that didn't survive the leaf intersection."
The field exists for single-host mode; fleet leaves it `None`.

### `leaf_dep_tree: serde_json::Value`

**Fleet semantics:** Filtered dep tree containing only entries for
packages that survived the leaf intersection. Maps leaf `name.arch` to
their dependency `name.arch` lists. Computed by taking the first
authoritative host's dep tree and removing entries for packages that
were filtered out of the intersection.

This is the drill-down mechanism: when a user sees `git` in the fleet
refine view and wants to know what comes with it, the dep tree answers.

Note: the dep tree reflects source-host dependency chains, which are
informational. Actual deps are resolved at build time by `dnf` against
the target image's repos.

**Degraded state:** When all hosts have degraded leaf classification,
`leaf_dep_tree` serializes as `{}` (empty object), NOT `null`. This
matches the existing repo contract where `serde(default)` on
`serde_json::Value` produces `Value::Null`, but the inspectah schema
convention uses an empty object for "no dep tree data." The degraded
state for the full triplet is:
- `leaf_packages: None` (null in JSON)
- `auto_packages: None` (null in JSON)
- `leaf_dep_tree: {}` (empty object in JSON)

## Partial authority and degraded hosts

**Rule:** Hosts with `leaf_packages: None` (degraded or unavailable
classification) are skipped in the intersection. The fleet leaf data
is authoritative across the authoritative subset.

**Coverage metadata:** The merged `RpmSection` must carry a count of
contributing hosts vs total hosts for leaf classification (e.g.,
`leaf_authority_hosts: 8` / `leaf_total_hosts: 10`). Downstream
consumers that produce operator-facing guidance — Containerfile output,
refine view package list, and fleet report summary — **must** surface
partial authority when `leaf_authority_hosts < leaf_total_hosts`. The
format is a metadata line (e.g., "Leaf classification: 8/10 hosts"),
not a warning banner. When all hosts contribute, no indicator is shown.

**Threshold:** No minimum coverage threshold for this iteration. If
1 of 10 hosts has leaf data, the fleet uses that one host's leaf set.
This is an acknowledged limitation.

**Sunset note:** Long-term, degraded leaf classification should be
blocked at the scan level (fail or warn, not silently degrade). This
spec does not implement scan-level enforcement — that is a separate
concern. The partial authority handling here is interim.

## Implementation: where the filter goes

In `merge_rpm_sections` (`inspectah-core/src/fleet/merge.rs`):

1. **After `merge_items`** — prevalence metadata is already attached.
2. **Before repo-conflict detection** — conflicts should only apply to
   leaf packages.
3. **~20 lines:** Compute intersection of all hosts' leaf sets (skip
   `None` hosts), then partition merged `packages_added` into leaf
   (stays) and auto (removed).

### Downstream impact

| Consumer | Impact |
|----------|--------|
| **Containerfile renderer** | Own leaf filter becomes a no-op for fleet (redundancy, not breakage) |
| **Refine view (`session.rs`)** | `!is_fleet_snapshot` guard becomes unnecessary — data is pre-filtered |
| **`service_intent::effective_target_packages`** | **Must fix.** See Service Intent section below. |
| **Report templates, web API** | Show fewer packages — that's the point |
| **Audit log** | Shows leaf-only fleet findings — correct |

### Canonical identity

All leaf/auto set operations use `name.arch` canonical identity (e.g.,
`httpd.x86_64`). This is consistent with the existing
`canonical_package_id` function and the `package-identity-is-name-dot-arch`
skill. Bare names must never be used — multiarch collisions are real.

### Service intent: fleet behavior

`service_intent::effective_target_packages` unions baseline package
names with all included `packages_added` to decide which services can
be omitted (a service owned by an included package doesn't need manual
enablement). With leaf-only `packages_added`, an auto package's service
could be incorrectly flagged as needing manual enablement.

**Decision: gate service omission on fleet.** In fleet mode, skip
package-based service omission entirely. Show all services that differ
from baseline and let the user decide. Rationale:

1. Service omission is a single-host optimization where the dep chain
   is known and local. Fleet aggregates across hosts with potentially
   different dep chains — the optimization's premise doesn't hold.
2. The conservative behavior (show all services) is correct for fleet
   triage. Missing a service is worse than showing an extra one.
3. Avoids carrying a parallel unfiltered package set through the merge
   pipeline, which would add complexity for marginal benefit.

**Implementation: guard at the caller, not per-function.** The service
intent engine has two functions that read `packages_added`:

- `effective_target_packages()` — builds the target package set from
  baseline + `packages_added` (drives Tier 5/7 decisions)
- `classify_service_presence()` — directly checks `packages_added`
  for per-service include/installability state (drives Tiers 2-4)

Both would produce wrong results with leaf-only fleet `packages_added`.
Gating only `effective_target_packages` is insufficient — Tier 7 in
`classify_service_presence` would still omit auto-package services.

The guard goes in `render_service_intent()` (the caller): when the
snapshot is fleet-aggregated, skip `classify_service_presence` entirely
and force all services to `Emit`. This covers both code paths in one
guard and ensures no fleet service is silently omitted.

### Deterministic ordering

The intersection result must be sorted by canonical identity to ensure
deterministic JSON output regardless of host processing order.

## Surfaces Touched

- [x] Backend (core types, collect, pipeline)
- [ ] CLI (new flags, subcommands, output changes)
- [ ] HTML report (new section, template, rendering)
- [ ] Web refine UI
- [ ] TUI refine UI
- [x] Containerfile rendering
- [ ] Audit log output
- [ ] Docs (user-facing documentation)

## Pipeline Stages

| Stage | Status | Justification (if skipped) |
|-------|--------|---------------------------|
| Brainstorm | skip | Prespec fully describes the approach after multi-round design |
| Spec Review | required | — |
| Plan | required | — |
| Plan Review | skip | Sequenced delivery, moderate blast radius |

## Feature Type Checklist

### Backend (core / collect / pipeline)

- [ ] Compute intersection of all hosts' leaf sets in `merge_rpm_sections` (skip `None` hosts)
- [ ] Filter merged `packages_added` to intersection-leaf packages only
- [ ] Set fleet `auto_packages` to `None`
- [ ] Filter `leaf_dep_tree` to only entries surviving the intersection
- [ ] Add `leaf_authority_hosts` / `leaf_total_hosts` coverage metadata to merged `RpmSection`
- [ ] All-degraded state: `leaf_packages=None`, `auto_packages=None`, `leaf_dep_tree={}` (empty object, not null)
- [ ] Gate `render_service_intent()` — skip `classify_service_presence` and force all services to Emit for fleet snapshots
- [ ] Surface partial authority metadata on Containerfile output, refine view, and fleet report when coverage is partial
- [ ] Sort intersection result by canonical `name.arch` identity
- [ ] Tests: intersection across hosts with matching leaf data produces correct leaf-only `packages_added`
- [ ] Tests: package that is leaf on some hosts, auto on others → excluded
- [ ] Tests: `Some([])` (authoritative empty) vs `None` (degraded) handled distinctly
- [ ] Tests: degraded hosts skipped in intersection
- [ ] Tests: all hosts degraded → `leaf_packages=None`, `auto_packages=None`, `leaf_dep_tree={}` (empty object)
- [ ] Tests: host-present vs host-absent leaf packages
- [ ] Tests: multiarch identity (`name.arch`) used throughout, no bare names
- [ ] Tests: full triplet coherence — `leaf_packages` / `auto_packages` / `leaf_dep_tree` consistent
- [ ] Tests: intersection result is order-independent (deterministic regardless of host order)
- [ ] Tests: `service_intent` fleet path does not incorrectly omit auto-package services
- [ ] Existing fleet merge tests pass (`cargo test --workspace`)
- [ ] `cargo clippy --workspace -- -D warnings` clean

### Containerfile Rendering

- [ ] Verify with fleet Containerfile snapshot test that install line contains only intersection-leaf packages
- [ ] Verify renderer's own leaf filter is a no-op for fleet (no behavioral change, just redundancy)
