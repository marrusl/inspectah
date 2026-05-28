# Fleet Refine Engine Spec

Spec 1 of 2 for fleet refine. This spec covers the Rust backend: zone
classification, fleet-aware attention scoring, variant operations, diff
engine, session persistence, and export. Spec 2 (UI) will be written
after this spec is implemented, built against the real API contract.

## Overview

Fleet refine adds interactive refinement to fleet aggregate output. A
sysadmin runs `inspectah fleet aggregate` to merge N host scans into a
single fleet tarball, then runs `inspectah refine` on that tarball to
interactively review and adjust what goes into the final container
image. The refine engine detects fleet mode automatically from snapshot
metadata — no separate command.

### Phasing Context

Fleet work is three phases:
- **Phase 1 (shipped):** Fleet aggregate — merge engine, CLI, tarball
  output
- **Phase 2 (this spec):** Fleet refine engine — Rust backend for
  interactive fleet refinement
- **Phase 3 (future):** Fleet refine UI — web frontend consuming this
  spec's API contract

### Spec Boundary

This spec owns:
- **Engine types:** `PrevalenceZone`, `FleetAttention`, `AttentionScore`,
  `ItemId`, `ContentHash`, `RefineMode`, `FleetContext`, variant op
  types, diff types, session persistence types
- **Engine logic:** zone classification, attention scoring, variant op
  execution, diff computation, auto-save, session resume, export
  projection
- **Behavioral contracts:** sort order, undo/redo semantics, content
  collision rules, source-of-truth rules, export round-trip guarantees

This spec does NOT own:
- **HTTP handlers:** endpoint wiring, request parsing, response
  serialization. Spec 2 defines the wire format (JSON field names,
  query string encoding, response shapes).
- **UI rendering:** component layout, interaction patterns, visual
  design.
- **API endpoint signatures:** the API Contract Summary section below
  describes the engine's capabilities in terms of what data it can
  produce, not the HTTP surface. Spec 2 pins the actual endpoint
  paths, query parameters, and JSON schemas.

The seam between specs is the engine's public Rust API — the functions
and types that `inspectah-web` handlers call. Spec 2 will reference
this spec's types by name and define how they serialize to JSON.

## Architecture

### Crate Changes

**inspectah-core** (minimal additions):
- `PrevalenceZone` enum added to `types/fleet.rs`
- `classify_zone()` pure function added to `fleet/mod.rs`

**inspectah-refine** (bulk of the work):
- `types.rs` — three new `RefinementOp` variants: `SelectVariant`,
  `EditVariant`, `DiscardVariant`
- `session.rs` — `RefineMode` enum, `AttentionScore` enum,
  `FleetContext` struct
- `autosave.rs` — new module for session persistence
- `fleet/` submodule:
  - `attention.rs` — fleet-aware attention scoring
  - `variant_ops.rs` — variant operation execution logic
  - `diff.rs` — LCS-based variant diffs via `similar` crate

**inspectah-web** (Spec 2 territory):
- Handler wiring, endpoint paths, JSON wire format, and response
  shapes are defined by Spec 2. This spec provides the engine types
  and functions that handlers call.

### Module Layout

```
inspectah-core/src/
  types/fleet.rs        + PrevalenceZone enum
  fleet/mod.rs          + classify_zone() function

inspectah-refine/src/
  types.rs              + SelectVariant, EditVariant, DiscardVariant ops
                        + ItemId enum, ContentHash newtype
                        + AttentionScore enum
  session.rs            + RefineMode enum, FleetContext struct
                        + auto-detect logic at session init
  attention.rs            (existing single-host scoring, unchanged)
  session.rs            + render_refine_export() extended for variant-aware export
  autosave.rs           + session persistence (new)
  fleet/
    mod.rs              + re-exports
    attention.rs        + fleet-aware attention scoring
    variant_ops.rs      + SelectVariant, EditVariant, DiscardVariant
    diff.rs             + LCS diff via similar crate
```

### Data Flow

```
fleet tarball
  → RefineSession::new()
  → auto-detect Fleet mode from snapshot metadata
  → classify zones (pure function, once at init)
  → score attention (prevalence-first, zones + tiers)
  → serve via API

user action
  → RefinementOp
  → apply to session state
  → auto-save to disk
  → re-score attention
  → updated API response

export
  → project user decisions onto snapshot (include/exclude + VariantSelection)
  → call render_refine_export() (existing focused export pipeline)
  → materialize fleet/variants/ if variant data exists
  → package as .tar.gz
```

## Zone Classification

### PrevalenceZone Enum

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
         Serialize, Deserialize)]
pub enum PrevalenceZone {
    Divergent,      // <50% of hosts
    NearConsensus,  // 50-99% of hosts
    Consensus,      // 100% of hosts (strict intersection)
}
```

Variant declaration order defines `Ord`: Divergent < NearConsensus <
Consensus. Derived `Ord` on enums uses declaration order — sorting a
`Vec<PrevalenceZone>` produces Divergent-first ordering automatically.

### Classifier Function

```rust
/// Classify an item's prevalence into a zone.
///
/// Fixed boundaries, no threshold parameter:
/// - count == total → Consensus
/// - count * 2 >= total → NearConsensus
/// - otherwise → Divergent
pub fn classify_zone(prevalence: &FleetPrevalence) -> PrevalenceZone {
    if prevalence.count == prevalence.total {
        PrevalenceZone::Consensus
    } else if prevalence.count * 2 >= prevalence.total {
        PrevalenceZone::NearConsensus
    } else {
        PrevalenceZone::Divergent
    }
}
```

Lives in `inspectah-core::fleet` alongside the existing merge engine.
Pure function — no side effects, no I/O.

Uses `count * 2 >= total` instead of `count >= total / 2` to avoid
integer division rounding issues.

### Zone Semantics

- Zones classify **item prevalence**, not item importance
- Variants are a **separate dimension** — a 100%-prevalent config file
  with 3 content variants is Consensus (all hosts have it). The
  variants need review, but the zone classification is correct.
- Zones are computed **once** at `RefineSession::new()` and stored in
  `FleetContext.zones`. No reclassification during the session.
- No threshold tunables in v1. Fixed boundaries are opinionated and
  non-adjustable. If real users request tuning, add CLI flags on
  `fleet aggregate` (not refine-time controls).

### Edge Cases

- **Fleet of 2:** `RefineMode::Fleet` with `zones_active: false`.
  Zones suppressed because two hosts is a diff, not a prevalence
  distribution. Variant ops remain available.
- **Fleet of 3+:** `RefineMode::Fleet` with `zones_active: true`.
  Full zone presentation.
- **Single-host snapshot (no `FleetSnapshotMeta`):**
  `RefineMode::SingleHost`. No fleet context, no variant ops.
- **Items with `fleet: None`:** Should not exist in a fleet snapshot.
  Log `tracing::warn!` with the item ID. These items are excluded
  from the zone map — `FleetContext.zones` has no entry for them.
  Callers that look up a missing zone should treat it as unclassified
  and sort it last (after Consensus). This avoids silently promoting
  data bugs into the Consensus tier.

## Fleet-Aware Attention Scoring

### Types

```rust
pub struct FleetAttention {
    pub zone: PrevalenceZone,       // primary sort axis
    pub attention: AttentionLevel,  // secondary sort axis
    pub prevalence: u32,            // tertiary sort (raw count)
}
// Implements Ord encoding the full sort contract.
// Identity key tiebreaker applied at the call site.

