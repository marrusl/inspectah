# Unified Include-Default Model

## Problem

inspectah has 25 toggleable item types using 5 different strategies to set
`include` defaults: tier-based classifiers (packages, configs), collector-sets-true
(services, drop-ins), fleet prevalence gate (quadlets, flatpaks, tuned),
no normalization at all (12 item types), and content-based gates (repos, GPG keys).
This patchwork produces edge cases — items that default to excluded in single-host
mode, render-layer overrides that paper over missing collector defaults, and no
single source of truth for "should this be included?"

## Principle

**Single-host snapshots are truth.** Everything the collector finds defaults to
included. The only things that set `include: false` are:

1. **Classifiers** — analytical decisions about migration intent (RPM packages,
   config files, tuned stock-profile detection)
2. **Fleet aggregation** — narrowing by prevalence/intersection (strict
   universality: `count == total`)
3. **Semantic exclusions** — items that are wrong in image mode regardless of
   context (merge-hostile configs, image-incompatible services)

No downstream normalization. No render-layer overrides.

## Design

### 1. Collector Behavior

Every inspector sets `include: true` on collected items, with two exceptions:

- **RPM packages** — the existing classifier decides (leaf packages, baseline
  subtraction, tier-based). No change.
- **Config files** — the existing classifier decides. The classification is:
  - `RpmOwnedDefault` → Baseline → `false` (package manager handles it)
  - `BaselineMatch` → Baseline → `false` (matches base image)
  - `RpmOwnedModified` → Investigate → `true` (user changed it)
  - `Unowned` → Site → `true` (user/automation created it)
  - `Orphaned` → Site → `false` (package removed, config left behind)
  - No change to this logic.

**Tuned profile:** `is_stock_tuned_profile()` is a **classifier**, not a
normalization hack. It distinguishes stock profiles (e.g., `virtual-guest`,
`throughput-performance`) from non-stock profiles. Non-stock profiles default to
`include: true`; stock profiles default to `include: false`. The classifier
cannot currently distinguish "tuned auto-selected a stock profile" from "admin
intentionally pinned a stock profile" — both are excluded by default. This is a
known limitation; intentionally pinned stock profiles are recovered via the
refine toggle. No change to the existing collector logic
(`crates/collect/src/inspectors/kernelboot.rs:140`) or fleet merge logic
(`crates/core/src/fleet/merge.rs:1514`). Both already call
`is_stock_tuned_profile()` and produce the correct defaults. Tuned is a standard
prevalence-tracked item with strict universality in fleet mode.

**Inspectors that change:** non-RPM software, compose files, flatpak apps,
quadlets, firewall zones, NM connections, cron jobs, generated timers,
sysctl overrides, kernel modules, SELinux port labels. All set `include: true`
explicitly instead of relying on serde default of `false`.

