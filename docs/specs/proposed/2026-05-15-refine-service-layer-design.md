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

- Package operations: include/exclude individual packages by name
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
    ExcludePackage { name: String },
    IncludePackage { name: String },
    ExcludeConfig { path: PathBuf },
    IncludeConfig { path: PathBuf },
}
```

Constraints:

- **Validated on apply.** `ExcludePackage { name: "nonexistent" }` returns
  `Err(RefineError::UnknownTarget)`. The snapshot is the source of truth for
  what exists.
- **Idempotent.** Excluding an already-excluded package is a no-op -- it
  succeeds but does not push onto the ops stack or invalidate the cache.
- **Cheap to clone.** All variants are small owned data. `Clone` is derived.
- **Serde-tagged.** The `#[serde(tag = "op", content = "target")]` layout
  produces JSON like `{"op": "ExcludePackage", "target": {"name": "httpd"}}`,
  which the web layer sends/receives directly.

### Session State

```rust
/// The refinement session. Owns the original snapshot, the operation
/// history, and a cached projected view.
///
/// Never mutates `original`. All state is expressed as a sequence of
/// operations replayed over the original to produce a RefinedView.
pub struct RefineSession {
    /// The unmodified inspection snapshot loaded from the scan tarball.
    original: InspectionSnapshot,

    /// Linear operation history. Only ops[0..cursor] are "active."
    ops: Vec<RefinementOp>,

    /// Points one past the last active op. Undo decrements, redo increments.
    /// Invariant: cursor <= ops.len()
    cursor: usize,

    /// Cached projection. Set to None on any mutation (apply/undo/redo).
    /// Lazily recomputed on next view() call.
    cached_view: Option<RefinedView>,
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
}

/// Summary of changes relative to the original snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesSummary {
    pub packages_included: Vec<String>,
    pub packages_excluded: Vec<String>,
    pub configs_included: Vec<String>,
    pub configs_excluded: Vec<String>,
    /// True when the current state differs from the original snapshot.
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
    /// Calls render_all() to a tempdir, then tarball::create_tarball().
    /// The output format matches scan output exactly -- build can consume it.
    pub fn export_tarball(&mut self, path: &Path) -> Result<(), RefineError>;
}
```

### Loading from tarball

```rust
/// Load a RefineSession from a scan output tarball.
///
/// Extracts the tarball to a tempdir, reads inspection-snapshot.json,
/// deserializes + migrates the snapshot, and creates a RefineSession.
///
/// This is a standalone function, not an inherent method on RefineSession,
/// because tarball I/O is not a concern of the session itself.
pub fn from_tarball(path: &Path) -> Result<RefineSession, RefineError>;
```

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
            RefinementOp::ExcludePackage { name } => {
                // Find package in snap.rpm.packages_added, set include = false
            }
            RefinementOp::IncludePackage { name } => {
                // Find package in snap.rpm.packages_added, set include = true
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

    RefinedView { packages, config_files, containerfile_preview, stats }
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
| GET | `/api/ops` | Operation history | `Vec<RefinementOp>` JSON (200) |
| GET | `/api/changes` | Pending changes | `ChangesSummary` JSON (200) |
| POST | `/api/tarball` | Full render + download | `application/gzip` stream (200) |

### Request/Response Details

**POST /api/op**

Request body is a serde-tagged `RefinementOp`:

```json
{"op": "ExcludePackage", "target": {"name": "httpd"}}
```

Returns the full `RefinedView` on success (the UI replaces its entire state
from the response). Error responses:

- `400 Bad Request` -- malformed JSON or unknown `op` variant
- `422 Unprocessable Entity` -- valid op but unknown target (package/config
  not in snapshot). Body: `{"error": "unknown target: httpd-nonexistent"}`

**POST /api/undo, POST /api/redo**

No request body. Returns `RefinedView` on success.
`409 Conflict` when there is nothing to undo/redo.
Body: `{"error": "nothing to undo"}` or `{"error": "nothing to redo"}`.

**POST /api/tarball**

No request body. Triggers `export_tarball()` to a tempfile, then streams
the file as `application/gzip` with `Content-Disposition: attachment;
filename="inspectah-refine-output.tar.gz"`. This is the expensive path --
the response may take seconds. The HTTP layer uses `tokio::task::spawn_blocking`
for the render + tar work.

**GET /api/ops**

Returns the full ops history as a JSON array. Includes ops beyond the cursor
(redo-able ops). Each entry is annotated:

```json
[
  {"op": "ExcludePackage", "target": {"name": "httpd"}, "active": true},
  {"op": "IncludeConfig", "target": {"path": "/etc/hosts"}, "active": false}
]
```

`active: true` for ops below the cursor, `active: false` for redo-able ops
above it.

### Transport Concerns

- **CORS:** Enabled for all origins. The server is localhost-only and
  single-operator. No auth.
- **Content-Type:** All JSON endpoints use `application/json`.
  Tarball uses `application/gzip`.
- **Concurrency:** `Arc<Mutex<RefineSession>>` serializes all mutations.
  No concurrent writes. Reads also take the lock (view() may recompute
  the cache). This is fine for single-operator localhost use.
- **Error shape:** All error responses use `{"error": "<message>"}`.
  The message is the `Display` impl of `RefineError`.

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

1. `from_tarball()` loads and migrates the snapshot.
2. `RefineSession::new()` computes the initial view.
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
- Reject unknown target: apply `ExcludePackage { name: "nonexistent" }`,
  assert `Err(RefineError::UnknownTarget)`.
- Idempotency: exclude an already-excluded package, verify no-op (ops
  stack length unchanged, cache not invalidated).
- Apply then undo: verify state matches original.
- Apply-apply-undo-redo: verify cursor walks correctly.
- Apply after undo (mid-stack): verify redo history is truncated.

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
- After one exclude: `packages_excluded` contains the name, `is_dirty: true`.
- After undo: back to empty and clean.

**Containerfile preview fidelity:**
- Preview matches what `render_containerfile()` produces on the projected
  snapshot. This is a golden-test candidate (insta snapshot).

**Tarball export:**
- `export_tarball()` to a tempdir, read back the tarball, verify it
  contains `Containerfile`, `inspection-snapshot.json`, `audit-report.md`,
  etc. Verify the Containerfile inside the tarball reflects exclusions.

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
- `GET /api/ops` -> 200, body is array of annotated ops.
- `GET /api/changes` -> 200, body is `ChangesSummary`.
- `POST /api/tarball` -> 200, content-type is `application/gzip`, body
  is a valid tar.gz.

**State persistence across requests:**
- Apply op, then GET /api/view: view reflects the op.
- Apply op, undo, GET /api/view: view matches original.

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
