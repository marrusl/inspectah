# Phase 6: Base Image Selection & Baseline Extraction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded Containerfile FROM with auto-detected/overridable base image, extract real baseline packages from the target image, and upgrade classification to use accurate baseline data.

**Architecture:** New `baseline` modules in `inspectah-core` (pure resolution/normalization logic) and `inspectah-collect` (image pull + extraction via nsenter). Snapshot schema adds top-level `target_image` and `baseline` fields independent of `RpmSection`. Classification matrix in `inspectah-refine` upgraded with exhaustive `PackageState × baseline mode` coverage. Collect-side RPM classifier updated so `PackageState` assignment uses baseline data at collection time. `inspectah-refine` is the canonical owner of derived baseline summary. Pipeline order: resolve → normalize ref → pull + extract → scan host.

**Tech Stack:** Rust (edition 2024), serde/serde_json for schema, clap for CLI, axum for web API, existing `Executor` trait for host commands. Workspace version `0.8.0-alpha.1`, current `SCHEMA_VERSION = 14` (bumps to 15).

**Spec:** `docs/specs/proposed/2026-05-17-phase6-base-image-selection-design.md` (revision 3, approved round 3)

**Execution:** SDD cadence — Tang implements, Thorn code quality review.

**Plan revision 2:** Addresses round 1 plan review blockers: canonical normalized-ref persistence, no hardcoded FROM fallback, correct digest source, collect-side classifier seam, stronger normalization validation, and expanded proof gates. Also adds version floor clamping (RHEL 9.6, Fedora 41).

---

## Canonical Target-Image Truth Contract

**One identity flows through the entire system.** Resolution produces a raw ref. Normalization validates and canonicalizes it. The **normalized** ref is what gets persisted into `target_image`, used for `podman pull`, rendered in the Containerfile FROM, shown in the banner, and reconstructed on reopen/export. No surface may use the raw resolved ref after normalization.

```
resolve_base_image() → raw ref
    → normalize_image_ref() → NormalizedImageRef
        → persisted in snapshot.target_image.image_ref ← single source of truth
        → used by podman pull
        → used by Containerfile FROM
        → used by BaselineSummary banner
        → used by reopen/export
```

When `target_image` is `null` (resolution failed in `--no-baseline` mode), the Containerfile emits an omission comment. No hardcoded fallback. Ever.

---

## File Map

### New files

| File | Responsibility |
|------|---------------|
| `inspectah-core/src/baseline.rs` | `BaseImageResolution`, `ResolutionStrategy`, `NormalizedImageRef`, `BaselineData`, `BaselinePackageEntry`, `TargetImageIdentity`, `IncompatibleServiceEntry`, `UblueMetadata`, `resolve_base_image()`, `normalize_image_ref()`, `clamp_version()`, version floor constants, incompatible services constant |
| `inspectah-collect/src/baseline.rs` | `extract_baseline()` — nsenter + podman orchestration with entrypoint override, container lifecycle, NEVRA parsing, repository-side digest capture |
| `inspectah-collect/tests/baseline_test.rs` | Extraction tests: NEVRA parsing, exact `--entrypoint`/`--network none` arg assertion, command ordering proof, cleanup across all failure points, mixed-arch baseline keys, digest capture from image (not container) |
| `inspectah-refine/src/baseline_summary.rs` | `BaselineSummary` derivation from classification counts (not mutable `include` state) |

### Modified files

