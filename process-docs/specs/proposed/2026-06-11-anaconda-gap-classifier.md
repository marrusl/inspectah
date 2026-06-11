# Anaconda Gap Classifier

## Revision History

- **R1 (2026-06-11):** Initial draft.
- **R2 (2026-06-11):** Revised per review panel (Collins, Tang, Thorn).
  Narrowed anaconda signal scope with precedence rules. Shrunk Tier 1
  locked set to true bootloader/EFI only. Fixed wire format to
  snake_case. Split Tier 2 into two typed promotion variants. Added
  safe fallback for missing evidence. Scoped out UI grouping and audit
  report annotations (follow-on).
- **R3 (2026-06-11):** Resolved Tier 1 precedence contradiction —
  platform plumbing is checked first, before the precedence gate.
  Split Tier 3 into high-confidence installer noise (soft-exclude) and
  ambiguous anaconda remainder (Investigate). Fixed chrono → chrony.
- **R4 (2026-06-11):** Added group-install detection as a rendering
  concern (not classification). New collection step (`dnf group list
  --installed`), new `installed_groups` snapshot field, and `dnf group
  install` rendering in Containerfile. Group membership affects how
  packages are expressed in the Containerfile, not how they are
  classified.

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
as user workload by default. However, install-time user intent IS still
user intent — packages selected via Anaconda group selection (e.g.,
container-tools) or kickstart `%packages` are deliberate choices and
must not be silently excluded.

## Classification Model

### Primary Signal and Precedence

`source_repo == "anaconda"` identifies packages installed at OS install
time by the Anaconda installer. This signal is reliable across RHEL,
CentOS Stream, and Fedora (same Anaconda installer stack).

**Critical constraint:** `source_repo == "anaconda"` is necessary but
NOT sufficient for reclassification. It gates entry into the anaconda
classifier, but the classifier has two precedence layers:

1. **Tier 1 (platform plumbing) is checked first and always wins.**
   If a package matches `PLATFORM_PLUMBING_PREFIXES`, it is hard-
   excluded regardless of any other signal. These packages conflict
   with bootc's boot chain management and must never appear in a
   container workload layer, even if the user explicitly installed
   them or they have version-change signals.

2. **For all other tiers, stronger existing classifications are
   preserved.** If `classify_packages()` has already assigned a
   reason of `PackageVersionChanged`, `PackageLocalInstall`,
   `PackageNoRepoSource`, `PackageConfigCaptured`, or any other
   investigate-class signal, the anaconda classifier does NOT override
   it. Those signals indicate the package has a more specific story
   than "installer default." The anaconda classifier only reclassifies
   packages that would otherwise receive `PackageUserAdded` or
   `PackageProvenanceUnavailable` — packages where anaconda provenance
   is the most informative signal available.

Packages where `source_repo != "anaconda"` are completely unaffected.

### Four Tiers

#### Tier 1: Hard Exclude (Baseline / platform_plumbing)

Packages that are true bootloader and EFI ownership — they conflict
with bootc's boot chain management via bootupd and must not appear in
a container workload layer.

```
PLATFORM_PLUMBING_PREFIXES:
  grub2-*
  grubby
  shim-*
  efibootmgr
```

This list is intentionally narrow: only packages where bootc owns the
lifecycle and including them would conflict with bootc's bootloader
management or break cross-arch builds. The arch-specific variants
(grub2-efi-aa64-cdboot, shim-aa64) are the primary motivation.

Implementation: `include: false`, `locked: true`.

Locking contract: a `SetInclude(true)` operation on a locked package
is a silent no-op at the session layer. The UI disables the toggle.
`clamp_locked_items()` enforces at export as defense-in-depth. Tests
must assert the no-op behavior at the session and web API boundary,
not just final export state.

#### Tier 2: Promote to Site (user-intent detected)

Anaconda packages where the snapshot contains evidence of active user
customization. These are classified as Site and included in the
Containerfile.

**Two distinct promotion paths with separate reason variants:**

**Path A — Dual-signal promotion:**

Package has an associated systemd service that is user-enabled (not
just default-enabled) AND has user-modified configuration files
(`ConfigFileKind::RpmOwnedModified`). Both signals must be present.

- Service signal: `current_state` is enabled AND (`default_state` is
  not `PresetDefault::Enable`, OR `default_state` is `None`). If
  `default_state == PresetDefault::Enable`, the enabled state is
  ambiguous — the user may not have enabled it. Treat as not meeting
  the service signal.
- Config signal: at least one config file owned by this package has
  `ConfigFileKind::RpmOwnedModified`.

Reason: `TriageReason::PackageInstallerPromotedService`

Example: firewalld with user-enabled service + custom zones detected.

**Path B — Config-only promotion (curated list):**

