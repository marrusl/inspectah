# CLI Reference

Complete flag reference for all yoinkc subcommands. For usage examples, see the [README](../../README.md).

---

## `yoinkc`

Top-level command. All functionality is accessed through subcommands.

```
yoinkc [-h] {inspect,fleet,refine,architect} ...
```

If no subcommand is given, or if the first argument looks like a flag (e.g. `--from-snapshot`), `inspect` is assumed for backwards compatibility.

| Subcommand | Description |
|------------|-------------|
| `inspect` | Inspect a host and generate migration artifacts (default) |
| `fleet` | Aggregate multiple inspection snapshots into a fleet report |
| `refine` | Interactively edit and re-render inspection output |
| `architect` | Plan layer decomposition from refined fleets |

---

## `yoinkc inspect`

Default subcommand. Inspects a host and generates migration artifacts.

```
yoinkc inspect [-h] [--host-root PATH] [-o FILE | --output-dir DIR]
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

These flags are set automatically by `yoinkc refine` during re-rendering. They are not intended for direct use.

| Flag | Description |
|------|-------------|
| `--refine-mode` | Enable editor UI in the rendered report (set by `yoinkc refine`) |
| `--original-snapshot PATH` | Path to unmodified original snapshot for editor diff/reset support (set by `yoinkc refine` during re-render) |

### Examples

```bash
# Basic host inspection (inside container via run-yoinkc.sh)
sudo ./run-yoinkc.sh

# Inspect with a specific target version
yoinkc inspect --target-version 9.6

# Re-render from a saved snapshot without re-inspecting
yoinkc inspect --from-snapshot inspection-snapshot.json -o refreshed.tar.gz

# Air-gapped: provide baseline package list manually
yoinkc inspect --baseline-packages rhel9-base-packages.txt

# Full inspection with config diffs and container enumeration
yoinkc inspect --config-diffs --query-podman

# Inspect and validate the generated Containerfile builds
yoinkc inspect --output-dir ./output --validate

# Override the base image entirely
yoinkc inspect --target-image registry.redhat.io/rhel10/rhel-bootc:10.2
```

---

## `yoinkc refine`

Serves a yoinkc output tarball over HTTP for interactive editing in the browser. Toggle individual packages, config files, and services on or off, then re-render to update the Containerfile and reports. Download the updated tarball when done.

```
yoinkc refine [-h] [--no-browser] [--port PORT] TARBALL
```

| Flag | Description |
|------|-------------|
| `TARBALL` | Path to a yoinkc output tarball (`.tar.gz`) -- positional argument |
| `--no-browser` | Don't auto-open the browser on startup |
| `--port PORT` | Listen port (default: 8642, falls back to next available if busy) |

### How It Works

1. Extracts the tarball to a temporary directory
2. Re-renders the report with the editor UI enabled
3. Starts an HTTP server on localhost
4. Opens the report in your default browser
5. Handles re-render requests when you toggle items and click **Re-render**
6. Serves the updated tarball for download

The re-render pipeline calls `yoinkc inspect --from-snapshot` under the hood, so changes to toggles are reflected in the Containerfile, audit report, and all other output artifacts.

### Examples

```bash
# Refine a single-host inspection
./run-yoinkc.sh refine webserver01-20260312-143000.tar.gz

# Refine a fleet output
./run-yoinkc.sh refine web-servers-fleet.tar.gz

# Use a specific port and skip auto-opening the browser
yoinkc refine output.tar.gz --port 9000 --no-browser
```

---

## `yoinkc fleet`

Aggregates inspection snapshots from multiple hosts into a single fleet specification. Produces a merged tarball with a combined Containerfile, reports, and fleet metadata.

```
yoinkc fleet [-h] [-p PCT] [-o FILE] [--output-dir DIR] [--json-only]
             [--no-hosts] INPUT_DIR
```

| Flag | Description |
|------|-------------|
| `INPUT_DIR` | Directory containing yoinkc tarballs (`.tar.gz`) and/or JSON snapshot files -- positional argument |
| `-p`, `--min-prevalence PCT` | Include items on >= N% of hosts (1--100, default: 100). Lower values include items found on fewer hosts. |
| `-o`, `--output-file FILE` | Output tarball path (default: auto-named in current directory) |
| `--output-dir DIR` | Write rendered files to a directory instead of a tarball |
| `--json-only` | Write merged JSON only, skip rendering Containerfile and reports |
| `--no-hosts` | Omit per-item host lists from fleet metadata (reduces output size for large fleets) |

### Examples

```bash
# Aggregate 3 web servers with strict intersection (only items on ALL hosts)
mkdir web-servers && cp web-0{1,2,3}.tar.gz web-servers/
./run-yoinkc.sh fleet ./web-servers/

# Include items on 80%+ of hosts
./run-yoinkc.sh fleet ./web-servers/ -p 80

# Output merged JSON only (no Containerfile rendering)
yoinkc fleet ./web-servers/ --json-only -o web-fleet.json

# Write to a directory instead of a tarball
yoinkc fleet ./web-servers/ --output-dir ./fleet-output/
```

---

## `yoinkc architect`

Plans layer decomposition from multiple refined fleet outputs. Takes 2+ fleet tarballs and proposes a shared base image plus derived role-specific images. Launches an interactive web UI for exploring and adjusting the layer topology.

> **Early development:** Currently handles package list decomposition only. Config files, services, and other artifacts are not yet split across layers.

```
yoinkc architect [-h] [--port PORT] [--no-browser] [--bind ADDRESS] INPUT
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
./run-yoinkc.sh architect ./refined-fleets/

# Use a custom port
yoinkc architect ./refined-fleets/ --port 9090

# Headless mode (no browser)
yoinkc architect ./refined-fleets/ --no-browser
```

---

## `yoinkc-build`

Standalone companion script (not a subcommand). Wraps `podman build` with automatic RHEL subscription cert handling. Solves the problem of building RHEL-based bootc images on non-RHEL hosts (Mac, Windows, Fedora) by auto-detecting and bind-mounting entitlement certs so `dnf install` works inside the build.

```bash
./yoinkc-build TARBALL_OR_DIR TAG [--push REGISTRY/IMAGE:TAG] [--no-cache]
```

| Argument / Flag | Description |
|-----------------|-------------|
| `TARBALL_OR_DIR` | yoinkc output tarball or unpacked directory -- positional |
| `TAG` | Image name and optional tag for the built image (default tag: `latest`) -- positional |
| `--push DEST` | Push the built image to a registry after building |
| `--no-cache` | Clean rebuild without layer caching |

Requirements: Python 3.9+ (stdlib only). Podman or Docker.

### Examples

```bash
# Build from a tarball
./yoinkc-build webserver01-20260312-143000.tar.gz my-bootc-image:latest

# Build from an unpacked directory
./yoinkc-build ./yoinkc-output/ my-bootc-image:v1.0

# Build and push to a registry
./yoinkc-build ./yoinkc-output/ my-bootc-image --push registry.example.com/my-bootc-image:v1.0

# Rebuild without cache
./yoinkc-build ./yoinkc-output/ my-bootc-image --no-cache
```

---

## Environment Variables

| Variable | Effect |
|----------|--------|
| `YOINKC_IMAGE` | Override the container image used by `run-yoinkc.sh` (e.g. a local build or pinned tag) |
| `YOINKC_HOSTNAME` | Override the reported hostname in inspection output |
| `YOINKC_DEBUG` | Set to `1` to enable debug logging |
| `YOINKC_OUTPUT_DIR` | Override the output directory for `run-yoinkc.sh` (default: current directory) |
