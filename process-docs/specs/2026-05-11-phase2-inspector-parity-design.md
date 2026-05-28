# Phase 2: Inspector Parity Design

**Date:** 2026-05-11
**Status:** Approved (2026-05-11 — all five review lanes signed off: Tang R4, Thorn R2, Collins R2, Press R3, Slate R2)
**Scope:** Implement all remaining inspectors to achieve inspector-section parity with Go v13 on package-mode systems, with renderer smoke tests per-slice and a full rendered-artifact exit gate.

## Core Principle: Output Equivalence, Not Implementation Equivalence

The parity contract is about the **essential information captured**, not identical internal structures. Rendered artifacts (Containerfile, audit report, secrets review, config tree) and snapshot JSON sections must convey the same meaning as Go output — but how Tang collects, structures, and stores data internally is his design space.

Idiomatic Rust improvements are welcome when they serve correctness, clarity, or efficiency:

- Typed enums over flat strings when the domain has a finite set of values
- Structured FFI over shell-out-and-parse when librpm provides the data directly
- Dropping internally-collected data that no renderer ever reads (with a divergence allowlist entry)
- Leveraging serde, iterators, or ownership patterns that have no Go equivalent

The parity gate validates output. The implementation is Tang's craft.

## System Scope

**Phase 2 targets package-mode systems (traditional RHEL, CentOS, Fedora).** The parity gate, golden files, and test fixtures all reference package-mode scans. This is where the Go golden files come from and where inspectah's primary use case lives.

**The type boundary is wide from day one.** Inspectors receive `InspectionContext` carrying the full `SourceSystem` (including bootc `booted_image`, rpm-ostree variant context, and system type). This avoids freezing a narrow abstraction that needs expensive widening later.

**Bootc/ostree handling in Phase 2:**

- Phase 2 inspectors implement `applicable_to(&self) -> &[SourceSystemKind]` per the approved contract; the returned slice contains only `SourceSystemKind::PackageBased` — the orchestrator never calls `inspect()` on bootc/ostree systems
- The orchestrator returns `Err(Skipped { reason })` for inspectors whose `applicable_to()` excludes the current system kind
- Phase 2 parity gate tests package-mode only
- Phase 3+ expands `applicable_to()` to include `Ostree` and `Bootc` kinds, with dedicated golden files and source/baseline contracts
- Internal `match` arms on `source_system.kind()` within inspectors are exhaustive (compiler-enforced), but non-package-mode arms are unreachable in Phase 2

## Relationship to Approved Core Contract

This spec builds on the approved core contract in `docs/specs/proposed/2026-05-09-rust-rewrite-design.md`. It does not supersede those type definitions. The canonical `inspectah-core` boundary types are:

```rust
pub trait Inspector: Send + Sync {
    fn id(&self) -> InspectorId;
    fn applicable_to(&self) -> &[SourceSystemKind];
    fn inspect(&self, ctx: &InspectionContext) -> Result<InspectorOutput, InspectorError>;
}

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

Phase 2 implements this contract for 10 new inspectors. No modifications to the trait signature, return types, or error model.

## Inspector Context Contract

`InspectionContext` is the runtime context passed to `inspect()`. It carries source-of-truth system information:

```
InspectionContext {
    source_system: SourceSystem,  // package-mode | ostree | bootc (with variant details)
    executor: &Executor,          // shell command execution
    rpm_state: Option<&RpmState>, // available after RPM inspector completes
}
```

**`SourceSystem` is the source of truth.** There is no separate `system_type` field — system-type branching is derived from `source_system` via `source_system.kind()` when an inspector needs it. This prevents mismatch states where `system_type` and `source_system` could disagree.

**Why `InspectionContext` carries `SourceSystem`:** Collins's review identified that bare `SystemType` loses source-system truth. Bootc systems have a `booted_image` reference; ostree systems have variant context. Inspectors that need this distinction (config, kernelboot) `match` on `source_system` directly. Inspectors that only need kind-level branching call `source_system.kind()`.

**Applicability is static, not dynamic.** Each inspector declares which `SourceSystemKind` values it supports via `applicable_to(&self) -> &[SourceSystemKind]`. The orchestrator calls this before `inspect()` — inspectors that don't apply never run. Phase 2 inspectors return `&[SourceSystemKind::PackageBased]` (with bootc/ostree kinds added in Phase 3+).

## RpmState Capability Contract

`RpmState` is a read-only capability surface produced by the RPM inspector and consumed by dependent inspectors. It exposes:

| Method | Return Type | Purpose |
|--------|------------|---------|
| `installed_packages()` | `&[Package]` | Full NEVRA package list |
| `owned_paths()` | `&HashSet<PathBuf>` | All filesystem paths owned by any installed RPM |
| `is_rpm_owned(path)` | `bool` | O(1) ownership check for a single path |
| `package_for_path(path)` | `Option<&Package>` | Which package owns this path |
| `verification_results()` | `&[RpmVaEntry]` | `rpm -Va` output (modified/missing/etc.) |
| `module_streams()` | `&[ModuleStream]` | Enabled module streams |

**Design constraints:**

- Immutable after construction — no interior mutability, no write access
- All lookups are O(1) or O(log n) — dependent inspectors pay no query cost
- `HashSet<PathBuf>` for `owned_paths()` is built once during RPM collection and shared by reference across all dependent inspectors
- No public constructor outside `inspectah-collect` — only the RPM inspector builds `RpmState`

## Slice Structure

Phase 2 is delivered in 3 mandatory slices plus 1 optional, grouped by dependency on `RpmState` and complexity.

### Slice 2a — Foundation (3 inspectors)

| Inspector | Data Sources | Key Parsing |
|-----------|-------------|-------------|
| **services** | `systemctl list-unit-files`, preset files, drop-in dirs | Preset glob matching (first-match-wins, `*`/`?` wildcards), enabled/disabled/masked state, static unit detection |
| **storage** | `findmnt --json`, `lvs --reportformat json`, `/etc/fstab`, automount units | fstab field parsing, mount option extraction, LVM metadata, migration recommendations (tmpfs, NFS, bind) |
| **kernelboot** | `lsmod`, `tuned-adm active`, `/proc/cmdline`, sysctl files, dracut/modules-load configs | Sysctl three-way diff (compiled defaults vs runtime vs file overrides), module parameter extraction, locale/timezone detection |

**Delivers:** Parallel execution pattern proven with `std::thread::scope`. `InspectionContext` threading established. FakeExecutor test fixtures for all three. Section parity gate covers RPM + these 3 sections. Renderer smoke tests for all 3 sections. CI workflow (Tier 1 + Tier 2) established.

### Slice 2b — Expansion (3 inspectors)

| Inspector | Data Sources | Key Parsing |
|-----------|-------------|-------------|
| **network** | `ip route show`, `ip rule`, firewalld zone XMLs, NM keyfile INI, `/etc/hosts`, proxy env vars | INI parsing for NM keyfiles, XML parsing for firewalld zones, route table extraction |
| **containers** | Quadlet `.container`/`.network`/`.volume` units, `docker-compose.yml`, `podman ps --format json`, `podman inspect`, `flatpak list` | Quadlet unit parsing, docker-compose YAML (without library — key extraction only), container image/port/volume extraction |
| **users** | `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow`, sudoers, `~/.ssh/authorized_keys` refs | UID/GID range classification (system vs human), shadow expiry parsing, 4-strategy provisioning (sysusers.d, useradd in Containerfile, kickstart, blueprint), sudoers rule extraction |

**Delivers:** Parity gate covers 6 independent sections total. Renderer smoke tests for 6 sections.

### Slice 2c — RPM-Dependent (4 inspectors)

| Inspector | Data Sources | RPM Dependency |
|-----------|-------------|----------------|
| **scheduled** | `/etc/crontab`, `/etc/cron.d/*`, `/var/spool/cron/*`, systemd timer units, `atq` | Uses `rpm_state.is_rpm_owned()` to classify cron entries as package-provided vs custom. 5-field cron expression parsing, `@shortcuts`, cron-to-systemd-timer conversion heuristics, `@reboot` flagged as non-convertible. |
| **config** | `rpm -Va`, file tree walk, `/usr/etc` (ostree) | RPM-owned paths for modified/unowned/orphan classification. 40+ exact-match + 50+ glob exclusion patterns for unowned filtering. 13 config categories (tmpfiles, sysctl, audit, journal, etc.). Note: `dnf download + rpm2cpio` diff enrichment is deferred to Phase 3 CLI expansion — Phase 2 config inspector does not generate diffs. |
| **selinux** | `sestatus`, `semanage boolean/fcontext/port/module -l`, `/etc/selinux/*/modules/`, audit rules | Package ownership for filtering RPM-provided policy modules vs custom modules. PAM file classification. |
| **nonrpm** | `readelf -S/-d`, `file`, `strings` (4KB head scan), pip/npm/gem metadata dirs, `.git/` detection, venv scanning | RPM package list to identify everything NOT from RPM. dist-info ownership cross-reference. Binary classification (Go/Rust/C/C++ via ELF section heuristics). |

**Delivers:** Full inspector parity. Three-wave parallel execution working end-to-end. Parity gate covers all 10 sections. Phase 2 exit gate: full rendered-artifact comparison against Go.

**`scheduled` placement rationale:** The prior draft placed `scheduled` in Slice 2b with `Option<&RpmState>`, creating a contradiction — the spec claimed independence while acknowledging RPM dependency. The honest resolution: `scheduled` requires `&RpmState` for correct ownership classification. It belongs in Slice 2c. Without RPM ownership data, the Go-equivalent output cannot be reproduced — cron entries would lack the "RPM-provided" vs "custom" distinction that the audit report surfaces.

### Slice 2d — Optional New Inspectors

**hardware** and **ostree/bootc** inspectors are new (no Go equivalent). Include in an earlier slice only if:
- The implementation is simple and self-contained
- It doesn't add risk to the parity-focused slice it joins
- Tang judges it fits naturally
- New `SectionData` variants, parity fixtures, and renderer consumption rules are budgeted

If neither fits, they defer to Phase 3. No pressure to include them.

## Parallel Execution Model

Three-wave execution within a single `std::thread::scope`:

```
Wave 1: Spawn RPM + all 6 independent inspectors in parallel
         (services, storage, kernelboot, network, containers, users)
         ↓
         Join RPM handle → RpmState available
         (independent inspectors may still be running — that's fine)
         ↓
Wave 2: Spawn scheduled, config, selinux, nonrpm with &RpmState
         ↓
Wave 3: Join all remaining handles, collect Result<InspectorOutput, InspectorError> values
```

**Why `std::thread::scope`:** Scoped threads borrow `&InspectionContext`, `&RpmState`, and `&Executor` directly — no `Arc`, no lifetime complexity. With 10 inspectors total, a worker pool adds indirection for no benefit. These are I/O-bound (subprocess execution), not CPU-bound, so `rayon` and `tokio` are wrong tools.

**Ordering:** Section order in the final snapshot is deterministic by section type, not thread completion order.

## Failure Policy and Artifact Emission

### Inspector Return Contract

Each inspector returns `Result<InspectorOutput, InspectorError>` per the approved core contract:

| Return | Meaning | Data Present |
|--------|---------|-------------|
| `Ok(InspectorOutput)` | All data collected successfully | Full `SectionData` + warnings + redaction hints |
| `Err(InspectorError::Degraded { partial, reason })` | Partial data collected, some commands failed | Partial `InspectorOutput` + description of what's missing |
| `Err(InspectorError::Failed { reason })` | Inspector couldn't produce usable output | Error description, no usable data |
| `Err(InspectorError::Skipped { reason })` | Inspector not applicable (returned by orchestrator when `applicable_to()` excludes this system kind) | Reason string |

Note: `Skipped` is returned by the orchestrator based on `applicable_to()`, not by the inspector itself. An inspector that starts running either succeeds, degrades, or fails.

### Dependency Failure

When a dependency fails, the dependent inspector's status reflects the root cause:

| Scenario | Dependent Inspector Status | Rationale |
|----------|---------------------------|-----------|
| RPM inspector succeeded | Normal operation | — |
| RPM inspector degraded | Dependent runs with degraded RpmState, reports its own status honestly | Partial ownership data may produce less precise classification |
| RPM inspector failed | Dependent returns `Failed` with reason "RPM dependency unavailable" | Cannot produce correct output without ownership data |

**RPM failure is a foundational failure, not ordinary section degradation.** The failure policy distinguishes between "my own commands failed" (Degraded) and "my upstream dependency failed" (Failed with dependency reason). This distinction is visible in the snapshot and audit report.

### Panic Containment

Inspector panics are caught at the `thread::scope` join boundary via `std::thread::Result`. A panicking inspector:
- Produces `Failed` with reason "inspector panicked: {message}"
- Does not affect other inspectors running in parallel
- Is logged at error level
- Triggers a `Completeness::Failed` annotation on the snapshot

### Artifact Emission Policy

| Inspector Result | Snapshot JSON | Containerfile / Config Tree | Audit Report | Secrets Review |
|-----------------|--------------|----------------------------|-------------|----------------|
| `Ok(InspectorOutput)` | Full data | Full contribution | Full entry | Full scan |
| `Err(Degraded { partial, .. })` | Partial data + reason | Contributes partial data with `# FIXME:` comment noting what's missing | Warning banner listing gaps | Scans what's present |
| `Err(Failed { .. })` | Error reason | **Excluded** — no unreliable data in build artifacts | Failure entry explaining what went wrong and why | Excluded |
| `Err(Skipped { .. })` | Reason | No contribution | Noted as skipped with reason | No contribution |

**The Containerfile is the artifact users build from.** It must never contain unreliable data. Degraded sections contribute their valid data with explicit FIXME markers. Failed sections are excluded entirely.

**The snapshot JSON always contains everything** — success, degraded, failed, and skipped sections with their status clearly marked. It is the raw truth; renderers apply editorial judgment.

### Snapshot-Level Completeness

The `Completeness` field on the snapshot aggregates across all sections:

| Condition | Snapshot Completeness |
|-----------|----------------------|
| All sections Success | `Complete` |
| Any section Degraded, none Failed | `Partial` (lists degraded sections) |
| Any section Failed | `Incomplete` (lists failed sections) |

## Sensitive Input Trust Contract

Phase 2 introduces inspectors that read data sources carrying secrets or security-sensitive state. Each source is classified by trust handling:

### Classification-Only Sources (data informs output but content is never persisted)

| Source | Inspector | Sensitive Content | Handling |
|--------|-----------|-------------------|----------|
| `/etc/shadow` | users | Password hashes, expiry data | Parse expiry/status fields only. Hash values are never stored in any section data, snapshot field, or rendered artifact. |
| `~/.ssh/authorized_keys` | users | Public key material | Record presence/count, not key content. |
| sudoers | users | Privilege rules | Rules are persisted (needed for provisioning), but redaction engine scans for embedded passwords/tokens. |
| Proxy env vars | network | May contain credentials in URLs | Redaction engine scans for embedded credentials. URLs are stored with credentials masked. |

### Persisted Sources (content appears in snapshot or artifacts)

| Source | Inspector | Sensitive Content | Handling |
|--------|-----------|-------------------|----------|
| Docker-compose YAML | containers | May contain env vars, secrets references | Redaction engine scans. Environment values with secret-like names are redacted. |
| `podman inspect` | containers | May contain env vars, mount paths | Redaction engine scans environment section. |
| `strings` output | nonrpm | May extract embedded secrets from binaries | 4KB head scan limit. Redaction engine scans results. Only version strings are persisted; other matches are classified but not stored. |
| `dnf download + rpm2cpio` | config | Original package file content | Opt-in enrichment only (disabled by default). Diffs are text, scanned by redaction engine. Never runs implicitly — requires explicit flag. |

### Trust Rule

All inspector output passes through the existing redaction engine before reaching any renderer. The `RedactionState` on the snapshot confirms whether redaction ran and whether it found anything. No inspector may bypass the redaction pipeline to write directly to rendered artifacts.

## Collector Execution Contract

All shell commands run through the `Executor` trait with explicit rules:

### Command Execution Rules

1. **Fixed argv, never shell strings.** Commands are specified as `(&str, &[&str])` — program name + argument array. No shell expansion, no string interpolation, no `sh -c` wrappers. This eliminates shell injection by construction.

2. **`LANG=C` / `LC_ALL=C` normalization.** All commands run with locale forced to C/POSIX. Prevents locale-dependent output formatting from breaking parsers (e.g., `systemctl` column alignment, `rpm -Va` field widths).

3. **PATH is not assumed.** Commands use absolute paths or are resolved against a known set (`/usr/bin/`, `/usr/sbin/`). No reliance on the host's `$PATH`.

4. **Timeout enforcement.** Each command has a per-inspector timeout (default: 30 seconds, configurable). Commands exceeding their timeout are killed and the inspector reports `Degraded` with the timed-out command listed.

5. **Output size limits.** Command stdout is capped at 64 MB. Commands exceeding this (e.g., an abnormally large `rpm -qa` on a bloated system) are truncated and the inspector reports `Degraded`.

6. **File size limits for direct reads.** When inspectors read files directly (e.g., `/etc/fstab`, firewalld zone XMLs), individual files are capped at 1 MB. Files exceeding this are skipped with a warning.

### Parser Failure Modes

Parsers for each format handle malformed input explicitly:

| Format | Inspectors | Failure Mode |
|--------|-----------|-------------|
| JSON (`findmnt`, `lvs`, `podman`) | storage, containers | `serde_json` deserialization error → `Degraded`, log malformed output |
| XML (firewalld zones) | network | XML parse error → skip zone, `Degraded` |
| INI (NM keyfiles) | network | Key-value parse error → skip file, `Degraded` |
| Cron expressions | scheduled | Invalid field → flag entry as unparseable, include raw line |
| YAML (docker-compose) | containers | Parse error → `Degraded`, record file path |
| ELF (`readelf`) | nonrpm | Parse error → classify as "unknown binary" |
| `semanage` tabular output | selinux | Column mismatch → skip entry, `Degraded` |

No parser panics. All parse paths return `Result` types that map to section-level `Degraded` status.

## Parity Verification

### Three-Tier Model

**Tier 1 — Per-section JSON parity (CI, every PR, cumulative per-slice):**

Section-by-section comparison against Go v13 golden output. Each slice expands the gate:
- After 2a: RPM + services + storage + kernelboot (4 sections)
- After 2b: + network + containers + users (7 sections)
- After 2c: + scheduled + config + selinux + nonrpm (all 10 sections)

Cumulative — a new inspector can't silently break an already-passing section. Zero undocumented divergences; CI fails on any diff not in the allowlist.

**Tier 2 — Renderer smoke tests (CI, every PR, per-slice):**

For each slice, verify that rendered artifacts:
- Build without error from the current section set
- Contain expected content from newly added sections (e.g., "services section heading appears in audit report")
- Respect the artifact emission policy (degraded sections have FIXME markers, failed sections are absent)

These are content assertions, not line-for-line Go comparison. The renderers already exist from Phase 1; these tests validate they correctly consume new section data.

**Tier 3 — Full rendered-artifact exit gate (one-time, Phase 2 completion):**

Run Go and Rust on the same package-mode system. Compare the complete tarball contents — all always-written artifacts:
- `inspection-snapshot.json` (full snapshot comparison)
- `Containerfile` (layer ordering, package grouping, FIXME placement)
- `report.html` (interactive PatternFly report — section coverage, completeness banners)
- `audit-report.md` (section coverage, finding detail)
- `secrets-review.md` (redaction coverage)
- `config/` tree (file selection, directory structure)
- `README.md` (build instructions, FIXME checklist)
- `kickstart-suggestion.ks` (conditional content)
- `schema/snapshot.schema.json` (schema accuracy)

This is durable review evidence — a committed artifact with host details, Go/Rust versions, and a reviewable diff. Not a "ran it on my laptop" claim.

### Divergence Allowlist Governance

`testdata/divergences.md` tracks all accepted differences between Go and Rust output.

**Entry format:**
```markdown
### [Section].[Field] — [short description]

- **Go output:** [what Go produces]
- **Rust output:** [what Rust produces]
- **Reason:** [why the divergence is acceptable]
- **Disposition:** permanent | temporary (target: [phase/date])
- **Approved by:** [reviewer name, date]
```

**Rules:**

1. **Every divergence requires review approval.** Tang proposes entries; Thorn or Mark approves. No self-approved divergences.
2. **Disposition is explicit.** Each entry is either `permanent` (intentional Rust improvement, accepted forever) or `temporary` (known debt with a target phase/date for resolution).
3. **Temporary divergences are backlog items.** When a temporary divergence is added, a corresponding backlog item is created with the target phase.
4. **Phase 2 exit requires zero unapproved divergences.** Every entry in the allowlist must have an approval annotation.
5. **The allowlist is append-only within a slice.** Divergences are never silently removed — resolved entries are marked `resolved` with the commit that fixed them.

## Testing Strategy

### Layer 1: Unit Tests with FakeExecutor

Each inspector gets 15-25 tests covering:
- Happy path with well-formed command output
- Missing/failed commands → `Degraded` status with correct gap reporting
- Malformed output → graceful parsing failure, no panics
- `source_system.kind()` branching in internal match arms (package-mode produces data; bootc/ostree arms are unreachable in Phase 2 but must compile)
- Empty cases (no services, no cron jobs, etc.)
- Dependency failure (for Slice 2c inspectors: RPM unavailable → `Failed`)

Fixtures live in `testdata/fixtures/<inspector>/` as raw command output files. Tests use `insta` for snapshot assertions — readable diffs on behavior changes.

### Layer 2: Per-Section Parity Gate

(Covered under Parity Verification Tier 1 above.)

### Layer 3: Integration Test (Local, Durable Evidence)

Full scan on a real RHEL-family package-mode system. Validates that actual commands produce parseable output and catches fixture-vs-reality drift.

**Per-slice evidence requirements:**

Each slice closure requires a committed evidence artifact at `testdata/evidence/slice-2{a,b,c}-host-validation.md` containing:
- Host OS version, kernel, architecture
- Go version and inspectah Go version
- Rust toolchain version and inspectah Rust version
- List of commands that succeeded/failed on this host
- Section-by-section comparison summary (pass/diverge/fail)
- Date and who ran it

This is reviewable, deterministic evidence. It lives in the repo so Thorn and Mark can inspect it during slice review.

### Layer 4: Failure Policy Tests

Dedicated tests for the failure/trust model:
- RPM inspector fails → dependent inspectors return `Failed` with dependency reason
- Inspector panics → caught at thread boundary, `Failed` status, other inspectors unaffected
- Degraded section → Containerfile includes data with FIXME, audit report shows warning
- Failed section → Containerfile excludes section entirely
- Redaction engine processes all inspector output before rendering

### Test Count Target

Phase 1: ~216 tests. Phase 2 target: ~450+ tests at completion (unit + parity + smoke + failure policy).

## CI Pipeline

### Tier 1 — `ubuntu-latest`, every push/PR to `rust` branch

- `cargo fmt --check`
- `cargo clippy -- -W clippy::all`
- `cargo test` (without `ffi-rpm` feature)

Covers all FakeExecutor unit tests, parity gate comparisons, renderer smoke tests, and failure policy tests. No special dependencies.

### Tier 2 — Fedora container job, every push/PR

- `cargo test --features ffi-rpm`
- Runs in `fedora:latest` container with `rpm-devel` installed
- Validates librpm FFI path

### Deferred: Self-hosted RHEL VM runner

Full integration tests on a booted RHEL system. Needed eventually for Layer 3 automation. Not blocking Phase 2 — integration tests produce durable evidence artifacts reviewed at slice closure.

## Inspector Implementation Pattern

Every inspector follows the same shape, implementing the approved `Inspector` trait from `inspectah-core`:

1. **Location:** `inspectah-collect/src/<inspector>.rs` — one module per inspector.

2. **Trait:** Implements `Inspector: Send + Sync` from `inspectah-core`.
   - `id(&self) -> InspectorId` — unique identifier for this inspector.
   - `applicable_to(&self) -> &[SourceSystemKind]` — static declaration of supported system kinds. Phase 2 inspectors return `&[SourceSystemKind::PackageBased]`. The orchestrator checks this before calling `inspect()`.
   - `inspect(&self, ctx: &InspectionContext) -> Result<InspectorOutput, InspectorError>` — collection entry point. Returns `Ok(InspectorOutput)` on success, `Err(Degraded { partial, reason })` on partial collection, `Err(Failed { reason })` on inability to produce output.

3. **Context branching:** When an inspector needs system-kind branching internally, it calls `ctx.source_system.kind()` — not a stored peer field. Compiler-checked `match` on `SourceSystemKind`. In Phase 2, non-package-mode arms are unreachable (the orchestrator filters via `applicable_to()`), but the match ensures they're addressed when Phase 3+ expands the supported kinds.

4. **Error handling:** `Result<InspectorOutput, InspectorError>` per the core contract. Dependency failure (RPM unavailable) returns `Err(Failed { reason: "RPM dependency unavailable" })`, distinguished from local command failures which return `Err(Degraded { partial, reason })`.

5. **Shell commands:** Via `ctx.executor.run_command()` with fixed argv (never shell strings), `LC_ALL=C`, timeout enforcement, and output size limits.

6. **Redaction:** All `InspectorOutput` passes through the redaction engine before reaching any renderer. `redaction_hints` in the output guide the engine. No inspector writes to rendered artifacts directly.

## Delivery Cadence

Each slice follows the SDD (Spec-Driven Development) cadence:

1. Tang implements the inspectors for the slice
2. Tang delivers phase completion report (what's done, test coverage, parity status, failure policy coverage, what's next)
3. Thorn checkpoints code quality and reviews divergence allowlist entries
4. Section parity gate + renderer smoke tests pass for all sections delivered so far
5. Host validation evidence artifact committed and reviewed
6. Mark reviews before the next slice begins

## Exit Criteria

Phase 2 is complete when:
- [ ] All 10 Go inspectors implementing approved `Inspector` trait with typed `SectionData` via `InspectorOutput`
- [ ] `RpmState` capability contract implemented and consumed by 4 dependent inspectors
- [ ] Per-section parity gate passing for all 10 sections (Tier 1)
- [ ] Renderer smoke tests passing for all 10 sections (Tier 2)
- [ ] Full rendered-artifact exit gate: Go vs Rust tarball comparison on same host, committed as durable evidence (Tier 3)
- [ ] Three-wave parallel execution working end-to-end
- [ ] CI workflow running Tier 1 + Tier 2 on every PR
- [ ] ~450+ tests (unit + parity + smoke + failure policy)
- [ ] Failure policy implemented: `Err(Degraded)` contributes with FIXME, `Err(Failed)` excluded from build artifacts
- [ ] Panic containment tested: inspector panics caught at thread boundary
- [ ] `Completeness` field accurate across all sections (Complete/Partial/Incomplete)
- [ ] `RedactionState` confirms all inspector output passed through redaction
- [ ] Divergence allowlist fully documented with per-entry review approval
- [ ] Zero temporary divergences without corresponding backlog items
- [ ] Per-slice host validation evidence artifacts committed
- [ ] All 10 inspectors implement `applicable_to(&self) -> &[SourceSystemKind]` returning only `PackageBased`; orchestrator returns `Err(Skipped)` for bootc/ostree systems
- [ ] Tang's final phase completion report delivered

## Out of Scope

- CLI flag expansion (stays at `scan --inspect-only --output` and `version`)
- Refine server, fleet aggregation, architect, build command
- Packaging changes (COPR/Homebrew remain on Go binary)
- Bootc/ostree correctness testing (Phase 3+ with dedicated golden files)
- hardware/ostree inspectors (unless they fit naturally into a slice)
- Self-hosted CI runner for integration tests
- `dnf download + rpm2cpio` diff generation (opt-in flag, deferred to Phase 3 CLI expansion)
