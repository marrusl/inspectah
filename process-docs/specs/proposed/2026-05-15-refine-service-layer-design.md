# Refine Service Layer Design

## Overview

The refine service layer is the interactive middle of the inspectah pipeline.
`scan` produces an `InspectionSnapshot`. `refine` loads that snapshot and lets
an operator include/exclude packages and config files, see attention routing,
preview the Containerfile after each change, and eventually export a full
render + tarball. `build` takes that tarball and produces a container image.

```
scan  ──>  InspectionSnapshot (tarball)
               │
           refine  ──>  RefinedView (live preview, interactive ops)
               │
           export  ──>  tarball (same format as scan output)
               │
           build   ──>  container image
```

Refine is a pure domain layer (`inspectah-refine`) with an HTTP skin
(`inspectah-web`). The service layer owns operations, state, attention
computation, undo/redo, and view projection. The web layer owns transport.

## Scope -- V1

**In scope:**

- Package operations: include/exclude individual packages by name+arch
- Config file operations: include/exclude individual config files by path
- Attention routing: server-computed triage tags (`NeedsReview` / `Informational` / `Routine`) with typed reasons shipped on every item
- Live Containerfile preview after each operation (cheap, immediate)
- Full re-render + tarball export as an explicit user action (expensive, on demand)
- Undo/redo with a linear operation stack and cursor
- Web UI: embedded axum server in the CLI binary, serves a PatternFly report with interactive controls
- Single operator, single session, localhost only

**Not in v1:**

- TUI (ratatui -- same `RefineSession` API, different transport)
- Architect feature (multi-artifact decomposition)
- Quadlet/flatpak operations
- Service/scheduled task/SELinux interactive operations
- Fleet baseline as attention heuristic input
- Collaborative multi-operator review
- Persistent sessions (session dies with the process)

## Go Refine Cutover Contract

This design is a **clean replacement** of the current Go refine seam, not a
compatibility shim or incremental extension. The current Go endpoints and
their behavioral contracts do not carry forward:

| Go endpoint | Disposition |
|-------------|-------------|
| `GET /api/snapshot` | **Dropped.** Replaced by `GET /api/view` which returns a `RefinedView` with attention tags, stats, and generation counter. |
| `PUT /api/snapshot` (with `revision`) | **Dropped.** State mutation is expressed as discrete operations via `POST /api/op`, not bulk snapshot replacement. There is no autosave. |
| `POST /api/render` | **Dropped.** Containerfile preview is computed on every view projection. Full re-render happens only at export time via `POST /api/tarball`. |
| `GET /api/tarball?render_id=...` | **Replaced** by `POST /api/tarball` with `generation` in the request body. The generation counter replaces `render_id` for binding export to reviewed state. |
| `POST /api/reset` | **Dropped.** Undo-all achieves the same result. A dedicated reset endpoint may return in a future version. |
| `GET /api/health` (`re_render` flag) | **Simplified.** `GET /api/health` returns `{"status":"ok"}`. The `re_render` browser detection flag is not needed -- the new UI always knows it is in refine mode. |

**The browser UI is also a clean replacement.** The current PatternFly
report with autosave, `revision` tracking, and `render_id`-gated download
is replaced by a new UI that consumes the `RefinedView` API. The new UI
tracks `generation` for stale-state detection and generation-bound export.

**What this means for `inspectah build`:** The exported tarball from
`POST /api/tarball` is consumable by `inspectah build` -- the artifact
contract is defined in "Tarball Artifact Contract" below. Build does not
need to know whether the tarball came from scan or refine.

## Crate Architecture

```
inspectah-core (types, snapshot, traits)
       ↑
inspectah-refine (service layer -- operations, state, attention, undo)
       ↑                       ↑
inspectah-pipeline (renderers) inspectah-web (axum handlers, rust-embed)
                                     ↑
                               inspectah-cli (refine subcommand)
```

`inspectah-refine` is a new workspace member. Key boundaries:

- `inspectah-refine` depends on `inspectah-core` (for `InspectionSnapshot`,
  `PackageEntry`, `ConfigFileEntry`, etc.) and `inspectah-pipeline` (for
  `render_containerfile` and `render_all`). Zero transport dependencies.
- `inspectah-web` depends on `inspectah-refine` and `inspectah-core`.
  Owns axum, tower, rust-embed. Never imported by refine.
- `inspectah-refine` is the boundary an eventual TUI would call into.
  The web layer is one consumer, not the consumer.

### New Cargo.toml

```toml
[package]
name = "inspectah-refine"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
inspectah-core = { path = "../inspectah-core" }
inspectah-pipeline = { path = "../inspectah-pipeline" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[dev-dependencies]
insta.workspace = true
tempfile.workspace = true
```

## Data Model

### Package Target Identity

Package operations use a composite key, not a bare name. The snapshot
model carries `name`, `arch`, `epoch`, `version`, `release` per
`PackageEntry` (see `inspectah-core/src/types/rpm.rs`), and the data
includes `multiarch_packages` and `duplicate_packages` lists where a
single name can appear more than once. A name-only key cannot guarantee
that each operation targets exactly one package.

```rust
use serde::{Deserialize, Serialize};

/// Composite key that uniquely identifies a package in a snapshot.
///
/// `name + arch` is the minimum stable identity. The current Go refine
/// seam already uses this granularity (`pkg-<name>-<arch>` in triage.go).
/// Full NEVRA is not required for target identity because a single
/// snapshot never contains two packages with the same name+arch and
/// different versions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageTarget {
    pub name: String,
    pub arch: String,
}

impl PackageTarget {
    /// Matches a PackageEntry by name and arch.
    pub fn matches(&self, entry: &PackageEntry) -> bool {
        self.name == entry.name && self.arch == entry.arch
    }
}

impl std::fmt::Display for PackageTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.name, self.arch)
    }
}
```