| File | Changes |
|------|---------|
| `inspectah-core/src/lib.rs` | Add `pub mod baseline;` |
| `inspectah-core/src/snapshot.rs` | Add `target_image` and `baseline` to `InspectionSnapshot`, bump `SCHEMA_VERSION` to 15, migration maps missing fields to degraded mode (`no_baseline = true`) |
| `inspectah-collect/src/lib.rs` | Add `pub mod baseline;` |
| `inspectah-collect/src/executor/mock.rs` | Add command-log recording (`Vec<String>`) and `with_command_prefix()` for flexible matching |
| `inspectah-collect/src/inspectors/rpm/classifier.rs` | Wire `BaselineData` into `PackageState` assignment so `packages_added` vs `base_image_only` partitioning is baseline-aware at collection time |
| `inspectah-refine/src/lib.rs` | Add `pub mod baseline_summary;` |
| `inspectah-refine/src/attention.rs` | Add baseline-aware attention reasons, exhaustive classification matrix |
| `inspectah-refine/src/normalize.rs` | Incompatible service filtering — one authoritative post-normalization representation |
| `inspectah-refine/src/session.rs` | Materialize `BaselineData`, derive `BaselineSummary` from original classification counts |
| `inspectah-pipeline/src/render/containerfile.rs` | Replace `base_image_from_snapshot()`: read `target_image.image_ref` (normalized), no hardcoded fallback, omission comment when null |
| `inspectah-cli/src/commands/scan.rs` | `--base-image` and `--no-baseline` flags (mutually exclusive), resolution + extraction before host scan, progress output. Resolution failure in `--no-baseline` produces `target_image = null`, not swallowed with `.ok()` |
| `inspectah-web/src/handlers.rs` | Add `baseline_summary` to `ViewResponse`, serialized from `RefineSession` (not derived by web layer) |

---

## Task 1: Core Baseline Types

**Files:**
- Create: `inspectah-core/src/baseline.rs`
- Modify: `inspectah-core/src/lib.rs`

- [ ] **Step 1: Write types, version floor constants, incompatible services constant, and tests**

Create `inspectah-core/src/baseline.rs` with all types from the spec:

- `ResolutionStrategy` enum (5 variants, `#[serde(rename_all = "kebab-case")]`)
- `BaseImageResolution` struct (raw `image_ref` + `strategy`)
- `NormalizedImageRef` struct (private `ref_string` field, `from_validated()` constructor, `as_str()` accessor, `Display` impl)
- `BaselinePackageEntry` struct (name, epoch, version, release, arch)
- `BaselineData` struct (resolution, normalized_ref, image_digest, packages HashMap, extracted_at)
- `TargetImageIdentity` struct — stores the **normalized** ref string + strategy. This is what gets persisted in the snapshot.
- `IncompatibleServiceEntry` struct (unit, reason) and `INCOMPATIBLE_SERVICES` constant (4 entries)
- `UblueMetadata` struct with serde rename for `image-ref`, `image-tag`, `image-name`, `image-vendor`
- Version floor constants:
  ```rust
  pub const RHEL_BOOTC_MIN: &[(&str, &str)] = &[("9", "9.6"), ("10", "10.0")];
  pub const FEDORA_BOOTC_MIN: u32 = 41;
  ```
- Error types: `ResolutionError`, `NormalizationError`

Tests:
- `BaselineData` serde roundtrip with NEVRA
- `ResolutionStrategy` serde produces kebab-case (`"os-release"`, `"cli-override"`, `"fedora-atomic-desktop"`)
- `TargetImageIdentity` roundtrip
- `INCOMPATIBLE_SERVICES` has exactly 4 entries with expected unit names
- Version floor constants are correct

- [ ] **Step 2: Add `pub mod baseline;` to `inspectah-core/src/lib.rs`**

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-core -- baseline`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/baseline.rs inspectah-core/src/lib.rs
git commit -m "feat(core): add Phase 6 baseline types, version floors, and incompatible services constant"
```

---

## Task 2: Base Image Resolution Chain

**Files:**
- Modify: `inspectah-core/src/baseline.rs`

- [ ] **Step 1: Write resolution tests**

Test every path in the resolution chain:

