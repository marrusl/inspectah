# Phase 6: Base Image Selection & Baseline Extraction

**Status:** Proposed (revision 3)
**Author:** Mark Russell (with team input from Collins, Ember, Fern, Tang)
**Date:** 2026-05-17
**Review round 1:** 2026-05-16 — request-changes from Collins, Tang, Thorn, Slate. Revision 2 addressed all four cross-lane blockers.
**Review round 2:** 2026-05-16 — request-changes from Collins, Tang. This revision addresses the three remaining blockers: UBlue metadata path/tag, repository-side digest, and degraded-mode persisted resolution.

## Problem

The Containerfile renderer hardcodes `FROM rhel9/rhel-bootc:9.4` regardless of the source system's distribution or version. Phase 5's three-tier attention model classifies packages using approximated baseline data — the `PackageProvenanceUnavailable` path fires for every package when no real baseline exists. This produces inflated triage workloads (dozens of Tier 2 cards for packages that are obviously baseline) and incorrect Containerfile output.

Phase 6 makes baseline subtraction accurate by pulling the actual target bootc base image, extracting its package list, and using that as the ground truth for classification.

## Scope

### In scope

1. **Base image resolution** — auto-detect the correct bootc base image for the source system, with a `--base-image` CLI override for cross-distro conversion
2. **Ref normalization and validation** — canonical validation gate before any pull; host-derived refs normalized to pullable OCI references
3. **Baseline package extraction** — pull the target image, extract its package list with full NEVRA identity, without executing the image's entrypoint
4. **Accurate package classification** — replace approximated baseline with real package set; exhaustive state matrix across all `PackageState` variants and baseline modes
5. **Incompatible service flagging** — static list of services incompatible with image mode (dnf-makecache, packagekit), with one authoritative post-normalization representation
6. **CLI scan progress feedback** — stage-by-stage stderr output (image pull adds latency)
7. **Verification banner** — web UI banner confirming which image the baseline was verified against, with honest language matched to stored facts

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
| 2 | Universal Blue | `/usr/share/ublue-os/image-info.json` | `image-ref` + `image-tag` combined, or synthesized from vendor/name/tag |
| 3 | bootc status | `bootc status --json` | `status.booted.image.image.image` |
| 4 | Fedora Atomic desktop | `/etc/os-release` VARIANT_ID ∈ known set | `quay.io/fedora-ostree-desktops/{variant}:{VERSION_ID}` |
| 5 | os-release mapping | `/etc/os-release` ID + VERSION_ID | See distro table below |

**Precedence fix (round 1 blocker 1):** Strategy 4 (Fedora Atomic desktop) is checked BEFORE the generic os-release mapping (strategy 5). On Silverblue, `ID=fedora` and `VARIANT_ID=silverblue` are both present. The round 1 spec had os-release (generic) at priority 4 and desktop at priority 5, which meant `ID=fedora` matched first and the desktop-variant table was unreachable. The corrected order ensures Silverblue/Kinoite/Sway Atomic/etc. resolve to their desktop-specific images.

**Universal Blue metadata fix (round 1 blocker 1, round 2 blocker 1):** The correct metadata path is `/usr/share/ublue-os/image-info.json` (matching current inspectah Go resolver/tests and current UBlue build metadata). Current UBlue metadata uses a transport-prefixed, tagless `image-ref` (e.g., `ostree-image-signed:docker://ghcr.io/ublue-os/bazzite`) with the effective tag carried separately in `image-tag` (e.g., `stable`). The UBlue resolution rule:

1. Read `image-ref` and strip known transport prefixes (`ostree-image-signed:docker://`, `docker://`).
2. If the stripped ref has no tag component (no `:`), combine it with the `image-tag` field: `{stripped_ref}:{image_tag}`.
3. If `image-ref` already contains a tag, use it as-is after stripping.
4. If `image-ref` is absent but `image-vendor`, `image-name`, and `image-tag` are all present, synthesize: `ghcr.io/{image-vendor}/{image-name}:{image-tag}`.
5. Pass the result to the canonical normalization gate (section 1a).

If the metadata file exists but is malformed (missing required fields, unparseable JSON), resolution **fails closed** — returns an error rather than silently falling through to strategy 3+. The operator sees: "Universal Blue metadata found but unreadable — use --base-image to override."

