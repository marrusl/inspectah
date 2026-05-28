# inspectah Rust Rewrite Design Spec

**Date:** 2026-05-09
**Status:** Proposed (Round 4 — contract cleanup, all reviewers approved)
**Authors:** Mark Russell, with Ember (product strategy), Tang (Rust architecture)
**Reviewers:** Collins, Thorn, Slate, Press (Rounds 1-2)

---

## Overview

A ground-up rewrite of inspectah in Rust, replacing the Go codebase entirely. This is not a mechanical port — it is an output-contract-backward redesign that uses Rust's type system, ownership model, and enum-based patterns to produce a cleaner, safer, and more expressive tool.

### Why Rust

1. **Upstream alignment.** bootc, composefs-rs, and chunkah are all Rust. A Rust inspectah shares the language ecosystem and could explore direct library integration with bootc in the future. That potential is a research direction, not a near-term architectural dependency — bootc does not currently publish stable external library surfaces.
2. **Dynamic linking with security-sensitive code.** System libraries (openssl, librpm, libselinux) get security patches through the OS package manager. Statically linked binaries don't. inspectah should link against the system's security-sensitive libraries, not bundle its own.
3. **Type safety.** With 14+ inspectors producing structured data through a multi-stage pipeline, Rust's enums, typestate, and ownership model encode invariants that Go checks at runtime (if at all).

### Go Codebase Relationship

Go development is frozen. The Go codebase at `cmd/inspectah/` is reference material for understanding the output contract and edge-case handling. It is not a blueprint. The Rust implementation may produce identical output through entirely different internal structures.

The current Go snapshot schema is at version 13 (numeric integer). The Rust rewrite introduces a new schema generation — see Output Contract for the compatibility matrix.

### Scope

**Viability gates** (required for rewrite to be considered successful):
- All current detection capabilities — no regression against Go output
- Trustworthy artifact generation — `FullyRedacted` state verified before re-rendering, paths sanitized, failure policy enforced
- Deterministic contract tests — normalized Go-vs-Rust comparison in CI

**Full scope** (phased implementation after viability):
- Expanded source type support (package-based, rpm-ostree, bootc hosts)
- Cross-stream targeting as advisory reproduction with incompatibility reporting
- Architect v2 multi-artifact decomposition
- Dual interface: web UI + TUI for both refine and architect
- Redesigned redaction engine with pluggable detectors
- Plugin inspector architecture (explicitly trusted internal teams only)
- Hardware/driver dependency detection
- Non-RPM software expansion (Flatpak, snap, systemd-nspawn)
- RPM preflight validation
- Config file provenance tracking
- Containerless re-rendering (with snapshot trust verification)
- UEFI/firmware informational collection

---

## Crate Architecture

Cargo workspace with five crates:

```
inspectah/
  Cargo.toml                    # workspace root
  inspectah-core/               # types, traits, schema, serde contracts
  inspectah-collect/            # inspector implementations, FFI bindings
  inspectah-pipeline/           # collect → validate → redact → render
  inspectah-cli/                # clap binary + ratatui TUI
  inspectah-web/                # axum server + embedded HTML/JS
```

### Dependency graph

```
cli  ─────────┐     ┌───────── web
              │     │
              ▼     ▼
            pipeline
                │
          ┌─────┴─────┐
          ▼           ▼
       collect      core  ◀── everything depends on core
          │           ▲
          └───────────┘
```

### Crate responsibilities

**inspectah-core** — The gravity well. Every other crate depends on it. Contains all trait definitions (Inspector, Executor, SecretDetector, Renderer), all schema types (InspectionSnapshot, section types), the system type model (SourceSystem, TargetSystem, MigrationContext), and pipeline typestate types. Never touches IO, FFI, or network. Compiles in under 2 seconds.

Stability boundary: changes to `Inspector`, `Executor`, `InspectorOutput`, `SectionData`, and the snapshot schema types affect all downstream crates. These types should be treated as load-bearing and changed deliberately. Types that are internal to a single pipeline stage (e.g., intermediate redaction state) should live in `inspectah-pipeline`, not core.

**inspectah-collect** — Inspector implementations and FFI bindings. Feature-gated: `ffi-rpm` and `ffi-selinux` are optional features, so a contributor can compile and test the config inspector without `librpm-dev` installed. Contains RealExecutor and MockExecutor.

**inspectah-pipeline** — Orchestration layer. Pipeline typestate enforcement (Collected → Validated → Redacted → Rendered). Owns preflight validation, the redaction engine, fleet merge, architect v2 decomposition, and all renderers (Containerfile, HTML report, audit, tarball construction). Also owns snapshot import with trust verification (see Snapshot Trust Model).

**inspectah-cli** — The binary. clap v4 derive for subcommand parsing. ratatui TUI interfaces for refine and architect. Thin presentation layer — all logic lives in pipeline. Binds web server to loopback (127.0.0.1) by default; `--bind` flag required for non-loopback addresses.

**inspectah-web** — axum HTTP server. REST API for refine and architect web UIs. rust-embed for static HTML/JS/CSS assets. Refine preserves the existing PatternFly 6 frontend. Architect gets a redesigned web UI. Loopback-only by default. Request body size limits enforced. Plugin directory configuration is CLI/config-file only — not settable via web API.

---

## Core Type Model

### Source system types

Both `RpmOstree` and `Bootc` variants share the underlying ostree filesystem model for `/etc`: machine-local state managed via ostree's 3-way merge (base tree, deployed tree, local modifications). This is not an "overlay" in the union-filesystem sense — it is ostree's merge-based model for `/etc` that both rpm-ostree and bootc inherit.