- CLI override wins over all other strategies
- UBlue with transport-prefixed tagless `image-ref` + separate `image-tag` → combined ref
- UBlue with already-tagged `image-ref` → used as-is
- UBlue synthesis fallback (no `image-ref`, has vendor/name/tag)
- UBlue tagless ref without `image-tag` → fail closed (not fall through)
- UBlue malformed metadata → `ResolutionError::MalformedUblueMetadata`
- bootc status ref → `ResolutionStrategy::BootcStatus`
- Fedora Atomic desktop (`VARIANT_ID=silverblue`) resolves BEFORE generic Fedora
- Generic Fedora (no variant) → `fedora-bootc`
- CentOS Stream → `centos-bootc:stream{MAJOR}`
- RHEL → `rhel{MAJOR}/rhel-bootc:{VERSION_ID}`
- **RHEL version floor:** RHEL 9.4 → clamped to `rhel-bootc:9.6`
- **RHEL version floor:** RHEL 9.6 → `rhel-bootc:9.6` (no change)
- **RHEL version floor:** RHEL 10.0 → `rhel-bootc:10.0` (at floor)
- **Fedora version floor:** Fedora 40 → clamped to `fedora-bootc:41`
- **Fedora version floor:** Fedora 42 → `fedora-bootc:42` (above floor)
- Unknown distro → `ResolutionError::UnknownDistro`
- All 7 desktop variants resolve correctly

- [ ] **Step 2: Implement `clamp_version()` and `resolve_base_image()`**

Port `clampVersion()` from Go: compare dot-separated integer components, return max(version, minimum).

Resolution chain:
1. CLI override → `CliOverride`
2. UBlue: strip transport prefix, combine with `image-tag` if tagless, fail closed on malformed → `UniversalBlue`
3. bootc status → `BootcStatus`
4. Fedora Atomic desktop (variant in known set) → `FedoraAtomicDesktop`
5. os-release mapping with version clamping → `OsRelease`

UBlue metadata path: `/usr/share/ublue-os/image-info.json`

Transport prefix stripping: `ostree-image-signed:docker://`, `docker://`, `containers-storage:`

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-core -- baseline`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/baseline.rs
git commit -m "feat(core): implement resolution chain with UBlue, Fedora Atomic, and version floor clamping"
```

---

## Task 3: Ref Normalization Gate

**Files:**
- Modify: `inspectah-core/src/baseline.rs`

- [ ] **Step 1: Write normalization tests**

- Transport prefix stripping (all 3 prefixes)
- Empty ref → `NormalizationError::Empty`
- Whitespace / shell metacharacters → `NormalizationError::InvalidCharacters`
- Bare ref without registry (`rhel-bootc:9.6`) → `NormalizationError::NotFullyQualified`
- **Registry hostname validation:** `foo/bar:tag` (no dot in first component) → `NotFullyQualified`. `registry.redhat.io/rhel9/rhel-bootc:9.6` (dot in first component) → accepted. `localhost/foo:tag` → `NormalizationError::LocalOnly`.
- No tag, no digest → appends `:latest`
- Digest preserved (`@sha256:...`)
- Tag preserved (`:9.6`)

- [ ] **Step 2: Implement `normalize_image_ref()`**

Validation rules:
1. Non-empty, no whitespace/metacharacters
2. Strip transport prefixes
3. Reject `localhost/` and `containers-storage:` (local-only)
4. **Must be fully qualified:** first path component (before first `/`) must contain a `.` or a `:` (port) — this validates it as a registry hostname, not a namespace-only short name
5. If `@` present → digest ref, preserve as-is
6. If tag present → preserve
7. No tag, no digest → append `:latest`

Return `NormalizedImageRef` (validated, pullable).

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-core -- normalize_`
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/baseline.rs
git commit -m "feat(core): add ref normalization with registry hostname validation"
```

---

## Task 4: Snapshot Schema Changes

**Files:**
- Modify: `inspectah-core/src/snapshot.rs`

- [ ] **Step 1: Write schema tests**