**Fstab entries:** Keep the `include` field on `FstabEntry` for schema
compatibility but have refine ignore it — no toggle rendered, displayed as
reference-only with an exclusion reason badge (e.g., "host state — not
image-portable"). Fstab entries are host state — on a fresh bootc deployment,
`/etc/fstab` starts empty and is populated by the host's first-boot provisioning
or carried forward from the pre-migration host's persistent `/etc`. Baking fstab
into a Containerfile would impose one host's disk layout on every image consumer.

### 2. What Gets Deleted

**Session layer (`crates/refine/src/session.rs`):**
- Delete the single-host normalization block (~lines 337-410, the 12-item block)
- Keep `normalize_package_defaults` — real classifier
- Keep `normalize_config_defaults` — real classifier
- Delete the fleet prevalence gate's per-item `include = false` logic
  (~lines 182-327). Fleet narrowing moves to the aggregate pass (see §3).

**Render layer (`crates/pipeline/src/render/containerfile.rs`, `configtree.rs`):**
- Delete all `|| is_single_host` overrides (6 occurrences across 2 files)
- Render filters on `.include` only — the flag is already correct from
  collector + classifier + semantic exclusions

### 3. Fleet Narrowing Moves to Aggregate

The fleet aggregate pass lives in `crates/core/src/fleet/merge.rs`. The
`MergeWith` trait already has `set_include(&mut self, val: bool)` wired up for
every section type.

**New behavior:** During fleet merge, after computing prevalence for each item,
set `include: false` on items that are not universal across the fleet (i.e.,
`count < total`). This replaces the current per-item prevalence gate in
`session.rs`. Strict universality is the threshold — no exceptions for any
item type including tuned profiles.

The fleet triage classification in `crates/refine/src/fleet/classify.rs`
continues to set triage labels for UI presentation. It does not set `include` —
that is now solely the aggregate pass's responsibility.

**Fleet reference sections invariant:** Fleet reference sections in
`crates/web/src/fleet_handlers.rs` must consume stored `.include` values from
the projected snapshot. They must not derive or recompute inclusion. Several
reference section builders currently call `fleet_include_default(fp)` to
recompute inclusion from prevalence — these must be refactored to read the
stored `.include` value set by the aggregate pass. This is required
implementation work, not current behavior.

### 4. Semantic Exclusions

Two categories of items that are wrong in image mode regardless of context.
Both follow the same enforcement pattern: the normalize layer in
`inspectah-refine` sets `include: false` and marks the item as `locked: true`
to prevent re-toggling via the UI. The renderer needs no changes — it continues
to filter on `.include` uniformly.

#### 4a. Merge-Hostile Configs

Files that fight the bootc `/etc` 3-way merge if image-baked. Collected for
diagnostic value but locked out of the Containerfile.

**Deny list:**
- `/etc/fstab` (collected by storage inspector, not config walker)
- `/etc/crypttab`

**Already excluded by the walker** (never collected, no change needed):
- `/etc/hostname`
- `/etc/machine-id`
- `/etc/resolv.conf`
- `/etc/adjtime`
- `/etc/localtime`

**Implementation:** Add `normalize_merge_hostile_configs()` in
`crates/refine/src/normalize.rs`, following the existing
`normalize_incompatible_services()` pattern. Called from `load_for_refine()`.
Sets `include = false` and `locked = true` on matched paths.

#### 4b. Image-Incompatible Services

Services that are semantically wrong in image mode. The existing
`normalize_incompatible_services()` function in `crates/refine/src/normalize.rs`
already handles this — it sets `include = false` and
`attention_reason = "service-image-mode-incompatible"` on matched units.

**Current deny list** (in `INCOMPATIBLE_SERVICES`, `crates/core/src/baseline.rs`):
- `dnf-makecache.service`
- `dnf-makecache.timer`
- `packagekit.service`
- `packagekit-offline-update.service`

**Changes needed:**
- Add `locked: true` to prevent re-toggling (same field as merge-hostile)
- These services should be **visible but excluded** in the UI — shown grayed out
  with a reason badge explaining why they are incompatible (e.g., "manages package
  repos at runtime"). Hiding them would make users question the scan's completeness.
- **Service-owned drop-ins:** When a service is image-incompatible, its associated
  drop-ins (e.g., drop-ins under `dnf-makecache.service.d/`) must also be locked.
  `normalize_incompatible_services()` must walk `services.drop_ins` and lock any
  drop-in whose parent unit matches an incompatible service.

#### Enforcement: The `locked` Field

Add a `locked: bool` field (default `false`) to item types that can be
semantically excluded. When `locked` is true:
- The session toggle handler refuses to flip `include` back to `true`
- The API response DTO passes `locked` and the reason to the frontend
- The frontend renders the item as visible but non-toggleable (grayed out toggle
  with reason badge)

This makes semantic exclusions **impossible to override** through the UI or API
while keeping the items visible for diagnostic reference. The renderer does not
check `locked` — it only checks `.include`, which the normalize layer has already
set to `false`.

**Session resume enforcement:** Session resume (`session.rs:532-537`) restores
autosaved ops via direct assignment, bypassing `apply()` validation. The
`recompute_view()` op replay loop (line ~1339, `RefinementOp::SetInclude`)
must check `locked` before applying any `SetInclude(true)` op. If the target
item is locked, skip the op silently.

**Export/render clamp:** As a defense-in-depth measure, the snapshot projection
layer (where `recompute_view()` produces the projected snapshot consumed by
renderers) must clamp locked items to `include: false` regardless of stored
state. This ensures both the Containerfile renderer and the configtree
materializer receive correct values without needing their own locked checks.

**Reason storage:** For merge-hostile configs, the reason is stored as
`attention_reason` on the `ConfigFileEntry` or `FstabEntry` (same field the
image-incompatible services already use on `ServiceStateChange`). The API
response DTO surfaces both `locked: true` and the `attention_reason` string
so the frontend can render the badge.

**Note:** The config walker's existing `UNOWNED_EXCLUDE_EXACT` list (77 exact
paths, 30+ globs, 3 prefixes in `crates/collect/src/inspectors/config/walk.rs`)
stays as-is. It serves a different purpose: collection-time noise filtering for
files with no diagnostic value. Semantic exclusions operate at classification
time on files that ARE collected. Two lists, two stages, clean separation.

### 5. Type Normalization

Normalize `Option<bool>` include fields to plain `bool` with
`#[serde(default = "default_true")]`.

The `Option<bool>` three-state carries no meaningful semantics today. 18+ structs
already use plain `bool`. No custom deserializer for null values — existing
snapshots re-scan per standing policy.

**Implementation requirement:** The implementer must `grep` for
`include: Option<bool>` across all `crates/core/src/types/` structs at
implementation time and normalize each to `include: bool` with
`#[serde(default = "default_true")]`. Do not rely on the examples from earlier
brainstorming — audit the actual code.

### 6. Boot-Chain Exclusion (dropped)

The config walker only scans `/etc/`, never `/boot/`. Boot loader configs,
BLS entries, and GRUB configs are not collected. No exclusion needed.

## Items Explicitly Out of Scope

- **Non-RPM software rendering improvements** (real `RUN pip install` instructions
  instead of advisory stubs) — separate feature work
- **Compose-to-quadlet conversion** — separate feature work
- **Ember's classification-not-filtering concept** (tagging noise for bulk dismiss) —
  separate UX feature
- **Config noise filtering** (fleet consensus, deny lists for non-merge-hostile
  noise) — see `topics/inspectah-driftify.md` config noise filtering topic
- **Tuned scalar threshold** (dominant profile as fleet default) — decided against;
  strict universality applies, prevalence sorting handles the UI discoverability

## Implementation Notes (deferred from review)

The following items were raised during team review rounds 2-3 and are
**implementation-level concerns**, not spec-level design gaps. The implementation
plan must address each one.

1. **Tuned fleet merge interaction.** Tuned uses `most_prevalent_scalar()` in
   `fleet/merge.rs` to pick the winning profile, then `is_stock_tuned_profile()`
   to set `tuned_include`. The plan must verify that the new aggregate narrowing
   (`set_include` on non-universal items) composes correctly with this scalar
   merge — specifically, what happens when tuned is universal but stock, vs.
   non-universal but custom. Write tests for both cases.

2. **Fleet reference handlers.** Earlier investigation found that
   `crates/web/src/fleet_handlers.rs` reads stored `.include` values for all
   item types. Round 3 reviewers still flagged potential recomputation in some
   reference sections. The plan must include a code audit of every reference
   section in `fleet_handlers.rs` to confirm no path recomputes inclusion, and
   add a test asserting stored-value passthrough.

3. **Stock-profile intent ambiguity.** `is_stock_tuned_profile()` cannot
   distinguish "auto-selected by tuned" from "admin deliberately pinned a stock
   profile." This is inherent to the heuristic and accepted as a known
   limitation. If a user intentionally pinned `throughput-performance`, they
   toggle it back on during refinement — same as any classifier-excluded item.

## Migration / Compatibility

- No snapshot schema breaks. Fields are kept, defaults change. The new `locked`
  field defaults to `false` via serde, so old snapshots load without issue.
- Old snapshots with `include: false` on items that should now default to true
  will show the old defaults until re-scanned. This is acceptable per the
  no-old-tarball-compat policy.
- Fleet snapshots generated by old aggregate code will have the old narrowing
  behavior until re-aggregated.

## Test Impact

- ~10 render-layer tests asserting `is_single_host` override behavior need
  rewriting (the concept disappears)
- Session-layer tests for the single-host normalization block become dead code
- Fleet merge tests need updating for the new narrowing behavior in `merge.rs`
- Regression guard: add a validation pass that tracks ownership of `include: false`.
  Valid sources are: classifiers (RPM, config, tuned stock-profile), fleet
  aggregate (non-universal), and semantic exclusions (merge-hostile,
  image-incompatible including drop-ins). Any `include: false` not attributable
  to one of these sources is a regression.

## Summary of Changes by Crate

| Crate | Change |
|-------|--------|
| `inspectah-collect` | Set `include: true` in all inspectors (except config, which keeps `false` for classifier). Tuned `is_stock_tuned_profile()` stays as-is (it is a classifier). |
| `inspectah-core` | Normalize `Option<bool>` → `bool` (verify list at impl time). Move fleet narrowing into `fleet/merge.rs` `set_include` calls. Tuned `is_stock_tuned_profile()` in fleet merge stays as-is. Add `locked: bool` field to relevant item types. |
| `inspectah-refine` | Delete single-host normalization block. Delete fleet prevalence gate. Add `normalize_merge_hostile_configs()`. Add `locked = true` to `normalize_incompatible_services()`. Enforce `locked` in session toggle handler and `recompute_view()` op replay. Add export/render clamp in snapshot projection. |
| `inspectah-pipeline` | Delete `is_single_host` overrides in `containerfile.rs` and `configtree.rs`. No other changes — renderer continues to filter on `.include` only. |
| `inspectah-web` | Pass `locked` and `attention_reason` fields through to frontend. Render locked items as visible-but-excluded with reason badge. Fleet handlers continue to consume stored `.include` (no recomputation). |
| `inspectah-tui` | Respect `locked` for semantic exclusions. Show reason inline. |
