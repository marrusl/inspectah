# Fleet Aggregate Spec

**Spec 1 of 3** in the fleet redesign. This spec covers the merge engine
and CLI command. Fleet Refine (Spec 2) and Architect (Spec 3) are
separate, later specs.

## Overview

`inspectah fleet` aggregates N single-host tarballs into one fleet
tarball with prevalence metadata. The goal is role-level consolidation:
"what should a web server look like?" The output is a complete,
inspectable, buildable **draft** artifact — same rendered file set as a
single-host scan, plus fleet prevalence data and content variant storage.
`inspectah refine` is the intended review and sign-off step (blocked
today by the tarball provenance gate — see Refine Import: Three Layers).

Fleet aggregate is a pure data operation. It annotates prevalence and
preserves all content variants. It does not make editorial decisions
about inclusion or exclusion — that judgment lives in the refine layer
(Spec 2).

## Phasing Context

The end-to-end workflow is bottom-up:

1. **Scan** — run inspectah across N hosts in a role
2. **Aggregate** (this spec) — combine tarballs into one fleet tarball
3. **Refine** (Spec 2) — interactive session to build the "perfect"
   role definition
4. **Repeat** — steps 1-3 per role (web, DB, app-server, etc.)
5. **Architect** (Spec 3) — takes refined fleet tarballs, discovers
   cross-role hierarchy, exports decomposed tarball set

Architect is a separate tool designed after fleet aggregate and refine
are shipped and validated.

## Snapshot Contract

### Schema Version

Adding `fleet_meta` to `InspectionSnapshot` bumps `SCHEMA_VERSION`. The
fleet merge engine requires all input snapshots to share the same
schema version (validated before merge). The merged output uses the
current `SCHEMA_VERSION`.

### Fleet Metadata Field

`FleetSnapshotMeta` is carried as a new top-level field on
`InspectionSnapshot`:

```rust
pub struct InspectionSnapshot {
    // ... existing fields ...

    /// Present only on fleet-merged snapshots. None on single-host
    /// snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_meta: Option<FleetSnapshotMeta>,
}
```