- Snapshot with `target_image` + `baseline` roundtrips correctly
- `target_image.image_ref` stores the **normalized** ref (this is verified by setting it from `NormalizedImageRef.as_str()` and asserting it back)
- Degraded snapshot: `target_image` present, `baseline` null, `no_baseline = true` — roundtrips
- Degraded snapshot with null `target_image`: both null, `no_baseline = true` — roundtrips
- **Pre-Phase-6 migration:** snapshot with `schema_version: 14` and no `target_image`/`baseline`/`no_baseline` fields deserializes via `serde(default)`, then `migrate()` sets `schema_version = 15`. Result: `target_image = None`, `baseline = None`, `no_baseline = false` (serde default). The refine layer interprets missing `target_image` + missing `baseline` + `no_baseline = false` as a legacy snapshot and enters degraded mode for display.

- [ ] **Step 2: Add fields to `InspectionSnapshot`**

```rust
pub const SCHEMA_VERSION: u32 = 15;
```

Add after `completeness`:
```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub target_image: Option<crate::baseline::TargetImageIdentity>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub baseline: Option<crate::baseline::BaselineData>,
#[serde(default, skip_serializing_if = "crate::is_false")]
pub no_baseline: bool,
```

Migration: `serde(default)` handles missing fields. `migrate()` bumps version to 15. No special migration logic needed — the defaults produce a valid legacy state.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-core`
Expected: all pass (existing tests unaffected — new fields default to None/false).

- [ ] **Step 4: Commit**

```bash
git add inspectah-core/src/snapshot.rs
git commit -m "feat(core): add target_image and baseline to snapshot, bump schema to v15"
```

---

## Task 5: Mock Executor Enhancements + Baseline Extraction

**Files:**
- Modify: `inspectah-collect/src/executor/mock.rs`
- Create: `inspectah-collect/src/baseline.rs`
- Modify: `inspectah-collect/src/lib.rs`
- Create: `inspectah-collect/tests/baseline_test.rs`

- [ ] **Step 1: Add command-log recording and prefix matching to MockExecutor**

- `command_log: Mutex<Vec<String>>` — records every `run()` call in order
- `command_log(&self) -> Vec<String>` accessor
- `with_command_prefix(prefix, result)` — matches when command starts with prefix (for flexible arg matching)

- [ ] **Step 2: Write extraction tests**

In `inspectah-collect/tests/baseline_test.rs`:

**Happy path:** Mock executor returns success for pull → create → start → exec (NEVRA output) → rm. Verify packages parsed correctly, digest captured.

**Command ordering proof:** Assert the exact sequence: pull, create, start, exec (rpm -qa), rm. `podman inspect` for digest runs SEPARATELY on the image (not the container) — assert this is a `podman inspect <image_ref>` call, not `podman inspect <container>`.

**Exact arg assertion:** Verify the `podman create` command includes:
- `--entrypoint` with `["sleep", "infinity"]`
- `--network none`
- `--name inspectah-baseline-*`

**Cleanup on failure at each step:**
- Pull fails → no container to clean up, error returned
- Create fails → no container to clean up, error returned
- Start fails → `podman rm -f` runs, error returned
- Exec fails → `podman rm -f` runs, error returned

**Digest capture:** `podman inspect --format '{{.Digest}}' <image_ref>` on the IMAGE object, not the container. When `.Digest` is empty, fall back to `podman inspect --format '{{index .RepoDigests 0}}' <image_ref>` and extract the digest portion after `@`.

**Mixed-arch baseline:** When extracting from an aarch64 image on an x86_64 host, verify package keys use the IMAGE's architecture (`bash.aarch64`), not the host's.

- [ ] **Step 3: Implement `extract_baseline()`**

Create `inspectah-collect/src/baseline.rs`:

Extraction sequence:
1. `nsenter ... podman pull <normalized_ref>`
2. `nsenter ... podman create --name inspectah-baseline-<ts> --entrypoint '["sleep", "infinity"]' --network none <normalized_ref>`
3. `nsenter ... podman start <container>`
4. `nsenter ... podman exec <container> rpm -qa --queryformat '%{NAME}\t%{EPOCH}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n'`
5. `nsenter ... podman rm -f <container>` (drop guard — runs on all exit paths)
6. SEPARATELY: `nsenter ... podman inspect --format '{{.Digest}}' <normalized_ref>` on the IMAGE. Fallback: `podman inspect --format '{{index .RepoDigests 0}}' <normalized_ref>`.

Key: digest is queried from the IMAGE object, not the temporary container. The container is cleaned up before or after; digest capture is independent.

Container cleanup uses a drop guard struct that holds the container name and executor reference, ensuring `podman rm -f` runs even on panic.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-collect -- baseline`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-collect/src/baseline.rs inspectah-collect/src/lib.rs \
       inspectah-collect/src/executor/mock.rs inspectah-collect/tests/baseline_test.rs