```rust
/// What we're inspecting. Each variant carries exactly the data
/// its inspectors need.
pub enum SourceSystem {
    PackageBased {
        os_release: OsRelease,
    },
    RpmOstree {
        os_release: OsRelease,
        variant: OstreeVariant,
        base_image: Option<ImageRef>,
    },
    Bootc {
        os_release: OsRelease,
        /// The currently booted image. This is the sole baseline
        /// truth for rendering. All drift is measured against this.
        booted_image: ImageRef,
        /// Informational only. Staged deployments are queued
        /// next-boot state — they must not influence migration
        /// output for the currently running system.
        staged_image: Option<ImageRef>,
    },
}

/// rpm-ostree desktop/immutable variants
pub enum OstreeVariant {
    Silverblue,
    Kinoite,
    Sericea,
    Onyx,
    UniversalBlue { image_ref: ImageRef },
    CentOSStream { major: u8 },
    Rhel { major: u8, minor: u8 },
    Unknown(String),
}

/// What we're migrating TO. Always bootc-based.
pub enum TargetSystem {
    BootcImage { image_ref: ImageRef },
    CustomImage { image_ref: ImageRef, base: ImageRef },
}

/// The migration vector. Source + target determine which inspectors
/// run, how baseline subtraction works, and what the Containerfile
/// looks like. Migration characteristics (cross-major, cross-vendor,
/// cross-stream) are derived from source/target, not stored separately.
pub struct MigrationContext {
    pub source: SourceSystem,
    pub target: TargetSystem,
}

impl MigrationContext {
    pub fn is_cross_major(&self) -> bool { /* derived */ }
    pub fn is_cross_vendor(&self) -> bool { /* derived */ }
    pub fn migration_kind(&self) -> MigrationKind { /* derived */ }
}

/// Derived migration characteristics — replaces the `cross_major` boolean
pub enum MigrationKind {
    SameStream,           // e.g., RHEL 9 package-based → RHEL 9 bootc
    MajorUpgrade,         // e.g., RHEL 9 → RHEL 10
    VendorTransition,     // e.g., CentOS Stream → RHEL
    CommunityToEnterprise,// e.g., Fedora → CentOS Stream
    OstreeToBootc,        // e.g., Silverblue → bootc
}
```

### The booted-only baseline rule

For `SourceSystem::Bootc`, the `booted_image` is the sole baseline for all pipeline operations:
- **Render, preflight, and drift classification** use only the booted deployment.
- **Staged image** is surfaced as informational context in reports and warnings ("Note: a staged deployment exists and will activate on next boot"). It must not influence Containerfile generation, baseline subtraction, or migration recommendations.
- A staged deployment can be download-only, replaced by a newer update, or discarded before use. It is not a reliable representation of the system's current state.

### Inspector trait

```rust
pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext) -> Result<InspectorOutput, InspectorError>;
}

/// Three-state error model. Degraded returns partial results.
pub enum InspectorError {
    Skipped { reason: String },
    Degraded { partial: InspectorOutput, reason: String },
    Failed { reason: String },
}

pub struct InspectorOutput {
    pub section: SectionData,
    pub warnings: Vec<Warning>,
    pub redaction_hints: Vec<RedactionHint>,
}
```

The `applicable_to` method replaces the Go pattern of checking `systemType` inside each inspector. The orchestrator calls it before `inspect()` — inspectors that don't apply never run.

`InspectorError::Degraded` is a key addition: real production hosts are messy. If the RPM database is partially corrupted but 95% of packages are readable, that's better than failing entirely. See Failure Policy for which degraded states are tolerable.

### Executor trait (testability seam)

```rust
pub trait Executor: Send + Sync {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult;
    fn read_file(&self, path: &Path) -> io::Result<String>;
    fn file_exists(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    fn host_root(&self) -> &Path;
}
```

MockExecutor substitutes canned command output for offline testing. RealExecutor runs commands on the live host.

### Pipeline typestate

```rust
Pipeline<()>
  │  .collect(ctx, inspectors)
  ▼
Pipeline<Collected>        ← holds RawSnapshot
  │  .validate()  or  .skip_validation()
  ▼
Pipeline<Validated>        ← holds ValidatedSnapshot
  │  .redact(opts)  or  .skip_redaction()
  ▼
Pipeline<Redacted>         ← holds RedactedSnapshot
  │  .render(renderers)
  ▼
Artifacts                  ← tarball, Containerfile, reports
```

You cannot call `render()` on a `Pipeline<Collected>`. The type system prevents it.

### Snapshot import (containerless re-rendering)

Importing a snapshot from disk requires trust verification. A deserialized snapshot does not automatically become `Pipeline<Redacted>`:

```rust
let imported = SnapshotImport::load(file)?;
let pipeline = match imported.redaction_state {
    RedactionState::FullyRedacted { .. } =>
        Pipeline::from_verified(imported),  // → Pipeline<Redacted>
    RedactionState::PartiallyRedacted { .. } =>
        Pipeline::from_partial(imported)    // → Pipeline<Validated>
            .redact(&opts),                 // re-evaluate unresolved findings
    RedactionState::Unknown | RedactionState::Raw =>
        Pipeline::from_raw(imported)        // → Pipeline<Collected>
            .validate()?
            .redact(&opts),
};
let artifacts = pipeline.render(&renderers)?;
```

Only `FullyRedacted` skips redaction. Everything else gets a redaction pass. See Snapshot Trust Model for details.

### Two-phase collection

RPM runs first and produces `RpmState` (packages, owned paths, module streams). All other inspectors run in parallel with `&RpmState` as read-only context. The SELinux, config, and scheduled task inspectors all need package ownership data to classify findings correctly.

Parallelism is bounded: a configurable worker pool (default: number of logical CPUs, capped at 16) limits concurrent inspectors. If any inspector panics (should not happen but defense-in-depth), the panic is caught at the task boundary and converted to `InspectorError::Failed`. Other inspectors continue.

---

## Failure Policy

The three-state inspector error model (`Skipped`, `Degraded`, `Failed`) requires explicit rules about which failures are tolerable for which commands.

### Per-inspector failure tolerance