Serialized JSON field name: `"fleet_meta"`. Single-host snapshots omit
this field (serde skip_serializing_if). Downstream code (refine, render)
detects a fleet snapshot by checking `fleet_meta.is_some()`.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetSnapshotMeta {
    pub label: String,
    pub host_count: usize,
    pub hostnames: Vec<String>,         // sorted lexicographically
    pub merged_at: String,              // ISO 8601 UTC
    pub baseline_provisional: bool,     // true when baseline was auto-selected from conflicting inputs
    pub section_host_counts: BTreeMap<String, usize>,  // sorted; per-section reporting counts for Spec 2 UI
}
```

### Existing Snapshot-Level Fields

The merged snapshot uses the existing `InspectionSnapshot` fields for
baseline and trust — no competing channels.

| Field | Merged value | Notes |
|-------|-------------|-------|
| `target_image` | Manifest `baseline` override if set; else most-common autodetected `target_image` across inputs. | This IS the `FROM` source. No separate `baseline_image` field. |
| `baseline` | Baseline data from the host matching the selected `target_image`. If multiple hosts share the same target image, use the baseline data from the first host (sorted by hostname). | If no input has baseline data, set to `None`. |
| `no_baseline` | `true` if `baseline` is `None` after merge. | Same semantics as single-host. |
| `completeness` | Conservative merge using the actual `Completeness` enum: `Complete` if ALL inputs are `Complete`; otherwise `Partial { degraded_sections, reason }` or `Incomplete { failed_sections, degraded_sections, reason }` with merged section lists and reason strings from all non-Complete inputs. See Missing Data Semantics section. | Refine consumes this field to display completeness badges. |
| `redactions` | Deduplicated union of all input redaction findings. | Identity by path + pattern. |
| `redaction_hints` | Deduplicated union of all input redaction hints. | Identity by path + reason. |
| `redaction_state` | `None`. Fleet aggregate does not redact. The merged snapshot is unredacted. Note: current `from_tarball()` rejects `None` — see Refine Import: Three Layers for the full provenance story. | |
| `sensitive_snapshot` | `true` if ANY input has `sensitive_snapshot: true`. | Conservative: if one host preserved sensitive data, the fleet inherits that flag. |
| `preserved_credentials` | `true` if ANY input has `preserved_credentials: true`. | Fleet inherits per-host trust flags. |
| `preserved_ssh_keys` | `true` if ANY input has `preserved_ssh_keys: true`. | Fleet inherits per-host trust flags. |
| `warnings` | Deduplicated union of all input warnings, plus any fleet-specific warnings (baseline conflict, stale scans). | |
| `os_release` | From the first input (sorted by hostname). All inputs share the same OS major version (validated). | Minor version differences are noted in warnings. |
| `system_type` | From the first input. All inputs should share system type. | Mismatch is a validation warning. |
| `meta` | Merged HashMap. Fleet-specific keys added: `"fleet_source": "aggregate"`. Host-specific keys from individual snapshots are dropped. | |
| `preflight` | Dropped (set to default). Not meaningful for merged snapshots. | |

### Refine Trust Contract

The merged snapshot is an **unredacted, unconfirmed draft**. Its trust
posture is:

- `redaction_state: None` — current `from_tarball()` rejects this
  state. Fleet tarballs cannot be imported into refine today. See
  Refine Import: Three Layers for resolution options (Spec 2 scope).
- `fleet_meta.baseline_provisional` — refine SHOULD surface this flag
  and allow the operator to confirm or change the baseline before
  export. This is a Spec 2 UX concern; aggregate just persists the
  flag.
- `VariantSelection::Selected` — refine SHOULD allow swapping the
  active variant. This is a Spec 2 UX concern; aggregate just sets
  the deterministic default.

### Refine Import: Three Layers

The relationship between fleet aggregate output and refine has three
distinct layers. Each has a different compatibility story today.

**Layer 1: Serde deserialization — works today.**
New fields (`fleet_meta`, `VariantSelection`) use `serde(default)` /
`skip_serializing_if`. The merged snapshot JSON deserializes through
`load_for_refine()` without error. Non-fleet snapshots see
`fleet_meta: None` and default `VariantSelection` values.

**Layer 2: Tarball import provenance gate — BLOCKS today.**
`from_tarball()` calls `validate_provenance()`, which accepts
snapshots with explicit post-redaction states (`FullyRedacted`,
`PartiallyRedacted`, `SensitiveRetained`) and rejects unset or raw
states (`None`, `Raw`, `Unknown`). Fleet aggregate produces
`redaction_state: None` (unredacted output), which falls in the
rejected category. This means `inspectah refine <fleet-tarball>`
will fail with a provenance error under the current import path.

Resolution options (Spec 2 scope):
- Extend the provenance gate to accept `redaction_state: None` when
  `fleet_meta.is_some()` (fleet-specific exception)
- Require the operator to redact the fleet tarball before refine
  import (same as any other unredacted tarball)
- Add a fleet-specific import path that bypasses the provenance
  requirement

Until Spec 2 addresses this, the fleet tarball is a standalone draft
artifact — readable Containerfile, browsable config tree, rendered
reports. The operator can inspect and use it without refine.

**Layer 3: Fleet-aware display — requires Spec 2 enhancements.**
Even after the provenance gate is resolved, the existing refine UX
will show fleet items as regular items with `include: true` —
functional but limited. Full fleet-aware display requires:

- Prevalence columns and threshold controls
- Variant swapping UI (`VariantSelection` awareness)
- Baseline confirmation workflow (`baseline_provisional` awareness)
- Section-level "reported by N of M hosts" indicators

Fleet aggregate does NOT require refine-side code changes to ship.
The merged tarball is a usable draft artifact on its own. Refine
enhancements improve the interactive experience but are not gating.

When `baseline_provisional` is `true` in `FleetSnapshotMeta`, the
`target_image` value is a deterministic auto-selection, not operator
intent. The Containerfile header and audit report should note this
provisionality. Refine (Spec 2) allows the operator to confirm or
change the baseline.

## Data Model

### FleetMergeable Trait

A trait implemented on item types that participate in prevalence-tracked
merging. Not all item types implement this trait — only those that carry
`fleet: Option<FleetPrevalence>` and `include: bool` fields. Types
without these fields are handled by section adapters using simpler
deduplication strategies (see Round-1 Coverage Table).

```rust
trait FleetMergeable: Clone {
    /// Identity key for grouping. See coverage table for per-type keys.
    fn identity_key(&self) -> Cow<'_, str>;

    /// Mutable access to the fleet prevalence field.
    fn fleet_mut(&mut self) -> &mut Option<FleetPrevalence>;

    /// Set the include flag.
    fn set_include(&mut self, val: bool);

    /// Mutable access to the variant selection field. Types without
    /// content variants return None (default).
    fn variant_selection_mut(&mut self) -> Option<&mut VariantSelection> { None }

    /// Content hash for variant detection. Returns None for types
    /// without content variants. Types with content variants return
    /// a SHA-256 hash of their content field.
    fn content_variant_key(&self) -> Option<Cow<'_, str>> { None }
}
```

The trait is only implemented on types that already carry both `fleet`
and `include` fields. Types that need these fields added for fleet
support are listed in the coverage table.

### VariantSelection Enum

Replaces the `tie`/`tie_winner` bool pair with a proper enum. Three
states, all valid — no illegal combinations.

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantSelection {
    #[default]
    Only,          // single version, no variants exist
    Selected,      // multiple variants, this is the active one
    Alternative,   // multiple variants, not active
}
```

