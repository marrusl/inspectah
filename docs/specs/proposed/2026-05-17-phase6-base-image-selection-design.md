# Phase 6: Base Image Selection & Baseline Extraction

**Status:** Proposed
**Author:** Mark Russell (with team input from Collins, Ember, Fern, Tang)
**Date:** 2026-05-17

## Problem

The Containerfile renderer hardcodes `FROM rhel9/rhel-bootc:9.4` regardless of the source system's distribution or version. Phase 5's three-tier attention model classifies packages using approximated baseline data — the `PackageProvenanceUnavailable` path fires for every package when no real baseline exists. This produces inflated triage workloads (dozens of Tier 2 cards for packages that are obviously baseline) and incorrect Containerfile output.

Phase 6 makes baseline subtraction accurate by pulling the actual target bootc base image, extracting its package list, and using that as the ground truth for classification.

## Scope

### In scope

1. **Base image resolution** — auto-detect the correct bootc base image for the source system, with a `--base-image` CLI override for cross-distro conversion
2. **Baseline package extraction** — pull the target image, extract its package list via `rpm -qa` inside a temporary container
3. **Accurate package classification** — replace approximated baseline with real package set; promote `PackageUserAdded` (recognized repo, not in baseline) to Tier 1 auto-include
4. **Incompatible service flagging** — static list of services incompatible with image mode (dnf-makecache, packagekit)
5. **CLI scan progress feedback** — stage-by-stage stderr output (image pull adds latency)
6. **Verification banner** — web UI banner confirming which image the baseline was verified against

### Out of scope

- Service state extraction from the target image (vendor presets are the ground truth; static incompatible list is sufficient)
- Config file extraction from the target image (RPM-owned-default detection handles config baselines)
- Web UI base image selector (FROM line in Containerfile preview is sufficient)
- Fleet baseline aggregation (fleet mode is a later phase)
- Preset file parsing for service baselines (deferred unless needed)

## Design

### 1. Base Image Resolution

New `baseline` module in `inspectah-core`. Pure logic, no I/O — fully testable without an executor.

**Resolution chain** (first match wins):

| Priority | Strategy | Source | Example output |
|----------|----------|--------|---------------|
| 1 | CLI override | `--base-image` flag | Whatever the user provides |
| 2 | Universal Blue | `/usr/lib/image-info.json` | `image-ref` field, or synthesized from vendor/name/tag |
| 3 | bootc status | `bootc status --json` | `status.booted.image.image.image` |
| 4 | os-release mapping | `/etc/os-release` ID + VERSION_ID | See distro table below |
| 5 | rpm-ostree desktop | `/etc/os-release` VARIANT_ID | Fedora Atomic desktop variants |

**Distro mapping table (strategy 4):**

| ID | Image reference pattern |
|----|------------------------|
| `fedora` | `quay.io/fedora/fedora-bootc:{VERSION_ID}` |
| `centos` | `quay.io/centos-bootc/centos-bootc:stream{MAJOR}` |
| `rhel` | `registry.redhat.io/rhel{MAJOR}/rhel-bootc:{VERSION_ID}` |

**Fedora Atomic desktop variants (strategy 5):**

silverblue, kinoite, sway-atomic, budgie-atomic, lxqt-atomic, xfce-atomic, cosmic-atomic → `quay.io/fedora-ostree-desktops/{variant}:{VERSION_ID}`

**Return type:**

```rust
struct BaseImageResolution {
    image_ref: String,
    strategy: ResolutionStrategy,
}

enum ResolutionStrategy {
    CliOverride,
    UniversalBlue,
    BootcStatus,
    OsRelease,
    RpmOstreeDesktop,
}
```

Resolution returns `Option<BaseImageResolution>` — `None` when no mapping is possible (unknown distro, no bootc status, no UBlue file). When `None` and `--base-image` was not provided, the pipeline aborts with a clear error suggesting `--base-image` or `--no-baseline`.

**CLI flag:** `--base-image <IMAGE_REF>` on the `scan` subcommand. Enables cross-distro conversion (e.g., `--base-image registry.redhat.io/rhel9/rhel-bootc:9.6` on a CentOS Stream 9 host).

### 2. Baseline Extraction

New `baseline` module in `inspectah-collect`. Uses the existing `Executor` trait for host command execution via `nsenter -t 1 -m -u -i -n --` (canonical pattern for privileged container host access).

**Extraction sequence:**

1. **Pull image:** `nsenter ... podman pull <image_ref>`. Fail fast on auth/network/invalid ref errors.
2. **Start container:** `nsenter ... podman run -d --name inspectah-baseline-<timestamp> <image_ref> sleep infinity`
3. **Extract packages:** `nsenter ... podman exec <container> rpm -qa --queryformat '%{NAME}.%{ARCH}\n'`. Parse into `HashSet<String>`.
4. **Cleanup:** `nsenter ... podman rm -f <container>`. Always runs (drop guard).