pub enum AttentionScore {
    SingleHost(AttentionLevel),     // true single-host (no FleetSnapshotMeta)
    Fleet(FleetAttention),          // fleet-of-2+ (zones_active false for 2, true for 3+)
}
```

`AttentionLevel` must derive `Ord` with variants declared in severity
order (needs-review first). `FleetAttention` implements `Ord` manually,
composing zone → attention → prevalence.

### Sort Contract

Deterministic, stable, four levels:

1. **Zone:** Divergent → NearConsensus → Consensus
2. **Within zone:** AttentionLevel (needs-review first)
3. **Within zone + attention:** prevalence ascending (rarest first —
   items on fewer hosts sort before items on more hosts)
4. **Tiebreaker:** alphabetical by identity key

The identity key tiebreaker is NOT part of `FleetAttention` — it comes
from the item's `ItemId`, applied at the sort call site as
`sort_by_key(|item| (item.fleet_attention, item.identity_key()))`.

### Scoring Rules

- Zone: lookup from `FleetContext.zones` (precomputed at init)
- AttentionLevel: existing single-host scoring logic applied to the
  merged item (same rules, same code path)
- Prevalence: `fleet.count` from the item's `FleetPrevalence`

### Variant Count

`variant_count: u16` is NOT part of `FleetAttention`. It is computed
at the API serialization boundary from the item's variant data and
included in the view response. The engine provides the data; the UI
decides how prominently to surface it (badge, icon, etc.).

### FleetContext

```rust
pub struct FleetContext {
    pub fleet_meta: FleetSnapshotMeta,
    pub zones: HashMap<ItemId, PrevalenceZone>,
    pub total_hosts: usize,
    pub zones_active: bool,  // false for host_count < 3
}
```

Constructed at `RefineSession::new()` when fleet metadata is detected.
`zones_active` is `true` when `host_count >= 3`, `false` for fleets
of 1-2 hosts. Small fleets remain `RefineMode::Fleet` (they are fleet
snapshots with fleet data) — they do NOT collapse to `SingleHost`.
Zone presentation is suppressed but variant ops remain available.

### RefineMode

```rust
pub enum RefineMode {
    SingleHost,
    Fleet(FleetContext),
}
```

Auto-detected at session init by checking for `FleetSnapshotMeta` in
the snapshot. The session dispatches to fleet or single-host scoring
based on a match on this enum. No runtime booleans.

## Variant Operations

### New Types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "key")]
pub enum ItemId {
    // RPM section
    Package { name_arch: String },                    // "httpd.x86_64"
    Repo { path: String },                            // "/etc/yum.repos.d/epel.repo"
    ModuleStream { module_stream: String },           // "nodejs:18"
    VersionLock { name_arch: String },                // "kernel.x86_64"

    // Config section
    Config { path: String },                          // "/etc/nginx/nginx.conf"

    // Services section
    Service { unit: String },                         // "httpd.service" (unit name only, NOT unit:action)
    DropIn { path: String },                          // "/etc/systemd/system/httpd.service.d/override.conf"

    // Containers section
    Quadlet { path: String },                         // "/etc/containers/systemd/app.container"
    Compose { path: String },                         // "/opt/app/docker-compose.yml"

    // Network section
    NMConnection { path: String },                    // "/etc/NetworkManager/system-connections/eth0.nmconnection"
    FirewallZone { path: String },                    // zone file path

    // Kernel/boot section
    KernelModule { name: String },                    // "br_netfilter"
    Sysctl { key: String },                           // "net.ipv4.ip_forward"

    // Scheduled section
    CronJob { path: String },                         // "/etc/cron.d/backup"
    SystemdTimer { name: String },                    // "logrotate.timer"
    AtJob { file: String },                           // at job file
    GeneratedTimer { name: String },                  // generated timer unit name

    // SELinux section
    SelinuxPort { protocol_port: String },            // "tcp:8080"

    // Storage section
    Fstab { mount_point: String },                    // "/data"

    // Non-RPM section
    NonRpm { name: String },                          // "custom-tool"
}
```

**Identity alignment:** Every `ItemId` variant round-trips the exact
value returned by `FleetMergeable::identity_key()` for that type in
`inspectah-core/src/fleet/merge.rs`. The field names document the
format; the values are the canonical identity strings. This is the
single identity contract used across zone maps, variant ops, diff
targeting, and the future wire/API surface.

`ContentHash` is a newtype wrapping a hex-encoded SHA-256 digest.
Prevents accidentally passing a filename or item ID where a hash is
expected. The constructor validates that the string is exactly 64
hex characters; invalid inputs are rejected at parse time, not at
use time.

