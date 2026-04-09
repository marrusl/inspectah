# ostree/bootc Source System Scanning

**Status:** Proposed (revised after Seal + Collins review)
**Date:** 2026-04-09
**Related:** Kit's code audit (`marks-inbox/reviews/2026-04-08-yoinkc-code-audit-ostree-assumptions.md`), Seal's filesystem analysis (`marks-inbox/reviews/2026-04-08-yoinkc-ostree-filesystem-analysis.md`), Plum's desktop base image research (`marks-inbox/research/2026-04-09-bootc-desktop-base-flatpak-research.md`), Seal review (`marks-inbox/reviews/2026-04-09-seal-review-ostree-bootc-source-scanning-design.md`), Collins review (`marks-inbox/reviews/2026-04-09-collins-review-ostree-bootc-source-scanning-design.md`)

## Design Principle

bootc is always the target. This spec enables users to capture customizations from rpm-ostree or bootc source systems and produce a Containerfile for a pipelined bootc build. rpm-ostree is a source system type, not a destination — it is in maintenance mode.

## Problem

yoinkc assumes traditional package-mode RHEL as the source system. Running on rpm-ostree systems (Silverblue, Kinoite, Universal Blue) or bootc systems produces wrong results (9 issues) and noise (6 issues) across inspectors. The primary user story: a desktop Linux user who's been managing their system manually wants to capture their customizations into a Containerfile for a pipelined bootc build via GitHub Actions.

## Scope

This spec covers detection and adaptation for both rpm-ostree and bootc source systems. The implementation is organized into two slices — Slice 1 fixes inspectors that produce wrong results, Slice 2 fixes noise and adds Flatpak support. Both slices ship; the sequencing is implementation order, not deferral.

## System Type Detection

On startup, yoinkc detects the source system type:

1. Check for `/ostree` directory — if absent, proceed as package-mode (existing behavior unchanged).
2. If present, check `bootc status` exit code — if it succeeds, classify as **bootc system**.
3. If `bootc status` fails but `rpm-ostree status` succeeds, classify as **rpm-ostree system**.
4. If `/ostree` exists but both `bootc status` and `rpm-ostree status` fail, **hard error:**
   ```
   Error: Detected ostree system (/ostree exists) but could not determine
   system type — both 'bootc status' and 'rpm-ostree status' failed.

   This system may use an ostree configuration yoinkc does not yet support.
   ```
   Do not fall back to package-mode behavior — an ostree system misidentified as package-mode would produce silently wrong results.
5. Print a gate message: `Detected bootc system, adapting inspection` or `Detected rpm-ostree system, adapting inspection`.

The system type is stored on the snapshot and flows through to inspectors, renderers, and the Containerfile output. Each inspector can branch on system type where behavior differs.

## Base Image Mapping

When scanning an rpm-ostree/bootc system, yoinkc determines the target base image for baseline subtraction (separating user customizations from base image content).

### Auto-mapping for known systems

| Source system | Detection signal | Target base image |
|---------------|-----------------|-------------------|
| Fedora Silverblue | `VARIANT_ID=silverblue` in `/etc/os-release` | `quay.io/fedora-ostree-desktops/silverblue:NN` |
| Fedora Kinoite | `VARIANT_ID=kinoite` in `/etc/os-release` | `quay.io/fedora-ostree-desktops/kinoite:NN` |
| Universal Blue | `/usr/share/ublue-os/image-info.json` exists | Parse `image-info.json` for upstream image ref. Validate that the file contains at minimum `image-name` and `image-vendor` fields; if malformed, treat as unknown system and require `--base-image`. |
| Fedora bootc (minimal) | `bootc status` shows `fedora-bootc` ref | `quay.io/fedora/fedora-bootc:NN` |
| CentOS bootc | `bootc status` shows `centos-bootc` ref | `quay.io/centos-bootc/centos-bootc:streamNN` |
| RHEL bootc | `bootc status` shows RHEL bootc ref | Match the same ref |

`NN` derived from `VERSION_ID` in `/etc/os-release`. Derivation rules:
- Fedora: `VERSION_ID` is a plain integer (e.g., `41`) — use directly as tag.
- CentOS Stream: `VERSION_ID` is a plain integer (e.g., `10`) — prefix with `stream` (e.g., `stream10`).
- RHEL: `VERSION_ID` is `major.minor` (e.g., `9.4`) — use the major version only for tag matching (e.g., `9`).