**Return type:**

```rust
struct BaselineData {
    resolution: BaseImageResolution,
    packages: HashSet<String>,  // "name.arch" keyed
    extracted_at: DateTime<Utc>,
}
```

**Container lifecycle:** Ephemeral — created, queried, destroyed in one function call. Container name uses a timestamp suffix (`inspectah-baseline-1716000000`) so stale containers from interrupted runs are identifiable. No long-lived container.

**nsenter rationale:** `systemd-run` was evaluated and rejected. It requires the host's D-Bus socket bind-mounted into the container plus D-Bus auth — trading one privilege requirement for two, with no security benefit. `nsenter -t 1` is the canonical pattern used by toolbox, cri-o debugging containers, and Red Hat's own privileged container tools. It requires only the capabilities inspectah already has for host inspection.

**Snapshot persistence:** `BaselineData` is serialized into the snapshot JSON so refine sessions and re-renders have baseline context without re-pulling. Schema fields: `base_image: Option<String>`, `baseline_package_names: Option<Vec<String>>`, `no_baseline: bool`.

### 3. Pipeline Integration

The pipeline order inverts the Go design. Go scans the host first, then pulls the image during a separate "preflight" stage. Rust resolves and pulls the image **before** host scanning — fail fast on auth/network failures before spending time on a full host scan.

**Pipeline flow:**

```
resolve image → pull + extract baseline → scan host → redact → normalize → render
      ↑ fail fast here                       ↑ baseline available for classification
```

**Data flow:**

1. CLI parses `--base-image` and `--no-baseline` flags
2. If not `--no-baseline`: resolve base image (core), then extract baseline (collect)
3. `BaselineData` passed into `collect_snapshot()` — available to RPM inspector
4. `BaselineData` passed into `normalize()` — drives accurate package classification
5. `BaselineData` passed into `render()` — dynamic FROM line, verification banner data

**Crate placement:**

| Crate | New additions |
|-------|--------------|
| `inspectah-core` | `baseline` module: `BaseImageResolution`, `ResolutionStrategy`, `resolve_base_image()`, `BaselineData` type, incompatible services constant |
| `inspectah-collect` | `baseline` module: `extract_baseline()` (nsenter + podman orchestration) |
| `inspectah-pipeline` | Normalize: baseline-aware classification. Render: dynamic FROM line, `BaselineSummary` in `ViewResponse` |
| `inspectah-cli` | `--base-image` and `--no-baseline` flags, progress output |
| `inspectah-web` | Verification banner component |

### 4. Package Classification (Revised)

Phase 5's three-tier attention model is upgraded with accurate baseline data. The key change: `PackageUserAdded` (recognized repo, not in baseline) becomes **Tier 1 auto-include** instead of Tier 2 NeedsReview.

**Rationale:** In single-machine refine, every user-installed package has 100% prevalence. The signal stack — recognized repo + user-installed + full prevalence — all points to intent. Forcing review on these creates alert fatigue that degrades attention on the cards that actually matter (no repo source, uncertain provenance).

**Revised classification matrix:**

| Tier | Attention reason | Treatment | UI |
|------|-----------------|-----------|-----|
| 1 (Routine) | `PackageBaselineMatch` | Auto-include | Collapsed: "N packages in base image" |
| 1 (Routine) | `PackageUserAdded` | Auto-include | Collapsed: "N user-installed packages (auto-included)" |
| 2 (NeedsReview) | `PackageProvenanceUnavailable` | Triage card | Standard NeedsReview card with "provenance unavailable" badge |
| 3 (Critical) | `PackageNoRepoSource` | Flagged card | Critical card with "no repository source" badge |

**Tier 1 sections:** Baseline packages and user-installed packages are shown in separate collapsed sections. Both are expandable with individual exclude toggles. The user-installed section gives visibility into what was auto-included without demanding attention.

**Fleet mode note (out of Phase 6 scope):** When prevalence drops below 100% in fleet mode, `PackageUserAdded` should revert to Tier 2. The tier assignment becomes prevalence-gated. This is a fleet-phase concern, not Phase 6.

### 5. Incompatible Service Flagging

Static list of services that are architecturally incompatible with image mode — package-manager-based services that cannot function with an immutable `/usr`.

**List:**

- `dnf-makecache.service`
- `dnf-makecache.timer`
- `packagekit.service`
- `packagekit-offline-update.service`

