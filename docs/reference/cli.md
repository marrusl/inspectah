---
title: CLI Reference
parent: Reference
nav_order: 1
---

# CLI Reference

## inspectah

Inspect and prepare RHEL systems for image-mode migration.

```
inspectah [COMMAND]
```

### Commands

| Command   | Description                                          |
|-----------|------------------------------------------------------|
| `scan`    | Scan the current system and produce a migration snapshot |
| `refine`  | Interactively refine scan output and re-render       |
| `fleet`   | Aggregate and manage fleet-wide migration snapshots  |
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
| `--base-image <BASE_IMAGE>` | string | — | Target base image for cross-distro conversion |
| `--no-baseline` | bool | `false` | Skip baseline extraction (degraded classification mode) |
| `--preserve-password-hashes` | bool | `false` | Preserve password hashes for users with status `password_set` |
| `--preserve-ssh-keys` | bool | `false` | Preserve full SSH `authorized_keys` content per user |
| `--acknowledge-sensitive` | bool | `false` | Acknowledge that snapshot contains sensitive data (required for export when preserve flags used) |
| `--progress <MODE>` | enum | `rich` | Progress display mode: `rich`, `plain`, or `flat` |
| `-v, --verbose` | bool | `false` | Show sub-step detail for all inspectors, including fast ones |
| `-q, --quiet` | bool | `false` | Suppress the scan progress checklist (completion summary still prints) |

### Progress Modes

| Mode    | Description                                            |
|---------|--------------------------------------------------------|
| `rich`  | Block-redraw checklist with spinners (default for TTY) |
| `plain` | Append-only lines with Unicode symbols (durable scrollback) |
| `flat`  | Numbered sequential lines, no ANSI (CI / piped output) |

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
sudo inspectah scan --base-image registry.redhat.io/rhel9/rhel-bootc:9.6
```

Scan with sensitive data preserved (requires acknowledgment):

```bash
sudo inspectah scan --preserve-password-hashes --preserve-ssh-keys --acknowledge-sensitive
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
inspectah refine --no-open /tmp/migration-snapshot.tar.gz
```

Discard previous refinement progress and start fresh:

```bash
inspectah refine --fresh /tmp/migration-snapshot.tar.gz
```

---

## inspectah fleet

Aggregate and manage fleet-wide migration snapshots.

```
inspectah fleet <COMMAND>
```

### Subcommands

| Command     | Description                                          |
|-------------|------------------------------------------------------|
| `aggregate` | Aggregate host tarballs into a fleet tarball         |
| `init`      | Generate a fleet manifest from a directory of tarballs |

---

### inspectah fleet init

Generate a TOML fleet manifest from a directory of host tarballs.

```
inspectah fleet init [OPTIONS] <DIRECTORY>
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
inspectah fleet init /srv/snapshots/
```

Write the manifest to a specific path:

```bash
inspectah fleet init --output fleet.toml /srv/snapshots/
```

Overwrite an existing manifest:

```bash
inspectah fleet init --overwrite --output fleet.toml /srv/snapshots/
```

---

### inspectah fleet aggregate

Aggregate individual host tarballs into a single fleet tarball.

```
inspectah fleet aggregate [OPTIONS] [INPUTS]...
```

#### Arguments

| Argument      | Required | Description                                    |
|---------------|----------|------------------------------------------------|
| `[INPUTS]...` | no       | Input tarballs or directory containing tarballs |

#### Options

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--manifest <MANIFEST>` | path | — | Path to a fleet manifest (TOML) specifying sources |
| `--baseline <BASELINE>` | string | — | Override the baseline image reference |
| `--output-dir <OUTPUT_DIR>` | path | — | Output directory for the fleet tarball |
| `--output-file <OUTPUT_FILE>` | path | — | Output file path for the fleet tarball |
| `--json-only` | bool | `false` | Write JSON snapshot instead of tarball (to stdout, `--output-file`, or `--output-dir`) |
| `--strict` | bool | `false` | Treat warnings as errors |
| `-v, --verbose` | bool | `false` | Show per-host detail in output |

#### Examples

Aggregate all tarballs in a directory:

```bash
inspectah fleet aggregate /srv/snapshots/
```

Aggregate specific tarballs:

```bash
inspectah fleet aggregate host-a.tar.gz host-b.tar.gz host-c.tar.gz
```

Aggregate from a fleet manifest:

```bash
inspectah fleet aggregate --manifest fleet.toml
```

Override the baseline image during aggregation:

```bash
inspectah fleet aggregate --baseline registry.redhat.io/rhel9/rhel-bootc:9.6 /srv/snapshots/
```

Write fleet JSON to stdout:

```bash
inspectah fleet aggregate --json-only /srv/snapshots/
```

Write fleet tarball to a specific file:

```bash
inspectah fleet aggregate --output-file /tmp/fleet.tar.gz /srv/snapshots/
```

Strict mode (fail on warnings):

```bash
inspectah fleet aggregate --strict --manifest fleet.toml
```

---

## inspectah version

Print version, commit SHA, and build date.

```
inspectah version
```

No additional options beyond `--help`.