git commit -m "feat(collect): baseline extraction with entrypoint override, digest from image, order proof"
```

---

## Task 6: Collect-Side RPM Classifier Update

**Files:**
- Modify: `inspectah-collect/src/inspectors/rpm/classifier.rs`

- [ ] **Step 1: Write classifier tests with baseline data**

Test that when `BaselineData` is provided to the RPM classifier:
- A package present in `BaselineData.packages` (by `name.arch` key) gets `PackageState::Added` with `include: true` (baseline match — the refine layer will assign attention)
- A package in `BaselineData.packages` with a different version gets `PackageState::Modified`
- A package NOT in `BaselineData.packages` but from a recognized repo keeps `PackageState::Added`
- The `base_image_only` partition is populated from `BaselineData.packages` entries not found on the host
- Without `BaselineData`, behavior is identical to Phase 5 (no regression)

- [ ] **Step 2: Wire `BaselineData` into the classifier**

The RPM classifier currently assigns `PackageState` based on host-only data. Add an optional `baseline_packages: Option<&HashMap<String, BaselinePackageEntry>>` parameter. When present:
- Check each host package against baseline by `name.arch` key
- If present with same EVR → `PackageState::Added` (the attention layer handles the rest)
- If present with different EVR → `PackageState::Modified`
- Populate `base_image_only` from baseline entries not found on host
- When `None`, existing Phase 5 behavior unchanged

This ensures the snapshot's `packages_added` vs `base_image_only` partition reflects baseline truth at collection time, not just at attention-assignment time.

- [ ] **Step 3: Run tests**

Run: `cargo test -p inspectah-collect -- rpm`
Expected: all pass (new + existing).

- [ ] **Step 4: Commit**

```bash
git add inspectah-collect/src/inspectors/rpm/classifier.rs
git commit -m "feat(collect): wire baseline data into RPM classifier for accurate PackageState partitioning"
```

---

## Task 7: Package Classification Matrix + Service Flagging

**Files:**
- Modify: `inspectah-refine/src/attention.rs`
- Modify: `inspectah-refine/src/normalize.rs`

- [ ] **Step 1: Write exhaustive classification tests**

Every cell of the spec's section 4 matrix. 14 cells across `PackageState × provenance × baseline mode`:

| PackageState | Repo provenance | Verified mode | Degraded mode |
|---|---|---|---|
| Added + recognized + in baseline | → Routine / BaselineMatch | → NeedsReview / ProvenanceUnavailable |
| Added + recognized + NOT in baseline | → Routine / UserAdded | → NeedsReview / ProvenanceUnavailable |
| Added + no repo | → NeedsReview(critical) / NoRepoSource | → same |
| Modified + recognized + in baseline | → NeedsReview / VersionChanged | → NeedsReview / ProvenanceUnavailable |
| Modified + recognized + NOT in baseline | → NeedsReview / VersionChanged | → NeedsReview / ProvenanceUnavailable |
| Modified + no repo | → NeedsReview(critical) / NoRepoSource | → same |
| LocalInstall | → NeedsReview(critical) / NoRepoSource | → same |
| NoRepo | → NeedsReview(critical) / NoRepoSource | → same |
| BaseImageOnly | → not rendered | → n/a |

- [ ] **Step 2: Write service flagging tests**

One authoritative post-normalization representation:
- `dnf-makecache.service` in `state_changes` → `include: false`, `attention_reason: ServiceImageModeIncompatible`, reason text from `INCOMPATIBLE_SERVICES`
- `httpd.service` → NOT flagged
- Flagged services removed from `enabled_units`
- **Surface agreement:** same normalized state consumed by UI, Containerfile preview, and export. Test that all three surfaces agree on a fixture with incompatible services.

- [ ] **Step 3: Implement classification and service flagging**

Add attention reason variants. Update classification to accept `Option<&BaselineData>`. Service flagging reads from `INCOMPATIBLE_SERVICES` constant (core), sets `include: false` + reason on matched services, removes from `enabled_units`.

Wire-level mapping: "Tier 3 (Critical)" is `NeedsReview` with a `severity: critical` flag, NOT a new `AttentionLevel` variant. Counts and completion key off `AttentionLevel`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p inspectah-refine`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add inspectah-refine/src/attention.rs inspectah-refine/src/normalize.rs
git commit -m "feat(refine): exhaustive baseline-aware classification matrix and service flagging"
```

---

## Task 8: Containerfile Dynamic FROM + BaselineSummary + ViewResponse

**Files:**
- Modify: `inspectah-pipeline/src/render/containerfile.rs`
- Create: `inspectah-refine/src/baseline_summary.rs`
- Modify: `inspectah-refine/src/lib.rs`
- Modify: `inspectah-refine/src/session.rs`
- Modify: `inspectah-web/src/handlers.rs`

- [ ] **Step 1: Write FROM line tests**

- `target_image` present → FROM uses `target_image.image_ref` (the normalized value)
- `target_image` present + `no_baseline = true` → same FROM (degraded still has correct FROM)
- `target_image` null + `no_baseline = true` → FROM omitted with comment: `# FROM line omitted — target image could not be determined. Use --base-image to specify.`
- **No hardcoded fallback.** Remove the `"registry.redhat.io/rhel9/rhel-bootc:9.4"` string entirely.
- Legacy Go snapshots with `rpm.base_image` set → still use that value (backward compat for Go-generated snapshots only, not Phase 6 Rust output)