Some packages have user-modified config but no systemd service, or a
service that was default-enabled. A curated list of known
config-centric packages can promote on config signal alone.

```
CONFIG_ONLY_PROMOTION:
  sudo
  logrotate
  chrony
  sssd
  pam
```

Requires: at least one config file owned by this package has
`ConfigFileKind::RpmOwnedModified`.

Reason: `TriageReason::PackageInstallerPromotedConfig`

This list is intentionally conservative. Packages not on this list
require both signals (Path A) for promotion.

**Both paths:** `include: true`, `locked: false`.

#### Tier 3: Soft Exclude (Baseline / installer_default)

High-confidence installer noise: packages that are clearly not user
workload and would never be intentionally selected via group-install
or kickstart. These are identified by a curated list of known
installer-noise patterns.

```
INSTALLER_NOISE_PATTERNS:
  *-fonts           (font packages)
  *-fonts-common
  fonts-filesystem
  default-fonts-*
  lshw              (bare-metal HW inspection)
  lsscsi
  libsysfs
  initscripts-*     (legacy init compat)
  prefixdevname     (bare-metal NIC naming)
  rootfiles         (default shell dotfiles)
  kernel-tools*     (CPU frequency, bare-metal)
  dracut-config-rescue
  mtools            (floppy/EFI media tools)
  biosdevname       (legacy BIOS device naming)
```

These are excluded by default but toggleable in the refine UI.

Implementation: `include: false`, `locked: false`.

Reason: `TriageReason::PackageInstallerDefault`

#### Tier 4: Ambiguous Anaconda (Investigate / installer_ambiguous)

Everything else in the anaconda set that passed the precedence check,
has no promotion signal, and doesn't match the installer-noise list.
These are packages where we cannot distinguish between "Anaconda
dragged this in as a default" and "the user selected this group or
package at install time."

This is the critical safety net: group-install choices like
container-tools, firewalld (without custom config), and other
deliberate install-time selections land here instead of being silently
excluded. The user reviews them in refine and makes the call.

Implementation: classified as `Investigate`, `include: true`,
`locked: false`.

Reason: `TriageReason::PackageInstallerAmbiguous`

The default is `include: true` because these packages may represent
user intent. It is safer to include something the user can exclude
than to exclude something the user intended to keep.

### Missing-Signal Fallback

When the evidence required for promotion is unavailable — `snap.services`
is `None`, `snap.config` is `None`, `owning_package` is missing, or
config-to-package ownership joins fail — the classifier MUST NOT fall
through to Tier 3.

**Fallback behavior:** preserve the package's existing classification
from the standard `classify_packages()` path. If no existing
classification applies, classify as `Investigate` with reason
`PackageInstallerEvidenceUnavailable`.

The principle: when evidence collection is incomplete, the classifier
becomes less confident, not more willing to exclude.

### Classification Flow

For each package in `packages_added` where `source_repo == "anaconda"`:

1. If name matches `PLATFORM_PLUMBING_PREFIXES` → Tier 1 (hard
   exclude, locked). Checked first — platform plumbing always wins
   regardless of other signals.
2. **Precedence check:** If the package already has a reason other
   than `PackageUserAdded` or `PackageProvenanceUnavailable`, skip —
   the existing classification is stronger.
3. **Evidence availability check:** If service or config data is
   missing or ownership joins fail → preserve existing classification
   or `Investigate / installer_evidence_unavailable`.
4. If package meets Path A (dual-signal promotion) → Tier 2,
   reason `package_installer_promoted_service`.
5. If package is in `CONFIG_ONLY_PROMOTION` AND meets config
   signal → Tier 2, reason `package_installer_promoted_config`.
6. If name matches `INSTALLER_NOISE_PATTERNS` → Tier 3 (soft
   exclude, toggleable), reason `package_installer_default`.
7. Else → Tier 4 (Investigate, included by default), reason
   `package_installer_ambiguous`.

Note: group membership is NOT a classification signal — it is a
rendering concern. Group-installed packages follow the same
classification flow as any other package. See the Containerfile
Renderer section for how group membership affects output.

## Pipeline Integration

### Where

`crates/refine/src/classify.rs`, inside `classify_packages()`. The
anaconda gap classification runs as a post-pass after the existing
classification logic, respecting the precedence rules above.

### Group-Install Collection (New)

A new collection step in the RPM inspector gathers installed dnf
groups:

1. Run `dnf group list --installed` to get the list of installed
   group names.
2. For each installed group, run `dnf group info <group>` to get the
   member package list (mandatory + default + optional installed).
3. Build a map: `package_name → Vec<group_name>` for all member
   packages across all installed groups.

This data is stored in a new field on the RPM snapshot section:

```rust
pub installed_groups: Option<Vec<InstalledGroup>>

pub struct InstalledGroup {
    pub name: String,
    pub packages: Vec<String>,
}
```

The field is `Option` so that missing group data (e.g., dnf not
available, or running on a system without group metadata) triggers
the missing-signal fallback rather than false classification.

**Performance:** `dnf group list --installed` is fast (<1s). `dnf
group info` for each group adds ~0.5s per group. Typical installs
have 1-3 groups. Total: ~2-3s, acceptable within the scan budget.

### Cross-Referencing Service and Config Data

Build two lookup structures at the start of the anaconda classification
block:

1. A map of package names to their service state (from
   `snap.services` via `owning_package`, filtered to services where
   `current_state` is enabled and `default_state` is not
   `PresetDefault::Enable`).
2. A set of package names that own at least one
   `ConfigFileKind::RpmOwnedModified` config file (from `snap.config`
   via RPM ownership).

If `snap.services` or `snap.config` is `None`, the corresponding
lookup is empty and the missing-signal fallback applies.

### New Type Variants

In `inspectah-refine` (where `TriageReason` is defined), using the
existing `#[serde(rename_all = "snake_case")]` convention:

- `PackagePlatformPlumbing` → wire: `"package_platform_plumbing"`
- `PackageInstallerDefault` → wire: `"package_installer_default"`
- `PackageInstallerPromotedService` → wire:
  `"package_installer_promoted_service"`
- `PackageInstallerPromotedConfig` → wire:
  `"package_installer_promoted_config"`
- `PackageInstallerAmbiguous` → wire:
  `"package_installer_ambiguous"`
- `PackageInstallerEvidenceUnavailable` → wire:
  `"package_installer_evidence_unavailable"`

All follow the existing `Package*` naming family and snake_case wire
format. Serialization regression tests must assert the exact emitted
strings.

### Containerfile Renderer

**Group-install rendering (new).** When included packages belong to
an installed dnf group, the renderer emits `dnf group install`
commands instead of listing those packages individually. This
preserves the user's original install-time intent and produces a
cleaner, more maintainable Containerfile.

```dockerfile
# === Package Groups (1) ===
RUN dnf group install -y "Container Management" \
    && dnf clean all

# === Packages (5) ===
RUN dnf install -y \
    bat \
    htop \
    ...
```

The group-install section is placed before the individual package
`dnf install` block. Packages that belong to an installed group and
are included are rendered via their group and excluded from the
individual package list to avoid duplication.

If a package belongs to multiple groups, it is attributed to the
first group alphabetically (deterministic output). If a user
excludes individual packages from a group in refine, those packages
are excluded from the group — if all members of a group are
excluded, the `dnf group install` line is dropped entirely.

Group membership is purely a rendering concern. It does not affect
classification, triage, include/exclude defaults, or refine toggle
behavior. A group-installed package follows the same classification
flow as any other package — it may be baseline-suppressed, promoted
via service/config signals, soft-excluded as installer noise, or
sent to Investigate as ambiguous.

**Repo assumption:** `dnf group install` works from standard RHEL
BaseOS/AppStream repos (and Fedora equivalents). Group definitions
are comps metadata shipped with the repo — no additional
configuration required beyond what the `FROM` base image already
provides. This is the same repo surface `dnf install` already uses.

For non-group packages, the existing `dnf install` rendering is
unchanged. The renderer already respects `include: true/false` on
each package. The `locked` field is already enforced by
`clamp_locked_items()` on export.

### Refine UI (Web)

The new reason variants surface through existing reason display
mechanisms.

**Group display (new).** When packages belong to installed dnf
groups, the group replaces its member packages in the list:

- **Group row (collapsed):** Single row with chevron, group name
  in semibold, package count as muted suffix ("12 packages"), and
  one include/exclude toggle. Groups sort alphabetically alongside
  individual packages — no separate section.
- **Expanded view:** Clicking the chevron reveals the direct member
  packages indented one level (name + version). No individual
  toggles — the group is the unit of decision, matching the
  Containerfile's `dnf group install` rendering.
- **Search:** Searching "podman" surfaces the Container Management
  group if podman is a member, with "contains: podman" subtitle.
  The group expands in place to confirm the match.
- **Summary line:** Top of the package list shows composition at a
  glance: "4 groups, 47 individual packages."

**Ungroup action.** An icon button on the group row (secondary to
the toggle) dissolves the group into individual package rows:

- Click replaces the group row with its member packages, each with
  its own toggle, sorted alphabetically into the flat list.
- The Containerfile switches from `dnf group install` to individual
  `dnf install` lines for those packages.
- Brief inline toast: "Container Management broken into 12 packages."
- One-way door — no re-group action. Reset or re-scan to restore.
- Optional: ungrouped rows show a muted provenance tag "(was:
  Container Management)" for context.