`ItemId` uses `#[serde(tag = "kind", content = "key")]` for consistent
JSON representation matching the existing `RefinementOp` pattern.

### Variant-Capable Types

Not all `ItemId` types support variant operations. Variant ops
(SelectVariant, EditVariant, DiscardVariant, diff) apply only to types
whose `FleetMergeable` implementation returns
`Some` from `content_variant_key()`:

| ItemId variant | Variant ops | identity_key() | content_variant_key() |
|---------------|-------------|----------------|----------------------|
| Config | yes | path | SHA-256 of `content` field |
| DropIn | yes | path | SHA-256 of `content` field |
| Quadlet | yes | path | SHA-256 of `content` field |
| Compose | **SelectVariant only** | path | SHA-256 of serialized `images` list |
| Repo | **no** | path | not implemented |
| Package | no | name.arch | — |
| Service | no | unit | — |
| ModuleStream | no | module_name:stream | — |
| VersionLock | no | name.arch | — |
| NMConnection | no | path | — |
| FirewallZone | no | path | — |
| KernelModule | no | name | — |
| Sysctl | no | key | — |
| CronJob | no | path | — |
| SystemdTimer | no | name | — |
| AtJob | no | file | — |
| GeneratedTimer | no | name | — |
| SelinuxPort | no | protocol:port | — |
| Fstab | no | mount_point | — |
| NonRpm | no | name | — |

**Compose limitation:** ComposeFile's variant key hashes the serialized
`images` list, not raw file content. Because the carrier is structured
(not plain text), EditVariant and diff are not supported for Compose
items in v1. Compose items support SelectVariant only — users can pick
between existing host-sourced compose variants but cannot edit or diff
them. EditVariant/diff for compose files requires defining a
serialize/parse/validate seam for the structured images carrier, which
is deferred to a future spec.

**Repo is NOT variant-capable.** RepoFile implements `FleetMergeable`
(it has `fleet` and `include` fields) but does NOT implement
`content_variant_key()` or `variant_selection_mut()`. Repo files get
zone classification and prevalence badges but not variant selection,
editing, or diffing.

Variant ops on non-variant-capable types return
`RefineError::NoVariants`.

### New RefinementOp Variants

```rust
// Added to the existing RefinementOp enum:

SelectVariant {
    item_id: ItemId,
    target: ContentHash,       // which variant to make active
}

EditVariant {
    item_id: ItemId,
    content: String,
    based_on: Option<ContentHash>,
}

DiscardVariant {
    item_id: ItemId,
    variant: ContentHash,
}
```

`SelectVariant` carries a `ContentHash` that names the concrete variant
to activate. `VariantSelection` is **derived state** computed from the
variant pool — it is never part of a request payload. When
`SelectVariant` is applied, the engine sets the target hash's variant
to `Selected` and derives `Alternative` for all others. When only one
variant remains after a discard, the engine derives `Only`.

### SelectVariant

Sets which content variant is active for an item with multiple versions
across hosts.

**Behavior:**
- Looks up the `target` ContentHash in the item's variant pool.
- Sets that variant to `Selected`, all others for the same identity
  key to `Alternative`.
- The previously-selected variant is stashed (logically moved to the
  variant pool). No content is deleted — all variants are preserved.
- On export, the selected variant's content goes into the config tree
  at its original path. All alternatives go to `fleet/variants/`.

**Validation:**
- Item must exist and have variants (`VariantSelection` is not `Only`).
  Error if the item has no variants.
- The `target` ContentHash must exist in the item's variant pool.
  Error with the hash value if not found.

### EditVariant

Creates a user-authored variant — either from scratch or by modifying
an existing variant.

**Behavior:**
- Stores the user-provided content as a new variant, content-addressed
  by SHA-256 of the content.
- Sets the new variant to `Selected`, demotes all existing variants to
  `Alternative`.
- The previously-selected variant is stashed (preserved, available for
  re-selection).
- `based_on: Some(hash)` records provenance — this variant was derived
  from an existing one. `None` means created from scratch.
- User-edited variants are flagged `edited: true` in the variant
  metadata to distinguish from host-sourced variants.

**Content collision:** If the user edits to produce content whose
SHA-256 matches any existing variant **for the same item** — whether
host-sourced or user-created from a prior edit — detect the collision
and promote the existing variant to `Selected` instead of creating a
duplicate. Do NOT add a new entry to `user_variants` for this item.
Log in the audit trail as "user edit converged with existing variant X."

Convergence checks the **full variant pool for the target item**:
host-sourced variants (from the snapshot) and user-created variants
(from prior `EditVariant` ops on the same item). This is the right
scope because variants are item-scoped — a hash match on a different
item is irrelevant.

**Undo on converged edit:** Since no new content was created, undo
restores the previous selection without removing any content from the
item's variant pool. This is the one case where an `EditVariant` undo
does NOT shrink the variant set.

**Validation:**
- Item must exist and have variants.
- If `based_on` is `Some`, the referenced hash must exist in the
  variant set. Error otherwise.

### DiscardVariant

Removes a specific user-created variant. Host-sourced variants cannot
be discarded.

**Behavior:**
- Removes the variant from the variant set.
- If the discarded variant was `Selected`, falls back to the
  **most-prevalent host-sourced variant** (the same deterministic
  selection the aggregate engine would have chosen). This is NOT
  undo-history-dependent — the fallback is always the aggregate
  default, regardless of what the user previously selected. This
  avoids coupling discard behavior to op-stack position.
- After discard, if the item is back to a single variant, its
  `VariantSelection` becomes `Only`.

**Invariant:** The set of **host-sourced** variants never shrinks.
User-created variants can be explicitly discarded. Undo on a discard
restores the variant and its previous selection state.

**Validation:**
- The variant must be user-created (`edited: true`). Error if
  attempting to discard a host-sourced variant.
- The variant must exist.

### Variant Position Model

In the in-memory session, variant position is logical, not physical:

```rust
enum VariantState {
    Active,
    Stashed,
}
```