`Selected` is a deterministic bootstrap default chosen by prevalence
(ties broken by lexicographic content hash). It is NOT operator intent —
it is a provisional starting point for the refine workflow. The
Containerfile and build tree render the `Selected` variant, but the
audit report notes when selection was automatic rather than confirmed.

This field replaces `tie: bool` and `tie_winner: bool` on item structs
that support content variants (ConfigFileEntry, SystemdDropIn,
QuadletUnit, ComposeFile, RepoFile). Schema migration maps
`tie_winner: true` → `Selected`, `tie: true, tie_winner: false` →
`Alternative`, neither → `Only`.

### FleetManifest

Deserialized from TOML. Declares inputs and baseline override. This is
an input declaration, not a behavioral config — no thresholds, filters,
or output formatting settings belong here.

```toml
# inspectah fleet manifest
# Edit label and baseline as needed. Sources are relative to this file.

label = "web-servers"
baseline = "registry.redhat.io/rhel9/rhel-bootc:9.6"

sources = [
  "host-web01.tar.gz",
  "host-web02.tar.gz",
  "host-web03.tar.gz",
]
```

```rust
struct FleetManifest {
    label: Option<String>,
    baseline: Option<String>,
    sources: Vec<PathBuf>,
}
```

The field is named `sources` (not `tarballs`) to allow for possible
future support of direct snapshot JSON input. Paths in `sources` are
resolved relative to the manifest file's parent directory.

## Round-1 Section Coverage Table

This table defines which sections and item types participate in fleet
merging for round 1, their identity keys, variant support, and non-key
field resolution rules.

### Prevalence-tracked types (implement FleetMergeable)

These types carry `fleet: Option<FleetPrevalence>` and `include: bool`
and participate in the generic merge function.

| Section | Type | Identity Key | Has Variants | Non-Key Resolution |
|---------|------|-------------|-------------|-------------------|
| `rpm` | `PackageEntry` | `name.arch` | No | Version/release: most prevalent. Epoch: most prevalent. State: most prevalent. Source repo: most prevalent. |
| `rpm` | `RepoFile` | `path` | Yes (content) | `is_default_repo`: most prevalent value. |
| `config` | `ConfigFileEntry` | `path` | Yes (content) | `kind`, `category`: most prevalent. `package`, `rpm_va_flags`, `diff_against_rpm`: from selected variant's host. |
| `services` | `ServiceStateChange` | `unit` | No | `current_state`, `default_state`, `action`: most prevalent. `owning_package`: most prevalent non-empty value. |
| `services` | `SystemdDropIn` | `path` | Yes (content) | None — content IS the non-key differentiator. |
| `containers` | `QuadletUnit` | `path` | Yes (content) | All metadata from selected variant's host. |
| `containers` | `ComposeFile` | `path` | Yes (content) | All metadata from selected variant's host. |
| `rpm` | `ModuleStream` | `module:stream` | No | `profiles`: union. `baseline_match`: most prevalent. Already has `fleet`/`include`. |
| `rpm` | `VersionLockEntry` | `name.arch` | No | `version`, `release`, `epoch`: most prevalent. Already has `fleet`/`include` (if present; add if missing). |
| `selinux` | `SelinuxPortLabel` | `protocol:port` | No | `label_type`: most prevalent. |

"Most prevalent" means the value held by the most hosts. Ties broken
by lexicographic comparison of the value itself.

### Types needing fleet field added

These types have `include: bool` but no `fleet: Option<FleetPrevalence>`
today. Round 1 adds the `fleet` field to enable prevalence tracking.

| Section | Type | Identity Key | Notes |
|---------|------|-------------|-------|
| `kernel_boot` | `KernelModule` | `name` | Add `fleet: Option<FleetPrevalence>`. |
| `kernel_boot` | `SysctlOverride` | `key` | Add `fleet: Option<FleetPrevalence>`. |
| `non_rpm_software` | `NonRpmItem` | `name` | Add `fleet: Option<FleetPrevalence>`. Has content field but variant support deferred — use content from most prevalent host. |

### Non-prevalence types (section adapter handles directly)

These types do not carry `fleet` or `include` and are handled by
section adapters with simpler strategies.