**Why name+arch, not full NEVRA:** A single snapshot is a point-in-time
capture of one host. It never contains two packages with the same
name+arch but different epoch:version-release. `name+arch` is the
natural unique key within a snapshot. Full NEVRA would make the wire
protocol heavier without adding disambiguation power.

**Multiarch example:** A snapshot with `glibc.x86_64` and `glibc.i686`
requires two separate operations to exclude both. The UI must surface
arch alongside name so the operator can target precisely.

### Operations

```rust
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// A single refinement operation. Serde-tagged for JSON transport.
///
/// Each variant targets exactly one item. Batch operations are sequences
/// of individual ops -- the undo stack is per-op, not per-batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "target")]
pub enum RefinementOp {
    ExcludePackage(PackageTarget),
    IncludePackage(PackageTarget),
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
}
```

Constraints:

- **Validated on apply.** `ExcludePackage(PackageTarget { name: "nonexistent", arch: "x86_64" })`
  returns `Err(RefineError::UnknownTarget)`. The snapshot is the source
  of truth for what exists. Validation matches on both name and arch.
- **Idempotent.** Excluding an already-excluded package is a no-op -- it
  succeeds but does not push onto the ops stack or invalidate the cache.
- **Cheap to clone.** All variants are small owned data. `Clone` is derived.
- **Serde-tagged.** The `#[serde(tag = "op", content = "target")]` layout
  produces JSON like `{"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}}`,
  which the web layer sends/receives directly.
- **Unambiguous.** Each op variant targets exactly one item. For package
  ops, `PackageTarget.matches()` must match exactly one `PackageEntry`
  in the snapshot. Zero matches is `UnknownTarget`. Multiple matches
  (which should not happen with name+arch in a single snapshot) is a
  bug in the snapshot, not in the op.

### Session State

```rust
/// The refinement session. Owns the original snapshot, the operation
/// history, and a cached projected view.
///
/// Never mutates `original`. All state is expressed as a sequence of
/// operations replayed over the original to produce a RefinedView.
pub struct RefineSession {
    /// The normalized inspection snapshot loaded from the scan tarball.
    /// This is the post-normalization baseline, not the raw deserialized
    /// bytes. See "Snapshot Normalization" below.
    original: InspectionSnapshot,

    /// Linear operation history. Only ops[0..cursor] are "active."
    ops: Vec<RefinementOp>,

    /// Points one past the last active op. Undo decrements, redo increments.
    /// Invariant: cursor <= ops.len()
    cursor: usize,

    /// Cached projection. Set to None on any mutation (apply/undo/redo).
    /// Lazily recomputed on next view() call.
    cached_view: Option<RefinedView>,

    /// Monotonically increasing generation counter. Incremented on every
    /// state-changing operation (apply, undo, redo). Included in every
    /// response so the UI can detect stale state. Export requires the
    /// caller to supply the generation they reviewed -- stale exports
    /// are rejected.
    generation: u64,
}
```

The cursor model follows standard undo/redo semantics:

- `apply()` truncates `ops` at `cursor` (discarding any redo-able ops),
  pushes the new op, increments `cursor`. Invalidates cache.
- `undo()` decrements `cursor`. Does not remove ops from the vec.
  Returns `Err(RefineError::NothingToUndo)` when `cursor == 0`.
- `redo()` increments `cursor`. Returns `Err(RefineError::NothingToRedo)`
  when `cursor == ops.len()`.

### Attention Model

```rust
/// How urgently an item needs operator attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionLevel {
    /// Operator should actively decide include/exclude.
    NeedsReview,
    /// Worth seeing but probably fine as-is.
    Informational,
    /// Default-safe, bulk-actionable.
    Routine,
}

/// Why an item received its attention level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    // Config reasons
    ConfigModified,
    ConfigUnowned,
    ConfigOrphaned,
    SensitivePath,

    // Package reasons
    PackageNotInBaseline,
    PackageLocalInstall,
    PackageStateChanged,
    PackageNoRepo,

    // Catch-all for future expansion
    Custom(String),
}

/// An attention tag attached to a refined item. Multiple tags per item
/// are allowed -- the highest-level tag wins for display sorting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionTag {
    pub level: AttentionLevel,
    pub reason: AttentionReason,
    pub detail: Option<String>,
}
```

Attention is **computed server-side** during view projection and shipped
with every item in the `RefinedView`. The UI sorts/groups by attention
level but never computes it.

V1 heuristics are intentionally simple -- the model is what matters:

| Reason | Level | Trigger |
|--------|-------|---------|
| `ConfigModified` | NeedsReview | `ConfigFileKind::RpmOwnedModified` |
| `ConfigUnowned` | NeedsReview | `ConfigFileKind::Unowned` |
| `ConfigOrphaned` | Informational | `ConfigFileKind::Orphaned` |
| `SensitivePath` | NeedsReview | Path matches `/etc/shadow`, `/etc/ssh/`, etc. |
| `PackageNotInBaseline` | NeedsReview | `PackageState::Added` and not in baseline list |
| `PackageLocalInstall` | NeedsReview | `PackageState::LocalInstall` |
| `PackageStateChanged` | Informational | `PackageState::Modified` |
| `PackageNoRepo` | Informational | `PackageState::NoRepo` |
| `ConfigFileKind::RpmOwnedDefault` | Routine | Unmodified RPM-owned config |

The sensitive paths list is a `&[&str]` constant in the refine crate. Not
configurable in v1.

### Refined View

