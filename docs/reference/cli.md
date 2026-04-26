# CLI Reference

Complete flag reference for all inspectah subcommands. For usage examples, see the [README](../../README.md).

---

## `inspectah`

Top-level command. All functionality is accessed through subcommands.

```
inspectah [-h] {scan,fleet,refine,architect} ...
```

If no subcommand is given, or if the first argument looks like a flag (e.g. `--from-snapshot`), `scan` is assumed for backwards compatibility.

| Subcommand | Description |
|------------|-------------|
| `scan` | Scan a host and generate migration artifacts (default) |
| `fleet` | Aggregate multiple inspection snapshots into a fleet report |
| `refine` | Interactively edit and re-render inspection output |
| `architect` | Plan layer decomposition from refined fleets |

---

## `inspectah scan`

Default subcommand. Scans a host and generates migration artifacts.

```
inspectah scan [-h] [--host-root PATH] [-o FILE | --output-dir DIR]
               [--no-subscription] [--from-snapshot PATH] [--inspect-only]
               [--target-version VERSION] [--target-image IMAGE]
               [--baseline-packages FILE] [--no-baseline]
               [--user-strategy STRATEGY] [--config-diffs]
               [--deep-binary-scan] [--query-podman] [--skip-preflight]
               [--validate] [--push-to-github REPO]
               [--github-token TOKEN] [--public] [--yes]
```

### Core Options

| Flag | Description |
|------|-------------|
| `--host-root PATH` | Root path for host inspection (default: `/host`) |
| `-o FILE` | Write tarball to FILE (default: `HOSTNAME-TIMESTAMP.tar.gz` in current directory) |
| `--output-dir DIR` | Write files to a directory instead of producing a tarball. Mutually exclusive with `-o`. |
| `--no-subscription` | Skip bundling RHEL subscription certs into the output |
| `--from-snapshot PATH` | Skip inspection; load snapshot from file and run renderers only. Mutually exclusive with `--inspect-only`. |
| `--inspect-only` | Run inspectors and save snapshot to output; do not run renderers. Mutually exclusive with `--from-snapshot`. |

### Target Image

| Flag | Description |
|------|-------------|
| `--target-version VERSION` | Target bootc image version (e.g. `9.6`, `10.2`). Default: source host version, clamped to minimum bootc-supported release (9.6 for RHEL 9) |
| `--target-image IMAGE` | Full target bootc base image reference (e.g. `registry.redhat.io/rhel10/rhel-bootc:10.2`). Overrides `--target-version` and all automatic mapping |

### Inspection Options

| Flag | Description |
|------|-------------|
| `--baseline-packages FILE` | Path to a newline-separated package list for air-gapped environments where the base image cannot be queried via podman |
| `--no-baseline` | Run without base image comparison -- all installed packages will be included in the Containerfile. Use when the base image cannot be queried and `--baseline-packages` is unavailable. Mutually exclusive with `--baseline-packages`. |
| `--config-diffs` | Generate line-by-line diffs for modified configs via `rpm2cpio` (retrieves from local cache or downloads from repos) |
| `--deep-binary-scan` | Full `strings` scan on unknown binaries with extended version pattern matching (slow) |
| `--query-podman` | Connect to podman to enumerate running containers with full inspect data |
| `--user-strategy STRATEGY` | Override user creation strategy for all users. Valid: `sysusers`, `blueprint`, `useradd`, `kickstart`. Default: auto-assigned per classification (service users get `sysusers`, human users get `kickstart`, ambiguous users get `useradd`). |
| `--skip-preflight` | Skip container privilege checks (rootful, `--pid=host`, `--privileged`, SELinux) |

### Output Options

| Flag | Description |
|------|-------------|
| `--validate` | After generating output, run `podman build` to verify the Containerfile. Requires `--output-dir`. |
| `--push-to-github REPO` | Push output directory to a GitHub repository (e.g. `owner/repo`). Requires `--output-dir`. |
| `--github-token TOKEN` | GitHub personal access token for repo creation (falls back to `GITHUB_TOKEN` env var) |
| `--public` | When creating a new GitHub repo, make it public (default: private) |
| `--yes` | Skip interactive confirmation prompts |

### Internal Flags

These flags are set automatically by `inspectah refine` during re-rendering. They are not intended for direct use.

| Flag | Description |
|------|-------------|
| `--refine-mode` | Enable editor UI in the rendered report (set by `inspectah refine`) |
| `--original-snapshot PATH` | Path to unmodified original snapshot for editor diff/reset support (set by `inspectah refine` during re-render) |

### Examples

```bash
# Basic host inspection (inside container via run-inspectah.sh)
sudo ./run-inspectah.sh

# Scan with a specific target version
inspectah scan --target-version 9.6

# Re-render from a saved snapshot without re-scanning
inspectah scan --from-snapshot inspection-snapshot.json -o refreshed.tar.gz

# Air-gapped: provide baseline package list manually
inspectah scan --baseline-packages rhel9-base-packages.txt

# Full scan with config diffs and container enumeration
inspectah scan --config-diffs --query-podman

# Scan and validate the generated Containerfile builds
inspectah scan --output-dir ./output --validate

# Override the base image entirely
inspectah scan --target-image registry.redhat.io/rhel10/rhel-bootc:10.2
```

---

## `inspectah refine`

Serves a inspectah output tarball over HTTP for interactive editing in the browser. Toggle individual packages, config files, and services on or off, then re-render to update the Containerfile and reports. Download the updated tarball when done.

```
inspectah refine [-h] [--no-browser] [--port PORT] TARBALL
```

| Flag | Description |
|------|-------------|
| `TARBALL` | Path to a inspectah output tarball (`.tar.gz`) -- positional argument |
| `--no-browser` | Don't auto-open the browser on startup |
| `--port PORT` | Listen port (default: 8642, falls back to next available if busy) |