| Section | Type/Field | Strategy |
|---------|-----------|----------|
| `rpm` | `VersionChange` | Deduplicate by `name.arch`. Keep most-common direction. |
| `rpm` | `ModuleStream` | Deduplicate by `module:stream`. Prevalence-tracked (already has `fleet`/`include`). |
| `rpm` | `dnf_history_removed` | Deduplicate string list (union). |
| `rpm` | `baseline_package_names` | From the selected baseline only (the host whose `target_image` matches the merged `target_image`). If `baseline_provisional` is true, these names reflect the auto-selected baseline and may change when the operator confirms a different baseline in refine. Set to `None` when no input has baseline data. |
| `rpm` | `baseline_suppressed` | Recomputed against the selected baseline's `baseline_package_names`. Packages in `packages_added` whose `name.arch` appears in `baseline_package_names` are flagged. |
| `rpm` | `no_baseline` | `true` if `baseline_package_names` is `None` after merge. Matches the selected baseline's truth, not a union. |
| `rpm` | `base_image` | From the merged `target_image`. Single authoritative value. |
| `services` | `enabled_units` / `disabled_units` | Deduplicate string lists (union). |
| `network` | `FirewallZone` | Deduplicate by `name`. Content variants deferred — use most prevalent content. |
| `network` | `NmconnProfile` | Deduplicate by `filename`. |
| `network` | `ProxyEntry` | Deduplicate by `env_var`. |
| `storage` | `StorageMount` | Deduplicate by `mountpoint`. |
| `storage` | `IscsiTarget` / `NfsMount` | Deduplicate by identity (target/share path). |
| `scheduled_tasks` | `ScheduledTask` | Deduplicate by `name`. |
| `scheduled_tasks` | `GeneratedTimerUnit` | Deduplicate by `unit_name`. |
| `kernel_boot` | `cmdline` / `grub_defaults` | Most prevalent string value. |
| `kernel_boot` | `AlternativeEntry` | Deduplicate by `name`. |
| `kernel_boot` | `ConfigSnippet` (modules_load_d, modprobe_d) | Deduplicate by `path`. Content from most prevalent. |
| `selinux` | `custom_modules` / `fcontext_rules` | Deduplicate string lists (union). |
| `selinux` | `boolean_overrides` | Deduplicate by JSON value equality. |
| `selinux` | `CarryForwardFile` (audit_rules, pam_configs) | Deduplicate by `path`. Content from most prevalent. |
| `selinux` | `mode` / `fips_mode` | Most prevalent value. |
| `users_groups` | Full section | Deduplicate users by `name`, groups by `name`. Membership lists are unioned. |

### Sections NOT in round 1

| Section | Reason |
|---------|--------|
| `containers.running_containers` | Runtime state, not configuration. Not meaningful to merge. |

## Missing Data Semantics

### Global Denominator Rule

Prevalence always uses total host count as the denominator, regardless
of how many hosts actually reported a given section. If 30 of 100 hosts
have a config file and 70 hosts have no config section at all, the
file's prevalence is 30/100, not 30/30.

Rationale: a migration tool needs to answer "how common is this?"
relative to the whole fleet population. Per-section denominators
inflate prevalence of niche configurations, which pushes the user
toward including items that aren't actually widespread. Global
denominator gives a conservative floor — the right bias for golden
image decisions.

### Section-Level Host Counts

`FleetSnapshotMeta.section_host_counts` records how many hosts reported
each section (keyed by section name: `"rpm"`, `"config"`, `"services"`,
etc.). This metadata enables the Spec 2 refine UI to display a
"reported by N of M hosts" indicator at the section level, giving users
calibration without changing the prevalence math.

### Completeness

`completeness` on the merged snapshot is a conservative merge using
the actual `Completeness` enum:

- `Complete` — only if ALL inputs are `Complete`
- `Partial { degraded_sections, reason }` — if any input is `Partial`
  and none are `Incomplete`. `degraded_sections` is the union of all
  degraded inspector IDs across inputs. `reason` is a merged string
  noting the host count and which inspectors degraded.
- `Incomplete { failed_sections, degraded_sections, reason }` — if
  any input is `Incomplete`. `failed_sections` and
  `degraded_sections` are unions across inputs. `reason` is merged.

This tells downstream consumers "this fleet data may be incomplete
for section X" without changing prevalence calculations.

## Merge Engine

### Architecture: Hybrid (Generic Core + Section Adapters)

The adapter layer is lightweight — private functions in `merge.rs`, not
a separate abstraction:

**Generic item merge** — a single function handling any
`FleetMergeable` type:

- Takes items from N snapshots (consumed by value), groups by
  `identity_key()` into `HashMap<String, Vec<(usize, T)>>` where
  `usize` is the source host index
- For each group: computes prevalence (count/total/hosts), sets
  `FleetPrevalence`
- If `content_variant_key()` returns `Some`: subgroups by content hash.
  Most prevalent variant becomes `Selected`, others become
  `Alternative`. Ties broken by lexicographic hash comparison for
  determinism.
