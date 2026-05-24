# Baseline Config File Dedup

Pre-spec for brainstorm review. Captures analysis from Collins consult session.

## Problem

When inspectah scans a package-mode host for migration to image-mode, it discovers config files in `/etc` that aren't RPM-owned and classifies them as `ConfigFileKind::Unowned`. Many of these are actually untouched defaults from the target base image — they shipped with the bootc image and were never modified by the user. This creates noise in both the single-host report and fleet aggregate views, inflating the "work to migrate" signal with files that require zero action.

The existing `/usr/etc` diff handles the bootc-to-bootc case (where the host already runs image-mode and ostree provides a 3-way merge). This feature covers the **package-mode-to-image-mode gap**, where no ostree metadata exists and the only way to identify base-image defaults is to compare against the target image directly.

## Proposed Solution

Extend baseline extraction to capture a config file inventory (path + hash) from the base image. At scan time, compare host `Unowned` files against this inventory. Matches get reclassified as `ConfigFileKind::BaselineMatch`.

### Type Model

New struct in `inspectah-core/src/baseline.rs`:

```rust
/// A config file present in the base image's /etc tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineConfigEntry {
    pub path: String,       // e.g., "/etc/nsswitch.conf"
    pub sha256: String,     // hex digest from base image
    pub size: u64,          // cheap pre-filter before hashing
}
```

Added to the existing `BaselineData` struct:

```rust
pub struct BaselineData {
    pub image_digest: String,
    pub packages: HashMap<String, BaselinePackageEntry>,
    pub extracted_at: String,
    #[serde(default)]               // backward-compatible: old snapshots deserialize as empty
    pub config_inventory: Vec<BaselineConfigEntry>,
}
```

`#[serde(default)]` makes this a non-breaking schema change. Existing serialized `BaselineData` without the field will deserialize with an empty `Vec`.

### Collection Phase

Piggybacks on the existing `extract_baseline()` in `inspectah-collect/src/baseline.rs`. The function already runs a podman container (pull, create, start, exec `rpm -qa`, cleanup). The change adds one more `podman exec` step between the RPM query (step 4) and cleanup (step 5):

```
podman exec <container> find /etc -type f -exec sha256sum {} \;
```

Parse the output into `Vec<BaselineConfigEntry>`, extracting path, hash, and size (size can come from a parallel `stat` call or a combined `find -printf` if we want to avoid the second exec).

This is a single additional exec against an already-running container. No extra image pull, no extra container lifecycle.

### Scan-Time Logic

In the config inspector (likely `inspectah-collect/src/inspectors/config/mod.rs`), when processing an `Unowned` file:

1. Check if `baseline_data.config_inventory` has an entry for this path
2. If no path match: stays `Unowned` (file doesn't exist in base image at all)
3. If path matches: compare `size` first (cheap pre-filter to skip hashing when sizes differ)
4. If sizes match: compute `sha256` of the host file, compare against `BaselineConfigEntry.sha256`
5. Hash matches: reclassify as `ConfigFileKind::BaselineMatch`
6. Hash differs: stays `Unowned` (user modified this file, or it was overwritten by a package scriptlet)

The `ConfigFileKind::BaselineMatch` variant already exists in the enum at `inspectah-core/src/types/config.rs`. No new variants needed.

### Impact on Existing Features

- **Containerfile render:** `BaselineMatch` files should be excluded from the generated Containerfile. They're base-image defaults that will already be present in the target image. (Open question: should they appear in a separate "inherited from base" comment section for visibility?)
- **Fleet aggregation:** Multiple hosts with the same `BaselineMatch` file should collapse in fleet views rather than appearing as N separate entries. The fleet engine already groups by path — `BaselineMatch` files just need to be filterable/excludable.
- **`classify_config_path`** (`inspectah-collect/src/inspectors/config/classify.rs`): No changes. Classification (category assignment: `Tmpfiles`, `Environment`, `Audit`, etc.) is orthogonal to kind assignment (`Unowned` vs `BaselineMatch`). Kind is set before or after classification.

## Alternatives Considered

| Approach | Why rejected |
|---|---|
| Static list of known default files | Rots on every RHEL minor release. Unmaintainable. |
| RPM-owned inference only | Misses non-RPM files added via Containerfile `COPY`/`RUN` or package scriptlets |
| File metadata comparison (mtime, ownership) | Unreliable — ostree merge and package installs clobber these |
| Store full file contents in baseline | Unnecessary overhead. Hashes are sufficient for equality checks and much cheaper to store/transmit |

## Open Questions

1. **RPM-owned config hashing.** Should we also hash RPM-owned config files against the baseline to detect "modified but matches base image" cases? This would let us distinguish "user changed this config file but it happens to match the default" from "user intentionally set this." Likely low-value — RPM already tracks modifications via `rpm -Va`.

2. **Fleet collapse behavior.** If 3 hosts all have the same `Unowned` file that matches the baseline, fleet should collapse those rather than showing 3 separate "base image default" entries. Does the existing fleet grouping-by-path handle this naturally, or does the `BaselineMatch` reclassification need explicit fleet-level support?

3. **Containerfile rendering.** Should `BaselineMatch` files be excluded entirely from the Containerfile, or shown in a commented "inherited from base image" section? Excluding is cleaner; commenting preserves the audit trail.

4. **ostree 3-way merge edge cases.** Collins question: are there cases where a file exists on host, exists in the base image with the same hash, but was actually user-intentional? Example: user deleted a config, then manually copied the default back. This would be a false positive (we'd call it `BaselineMatch` when the user did touch it). Likely acceptable — the outcome is the same (file matches base image, no migration action needed).

5. **Typical /etc file count.** What's the realistic file count in a RHEL base image's `/etc`? Estimated 200-400 files. This determines whether the `find | sha256sum` is effectively instant or needs progress indication.

6. **Config inventory optionality.** Should `config_inventory` be truly optional (wrapped in `Option<Vec<...>>`) or just default-empty? `#[serde(default)]` with `Vec` means "empty if absent" which is semantically correct — no baseline config data available. An explicit `Option` would let us distinguish "extraction was attempted but found nothing" from "extraction was not performed." Probably overkill for a pre-spec; `#[serde(default)]` is simpler.

## Scope

- ~85 lines of new Rust code across `inspectah-core` and `inspectah-collect`
- 4-5 new test cases (path match/mismatch, hash match/mismatch, empty inventory, backward compat)
- Backward-compatible schema change via `#[serde(default)]`
- No CLI flag changes (baseline extraction already has its own flag; config dedup activates automatically when baseline data includes a config inventory)