```rust
/// A refined package with attention tags applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedPackage {
    /// The underlying package entry (owned, projected from snapshot).
    pub entry: PackageEntry,
    /// Server-computed attention tags.
    pub attention: Vec<AttentionTag>,
}

/// A refined config file with attention tags applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedConfig {
    /// The underlying config file entry (owned, projected from snapshot).
    pub entry: ConfigFileEntry,
    /// Server-computed attention tags.
    pub attention: Vec<AttentionTag>,
}

/// Aggregate statistics about the current refinement state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefineStats {
    pub total_packages: usize,
    pub included_packages: usize,
    pub excluded_packages: usize,
    pub total_configs: usize,
    pub included_configs: usize,
    pub excluded_configs: usize,
    pub needs_review_count: usize,
    pub ops_applied: usize,
    pub can_undo: bool,
    pub can_redo: bool,
}

/// The complete refined view. Returned by RefineSession::view() and
/// serialized as the response body for most HTTP endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefinedView {
    /// All packages with current include/exclude state and attention tags.
    pub packages: Vec<RefinedPackage>,
    /// All config files with current include/exclude state and attention tags.
    pub config_files: Vec<RefinedConfig>,
    /// Live Containerfile preview reflecting current include/exclude state.
    pub containerfile_preview: String,
    /// Aggregate statistics.
    pub stats: RefineStats,
    /// Monotonic generation counter. Incremented on every mutation.
    /// The UI uses this to detect stale state and to bind export requests
    /// to the exact state the operator reviewed.
    pub generation: u64,
}

/// Summary of changes relative to the normalized original snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub packages_included: Vec<PackageTarget>,
    pub packages_excluded: Vec<PackageTarget>,
    pub configs_included: Vec<String>,
    pub configs_excluded: Vec<String>,
    /// True when the current projected state differs from the normalized
    /// original. Computed by comparing projected include/exclude state
    /// against the normalized baseline, not by checking whether the ops
    /// stack is empty (an exclude followed by an include of the same
    /// target yields a non-empty stack but is_dirty: false).
    pub is_dirty: bool,
}
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum RefineError {
    #[error("unknown target: {0}")]
    UnknownTarget(String),

    #[error("nothing to undo")]
    NothingToUndo,

    #[error("nothing to redo")]
    NothingToRedo,

    #[error("stale generation: expected {expected}, got {actual}")]
    StaleGeneration { expected: u64, actual: u64 },

    #[error("render failed: {0}")]
    RenderFailed(String),

    #[error("tarball error: {0}")]
    TarballError(String),

    #[error("snapshot load error: {0}")]
    SnapshotLoad(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

## Service Layer API

```rust
impl RefineSession {
    /// Create a new session from an InspectionSnapshot.
    /// Computes the initial RefinedView eagerly (first view is never lazy).
    pub fn new(snapshot: InspectionSnapshot) -> Self;

    /// Apply a refinement operation.
    ///
    /// - Validates the target exists in the snapshot.
    /// - Idempotent: no-op if the item is already in the requested state.
    /// - On real mutation: truncates redo history at cursor, pushes op,
    ///   invalidates cached view.
    pub fn apply(&mut self, op: RefinementOp) -> Result<(), RefineError>;

    /// Undo the last applied operation.
    /// Decrements cursor, invalidates cache.
    pub fn undo(&mut self) -> Result<(), RefineError>;

    /// Redo the last undone operation.
    /// Increments cursor, invalidates cache.
    pub fn redo(&mut self) -> Result<(), RefineError>;

    /// Return the current refined view. Recomputes from ops[0..cursor]
    /// if the cache is invalidated, otherwise returns the cached value.
    ///
    /// View projection:
    /// 1. Clone the original snapshot.
    /// 2. Replay ops[0..cursor], toggling include/exclude on each target.
    /// 3. Compute attention tags for every package and config item.
    /// 4. Render the Containerfile preview from the projected snapshot.
    /// 5. Compute stats.
    /// 6. Cache and return reference.
    pub fn view(&mut self) -> &RefinedView;

    /// Return the full operation history (all ops, including redo-able).
    pub fn ops_history(&self) -> &[RefinementOp];

    /// The cursor position in the ops stack.
    pub fn cursor(&self) -> usize;

    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;

    /// Compute a summary of changes relative to the original snapshot.
    pub fn pending_changes(&self) -> ChangesSummary;

    /// Whether the current state differs from the original.
    pub fn is_dirty(&self) -> bool;

    /// Render just the Containerfile from the current projected state.
    /// Cheap -- calls inspectah_pipeline::render::containerfile::render_containerfile
    /// on the projected snapshot. This is the same function that produces
    /// containerfile_preview in the RefinedView, exposed separately for
    /// callers that need just the Containerfile without a full view rebuild.
    pub fn render_containerfile(&mut self) -> String;

    /// Run all 8 renderers against the current projected state.
    /// Expensive. Writes artifacts to output_dir.
    /// Calls inspectah_pipeline::render::render_all().
    pub fn render_all(&mut self, output_dir: &Path) -> Result<(), RefineError>;

    /// Render all artifacts + pack into a .tar.gz at the given path.
    /// The output matches the tarball artifact contract defined below.
    ///
    /// `expected_generation` must match `self.generation` or the call
    /// returns `Err(RefineError::StaleGeneration)`. This guarantees the
    /// export reflects the exact state the caller reviewed.
    ///
    /// **Implementation pipeline (executed in order):**
    ///
    /// 1. Validate `expected_generation == self.generation`. If not,
    ///    return `Err(RefineError::StaleGeneration)`.
    /// 2. Create a tempdir (`tempfile::tempdir()`).
    /// 3. Call `self.render_all(&tempdir_path)` — this runs all 8
    ///    renderers against the current projected snapshot, writing
    ///    `Containerfile`, `audit-report.md`, `config/`, `env-files/`,
    ///    and `schema/snapshot.schema.json` into the tempdir.
    /// 4. Serialize the projected `InspectionSnapshot` to
    ///    `inspection-snapshot.json` in the tempdir. This is the
    ///    post-refinement snapshot reflecting all applied operations.
    /// 5. Create a `.tar.gz` from the tempdir contents **flat** — the
    ///    tarball entries are rooted at the archive root with no
    ///    hostname prefix subdirectory. This differs from scan output,
    ///    which uses a `hostname-timestamp/` prefix. The export tarball
    ///    is prefix-free by design.
    /// 6. Write (or stream) the `.tar.gz` to `path`.
    /// 7. Drop the tempdir (automatic cleanup).
    ///
    /// The flat layout means `tar tf output.tar.gz` yields paths like
    /// `inspection-snapshot.json`, `Containerfile`, etc. — never
    /// `some-prefix/inspection-snapshot.json`.
    pub fn export_tarball(
        &mut self,
        path: &Path,
        expected_generation: u64,
    ) -> Result<(), RefineError>;