The `fleet/variants/` directory structure is an **export-time concern**.
During the refine session, host-sourced variants live in the snapshot's
item structs and user-created variants live in the item-scoped
`user_variants` working map. The export pipeline materializes the
directory layout from both sources.

### Undo/Redo

All three variant ops participate in the existing undo/redo op stack.
The ops are stored as-is in the persisted `SessionState.ops` list.
In-memory undo metadata (previous selections, content backups) is
reconstructed by replaying the op journal on session load. See the
Undo Stack Contract in the Auto-Save section for the full replay
model and per-op undo semantics.

## Diff Engine

### Overview

Lives in `inspectah-refine::fleet::diff`. Uses the `similar` crate
(pure Rust, LCS-based, already a transitive dependency via
`cargo-insta`). The engine is the single source of truth for diffs —
the UI never computes diffs client-side.

### Types

```rust
pub struct DiffResult {
    pub base: ContentHash,
    pub target: ContentHash,
    pub hunks: Vec<DiffHunk>,
    pub stats: DiffStats,
}

pub struct DiffHunk {
    pub old_range: LineRange,
    pub new_range: LineRange,
    pub changes: Vec<DiffChange>,
}

pub struct DiffChange {
    pub kind: ChangeKind,   // Equal, Insert, Delete
    pub content: String,
}

pub struct DiffStats {
    pub total_changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}

pub struct LineRange {
    pub start: usize,
    pub count: usize,
}
```

`ChangeKind` maps directly to `similar::ChangeTag` (renamed for
clarity). Three variants: `Equal`, `Insert`, `Delete`.

### Core Function

```rust
pub fn compute_diff(
    base: &str,
    target: &str,
    context_lines: usize,  // default 3
) -> DiffResult
```

Pure function — takes strings, returns structs. No I/O, no side
effects. The handler resolves content hashes to actual content before
calling this function.

Uses `similar::TextDiff::from_lines()` → `grouped_ops(context_lines)`
→ convert to `DiffHunk` / `DiffChange` types. Context parameter trims
Equal runs to N lines around changes — full-file Equal runs waste
payload on large configs.

### Batch Diff Function

```rust
pub fn compute_batch_diff(
    base: &str,
    targets: &[(ContentHash, &str)],
    context_lines: usize,
) -> BTreeMap<ContentHash, Result<DiffResult, DiffError>>
```

Returns a keyed map of diff results. Per-target `Result` so one
missing or invalid input doesn't fail the whole batch. The natural
consumer model is "diff all against selected" — one call returns all
comparisons. Spec 2 decides how to expose this over HTTP.

### Error Cases

- **Hash not found:** Per-target error in the batch response (not a
  whole-request failure).
- **Identical content:** Return `DiffResult` with empty `hunks` and
  zero stats. Callers check `hunks.is_empty()`.
- **Binary content:** If either input contains null bytes, return
  `DiffError::BinaryContent`. Config files are text; binary is a data
  bug.
- **Very large files:** v1 guardrail: if either input exceeds 100KB,
  return `DiffError::InputTooLarge` instead of attempting the diff.
  Config files are typically under 10KB; 100KB is generous. `similar`
  is O(n*m) worst case on Myers diff, so a cap prevents pathological
  runtimes. The threshold is a const, easy to adjust if real users
  hit it.

### Character-Level Highlighting

Deferred to v2. `similar` supports `iter_inline_changes()` for
intra-line spans. When added, gate behind a `?inline=true` query
parameter so the default response stays lean. Config files have short
lines; line-level diffs are sufficient for v1.

### Variant Summary

Separate from diffs — a fleet-level overview of variant distribution.

```rust
pub struct VariantSummary {
    pub total_hosts: usize,
    pub paths_with_variants: usize,
    pub variant_distribution: BTreeMap<String, PathVariantInfo>,
}

pub struct PathVariantInfo {
    pub variant_count: usize,
    pub host_split: Vec<usize>,       // sorted descending
    pub differing_lines: usize,       // vs selected variant
}
```

Exposed via `variant_summary(session)` engine function. Spec 2 decides
the HTTP surface (dedicated endpoint vs. field on fleet metadata
response). Computed from the existing `FleetPrevalence` and
content-variant grouping — the data is already in the snapshot.

### Content Agnostic

The diff engine operates on any text content. It works for config
files, systemd drop-ins, quadlet units, compose files, and repo files
without format-specific logic. Syntax-aware diffing and significance
classification are deferred to future work.

## Auto-Save & Session Persistence

### Trigger

Every cursor-changing mutation triggers an auto-save: `apply()`,
`undo()`, and `redo()`. Synchronous with the mutation — no timer-based
saves. The persisted cursor always matches the user's last visible
state. Without this, undo/redo would change visible state without
saving, and resume could reopen a state the user already backed out of.

### Session File

**Location:** next to the source tarball.
**Filename:** `.inspectah-session-<tarball-stem>.json`

Example: refining `fleet-web-servers-2026-05-20.tar.gz` produces
`.inspectah-session-fleet-web-servers-2026-05-20.json` in the same
directory.

Multiple fleet sessions can coexist — each tarball gets its own session
file. Clearable with `rm .inspectah-session-*`.

### Persisted State

```rust
pub struct SessionState {
    pub schema_version: u32,          // always 1; reject unknown versions
    pub tarball_path: PathBuf,
    pub tarball_hash: ContentHash,    // for stale detection
    pub ops: Vec<RefinementOp>,
    pub cursor: usize,                // undo/redo position
    pub saved_at: String,             // ISO-8601 timestamp
}
```

**The op journal is the sole durable content source.** `EditVariant`
ops carry their `content: String` inline. There is no separate
`user_variants` map in the persisted state. During replay, the engine
encounters `EditVariant` ops and populates an in-memory working store.

**The working store is item-scoped:**
`user_variants: HashMap<ItemId, HashMap<ContentHash, String>>`

Content is keyed by `(ItemId, ContentHash)`, not by `ContentHash`
alone. This means:
- If two items happen to have identical edited content (same hash),
  they maintain independent entries.
