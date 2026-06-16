---
title: Aggregate Manifest
parent: Reference
nav_order: 6
---

# Aggregate Manifest

An aggregate manifest is a TOML file that declares which host snapshots to aggregate
into a multi-host view. It is consumed by `inspectah aggregate`.

**Source:** `crates/core/src/aggregate/manifest.rs`
{: .text-grey-dk-000 }

## Format

```toml
# inspectah aggregate manifest
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
| `label` | string | No | Human-readable name for this group (e.g., `"web-servers"`, `"db-tier"`). Used in output filenames and aggregate metadata. |
| `target_image` | string | No | Target base image reference for baseline comparison (e.g., `"quay.io/centos-bootc/centos-bootc:stream9"`). Overridable via `--target-image` CLI flag. |
| `sources` | array of strings | **Yes** | Paths to host snapshot tarballs. Relative paths are resolved relative to the manifest file's parent directory. |

## Path resolution

Source paths in the manifest can be either absolute or relative:

- **Relative paths** are resolved against the directory containing the manifest file.
- **Absolute paths** are used as-is.

For example, given this layout:

```
aggregate/
  aggregate.toml        # contains sources = ["scans/a.tar.gz"]
  scans/
    a.tar.gz
```

The path `scans/a.tar.gz` resolves to `aggregate/scans/a.tar.gz`.

## Generating a manifest

Use `inspectah aggregate init` to generate a manifest from a directory of tarballs:

```bash
inspectah aggregate init /path/to/scans/
```

This scans the directory for `.tar.gz` files and writes an `aggregate.toml` with:

- `label` derived from the directory name
- `target_image` commented out (placeholder)
- `sources` populated with relative paths to each tarball

### Options

| Flag | Description |
|:-----|:------------|
| `--output <PATH>` | Output path for the generated manifest. Defaults to `aggregate.toml` in the current directory. |
| `--overwrite` | Overwrite an existing manifest file. Without this flag, existing files are not overwritten. |

## Using a manifest with aggregate

Pass the manifest to `aggregate` via the `--manifest` flag:

```bash
inspectah aggregate --manifest aggregate.toml
```

When using `--manifest`, positional input arguments are not allowed -- the manifest
is the sole source of truth for which tarballs to include.

### CLI overrides

| Flag | Behavior |
|:-----|:---------|
| `--target-image <IMAGE>` | Overrides the `target_image` field from the manifest. |
| `--output-dir <DIR>` | Output directory for the aggregate tarball. |
| `--output-file <FILE>` | Output file path for the aggregate tarball. |
| `--json-only` | Write JSON snapshot instead of tarball. |
| `--strict` | Treat warnings as errors. |
| `-v, --verbose` | Show per-host detail in output. |

## Minimal manifest

Only `sources` is required. A minimal manifest:

```toml
sources = ["a.tar.gz", "b.tar.gz"]
```

This produces an aggregate snapshot with no label and no target image comparison.

## Validation

During aggregation, the following validations apply:

- Each source path must point to a readable tarball file.
- Each tarball must contain a valid inspection snapshot.
- Hostnames across sources must be unique -- duplicate hostnames produce an error.
- Empty snapshots (no inspector data) produce an error.