| Inspector | If Degraded | If Failed |
|-----------|-------------|-----------|
| rpm | Continue with warnings. Mark affected package sections as incomplete. Config provenance that depends on RPM ownership data is degraded. | Abort `scan`. RPM data is foundational — without it, baseline subtraction and most other inspectors produce unreliable output. |
| ostree | Continue with warnings. Fall back to RPM-only baseline subtraction. | Continue with warnings on package-based hosts (inspector was skipped). Abort on rpm-ostree/bootc hosts — ostree data is the primary source type signal. |
| config | Continue. Partial config inventory is still useful. | Continue with warnings. Report and Containerfile mark config section as incomplete. |
| All others | Continue with warnings. Mark affected section as incomplete. | Continue with warnings. Mark affected section as incomplete. |

### Preflight sequencing

Preflight is a render-time concern, not a scan-time concern. The sequencing model:

- **`scan`** collects host state. No preflight. No target knowledge needed. Produces a snapshot.
- **Preflight runs inside the render path** when a target is specified. It consumes snapshot data (what packages are needed) + target context (where to check availability).
- **`scan --target <image>`** is sugar for scan + immediate render with target context. Preflight runs before artifact emission.
- **`render --target <image>`** runs preflight against an imported snapshot before producing artifacts.
- **Without `--target`**, no preflight runs. Same-stream rendering (source OS = target OS) does not need package availability checks.

This means preflight is not a separate pipeline typestate — it is a step within rendering that is activated by the presence of target context in `RenderRequest`.

### Per-command failure behavior

| Command | Degraded inspectors | Failed inspectors | Preflight (when target specified) |
|---------|--------------------|--------------------|----------------------------------|
| `scan` (no target) | Proceed. Output marked incomplete per section. | Proceed unless RPM or ostree (on applicable source type) failed → abort. | Not run. |
| `scan --target` | Same as above for collection. Preflight runs before artifact emission. | Same as above for collection. | Incomplete: proceed with per-package warnings. Failed: proceed with "preflight failed" warning. |
| `refine` | Proceed. Incomplete sections shown but not editable. | Proceed. Failed sections omitted from triage. | Not run during triage. Runs on re-render if target specified. |
| `fleet` | Proceed. Prevalence calculation notes incomplete hosts. | Proceed. Failed hosts flagged in aggregate. | Not run during merge. Runs on fleet-level render if target specified. |
| `render` (containerless) | Proceed only from `FullyRedacted` snapshots. | Refuse. Re-rendering from a snapshot with failed inspectors requires re-scan. | Incomplete: proceed with per-package warnings. Failed: proceed with warning. Skip: "not verified" warning. |

### Artifact trustworthiness under degraded input

Every rendered artifact includes a `completeness` field:

```rust
pub enum Completeness {
    Full,
    Partial { incomplete_sections: Vec<InspectorId>, reason: String },
    Unverified { missing: Vec<InspectorId> },
}
```

The Containerfile, audit report, and HTML dashboard all surface completeness status. A `Partial` Containerfile includes comments marking which sections may be incomplete. An `Unverified` artifact (from an imported snapshot with unknown provenance) carries a prominent warning.

---

## Detection Scope

### Inspector inventory

| Inspector | Source Types | Status | FFI | Key Changes from Go |
|-----------|-------------|--------|-----|-------------------|
| rpm | All | Carry | librpm | FFI for hot-path queries. rpm-ostree: layered packages from `rpm-ostree status --json`. bootc: admin-managed local deltas from booted image reference. |
| services | All | Carry | Shell | systemctl. Baseline subtraction varies by source type. |
| config | All | Carry | Shell | + provenance tracking. On ostree-backed systems (both rpm-ostree and bootc): detects machine-local `/etc` state via ostree 3-way merge model, not overlay diff. |
| network | All | Carry | Shell | NM profiles, firewalld, /etc/hosts, routes, proxy. |
| storage | All | Carry | Shell | fstab, LVM, mounts. `/var` is out of scope for image reproduction — see note below. |
| selinux | All | Carry | libselinux | FFI for getenforce, contexts, booleans. Policy modules via shell. |
| users_groups | All | Carry | Shell | passwd, shadow, group, sudoers, SSH keys. |
| scheduled | All | Carry | Shell | cron, systemd timers, at jobs. |
| containers | All | Carry | Shell | + systemd-nspawn. Better rootful/rootless, compose vs quadlet classification. |
| non_rpm | All | Carry | Shell | + Flatpak, snap. Existing: pip, npm, gems, standalone binaries. |
| kernel_boot | All | Carry | Shell | sysctl, modules, GRUB, kernel args. + UEFI info (informational only). |
| os_release | All | Carry | Shell | Populates OsRelease for SourceSystem. |
| **hardware** | All | **NEW** | Shell | Kernel modules (lsmod), DKMS packages, out-of-tree drivers, PCI/USB device→driver mapping. Flags drivers that may not exist in base image. |
| **ostree** | RpmOstree, Bootc | **NEW** | Shell | Deployments, layered packages, overrides, local state. Uses `rpm-ostree status --json` and `bootc status --json` (deployment status, not package-layering API). |

### `/var` scoping

On both rpm-ostree and bootc systems, `/var` is persistent machine-local state that survives image updates. inspectah does not inspect or reproduce `/var` contents into the image — that state is not part of the image definition and belongs to the running system.

This is a deliberate product boundary: inspectah tells you what goes into the *image recipe* (Containerfile + configs), not what lives on the *running system's persistent storage*. The rendered output (Containerfile, audit report) explicitly notes this boundary: "Persistent state in /var is not captured. Review /var contents separately for data migration planning."

### Source type behavior matrix

| Behavior | PackageBased | RpmOstree | Bootc |
|----------|-------------|-----------|-------|
| RPM baseline | Diff host RPM list vs base image RPM list | Layered/overridden packages from `rpm-ostree status --json` | Admin-managed local deltas from booted image reference |
| Config detection | `rpm -Va` + /etc scan | Machine-local `/etc` state via ostree 3-way merge (base tree vs deployed vs local modifications) | Machine-local `/etc` state via ostree 3-way merge (same model as rpm-ostree) |
| ostree inspector | Skipped (not applicable) | Deployments, pinned commits, rollback state | Booted image ref (baseline truth), staged image (informational only) |
| Containerfile output | `FROM base` → add all user packages, configs, services | `FROM base` → add layered packages + machine-local state | `FROM booted-image` → reconstruct image-relevant local deltas |

