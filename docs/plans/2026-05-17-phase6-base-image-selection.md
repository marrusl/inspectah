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
| `inspectah-collect/src/baseline.rs` | `extract_baseline()` — nsenter + podman orchestration with entrypoint override, container lifecycle, NEVRA parsing, repository-side digest capture. Returns `BaselineData` (digest + packages + timestamp, no ref — ref lives in `TargetImageIdentity`) |
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
- `BaselineData` struct (image_digest, packages HashMap, extracted_at). **No `normalized_ref` field** — the canonical ref lives in `TargetImageIdentity` only, avoiding a second carrier.
- `TargetImageIdentity` struct — stores the **normalized** ref string + strategy. This is the single authoritative image identity field in the snapshot. `BaselineData` does not duplicate it.

**Invariant:** `target_image.image_ref` is the one canonical ref. `BaselineData` carries extraction results (digest, packages, timestamp) but not the ref itself. When baseline data is present, `target_image` is always also present — the legal state table enforces this.
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
- **Pre-Phase-6 migration:** snapshot with `schema_version: 14` and no `target_image`/`baseline`/`no_baseline` fields deserializes via `serde(default)` (all `None`/`false`), then `migrate()` sets `schema_version = 15` AND sets `no_baseline = true`. This is explicit: a legacy snapshot has no baseline data, so `no_baseline = true` is the honest state. No refine-layer heuristic needed.

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

Migration: `serde(default)` handles missing fields (all `None`/`false`). `migrate()` bumps version to 15 AND sets `no_baseline = true` when `baseline` is `None` and `no_baseline` is `false` (the serde default for a pre-Phase-6 snapshot). This ensures legacy snapshots enter degraded mode explicitly, not via refine-layer heuristics.

```rust
pub fn migrate(snap: &mut InspectionSnapshot) {
    if snap.schema_version >= SCHEMA_VERSION {
        return;
    }
    // v14→v15: legacy snapshots have no baseline data — mark explicitly
    if snap.schema_version <= 14 && snap.baseline.is_none() && !snap.no_baseline {
        snap.no_baseline = true;
    }
    snap.schema_version = SCHEMA_VERSION;
}
```

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

**Happy path:** Mock executor returns success for pull → create → start → exec (NEVRA output) → rm, plus a separate image inspect for digest. `extract_baseline` takes `&NormalizedImageRef` (not the full resolution struct — the ref is the only thing needed for podman commands). Verify packages parsed correctly, digest captured.

**Command ordering proof:** Assert the exact sequence: pull, create, start, exec (rpm -qa), rm. `podman inspect` for digest runs SEPARATELY on the IMAGE object (not the container) — assert this is a `podman inspect <image_ref>` call, not `podman inspect <container>`.

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
- Modify: `inspectah-collect/src/inspectors/rpm/mod.rs`
- Modify: `inspectah-pipeline/src/collect.rs`

**Typed handoff boundary:** `BaselineData` flows from the CLI into the collect pipeline via `inspectah-pipeline/src/collect.rs`'s `collect()` function, which already accepts configuration and the executor. Add `baseline: Option<&BaselineData>` to `collect()`'s parameters. `collect()` passes `baseline.map(|b| &b.packages)` to the RPM inspector. The RPM inspector (`inspectah-collect/src/inspectors/rpm/mod.rs`) passes it to the classifier (`classifier.rs`). This is the same pattern used for `source_repos` threading.

- [ ] **Step 1: Write classifier tests with baseline data**

Test that when `baseline_packages: Option<&HashMap<String, BaselinePackageEntry>>` is provided to the RPM classifier:
- A package present in baseline (by `name.arch` key) with same EVR gets `PackageState::Added` with `include: true`
- A package in baseline with different EVR gets `PackageState::Modified`
- A package NOT in baseline but from a recognized repo keeps `PackageState::Added`
- The `base_image_only` partition is populated from baseline entries not found on the host
- Without baseline (`None`), behavior is identical to Phase 5 (no regression)

- [ ] **Step 2: Wire `BaselineData` into the classifier**

The RPM classifier currently assigns `PackageState` based on host-only data. Add `baseline_packages: Option<&HashMap<String, BaselinePackageEntry>>` to the classify function. When present:
- Check each host package against baseline by `name.arch` key
- If present with same EVR → `PackageState::Added`
- If present with different EVR → `PackageState::Modified`
- Populate `base_image_only` from baseline entries not found on host
- When `None`, existing Phase 5 behavior unchanged

