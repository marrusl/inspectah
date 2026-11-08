---
title: Baseline Subtraction
parent: How-To Guides
nav_order: 3
---

# Baseline Subtraction

Baseline subtraction compares your host's packages against a target container
image so inspectah can distinguish OS-provided content from site-specific
additions. This guide covers how to control that comparison.

## How baseline resolution works

When you run `inspectah scan`, it automatically resolves the target base image
using this chain (first match wins):

1. **CLI override**: The `--base-image` flag, if provided
2. **Universal Blue metadata**: Detected from UBlue-specific files on the host
3. **bootc status**: The image reference reported by `bootc status`
4. **Fedora Atomic Desktop**: Matched from os-release variant ID
5. **os-release mapping**: Derived from `/etc/os-release` with version clamping

The os-release mapping reads the distribution ID and version and picks the
matching bootc base:

- **RHEL 9 and 10** resolve to the `rhel-bootc` image for the host's own
  release, for example `registry.redhat.io/rhel9/rhel-bootc:9.8` on a RHEL 9.8
  host. Releases older than the first one that ships a bootc image (9.6 on
  RHEL 9) are raised to that release.
- **CentOS Stream** resolves to `quay.io/centos-bootc/centos-bootc:stream9`
  (or `stream10`).
- **RHEL 8 and CentOS Stream 8** have no bootc image, so they resolve to the
  EL9 base by default: `registry.redhat.io/rhel9/rhel-bootc:9.6` and
  `quay.io/centos-bootc/centos-bootc:stream9`. Pass `--base-image` if you want
  a different target.

Once resolved, inspectah uses the container image from local podman storage,
or pulls it from the registry when it is not stored, and extracts its package
manifest. Every package on your host that matches the base image is classified
as **Baseline** and excluded from the generated Containerfile by default.

## Specify a target base image

When auto-detection picks the wrong image, or you want to compare against a
different target, use `--base-image`:

```bash
# Fedora example
sudo inspectah scan --base-image quay.io/fedora/fedora-bootc:41

# CentOS Stream example
sudo inspectah scan --base-image quay.io/centos-bootc/centos-bootc:stream9

# RHEL example (requires registry.redhat.io authentication)
sudo inspectah scan --base-image registry.redhat.io/rhel9/rhel-bootc:9.6
```

This overrides all auto-detection and uses the specified image reference
directly. The image must be pullable from the host where you run the scan,
or already in that host's local podman storage.

### Cross-distro conversion

The `--base-image` flag also enables cross-distro comparison. For example,
if you are migrating a CentOS Stream host to RHEL, point at the RHEL base
image:

```bash
sudo inspectah scan --base-image registry.redhat.io/rhel9/rhel-bootc:9.6
```

Packages that exist in both the host and the target image are classified as
Baseline, even though the host was not originally running that image. This
gives you a clear view of what you need to carry forward versus what ships
with the target.

## Baseline extraction is mandatory

inspectah requires baseline extraction to classify packages and configurations
correctly. If the scan cannot get the target base image from local storage or the
registry, it exits with code 3 and shows a detailed error with remediation
guidance.

For disconnected or air-gapped environments, you have two options:

1. **Pre-stage the base image:** Pull the image on a connected machine, save it
   as a tarball with `podman save`, transfer it to the target host, and load it
   with `podman load`. The image you load has to be the one the scan resolves.
   Without `--base-image`, the scan resolves the image from the host's own
   release, so a RHEL 9.8 host pulls `rhel-bootc:9.8` and ignores a loaded
   `9.6`. Pass `--base-image` with the loaded reference to make the scan use
   it. Because the image is already in local storage, the scan uses it and
   does not contact the registry.

   ```bash
   # On a connected machine:
   podman pull registry.redhat.io/rhel9/rhel-bootc:9.6
   podman save -o rhel9-bootc-9.6.tar registry.redhat.io/rhel9/rhel-bootc:9.6

   # Transfer the tarball to the target host, then:
   sudo podman load -i rhel9-bootc-9.6.tar
   sudo inspectah scan --base-image registry.redhat.io/rhel9/rhel-bootc:9.6
   ```

2. **Use a local or mirror registry:** Configure a mirror registry in your
   air-gapped environment and ensure the target base image is replicated there.
   inspectah will pull from the configured registry.

## What baseline extraction does

During a baseline scan, inspectah:

1. **Resolves** the base image reference (auto-detection or CLI override)
2. **Normalizes** the image reference to a fully qualified form
3. **Pulls** the container image layers (with progress output), or uses the
   image already in local storage without pulling. A stored image is not
   refreshed: pull it again or remove it with `podman rmi` to pick up a newer
   build
4. **Extracts** the package manifest from the image
5. **Compares** each host package against the manifest

Packages that match are left out of `rpm.packages_added` in the snapshot
data, and the base image's package list is recorded under `baseline.packages`
(an object keyed by `name.arch`). The refine UI uses this to pre-populate
triage decisions, letting you focus on the packages that actually differ from
the base image.

### Network requirements

Baseline extraction needs the target image. When it is not already in local
podman storage, the host needs network access to the registry hosting it. For
air-gapped environments, see the section above on pre-staging the base image via
`podman save` and `podman load`.

## Verify the resolved image

The scan output shows which image was resolved and how:

```
Resolving target image...
  registry.redhat.io/rhel9/rhel-bootc:9.8 (OsRelease)
Preparing baseline image registry.redhat.io/rhel9/rhel-bootc:9.8...
Pulled registry.redhat.io/rhel9/rhel-bootc:9.8 (sha256:173c055cc9c7)
  Baseline extracted: 457 packages
  Resolved via: os-release (auto-detected)
```

When the image is already in local podman storage, no pull runs and the
`Pulled` line is replaced by:

```
Using local image registry.redhat.io/rhel9/rhel-bootc:9.8 (sha256:173c055cc9c7)
  Not pulled. To refresh it, run podman pull registry.redhat.io/rhel9/rhel-bootc:9.8 or podman rmi registry.redhat.io/rhel9/rhel-bootc:9.8.
```

The resolution strategy tells you which step in the chain was used. The
resolve line carries the strategy's internal name in parentheses, and the
`Resolved via` line spells it out; with `--base-image` it reads
`Resolved via: --base-image (user-specified)`. If auto-detection selected the
wrong image, re-run with `--base-image` to override.

## Aggregate baseline

When aggregating scans, you can override the baseline for the entire
aggregate using the `--target-image` flag on `inspectah aggregate`. See the
[Aggregation](aggregation.md) guide for details.