For RHEL bootc, the source ref from `bootc status` may contain private/authed registry paths that won't work in CI. yoinkc normalizes RHEL refs to their public equivalent (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.4`) in the generated Containerfile. If normalization isn't possible, emit the ref as-is with a comment: `# NOTE: This image ref may require authentication in CI.`

### Unknown systems

When yoinkc cannot map the source to a known base image, refuse with helpful guidance:

```
Detected rpm-ostree system: <image-ref>
Could not map to a known bootc base image.

Specify one with: yoinkc --base-image <registry/image:tag>

Common bases:
  quay.io/fedora-ostree-desktops/silverblue:41
  quay.io/fedora-ostree-desktops/kinoite:41
  quay.io/fedora/fedora-bootc:41
  quay.io/centos-bootc/centos-bootc:stream10
```

Rationale: a wrong base image produces a diff that looks plausible but is subtly wrong — extra packages, missing customizations, incorrect config deltas. Silent wrong output is the worst failure mode for a migration tool. Starting strict and loosening later is better than starting loose and discovering users built broken pipelines. (Ember + Fern aligned on this.)

### `--base-image` flag

Always available as an override, even for known systems. User-provided value takes precedence over auto-detection.

### bootc label for ostree-desktops bases

The `fedora-ostree-desktops` images are ostree-native containers that work as bootc `FROM` bases in practice (Universal Blue uses them this way), but may not carry bootc labels. When the detected or specified base is an ostree-desktops image, yoinkc emits `LABEL containers.bootc 1` in the generated Containerfile to ensure bootc compatibility.

**Caveat:** This is community-proven but not officially documented by Fedora as a supported path. Verify at implementation time that current image versions work correctly as bootc build bases. If they don't, the fallback is `fedora-bootc:NN` + desktop package layering.

## Package Detection

### rpm-ostree systems

- **Layered packages:** `rpm-ostree status --json` — explicitly listed, high-confidence signal. Emitted as `RUN dnf install` lines in the Containerfile.
- **Overridden packages:** `rpm-ostree status --json` — shows `rpm-ostree override replace` operations. Emitted as comments or `RUN rpm-ostree override` if targeting an ostree base.
- **Removed packages:** `rpm-ostree status --json` — shows `rpm-ostree override remove`. Emitted as `RUN dnf remove` or `rpm-ostree override remove`.
- **Base image packages:** Not emitted — already in the `FROM` image. `rpm -qa` available for reference but does not drive Containerfile output.

### bootc systems

- `bootc status --json` for image ref and metadata.
- If `rpm-ostree status` is available (typical on current bootc systems): use it for layered/overridden/removed packages, same as rpm-ostree systems.
- If `rpm-ostree status` is not available (pure bootc without rpm-ostree): fall back to diffing `rpm -qa` against the target base image's package list. **This path is explicitly low-confidence.** Tag drift, NVR skew, and arch differences between the running system and the resolved base image can produce noisy diffs. yoinkc must:
  - Resolve the base image ref to a specific digest and surface both the ref and digest in the output header.
  - Print a warning: `Package diff is approximate — base image was resolved to <digest> at scan time. Results may differ if the base image tag has moved.`
  - Note in `secrets-review.md` / report output that package detection used the low-confidence fallback path.

### Skip `rpm -Va` on ostree/bootc systems

`rpm -Va` on immutable `/usr` produces overwhelming false positives with no useful signal. Config drift is handled by `/usr/etc` diffing (see next section). Do not run `rpm -Va` on any ostree or bootc source system.

## Config Drift Detection

Three-tier approach for ostree/bootc systems. Replaces the current RPM-based config diffing which does not work correctly on immutable filesystems.

### Tier 1: `/usr/etc` → `/etc` diff (primary)

Walk `/usr/etc` and `/etc` in parallel. Any file that exists in both but differs in content, permissions, ownership, or SELinux context is a user customization.

This is the native ostree mechanism — fast, no RPM queries, no false positives from immutable `/usr`. Diffs are captured the same way yoinkc currently captures config diffs on package-mode systems: content goes into the snapshot, the delta goes into the Containerfile's config tree.