- [ ] **Step 3: Thread through the collect pipeline**

Update `inspectah-pipeline/src/collect.rs`'s `collect()` to accept `baseline: Option<&BaselineData>` and pass `baseline.map(|b| &b.packages)` to the RPM inspector. Update `inspectah-collect/src/inspectors/rpm/mod.rs`'s `inspect()` to accept and forward the baseline packages map.

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

`ViewResponse` gains `baseline_summary: Option<BaselineSummary>`. Web handler calls `session.baseline_summary()` — does NOT derive independently. If the existing `ViewResponse` or web frontend has a separate `baseline_available` boolean or similar signal, **retire it** and replace all reads with `baseline_summary.is_some()`. One signal, one truth path.

**Incompatible service carrier:** The normalized `ServiceStateChange` entry carries `include: false` + `attention_reason: "service-image-mode-incompatible"` + `reason: "..."` as serialized fields. This is the single carrier for all surfaces:
- Containerfile renderer reads `include == false` to exclude from `systemctl enable`
- Web UI reads `attention_reason` to render the incompatible badge
- Export tarball reads the same projected snapshot that preview uses
No surface re-derives incompatibility from the constant list independently — all read the already-normalized entry.

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

```rust
// 1. Resolve
eprintln!("Resolving target image...");
let resolution_result = resolve_base_image(
    &os_release, ublue.as_ref(), bootc_ref.as_deref(), args.base_image.as_deref(),
);

// 2. Handle resolution result — produce target_image and normalized ref
let (target_image, normalized_ref) = match resolution_result {
    Ok(res) => {
        let norm = normalize_image_ref(&res.image_ref)?;
        eprintln!("  {} ({})", norm.as_str(), res.strategy_label());
        let ti = TargetImageIdentity {
            image_ref: norm.as_str().to_string(),  // NORMALIZED ref persisted
            strategy: res.strategy,
        };
        (Some(ti), Some(norm))
    }
    Err(e) if args.no_baseline => {
        eprintln!("  not found ({}), continuing without baseline", e);
        (None, None)  // target_image = null, FROM will be omitted
    }
    Err(e) => return Err(e.into()),  // fail fast in normal mode
};

// 3. Extract baseline (only when baseline mode is active and resolution succeeded)
let baseline_data = match (&normalized_ref, args.no_baseline) {
    (Some(norm), false) => {
        eprintln!("Pulling target image...");
        let data = extract_baseline(&executor, norm)?;
        eprintln!("Pulling target image... done");
        eprintln!("Extracting baseline... {} packages", data.packages.len());
        Some(data)
    }
    _ => None,
};

// 4. Set snapshot fields
snapshot.target_image = target_image;
snapshot.baseline = baseline_data;
snapshot.no_baseline = args.no_baseline;
```

Key design points:
- `resolution_result` is consumed once in the match — no lingering borrows
- `extract_baseline` takes `&NormalizedImageRef` (not the resolution struct — the ref is the only thing it needs)
- Resolution failure in `--no-baseline` mode sets `target_image = None` explicitly
- Normal mode resolution failure aborts immediately

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
- Modify: `inspectah-web/ui/src/api/types.ts` — add `BaselineSummary` TypeScript type and add field to `ViewResponse` type
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — render the verification/degraded banner in the packages section header
- Modify: `inspectah-web/ui/src/hooks/useView.ts` — thread `baseline_summary` from API response if not already handled by the generic `ViewResponse` type

If an existing `baseline_available` boolean or similar signal exists in `types.ts` or `useView.ts`, **retire it** — all consumers switch to `baseline_summary != null`. One signal.

- [ ] **Step 1: Add `BaselineSummary` type to `types.ts`**

```typescript
export interface BaselineSummary {
  image_ref: string;
  image_digest: string;
  strategy: string;
  baseline_count: number;
  user_added_count: number;
  review_count: number;
}
```

Add `baseline_summary?: BaselineSummary` to the existing `ViewResponse` interface.

- [ ] **Step 2: Add banner rendering in `MainContent.tsx`**

In the Packages section header, read `baseline_summary` from view data:

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

- [ ] **Step 3: Test in browser**

Start dev server (`cd inspectah-web/ui && npm run dev`), verify both banner states, verify no UI regressions.

- [ ] **Step 4: Commit**

```bash
git add inspectah-web/ui/src/
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
