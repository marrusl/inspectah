# CLI Reference

Complete flag reference for all yoinkc subcommands. For usage examples, see the [README](../../README.md).

## `yoinkc inspect`

Default subcommand. Inspects a host and generates migration artifacts.

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
| `--no-baseline` | Run without base image comparison — all installed packages will be included in the Containerfile |
| `--config-diffs` | Generate line-by-line diffs for modified configs via `rpm2cpio` (retrieves from local cache or downloads from repos) |
| `--deep-binary-scan` | Full `strings` scan on unknown binaries with extended version pattern matching (slow) |
| `--query-podman` | Connect to podman to enumerate running containers with full inspect data |
| `--user-strategy STRATEGY` | Override user creation strategy for all users. Valid: `sysusers`, `blueprint`, `useradd`, `kickstart` |
| `--skip-preflight` | Skip container privilege checks (rootful, `--pid=host`, `--privileged`, SELinux) |

### Output Options

| Flag | Description |
|------|-------------|
| `--validate` | After generating output, run `podman build` to verify the Containerfile. Requires `--output-dir`. |
| `--push-to-github REPO` | Push output directory to a GitHub repository (e.g. `owner/repo`). Requires `--output-dir`. |
| `--github-token TOKEN` | GitHub personal access token for repo creation (falls back to `GITHUB_TOKEN` env var) |
| `--public` | When creating a new GitHub repo, make it public (default: private) |
| `--yes` | Skip interactive confirmation prompts |

## `yoinkc refine`

Serves a yoinkc output tarball over HTTP for interactive editing.

| Flag | Description |
|------|-------------|
| `TARBALL` | Path to a yoinkc output tarball (`.tar.gz`) — positional argument |
| `--no-browser` | Don't auto-open the browser on startup |
| `--port PORT` | Listen port (default: 8642, falls back to next available) |

## `yoinkc fleet`

Aggregates inspection snapshots from multiple hosts into a single fleet specification.

| Flag | Description |
|------|-------------|
| `INPUT_DIR` | Directory containing yoinkc tarballs (`.tar.gz`) and/or JSON snapshot files — positional argument |
| `-p`, `--min-prevalence` | Include items on >= N% of hosts (1-100, default: 100) |
| `-o`, `--output-file` | Output tarball path (default: auto-named in CWD) |
| `--output-dir` | Write rendered files to a directory instead of tarball |
| `--json-only` | Write merged snapshot JSON only, skip rendering |
| `--no-hosts` | Omit per-item host lists from fleet metadata |

## `yoinkc architect`

Plans layer decomposition from multiple refined fleet outputs. Requires at least 2 fleets.

| Flag | Description |
|------|-------------|
| `INPUT` | Directory containing refined fleet tarballs (`.tar.gz`), or a tarball bundle — positional argument |
| `--port PORT` | Port for the architect web UI (default: 8643) |
| `--no-browser` | Don't open browser automatically |
| `--bind ADDRESS` | Address to bind (default: 127.0.0.1) |

## `yoinkc-build`

Standalone companion script (not a subcommand). Wraps `podman build` with automatic RHEL subscription cert handling.

```bash
./yoinkc-build TARBALL_OR_DIR TAG [--push REGISTRY/IMAGE:TAG] [--no-cache]
```

| Argument / Flag | Description |
|-----------------|-------------|
| `TARBALL_OR_DIR` | yoinkc output tarball or unpacked directory — positional |
| `TAG` | Image tag for the built image — positional |
| `--push DEST` | Push the built image to a registry |
| `--no-cache` | Clean rebuild without layer caching |

Requirements: Python 3.9+ (stdlib only). Podman or Docker.
