# inspectah

inspectah scans a running RHEL, CentOS Stream, or Fedora host and generates everything you need to rebuild it as a [bootc](https://containers.github.io/bootc/) container image. bootc images are full operating system images managed and deployed as OCI containers — update your OS the same way you update your apps, with atomic upgrades and built-in rollback. inspectah figures out what you added to the base OS — packages, configs, services, users, cron jobs, container workloads — and generates only the delta. The output is a ready-to-build Containerfile, a config tree, an audit report, and an HTML audit report.

> **Status:** inspectah is an active prototype. It handles common RHEL 9, CentOS Stream, and Fedora configurations well, but expect rough edges on unusual setups. It targets RPM-based systems only — no Debian, no RHEL 7, no live/in-place migration.

## Quick Start

```bash
# 1. Install inspectah
sudo dnf copr enable mrussell/inspectah
sudo dnf install inspectah

# 2. Scan your system (run on the host you want to migrate)
inspectah scan

# 3. Review the output
ls -lh *.tar.gz
tar tzf hostname-*.tar.gz | head -20
```

The tarball contains a `Containerfile`, `config/` tree, `audit-report.md`, and an HTML audit report. Run `inspectah build <tarball> <tag>` to build the image, or extract and use `podman build` directly.

## Installation

### RPM (Fedora / RHEL / CentOS Stream)

```bash
sudo dnf copr enable mrussell/inspectah
sudo dnf install inspectah
```

Requires podman >= 4.4 (installed as a dependency if not present).

### From source

```bash
cargo build --release
sudo install target/release/inspectah /usr/local/bin/
```

Requires Rust toolchain (1.70+).

## What It Does

inspectah follows a four-step workflow:

1. **Scan** — Extract the delta between the base OS and your running system (packages, configs, services, users, containers)
2. **Inspect** — Classify findings as baseline (already in base image), user-added, or modified
3. **Triage** — Generate migration artifacts: Containerfile, config tree, audit report, secrets review
4. **Refine** (optional) — Edit findings in an interactive browser dashboard and re-render artifacts

inspectah can orchestrate the build via `inspectah build <tarball> <tag>` (runs `podman build` under the hood) or you can extract the tarball and build manually.

The baseline comparison is critical: inspectah extracts the target base image to determine what's already there, so it only includes the delta in your migration artifacts. Without baseline extraction (e.g., `--no-baseline`), all packages and configs are assumed user-added.

## Output

The default output is a tarball (`hostname-YYYYMMDD-HHMMSS.tar.gz`) containing:

```
hostname-20260312-143000.tar.gz
└── hostname-20260312-143000/
    ├── Containerfile                 # Layered image definition (cache-optimized)
    ├── README.md                     # Build/deploy commands, FIXME checklist
    ├── audit-report.md               # Detailed findings, storage plan, version drift
    ├── audit-report.html             # Self-contained HTML audit report
    ├── secrets-review.md             # Redacted sensitive content for review
    ├── inspection-snapshot.json      # Raw structured data (re-renderable)
    ├── config/                       # Files to COPY into the image
    │   ├── etc/                      # Modified configs, repos, firewall, timers
    │   ├── opt/                      # Non-RPM software (venvs, npm apps, binaries)
    │   └── usr/                      # Files under /usr/local
    ├── kickstart-suggestion.ks       # Suggested deploy-time settings (hostname, networking)
    ├── quadlet/                      # Container workload unit files (conditional)
    ├── inspectah-users.toml          # bootc-image-builder user config
    └── subscription/                 # RHEL subscription material (conditional)
        ├── entitlement/              # Cert/key pairs
        ├── rhsm/                     # CA certs + rhsm.conf
        └── redhat.repo               # Red Hat repo definition
```

Use `--output <dir>` to get an unpacked directory instead of a tarball.

## Commands

| Command | Description |
|---------|-------------|
| `scan` | Scan the current system and produce a migration snapshot |
| `refine` | Interactively refine scan output and re-render artifacts |
| `fleet` | Aggregate and manage fleet-wide migration snapshots |
| `build` | Build a bootc container image from an inspectah tarball |
| `version` | Print version, commit, and build date |

For full command-line reference, see the [CLI documentation](https://marrusl.github.io/inspectah/reference/cli.html).

### Scan Options

Common flags for `inspectah scan`:

- `--base-image <IMAGE>` — Target base image for cross-distro conversion (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`)
- `--no-baseline` — Skip baseline extraction (degraded classification mode, faster but less accurate)
- `--preserve <ITEM>` — Preserve sensitive data (password-hashes, ssh-keys, subscription, all). Comma-separated, repeatable
- `--no-redaction` — Skip the redaction pipeline, retaining raw secrets
- `--ack-sensitive` — Acknowledge sensitive data in snapshot (required with --preserve or --no-redaction). Alias: `--acknowledge-sensitive`
- `--progress <MODE>` — Progress display: `rich` (default TTY), `plain` (durable scrollback), `flat` (CI/non-TTY)
- `-o, --output <PATH>` — Output file path (tarball) or directory
- `-v, --verbose` — Show sub-step detail for all inspectors
- `-q, --quiet` — Suppress the scan progress checklist

Run `inspectah scan --help` for the full list.

### Refine

After scanning, copy the tarball to your workstation and launch the interactive editor:

```bash
scp target-host:~/hostname-*.tar.gz .
inspectah refine hostname-*.tar.gz
```

The browser opens automatically with the Refine dashboard. From here you can:

- **Toggle items on/off** — exclude packages, config files, or services you don't want in the migration image
- **Search and filter** — use the search box on each card to find specific packages, files, or services
- **Review classifications** — inspectah auto-classifies items; refine lets you override
- **Export** — regenerate the Containerfile, audit report, and all output artifacts with your changes applied, then download the updated tarball. Refining alone doesn't produce buildable artifacts — you must export to render final artifacts and package them.

Refine works on both single-host inspection tarballs and fleet-aggregated tarballs.

#### Terminal UI (experimental)

For terminal-based editing, use the `--tui` flag:

```bash
inspectah refine --tui hostname-*.tar.gz
```

The TUI provides keyboard-driven navigation and inline item toggling without leaving the terminal. This is an experimental feature — the browser-based workflow is recommended for most users.

### Fleet Aggregation

For managing multiple hosts, use `inspectah fleet`:

```bash
# Generate a fleet manifest from a directory of tarballs
inspectah fleet init ./scans/

# Aggregate the fleet into a single fleet tarball
inspectah fleet aggregate --manifest fleet.toml

# Refine the aggregated output
inspectah refine fleet-*.tar.gz
```

Fleet mode finds the intersection of packages/configs across hosts and identifies per-host exceptions.

## Workflows

```
One host:    Scan ───► Refine ───► Build
Many hosts:  Scan ───► Fleet ────► Refine ───► Build
```

Each step consumes and produces tarballs. Refine and Fleet are optional.

## Configuration

inspectah does not use a configuration file. Behavior is controlled through CLI flags and environment variables:

| Variable | Effect |
|----------|--------|
| `INSPECTAH_PROGRESS` | Override progress display mode (`rich`, `plain`, `flat`). Takes effect when no `--progress` CLI flag is provided. |
| `NO_COLOR` | Disable ANSI color output (follows [no-color.org](https://no-color.org/) convention). |

## Documentation

Full documentation is available at [marrusl.github.io/inspectah](https://marrusl.github.io/inspectah/):

- [Tutorials](https://marrusl.github.io/inspectah/tutorials/) — step-by-step guides for common tasks
- [How-to Guides](https://marrusl.github.io/inspectah/how-to/) — recipes for specific scenarios
- [Reference](https://marrusl.github.io/inspectah/reference/) — CLI commands, inspector catalog, output format
- [Explanation](https://marrusl.github.io/inspectah/explanation/) — concepts, architecture, design decisions

## License

[MIT](LICENSE)