- Undoing an edit on item A removes `(A, hash)` without affecting
  item B's `(B, hash)`.
- No reference counting needed. Each item's variant pool is
  independent.

**NOT persisted:** `user_variants` (reconstructed from op replay),
computed views (`RefinedView`, zone classifications, diff results),
undo metadata (previous selections, content backups).

### Source of Truth Rules

The session has one durable state artifact: the op journal
(`SessionState.ops`). Everything else is derived.

1. **Host-sourced variant content** lives in the snapshot JSON (loaded
   from the tarball). The session file does NOT duplicate it.
2. **User-created variant content** is carried inline in `EditVariant`
   ops in the op journal. On replay, the engine hashes each op's
   `content` field and populates the item-scoped `user_variants` map.
   This map is working state, not persisted separately. Content is
   keyed by `(ItemId, ContentHash)` so each item's variants are
   independent.
3. **On resume:** the engine loads the snapshot (host-sourced content),
   then replays the op journal up to `cursor`. Each `EditVariant`
   replay hashes the op's content and checks the target item's full
   variant pool (host-sourced + already-replayed user-created). If
   the hash already exists (convergence), no insertion — the existing
   variant is promoted. Otherwise, the content is inserted into
   `user_variants[item_id][content_hash]`. Each `DiscardVariant`
   replay removes from `user_variants[item_id]`. Each `SelectVariant`
   replay updates selection state. After replay, the session is fully
   reconstructed.
4. **On export:** the in-memory `user_variants` content is merged into
   the snapshot and written to the tarball. After export, the
   distinction is lost (see "What Survives Export" above).

### Undo Stack Contract

The undo stack is an ordered list of `RefinementOp` values with a
cursor position. Undo moves the cursor backward; redo moves it forward.
Ops after the cursor are discarded when a new op is applied.

Each op is persisted as-is in `SessionState.ops`. On resume, ops are
replayed in order up to `cursor`. No separate undo metadata is stored
— the op values themselves contain enough information to reverse:

- **Toggle (existing):** the op records the item_id. On undo, the
  engine reads the item's current `include` flag and flips it.
- **SelectVariant:** the op records `item_id` and `target` hash. On
  undo, the engine must also know the previous selection. This is
  computed during replay: when replaying forward, the engine records
  `previous_selected_hash` in an in-memory undo metadata map. This
  map is NOT persisted — it is reconstructed from replay.
- **EditVariant (normal):** the op records `item_id`, `content`, and
  `based_on`. On forward replay, the engine hashes the content and
  adds it to `user_variants[item_id]`. On undo, the engine removes
  the hash from `user_variants[item_id]` (that item's variant pool
  shrinks) and restores previous selection. Other items' pools are
  unaffected even if they share the same content hash.
- **EditVariant (converged):** if the content hash matches any existing
  variant for that item (host-sourced OR user-created from a prior
  edit), no new entry is added to `user_variants[item_id]`. On undo,
  no content is removed — only the previous selection is restored.
  The engine detects convergence by checking the item's full variant
  pool. This is consistent: the variant pool only shrinks when
  removing content that was added by the same op.
- **DiscardVariant:** the op records `item_id` and `variant` hash. On
  forward replay, the engine removes the hash from
  `user_variants[item_id]` and saves a content backup in the
  in-memory undo metadata map. On undo, the engine re-inserts the
  content into `user_variants[item_id]` and restores selection.

**Key principle:** `SessionState.ops` is the persisted journal.
In-memory undo metadata (previous selections, content backups for
discard) is reconstructed by replaying the journal forward. This
avoids storing redundant metadata that could drift from the ops.

### Content Disjointness (per item)

For each item, variant content exists in two pools: host-sourced (from
the snapshot) and user-created (in the item-scoped `user_variants`
working map). Within a single item, these pools are disjoint by
content hash:

- **EditVariant convergence** (user content matches any existing
  variant for the same item — host-sourced or user-created): the
  engine promotes the existing variant instead of adding a new entry.
  No duplicate content stored for that item.
- **Cross-item independence:** Two different items may each have a
  user-created variant with the same content hash. This is not a
  collision — each item's working pool is independent (`user_variants`
  is keyed by `(ItemId, ContentHash)`).
- **On resume replay:** when replaying an `EditVariant` op whose
  content hash matches an existing variant for that item, the engine
  detects convergence and skips adding to the item's working pool.
  The op journal preserves the content string for replay fidelity.

### Atomic Writes

Write to a temp file in the same directory, then rename. Guarantees
atomicity on POSIX — no corruption if the process is killed mid-write.

### Autosave Failure Policy

`apply()` has two phases: apply the op to in-memory state, then
auto-save to disk. Three outcomes:

| Outcome | In-memory | Disk | Generation | Behavior |
|---------|-----------|------|------------|----------|
| **committed-and-saved** | op applied | session file updated | advanced | Normal path. API returns success. |
| **committed-but-unsaved (transient)** | op applied | session file NOT updated | advanced | Op succeeds. API returns success with durability warning. Next op retries the save. Export is eligible (from in-memory state). |
| **committed-but-unsaved (permanent)** | op applied | session file NOT updated | advanced | Op succeeds. API returns success with durability warning. `durability_degraded` flag set; all subsequent ops skip save (see permanent classifier below). Export is eligible. |
| **rejected** | no change | no change | unchanged | Op validation failed (e.g., item not found, invalid hash). API returns error. |

**Generation tracks in-memory state, not durable state.** The
generation counter advances on any successful `apply()`, regardless
of save outcome. This matches the existing refine contract where
`POST /api/tarball` uses generation to detect stale exports.

**Read-only filesystem:** When the first save attempt fails with
`EROFS` or `EACCES`, the engine classifies the failure as permanent
and sets `durability_degraded: bool` on the session. All subsequent
ops **skip the save attempt entirely** — no retries, no per-op I/O
overhead. The flag persists for the session lifetime. The engine
includes `durability_degraded` in health/status data so the UI can
show a persistent warning. Transient I/O errors (e.g., disk full)
do NOT set the permanent flag — those retry on the next op.