**Distro mapping table (strategy 5):**

| ID | Image reference pattern |
|----|------------------------|
| `fedora` | `quay.io/fedora/fedora-bootc:{VERSION_ID}` |
| `centos` | `quay.io/centos-bootc/centos-bootc:stream{MAJOR}` |
| `rhel` | `registry.redhat.io/rhel{MAJOR}/rhel-bootc:{VERSION_ID}` (see RHEL version floor below) |

**Version floors:** Bootc base images are not available for all distro versions. The resolution chain clamps to the minimum supported version:

| Distro | Floor | Behavior |
|--------|-------|----------|
| RHEL 9 | 9.6 | RHEL 9.0–9.5 → target `rhel9/rhel-bootc:9.6` |
| RHEL 10 | 10.0 | No clamping needed (bootc from 10.0) |
| Fedora | 41 | Fedora 40 and below → target `fedora-bootc:41`. No bootc image exists before F41. |
| CentOS Stream | (major only) | Maps by major version (`stream9`, `stream10`), no minor clamping |

Unknown `ID` values produce `None` — the pipeline aborts with a clear error.

**Fedora Atomic desktop variants (strategy 4):**

Current active variants: silverblue, kinoite, sway-atomic, budgie-atomic, cosmic-atomic → `quay.io/fedora-ostree-desktops/{variant}:{VERSION_ID}`

Historical variants retained for older source systems: lxqt-atomic, xfce-atomic → same pattern. These are no longer actively published but may exist on systems that were installed when they were current.

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
    FedoraAtomicDesktop,
    OsRelease,
}
```

Resolution returns `Result<BaseImageResolution, ResolutionError>` — `Err` when resolution fails (unknown distro, malformed metadata). `Ok` feeds into the ref normalization gate (section 1a) before any pull. When resolution fails and `--base-image` was not provided, the pipeline aborts with a clear error suggesting `--base-image` or `--no-baseline`.

**CLI flag:** `--base-image <IMAGE_REF>` on the `scan` subcommand. Enables cross-distro conversion (e.g., `--base-image registry.redhat.io/rhel9/rhel-bootc:9.6` on a CentOS Stream 9 host).

### 1a. Ref Normalization and Validation Gate

Every resolved image ref — regardless of source strategy — passes through a single canonical normalization step before the pipeline is allowed to pull.

**Normalization rules:**

1. Strip known OCI transport prefixes: `ostree-image-signed:docker://`, `docker://`, `containers-storage:`. Produce a bare `registry/repo:tag` or `registry/repo@sha256:digest` reference.
2. Reject refs that are empty, contain whitespace, or contain shell metacharacters.
3. Reject refs without a registry component (bare `image:tag` with no `/`). `podman pull` would resolve these against configured registries, which is ambiguous — require fully qualified refs.
4. If the ref contains a digest (`@sha256:`), preserve it. If it contains only a tag (`:tag`), preserve the tag. If neither, append `:latest` explicitly.

**Validation rules:**

1. The registry component must parse as a valid hostname (or `localhost`).
2. The ref must not point to a `localhost/` or `containers-storage:` local-only image — baseline extraction requires a pullable remote image.

**Output:** `NormalizedImageRef` — a validated, pullable OCI reference. This type is distinct from a raw string to enforce that all downstream consumers (pull, snapshot persistence, banner) use the validated form.

```rust
struct NormalizedImageRef {
    ref_string: String,  // fully qualified, transport-stripped
}
```

**Fail behavior:** Normalization failure aborts the pipeline. Error messages include the raw input and which rule rejected it: "Image ref 'ostree-image-signed:docker://...' could not be normalized — unsupported transport prefix after stripping."

### 2. Baseline Extraction

New `baseline` module in `inspectah-collect`. Uses the existing `Executor` trait for host command execution via `nsenter -t 1 -m -u -i -n --` (canonical pattern for privileged container host access).

**Extraction sequence (round 1 blocker 4 — safer extraction):**