**Implementation:** Constant array in `inspectah-core`. Normalize stage checks service state changes against this list. Matched services get `AttentionReason::ServiceImageModeIncompatible` and are auto-excluded from the Containerfile's `systemctl enable` block.

**UI treatment:** Shown in the Services section with an explanatory badge: "incompatible with image mode." Not a triage card — the decision is made for the user (these services are objectively wrong in image mode).

**Versioning:** The list covers RHEL 9.x, RHEL 10.x, CentOS Stream 9, and Fedora bootc images. If the list ever needs to diverge by release stream, the constant can become a match on the distro. For now, a single static list is sufficient.

### 6. Scan Progress Feedback

Simple stderr output at pipeline stage boundaries. No progress bar library, no progress trait — just clear status messages.

**Output format:**

```
Resolving target image... registry.redhat.io/rhel9/rhel-bootc:9.6 (os-release)
Pulling target image... done (12.3s)
Extracting baseline... 682 packages
Scanning host...
  [1/11] RPM packages
  [2/11] Services
  [3/11] Storage
  ...
  [11/11] Non-RPM files
Redacting sensitive data... done
Generating artifacts... done
```

Each pipeline stage writes to stderr before and after execution. The image pull line updates in-place or appends "done" with elapsed time. Inspector progress uses `[N/11]` counting — each inspector already returns results, so wiring up the counter is straightforward.

The web UI does not receive real-time progress. The scan completes before the triage interface opens. The verification banner communicates the result.

### 7. Verification Banner

New `BaselineSummary` in `ViewResponse` drives the web UI banner.

**Data model:**

```rust
struct BaselineSummary {
    image_ref: String,          // e.g., "registry.redhat.io/rhel9/rhel-bootc:9.6"
    strategy: String,           // e.g., "os-release", "cli-override"
    baseline_count: usize,      // packages matched to baseline
    user_added_count: usize,    // user-installed, auto-included
    review_count: usize,        // packages requiring review
}
```

**Banner text (baseline present):**

> Baseline verified against registry.redhat.io/rhel9/rhel-bootc:9.6 — 682 packages in base image, 47 user-installed, 3 require review

**Banner text (degraded mode):**

> Baseline unavailable — all added packages shown as NeedsReview

The banner appears in the Packages section header, same location as Phase 5's degraded banner. Operators who learned to check that spot will notice the upgrade.

### 8. Fallback Behavior

Failed image pull **aborts the pipeline by default**. No silent degradation.

`--no-baseline` explicitly opts into degraded mode: skip image resolution and pull, use Phase 5's approximated classification.

| Mode | Image pull | Classification | Banner |
|------|-----------|---------------|--------|
| Normal (default) | Required, fail-fast | Accurate baseline | "Baseline verified against [image:tag]" |
| `--no-baseline` | Skipped | Approximated (ProvenanceUnavailable) | "Baseline unavailable" |

**Error messages for common failures:**

- **Auth:** "Authentication failed for registry.redhat.io — run `podman login registry.redhat.io` on the host first"
- **Network:** "Cannot reach registry.redhat.io — check network connectivity or use --no-baseline to skip"
- **Unknown distro:** "No bootc base image found for this system (ID=ubuntu). Use --base-image to specify manually or --no-baseline to skip"

Each error suggests the fix and always mentions `--no-baseline` as the escape hatch.

## Testing Strategy

### Unit tests (inspectah-core)

- Resolution chain: verify priority order, each strategy in isolation, fallback to None
- Distro mapping: all supported distros + edge cases (unknown ID, missing VERSION_ID)
- Classification: baseline match, user-added auto-include, provenance unavailable, no repo source
- Incompatible service list: matching, non-matching, case handling

### Unit tests (inspectah-collect)

- Baseline extraction: mock executor returning rpm -qa output, parse into HashSet
- Container lifecycle: verify cleanup runs on success and failure paths
- Pull failure modes: auth error, network error, invalid ref — verify abort behavior

### Unit tests (inspectah-pipeline)

- Normalize with baseline: verify Tier 1/2/3 classification with real baseline data
- Normalize without baseline: verify degraded path matches Phase 5 behavior
- Containerfile FROM line: dynamic from BaselineData, not hardcoded
- Service flagging: incompatible services auto-excluded, badge text correct
- ViewResponse: BaselineSummary populated correctly

### Integration tests

- End-to-end with mock executor: full pipeline from resolution through render
- Snapshot round-trip: BaselineData serializes/deserializes correctly
- Degraded mode: --no-baseline produces Phase 5-equivalent output

### Manual validation

- Run on CentOS Stream 9 VM with real image pull
- Verify auto-detection resolves to correct CentOS bootc image
- Verify --base-image override produces correct cross-distro Containerfile
- Verify auth failure produces clear error message