Note: `bootc status --json` is a deployment-status interface that reports booted and staged deployments. It is not a package-layering API — bootc does not have the same layering concept as rpm-ostree. The ostree inspector uses it to identify the booted image reference for baseline truth.

### Cross-stream targeting

Cross-stream targeting produces **advisory reproduction with incompatibility reporting**. It is not a guaranteed conversion. The generated Containerfile is a starting point that requires operator review; unresolved cross-stream differences are surfaced as first-class warnings, not silently papered over.

The `MigrationContext` encodes the source → target pairing. `MigrationKind` is derived from this pairing and determines which additional checks and warnings apply.

Supported cross-stream scenarios:

- **CentOS Stream 9 → RHEL 10 image mode** — RPM names may differ, repos change entirely, package availability varies. Output: Containerfile + package availability warnings + manual follow-up list for retired/renamed packages.
- **RHEL 9 → RHEL 10 image mode** — Major version differences in package versions, config format changes, deprecated services, SELinux policy differences. Output: Containerfile + cross-major warnings covering packages, services, configs, and kernel/boot changes that require operator judgment.
- **Fedora → CentOS Stream** — Package subset, different release cadence. Output: Containerfile + missing-package warnings for Fedora-only packages.
- **Silverblue/Kinoite → bootc** — rpm-ostree to bootc lateral move. Layered packages become Containerfile instructions. Output: Containerfile + informational notes on deployment model differences.

**What "supported" means per vector:** inspectah generates an artifact (Containerfile + configs + report) and a set of warnings. It does not promise the artifact will build without modification. It does not normalize semantic differences between distros (changed defaults, retired services, policy differences). Those are surfaced as warnings requiring operator judgment.

### Preflight data-source contract

RPM preflight validation checks whether target packages exist in the target repos before rendering. This is critical for cross-stream targeting where package names and availability change between OS versions.

**Required inputs:**
- Target repository definitions (repo files or `--target-repos` pointing to repo URLs/paths)
- For RHEL targets: subscription/entitlement context (cert paths or `--entitlement-dir`)

**Operating modes:**
- **Online:** Query target repos directly via `dnf repoquery --repofrompath`. Requires network access and (for RHEL targets) valid entitlement certificates.
- **Offline/manifest:** Accept a pre-generated package manifest (`--target-manifest <path>`) listing available packages in the target repos. Enables disconnected environments and deterministic CI testing.
- **Skip:** `--skip-preflight` disables preflight entirely. Output includes a prominent "preflight not run" warning.

**When preflight is incomplete or fails:**
- Preflight failure does not block rendering. The Containerfile is still generated, but includes inline comments marking each unverified package: `# PREFLIGHT: package 'foo' availability not verified`.
- The audit report and HTML dashboard show preflight status per package, with unresolved items prominently flagged.
- The rendered tarball's `completeness` field records preflight status.

**CI/test strategy:**
- Deterministic testing uses offline manifests generated from known repo snapshots. No ambient network or credentials in CI.
- Fixture manifests for CentOS 9, RHEL 9, RHEL 10, and Fedora targets are maintained in `testdata/manifests/`.

---

## Dual Interface: Web + TUI

Both refine and architect get two presentation interfaces. A shared API layer ensures both call the same operations through the same types.

### Shared API types

```rust
pub enum TriageAction {
    Toggle { item_id: ItemId, included: bool },
    Reclassify { item_id: ItemId, category: Category },
    BulkApply { filter: Filter, action: Box<TriageAction> },
}

pub enum ArchitectCommand {
    MoveToLayer { items: Vec<ItemId>, target: LayerId },
    CreateLayer { name: String, parent: Option<LayerId> },
    MergeFleets { fleets: Vec<FleetId>, threshold: f64 },
    ExportLayer { layer: LayerId, format: ExportFormat },
}

pub struct RenderRequest {
    pub snapshot: RedactedSnapshot,
    pub triage_actions: Vec<TriageAction>,
    pub renderers: Vec<RendererKind>,
    /// When present, activates preflight and cross-stream rendering.
    /// When None, same-stream rendering with no preflight.
    pub target: Option<RenderTarget>,
}

/// Target context for cross-stream rendering and preflight.
/// This is render-time input, not snapshot state — the same
/// snapshot can be rendered against different targets.
pub struct RenderTarget {
    pub system: TargetSystem,
    pub preflight: PreflightMode,
}

pub enum PreflightMode {
    /// Query target repos via dnf repoquery.
    Online { entitlement_dir: Option<PathBuf> },
    /// Check against a pre-generated package manifest.
    Manifest { path: PathBuf },
    /// Skip preflight. Output includes "not verified" warnings.
    Skip,
}
```

Target context lives in `RenderRequest`, not in the snapshot. This is deliberate: the same snapshot can be rendered against different targets (e.g., "what would this CentOS 9 host look like as RHEL 9 image mode? As RHEL 10?"). Target selection is a render-time decision, not a scan-time decision.

### Refine

**Web (preserved):** Port the backend from Go to axum. Keep the existing PatternFly 6 HTML/JS frontend — toggle switches, search/filter, re-render workflow. Embed assets with `rust-embed`. Same port (8642), same workflow.

**TUI (new):** ratatui-based terminal interface. Split panes: category tree (left), findings list (center), detail/diff (right). Vim-style navigation: `j/k` scroll, `/` search, `space` toggle, `Enter` expand. Inline config diff viewer. Re-render in place.

### Architect

**Web (redesigned):** Clean-slate web UI for multi-artifact architect v2. Visual layer hierarchy with drag-and-drop. Artifact breakdown across all 7 types (packages, configs, services, firewall, quadlets, users, sysctls). Diff view showing shared vs fleet-specific items. Full Containerfile export per layer.