This is necessary for consistency: if the Containerfile renders
`dnf group install`, the refine UI must show the user what that
group contains so they can make informed include/exclude decisions.
The ungroup action gives users an escape hatch for granular control
when the group-as-unit model doesn't fit their needs.

**Deferred to follow-on:** Grouped display for platform-plumbing
packages (collapsed grayed-out section), signal evidence subtitles
for promoted packages, and audit report annotations. These require
typed presentation metadata that the current refine projection and
report renderer do not support.

### Snapshot Schema

One new field: `installed_groups: Option<Vec<InstalledGroup>>` on the
RPM snapshot section. This is the only schema addition in this spec.

`source_repo: "anaconda"` is already in the snapshot. The new
classification reasons are refine-layer metadata stored in
`RefinedPackage`, not in the raw `InspectionSnapshot`.

## Scope

### In Scope

- Group-install collection (`dnf group list --installed` + member
  resolution) and new `installed_groups` snapshot field
- Anaconda gap classification post-pass in `classify_packages()`
- Tier 1 platform plumbing checked first, always wins
- Precedence rules preserving stronger existing signals (Tiers 2-4)
- Two typed promotion paths (dual-signal, config-only)
- Group-install collection and `dnf group install` rendering in
  Containerfile (rendering concern, not classification)
- Refine UI group display (collapsible group headers, group-level
  and individual toggles)
- Installer-noise soft-exclude for high-confidence non-workload
- Ambiguous-anaconda → Investigate for remaining unclassified packages
- Missing-signal fallback to preserve/investigate
- Serialization regression tests for new reason variants
- Locking contract tests at session and API boundary

### Out of Scope

- UI grouping for platform-plumbing (collapsed grayed-out section)
- Audit report annotations for installer defaults
- Signal evidence subtitles in refine UI
- Fedora-specific validation (follow-on with Fedora tarballs)
- New snapshot schema fields beyond `installed_groups`
- Boot/kernel config merging into Containerfile output
- Cross-distro (openSUSE) support — separate roadmap item

## Testing

- Unit tests for each tier in `classify.rs`: platform-plumbing match,
  dual-signal promotion, config-only promotion, installer-noise
  soft-exclude, ambiguous-anaconda → Investigate
- **Precedence tests:** anaconda-sourced package with
  `PackageVersionChanged` keeps its existing classification (Tier 1
  exception: platform plumbing still wins). Anaconda-sourced package
  with `PackageLocalInstall` is not reclassified. Anaconda-sourced
  package with `PackageUserAdded` IS reclassified. Platform-plumbing
  package with `PackageVersionChanged` IS still hard-excluded.
- **Tier 4 tests:** anaconda-sourced package not matching noise
  patterns and without promotion signals → `Investigate` with
  `include: true`. Validates group-install packages are not silently
  excluded.
- **Missing-signal tests:** `snap.services = None` → packages
  preserve existing classification or move to Investigate.
  `snap.config = None` → same. Missing `owning_package` on service →
  same.
- **Locking tests:** Tier 1 `SetInclude(true)` is a no-op at session
  layer. Export clamps locked items. API returns unchanged state.
- **Group-install tests:** collection parses `dnf group list`/`dnf
  group info` output. Containerfile renders `dnf group install` before
  individual packages. Group members excluded from individual `dnf
  install` block. Excluding one member removes it from group render
  but keeps the group. Excluding all members drops the group line.
- **Refine UI group tests:** group-member packages hidden behind
  group row. Group toggle includes/excludes all members. Search for
  a member surfaces the group with "contains:" subtitle. Ungroup
  dissolves group into individual package rows with own toggles.
  Ungrouped packages render as individual `dnf install` lines.
- **Serialization tests:** all six new reason variants
  serialize/deserialize to the expected snake_case strings.
- Containerfile output excludes platform-plumbing and
  installer-default packages, includes promoted and ambiguous packages.
  Group-promoted packages appear as `dnf group install` commands.
- Snapshot round-trip: new reason variants survive
  serialize/deserialize cycle.

## Known Limitations

- `CONFIG_ONLY_PROMOTION` and `INSTALLER_NOISE_PATTERNS` are curated
  lists that may need expansion over time as new packages are
  identified. The lists are intentionally conservative.
- Group-install detection resolves the most common install-time intent
  ambiguity, but kickstart `%packages` entries that aren't part of a
  named group still lack a detection signal. Tier 4 (Investigate /
  ambiguous) handles these by defaulting to included.
- Fedora validation is deferred. The classification model is
  signal-based and should generalize, but needs confirmation with
  Fedora scan tarballs.
- UI presentation improvements (grouped display, evidence subtitles,
  audit annotations) are deferred to a follow-on spec that addresses
  the typed presentation model gap.