- If `content_variant_key()` returns `None`: straightforward dedup by
  identity. Non-key fields resolved per the coverage table ("most
  prevalent" = value held by the most hosts, ties broken by
  lexicographic comparison of the value itself).
- All items get `include: true` — no threshold filtering at aggregate
  time

**Section adapters** — thin private functions per snapshot section:

- Extract `Vec<T>` fields from each snapshot's section
- Call the generic merge for each `FleetMergeable` field
- Handle non-prevalence types: deduplicate flat string lists,
  deduplicate by identity with most-prevalent-value resolution,
  pass through metadata fields
- Assemble the merged section struct

**Top-level orchestrator** — `merge_snapshots()`:

- Takes `Vec<InspectionSnapshot>` by value (consume, don't clone)
- Runs validation (see Validation section) — returns early if hard
  errors. Warnings are collected and returned alongside the merged
  snapshot.
- Iterates sections: if any snapshot has a given section, runs its
  adapter with the global host count as denominator
- Populates `FleetSnapshotMeta` including `section_host_counts`
- Populates `target_image`, `baseline`, `completeness` per the
  Snapshot Contract rules
- Returns `(InspectionSnapshot, Vec<FleetWarning>)`

### Deterministic Ordering

The determinism guarantee is **input-order independence**: the same set
of input tarballs always produces the same merged output regardless of
CLI argument order, directory traversal order, or glob expansion order.

The output is NOT byte-identical across repeated runs because
`FleetSnapshotMeta.merged_at` records the current timestamp. All other
serialized data is deterministic.

Ordering rules:

- `FleetSnapshotMeta.hostnames`: sorted lexicographically
- `FleetSnapshotMeta.section_host_counts`: serialized as sorted entries
  (use `BTreeMap<String, usize>` in Rust, not `HashMap`)
- `FleetPrevalence.hosts` on each item: sorted lexicographically
- Deduplicated string lists (`dnf_history_removed`, `enabled_units`,
  etc.): sorted lexicographically
- Merged item lists within each section: sorted by identity key
- Items with variants: selected variant first, then alternatives
  sorted by content hash

### Module Layout

New `fleet/` module in `inspectah-core`:

- `fleet/mod.rs` — `merge_snapshots()` orchestrator, public API
- `fleet/merge.rs` — generic merge function + section adapters
- `fleet/validate.rs` — validation checks
- `fleet/manifest.rs` — TOML manifest parsing

`inspectah-core` is the right crate because fleet merge operates on
`InspectionSnapshot`, which lives in core. A separate crate would create
circular dependencies.

## Validation

Validation runs as a separate pass before the merge begins. All checks
are collected into a `Vec<FleetValidationError>` and reported together —
never abort on the first error.

### Hard Errors (block merge, no override)

- **Schema version incompatibility** — tarballs with different schema
  versions cannot be merged
- **Duplicate hostname** — same host appearing in multiple tarballs.
  Two snapshots of the same host means one is stale or the scan ran
  twice. User should deduplicate before retrying.
- **Architecture mismatch** — mixing x86_64 and aarch64 hosts. Merged
  package lists would be meaningless across architectures.
- **Empty/zero-package tarball** — a scan that captured nothing (failed
  sudo, broken snapshot). Merging it dilutes the fleet.
- **OS major version mismatch** — mixing RHEL 8 and RHEL 9 (or
  different Fedora versions) in one fleet. Cross-major package sets are
  incompatible and would produce invalid Containerfile output. Minor
  version mixing (RHEL 9.4 + 9.6) is fine. Users evaluating RHEL 8
  hosts for migration to 9 should aggregate them as separate fleets
  and compare the refined outputs.

### Warnings (print and proceed)

- **Stale scan dates** — flag when tarballs have significantly different
  scan dates (>30 days apart). Shows the date spread so the user can
  decide.
- **Unparseable files in input** — if a glob or directory includes files
  that aren't valid tarballs, warn with filenames. Never silently skip.
- **Baseline image conflicts** — different autodetected baseline images
  across tarballs (when no manifest override is set). Reports the
  distribution and uses the most common. When two or more baselines
  are tied for most common, selects by lexicographic comparison of
  the image ref string (deterministic, not meaningful — the operator
  should confirm in refine). Sets
  `FleetSnapshotMeta.baseline_provisional = true` so the provisionality
  is persisted in the artifact, not just the CLI output.
- **Minor version spread** — different OS minor versions across inputs.
  Notes the spread. Does not block.
- **System type mismatch** — different `system_type` values across
  inputs. Notes the mismatch.

### --strict Flag

Promotes all warnings to hard errors. Designed for CI pipelines where
surprising-but-valid conditions should block.

### Guiding Principle

Anything that makes downstream output (refine, Containerfile render)
structurally invalid is a hard error. Anything that makes it surprising
but technically valid is a warning.

## CLI

### fleet Command

```
inspectah fleet <inputs>... [flags]
inspectah fleet --manifest <path> [flags]
```

**Input modes** (mutually exclusive):

- **Positional args — directory:** `inspectah fleet ./web-servers/`.
  A single positional arg that is a directory loads all tarballs from
  it. Label defaults to directory name.
- **Positional args — file list:** `inspectah fleet *.tar.gz` or
  `inspectah fleet host1.tar.gz host2.tar.gz`. Shell expands globs.
  Label defaults to `fleet`.
- **Manifest flag:** `inspectah fleet --manifest fleet.toml`. Reads
  manifest for sources, label, and baseline. Positional args are an
  error when `--manifest` is set.

**Flags:**

| Flag | Purpose |
|------|---------|
| `--manifest <path>` | TOML manifest file |
| `--baseline <image>` | Baseline image override (error if also set in manifest) |
| `--output-dir <path>` | Write output to this directory |
| `--output-file <path>` | Explicit output tarball name |
| `--json-only` | Emit merged snapshot JSON (see behavior table below) |
| `--strict` | Promote warnings to errors (for CI) |
| `--verbose` | Per-host package counts, full prevalence breakdown |

**Default output** (3 lines + warnings):

```
Fleet: web-servers (50 hosts)
Merged: 847 packages, 23 config files, 12 services
Output: inspectah-fleet-web-servers-20260519.tar.gz
```

The `→ inspectah refine` nudge is intentionally omitted because the
current refine tarball import path rejects `redaction_state: None`.
This nudge will be added when Spec 2 resolves the provenance gate
(see Refine Import: Three Layers).

Warnings print above the summary, visually distinct. Invalid tarballs
are named individually — never silently skipped.

**`--json-only` behavior:**

| Combination | Behavior |
|------------|----------|
| `--json-only` alone | Write JSON to stdout |
| `--json-only --output-file <path>` | Write JSON to `<path>` |
| `--json-only --output-dir <dir>` | Write JSON to `<dir>/fleet-snapshot.json` |
| `--json-only --output-file --output-dir` | Error: conflicting output flags |

When `--json-only` writes to stdout, the summary is suppressed (stdout
is the data channel). Warnings always go to stderr, never stdout —
this ensures JSON output is clean for piping. When writing to a file,
the summary line shows the JSON output path instead of the tarball
path.

### fleet init Command

```
inspectah fleet init <directory> [flags]
```

Scans a directory of tarballs and generates a `fleet.toml` manifest.
Reads tarballs to extract hostnames and autodetect baseline images.
Produces a commented, self-documenting TOML file.

**Behavior:**

- Writes `fleet.toml` in the current directory by default
- Refuses if `fleet.toml` already exists (unless `--overwrite`)
- `--output <path>` to change the output filename/location
- Baseline image conflicts reported on stderr with distribution
  summary; most common image written to manifest
- Summary on stderr: `wrote fleet.toml (12 sources, baseline:
  rhel-bootc:9.6)`
- Invalid tarballs warned on stderr, not included in manifest

**Source path normalization:** Generated `sources` entries are relative
to the manifest file's parent directory. When the manifest is written
to the current directory (default) and the scanned directory is
`./web-servers/`, sources are written as `web-servers/host1.tar.gz`.
When `--output /other/path/fleet.toml` is used, sources are rewritten
relative to `/other/path/`. The manifest is portable — it works from
any CWD as long as the relative paths resolve.

**Generated manifest includes comments explaining each field** — the
manifest is scaffolding that teaches the format.

**Where it lives:** `inspectah-cli/src/commands/fleet.rs`, following the
existing CLI command pattern.

## Output: Fleet Tarball

The fleet tarball inherits the scan tarball contract. It is a complete,
inspectable, buildable draft artifact produced by the same pipeline
single-host scan uses, plus one fleet-specific filesystem addition
(`fleet/variants/`). Interactive review via `inspectah refine` is the
intended sign-off step but is blocked today by the tarball provenance
gate (see Refine Import: Three Layers).

### File set

The fleet tarball inherits the **scan tarball contract** — the same
archive structure `inspectah scan` produces. Fleet aggregate builds
the tarball using the same pipeline:

1. Save `inspection-snapshot.json` (merged snapshot)
2. Call `render_all()` (shared renderer — Containerfile, config tree,
   reports, kickstart, etc.)
3. Package into a `.tar.gz` with a prefixed archive root

Fleet aggregate adds exactly **one filesystem addition** on top of
the scan tarball contract:

- **`fleet/variants/`** — non-selected content variants, organized
  by path, content-addressed by 8-char SHA-256 prefix. Only present
  when content variants exist (items with
  `VariantSelection::Alternative`).

Fleet aggregate also prepends a draft header comment to the
renderer-produced Containerfile (see Containerfile section below).
This is a content modification to an existing file, not a new file.

No other differences exist between a fleet tarball and a scan tarball
at the file-set level. The renderer sees the merged snapshot (with
`Selected` variants and `include: true` on all items) and
materializes the same artifacts it would for any snapshot.

### Archive root naming

The archive root is a single prefixed directory:
- Default: `inspectah-fleet-{label}-{datestamp}`
- With `--output-file custom.tar.gz`: archive root is `custom`
  (filename stem without `.tar.gz`)
- With `--output-dir`: archive root name unchanged, tarball written
  to the specified directory

### Build tree

The main directory tree (`etc/`, `usr/`, etc.) contains only the
`Selected` variant's content for each path. This is what
`podman build` would use — the Containerfile's `COPY` directives
reference these paths.

### fleet/variants/

Non-selected content variants stored by content hash, organized by
path. The snapshot JSON is the source of truth for which hosts map to
which variant and which variant is selected. The files are for human
inspection and potential swap during refine.

Content-addressed, not host-addressed. 200 hosts with the same config
content produce 1 file, not 200. Variant filenames use the first 8
characters of the SHA-256 content hash (e.g., `a1b2c3d4.conf`).

### Containerfile

The rendered Containerfile includes a draft header comment:

```dockerfile
# Fleet aggregate: web-servers (50 hosts)
# This is a draft — review before use
# Baseline: registry.redhat.io/rhel9/rhel-bootc:9.6 (auto-selected, provisional)
```

When `baseline_provisional` is true, the header explicitly notes this.
Prevalence information is included as inline comments on relevant lines.

### Audit report

The Rust renderer already produces `audit-report.md` via
`render::audit`. For fleet aggregate, the audit report includes a
fleet summary section noting:
- Host count and hostname list
- Baseline selection method (manifest override vs. auto-selected) and
  provisionality status
- Section coverage (which sections had data from how many hosts)
- Variant conflicts (paths with multiple content versions, which was
  auto-selected)

This may require fleet-aware additions to the existing audit renderer.
The base audit content is produced by the shared renderer; fleet
metadata augments it.

### Naming

`inspectah-fleet-{label}-{datestamp}.tar.gz`. Label from manifest,
directory name, or `fleet` as fallback.

## Known Limitations

### Cross-distro Containerfile validity

If a fleet aggregates tarballs from different OS minor versions (e.g.,
RHEL 9.4 + 9.6), the rendered Containerfile may reference repo files or
package versions that don't align perfectly with all source hosts. This
is a rendering concern to address in Spec 2 (fleet refine) or the
Containerfile renderer — not an aggregate problem.

OS major version mixing (RHEL 8 + 9) is a hard error and does not
produce output.

### No threshold filtering at aggregate time

The Go fleet command has a `--min-prevalence` flag that sets
`include=false` for items below a threshold during merge. The Rust
version moves this decision to the refine layer, where the user can
make interactive inclusion decisions with full context. All items are
`include: true` after aggregate.

### Architect integration

The fleet tarball format is designed to be consumable by the future
architect command (Spec 3), but no architect-specific metadata or
structure is included in this spec. Architect's needs will be addressed
in its own spec after fleet aggregate and refine are shipped.

### NonRpmItem variant support

`NonRpmItem` has a `content` field but full variant support
(VariantSelection enum, content-addressed storage in `fleet/variants/`)
is deferred. Round 1 uses content from the most prevalent host. This
can be added in a follow-up if needed.

## Brainstorm Team

Design input from:
- **Ember** — product strategy: adoption friction, manifest design,
  "generate then edit" pattern, render-on-aggregate, prevalence
  denominator
- **Fern** — interaction design: CLI UX patterns, output formatting,
  validation presentation, fleet init behavior, missing-data indicators
- **Tang** — architecture: trait design, variant modeling,
  VariantSelection enum, module layout, ownership model, data model
  review, snapshot contract
- **Collins** — domain: package identity (name.arch), baseline contract,
  snapshot-level field merging
- **Thorn** — behavioral testing: missing data semantics, prevalence
  truthfulness, variant selection edge cases
- **Mango** — documentation: CLI contract, fleet init normalization,
  --json-only behavior

## Review History

### Round 1

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes.

**Must-fix items addressed in round 2 revision:**
1. Snapshot contract pinned: `fleet_meta` field on InspectionSnapshot,
   schema version bump, existing field merge rules defined
2. Per-section coverage table with canonical `name.arch` package
   identity, explicit non-key field resolution
3. Global denominator rule with section-level host counts for Spec 2
4. CLI contract: fleet init path normalization, --json-only behavior
   table, exact tarball file set

**Should-fix items addressed:**
1. Provisional choices (baseline, variant selection) persisted in
   `FleetSnapshotMeta.baseline_provisional` and documented as
   deterministic defaults, not operator intent
2. Deterministic ordering rules for all serialized fleet collections

### Round 2

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes
(Tang/Thorn block, Collins needs-revision, Mango approve-with-nits).

**Must-fix items addressed in round 3 revision:**
1. Determinism claim narrowed to input-order independence (not
   byte-identical across runs due to `merged_at`).
   `section_host_counts` changed from `HashMap` to `BTreeMap`.
2. `rpm.baseline_package_names` derived from selected baseline only,
   not unioned across all inputs. Added `baseline_suppressed`,
   `no_baseline`, `base_image` to the RPM field merge table, all
   aligned to the selected baseline truth.
3. Full trust/re-import contract added: `redaction_state`,
   `redaction_hints`, `completeness` carrier defined. Refine
   compatibility section rewritten to be honest about what works
   today vs. what needs Spec 2 enhancements.
4. Fleet tarball explicitly defined as a new contract extending scan.
   Files classified as required vs. conditional. Archive-root naming
   under `--output-file` pinned.

**Should-fix items addressed:**
1. Coverage table expanded: `ModuleStream`, `VersionLockEntry` added.
2. Baseline tie behavior defined (lexicographic image ref).
3. Warnings explicitly stay on stderr in `--json-only` stdout mode.

### Round 3

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes
(Tang/Thorn block, Collins approve-with-nits, Mango request-changes).
Determinism and baseline-truth blockers closed.

**Must-fix items addressed in round 4 revision:**
1. Refine compatibility contradiction resolved: removed the
   contradicting "Refine Compatibility" section. Consolidated into
   "Deserialization vs. Fleet-Aware Display" — honestly separates
   serde-layer compatibility (works now) from fleet-aware UX (needs
   Spec 2). No more claiming compatibility without refine-side changes.
2. Trust-sensitive fields completed: added `sensitive_snapshot`,
   `preserved_credentials`, `preserved_ssh_keys` to the snapshot
   merge table with conservative OR semantics. Fixed `completeness`
   to use actual enum variants (`Complete`, `Partial`, `Incomplete`)
   with correct field structures.
3. Tarball contract made testable: exhaustive file set table with
   REQUIRED / CONDITIONAL categories. Contract defined by enumerated
   file set, not by "whatever the renderer happens to produce."

### Round 4

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes.
Determinism, baseline-truth, and trust-field blockers closed.

**Must-fix items addressed in round 5 revision:**
1. Refine import three-layer model: honestly separates serde
   deserialization (works), tarball provenance gate (BLOCKS — current
   `from_tarball()` hard-rejects non-FullyRedacted, fleet produces
   None), and fleet-aware display (Spec 2). No more overclaiming
   current refine compatibility.
2. Tarball file set corrected: the Rust renderer DOES produce
   audit-report.md, README.md, report.html, secrets-review.md, and
   kickstart-suggestion.ks. All now listed as REQUIRED. Added
   `users/home/` SSH key staging as CONDITIONAL. `fleet/variants/`
   documented as the only fleet-specific addition to the standard
   renderer output.
3. users_groups confirmed in round-1 scope per Mark's direction.

### Round 5

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes.

**Must-fix items addressed in round 6 revision:**
1. Provenance gate corrected: current `from_tarball()` accepts
   `FullyRedacted`, `PartiallyRedacted`, and `SensitiveRetained`;
   rejects `None`, `Raw`, and `Unknown`. Fleet produces `None` →
   blocked. Removed the `→ inspectah refine` nudge from CLI output.
2. Tarball contract: stopped enumerating renderer internals. Fleet
   inherits the scan tarball contract via `render_all()` and adds
   exactly one thing: `fleet/variants/`. The scan contract is the
   authoritative reference for the renderer's file set; this spec
   does not re-enumerate or filter it.

### Round 6

Reviewers: Tang, Collins, Thorn, Mango. Verdict: request-changes.

**Must-fix items addressed in round 7 revision:**
1. Stale redaction/import prose: fixed `redaction_state` row in
   snapshot field table (now points to Three Layers), fixed Refine
   Trust Contract bullet (now says current import rejects, not
   re-runs redaction), fixed Containerfile header (removed
   `inspectah refine` reference).
2. Tarball contract seam: one boundary now — fleet inherits the scan
   tarball contract (same pipeline: save snapshot, render_all,
   package). Fleet adds one filesystem addition (`fleet/variants/`)
   and one content modification (Containerfile draft header). No
   competing contract definitions.