1. **Pull image:** `nsenter ... podman pull <normalized_ref>`. Fail fast on auth/network/invalid ref errors.
2. **Create container without starting:** `nsenter ... podman create --name inspectah-baseline-<timestamp> --entrypoint '["sleep", "infinity"]' --network none <normalized_ref>`. This overrides the image's `ENTRYPOINT`/`CMD` so no image-supplied code executes. `--network none` prevents network access during the query phase.
3. **Start container:** `nsenter ... podman start <container>`.
4. **Extract packages:** `nsenter ... podman exec <container> rpm -qa --queryformat '%{NAME}\t%{EPOCH}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n'`. Parse into a `HashMap<String, BaselinePackageEntry>` keyed by `name.arch`.
5. **Capture image identity:** `nsenter ... podman inspect --format '{{.Digest}}' <normalized_ref>`. Records the repository-side manifest digest (e.g., `sha256:abc123...`) — this is the remote object identity, not the local storage ID (`.Id`). If `.Digest` is empty (can happen with locally-built images), fall back to the matching entry in `.RepoDigests` for the normalized ref's registry/repo. The persisted value must answer "which exact remote object was this baseline compared against?"
6. **Cleanup:** `nsenter ... podman rm -f <container>`. Always runs (drop guard on the container name — cleanup executes even if steps 3-5 fail or the process is interrupted).

**Entrypoint override rationale (round 1 blocker 4):** The round 1 spec used `podman run ... sleep infinity`, which still executes the image's `ENTRYPOINT` before `sleep`. `podman create --entrypoint '["sleep", "infinity"]'` replaces the entrypoint entirely, so no image-supplied code runs. Combined with `--network none`, the extraction container cannot execute arbitrary code or make network calls.

**Package identity (round 1 blocker 2):** The round 1 spec extracted only `name.arch`, which is too lossy for version-drift classification. The revised extraction captures full NEVRA (Name, Epoch, Version, Release, Architecture) so the classifier can distinguish "present in image" from "present but different version."

**Return type:**

```rust
struct BaselineData {
    resolution: BaseImageResolution,
    normalized_ref: NormalizedImageRef,
    image_digest: String,                    // sha256 digest of pulled image
    packages: HashMap<String, BaselinePackageEntry>,  // keyed by "name.arch"
    extracted_at: DateTime<Utc>,
}

struct BaselinePackageEntry {
    name: String,
    epoch: Option<String>,
    version: String,
    release: String,
    arch: String,
}
```

**Container lifecycle:** Ephemeral — created, queried, destroyed in one function call. Container name uses a timestamp suffix (`inspectah-baseline-1716000000`) so stale containers from interrupted runs are identifiable. The drop guard ensures `podman rm -f` runs on all exit paths including panics.

**nsenter rationale:** `systemd-run` was evaluated and rejected. It requires the host's D-Bus socket bind-mounted into the container plus D-Bus auth — trading one privilege requirement for two, with no security benefit. `nsenter -t 1` is the canonical pattern used by toolbox, cri-o debugging containers, and Red Hat's own privileged container tools. It requires only the capabilities inspectah already has for host inspection.

**Snapshot persistence (round 1 blocker 2, round 2 blocker 3):** The snapshot schema must persist enough identity to reconstruct the baseline context in later refine sessions without re-pulling. Additionally, the resolved target image must survive independently of baseline extraction so that `--no-baseline` snapshots can still produce correct FROM lines on reopen/export.

**Schema fields — two independent concerns:**

1. **`target_image`** (top-level, independent of baseline): The resolved target image identity. Persisted even in degraded mode so the Containerfile FROM line is correct on reopen/export.

2. **`baseline`** (present only when extraction was performed): Full baseline extraction data.

```json
{
  "target_image": {
    "image_ref": "registry.redhat.io/rhel9/rhel-bootc:9.6",
    "strategy": "os-release"
  },
  "baseline": {
    "image_digest": "sha256:abc123...",
    "packages": {
      "bash.x86_64": { "epoch": "0", "version": "5.2.26", "release": "3.el9", "arch": "x86_64" },
      "...": "..."
    },
    "extracted_at": "2026-05-17T01:00:00Z"
  },
  "no_baseline": false
}
```

**Legal snapshot states (round 1 blocker 2, round 2 blocker 3):**