    /// Return the current generation counter.
    pub fn generation(&self) -> u64;
}
```

### Loading from tarball

```rust
/// Load a RefineSession from a scan output tarball.
///
/// 1. Extract the tarball to a tempdir.
/// 2. Flatten prefixed archives: if extraction yields a single
///    subdirectory at root (e.g., `hostname-20260515-1430/`), move
///    its contents up to the extraction root. This matches Go refine's
///    `validateOutputDir()` behavior and handles the prefix that
///    `create_tarball(..., &stamp)` adds in scan.
/// 3. Read `inspection-snapshot.json` from the (flattened) root.
/// 4. Deserialize + schema-migrate the snapshot.
/// 5. Normalize the snapshot (see "Snapshot Normalization" below).
/// 6. Validate import provenance (see "Import Provenance" below).
/// 7. Create a RefineSession with the normalized snapshot as `original`.
///
/// This is a standalone function, not an inherent method on RefineSession,
/// because tarball I/O is not a concern of the session itself.
pub fn from_tarball(path: &Path) -> Result<RefineSession, RefineError>;
```

### Tarball Artifact Contract

The exported tarball from `export_tarball()` and the imported tarball
consumed by `from_tarball()` share a common artifact format. This
section defines that format precisely so `inspectah build` compatibility
is verifiable.

#### Exported file set

The tarball contains the following files at its root (no prefix
subdirectory in the export):

| File / Directory | Required | Description |
|-----------------|----------|-------------|
| `inspection-snapshot.json` | yes | The projected snapshot reflecting all applied refinement operations. |
| `Containerfile` | yes | Generated by `render_containerfile()` on the projected snapshot. |
| `audit-report.md` | yes | Human-readable report rendered from the projected snapshot. |
| `config/` | conditional | Config file tree materialized by `render_all()`. Present when the snapshot contains config files with `include: true`. |
| `env-files/` | conditional | Environment files materialized by `render_all()`. Present when the snapshot contains env-file data. |
| `schema/snapshot.schema.json` | yes | JSON schema for the snapshot format. |

**Not exported (server-private sidecars):**

| File | Why excluded |
|------|-------------|
| `original-inspection-snapshot.json` | Server-internal sidecar used by Go refine to track the pre-refinement baseline. In the Rust design, the normalized original is held in memory, not persisted to the tarball. |

#### Load behavior for prefixed archives

Scan tarballs are created with a `hostname-timestamp/` prefix directory
(see `inspectah-cli/src/commands/scan.rs` and
`inspectah-pipeline/src/render/tarball.rs`). `from_tarball()` handles
this by flattening: if the extraction root contains exactly one
subdirectory and no files, the loader moves the subdirectory contents
up to root before looking for `inspection-snapshot.json`.

This matches the Go refine behavior in
`cmd/inspectah/internal/refine/server.go` (`validateOutputDir()`).

#### Build compatibility

`inspectah build` consumes a tarball and expects to find at minimum:
`inspection-snapshot.json`, `Containerfile`, and the `config/` tree.
The export artifact set is a superset of what build requires. A tarball
exported from refine is indistinguishable from a tarball produced by
scan+render from build's perspective.

#### Preview/export fidelity

The Containerfile in the exported tarball must be byte-identical to what
`view().containerfile_preview` returned for the same generation. Both
paths call `render_containerfile()` on the same projected snapshot. The
full export goes through `render_all()`, which materializes the config
tree first and then renders the Containerfile from the materialized
roots -- the test plan must prove that preview and export Containerfiles
match for the same projected state.

### Snapshot Normalization

When `from_tarball()` loads a snapshot, it normalizes the deserialized
data before freezing it as the session's `original`. This normalization
step is what makes `is_dirty()`, `pending_changes()`, and "undo back
to clean" deterministic -- without it, different importers could
disagree about what the baseline state actually is.

**Normalization rules (applied in order):**

1. **Include field defaults.** Any `PackageEntry` or `ConfigFileEntry`
   where `include` is missing, null, or was not present in older schema
   versions gets `include: true` (the default-include convention).

2. **Leaf package classification.** If `leaf_packages` or
   `auto_packages` are present, packages not in `leaf_packages` that
   are in `auto_packages` retain their include state. Packages that
   appear as leaf packages but have `include: false` keep that state.
   This preserves the scanner's classification without altering
   operator intent.

3. **Empty string canonicalization.** Fields like `epoch`, `source_repo`,
   and `arch` that are empty strings are left as empty strings (not
   converted to `None` or `"0"`). This matches the current
   `PackageEntry` serde defaults.

4. **Schema migration.** Any older snapshot schema versions are migrated
   to the current version before normalization. The migration +
   normalization pipeline is: `deserialize -> migrate -> normalize`.

**The "original" is the post-normalization snapshot.** All of
`is_dirty()`, `pending_changes()`, `ChangesSummary`, and undo-to-clean
compare projected state against this normalized baseline.

**`is_dirty()` semantics:** Compares the projected include/exclude state
of every package and config item against the normalized original. An
exclude-then-include of the same target yields `is_dirty: false` even
though the ops stack is non-empty. Dirty tracking is state-based, not
history-based.

**"Reset to clean"** means reverting to the normalized original state.
In v1 this is achieved by undoing all operations (`cursor = 0`). A
dedicated reset endpoint may be added later.

### Import Provenance

`from_tarball()` checks the snapshot's `redaction_state` field during
import and **rejects anything that is not `FullyRedacted`**:

| `redaction_state` value | Behavior |
|------------------------|----------|
| `FullyRedacted` | Accept. Normal import path. |
| `PartiallyRedacted` | **Reject.** Return `Err(RefineError::UntrustedSnapshot)` with message: "Snapshot has not been fully redacted. Run inspectah scan to produce a redacted snapshot." |
| `Unknown` or absent | **Reject.** Return `Err(RefineError::UntrustedSnapshot)` with message: "Snapshot has not been fully redacted. Run inspectah scan to produce a redacted snapshot." |
| `Raw` | **Reject.** Return `Err(RefineError::UntrustedSnapshot)` with message: "Snapshot has not been fully redacted. Run inspectah scan to produce a redacted snapshot." |

Add to the error enum:

```rust
    #[error("untrusted snapshot: {0}")]
    UntrustedSnapshot(String),

    #[error("archive safety violation: {0}")]
    ArchiveSafety(String),