### Session Resume

When `inspectah refine` opens a tarball with an adjacent session file:

1. **Stale detection:** Compare stored `tarball_hash` with current
   tarball content hash. If mismatched:
   ```
   Tarball has changed since this session was saved.
   Session may not apply cleanly.
     [f] Fresh start   [r] Resume anyway   [q] Quit
   ```
   Default to `[f]` — stale resumes are the riskier path.

2. **Normal resume prompt:**
   ```
   Saved session found (12 ops applied, last modified 2h ago)
     [r] Resume   [f] Fresh start   [q] Quit
   ```
   One keystroke, no typing. The parenthetical gives enough context
   to decide.

3. **Reconstruction:** Load the original snapshot, replay the op stack
   up to the cursor position. Recompute zones and attention scores.
   Guarantees consistent state — no stale caches.

### CLI Flag

`--fresh` skips the resume prompt and starts a clean session.
Destructive: confirm before discarding a saved session:
```
Discard saved session? This cannot be undone. [y/N]
```

### Read-Only Filesystem

If the session file write fails with `EROFS` or `EACCES`, warn:
```
Can't auto-save — filesystem is read-only.
Changes won't persist across restarts.
```
Do not silently fall back to an alternate location. The user should
know they lose persistence.

### Export Interaction

"Export Snapshot" produces the tarball. The session file remains in
place — the user can continue refining after export. The session file
is only removed by explicit user action (deleting it, or `--fresh`).

## Export & Build Output

### Contract Relationship to Existing Refine

This spec **extends** the existing refine export/import contract. It
does not replace it.

#### Import Contract

Same entry point: `RefineSession::new()` reads
`inspection-snapshot.json` from an extracted tarball working directory.

Fleet mode is auto-detected by checking for `FleetSnapshotMeta` in
the snapshot JSON. If present, the session constructs `FleetContext`
(zone map, fleet metadata). If absent, single-host mode. No new
loader — the same code path handles both.

**`inspection-snapshot.json` is the sole authoritative artifact on
import.** The snapshot JSON carries all variant content inline (on the
item structs, same as aggregate output). The `fleet/variants/`
directory is NOT read by the loader — it is a materialized convenience
for downstream file-based consumers (e.g., `podman build`). If
`fleet/variants/` is missing, the tarball is still valid and fully
importable.

**Session files are not part of the artifact contract.** The
`.inspectah-session-*.json` file is a local convenience for resuming
refine sessions. It is never read by import, by `architect`, or by
any consumer other than `inspectah refine` itself on the same machine.

#### Export Contract

Same entry point: `render_refine_export()`. This is NOT `render_all()`
— it is the existing focused export pipeline that produces the tested
refine tarball file set. Fleet refine extends this pipeline with one
additive step: materializing `fleet/variants/` from the variant data
already in the snapshot JSON.

The projection step applies user decisions to the snapshot:
- Include/exclude flags on items
- `VariantSelection` values on variant-capable items
- User-created variant content merged into the snapshot's item structs

After projection, `render_refine_export()` produces its standard file
set. The fleet extension adds `fleet/variants/` alongside (if variant
data exists). Single-host export is completely unaffected.

#### Fleet Tarball File Set

A fleet refine tarball contains the `render_refine_export()` file set
plus:

| Path | Required | Source |
|------|----------|--------|
| `inspection-snapshot.json` | required | projected snapshot with decisions applied |
| `fleet/variants/<path>/<hash-prefix>` | optional | materialized alternative variant content |

`fleet/variants/` is optional because the snapshot JSON is
authoritative. The directory exists for file-based consumers that need
variant content at filesystem paths. Its absence is not an error.

### Tarball Equivalence

All tarballs are structurally equivalent. A fleet aggregate output and
a fleet refine export are the same format. There is no "build-only"
variant. The future `architect` command accepts any tarball without
knowing whether it was refined.

The sole authoritative artifact on load is
`inspection-snapshot.json`. The snapshot carries all variant content
inline on item structs, `VariantSelection` values reflecting current
state, and `fleet_meta` for fleet metadata. `fleet/variants/` is a
materialized convenience directory — it is NOT read on load and its
absence is not an error. All other files (Containerfile, config tree,
reports) are derived from the snapshot and can be regenerated.

### What Export Does

1. Project user decisions onto the snapshot: include/exclude flags and
   `VariantSelection` enum values on items with variants. Merge
   user-created variant content from `user_variants` into the
   snapshot's item structs.
2. Call `render_refine_export()` — the existing focused export pipeline
   that produces the tested refine tarball file set.
3. Materialize `fleet/variants/` with alternative variant content,
   content-addressed by SHA-256 prefix (additive fleet extension).
4. Package as `.tar.gz`.

### Variant Handling on Export

- **Selected variant:** Content placed at the original path in the
  config tree. This is what the Containerfile COPY's.
- **Alternative variants:** Content materialized in `fleet/variants/`,
  content-addressed by SHA-256 prefix. The snapshot JSON remains the
  authoritative source; this directory is a convenience for
  file-based consumers.
- **User-edited variants:** Content placed identically to host-sourced
  variants in the config tree / variants directory. Content is
  indistinguishable from host-sourced content after export.

### What Survives Export (and What Does Not)

The exported tarball preserves:
- All variant **content** (selected and alternatives)
- Current **selection state** (`VariantSelection` enum values in the
  snapshot JSON)
- **Host attribution** on host-sourced variants (via `FleetPrevalence`
  on items)

The exported tarball does **NOT** preserve:
- Whether a variant is user-authored or host-sourced (`edited: true`
  flag is session-only)
- Derivation chain (`based_on` provenance is session-only)
- Whether selection was auto-selected or operator-confirmed
  (session-only)
- Discard eligibility (all variants in an export are permanent —
  the user-created/host-sourced distinction is lost)
- Op history (undo/redo stack is session-only)

**Rationale:** The tarball is a portable workbench artifact, not an
audit trail. Provenance metadata lives in the session file
(`.inspectah-session-*.json`) as a local convenience for the refine
workflow only. Session files are never consumed by `architect`, by
import, or by any downstream tool. The future `architect` command
reads only the tarball — it never looks for session files.