| `target_image` | `baseline` | `no_baseline` | Meaning | Valid? |
|----------------|-----------|--------------|---------|--------|
| Present | Present | `false` | Full verified baseline — accurate classification, correct FROM | Yes |
| Present | `null` | `true` | Resolution succeeded, extraction skipped (`--no-baseline`) — correct FROM, degraded classification | Yes |
| `null` | `null` | `true` | Resolution failed + `--no-baseline` — FROM omitted with comment, degraded classification | Yes |
| `null` | `null` | `false` | Resolution/extraction failed (pipeline aborted) | No — pipeline never produces this |
| Present | Present | `true` | Contradictory — baseline data with degraded flag | No — rejected at scan time |
| `null` | Present | any | Contradictory — baseline without resolution | No — impossible |

**Migration rule for pre-Phase-6 snapshots:** Snapshots that lack `target_image`, `baseline`, and `no_baseline` fields are treated as: `target_image = null`, `baseline = null`, `no_baseline = true`. This enters degraded mode with FROM omitted. The schema version distinguishes pre-Phase-6 from post-Phase-6 snapshots.

The authoritative baseline identity key across all paths is `name.arch`. NEVRA data is available for version-drift classification but the membership test (in-baseline vs. not-in-baseline) uses `name.arch`.

### 3. Pipeline Integration

The pipeline order inverts the Go design. Go scans the host first, then pulls the image during a separate "preflight" stage. Rust resolves and pulls the image **before** host scanning — fail fast on auth/network failures before spending time on a full host scan.

**Pipeline flow:**

```
resolve image → normalize ref → pull + extract baseline → scan host → redact → normalize → render
      ↑ fail fast here                                       ↑ baseline available for classification
```

**Data flow:**

1. CLI parses `--base-image` and `--no-baseline` flags
2. `--base-image` + `--no-baseline` together is rejected: "Cannot specify both --base-image and --no-baseline. Use --base-image to set the target image, or --no-baseline to skip baseline extraction."
3. If not `--no-baseline`: resolve base image (core) → normalize ref (core) → extract baseline (collect)
4. `BaselineData` passed into `collect_snapshot()` — available to RPM inspector
5. `BaselineData` passed into `normalize()` — drives accurate package classification
6. `BaselineData` passed into `inspectah-refine` — materialized into canonical session state
7. `inspectah-refine` derives `BaselineSummary` for view response — `inspectah-web` serializes it, does not derive it

**Crate placement (round 1 blocker 3 — added inspectah-refine):**

| Crate | New additions |
|-------|--------------|
| `inspectah-core` | `baseline` module: `BaseImageResolution`, `ResolutionStrategy`, `NormalizedImageRef`, `resolve_base_image()`, `normalize_image_ref()`, `BaselineData`, `BaselinePackageEntry`, incompatible services constant, `IncompatibleServiceEntry` |
| `inspectah-collect` | `baseline` module: `extract_baseline()` (nsenter + podman orchestration with entrypoint override) |
| `inspectah-pipeline` | Normalize: baseline-aware classification with exhaustive state matrix. Render: dynamic FROM line from `NormalizedImageRef` |
| `inspectah-refine` | Canonical owner of baseline-derived session state. Materializes baseline into refine session. Derives `BaselineSummary` for view response. Enforces preview/export parity using the same projected snapshot. |
| `inspectah-cli` | `--base-image` and `--no-baseline` flags (mutually exclusive), progress output |
| `inspectah-web` | Serializes `BaselineSummary` from refine into HTTP response. Renders verification banner component. Does not derive baseline data. |

### 4. Package Classification (Exhaustive State Matrix)

Phase 5's three-tier attention model is upgraded with accurate baseline data. The key change: `PackageUserAdded` (recognized repo, not in baseline) becomes **Tier 1 auto-include** instead of Tier 2 NeedsReview.

**Rationale:** In single-machine refine, every user-installed package has 100% prevalence. The signal stack — recognized repo + user-installed + full prevalence — all points to intent. Forcing review on these creates alert fatigue that degrades attention on the cards that actually matter (no repo source, uncertain provenance).

**Wire-level mapping (round 1 blocker 3):** "Tier 1/2/3" are product-facing presentation labels. The wire-level attention contract uses the existing `AttentionLevel` enum: `Routine`, `Informational`, `NeedsReview`. "Tier 3 (Critical)" is NOT a new wire-level state — it is `NeedsReview` with a presentation severity flag. Counts, badges, and completion logic key off `AttentionLevel`, not tier labels.

