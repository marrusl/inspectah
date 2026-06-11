# Anaconda Gap Classifier

## Problem

When inspectah scans a bare-metal host installed via Anaconda, the OS
installer adds packages beyond what the bootc base container image
contains. Baseline subtraction correctly filters packages that ARE in
the base image (~393 in a typical RHEL 10 scan), but packages Anaconda
installs that aren't in the container base image survive into the delta.
These leak into the generated Containerfile.

Evidence from three recent scan tarballs (web-01, web-02, web-03): 46
anaconda-sourced packages survived baseline subtraction, including
`grub2-efi-aa64-cdboot` (ARM64-specific — breaks x86_64 builds),
`grub2-tools-extra`, `grubby`, `kernel-tools`, 8 font packages, and
assorted bare-metal tools. All have `source_repo: "anaconda"` and
`include: true`.

The root cause is a fundamental mismatch: Anaconda's "minimal server"
install and a bootc base container image are different package sets.
The container base image doesn't include bootloader packages because
bootc manages booting at a different layer. But the bare-metal host has
them because Anaconda installs them for hardware provisioning. Baseline
subtraction catches the overlap — not the gap.

### Impact

- **Arch-specific packages break cross-architecture builds.** A
  Containerfile with `grub2-efi-aa64-cdboot` fails on x86_64.
- **Noisy output erodes trust.** Fonts in a server Containerfile makes
  users question whether inspectah understands their system.
- **Users manually prune packages** that inspectah should have
  classified correctly.

## Design Principle

Anything required to boot on a particular platform is, by definition,
included in the base image. These packages are not part of the
user-defined layers. inspectah output should be arch-agnostic — the
Containerfile delta should work on any architecture the base image
supports.

More broadly: packages installed by Anaconda that the user never
explicitly requested should be classified by intent signal, not treated
as user workload by default.

## Classification Model

### Primary Signal

`source_repo == "anaconda"` identifies packages installed at OS install
time by the Anaconda installer, not explicitly added by the user via
`dnf install`. This signal is reliable across RHEL, CentOS Stream, and
Fedora (same Anaconda installer stack). The value comes from `dnf
history` provenance tracking.

Packages where `source_repo != "anaconda"` are unaffected by this
classifier. The existing Baseline/Site/Investigate classification
continues to apply.

### Three Tiers

#### Tier 1: Hard Exclude (Baseline / platform-plumbing)

Packages matching boot-path prefixes that conflict with bootc's boot
chain management. These are unconditionally excluded and locked — the
user cannot toggle them back on in the refine UI.

```
PLATFORM_PLUMBING_PREFIXES:
  grub2-*
  grubby
  kernel-tools*
  dracut-config-rescue
  mtools
  shim-*
  efibootmgr
  biosdevname
```

Rationale: bootc owns the boot chain via bootupd. Including these
packages in a Containerfile produces an image that may conflict with
bootc's bootloader management on the next update. The arch-specific
variants (grub2-efi-aa64-cdboot, shim-aa64) also break cross-arch
builds.

Implementation: `include: false`, `locked: true`. The `locked` field
and `clamp_locked_items()` enforcement already exist in the codebase.

UI treatment: shown in a collapsed "Platform (not configurable)" group
at the bottom of the packages section, grayed out, with reason text
"Required by boot chain — excluded unconditionally." Visible for
auditability but not interactive.

#### Tier 2: Promote to Site (user-intent detected)

Anaconda packages where the snapshot contains evidence of active user
customization. These are classified as Site and included in the
Containerfile.

**Dual-signal promotion (default path):** Package has an associated
systemd service that is currently enabled AND has user-modified
configuration files. Both signals must be present.

Example: firewalld with custom zones detected → service enabled + config
modified → promoted to Site.

**Config-only promotion (curated list):** Some packages have
user-modified config but no systemd service (or a service that was
default-enabled, making the enabled signal ambiguous). A curated list
of known config-centric packages can promote on config signal alone.

```
CONFIG_ONLY_PROMOTION:
  sudo
  logrotate
  chrony
  sssd
  pam
```

This list is intentionally conservative. Packages not on this list
require both signals for promotion.

Implementation: classified as `Site`, reason
`active-service-with-config`, `include: true`, `locked: false`.

UI treatment: standard Site classification. Subtitle text showing
signal evidence: "service enabled, custom zones detected" or "custom
sudoers configuration detected."

#### Tier 3: Soft Exclude (Baseline / installer-default)

Everything else in the anaconda set that has no user-intent signal.
Fonts, bare-metal hardware tools, initscripts, OS provisioning
artifacts. Excluded by default but toggleable in the refine UI.

Typical packages in this tier: google-noto-* fonts, lshw, lsscsi,
libsysfs, initscripts-*, prefixdevname, rootfiles, dnf-plugins-core,
glibc-langpack-en, parted.