- [ ] **Step 2: Update `base_image_from_snapshot()`**

```rust
pub fn base_image_from_snapshot(snap: &InspectionSnapshot) -> Option<String> {
    // Phase 6: prefer top-level target_image (stores normalized ref)
    if let Some(ref ti) = snap.target_image {
        return Some(ti.image_ref.clone());
    }
    // Backward compat: Go-generated snapshots with rpm.base_image
    if let Some(rpm) = &snap.rpm {
        if let Some(ref base) = rpm.base_image {
            if !base.is_empty() {
                return Some(base.clone());
            }
        }
    }
    // No target image resolved — caller decides (omission comment or error)
    None
}
```

Return type changes from `String` to `Option<String>`. Callers handle `None` with the omission comment. **No hardcoded fallback.**

- [ ] **Step 3: Implement `BaselineSummary`**

Create `inspectah-refine/src/baseline_summary.rs`:

```rust
pub struct BaselineSummary {
    pub image_ref: String,
    pub image_digest: String,
    pub strategy: String,
    pub baseline_count: usize,
    pub user_added_count: usize,
    pub review_count: usize,
}
```

**Count authority:** `baseline_count`, `user_added_count`, and `review_count` are derived from the **original classification result** (attention reasons assigned during normalization), NOT from mutable `include` booleans. This means:
- `baseline_count` = number of packages with `PackageBaselineMatch` reason
- `user_added_count` = number of packages with `PackageUserAdded` reason
- `review_count` = number of packages with `NeedsReview` attention level

These counts do NOT change when the user includes/excludes packages during refine. They reflect the classification state, not the triage state.

Add test: create a `BaselineSummary`, apply include/exclude ops to the session, re-derive summary — counts must be identical.