Metadata comparison (permissions, ownership, SELinux contexts) via `stat` and `ls -Z` between `/usr/etc` and `/etc` entries. No RPM verification needed for files present in both trees.

**Handling edge cases:**
- **Volatile files:** Skip known volatile files that change on every boot and are not user customizations (e.g., `/etc/resolv.conf`, `/etc/hostname`, `/etc/machine-id`). Maintain a volatile file list, configurable at implementation time.
- **Files only in `/usr/etc`:** A file present in `/usr/etc` but absent in `/etc` means the user has not modified it — ostree's 3-way merge has not created a mutable copy. Do not report these as missing customizations. This is normal ostree behavior.
- **Symlinks:** Compare symlink targets, not symlink content. A symlink in `/etc` pointing to a different target than `/usr/etc` is a user customization.

### Tier 2: `/etc`-only files (fallback)

Files in `/etc` with no `/usr/etc` counterpart could be user-created or from a rare RPM `%post` script.

Run targeted `rpm -V` against these specific files only (not `rpm -Va` full scan) to determine ownership:
- Files owned by no RPM are definitively user-created — include them.
- Files owned by an RPM but not shipped to `/usr/etc` (rare `%post` case) — include the diff.

### Tier 3: `/var` and `/usr/local` sweep

ostree does not track `/var` at all — it is fully mutable state. `/usr/local` is similarly outside ostree's purview. `rpm -Va` does not help here either since RPMs do not install to these paths.

yoinkc's existing non-RPM software inspector scans these areas. On ostree/bootc systems, it needs filtering to suppress ostree-internal paths (see Inspector Adaptations).

## Flatpak Support

Flatpaks cannot be installed at Containerfile build time — they require a running system bus. This is not an ostree-specific limitation; it applies universally. Flatpak detection runs on **all system types** (package-mode, rpm-ostree, bootc) whenever `flatpak` is present on the system.

### Detection

- Check `which flatpak` — if not present, skip silently.
- List installed Flatpaks via `flatpak list --app --columns=application,origin,branch`.
- Captured in the containers inspector alongside Podman containers and Quadlets.
- Togglable on/off in output.

### Output artifact

`flatpaks.list` — one app ID per line, annotated with remote name:

```
# Flatpak applications detected by yoinkc
# Install with: xargs flatpak install < flatpaks.list
# Or wire into your preferred first-boot mechanism (systemd unit, Brewfile, etc.)
flathub org.mozilla.firefox
flathub org.gnome.Calculator
fedora org.fedoraproject.MediaWriter
```

yoinkc captures what is installed. The installation mechanism (systemd oneshot unit, Universal Blue Brewfile, Ansible playbook, manual install) is the user's choice. yoinkc provides the data, not the plumbing.

### Not included

- Flatpak runtime dependencies — Flatpak resolves these automatically at install time.
- Flatpak user data from `~/.var/app/` — user-space, not system-level.
- No attempt to match any ecosystem-specific manifest format by default.

## Inspector Adaptations

Two slices, both ship. Slice 1 fixes wrong results, Slice 2 fixes noise and adds Flatpak.

### Slice 1 — Wrong results (must-have)

| Inspector | Problem on ostree/bootc | Fix |
|-----------|------------------------|-----|
| **RPM** | `rpm -Va` floods false positives on immutable `/usr`; layered/removed packages invisible to current detection | Skip `rpm -Va` entirely. Use `rpm-ostree status --json` for layered, overridden, and removed packages. |
| **Config** | Downloads RPMs to reconstruct vendor defaults; `/usr/etc` vs `/etc` path mismatch makes nearly all configs appear unowned | Use `/usr/etc` → `/etc` diff as primary mechanism. Targeted `rpm -V` fallback for `/etc`-only files. Skip RPM download path. |
| **Non-RPM software** | Reports `/usr/local` and `/usr/lib/python3` immutable content as operator-installed software | Skip immutable `/usr` paths on ostree systems. Only scan mutable areas (`/var`, `/opt`, user-created paths in `/etc`). |

### Slice 2 — Noise reduction + Flatpak