| Product tier | Wire-level `AttentionLevel` | Completion counts toward |
|-------------|---------------------------|------------------------|
| Tier 1 (Routine) | `Routine` | Not counted (auto-included) |
| Tier 2 (NeedsReview) | `NeedsReview` | Counted — must be explicitly included/excluded |
| Tier 3 (Critical) | `NeedsReview` + `severity: critical` | Counted — same as Tier 2, different badge |

**Exhaustive classification matrix (round 1 blocker 3):**

Covers all `PackageState` variants across both baseline modes.

| PackageState | Repo provenance | Baseline mode: verified | Baseline mode: degraded (`--no-baseline`) |
|-------------|----------------|------------------------|------------------------------------------|
| `Added` | Recognized repo, in baseline | `PackageBaselineMatch` → Routine | `PackageProvenanceUnavailable` → NeedsReview |
| `Added` | Recognized repo, NOT in baseline | `PackageUserAdded` → Routine | `PackageProvenanceUnavailable` → NeedsReview |
| `Added` | No repo source | `PackageNoRepoSource` → NeedsReview (critical) | `PackageNoRepoSource` → NeedsReview (critical) |
| `Modified` | Recognized repo, in baseline | `PackageVersionChanged` → NeedsReview | `PackageProvenanceUnavailable` → NeedsReview |
| `Modified` | Recognized repo, NOT in baseline | `PackageVersionChanged` → NeedsReview | `PackageProvenanceUnavailable` → NeedsReview |
| `Modified` | No repo source | `PackageNoRepoSource` → NeedsReview (critical) | `PackageNoRepoSource` → NeedsReview (critical) |
| `LocalInstall` | (always no repo) | `PackageNoRepoSource` → NeedsReview (critical) | `PackageNoRepoSource` → NeedsReview (critical) |
| `NoRepo` | (always no repo) | `PackageNoRepoSource` → NeedsReview (critical) | `PackageNoRepoSource` → NeedsReview (critical) |
| `BaseImageOnly` | (in baseline only) | Not rendered (base-image-only packages are not user drift) | Not applicable (no baseline data) |

**Key decisions in the matrix:**
- `Modified` packages ALWAYS require review regardless of baseline mode. Version drift is a meaningful signal — the user may or may not want the divergent version.
- `PackageUserAdded` (recognized repo, not in baseline) is Routine in verified mode. This is the core Phase 6 improvement: accurate baseline data lets us distinguish "user intentionally installed httpd from appstream" from "user has a sketchy RPM from nowhere."
- `BaseImageOnly` packages (present in image but not on host) are informational only — they represent packages the base image ships that the user's system doesn't have. Not rendered in the Containerfile.

**Tier 1 sections:** Baseline packages and user-installed packages are shown in separate collapsed sections. Both are expandable with individual exclude toggles. The user-installed section gives visibility into what was auto-included without demanding attention.

**Fleet mode note (out of Phase 6 scope):** When prevalence drops below 100% in fleet mode, `PackageUserAdded` should revert to Tier 2. The tier assignment becomes prevalence-gated. This is a fleet-phase concern, not Phase 6.

### 5. Incompatible Service Flagging

Static list of services that are architecturally incompatible with image mode — package-manager-based services that cannot function with an immutable `/usr`.

**List:**

- `dnf-makecache.service`
- `dnf-makecache.timer`
- `packagekit.service`
- `packagekit-offline-update.service`

**Authoritative representation (round 1 blocker 3):** The incompatible service list is defined as a constant in `inspectah-core`:

```rust
struct IncompatibleServiceEntry {
    unit: &'static str,
    reason: &'static str,  // e.g., "package-manager service incompatible with immutable /usr"
}
```

Normalize stage checks service state changes against this list. Matched services get:
- `include: false` (auto-excluded from Containerfile)
- `attention_reason: ServiceImageModeIncompatible`
- The `reason` field from the entry (for badge/tooltip text)

This is the **single authoritative post-normalization representation**. The UI, Containerfile preview, and tarball export all read from this normalized state. Specifically:
- `enabled_units` in the Containerfile renderer: incompatible services are removed before rendering `systemctl enable`
- `state_changes` in the UI: incompatible services are shown with the badge, not as triage cards
- Export tarball: the projected snapshot reflects the normalized state — incompatible services are excluded

