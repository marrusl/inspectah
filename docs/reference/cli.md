---
title: CLI Reference
parent: Reference
nav_order: 1
---

# CLI Reference

## inspectah

Inspect and prepare RPM-based Linux systems for bootc image-mode migration.

```
inspectah [COMMAND]
```

### Commands

| Command   | Description                                          |
|-----------|------------------------------------------------------|
| `scan`    | Scan the current system and produce a migration snapshot |
| `build`   | Build a bootc container image from an inspectah tarball |
| `refine`  | Interactively refine scan output and re-render       |
| `aggregate`   | Aggregate and manage aggregate-wide migration snapshots  |
| `version` | Print version, commit, and build date                |
| `help`    | Print help for a command                             |

### Global Options

| Flag          | Description    |
|---------------|----------------|
| `-h, --help`  | Print help     |
| `-V, --version` | Print version |

---

## inspectah scan

Scan the current system and produce a migration snapshot.

```
inspectah scan [OPTIONS]
```

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--inspect-only` | bool | `false` | Write JSON snapshot only, skip tarball/artifact generation |
| `-o, --output <OUTPUT>` | path | — | Output file path (tarball) or directory (with `--inspect-only`) |
| `--base-image <BASE_IMAGE>` | string | — | Target base image for version upgrades or cross-distro conversion (e.g., upgrade from RHEL 9.4 to 9.6, or convert from CentOS to RHEL) |
| `--preserve <ITEM>` | string | — | Preserve sensitive data (password-hashes, ssh-keys, subscription, all). Comma-separated, repeatable |
| `--no-redaction` | bool | `false` | Skip redaction pipeline, retaining raw secrets (requires --ack-sensitive) |
| `--ack-sensitive` | bool | `false` | Acknowledge sensitive data in the snapshot (required with --preserve or --no-redaction). Alias: `--acknowledge-sensitive` |
| `--progress <MODE>` | enum | `(auto)` | Progress display mode: `pretty` or `flat`. Auto-detected: TTY → `pretty`, non-TTY/CI → `flat`. Override with `INSPECTAH_PROGRESS` env var. |
| `-v, --verbose` | bool | `false` | Show sub-step detail for all inspectors (works with both pretty and flat modes) |
| `-q, --quiet` | bool | `false` | Suppress the scan progress checklist (completion summary still prints) |

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

Write JSON snapshot only, no tarball:

```bash
sudo inspectah scan --inspect-only -o /tmp/snapshot/
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

---

## inspectah refine

Launch the interactive refinement UI to review and adjust scan output.

```
inspectah refine [OPTIONS] <TARBALL>
```

### Arguments

| Argument    | Required | Description                          |
|-------------|----------|--------------------------------------|
| `<TARBALL>` | yes      | Path to scan output tarball (`.tar.gz`) |

### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--port <PORT>` | integer | `8642` | Port to bind (use `0` for ephemeral) |
| `--open <OPEN>` | bool | `true` | Open browser automatically (use `--no-open` to suppress) |
| `--fresh` | bool | `false` | Start a fresh session, discarding any saved progress |
| `--tui` | bool | `false` | Use terminal UI instead of web browser |

### Examples

Open the refinement UI for a scan snapshot:

```bash
inspectah refine /tmp/migration-snapshot.tar.gz
```

Use the terminal UI instead of a web browser:

```bash
inspectah refine --tui /tmp/migration-snapshot.tar.gz
```

Bind to a custom port:

```bash
inspectah refine --port 9000 /tmp/migration-snapshot.tar.gz
```

Start without opening a browser (headless/remote use):

```bash
inspectah refine --no-open /tmp/migration-snapshot.tar.gz
```

Discard previous refinement progress and start fresh:

```bash
inspectah refine --fresh /tmp/migration-snapshot.tar.gz
```

---

## inspectah aggregate

Aggregate and manage aggregate-wide migration snapshots.

```
inspectah aggregate <COMMAND>
```

### Subcommands

| Command     | Description                                          |
|-------------|------------------------------------------------------|
| `aggregate` | Aggregate host tarballs into a aggregate tarball         |
| `init`      | Generate a aggregate manifest from a directory of tarballs |

---

### inspectah aggregate init

Generate a TOML aggregate manifest from a directory of host tarballs.

```
inspectah aggregate init [OPTIONS] <DIRECTORY>
```

#### Arguments

| Argument      | Required | Description                        |
|---------------|----------|------------------------------------|
| `<DIRECTORY>` | yes      | Directory containing host tarballs |

#### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--output <OUTPUT>` | path | — | Output path for the generated manifest |
| `--overwrite` | bool | `false` | Overwrite an existing manifest file |

#### Examples

Generate a manifest from a directory of tarballs:

```bash
inspectah aggregate init /srv/snapshots/
```

Write the manifest to a specific path:

```bash
inspectah aggregate init --output aggregate.toml /srv/snapshots/
```

Overwrite an existing manifest:

```bash
inspectah aggregate init --overwrite --output aggregate.toml /srv/snapshots/
```

---

### inspectah aggregate

Aggregate individual host tarballs into a single aggregate tarball.

```
inspectah aggregate [OPTIONS] [INPUTS]...
```

#### Arguments

| Argument      | Required | Description                                    |
|---------------|----------|------------------------------------------------|
| `[INPUTS]...` | no       | Input tarballs or directory containing tarballs |

#### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--manifest <MANIFEST>` | path | — | Path to a aggregate manifest (TOML) specifying sources |
| `--target-image <IMAGE>` | string | — | Override the target image reference for baseline comparison |
| `--output-dir <OUTPUT_DIR>` | path | — | Output directory for the aggregate tarball |
| `--output-file <OUTPUT_FILE>` | path | — | Output file path for the aggregate tarball |
| `--json-only` | bool | `false` | Write JSON snapshot instead of tarball (to stdout, `--output-file`, or `--output-dir`) |
| `--strict` | bool | `false` | Treat warnings as errors |
| `-v, --verbose` | bool | `false` | Show per-host detail in output |
| `--ack-sensitive` | bool | `false` | Acknowledge that the merged output may contain sensitive data (subscription certs, password hashes, SSH keys). Required when any contributing snapshot has `sensitive_snapshot` set. Alias: `--acknowledge-sensitive` |

#### Examples

Aggregate all tarballs in a directory:

```bash
inspectah aggregate /srv/snapshots/
```

Aggregate specific tarballs:

```bash
inspectah aggregate host-a.tar.gz host-b.tar.gz host-c.tar.gz
```

Aggregate from a aggregate manifest:

```bash
inspectah aggregate --manifest aggregate.toml
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
inspectah aggregate --strict --manifest aggregate.toml
```

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
inspectah build [OPTIONS] <TARBALL> --tag <TAG>
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
| `[-- <PODMAN_ARGS>...]` | string | — | Additional arguments to pass to `podman build` (after `--`) |

### Behavior

1. **Extract** -- extracts the tarball to a temporary directory (or a named
   cache directory when `--keep-context` is set).
2. **Validate** -- confirms the tarball contains a `Containerfile` and checks
   archive safety (rejects path traversal, symlink escapes, hardlinks, and
   device nodes).
3. **Detect RHEL pass-through** -- if building on a subscribed RHEL host,
   uses the host's ambient subscription. If not (macOS, Fedora, CI), falls
   back to subscription material from the tarball's `subscription/` directory.
4. **Check certificate expiry** -- warns when entitlement certificates are
   expired or expiring within 14 days.
5. **Build** -- constructs and runs the `podman build` command with
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
inspectah version
```

No additional options beyond `--help`.