| Inspector | Problem / Gap | Fix | Scope |
|-----------|--------------|-----|-------|
| **Containers** | Flatpak inventory missing | Add Flatpak detection, output `flatpaks.list` | **All system types** |
| **Storage** | Reports ostree internal mounts (`/sysroot`, `/ostree`) as custom storage | Filter ostree-managed mounts from storage inventory | ostree/bootc only |
| **Kernel/boot** | GRUB config detection produces noise on bootc systems with BLS entries | Filter bootc/ostree-managed boot entries, only surface user-added kernel arguments | ostree/bootc only |
| **Services / Scheduled tasks** | Vendor timer filtering incomplete for ostree-shipped timers | Extend vendor timer filter to use `/usr/lib/systemd/` as the vendor baseline on ostree systems | ostree/bootc only |
| **Users/groups** | Works correctly | No changes needed | — |
| **SELinux** | Works correctly | No changes needed | — |
| **Network** | Works correctly | No changes needed | — |

## Testing

### System detection tests
- Package-mode system (no `/ostree`): existing behavior unchanged
- rpm-ostree system (`/ostree` present, `rpm-ostree status` succeeds, `bootc status` fails): classified correctly, gate message printed
- bootc system (`/ostree` present, `bootc status` succeeds): classified correctly, gate message printed
- Unknown ostree (`/ostree` present, both commands fail): hard error, does not fall back to package-mode
- System type stored on snapshot and accessible to all inspectors

### Base image mapping tests
- Silverblue detected: maps to `fedora-ostree-desktops/silverblue:NN`
- Kinoite detected: maps to `fedora-ostree-desktops/kinoite:NN`
- Universal Blue detected: parses `image-info.json` for upstream ref
- Unknown system: refuses with error message showing common bases
- `--base-image` override: takes precedence over auto-detection
- bootc label emitted when base is ostree-desktops image

### Package detection tests
- Layered packages from `rpm-ostree status` appear as `RUN dnf install`
- Overridden packages from `rpm-ostree status` appear in output
- Removed packages from `rpm-ostree status` appear as `RUN dnf remove`
- `rpm -Va` is NOT called on ostree/bootc systems
- Base image packages are not emitted in Containerfile

### Config drift tests
- File modified in `/etc` vs `/usr/etc`: detected as user customization
- File only in `/etc` (no `/usr/etc` counterpart): classified via targeted `rpm -V`
- File only in `/usr/etc` (no `/etc` copy): not reported (normal ostree behavior)
- Volatile files (`/etc/resolv.conf`, `/etc/hostname`, `/etc/machine-id`): skipped
- Permissions/ownership/SELinux changes detected via metadata comparison
- Symlink target changes detected
- `/var/lib/ostree`, `/var/lib/rpm-ostree`, `/var/lib/flatpak` filtered from non-RPM sweep
- `/usr/local` immutable content not reported as operator-installed

### Flatpak tests
- Flatpak present: `flatpaks.list` generated with correct app IDs and remotes
- Flatpak not present: silently skipped, no error
- Works on package-mode systems (not just ostree)
- Togglable on/off in output

### Inspector adaptation tests (Slice 1)
- RPM inspector on ostree: no `rpm -Va`, uses `rpm-ostree status`
- Config inspector on ostree: uses `/usr/etc` diffing, no RPM downloads
- Non-RPM inspector on ostree: skips immutable `/usr` paths

### Inspector adaptation tests (Slice 2)
- Storage inspector: ostree mounts filtered
- Kernel/boot inspector: BLS entries filtered, user kernel args preserved
- Service/timer inspector: vendor timers from `/usr/lib/systemd/` filtered

### Fixture requirements
Slice 2 boot/timer filtering tests must use real captured fixtures from bootc and Silverblue/Kinoite systems to avoid layout drift. Synthetic test data is acceptable for Slice 1 and system detection tests, but Slice 2 tests should validate against actual system output.

## Out of Scope

- **Containerfile build verification** (does the generated Containerfile actually build and boot?) — valuable but a separate capability
- **User home directory scanning** — `~/.config`, `~/.local`, Flatpak user data
- **Multi-user system support** — yoinkc captures system-level state, not per-user customizations
- **ostree-desktops → fedora-bootc migration path** — if ostree-desktops images stop working as bootc bases, a separate spec will address the `fedora-bootc:NN` + desktop package layering approach
- **Brewfile or other ecosystem-specific Flatpak manifest formats** — add if there's demand via a `--flatpak-format` flag