**TUI (new):** ratatui tree-based layer manipulation. Move items between layers with keyboard shortcuts. Inline prevalence bars. Tab between artifact types. Preview generated Containerfile in split pane.

### Why both

Sysadmins SSH into hosts. A web UI they can't reach from a terminal session is useless for the actual migration moment. TUI is the primary interface for hands-on work; web is for fleet-level planning sessions and sharing results with managers and stakeholders.

### Web server security defaults

- **Loopback-only by default.** `inspectah refine` and `inspectah architect` bind to `127.0.0.1`. A `--bind <addr>` flag is required for non-loopback addresses.
- **Request body size limits.** Triage and render requests are capped at 50MB (configurable). Prevents resource exhaustion from oversized payloads.
- **No remote plugin configuration.** Plugin directory and plugin enable/disable are CLI/config-file only. The web API cannot change plugin settings.

---

## Redaction Engine

Redesigned with a pluggable detector architecture, confidence levels, and typed secret classifications.

### Detector trait

```rust
pub trait SecretDetector: Send + Sync {
    fn id(&self) -> DetectorId;
    fn sensitivity(&self) -> Sensitivity;
    fn scan(&self, content: &str, context: &ScanContext) -> Vec<Finding>;
}

pub enum Sensitivity {
    Default,   // PEM keys, explicit passwords, API tokens
    Strict,    // + heuristic key-value matching, shadow entries
}
```

### Typed secret classifications

```rust
pub enum SecretKind {
    PrivateKey { format: KeyFormat },
    Certificate,
    ApiToken { provider: Option<String> },
    Password { context: PasswordContext },
    ConnectionString,
    ShadowEntry { status: ShadowStatus },
    EnvironmentSecret,
    GenericCredential,
}

pub enum ShadowStatus {
    Locked,       // !! or !* — not a secret
    Disabled,     // * — not a secret
    NoPassword,   // empty — flag but low confidence
    HasHash,      // actual hash — definitely redact
}
```

### Improvements over Go

- **ShadowStatus enum eliminates false positives.** Locked accounts (`!!`) are not secrets.
- **Confidence levels enable smarter defaults.** High-confidence findings auto-redact; low-confidence findings are flagged for review in the secrets-review output.
- **Pluggable detectors.** New patterns added by implementing the `SecretDetector` trait. Plugin inspectors can ship their own detectors, subject to the plugin trust model (see below).
- **`Cow<str>` for redaction.** Only strings containing findings are cloned and modified. Most content passes through untouched.

### Snapshot trust model

Every exported snapshot carries a `RedactionState` field:

```rust
pub enum RedactionState {
    /// Snapshot was fully redacted — all findings resolved.
    /// This is the ONLY state that permits direct re-rendering.
    FullyRedacted {
        redacted_by: String,     // inspectah version
        config_hash: String,     // hash of redaction patterns + sensitivity
    },
    /// Snapshot was redacted but has unresolved low-confidence
    /// findings that the operator has not yet triaged.
    /// Must be re-evaluated before rendering.
    PartiallyRedacted {
        redacted_by: String,
        config_hash: String,
        unresolved_count: u32,
        unresolved_hints: Vec<RedactionHint>,
    },
    /// Snapshot state is unknown (e.g., imported from external source).
    Unknown,
    /// Snapshot has not been redacted.
    Raw,
}
```

**Import rules — redaction state is separate from re-render eligibility:**
- `FullyRedacted` snapshots can be re-rendered directly (they enter the pipeline as `Pipeline<Redacted>`). This is the only state that skips redaction.
- `PartiallyRedacted` snapshots enter as `Pipeline<Validated>` and must pass through redaction. The redaction stage re-evaluates unresolved findings with current patterns and sensitivity settings. If all findings resolve (auto-redacted or confirmed safe), the snapshot transitions to `FullyRedacted`.
- `Unknown` and `Raw` snapshots must pass through the full redaction stage.
- **The confidentiality boundary is: only `FullyRedacted` snapshots produce artifacts without a second redaction pass.** `PartiallyRedacted` is an honest statement that redaction ran but has open questions — it does not grant trust for re-rendering.

**Low-confidence finding lifecycle:**
1. During initial `scan`, the redaction engine flags low-confidence findings in `redaction_hints`.
2. During `refine`, the operator reviews flagged findings — confirming them as secrets (redact) or safe (dismiss).
3. When all findings are resolved, the snapshot transitions from `PartiallyRedacted` to `FullyRedacted`.
4. If the operator exports before resolving all findings, the snapshot is exported as `PartiallyRedacted` — any subsequent import will require re-evaluation.

---

## Output Contract

The JSON schema is the product. Other tools, scripts, and future bootc integrations consume it.

### Compatibility matrix

The Go codebase uses a numeric `schema_version` (currently at version 13, with compatibility logic for version 12). The Rust rewrite introduces a new schema generation.

| Go Schema | Rust Behavior |
|-----------|--------------|
| v12 | Read via migration function. All fields mapped to Rust types with `#[serde(default)]` for missing fields. |
| v13 (current) | Read via migration function. Full field compatibility. |
| Rust-native (v14+) | The Rust schema continues the Go integer sequence (v14, v15, ...) rather than jumping to a separate versioning scheme. This preserves consumer compatibility — tools that read `schema_version` as an integer continue to work. |

**Parity surface area:**
- **Snapshot semantics:** Every inspector section in Go v13 has a corresponding Rust type. Field names match unless explicitly renamed (documented in a migration table).
- **Containerfile semantics:** Identical output for identical input on package-based hosts. Cross-stream and ostree-backed hosts produce new output not present in Go.
- **Tarball contents:** Same structure plus `schema/snapshot.schema.json` (additive, not breaking).
- **Warning classes:** All Go warning categories preserved. New categories (cross-stream, preflight, completeness) are additive.