### How It Works

1. Extracts the tarball to a temporary directory
2. Re-renders the report with the editor UI enabled
3. Starts an HTTP server on localhost
4. Opens the report in your default browser
5. Handles re-render requests when you toggle items and click **Re-render**
6. Serves the updated tarball for download

The re-render pipeline calls `inspectah scan --from-snapshot` under the hood, so changes to toggles are reflected in the Containerfile, audit report, and all other output artifacts.

### Examples

```bash
# Refine a single-host inspection
./run-inspectah.sh refine webserver01-20260312-143000.tar.gz

# Refine a fleet output
./run-inspectah.sh refine web-servers-fleet.tar.gz

# Use a specific port and skip auto-opening the browser
inspectah refine output.tar.gz --port 9000 --no-browser
```

---

## `inspectah fleet`

Aggregates inspection snapshots from multiple hosts into a single fleet specification. Produces a merged tarball with a combined Containerfile, reports, and fleet metadata.

```
inspectah fleet [-h] [-p PCT] [-o FILE] [--output-dir DIR] [--json-only]
             [--no-hosts] INPUT_DIR
```

| Flag | Description |
|------|-------------|
| `INPUT_DIR` | Directory containing inspectah tarballs (`.tar.gz`) and/or JSON snapshot files -- positional argument |
| `-p`, `--min-prevalence PCT` | Include items on >= N% of hosts (1--100, default: 100). Lower values include items found on fewer hosts. |
| `-o`, `--output-file FILE` | Output tarball path (default: auto-named in current directory) |
| `--output-dir DIR` | Write rendered files to a directory instead of a tarball |
| `--json-only` | Write merged JSON only, skip rendering Containerfile and reports |
| `--no-hosts` | Omit per-item host lists from fleet metadata (reduces output size for large fleets) |

### Examples

```bash
# Aggregate 3 web servers with strict intersection (only items on ALL hosts)
mkdir web-servers && cp web-0{1,2,3}.tar.gz web-servers/
./run-inspectah.sh fleet ./web-servers/

# Include items on 80%+ of hosts
./run-inspectah.sh fleet ./web-servers/ -p 80

# Output merged JSON only (no Containerfile rendering)
inspectah fleet ./web-servers/ --json-only -o web-fleet.json

# Write to a directory instead of a tarball
inspectah fleet ./web-servers/ --output-dir ./fleet-output/
```

---

## `inspectah architect`

Plans layer decomposition from multiple refined fleet outputs. Takes 2+ fleet tarballs and proposes a shared base image plus derived role-specific images. Launches an interactive web UI for exploring and adjusting the layer topology.

> **Early development:** Currently handles package list decomposition only. Config files, services, and other artifacts are not yet split across layers.

```
inspectah architect [-h] [--port PORT] [--no-browser] [--bind ADDRESS] INPUT
```

| Flag | Description |
|------|-------------|
| `INPUT` | Directory containing refined fleet tarballs (`.tar.gz`), or a tarball bundle -- positional argument |
| `--port PORT` | Port for the architect web UI (default: 8643) |
| `--no-browser` | Don't open browser automatically |
| `--bind ADDRESS` | Address to bind (default: 127.0.0.1) |

### Examples

```bash
# Plan layers from two refined fleets
mkdir refined-fleets
cp web-servers-refined.tar.gz db-servers-refined.tar.gz refined-fleets/
./run-inspectah.sh architect ./refined-fleets/

# Use a custom port
inspectah architect ./refined-fleets/ --port 9090

# Headless mode (no browser)
inspectah architect ./refined-fleets/ --no-browser
```

---

## `inspectah build`

Wraps `podman build` with automatic RHEL subscription cert handling. Solves the problem of building RHEL-based bootc images on non-RHEL hosts (Mac, Windows, Fedora) by auto-detecting and bind-mounting entitlement certs so `dnf install` works inside the build.

```bash
inspectah build TARBALL_OR_DIR -t IMAGE:TAG [flags] [-- EXTRA_PODMAN_ARGS...]
```

| Flag | Description |
|------|-------------|
| `-t, --tag` | Image name:tag (required) |
| `--platform` | Target os/arch (e.g., `linux/arm64`) |
| `--entitlements-dir` | Explicit entitlement cert directory |
| `--no-entitlements` | Skip entitlement detection entirely |
| `--ignore-expired-certs` | Proceed despite expired entitlement certs |
| `--no-cache` | Clean rebuild without layer caching |
| `--pull` | Base image pull policy (`always`, `missing`, `never`, `newer`) |
| `--dry-run` | Print the podman command without executing |
| `--verbose` | Print the podman command before executing |

Requirements: Podman installed.

### Examples

```bash
# Build from a tarball
inspectah build webserver01-20260312-143000.tar.gz -t my-bootc-image:latest

# Build from an unpacked directory
inspectah build ./inspectah-output/ -t my-bootc-image:v1.0

# Build for a different architecture
inspectah build output.tar.gz -t my-image:latest --platform linux/arm64

# Dry run (print command without executing)
inspectah build output.tar.gz -t my-image:latest --dry-run

# Rebuild without cache
inspectah build ./inspectah-output/ -t my-bootc-image --no-cache
```

---

## Environment Variables

| Variable | Effect |
|----------|--------|
| `INSPECTAH_IMAGE` | Override the container image used by `run-inspectah.sh` (e.g. a local build or pinned tag) |
| `INSPECTAH_HOSTNAME` | Override the reported hostname in inspection output |
| `INSPECTAH_DEBUG` | Set to `1` to enable debug logging |
| `INSPECTAH_OUTPUT_DIR` | Override the output directory for `run-inspectah.sh` (default: current directory) |