No surface may independently re-derive incompatibility. All read from the normalized `include: false` + `attention_reason` on the service entry.

**Versioning:** The list covers RHEL 9.x, RHEL 10.x, CentOS Stream 9, and Fedora bootc images. If the list ever needs to diverge by release stream, the constant can become a match on the distro. For now, a single static list is sufficient.

### 6. Scan Progress Feedback

Simple stderr output at pipeline stage boundaries. No progress bar library, no progress trait — just clear status messages.

**Output format:**

```
Resolving target image... registry.redhat.io/rhel9/rhel-bootc:9.6 (os-release)
Normalizing image reference... ok
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

`BaselineSummary` is derived by `inspectah-refine` from canonical session state, then serialized by `inspectah-web` into the HTTP response.

**Data model:**

```rust
struct BaselineSummary {
    image_ref: String,          // normalized ref used for pull
    image_digest: String,       // sha256 digest of the pulled image
    strategy: String,           // e.g., "os-release", "cli-override"
    baseline_count: usize,      // packages matched to baseline
    user_added_count: usize,    // user-installed, auto-included
    review_count: usize,        // packages requiring review
}
```

**Banner text (baseline present):**

> Baseline compared against registry.redhat.io/rhel9/rhel-bootc:9.6 (sha256:abc1…) — 682 in base image, 47 user-installed, 3 require review

**Honest language (round 1 blocker 2):** The round 1 banner said "verified against." The revised wording says "compared against" — this is honest because the baseline extraction compares package membership (`name.arch`), not full content verification. The image digest provides exact identity for reproducibility. "Verified" is reserved for when the tool can make a stronger claim (e.g., digest-pinned image with content-addressed package verification).

**Banner text (degraded mode):**

> Baseline unavailable — all added packages shown as NeedsReview

The banner appears in the Packages section header, same location as Phase 5's degraded banner. Operators who learned to check that spot will notice the upgrade.

### 8. Fallback Behavior

**Image resolution is separate from baseline extraction (round 1 blocker 3).** This split matters for degraded mode:

- `--no-baseline`: skips baseline extraction (no image pull, no package comparison), BUT still resolves the target image ref for the dynamic FROM line. The resolved ref is persisted in `target_image` — this survives reopen/export so the Containerfile FROM line is correct even in reopened degraded sessions. This prevents the degraded path from falling back to a hardcoded FROM.
- If resolution itself fails in `--no-baseline` mode (unknown distro, no bootc status): `target_image` is `null`, and the Containerfile FROM line is omitted entirely with a comment: `# FROM line omitted — target image could not be determined. Use --base-image to specify.` This state also persists correctly on reopen/export.

**Flag combinations:**

| `--base-image` | `--no-baseline` | Behavior |
|----------------|----------------|----------|
| Not set | Not set | Auto-detect + extract baseline (default) |
| Set | Not set | Use override + extract baseline from specified image |
| Not set | Set | Auto-detect for FROM line only, skip extraction |
| Set | Set | **Rejected** — contradictory intent |

**Error messages for common failures:**

- **Auth:** "Authentication failed for registry.redhat.io — run `podman login registry.redhat.io` on the host first, or use --no-baseline to skip"
- **Network:** "Cannot reach registry.redhat.io — check network connectivity or use --no-baseline to skip"
- **Unknown distro:** "No bootc base image found for this system (ID=ubuntu). Use --base-image to specify manually or --no-baseline to skip"
- **Malformed UBlue metadata:** "Universal Blue metadata found at /usr/share/ublue-os/image-info.json but could not be parsed — use --base-image to override"

Each error suggests the fix and always mentions `--no-baseline` as the escape hatch.

## Testing Strategy

### Unit tests (inspectah-core)

- **Resolution chain precedence:** Fedora with `VARIANT_ID=silverblue` resolves to desktop image, not generic fedora-bootc. Test all 5 strategies in isolation and in combination.
- **UBlue metadata:** Valid `image-ref` + `image-tag` combination (transport-prefixed tagless ref combined with separate tag), `ostree-image-signed:docker://` prefix stripping, tagged `image-ref` used as-is, synthesis fallback from vendor/name/tag, malformed JSON fails closed, missing file falls through. Path is `/usr/share/ublue-os/image-info.json`.
- **Ref normalization:** Transport prefix stripping, bare refs rejected, shell metacharacters rejected, digest preservation, tag defaulting.
- **Distro mapping:** All supported distros + edge cases (unknown ID, missing VERSION_ID, empty strings).
- **Classification matrix:** Every cell in the exhaustive matrix from section 4 — all `PackageState` variants × repo provenance × baseline mode. This is the contract-proof gate.
- **Incompatible service list:** Matching, non-matching, case sensitivity.

