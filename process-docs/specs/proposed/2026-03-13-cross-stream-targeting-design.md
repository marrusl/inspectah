# Cross-Stream Targeting

**Date:** 2026-03-13
**Status:** Brainstorming (incomplete — approaches and design pending)

## Problem

inspectah currently assumes source and target are the same OS stream: a
RHEL 9 host produces a RHEL 9 bootc image. Users migrating between
streams — CentOS Stream 9 → RHEL 9, RHEL 9 → RHEL 10, Fedora →
CentOS — must manually adjust the output. Package names, base images,
and repo configurations may differ between streams even within the Red
Hat family.

## Scope

**This spec:** Same-family stream targeting (RHEL, CentOS Stream,
Fedora). These share RPM/dnf, systemd, SELinux, and largely identical
config file layouts. The differences are in package naming, repository
structure, and base image selection.

**Future work (documented here, not implemented):**
- Package name mapping infrastructure (bundled data file)
- Cross-family migration (Ubuntu/Debian → RPM distro)
- Community-maintained package equivalence database

## Decisions So Far

### Target Inference

The target distro and version are derived from the base image's
`/etc/os-release` during the existing base image pull. When the user
specifies `--base-image registry.redhat.io/rhel9/rhel-bootc:9.6`,
inspectah reads the image's os-release and knows the target is RHEL 9.6.

Explicit overrides (`--target-distro`, `--target-version`) are available
for edge cases: `--inspect-only` mode, custom base images without
standard os-release, or testing.

**Rationale:** The base image already encodes the user's target choice.
Asking them to also specify `--target rhel:9` is redundant. Deriving
from the image is reliable (os-release is always present in bootc
images) and adds no extra pull since the image is already fetched for
baseline package comparison.

### Package Name Mapping

A bundled TOML or JSON data file shipped with inspectah, containing known
package name mappings between streams. Versioned alongside the code,
testable, and community-contributable via PRs.

Structure TBD — likely keyed by `(source_distro, source_version,
target_distro, target_version)` with entries for packages whose names
differ.

**Not in scope for v1:** runtime-fetched external databases, AI-powered
package equivalence, or automatic discovery of renamed packages.

### Use Cases (equally weighted)

- **Upgrade:** RHEL 9 host → RHEL 10 bootc image
- **Lateral:** CentOS Stream 9 host → RHEL 9 bootc image
- **Lateral:** Fedora host → CentOS Stream bootc image

## Open Questions (for next session)

1. **Approach selection:** How does the target context flow through the
   pipeline? Does the snapshot carry source+target metadata, or is the
   target only known at render time?

2. **Package mapping granularity:** Per-package name mapping, or
   higher-level "provides" mapping (package X on CentOS provides the
   same capability as package Y on RHEL)?

3. **Config file differences:** Between major versions (e.g., RHEL 9 →
   10), config file formats may change. How does inspectah handle config
   files that are valid on the source but not on the target?

4. **Repo file handling:** Source repo files (e.g., CentOS repos) are
   meaningless on the target (RHEL). Should the renderer strip them
   and substitute target-appropriate repos, or just flag them?

5. **Validation:** Should `inspectah --validate` (podman build) use the
   target's base image to verify the generated Containerfile works?

6. **Report presentation:** How does the HTML report surface
   cross-stream migration issues? A dedicated "Migration Notes" section?
   Inline warnings per affected item?

## Future Work: Cross-Family Migration

Ubuntu/Debian → RPM-based bootc images. This requires:

- **New inspectors** for apt/dpkg package management
- **Generalized package model** in the schema (or parallel schema types)
- **Config path translation** (e.g., `/etc/apache2/` → `/etc/httpd/`)
- **Security framework mapping** (AppArmor profiles → SELinux policies)
- **Service name translation** (sometimes differ between distros)
- **Package name mapping at scale** — the bundled data file from
  same-family work becomes the foundation, but cross-family mapping is
  orders of magnitude larger

The same-family work deliberately builds toward this: target inference,
package mapping infrastructure, and source/target metadata in the
snapshot are all prerequisites. But cross-family migration is a separate
project with its own spec cycle.