### Audit Report

The fleet export audit report includes host-attribution data that IS
in the snapshot:

- **Host attribution:** Which hosts had this item. "httpd.x86_64:
  present on web-01, web-02, web-03 (3/3 hosts)"
- **Variant listing:** Which variants exist and which is active.
  "Selected: variant A (web-01, web-02). Alternatives: variant B
  (db-01)"
- **Prevalence zone:** The item's zone classification at export time.

The audit report does NOT claim operator intent or provenance —
it reports content and selection state only.

### Re-Import

A fleet refine export tarball can be re-imported into a new refine
session. The session auto-detects fleet mode, reconstructs zone
classifications, and presents the previously-refined state for further
work.

**What re-import reconstructs:**
- All variant content (from snapshot JSON — the sole authoritative
  source; `fleet/variants/` is NOT read on import)
- Current selection state (from `VariantSelection` enum values)
- Zone classifications (recomputed from `FleetPrevalence` data)
- Attention scores (recomputed from zones + existing scoring rules)

**What re-import cannot reconstruct:**
- Op history — the new session starts with a clean undo stack
- User-authored vs host-sourced distinction — all variants in a
  re-imported tarball are treated as host-sourced
- Discard eligibility — all variants are permanent (no DiscardVariant
  on re-imported content)

This is intentional. Re-import starts a fresh session with the content
as-is. The editorial history belongs to the prior session.

## Engine Capabilities (for Spec 2 Consumption)

This section describes what the engine can produce. Spec 2 defines
the HTTP surface (endpoint paths, query parameters, JSON wire format,
serialization of these types).

### Engine Functions Available to Handlers

| Engine function | Returns | Purpose |
|----------------|---------|---------|
| `RefineSession::view()` | `RefinedView` with `AttentionScore` per item | Main view data including zone, prevalence, variant_count |
| `RefineSession::apply(op: RefinementOp)` | `Result<(), RefineError>` | Apply SelectVariant, EditVariant, DiscardVariant (+ existing ops) |
| `RefineSession::undo()` / `redo()` | `Result<(), RefineError>` | Undo/redo with variant-aware state restoration |
| `RefineSession::export()` | tarball bytes | Variant-aware export — all variants preserved |
| `compute_diff(base, target, context)` | `DiffResult` | Pairwise line-level diff between two variant contents |
| `compute_batch_diff(base, targets, ctx)` | `BTreeMap<ContentHash, Result<DiffResult, DiffError>>` | Multiple diffs against a base variant |
| `variant_summary(session)` | `VariantSummary` | Fleet-level variant distribution overview |
| `RefineSession::fleet_context()` | `Option<&FleetContext>` | Fleet metadata, zone map, zones_active flag (None only for true single-host snapshots without fleet_meta) |

### Fleet-of-1 and Fleet-of-2 Behavior

**Fleet of 2:** `RefineSession::fleet_context()` returns
`Some(&FleetContext)` with `zones_active: false`. Zone presentation
suppressed — Spec 2 should not render zone headers or variant summary.
Variant ops and diff remain available. `AttentionScore` is
`Fleet(FleetAttention)` with zone data present but flagged as
presentation-suppressed via `zones_active`.

**Single-host (no FleetSnapshotMeta):** `fleet_context()` returns
`None`. `AttentionScore` is `SingleHost(AttentionLevel)`. Standard
single-host view, no variant ops.

## Known Limitations

### Character-level diff highlighting

Deferred to v2. Line-level diffs are sufficient for config files with
short lines. When added, will be gated behind `?inline=true` query
parameter.

### Three-way merge

Pairwise comparison only. Comparing A vs B, A vs C, etc. No common-
ancestor three-way merge. Pairwise is the right pattern for config
variants — sysadmins compare each alternative against their chosen
version.

### Diff significance classification

No semantic classification of diffs ("numeric parameter change" vs
"structural config difference"). The engine returns raw line-level
diffs. Downstream consumers may annotate diffs with semantic meaning
in the future. The extension point is clean — add a post-processing
step on `DiffResult`.

### Zone threshold tunables

Fixed boundaries in v1 (100% / 50% / <50%). CLI flags on `fleet
aggregate` for adjustable thresholds (`--consensus-threshold`,
`--divergent-threshold`) will be added if real users request them.
Not a refine-time control.

### Syntax highlighting in diffs

No language-aware highlighting in diff output. The engine returns
plain text. Syntax highlighting is a UI rendering concern (Spec 2).

## Review History

### Round 1

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.

Five MUST-FIX themes addressed in revision:

1. **SelectVariant identity** — changed from `selection: VariantSelection`
   (state enum, can't name a target) to `target: ContentHash` (concrete
   variant identifier). `VariantSelection` is now derived state only.
2. **Export provenance claims narrowed** — export preserves content +
   selection state. Provenance metadata (user-authored, based_on,
   discard eligibility, selection intent) is session-file-only. Added
   explicit "What Survives Export" and "What Re-Import Reconstructs"
   sections.
3. **Export/import contract rewritten** — added "Contract Relationship
   to Existing Refine" section stating this spec extends (not replaces)
   the existing refine export/import contract. Defined authoritative
   artifacts on load.
4. **Autosave/undo/resume pinned** — added Source of Truth Rules (two
   disjoint sources: snapshot for host-sourced, user_variants for
   user-created). Added Undo Stack Contract with per-op state
   requirements. Added Content Collision During Resume.
5. **Phase 2 boundary clarified** — added Spec Boundary section. Engine
   owns types + logic + behavioral contracts. Spec 2 owns HTTP handlers
   + wire format + UI rendering. API Contract Summary rewritten as
   "Engine Capabilities" listing Rust functions, not HTTP endpoints.

### Round 2

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.

Three remaining blocker themes addressed:

1. **Export pipeline singularized** — explicitly stated: export uses
   `render_refine_export()`, NOT `render_all()`. Added import/export
   contract sections with exact authoritative artifact definitions.
   `inspection-snapshot.json` is the sole authoritative artifact on
   import. `fleet/variants/` is optional materialized convenience.
   Session files are explicitly NOT part of the artifact contract —
   they are local convenience only, never read by import, architect,
   or any downstream consumer.
2. **ItemId aligned to canonical identities** — every `ItemId` variant
   now round-trips the exact `FleetMergeable::identity_key()` value.
   Added Variant-Capable Types table explicitly listing which types
   support variant ops vs zone-only. Removed all identity drift.
3. **Replay/durability model pinned:**
   - Discard fallback: falls back to most-prevalent host-sourced
     variant (aggregate default), NOT undo-history-dependent.
   - Converged-edit undo: no content to remove, restores previous
     selection only. Explicitly documented as the one EditVariant
     undo case that does not shrink the variant pool.
   - Autosave failure policy: three explicit outcomes table
     (committed-and-saved, committed-but-unsaved, rejected) with
     generation, export eligibility, and UI warning behavior defined.
     Generation tracks in-memory state. `durability_degraded` flag
     for persistent read-only filesystem warning.

### Round 3

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.

Three narrower blocker themes addressed:

1. **Contradiction scrub (the biggest remaining blocker):**
   - Data Flow section: `render_all()` → `render_refine_export()`
   - What Export Does section: `render_all()` → `render_refine_export()`
   - Tarball Equivalence: removed `fleet/variants/` as authoritative
     on load — snapshot JSON is the sole authority
   - Re-Import: removed `fleet/variants/` as a load source
   - What Survives Export: removed "architect reads session file" —
     session files are local convenience only, never consumed by
     architect or any downstream tool
2. **Identity/scope corrections:**
   - Service identity: `unit_action` → `unit` (matches current
     `FleetMergeable::identity_key()` which returns unit name only)
   - Repo: removed from variant-capable list (does NOT implement
     `content_variant_key()` or `variant_selection_mut()`)
   - Compose: added structured-carrier caveat (variant key hashes
     serialized `images` list, not raw text content)
   - Added missing types: SystemdTimer, AtJob, GeneratedTimer, Fstab
3. **Persisted state model cleanup:**
   - Rewrote Undo Stack Contract as journal-replay model: ops persisted
     as-is, in-memory undo metadata reconstructed by forward replay
   - Reconciled converged-edit undo with generic EditVariant undo:
     convergence case detects existing hash in host-sourced pool, no
     content to remove
   - Replaced "Content Collision During Resume" with "Content
     Disjointness" section explaining the invariant and the one edge
     case where it can be violated (tarball re-generated between
     sessions)
   - Variant Operations Undo/Redo section now points to the Undo Stack
     Contract as the single authoritative source

### Round 4

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.
Collins and Lens approve-with-nits; Tang and Thorn block on one theme.

Closed this round: singular round-trip contract, tarball-only artifact
truth, session-sidecar boundary, ItemId/scope truth, singular
`render_refine_export()` export path.

One remaining blocker addressed:

1. **Single durable content home:** Removed `user_variants` from
   `SessionState`. The op journal (`SessionState.ops`) is the sole
   durable content source. `EditVariant` ops carry their `content`
   inline. On replay, the engine reconstructs the in-memory
   `user_variants` map from op content. No second durable home.
   Source of Truth Rules, Content Disjointness, and Undo Stack
   Contract all updated to reflect the journal-only model.

Three nits also addressed:
- `fleet: None` items excluded from zone map (unclassified, sort last)
  instead of defaulting to Consensus
- v1 diff guardrail: 100KB input cap with `DiffError::InputTooLarge`
- Read-only filesystem classification: permanent (`EROFS`/`EACCES`)
  skips all future saves; transient (disk full) retries next op

### Round 5

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.
Collins approves; Lens approve-with-nits; Tang and Thorn block on one
remaining issue.

Closed this round: singular durable content home, `fleet: None`
handling, diff guardrail, save-failure classification.

One remaining blocker addressed:

1. **Item-scoped working store + full-pool convergence:**
   - Working `user_variants` map keyed by `(ItemId, ContentHash)`,
     not flat `ContentHash`. Each item's variant pool is independent.
     Undo on item A never affects item B, even with identical hashes.
   - Convergence checks the full variant pool for the target item
     (host-sourced AND user-created from prior edits), not just
     host-sourced. Scope is per-item — a hash match on a different
     item is irrelevant.
   - Source of Truth Rules, Content Disjointness, and Undo Stack
     Contract all updated to reflect item-scoped semantics.

### Round 6

Panel: Tang, Collins, Thorn, Lens. Verdict: request-changes.
Tang approve-with-nits; Collins approve; Lens approve-with-nits;
Thorn blocks on stale replay wording.

Closed this round: item-scoped working store, full-pool convergence,
cross-item content independence.

Stale-wording scrub:
- Variant Position Model: removed "flat map" reference, replaced with
  item-scoped dual-source description
- Replay-facing Source of Truth rule: replay now explicitly checks full
  variant pool before inserting (convergence-aware)
- `fleet/variants/` export wording: softened "preserved for re-import"
  to "materialized convenience"
- `ContentHash`: added validation contract (64 hex chars, reject at
  parse time)

## Brainstorm Team

Design input from:
- **Tang** — architecture: crate layout, type design, `Ord` encoding,
  `ContentHash` newtype, `VariantPosition` model, `RefineMode` enum,
  `similar` crate selection, diff API design, variant summary struct
- **Fern** — interaction design: zone boundaries, sort order,
  prevalence badges, variant lifecycle, `DiscardVariant` op, session
  resume UX, export labeling, small-fleet zone suppression, stale
  detection
- **Ember** — product strategy: prevalence-first sort axis, fleet mode
  as transformative capability, variant summary as product feature,
  content-agnostic diff design
- **Thorn** — testing: verification contracts, property-based testing
  for zones, deterministic diff testing, single source of truth
- **Lens** — spec quality: two-spec horizontal split, serial waterfall
  sequencing, context saturation management, API contract as natural
  seam