- [ ] **Step 4: Wire into RefineSession and ViewResponse**

`RefineSession::baseline_summary()` delegates to `derive_baseline_summary()`.

`ViewResponse` gains `baseline_summary: Option<BaselineSummary>`. Web handler calls `session.baseline_summary()` — does NOT derive independently.

- [ ] **Step 5: Run tests**

Run: `cargo test -p inspectah-pipeline -- containerfile && cargo test -p inspectah-refine -- baseline_summary`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add inspectah-pipeline/src/render/containerfile.rs \
       inspectah-refine/src/baseline_summary.rs inspectah-refine/src/lib.rs \
       inspectah-refine/src/session.rs inspectah-web/src/handlers.rs
git commit -m "feat(render): dynamic FROM from normalized target_image, BaselineSummary with stable counts"
```

---

## Task 9: CLI Integration

**Files:**
- Modify: `inspectah-cli/src/commands/scan.rs`

- [ ] **Step 1: Add CLI flags**

```rust
#[arg(long)]
pub base_image: Option<String>,

#[arg(long)]
pub no_baseline: bool,
```

- [ ] **Step 2: Flag validation**

`--base-image` + `--no-baseline` together → error.

- [ ] **Step 3: Wire resolution + extraction + progress**

```
// 1. Resolve
eprintln!("Resolving target image...");
let resolution = resolve_base_image(&os_release, ublue.as_ref(), bootc_ref.as_deref(), args.base_image.as_deref());

// 2. Handle resolution result
let (target_image, normalized) = match resolution {
    Ok(res) => {
        let norm = normalize_image_ref(&res.image_ref)?;
        eprintln!("Resolving target image... {} ({})", norm.as_str(), /* strategy */);
        let ti = TargetImageIdentity {
            image_ref: norm.as_str().to_string(),  // NORMALIZED ref persisted
            strategy: res.strategy.clone(),
        };
        (Some(ti), Some(norm))
    }
    Err(e) => {
        if args.no_baseline {
            eprintln!("Resolving target image... not found ({}), continuing without baseline", e);
            (None, None)  // target_image = null, FROM will be omitted
        } else {
            return Err(e.into());  // fail fast in normal mode
        }
    }
};

// 3. Extract (only if not --no-baseline and resolution succeeded)
let baseline_data = if !args.no_baseline {
    let norm = normalized.as_ref().unwrap();
    eprintln!("Pulling target image...");
    let data = extract_baseline(&executor, resolution.as_ref().unwrap(), norm)?;
    eprintln!("Pulling target image... done");
    eprintln!("Extracting baseline... {} packages", data.packages.len());
    Some(data)
} else {
    None
};

