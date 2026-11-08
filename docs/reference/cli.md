---
title: CLI Reference
parent: Reference
nav_order: 1
---

# CLI Reference

## inspectah

Inspect and prepare RPM-based Linux systems for bootc image-mode migration.

```
Usage: inspectah [OPTIONS] [COMMAND]
```

### Commands

| Command   | Description                                          |
|-----------|------------------------------------------------------|
| `scan`    | Scan the current system and produce a migration snapshot |
| `build`   | Build a bootc container image from an inspectah tarball |
| `refine`  | Interactively refine scan output and re-render       |
| `aggregate`   | Combine multiple host scan tarballs into an aggregate snapshot |
| `version` | Print version, commit, and build date                |
| `help`    | Print this message or the help of the given subcommand(s) |

### Global Options

| Flag          | Description    |
|---------------|----------------|
| `-y, --yes`   | Assume yes to all interactive prompts (for CI/automation) |
| `-h, --help`  | Print help     |
| `-V, --version` | Print version |

`-y, --yes` is accepted before or after the subcommand name, and on every subcommand: `inspectah -y scan` and `inspectah scan -y` are equivalent.

---

## inspectah scan

Scan the current system and produce a migration snapshot.

```
Usage: inspectah scan [OPTIONS]
```

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--inspect-only` | bool | `false` | Write JSON snapshot only, skip tarball/artifact generation |
| `-o, --output <OUTPUT>` | path | none | Output file path (tarball). With `--inspect-only`, the path of the JSON file; a directory path is not accepted today (see the example below) |
| `--base-image <BASE_IMAGE>` | string | none | Target base image for cross-distro conversion (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`) |
| `--preserve <ITEM>` | enum | none | Preserve sensitive data in the snapshot: `password-hashes`, `ssh-keys`, `subscription`, or `all`. Comma-separated, repeatable |
| `--no-redaction` | bool | `false` | Skip the redaction phase; secrets remain unmasked in output (requires `--ack-sensitive`) |
| `--ack-sensitive` | bool | `false` | Acknowledge sensitive data in the snapshot (required with `--preserve` or `--no-redaction`). Alias: `--acknowledge-sensitive` |
| `--progress <MODE>` | enum | `(auto)` | Progress display mode: `pretty` or `flat`. Auto-detected: TTY → `pretty`, non-TTY/CI → `flat`. Override with `INSPECTAH_PROGRESS` env var. |
| `-v, --verbose` | bool | `false` | Show sub-step detail for all inspectors, including fast ones (works with both pretty and flat modes). Conflicts with `-q` |
| `-q, --quiet` | bool | `false` | Suppress the scan progress checklist (completion summary still prints). Conflicts with `-v` |
| `--include-unmanaged` | bool | `false` | Catalog and bundle unmanaged files from `/opt`, `/srv`, `/usr/local`. Prompts with total size before bundling (suppressed by `-y`/`--yes`). |
| `--exclude-path <PATH>` | string | none | Exclude specific paths from unmanaged file collection. Repeatable. |
| `--scan-home <all\|USER,...>` | string | none | Scan home directories for non-RPM software. `all` selects users with UID 1000 through 59999; otherwise a comma-separated list of usernames. See [Scan roots](#scan-roots-for-non-rpm-software) |
| `--scan-path <PATH>` | string | none | Add extra scan paths for non-RPM software detection. Repeatable. See [Scan roots](#scan-roots-for-non-rpm-software) |

### Scan roots for non-RPM software

The non-RPM software inspector looks in `/opt`, `/srv`, `/usr/local`, and `/var/www` by default. `--scan-home` and `--scan-path` add to that list; neither removes a default root.

- `--scan-home all` adds the home directory of every user with UID 1000 through 59999, the same range the users inspector treats as regular users, so `nobody` (UID 65534, home `/`) is not selected. `all` covers users in the regular range only: a user whose UID is outside it, including a directory-service user (SSSD or IdM) mapped above 59999, is not selected and must be named with `--scan-home <user>`. A home directory of `/` is never added, even for a named user. `--scan-home alice,bob` adds the home directories of the named users. A named user is included whatever its UID, and a name that does not resolve is skipped with a warning.
- `--scan-path` takes an absolute path. A relative path, or a path that does not exist, is skipped with a warning. A path with fewer than two components (such as `/var`) is accepted with a warning that it is very broad.


### Progress Modes

| Mode     | Description                                                |
|----------|-----------------------------------------------------------|
| `pretty` | Append-only receipt with Unicode symbols (default for TTY) |
| `flat`   | Numbered sequential lines, no ANSI (CI / piped output)     |

### Examples

Scan the local system and write the default tarball:

```bash
sudo inspectah scan
```

Scan and write output to a specific path:

```bash
sudo inspectah scan -o /tmp/migration-snapshot.tar.gz
```

Write JSON snapshot only, no tarball. Give `-o` the path of the JSON file; a directory path fails with `Is a directory`:

```bash
sudo inspectah scan --inspect-only -o ./snapshot/inspection-snapshot.json
```

Scan against a specific base image:

```bash
sudo inspectah scan --base-image quay.io/centos-bootc/centos-bootc:stream9
```

Scan with sensitive data preserved (requires acknowledgment):

```bash
sudo inspectah scan --preserve password-hashes --preserve ssh-keys --ack-sensitive
```

Scan with RHEL subscription material preserved (for building on non-RHEL hosts):

```bash
sudo inspectah scan --preserve subscription --ack-sensitive
```

Scan in CI with flat progress output:

```bash
sudo inspectah scan --progress flat -o snapshot.tar.gz
```

Scan with verbose sub-step output:

```bash
sudo inspectah scan -v
```

Scan with unmanaged file collection enabled:

```bash
sudo inspectah scan --include-unmanaged
```

Scan with unmanaged files, excluding specific paths:

```bash
sudo inspectah scan --include-unmanaged --exclude-path /opt/backups --exclude-path /srv/archive
```

Scan an extra directory for non-RPM software:

```bash
sudo inspectah scan --scan-path /apps/reporting
```

Scan the home directories of named users. The usernames are placeholders; this example is illustrative and the reader substitutes users that exist on the host:

```bash
sudo inspectah scan --scan-home alice,bob
```

Scan in CI with no interactive prompts:

```bash
sudo inspectah -y scan --include-unmanaged -o snapshot.tar.gz
```

---

## inspectah refine

Launch the interactive refinement UI to review and adjust scan output.

```
Usage: inspectah refine [OPTIONS] <TARBALL>
```

### Arguments

| Argument    | Required | Description                          |
|-------------|----------|--------------------------------------|
| `<TARBALL>` | yes      | Path to scan output tarball (`.tar.gz`) |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--port <PORT>` | integer | `8642` | Port to bind (use `0` for ephemeral) |
| `--open <OPEN>` | `true` or `false` | `true` | Open the browser automatically. The flag takes a value: pass `--open false` or `--open=false` to suppress |
| `--fresh` | bool | `false` | Start a fresh session, discarding any saved progress |

### Examples

Open the refinement UI for a scan snapshot:

```bash
inspectah refine /tmp/migration-snapshot.tar.gz
```

Bind to a custom port:

```bash
inspectah refine --port 9000 /tmp/migration-snapshot.tar.gz
```

Start without opening a browser (headless/remote use):

```bash
inspectah refine --open false /tmp/migration-snapshot.tar.gz
```

Discard previous refinement progress and start fresh:

```bash
inspectah refine --fresh /tmp/migration-snapshot.tar.gz
```

---

## inspectah aggregate

Combine multiple host scan tarballs into a single aggregate snapshot.

```
Usage: inspectah aggregate [OPTIONS] [INPUTS]...
```

### Arguments

| Argument      | Required | Description                                    |
|---------------|----------|------------------------------------------------|
| `[INPUTS]...` | yes      | Input tarballs or directory containing tarballs |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--target-image <IMAGE>` | string | none | Override the target image reference for baseline comparison |
| `--output-dir <OUTPUT_DIR>` | path | none | Output directory for the aggregate tarball |
| `--output-file <OUTPUT_FILE>` | path | none | Output file path for the aggregate tarball |
| `--json-only` | bool | `false` | Write JSON snapshot instead of tarball (to stdout, `--output-file`, or `--output-dir`) |
| `--strict` | bool | `false` | Treat warnings as errors |
| `-v, --verbose` | bool | `false` | Show per-host detail in output |
| `--ack-sensitive` | bool | `false` | Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys). Required when any contributing snapshot has `sensitive_snapshot` set. Alias: `--acknowledge-sensitive` |

### Examples

The `aggregate` examples use placeholder paths: `/srv/snapshots/` stands for a directory of scan tarballs, and `host-a.tar.gz` for a tarball from one host. They are illustrative; substitute your own paths.

Aggregate all tarballs in a directory:

```bash
inspectah aggregate /srv/snapshots/
```

Aggregate specific tarballs:

```bash
inspectah aggregate host-a.tar.gz host-b.tar.gz host-c.tar.gz
```

Override the target image during aggregation:

```bash
inspectah aggregate --target-image quay.io/centos-bootc/centos-bootc:stream9 /srv/snapshots/
```

Write aggregate JSON to stdout:

```bash
inspectah aggregate --json-only /srv/snapshots/
```

Write aggregate tarball to a specific file:

```bash
inspectah aggregate --output-file /tmp/aggregate.tar.gz /srv/snapshots/
```

Strict mode (fail on warnings):

```bash
inspectah aggregate --strict /srv/snapshots/
```

With `--strict`, any warning aborts the run with exit code 1. Aggregating scans whose timestamps differ prints a `stale scan dates` warning, so `--strict` fails on any set of scans taken at different times.

Aggregate snapshots that contain sensitive data (subscription certs, password hashes):

```bash
inspectah aggregate --ack-sensitive /srv/snapshots/
```

---

## inspectah build

Build a bootc container image from an inspectah tarball snapshot. Extracts
the tarball, validates its contents, plans the build (including RHEL
subscription certificate mounts when needed), and executes `podman build`.

```
Usage: inspectah build [OPTIONS] --tag <TAG> <TARBALL> [-- <PODMAN_ARGS>...]
```

### Arguments

| Argument    | Required | Description                                 |
|-------------|----------|---------------------------------------------|
| `<TARBALL>` | yes      | Path to inspectah tarball (`.tar.gz` snapshot) |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `-t, --tag <TAG>` | string | **required** | Image tag (must include version, e.g., `myimage:v1`) |
| `--dry-run` | bool | `false` | Show the build command without executing it |
| `--keep-context` | bool | `false` | Keep the extracted build context after build completes |
| `[-- <PODMAN_ARGS>...]` | string | none | Additional arguments to pass to `podman build` (after `--`) |

### Behavior

1. **Extract**: extracts the tarball to a temporary directory (or a named
   cache directory when `--keep-context` is set).
2. **Validate**: confirms the tarball contains a `Containerfile` and checks
   archive safety (rejects path traversal, symlink escapes, hardlinks, and
   device nodes).
3. **Detect RHEL pass-through**: if building on a subscribed RHEL host,
   uses the host's ambient subscription. If not (macOS, Fedora, CI), falls
   back to subscription material from the tarball's `subscription/` directory.
4. **Check certificate expiry**: warns when entitlement certificates are
   expired or expiring within 14 days.
5. **Build**: constructs and runs the `podman build` command with
   subscription certificate mounts (`-v`) when applicable.

### Examples

Build an image from a scan tarball:

```bash
inspectah build snapshot.tar.gz --tag my-bootc-image:v1
```

Preview the podman command without executing it:

```bash
inspectah build snapshot.tar.gz --tag my-bootc-image:v1 --dry-run
```

Keep the extracted build context for inspection after the build:

```bash
inspectah build snapshot.tar.gz --tag my-bootc-image:v1 --keep-context
```

Pass additional flags to podman:

```bash
inspectah build snapshot.tar.gz --tag my-bootc-image:v1 -- --no-cache --platform linux/arm64
```

---

## inspectah version

Print version, commit SHA, and build date.

```
Usage: inspectah version [OPTIONS]
```

No options beyond `-y, --yes` and `--help`.