**Normalized diff strategy for CI:**
- Golden files from Go v13 are captured as `insta` snapshots.
- A `normalize` function strips fields that are expected to differ (timestamps, version strings, field ordering) before comparison.
- Semantic divergences (e.g., a Rust inspector that classifies a config file more precisely) are documented in `testdata/divergences.md` and excluded from regression checks.
- Any undocumented divergence fails CI.

### Schema strategy

- **Version field:** `"schema_version": 14` (integer, continuing Go sequence) in every snapshot JSON.
- **Separate schema from Rust types:** A JSON Schema file (`schema/snapshot.schema.json`) is the contract. Rust types use `#[serde(rename)]` to match. Internal types can diverge freely.
- **Backward compatibility:** Rust reads Go-era v12/v13 snapshots via a migration function. New fields use `#[serde(default)]`. Removed fields use `#[serde(skip_serializing_if)]`.
- **Fleet aggregate format:** Same schema with prevalence annotations: `prevalence: { hosts: 3, total: 5, percentage: 60.0 }`.
- **Self-describing tarballs:** `schema/snapshot.schema.json` embedded in the tarball alongside the snapshot.

### Artifact path safety

All paths in snapshots, tarball entries, config trees, and generated Containerfile content are subject to safety rules:

- **Path canonicalization:** Reject absolute paths, parent-relative components (`..`), NUL bytes, and control characters in snapshot field values, tarball entry names, and config tree paths.
- **Tarball containment:** All tar entries are rooted under the tarball's top-level directory. Config tree entries are constrained to the `config/` subtree. No symlink entries that point outside the tarball.
- **Containerfile escaping:** Values interpolated into `RUN` commands, `COPY` sources, and comments are validated against a character allow-list (carried from Go's existing input sanitization). Shell-significant characters are escaped or quoted.
- **Snapshot data in HTML/JS:** Values rendered into HTML reports and dashboard content are HTML-escaped to prevent XSS if reports are served or shared.

### Tarball structure

The Rust tarball preserves the Go-era structure exactly — including the prefixed top-level directory and all renderer-owned outputs — and adds the self-describing schema.

```
inspectah-<hostname>-<timestamp>.tar.gz
└── inspectah-<hostname>-<timestamp>/
    ├── inspection-snapshot.json      # structured data (the output contract)
    ├── Containerfile                 # ready-to-build bootc image recipe
    ├── README.md                     # build/deploy commands, FIXME checklist
    ├── report.html                   # interactive PatternFly 6 dashboard
    ├── audit-report.md               # detailed findings in markdown
    ├── secrets-review.md             # redacted content for operator sign-off
    ├── kickstart-suggestion.ks       # deploy-time kickstart fragment
    ├── config/                       # /etc modifications tree for COPY
    │   ├── etc/                      # modified configs, repos, firewall, timers
    │   ├── opt/                      # non-RPM software (venvs, npm apps, binaries)
    │   └── usr/                      # files under /usr/local (+ bootc kargs.d)
    ├── drop-ins/                     # systemd drop-in overrides (conditional)
    ├── redacted/                     # placeholder files for excluded redactions (conditional)
    ├── quadlet/                      # container workload unit files (conditional)
    ├── flatpak/                      # flatpak manifest + provisioning service (conditional)
    │   ├── flatpak-install.json
    │   └── flatpak-provision.service
    ├── merge-notes.md                # fleet merge decisions (conditional: fleet output only)
    ├── entitlement/                  # RHEL subscription certs (conditional)
    ├── rhsm/                         # RHEL subscription manager config (conditional)
    └── schema/snapshot.schema.json   # NEW: self-describing output contract
```

**Always written** (by the Go renderer unconditionally, Rust preserves this):
`inspection-snapshot.json`, `Containerfile`, `README.md`, `report.html`, `audit-report.md`, `secrets-review.md`, `kickstart-suggestion.ks`, `schema/snapshot.schema.json`.

**Conditional** (written only when the relevant data is present):

| Artifact | Condition |
|----------|-----------|
| `config/` | Present whenever `writeConfigTree()` materializes any path under `config/`. This function walks the full snapshot — config files, repo/GPG files, firewall zones, kernel/boot snippets (modules-load.d, modprobe.d, dracut.conf.d, tuned profiles, bootc kargs), included systemd drop-ins (mirrored into `config/` alongside `drop-ins/`), generated/local timer and service units, and non-RPM env files. The directory and subdirectories (e.g., `config/etc/systemd/system/`) may be precreated by `MkdirAll` before include filtering, so they can exist even if no file survives filtering. The canonical reference for all `config/` materialization paths is `cmd/inspectah/internal/renderer/configtree.go`. |
| `merge-notes.md` | Fleet output only — written when one or more variant items (config files, drop-ins, quadlet units, compose files, or non-RPM env files) carry `Fleet` metadata in the snapshot |
| `drop-ins/` | One or more included systemd drop-ins are written (`di.Include == true`) |
| `redacted/` | Redaction findings with `kind=excluded` and `source=file` |
| `quadlet/` | Quadlet unit files with `include=true` |
| `flatpak/` | Flatpak apps with `include=true` |
| `entitlement/` | Host has RHEL entitlement certs and subscription bundling is enabled |
| `rhsm/` | Host has RHSM config and subscription bundling is enabled |

**Not a current Go artifact** (Rust-era addition, planned for Phase 7):
`inspectah-users.toml` — bootc-image-builder user migration config. The Go renderer references blueprint/kickstart user strategies in Containerfile comments but does not emit a standalone user config file. The Rust rewrite will add this as a new artifact when user migration support is implemented.

**Tarball naming and root directory:** follows the Go convention — `inspectah-<stamp>.tar.gz` with `inspectah-<stamp>/` as the root prefix inside, where stamp is `<hostname>-<YYYYMMDD>-<HHMMSS>`.

---

## Plugin Inspector Architecture

The `Inspector` trait is the internal extension boundary. Plugin inspectors implement the same contract via JSON serialization over a subprocess protocol.

### Trust model

**Plugins are full-trust code execution.** A plugin inspector:
- Receives the full `InspectionContext` (which may contain pre-redaction host state)
- Can influence what appears in durable artifacts (Containerfile, reports, tarball)
- Runs with the same privileges as inspectah itself

This is explicitly an internal extension point for trusted teams (e.g., Red Hat SAP, automotive), not a general marketplace. The trust model is: "installing a plugin is equivalent to editing inspectah's source code."

### Security controls

- **Disabled by default.** Plugin loading requires `--enable-plugins` flag or config file setting.
- **CLI/config-file only.** Plugin directory (`--plugin-dir`) cannot be set via web API.
- **Additive redaction only.** Plugin-provided `SecretDetector` implementations can add findings but cannot suppress, downgrade, or override built-in redaction decisions.
- **Namespaced sections.** Plugin inspector output is placed in plugin-namespaced sections of the snapshot (`plugin:<plugin-id>:section-name`), clearly separated from built-in inspector output.

### Loading model

Plugin inspectors are subprocess-based. inspectah invokes the plugin binary with JSON input (`InspectionContext` serialized to stdin) and reads JSON output (`InspectorOutput` serialized from stdout). This is:
- Simple and language-agnostic (plugins can be written in any language)
- No ABI stability concerns (Rust has no stable ABI)
- Clear process isolation (plugin crash does not take down inspectah)
- Versioned at the JSON schema level, not the Rust type level

The plugin contract is the JSON serialization of `InspectionContext` (input) and `InspectorOutput` (output). Changes to these serialized formats follow semver.

---

## FFI Strategy

Dynamic linking against system libraries for hot paths. Shell out for simple queries.

| Library | Approach | Rationale |
|---------|----------|-----------|
| librpm | FFI (feature-gated: `ffi-rpm`) | RPM inspector is the hottest path. Direct database queries avoid spawning processes for every package. |
| libselinux | FFI (feature-gated: `ffi-selinux`) | getenforce, contexts, booleans are frequent queries. The `selinux` crate provides safe bindings. |
| libsystemd | Shell out | Queries are simple (`systemctl list-unit-files`, `is-enabled`) and infrequent. `zbus`/`libsystemd` pull heavy dependencies for minimal gain. |
| bootc/rpm-ostree | Shell out | Detection probes, not hot paths. JSON output from `bootc status --json` and `rpm-ostree status --json` is trivially parseable with `serde_json`. |

Feature gates mean a contributor can compile and test non-FFI inspectors without system library headers installed.

### FFI safety rules

- **Safe wrappers only.** No raw pointers cross crate boundaries. FFI bindings are encapsulated in safe Rust wrapper types within `inspectah-collect`.
- **Defensive error handling.** Corrupt RPM databases, hostile library returns, and unexpected NULL pointers are handled with `Result` returns, not panics. Degraded data from FFI calls produces `InspectorError::Degraded`.
- **Feature isolation.** `ffi-rpm` and `ffi-selinux` are independent features. A build without either produces a fully functional inspectah that uses shell-based fallbacks for all queries (slower but portable).

### Release-build matrix

| Build Profile | Features | Target | Libraries Required | Use Case |
|--------------|----------|--------|--------------------|----------|
| Minimal | None (all shell-based) | Any Linux | None | Development, CI, cross-compilation |
| Full (RHEL 9) | `ffi-rpm`, `ffi-selinux` | x86_64-unknown-linux-gnu | librpm-devel, libselinux-devel | RHEL 9 release RPM |
| Full (RHEL 10) | `ffi-rpm`, `ffi-selinux` | x86_64-unknown-linux-gnu | librpm-devel, libselinux-devel | RHEL 10 release RPM |
| Full (Fedora) | `ffi-rpm`, `ffi-selinux` | x86_64-unknown-linux-gnu | rpm-devel, libselinux-devel | Fedora COPR RPM |
| Cross (aarch64) | `ffi-rpm`, `ffi-selinux` | aarch64-unknown-linux-gnu | Cross-compile libs | ARM release RPM |

CI builds and tests both minimal and full profiles. Release artifacts use the full profile.

---

## Testing Strategy

### Test lanes

| Lane | Trigger | What Runs | Environment |
|------|---------|-----------|-------------|
| **PR smoke** | Every PR | `cargo test`, `cargo clippy`, `cargo fmt --check`. Minimal build (no FFI). Unit tests, fixture tests, serde round-trips, `insta` snapshot checks. | CI (any Linux) |
| **Nightly VM contract** | Nightly schedule | Full build (with FFI). driftify → inspectah scan on RHEL 9, CentOS Stream 10, Fedora. Go-vs-Rust contract diff. Redaction verification suite. | CI VMs (provisioned per run) |
| **Scheduled cross-arch** | Weekly | Full build on aarch64. Smoke test on ARM VM. | CI ARM runner |
| **Browser/API parity** | Per refine/architect change | Playwright E2E tests for preserved refine UI. API contract tests for axum endpoints. | CI with headless browser |
| **Preflight contract** | Per preflight change | Offline manifest-based preflight against fixture repos. Cross-stream scenarios with known expected output. | CI (no network, fixture manifests) |

### Test layers

| Layer | Tool | What It Validates |
|-------|------|-------------------|
| Golden-file / snapshot | `insta` | Output contract. Go v13 JSON output captured as golden files. Normalized diff (see Output Contract) catches regressions. |
| Property-based | `proptest` | Edge cases: serialize → deserialize round-trips, redaction completeness, fleet prevalence math. |
| Testdata-driven | Fixture files | Per-inspector fixtures with canned MockExecutor output. Carried from Go's `testdata/` where applicable. |
| Integration | `driftify` + VM | End-to-end on real RHEL/CentOS/Fedora VMs. driftify creates known drift, Rust inspectah scans, output matches expectations. |
| Contract compatibility | Go vs Rust normalized diff | Both tools scan the same driftify'd VM. Normalized comparison per Output Contract section. Undocumented divergences fail CI. |
| Redaction | Dedicated suite | Planted secrets in fixtures. Assert every secret caught. Assert no false positives on locked accounts, empty values, disabled markers. Verify `RedactionState` (`FullyRedacted` / `PartiallyRedacted`) in exported snapshots. |
| Failure policy | Scenario tests | Simulate degraded/failed inspectors. Verify `Completeness` field, output warnings, and abort-vs-continue behavior per the Failure Policy matrix. |

---

## Implementation Phases

Design the full type system (excluding plugin externalization) in Phase 0. Implement in priority order. Each phase is independently testable and reviewable.

### Phase 0: Foundation
- inspectah-core: all types, all traits, schema v14 types, SourceSystem/TargetSystem/MigrationContext (with derived MigrationKind), Inspector/Executor/SecretDetector/Renderer traits, pipeline typestate types, RedactionState, Completeness, failure policy types
- Internal trait extensibility for inspectors (no externalized plugin ABI yet)
- Golden files captured from Go v13 output
- Normalized diff tooling for Go-vs-Rust comparison
- **Test:** compiles, serde round-trips pass, Go v13 snapshots deserialize into Rust types
- **Does not include:** plugin ABI externalization (deferred to Phase 7)

### Phase 1: First Inspector End-to-End
- RPM inspector (librpm FFI) + MockExecutor
- Pipeline (collect → validate → redact → render)
- Containerfile renderer + tarball output with path safety enforcement
- Basic CLI (`scan` subcommand only)
- Redaction with `RedactionState::FullyRedacted` in exported snapshots (when all findings resolved)
- **Test:** scan a package-based host, produce a tarball. Normalized diff against Go output for the RPM section passes.

### Phase 2: Inspector Parity
- All 12 carried inspectors + 2 new (hardware, ostree)
- Two-phase collection (RPM first, rest in parallel with bounded worker pool)
- Full redaction engine with heuristic parity
- Config provenance tracking
- Failure policy enforcement with `Completeness` field
- **Test:** full scan matches Go output (normalized). driftify integration test on VM. Failure scenario tests.
- **Milestone: Internal parity.** Single-host scans produce trustworthy output with verified `RedactionState` equivalent to Go. Not a release — packaging, install, and update story not yet addressed.

### Phase 3: Cross-Stream + Preflight
- Cross-stream targeting (MigrationContext with derived MigrationKind)
- RPM preflight validation with online/offline/skip modes
- Baseline subtraction varies by source type
- Cross-stream output framed as advisory + incompatibility reporting
- **Test:** CentOS 9 → RHEL 10 scan with offline manifest produces correct Containerfile + package availability warnings + manual follow-up list. Preflight skip mode produces "not verified" warnings.
- **Milestone: Cross-stream advisory.** The feature Go never had.

### Phase 4: Fleet + Architect v2
- Fleet merge with prevalence thresholds
- Architect v2 multi-artifact decomposition (packages, configs, services, firewall, quadlets, users, sysctls)
- **Test:** multi-host fleet merge, layer decomposition across all 7 artifact types
- *Can run in parallel with Phase 5 once snapshot shape and shared API types are stable.*

### Phase 5: Refine
- Web UI backend (axum, preserve existing HTML/JS frontend, loopback-only default)
- TUI refine interface (ratatui)
- Shared API layer (TriageAction, RenderRequest)
- Snapshot import with trust verification (containerless re-rendering)
- `refine` subcommand
- **Test:** triage operations produce correct re-rendered tarballs via both web and TUI. Snapshot import rejects raw/unknown snapshots without re-redaction. Browser/API parity tests.
- *Can run in parallel with Phase 4 once snapshot shape and shared API types are stable.*
- **Milestone: Full inspect → refine workflow** in both interfaces.

### Phase 6: Architect UI
- Redesigned web UI for architect v2
- TUI architect interface
- `architect` subcommand
- **Test:** layer decomposition + export via both interfaces

### Phase 7: Polish, Plugins, Packaging
- Non-RPM expansion (Flatpak, snap, systemd-nspawn)
- Containerless `render` subcommand (public-facing, with trust verification)
- Plugin inspector loading (subprocess protocol, disabled by default, additive redaction only)
- Plugin contract versioning and documentation
- `build` subcommand (podman integration)
- COPR/RPM packaging for Rust binary (full build profile)
- **Test:** full feature parity + all backlog features. Contract compatibility: Go vs Rust on same host. Plugin integration tests with fixture plugins.
- **Milestone: Feature-complete.** Go codebase can be archived once packaging and install story is proven.

### Phase dependency graph

```
P0: Foundation
     │
     ▼
P1: First Inspector E2E
     │
     ▼
P2: Inspector Parity  ──── internal parity milestone
     │
     ▼
P3: Cross-Stream
     │
     ├──────────┐
     ▼          ▼
P4: Fleet    P5: Refine    ← can parallelize (after shared API stable)
     │          │
     ▼          │
P6: Arch UI    │
     │          │
     └────┬─────┘
          ▼
P7: Polish, Plugins, Packaging  ── feature-complete, Go archived
```

---

## Strategic Context

### Competitive positioning

inspectah is a category-of-one tool. Nothing in the migration tooling landscape does what it does — LEAPP does in-place upgrades (different problem), Convert2RHEL is narrow, and nobody else generates bootc Containerfiles from live host state. A Rust rewrite in the same language ecosystem as bootc and composefs-rs creates strategic adjacency. Future exploration of direct bootc library integration is a research direction that could deepen this advantage, but it is not a near-term architectural dependency.

### Cross-stream priority order

1. **CentOS → RHEL** — highest revenue value
2. **RHEL 9 → RHEL 10** — highest volume
3. **Fedora → CentOS** — community goodwill
4. **Silverblue/Kinoite → bootc** — desktop immutable-to-bootc lateral move

### Output format as product

inspectah's output (the JSON snapshot, the Containerfile, the tarball) is its product. The schema is versioned, self-describing (embedded JSON Schema), and designed for external consumption. If bootc ever accepts "migration manifests," inspectah should be the tool that defined the format.
