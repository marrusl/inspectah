# inspectah

inspectah scans a running RHEL, CentOS Stream, or Fedora host and generates everything you need to rebuild it as a [bootc](https://containers.github.io/bootc/) container image. bootc images are full operating system images managed and deployed as OCI containers — update your OS the same way you update your apps, with atomic upgrades and built-in rollback. inspectah figures out what you added to the base OS — packages, configs, services, users, cron jobs, container workloads — and generates only the delta. The output is a ready-to-build Containerfile, a config tree, an audit report, and an HTML audit report.

> **Status:** inspectah is an active prototype. It handles common RHEL 9, CentOS Stream, and Fedora configurations well, but expect rough edges on unusual setups. It targets RPM-based systems only — no Debian, no RHEL 7, no live/in-place migration.

## Quick Start

```bash
# 1. Install inspectah
sudo dnf copr enable mrussell/inspectah
sudo dnf install inspectah
```

```bash
# 2. Scan your system (run on the host you want to migrate)
inspectah scan
```

```bash
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

### Prerequisites

- **Root access** — `inspectah scan` requires root privileges
- **Podman** — installed and available (`sudo dnf install podman`)
- **Target base image** — inspectah must be able to pull your target container image.
  For disconnected or air-gapped environments:
  - Pull the image on a connected machine: `podman save -o baseline.tar <image-ref>`
  - Transfer the tarball to the target host
  - Load it: `podman load -i baseline.tar`
  - Alternatively, use a local or mirror registry

### macOS (Homebrew)

```bash
brew tap marrusl/inspectah
brew install inspectah
```

Requires macOS on Apple Silicon (arm64). For Intel Macs, build from source.

### From source

```bash
cargo build --release
sudo install target/release/inspectah /usr/local/bin/
```

Requires Rust toolchain (1.70+).

## What It Does

`inspectah scan` does three things in one pass:

1. **Scan** — Snapshot everything on the running system: packages, configs, services, users, containers
2. **Classify** — Compare against the target base image and classify each finding as baseline (already in the image), user-added, or modified
3. **Render** — Generate migration artifacts from the classified findings: Containerfile, config tree, audit report, secrets review

The baseline comparison is critical: inspectah pulls the target base image to determine what's already there, so it only includes the delta in your migration artifacts.

From there, refine and build:

4. **Refine** (optional) — Open an interactive browser dashboard to toggle items on/off, override classifications, and re-render artifacts with your changes
5. **Build** — Build a bootc container image from the artifacts with `inspectah build <tarball> <tag>` (runs `podman build` under the hood), or extract the tarball and build manually

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
| `aggregate` | Aggregate and manage aggregate-wide migration snapshots |
| `build` | Build a bootc container image from an inspectah tarball |
| `version` | Print version, commit, and build date |

For full command-line reference, see the [CLI documentation](https://marrusl.github.io/inspectah/reference/cli.html).

### Scan Options

Common flags for `inspectah scan`:

- `--base-image <IMAGE>` — Target base image for cross-distro conversion (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`)
- `--preserve <ITEM>` — Preserve sensitive data (password-hashes, ssh-keys, subscription, all). Comma-separated, repeatable
- `--no-redaction` — Skip the redaction pipeline, retaining raw secrets
- `--ack-sensitive` — Acknowledge sensitive data in snapshot (required with --preserve or --no-redaction). Alias: `--acknowledge-sensitive`
- `--progress <MODE>` — Progress display: `pretty` (default TTY), `flat` (CI/non-TTY)
- `-o, --output <PATH>` — Output file path (tarball) or directory
- `-v, --verbose` — Show sub-step detail for all inspectors (works with both pretty and flat modes)
- `-q, --quiet` — Suppress the scan progress checklist

Run `inspectah scan --help` for the full list.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0    | Success — scan completed, report is trustworthy |
| 1    | General error (invalid arguments, missing permissions, etc.) |
| 2    | Incomplete scan — one or more inspectors failed, report has blind spots |
| 3    | Baseline pull failure — could not pull the target container image |
| 130  | Interrupted — scan was cancelled by the user (SIGINT / Ctrl-C) |

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
- **Export** — regenerate the Containerfile, audit report, and all output artifacts with your changes applied, then download the updated tarball. Refining alone doesn't produce buildable artifacts with your changes — you must export to render final output and package it.

Refine works on both single-host inspection tarballs and aggregated tarballs.

#### Terminal UI (experimental)

For terminal-based editing, use the `--tui` flag:

```bash
inspectah refine --tui hostname-*.tar.gz
```

The TUI provides keyboard-driven navigation and inline item toggling without leaving the terminal. This is an experimental feature — the browser-based workflow is recommended for most users.

### Aggregation

For managing multiple hosts, point `inspectah aggregate` at a directory of scan tarballs:

```bash
inspectah aggregate ./scans/
```

```bash
inspectah refine aggregate-*.tar.gz
```

This finds the intersection of packages and configs across hosts and identifies per-host exceptions. The aggregated tarball works with `refine` and `build` the same way a single-host tarball does.

You can also list specific tarballs instead of a directory: `inspectah aggregate host-a.tar.gz host-b.tar.gz`.

## Workflows

```
One host:    Scan ───► Refine ───► Build
Many hosts:  Scan ───► Aggregate ────► Refine ───► Build
```

Each step consumes and produces tarballs. Refine and Aggregate are optional.

## Configuration

inspectah does not use a configuration file. Behavior is controlled through CLI flags and environment variables:

| Variable | Effect |
|----------|--------|
| `INSPECTAH_PROGRESS` | Override progress display mode (`pretty`, `flat`). Takes effect when no `--progress` CLI flag is provided. |
| `NO_COLOR` | Disable ANSI color output (follows [no-color.org](https://no-color.org/) convention). |

## Documentation

Full documentation is available at [marrusl.github.io/inspectah](https://marrusl.github.io/inspectah/):

- [Tutorials](https://marrusl.github.io/inspectah/tutorials/) — step-by-step guides for common tasks
- [How-to Guides](https://marrusl.github.io/inspectah/how-to/) — recipes for specific scenarios
- [Reference](https://marrusl.github.io/inspectah/reference/) — CLI commands, inspector catalog, output format
- [Explanation](https://marrusl.github.io/inspectah/explanation/) — concepts, architecture, design decisions

## License

[MIT](LICENSE)