### Unit tests (inspectah-collect)

- **Baseline extraction:** Mock executor returning NEVRA-format rpm -qa output, parse into `HashMap<String, BaselinePackageEntry>`.
- **Entrypoint override:** Verify the `podman create` command includes `--entrypoint` and `--network none`.
- **Command ordering (round 1 blocker 4):** The mock executor must record command sequence, not just return canned output. Assert that the exact order is: pull → create → start → exec(rpm -qa) → exec(podman inspect) → rm. Assert that `rm` runs even when intermediate steps fail.
- **Cleanup on failure:** Simulate failure at each step (pull fails, create fails, start fails, exec fails). Verify `rm -f` is always attempted for the container name.
- **Pull failure modes:** Auth error, network error, invalid ref — verify pipeline abort with correct error message.
- **Mixed-arch baseline (round 1 blocker 4):** When the host is x86_64 and the baseline image is pulled for x86_64, the `name.arch` keys must reflect the image's architecture. Test that an aarch64 baseline image produces `name.aarch64` keys, not the host's arch.

### Unit tests (inspectah-pipeline)

- **Normalize with baseline:** Exhaustive coverage of the section 4 matrix — every `PackageState` × provenance × baseline mode cell produces the correct `AttentionLevel` and `AttentionReason`.
- **Normalize without baseline:** Verify degraded path produces identical output to Phase 5 behavior — no regressions.
- **Containerfile FROM line:** Dynamic from `NormalizedImageRef`, not hardcoded. Degraded mode with resolution uses resolved ref. Degraded mode without resolution omits FROM with comment.
- **Service flagging surface agreement (round 1 blocker 4):** Incompatible services are excluded from `enabled_units` AND marked in `state_changes` AND absent from export. Test all three surfaces against the same normalized input.
- **ViewResponse: `BaselineSummary` counts match classification output.**

### Unit tests (inspectah-refine)

- **Session state materialization:** `BaselineData` from snapshot is materialized into refine session state. Reopened sessions reconstruct `BaselineSummary` from persisted baseline.
- **Preview/export parity (round 1 blocker 4):** The Containerfile preview and the exported tarball Containerfile are derived from the same projected snapshot. Test that both agree on FROM line, package list, and service enablement.
- **Degraded mode replay:** A snapshot with `no_baseline: true` reopens correctly in degraded mode with appropriate banner and classification.
- **Degraded FROM persistence (round 2 blocker 3):** A `--no-baseline` snapshot with resolved `target_image` preserves the correct FROM line after reopen/export. A `--no-baseline` snapshot with `target_image = null` preserves the FROM-omission comment after reopen/export.

### Integration tests

- **End-to-end with mock executor:** Full pipeline from resolution through render, verified and degraded modes.
- **Snapshot round-trip:** `BaselineData` with full NEVRA serializes/deserializes correctly. Reopened session produces identical classification and banner.
- **Schema backward compatibility:** Snapshots from Phase 5 (no baseline fields) deserialize correctly and enter degraded mode automatically. Specifically: missing `target_image`/`baseline`/`no_baseline` fields map to `target_image = null`, `baseline = null`, `no_baseline = true` per the migration rule.

### Manual validation

- Run on CentOS Stream 9 VM with real image pull
- Verify auto-detection resolves to correct CentOS bootc image
- Verify `--base-image` override produces correct cross-distro Containerfile
- Verify auth failure produces clear error message and suggests `podman login`
- Verify `--no-baseline` produces correct FROM line (auto-detected) with degraded banner
- Verify incompatible services are excluded from Containerfile AND flagged in UI AND absent from export tarball
- Verify refine session reopen: close and reopen a refine session, confirm baseline data persists and classification is identical
- Verify degraded-mode reopen: scan with `--no-baseline`, close session, reopen, confirm degraded banner and classification persist