// 4. Set snapshot fields
snapshot.target_image = target_image;
snapshot.baseline = baseline_data;
snapshot.no_baseline = args.no_baseline;
```

Key: resolution failure in `--no-baseline` mode sets `target_image = None` (not swallowed with `.ok()`). The result is explicit: no target image → FROM omitted.

- [ ] **Step 4: Add progress output for host scan**

`[N/11]` stderr counting around inspector calls.

- [ ] **Step 5: Add UBlue metadata and bootc status reader helpers**

Read `/usr/share/ublue-os/image-info.json` via executor → parse as `UblueMetadata`.
Read `bootc status --json` via executor → extract `status.booted.image.image.image`.

**UBlue fail-closed test:** Add a test (here or in core) that malformed JSON at the metadata path produces an error, not a silent fallthrough.

- [ ] **Step 6: Run tests**

Run: `cargo test -p inspectah-cli`
Expected: compilation succeeds, existing tests pass.

- [ ] **Step 7: Commit**

```bash
git add inspectah-cli/src/commands/scan.rs
git commit -m "feat(cli): add --base-image and --no-baseline with normalized-ref persistence and progress"
```

---

## Task 10: Web UI Verification Banner

**Files:**
- Modify: `inspectah-web/frontend/src/` (banner component)

- [ ] **Step 1: Add banner rendering**

In the Packages section header, read `baseline_summary` from API response:

```tsx
{viewData.baseline_summary ? (
  <Alert variant="info" isInline title={
    `Baseline compared against ${viewData.baseline_summary.image_ref} ` +
    `(${viewData.baseline_summary.image_digest.substring(0, 19)}…) — ` +
    `${viewData.baseline_summary.baseline_count} in base image, ` +
    `${viewData.baseline_summary.user_added_count} user-installed, ` +
    `${viewData.baseline_summary.review_count} require review`
  } />
) : (
  <Alert variant="warning" isInline
    title="Baseline unavailable — all added packages shown as NeedsReview" />
)}
```

- [ ] **Step 2: Test in browser**

Start dev server, verify both banner states, verify no UI regressions.

- [ ] **Step 3: Commit**

```bash
git add inspectah-web/frontend/src/
git commit -m "feat(web): verification banner for baseline comparison status"
```

---

## Task 11: Integration Tests and Round-Trip Proofs

**Files:**
- Create/modify tests across crates

- [ ] **Step 1: Snapshot round-trip with NEVRA**

`BaselineData` with full NEVRA serializes → deserializes → produces identical classification and `BaselineSummary`.

- [ ] **Step 2: Degraded FROM persistence (target_image present)**

`--no-baseline` snapshot with resolved `target_image` → export → reimport → FROM line uses the persisted normalized ref.

- [ ] **Step 3: Degraded FROM persistence (target_image null)**

`--no-baseline` snapshot where resolution failed → `target_image = null` → export → reimport → FROM omission comment preserved.

- [ ] **Step 4: Pre-Phase-6 migration**

Phase 5 snapshot (schema v14, no new fields) → deserializes with defaults → migrates to v15 → refine layer enters degraded mode.

- [ ] **Step 5: Service surface agreement**

Incompatible service is:
- Excluded from `enabled_units` in Containerfile render
- Flagged with reason badge in service `state_changes`
- Absent from export tarball enabled units

All three surfaces read from the same normalized state.

- [ ] **Step 6: Preview/export parity**

Containerfile preview and exported tarball Containerfile agree on FROM line, package list, and service enablement.

- [ ] **Step 7: BaselineSummary count stability**

Create session → derive summary → apply include/exclude ops → re-derive summary → counts identical (counts reflect classification, not triage state).

- [ ] **Step 8: UBlue fail-closed helper test**

Malformed `/usr/share/ublue-os/image-info.json` → resolution error, not silent fallthrough to os-release strategy.

- [ ] **Step 9: Commit**

```bash
git add inspectah-refine/tests/ inspectah-core/tests/ inspectah-pipeline/
git commit -m "test: integration proofs — round-trip, degraded replay, surface agreement, count stability"
```

---

## Summary

| Task | Crate | What |
|------|-------|------|
| 1 | core | Baseline types, version floors, incompatible services constant |
| 2 | core | Resolution chain (5 strategies, UBlue, version clamping) |
| 3 | core | Ref normalization gate (registry hostname validation) |
| 4 | core | Snapshot schema v15 (target_image, baseline, migration) |
| 5 | collect | Baseline extraction (entrypoint override, digest from image, order proof) |
| 6 | collect | RPM classifier seam (baseline-aware PackageState assignment) |
| 7 | refine | Exhaustive classification matrix + service flagging |
| 8 | pipeline + refine + web | Dynamic FROM (no fallback), BaselineSummary (stable counts), ViewResponse |
| 9 | cli | --base-image, --no-baseline (no .ok() swallowing), progress output |
| 10 | web | Verification banner component |
| 11 | cross-crate | Integration proofs — round-trip, degraded replay, surface agreement |

Tasks 1–4 are pure types/logic. Task 5 requires MockExecutor enhancements. Task 6 wires baseline into collection. Tasks 7–8 are the core behavior changes. Task 9 integrates everything. Tasks 10–11 are UI and proof.