```

This is the simplest correct policy. A migration tool should not let
unredacted data into the review flow. The scan pipeline produces
`FullyRedacted` snapshots by default — any other state means the
snapshot was hand-crafted, partially processed, or from an older
pipeline version that did not complete redaction. Loosening this gate
(e.g., accepting `PartiallyRedacted` with warnings) is a future option
if demand exists — see "Deferred / Future Expansion" below.

### Archive Safety Rules

`from_tarball()` accepts operator-provided archives. Even though this is
a local tool, a malicious or malformed tarball should not escape the
tempdir or exhaust resources. The following rules apply during extraction:

| Rule | Limit | Behavior on violation |
|------|-------|---------------------|
| **Path traversal** | Reject entries with `..` components or absolute paths | `Err(RefineError::ArchiveSafety("path traversal: ..."))` |
| **Symlinks / hardlinks / device nodes** | Reject all non-regular-file, non-directory entries | `Err(RefineError::ArchiveSafety("unsupported entry type: ..."))` |
| **Total unpacked size** | 512 MiB | `Err(RefineError::ArchiveSafety("archive exceeds 512 MiB unpacked limit"))` |
| **Maximum file count** | 10,000 entries | `Err(RefineError::ArchiveSafety("archive exceeds 10,000 entry limit"))` |
| **Single file size** | 256 MiB | `Err(RefineError::ArchiveSafety("single file exceeds 256 MiB"))` |
| **Expected root layout** | Must contain `inspection-snapshot.json` at root (post-flattening) | `Err(RefineError::SnapshotLoad("missing inspection-snapshot.json"))` |

These limits are generous for legitimate inspectah tarballs (typically
< 10 MiB with < 100 files). They exist to bound resource consumption,
not to restrict normal use.

### View projection internals

The `project()` function (private) is the core of the session:

```rust
/// Replay ops[0..cursor] against a clone of the original snapshot,
/// then compute attention tags and Containerfile preview.
fn project(&self) -> RefinedView {
    let mut snap = self.original.clone();

    // Apply operations
    for op in &self.ops[..self.cursor] {
        match op {
            RefinementOp::ExcludePackage(target) => {
                // Find package in snap.rpm.packages_added where
                // target.matches(entry), set include = false.
                // Also check base_image_only for completeness.
            }
            RefinementOp::IncludePackage(target) => {
                // Find package in snap.rpm.packages_added where
                // target.matches(entry), set include = true.
            }
            RefinementOp::ExcludeConfig { path } => {
                // Find config in snap.config.files, set include = false
            }
            RefinementOp::IncludeConfig { path } => {
                // Find config in snap.config.files, set include = true
            }
        }
    }

    // Compute attention tags
    let packages = compute_package_attention(&snap);
    let config_files = compute_config_attention(&snap);

    // Render Containerfile preview
    let containerfile_preview = render_containerfile(&snap, None);

    // Compute stats
    let stats = compute_stats(&packages, &config_files, self.cursor,
                               self.can_undo(), self.can_redo());

    RefinedView {
        packages, config_files, containerfile_preview, stats,
        generation: self.generation,
    }
}
```

The attention computation functions (`compute_package_attention`,
`compute_config_attention`) are pure functions that take a snapshot
reference and return tagged items. They live in a submodule
`inspectah_refine::attention`.

## HTTP API

Nine endpoints. The server is a single-threaded axum router holding
`Arc<Mutex<RefineSession>>` as shared state.

### Endpoint Table

| Method | Path | Purpose | Response |
|--------|------|---------|----------|
| GET | `/` | Report HTML | Embedded static asset (200, text/html) |
| GET | `/api/health` | Health check | `{"status":"ok"}` (200) |
| GET | `/api/view` | Current refined state | `RefinedView` JSON (200) |
| POST | `/api/op` | Apply operation | `RefinedView` JSON (200) or error (400/422) |
| POST | `/api/undo` | Undo last op | `RefinedView` JSON (200) or error (409) |
| POST | `/api/redo` | Redo last undone op | `RefinedView` JSON (200) or error (409) |
| GET | `/api/ops` | Operation history | `Vec<AnnotatedOp>` JSON (200) |
| GET | `/api/changes` | Pending changes | `ChangesSummary` JSON (200) |
| POST | `/api/tarball` | Full render + download | `application/gzip` stream (200) |

### Request/Response Details

**POST /api/op**

Request body is a serde-tagged `RefinementOp`:

```json
{"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}}
```

Returns the full `RefinedView` on success, including the current `generation`
counter. The UI replaces its entire state from the response. Error responses:

- `400 Bad Request` -- malformed JSON or unknown `op` variant
- `422 Unprocessable Entity` -- valid op but unknown target (package/config
  not in snapshot). Body: `{"error": "unknown target: httpd.x86_64"}`

**POST /api/undo, POST /api/redo**

No request body. Returns `RefinedView` on success.
`409 Conflict` when there is nothing to undo/redo.
Body: `{"error": "nothing to undo"}` or `{"error": "nothing to redo"}`.

**POST /api/tarball**

Request body (required):

```json
{"generation": 7}
```

The `generation` field must match the session's current generation counter.
If it does not match, the server returns `409 Conflict` with body
`{"error": "stale generation: expected 7, got 5"}`. This guarantees the
exported tarball reflects exactly the state the operator reviewed in
the UI -- not a later or earlier mutation.

On match, triggers `export_tarball()` to a tempfile, then streams the file
as `application/gzip` with `Content-Disposition: attachment;
filename="inspectah-refine-output.tar.gz"`. This is the expensive path --
the response may take seconds.

The HTTP layer snapshots session state under the mutex, then releases the
lock before doing the expensive render + tar work via
`tokio::task::spawn_blocking`. This prevents export from monopolizing
the session lock.

**GET /api/ops**

Returns the full ops history as a JSON array of `AnnotatedOp`. Includes ops
beyond the cursor (redo-able ops). The response type is:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotatedOp {
    /// The operation, flattened via serde tag.
    #[serde(flatten)]
    pub op: RefinementOp,
    /// True for ops below the cursor (applied), false for redo-able ops.
    pub active: bool,
}
```

Example response:

```json
[
  {"op": "ExcludePackage", "target": {"name": "httpd", "arch": "x86_64"}, "active": true},
  {"op": "IncludeConfig", "target": {"path": "/etc/hosts"}, "active": false}
]
```

`active: true` for ops below the cursor, `active: false` for redo-able ops
above it.

### Transport Concerns

- **CORS:** Origin-restricted. The server sets
  `Access-Control-Allow-Origin` to the exact origin it serves (e.g.,
  `http://localhost:8642`), not `*`. This prevents malicious web pages
  in other browser tabs from reading refine state or triggering mutations
  via cross-origin requests to the loopback API. The served origin is
  known at bind time and configured as a tower-http CORS layer.
- **Content-Type:** All JSON endpoints use `application/json`.
  Tarball uses `application/gzip`.
- **Request body limits:** All JSON endpoints enforce a 1 MiB body limit.
  `POST /api/op` payloads are small (< 1 KiB). The limit prevents abuse
  without restricting legitimate use.
- **Concurrency:** `Arc<Mutex<RefineSession>>` serializes all mutations.
  No concurrent writes. Reads also take the lock (view() may recompute
  the cache). This is fine for single-operator localhost use.
  `POST /api/tarball` snapshots state under the lock and releases it
  before doing expensive render/tar work (see tarball endpoint above).
- **Error shape:** All error responses use `{"error": "<message>"}`.
  HTTP-safe error messages only -- internal details (file paths, stack
  traces) are logged server-side, not returned in the response body.
  `RefineError` variants map to transport-safe strings.

### Axum Handler Sketch

```rust
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use std::sync::{Arc, Mutex};

type AppState = Arc<Mutex<RefineSession>>;

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_report))
        .route("/api/health", get(health))
        .route("/api/view", get(get_view))
        .route("/api/op", post(apply_op))
        .route("/api/undo", post(undo))
        .route("/api/redo", post(redo))
        .route("/api/ops", get(get_ops))
        .route("/api/changes", get(get_changes))
        .route("/api/tarball", post(export_tarball))
        .with_state(state)
}
```

Static assets (report HTML, CSS, JS) are embedded at compile time via
`rust-embed`. The `GET /` handler serves `index.html` from the embedded
assets. Additional asset routes (`/assets/*`) serve CSS/JS/images.

## CLI Integration

### New Subcommand

```rust
#[derive(Subcommand)]
enum Commands {
    Scan(commands::scan::ScanArgs),
    /// Interactively refine scan output and re-render
    Refine(commands::refine::RefineArgs),
    Version,
}

#[derive(clap::Args)]
pub struct RefineArgs {
    /// Path to scan output tarball (.tar.gz)
    pub tarball: PathBuf,

    /// Port to bind (default: 8642, use 0 for ephemeral)
    #[arg(long, default_value = "8642")]
    pub port: u16,

    /// Open browser automatically
    #[arg(long, default_value = "true")]
    pub open: bool,
}
```

### Lifecycle

```
$ inspectah refine ./output/inspectah-host-20260515.tar.gz
Loading snapshot...
Starting refine server on http://localhost:8642
Press Ctrl-C to stop.
```

1. `from_tarball()` loads, migrates, normalizes, and validates the snapshot.
2. `RefineSession::new()` stores the normalized snapshot as `original` and
   computes the initial view (generation 0).
3. Axum server binds to `localhost:{port}`. Port 0 picks an ephemeral port
   (the actual port is printed).
4. If `--open` is true, `open::that(url)` launches the default browser.
5. Server runs until SIGINT/SIGTERM.
6. On shutdown: if `session.is_dirty()`, print a warning:
   `Warning: unsaved changes. Use POST /api/tarball to export before stopping.`

The server does not auto-export. The operator explicitly requests tarball
export via the UI or API. This avoids surprises.

## Testing Strategy

Three layers, matching the crate boundaries.

### inspectah-refine (unit tests)

Pure domain logic, no I/O, fast.

**Operation mechanics:**
- Each `RefinementOp` variant: apply on a known snapshot, verify the
  targeted item's `include` field toggled.
- Reject unknown target: apply `ExcludePackage(PackageTarget { name: "nonexistent", arch: "x86_64" })`,
  assert `Err(RefineError::UnknownTarget)`.
- Reject wrong arch: apply `ExcludePackage(PackageTarget { name: "glibc", arch: "s390x" })`
  on a snapshot that has `glibc.x86_64` only, assert `Err(RefineError::UnknownTarget)`.
- Idempotency: exclude an already-excluded package, verify no-op (ops
  stack length unchanged, cache not invalidated).
- Apply then undo: verify state matches original.
- Apply-apply-undo-redo: verify cursor walks correctly.
- Apply after undo (mid-stack): verify redo history is truncated.
- **Multiarch targeting:** snapshot with `glibc.x86_64` and `glibc.i686`,
  exclude only `glibc.i686`, verify `glibc.x86_64` remains included.

**Undo/redo edge cases:**
- Undo on empty stack: `Err(RefineError::NothingToUndo)`.
- Redo with nothing undone: `Err(RefineError::NothingToRedo)`.
- Undo all, then redo all: state matches the fully-applied state.

**Attention computation:**
- `ConfigFileKind::RpmOwnedModified` -> `AttentionLevel::NeedsReview`,
  reason `ConfigModified`.
- `ConfigFileKind::RpmOwnedDefault` -> `AttentionLevel::Routine`.
- `PackageState::Added` without baseline -> `NeedsReview`.
- `PackageState::LocalInstall` -> `NeedsReview`.
- Sensitive path (`/etc/shadow`) -> `NeedsReview`, reason `SensitivePath`.
- Multiple tags: a modified config at a sensitive path gets both
  `ConfigModified` and `SensitivePath`.

**View projection:**
- View after exclude: excluded item has `include: false` in the view.
- Containerfile preview after exclude: the excluded package does not
  appear in `dnf install`.
- Stats reflect current include/exclude counts.

**Changes and dirty state:**
- `pending_changes()` on fresh session: empty, `is_dirty: false`.
- After one exclude: `packages_excluded` contains the `PackageTarget`, `is_dirty: true`.
- After undo: back to empty and clean.
- **Exclude then re-include same target:** ops stack is non-empty but
  `is_dirty: false` (state-based, not history-based).
- **Normalized-clean load:** load a snapshot with missing `include` fields,
  verify `is_dirty: false` after normalization (not dirty due to defaults).
- **Undo all to clean:** apply 3 ops, undo 3, verify `is_dirty: false`
  and state matches normalized original.

**Containerfile preview fidelity:**
- Preview matches what `render_containerfile()` produces on the projected
  snapshot. This is a golden-test candidate (insta snapshot).

**Tarball export:**
- `export_tarball()` to a tempdir, read back the tarball, verify it
  contains `Containerfile`, `inspection-snapshot.json`, `audit-report.md`,
  `schema/snapshot.schema.json`. Verify the Containerfile inside the
  tarball reflects exclusions.
- Exported tarball does NOT contain `original-inspection-snapshot.json`.
- **Stale generation rejection:** apply an op (generation becomes 1),
  call `export_tarball(path, 0)`, assert `Err(RefineError::StaleGeneration)`.
- **Preview/export fidelity:** for a given generation, the Containerfile
  from `view().containerfile_preview` is byte-identical to the
  Containerfile inside the exported tarball.

**Tarball export round-trip:**
- `export_tarball()` to a tempfile, extract the tarball, verify the
  exact file set matches the documented contract: `inspection-snapshot.json`,
  `Containerfile`, `audit-report.md`, `schema/snapshot.schema.json`, and
  conditionally `config/` and `env-files/`. No extra files, no missing
  files, no prefix subdirectory. This is the contract-enforcement test.
- Verify the tarball is flat: all entries are at the archive root (no
  `hostname-timestamp/` or any other prefix directory).

**Tarball import:**
- Load a prefixed archive (`hostname-timestamp/inspection-snapshot.json`),
  verify flattening works and session starts.
- Load a flat archive (`inspection-snapshot.json` at root), verify session starts.
- **Archive safety:** tarball with `../escape.txt` entry, assert
  `Err(RefineError::ArchiveSafety)`.
- **Provenance rejection (FullyRedacted only):**
  - Tarball with `redaction_state: "Raw"` -> `Err(RefineError::UntrustedSnapshot)`.
  - Tarball with `redaction_state: "PartiallyRedacted"` -> `Err(RefineError::UntrustedSnapshot)`.
  - Tarball with `redaction_state: "Unknown"` -> `Err(RefineError::UntrustedSnapshot)`.
  - Tarball with `redaction_state` absent -> `Err(RefineError::UntrustedSnapshot)`.
  - Tarball with `redaction_state: "FullyRedacted"` -> accepted, session starts.

**Normalization:**
- Load snapshot with missing `include` field on a PackageEntry, verify
  it defaults to `true` and `is_dirty()` returns `false`.
- Load snapshot with `include: false` explicitly set, verify it is
  preserved and `is_dirty()` returns `false` (the normalized original
  respects explicit scanner intent).

### inspectah-web (integration tests)

HTTP contract tests using `axum::test::TestServer` (or direct `Router`
testing with `tower::ServiceExt`).

**Endpoint contracts:**
- `GET /api/health` -> 200, body `{"status":"ok"}`.
- `GET /api/view` -> 200, body deserializes to `RefinedView`.
- `POST /api/op` with valid op -> 200, body is `RefinedView`, stats
  reflect the change.
- `POST /api/op` with unknown target -> 422, body has `error` field.
- `POST /api/op` with malformed JSON -> 400.
- `POST /api/undo` on fresh session -> 409.
- `POST /api/redo` on fresh session -> 409.
- `POST /api/undo` after an op -> 200, view matches original.
- `GET /api/ops` -> 200, body is array of `AnnotatedOp`.
- `GET /api/changes` -> 200, body is `ChangesSummary`. Package entries
  use `PackageTarget` (name+arch), not bare strings.
- `POST /api/tarball` with matching generation -> 200, content-type is
  `application/gzip`, body is a valid tar.gz.
- `POST /api/tarball` with stale generation -> 409, body has `error` field.
- `POST /api/tarball` with no body -> 400.

**State persistence across requests:**
- Apply op, then GET /api/view: view reflects the op.
- Apply op, undo, GET /api/view: view matches original.

**Generation tracking:**
- Apply op, verify response includes `generation: 1`.
- Apply second op, verify `generation: 2`.
- Undo, verify `generation: 3` (undo increments generation too).
- `GET /api/view` returns current generation in every response.

**CORS:**
- Request with `Origin: http://localhost:8642` -> response includes
  `Access-Control-Allow-Origin: http://localhost:8642`.
- Request with `Origin: http://evil.example.com` -> response does NOT
  include `Access-Control-Allow-Origin` or returns the served origin
  only (never the requesting origin).

**Concurrency:**
- Parallel POST /api/op requests serialize correctly (no panics, no
  data races). Not a performance test -- just a correctness check that
  the Mutex works.

### End-to-end (CLI)

Integration tests that spawn the actual binary.

- `inspectah refine <tarball> --port 0`: verify server starts, extract
  port from stdout, GET /api/health -> 200.
- Apply ops via HTTP, verify GET /api/view reflects them.
- POST /api/tarball, verify the downloaded file is a valid tar.gz with
  expected contents.
- Ctrl-C (SIGINT): verify clean shutdown, dirty-state warning if
  applicable.

All tests use ephemeral ports (`:0`) -- no hardcoded port numbers.

## Deferred / Future Expansion

**TUI transport.** `ratatui` consuming the same `RefineSession` API. The
session is `!Send` only if we add non-Send state; keeping it `Send + Sync`
(via `Arc<Mutex<>>`) supports both web and TUI without restructuring.

**Quadlet/flatpak operations.** First expansion of `RefinementOp`. New
variants: `ExcludeQuadlet`, `IncludeQuadlet`, etc. The attention model
extends with new `AttentionReason` variants. The `RefinedView` gains new
item lists.

**Service/scheduled/SELinux operations.** Same pattern: new op variants,
new refined item types, new attention reasons.

**Fleet baseline as attention input.** When `FleetPrevalence` data is
present on packages/configs, the attention heuristics use it: high-prevalence
items are more likely `Routine`, low-prevalence items are more likely
`NeedsReview`. The `AttentionReason` enum gains `FleetUncommon` or similar.

**Architect feature.** Post-cutover from Go. Multi-artifact decomposition
sits above refine -- it consumes a `RefinedView` and splits it into
multiple Containerfiles. Different crate, calls into `inspectah-refine`.

**Collaborative review.** Multiple operators, shared session state,
conflict resolution. Requires replacing `Arc<Mutex<>>` with a more
sophisticated state manager (CRDT or OT). Well beyond v1.

**Loosened import provenance gate.** V1 rejects any snapshot that is not
`FullyRedacted`. If demand exists, a future version could accept
`PartiallyRedacted` snapshots with warnings (e.g., log to stderr, carry
`import_warnings` in `RefinedView`). The gate logic is a single match
arm in `from_tarball()`, so loosening is a small change.

## Revision History

### Round 3 (2026-05-15)

Revised to address two remaining blockers from round 2 review.

1. **Pinned export tarball pipeline path.** Added explicit 7-step
   implementation pipeline to `export_tarball()` documenting the exact
   sequence: validate generation, create tempdir, call `render_all()`,
   serialize projected snapshot, tar contents flat (no prefix), write
   to output path, cleanup. Added round-trip test requirement: export a
   tarball, extract it, verify the exact file set matches the documented
   contract with no extra files, no missing files, and no prefix
   subdirectory.

2. **Narrowed v1 import to FullyRedacted only.** Replaced the "accept
   with warnings" behavior for `PartiallyRedacted`, `Unknown`, and
   absent `redaction_state` with hard rejection via
   `RefineError::UntrustedSnapshot`. V1 now rejects any snapshot where
   `redaction_state` is not `FullyRedacted`. Updated `UntrustedSnapshot`
   error variant to carry a message string. Added explicit provenance
   test cases for each rejected state. Moved loosened import gate to
   "Deferred / Future Expansion."

### Round 2 (2026-05-15)

Revised to address five review blockers identified by Tang, Thorn, Kit,
and Slate.

1. **Package target identity (name -> name+arch).** Added `PackageTarget`
   composite type with `name + arch` fields. `RefinementOp::ExcludePackage`
   and `IncludePackage` now take `PackageTarget` instead of bare `String`.
   `ChangesSummary` uses `Vec<PackageTarget>` instead of `Vec<String>`.
   Validation, idempotency, undo/redo, and view projection all match on
   name+arch. Added multiarch test case.

2. **Browser/session cutover contract.** Added "Go Refine Cutover Contract"
   section explicitly stating this is a clean replacement, not a compat shim.
   Tabulated every Go endpoint with its disposition. Stated the browser UI is
   also a clean replacement.

3. **Tarball import/export artifact contract.** Added "Tarball Artifact
   Contract" section naming the exact exported file set, excluded sidecars,
   prefixed-archive load behavior, build compatibility, and preview/export
   fidelity requirement.

4. **Normalized original-state and dirty-state semantics.** Added "Snapshot
   Normalization" section defining normalization rules, the meaning of
   "original," state-based dirty tracking, and reset-to-clean semantics.
   Added "Import Provenance" section with redaction_state gating.

5. **Local API trust boundary.** Replaced `CORS: *` with origin-restricted
   CORS. Added request body limits. Specified error-message safety (no
   internal details in HTTP responses). Added "Archive Safety Rules" section
   with path traversal prevention, symlink rejection, and resource limits.
   Added `UntrustedSnapshot` and `ArchiveSafety` error variants.

Additional changes: added `generation` counter to `RefineSession` and
`RefinedView` for stale-state detection. `export_tarball()` now requires
`expected_generation` parameter. `POST /api/tarball` requires generation
in the request body and returns 409 on mismatch. Fixed `/api/ops` response
type from `Vec<RefinementOp>` to `Vec<AnnotatedOp>` with defined struct.
Added `StaleGeneration` error variant. Expanded test plan to cover all
new contracts.