Implementation: classified as `Baseline`, reason `installer-default`,
`include: false`, `locked: false`.

UI treatment: standard Baseline classification with reason text
"Installed by Anaconda, no active customization detected." Toggleable
— user can re-include if they know they need a specific package.

### Classification Flow

For each package in `packages_added` where `source_repo == "anaconda"`:

1. If name matches `PLATFORM_PLUMBING_PREFIXES` → Tier 1 (hard
   exclude, locked)
2. Else if package has enabled service AND modified config → Tier 2
   (promote to Site)
3. Else if package is in `CONFIG_ONLY_PROMOTION` AND has modified
   config → Tier 2 (promote to Site)
4. Else → Tier 3 (soft exclude, toggleable)

## Pipeline Integration

### Where

`crates/refine/src/classify.rs`, inside `classify_packages()`. This
function already handles all package triage (Baseline/Site/Investigate
decisions) and has access to the full `InspectionSnapshot` including
service state and config data.

The anaconda gap classification runs after baseline subtraction (which
already removed packages present in the base image) and after leaf/auto
classification.

### Cross-Referencing Service and Config Data

Build two lookup structures at the start of the anaconda classification
block:

1. A set of package names that own an enabled, non-default-state
   service (from `snap.services` via `owning_package` +
   `current_state`)
2. A set of package names that own modified config files (from
   `snap.config` via RPM ownership)

These are derived from existing snapshot fields — no new data
collection.

### New Type Variants

In `inspectah-core` (where `TriageReason` is defined):

- `TriageReason::PackagePlatformPlumbing` — serde: `"platform-plumbing"`
- `TriageReason::PackageInstallerDefault` — serde: `"installer-default"`
- `TriageReason::PackageActiveServiceWithConfig` — serde:
  `"active-service-with-config"`

All three use kebab-case serde rename strings, consistent with existing
variants.

### Containerfile Renderer

No changes. The renderer already respects `include: true/false` on
each package. The `locked` field is already enforced by
`clamp_locked_items()` on export.

### Refine UI (Web + TUI)

No structural changes to the refine UI. The new classification reasons
surface through existing reason display mechanisms.

- Platform-plumbing packages: shown grayed out in a collapsed group,
  toggle hidden (locked). Reason: "Required by boot chain — excluded
  unconditionally."
- Installer-default packages: shown with standard Baseline styling,
  toggle enabled. Reason: "Installed by Anaconda, no active
  customization detected."
- Promoted packages: shown with standard Site styling. Subtitle text
  showing signal evidence (e.g., "service enabled, custom zones
  detected").

### Audit Report

The HTML and markdown audit reports include an annotation in the
packages section showing what was filtered:

> **Installer defaults excluded (N packages):** [list]. These packages
> were installed by Anaconda but are not part of your workload. Platform
> packages (grub2, shim, efibootmgr) are excluded unconditionally.
> Others can be re-included in the refine UI.

### Snapshot Schema

No schema changes. `source_repo: "anaconda"` is already in the
snapshot. The new classification reasons are refine-layer metadata
stored in `RefinedPackage`, not in the raw `InspectionSnapshot`.

## Scope

### In Scope

- Anaconda gap classification in `classify_packages()`
- Platform plumbing hard-exclude with `locked: true`
- User-intent promotion (dual-signal + config-only curated list)
- Soft-exclude default for remaining anaconda packages
- Audit report annotation
- Refine UI display (grayed-out locked group, signal evidence subtitles)

### Out of Scope

- Fedora-specific validation (follow-on with Fedora tarballs)
- New snapshot schema fields or data collection
- Boot/kernel config merging into Containerfile output
- Cross-distro (openSUSE) support — separate roadmap item
- Enriching quadlet or kernel_boot with package-level dependency data

## Testing

- Unit tests in `classify.rs` for each tier: platform-plumbing match,
  promotion with dual signal, promotion with config-only, soft-exclude
  fallback
- Integration test with a snapshot fixture containing anaconda-sourced
  packages across all three tiers
- Verify `locked: true` enforcement: platform-plumbing packages cannot
  be toggled via the refine API
- Verify Containerfile output excludes platform-plumbing and
  installer-default packages, includes promoted packages
- Snapshot round-trip: new reason variants serialize/deserialize
  correctly

## Known Limitations

- `CONFIG_ONLY_PROMOTION` is a curated list that may need expansion
  over time as new config-centric packages are identified.
- `source_repo == "anaconda"` reliability on kickstart-minimal installs
  has not been validated — the signal should degrade gracefully (fewer
  matches, not wrong matches).
- Fedora validation is deferred. The classification model is
  signal-based and should generalize, but needs confirmation with
  Fedora scan tarballs.
