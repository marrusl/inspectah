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

1. **CLI override** -- The `--base-image` flag, if provided
2. **Universal Blue metadata** -- Detected from UBlue-specific files on the host
3. **bootc status** -- The image reference reported by `bootc status`
4. **Fedora Atomic Desktop** -- Matched from os-release variant ID
5. **os-release mapping** -- Derived from `/etc/os-release` with version clamping

Once resolved, inspectah pulls the container image and extracts its package
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
directly. The image must be pullable from the host where you run the scan.

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

## Skip baseline extraction

If you do not need package classification against a base image (for example,
when doing an inventory-only scan), skip the baseline step entirely:

```bash
sudo inspectah scan --no-baseline
```

This runs in **degraded classification mode**. Without a baseline, inspectah
cannot distinguish OS packages from site additions, so all packages receive
a provisional classification. The scan still completes successfully (exit 0)
but the triage data is less precise.

### Mutual exclusivity

You cannot use `--base-image` and `--no-baseline` together. They are
contradictory: one says "use this specific image" while the other says
"skip image comparison entirely." inspectah exits with an error if both
are specified.

## What baseline extraction does

During a baseline scan, inspectah:

1. **Resolves** the base image reference (auto-detection or CLI override)
2. **Normalizes** the image reference to a fully qualified form
3. **Pulls** the container image layers (with progress output)
4. **Extracts** the package manifest from the image
5. **Compares** each host package against the manifest

Packages that match are tagged as `baseline_match` in the snapshot data.
The refine UI uses this to pre-populate triage decisions, letting you focus
on the packages that actually differ from the base image.

### Network requirements

Baseline extraction requires pulling a container image. The host needs
network access to the registry hosting the target image. For air-gapped
environments, use `--no-baseline` and classify packages manually in the
refine UI.

## Verify the resolved image

The scan output shows which image was resolved and how:

```
  Baseline extracted: 847 packages
  Resolved via: OsRelease
```

The resolution strategy tells you which step in the chain was used. If
auto-detection selected the wrong image, re-run with `--base-image` to
override.

## Fleet baseline

When aggregating fleet scans, you can override the baseline for the entire
fleet using the `baseline` field in `fleet.toml` or the `--baseline` flag
on `inspectah fleet aggregate`. See the
[Fleet Aggregation](fleet-aggregation.md) guide for details.
