# inspectah

inspectah scans a running RHEL, CentOS Stream, or Fedora host and generates everything you need to rebuild it as a [bootc](https://containers.github.io/bootc/) container image. bootc images are full operating system images managed and deployed as OCI containers — update your OS the same way you update your apps, with atomic upgrades and built-in rollback. inspectah figures out what you added to the base OS — packages, configs, services, users, cron jobs, container workloads — and generates only the delta. The output is a ready-to-build Containerfile, a config tree, an audit report, and an interactive HTML dashboard.

> **Status:** inspectah is an active prototype. It handles common RHEL 9, CentOS Stream, and Fedora configurations well, but expect rough edges on unusual setups. It targets RPM-based systems only — no Debian, no RHEL 7, no live/in-place migration.

## Quick Start

```bash
# 1. Install inspectah
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah

# 2. Scan your system (run on the host you want to migrate)
inspectah scan

# 3. Review the output
ls -lh *.tar.gz
tar tzf hostname-*.tar.gz | head -20
```

The tarball contains a `Containerfile`, `config/` tree, `audit-report.md`, and an interactive HTML dashboard. Extract it and build the image with `podman build`.

## Installation

### RPM (Fedora / RHEL / CentOS Stream)

```bash
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah
```

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

**inspectah does NOT build images** — it generates the artifacts you need to build one yourself. After running `inspectah scan`, you get a tarball with a `Containerfile` and `config/` tree. Extract it and run `podman build` to create the bootc image.

The baseline comparison is critical: inspectah extracts the target base image to determine what's already there, so it only includes the delta in your migration artifacts. Without baseline extraction (e.g., `--no-baseline`), all packages and configs are assumed user-added.

## Output

The default output is a tarball (`hostname-YYYYMMDD-HHMMSS.tar.gz`) containing:

```
hostname-20260312-143000.tar.gz
└── hostname-20260312-143000/
    ├── Containerfile                 # Layered image definition (cache-optimized)
    ├── README.md                     # Build/deploy commands, FIXME checklist
    ├── audit-report.md               # Detailed findings, storage plan, version drift
    ├── report.html                   # Self-contained interactive HTML dashboard
    ├── secrets-review.md             # Redacted sensitive content for review
    ├── inspection-snapshot.json      # Raw structured data (re-renderable)
    ├── config/                       # Files to COPY into the image
    │   ├── etc/                      # Modified configs, repos, firewall, timers
    │   ├── opt/                      # Non-RPM software (venvs, npm apps, binaries)
    │   └── usr/                      # Files under /usr/local
    ├── quadlet/                      # Container workload unit files (conditional)
    ├── inspectah-users.toml          # bootc-image-builder user config (conditional)
    ├── entitlement/                  # RHEL subscription certs (conditional)
    └── rhsm/                         # RHEL subscription manager config (conditional)
```

Use `--output <dir>` to get an unpacked directory instead of a tarball.

## Commands

| Command | Description |
|---------|-------------|
| `scan` | Scan the current system and produce a migration snapshot |
| `refine` | Interactively refine scan output and re-render artifacts |
| `fleet` | Aggregate and manage fleet-wide migration snapshots |
| `version` | Print version, commit, and build date |

For full command-line reference, see the [CLI documentation](https://marrusl.github.io/inspectah/reference/cli.html).

### Scan Options

Common flags for `inspectah scan`:

- `--base-image <IMAGE>` — Target base image for cross-distro conversion (e.g., `registry.redhat.io/rhel9/rhel-bootc:9.6`)
- `--no-baseline` — Skip baseline extraction (degraded classification mode, faster but less accurate)
- `--preserve-password-hashes` — Preserve password hashes for users with status `password_set`
- `--preserve-ssh-keys` — Preserve full SSH `authorized_keys` content per user
- `--acknowledge-sensitive` — Required when using `--preserve-*` flags (acknowledges snapshot contains sensitive data)
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
- **Re-render** — regenerate the Containerfile, audit report, and all output artifacts with your changes applied
- **Download** — grab the updated tarball with your refinements baked in

Refine works on both single-host inspection tarballs and fleet-aggregated tarballs.

### Fleet Aggregation

For managing multiple hosts, use `inspectah fleet`:

```bash
# Initialize a fleet directory from scan outputs
inspectah fleet init ./my-fleet/ host1.tar.gz host2.tar.gz host3.tar.gz

# Aggregate the fleet into a single migration spec
inspectah fleet aggregate ./my-fleet/

# Refine the aggregated output
inspectah refine ./my-fleet/fleet-aggregate-*.tar.gz
```

Fleet mode finds the intersection of packages/configs across hosts and identifies per-host exceptions.

## Workflows

```
One host:    Scan ───► Refine ───► Build
Many hosts:  Scan ───► Fleet ────► Refine ───► Build
```

Each step consumes and produces tarballs. Refine and Fleet are optional.

## Configuration

Set environment variables to customize behavior:

| Variable | Effect |
|----------|--------|
| `INSPECTAH_HOSTNAME` | Override the reported hostname |
| `INSPECTAH_DEBUG` | Set to `1` to enable debug logging |

## Documentation

Full documentation is available at [https://marrusl.github.io/inspectah/](https://marrusl.github.io/inspectah/):

- [Tutorials](https://marrusl.github.io/inspectah/tutorials/) — step-by-step guides for common tasks
- [How-to Guides](https://marrusl.github.io/inspectah/how-to/) — recipes for specific scenarios
- [Reference](https://marrusl.github.io/inspectah/reference/) — CLI commands, inspector catalog, output format
- [Explanation](https://marrusl.github.io/inspectah/explanation/) — concepts, architecture, design decisions

## License

[MIT](LICENSE)
