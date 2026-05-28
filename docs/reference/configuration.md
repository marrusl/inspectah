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
| `INSPECTAH_PROGRESS` | `rich`, `plain`, `flat` | Auto-detected | Override progress display mode. Takes effect when no `--progress` CLI flag is provided. |
| `NO_COLOR` | (any value) | Unset | Disable ANSI color output. Follows the [no-color.org](https://no-color.org/) convention. |
| `TERM` | `dumb`, etc. | Varies | When set to `dumb`, forces `flat` progress mode (same as non-TTY detection). |

## Progress display modes

The progress display mode controls how scan progress is rendered. Resolution
priority: **CLI flag** > **`INSPECTAH_PROGRESS` env** > **TTY auto-detection**.

| Mode | Behavior | Best for |
|:-----|:---------|:---------|
| `rich` | Animated spinner with live sub-step updates. Requires a capable terminal. | Interactive terminal use |
| `plain` | Durable scrollback-friendly output. No cursor manipulation. | Terminal sessions where you want persistent output |
| `flat` | Numbered sequential lines, no ANSI escape codes. | CI/CD pipelines, piped output, non-TTY environments |

**Auto-detection logic:** If stderr is not a TTY or `TERM=dumb`, defaults to
`flat`. Otherwise defaults to `rich`.

## CLI flags (scan)

These flags are passed to `inspectah scan`. See the [CLI Reference](cli.html)
for the full command reference.

| Flag | Description |
|:-----|:------------|
| `--inspect-only` | Write JSON snapshot only, skip tarball generation. |
| `-o, --output <PATH>` | Output file path (tarball) or directory (with `--inspect-only`). |
| `--base-image <IMAGE>` | Target base image for baseline comparison (e.g., `quay.io/centos-bootc/centos-bootc:stream9`). |
| `--no-baseline` | Skip baseline extraction. Produces degraded classification (no added/removed distinction). |
| `--preserve-password-hashes` | Retain password hashes for users with `password_set` status. |
| `--preserve-ssh-keys` | Retain full SSH `authorized_keys` content per user. |
| `--acknowledge-sensitive` | Required for tarball export when preserve flags are used. Explicit operator acknowledgment. |
| `--progress <MODE>` | Override progress display: `rich`, `plain`, or `flat`. |
| `-v, --verbose` | Show sub-step detail for all inspectors. Conflicts with `--quiet`. |
| `-q, --quiet` | Suppress the scan progress checklist. Conflicts with `--verbose`. |

## CLI flags (fleet aggregate)

| Flag | Description |
|:-----|:------------|
| `--manifest <PATH>` | Path to a fleet manifest (TOML). Cannot be combined with positional inputs. |
| `--baseline <IMAGE>` | Override the baseline image reference from the manifest. |
| `--output-dir <DIR>` | Output directory for the fleet tarball. |
| `--output-file <FILE>` | Output file path for the fleet tarball. |
| `--json-only` | Write JSON snapshot to stdout (or file) instead of tarball. |
| `--strict` | Treat aggregation warnings as errors. |
| `-v, --verbose` | Show per-host detail in output. |

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
