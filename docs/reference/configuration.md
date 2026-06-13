---
title: Configuration
parent: Reference
nav_order: 7
---

# Configuration

inspectah does not use a configuration file. All behavior is controlled through
CLI flags and environment variables.

## Environment variables

| Variable | Values | Default | Description |
|:---------|:-------|:--------|:------------|
| `INSPECTAH_PROGRESS` | `pretty`, `flat` | Auto-detected | Override progress display mode. Takes effect when no `--progress` CLI flag is provided. |
| `NO_COLOR` | (any value) | Unset | Disable ANSI color output. Follows the [no-color.org](https://no-color.org/) convention. |
| `TERM` | `dumb`, etc. | Varies | When set to `dumb`, forces `flat` progress mode (same as non-TTY detection). |

## Progress display modes

The progress display mode controls how scan progress is rendered. Resolution
priority: **CLI flag** > **`INSPECTAH_PROGRESS` env** > **TTY auto-detection**.

| Mode | Behavior | Best for |
|:-----|:---------|:---------|
| `pretty` | Append-only receipt with Unicode symbols. No cursor manipulation. Sub-step detail shown only with `--verbose`. | Interactive terminal use |
| `flat` | Numbered sequential lines, no ANSI escape codes. Sub-step detail shown only with `--verbose`. | CI/CD pipelines, piped output, non-TTY environments |

**Auto-detection logic:** If stderr is not a TTY or `TERM=dumb`, defaults to
`flat`. Otherwise defaults to `pretty`.

## CLI flags (scan)

These flags are passed to `inspectah scan`. See the [CLI Reference](cli.html)
for the full command reference.

| Flag | Description |
|:-----|:------------|
| `--inspect-only` | Write JSON snapshot only, skip tarball generation. |
| `-o, --output <PATH>` | Output file path (tarball) or directory (with `--inspect-only`). |
| `--base-image <IMAGE>` | Target base image for baseline comparison (e.g., `quay.io/centos-bootc/centos-bootc:stream9`). |
| `--preserve <ITEM>` | Preserve sensitive data (password-hashes, ssh-keys, subscription, all). Comma-separated, repeatable. |
| `--no-redaction` | Skip redaction pipeline, retaining raw secrets (requires --ack-sensitive). |
| `--ack-sensitive` | Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction). Alias: `--acknowledge-sensitive`. |
| `--progress <MODE>` | Override progress display: `pretty` or `flat`. |
| `-v, --verbose` | Show sub-step detail for all inspectors. Conflicts with `--quiet`. |
| `-q, --quiet` | Suppress the scan progress checklist. Conflicts with `--verbose`. |

## CLI flags (build)

| Flag | Description |
|:-----|:------------|
| `-t, --tag <TAG>` | Image tag (required). Must include a version, e.g., `myimage:v1`. |
| `--dry-run` | Show the build command without executing it. |
| `--keep-context` | Keep the extracted build context after build completes. |
| `[-- <PODMAN_ARGS>...]` | Additional arguments to pass to `podman build`. |

## CLI flags (refine)

| Flag | Description |
|:-----|:------------|
| `--port <PORT>` | Port to bind the web UI (default: 8642, use 0 for ephemeral). |
| `--open <true\|false>` | Open browser automatically (default: true). Use `--no-open` to suppress. |
| `--fresh` | Start a fresh session, discarding any saved progress. |
| `--tui` | Use terminal UI instead of web browser. |

## CLI flags (fleet aggregate)

| Flag | Description |
|:-----|:------------|
| `--manifest <PATH>` | Path to a fleet manifest (TOML). Cannot be combined with positional inputs. |
| `--target-image <IMAGE>` | Override the target image reference from the manifest. |
| `--output-dir <DIR>` | Output directory for the fleet tarball. |
| `--output-file <FILE>` | Output file path for the fleet tarball. |
| `--json-only` | Write JSON snapshot to stdout (or file) instead of tarball. |
| `--strict` | Treat aggregation warnings as errors. |
| `-v, --verbose` | Show per-host detail in output. |
| `--ack-sensitive` | Acknowledge that the merged output may contain sensitive data. Required when any contributing snapshot has `sensitive_snapshot` set. Alias: `--acknowledge-sensitive`. |

## CLI flags (fleet init)

| Flag | Description |
|:-----|:------------|
| `--output <PATH>` | Output path for the generated manifest. Defaults to `fleet.toml`. |
| `--overwrite` | Overwrite an existing manifest file. |

## Exit codes

| Code | Meaning |
|:-----|:--------|
| `0` | Scan completed successfully (clean or degraded). |
| `1` | Fatal error (bad arguments, I/O failure, etc.). |
| `2` | Incomplete scan (critical inspectors failed). Also used when no subcommand is given. |
| `130` | Scan interrupted (e.g., Ctrl-C). |
