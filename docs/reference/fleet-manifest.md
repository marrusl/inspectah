---
title: Fleet Manifest
parent: Reference
nav_order: 6
---

# Fleet Manifest

A fleet manifest is a TOML file that declares which host snapshots to aggregate
into a fleet view. It is consumed by `inspectah fleet aggregate`.

**Source:** `crates/core/src/fleet/manifest.rs`
{: .text-grey-dk-000 }

## Format

```toml
# inspectah fleet manifest
# Edit label and target_image as needed. Sources are relative to this file.

label = "web-servers"
target_image = "quay.io/centos-bootc/centos-bootc:stream9"

sources = [
  "scans/host-a.tar.gz",
  "scans/host-b.tar.gz",
  "scans/host-c.tar.gz",
]
```

The `target_image` value depends on your distro. Common base images:

| Distro | Base image |
|:-------|:-----------|
| Fedora | `quay.io/fedora/fedora-bootc:41` |
| CentOS Stream 9 | `quay.io/centos-bootc/centos-bootc:stream9` |
| RHEL 9 | `registry.redhat.io/rhel9/rhel-bootc:9.6` |

## Fields

| Field | Type | Required | Description |
|:------|:-----|:---------|:------------|
| `label` | string | No | Human-readable name for the fleet (e.g., `"web-servers"`, `"db-tier"`). Used in output filenames and fleet metadata. |
| `target_image` | string | No | Target base image reference for baseline comparison (e.g., `"quay.io/centos-bootc/centos-bootc:stream9"`). Overridable via `--target-image` CLI flag. |
| `sources` | array of strings | **Yes** | Paths to host snapshot tarballs. Relative paths are resolved relative to the manifest file's parent directory. |

## Path resolution

Source paths in the manifest can be either absolute or relative:

- **Relative paths** are resolved against the directory containing the manifest file.
- **Absolute paths** are used as-is.

For example, given this layout:

```
fleet/
  fleet.toml        # contains sources = ["scans/a.tar.gz"]
  scans/
    a.tar.gz
```

The path `scans/a.tar.gz` resolves to `fleet/scans/a.tar.gz`.

## Generating a manifest

Use `inspectah fleet init` to generate a manifest from a directory of tarballs:

```bash
inspectah fleet init /path/to/scans/
```

This scans the directory for `.tar.gz` files and writes a `fleet.toml` with:

- `label` derived from the directory name
- `target_image` commented out (placeholder)
- `sources` populated with relative paths to each tarball

### Options

| Flag | Description |
|:-----|:------------|
| `--output <PATH>` | Output path for the generated manifest. Defaults to `fleet.toml` in the current directory. |
| `--overwrite` | Overwrite an existing manifest file. Without this flag, existing files are not overwritten. |

## Using a manifest with fleet aggregate

Pass the manifest to `fleet aggregate` via the `--manifest` flag:

```bash
inspectah fleet aggregate --manifest fleet.toml
```

When using `--manifest`, positional input arguments are not allowed -- the manifest
is the sole source of truth for which tarballs to include.

### CLI overrides

| Flag | Behavior |
|:-----|:---------|
| `--target-image <IMAGE>` | Overrides the `target_image` field from the manifest. |
| `--output-dir <DIR>` | Output directory for the fleet tarball. |
| `--output-file <FILE>` | Output file path for the fleet tarball. |
| `--json-only` | Write JSON snapshot instead of tarball. |
| `--strict` | Treat warnings as errors. |
| `-v, --verbose` | Show per-host detail in output. |

## Minimal manifest

Only `sources` is required. A minimal manifest:

```toml
sources = ["a.tar.gz", "b.tar.gz"]
```

This produces a fleet snapshot with no label and no target image comparison.

## Validation

During `fleet aggregate`, the following validations apply:

- Each source path must point to a readable tarball file.
- Each tarball must contain a valid inspection snapshot.
- Hostnames across sources must be unique -- duplicate hostnames produce an error.
- Empty snapshots (no inspector data) produce an error.
