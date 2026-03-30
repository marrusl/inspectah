# yoinkc

Scan package-based RHEL, CentOS Stream, and Fedora hosts and generate bootc image artifacts.

## What is yoinkc?

yoinkc scans a running RHEL, CentOS Stream, or Fedora host and generates everything you need to rebuild it as a [bootc](https://containers.github.io/bootc/) container image. bootc images are full operating system images managed and deployed as OCI containers — update your OS the same way you update your apps, with atomic upgrades and built-in rollback. yoinkc figures out what you added to the base OS — packages, configs, services, users, cron jobs, container workloads — and generates only the delta. The output is a ready-to-build Containerfile, a config tree, an audit report, and an interactive HTML dashboard. Point it at a real server and get a real migration artifact.

> **Status:** yoinkc is an active prototype. It handles common RHEL 9, CentOS Stream, and Fedora configurations well, but expect rough edges on unusual setups. It targets RPM-based systems only — no Debian, no RHEL 7, no live/in-place migration. See [driftify](https://github.com/marrusl/driftify) for a companion testing harness that validates yoinkc end-to-end with synthetic drift.

## Workflow

```
  One host:    Inspect ───► Refine ───► Build
  Many hosts:  Inspect ───► Fleet ────► Refine ───► Architect ───► Build

  Each step consumes and produces tarballs. Refine, Fleet, and Architect are optional.

  Inspect    run-yoinkc.sh                       Scan host, produce tarball
  Refine     run-yoinkc.sh refine *.tar.gz       Edit findings in the browser
  Fleet      run-yoinkc.sh fleet dir/ -p 80      Merge N hosts into one spec
  Architect  run-yoinkc.sh architect ./fleets/   Plan layer decomposition
  Build      yoinkc-build *.tar.gz tag           Build the bootc image
```

## Quick Start

### Inspect a host

```bash
curl -fsSL -o run-yoinkc.sh https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh
chmod +x run-yoinkc.sh
sudo ./run-yoinkc.sh
```

A hostname-stamped tarball appears in your current directory (e.g. `webserver01-20260312-143000.tar.gz`). It contains the Containerfile, config tree, reports, and snapshot. The script stays on disk for subsequent commands.

> **`sudo` must wrap the script, not `curl`.** The container requires rootful podman — if `sudo` only applies to the download, nsenter into host namespaces will fail.

> **RHEL hosts:** Run `sudo podman login registry.redhat.io` first. The base image requires authentication. CentOS Stream and Fedora need no auth.

### Refine findings

```bash
scp target-host:~/hostname-*.tar.gz .
./run-yoinkc.sh refine hostname-*.tar.gz
```

The browser opens automatically. Toggle packages, config files, and services; click Re-render to update the Containerfile; download the updated tarball when done.

### Build the image

`yoinkc-build` handles building bootc images from yoinkc output, primarily solving the problem of building RHEL images on non-RHEL hosts (Mac, Windows, Fedora). It auto-detects and bind-mounts RHEL subscription certs so `dnf install` works inside the build.

```bash
curl -fsSL -o yoinkc-build https://raw.githubusercontent.com/marrusl/yoinkc/main/yoinkc-build
chmod +x yoinkc-build
./yoinkc-build hostname-20260312-143000.tar.gz my-bootc-image:latest
```

Push with `--push registry.example.com/my-bootc-image:v1.0`.

## Output Artifacts

The default output is a tarball (`hostname-YYYYMMDD-HHMMSS.tar.gz`) containing:

```
hostname-20260312-143000.tar.gz
└── hostname-20260312-143000/
    ├── Containerfile                 # Layered image definition (cache-optimized)
    ├── README.md                     # Build/deploy commands, FIXME checklist
    ├── audit-report.md               # Detailed findings, storage plan, version drift
    ├── report.html                   # Self-contained interactive HTML dashboard
    ├── secrets-review.md             # Redacted sensitive content for review
    ├── kickstart-suggestion.ks       # Deploy-time config (conditional)
    ├── inspection-snapshot.json      # Raw structured data (re-renderable)
    ├── config/                       # Files to COPY into the image
    │   ├── etc/                      # Modified configs, repos, firewall, timers
    │   ├── opt/                      # Non-RPM software (venvs, npm apps, binaries)
    │   └── usr/                      # Files under /usr/local
    ├── quadlet/                      # Container workload unit files (conditional)
    ├── yoinkc-users.toml             # bootc-image-builder user config (conditional)
    ├── entitlement/                  # RHEL subscription certs (conditional)
    └── rhsm/                         # RHEL subscription manager config (conditional)
```

Use `--output-dir` to get unpacked directory output instead.

## Fleet Aggregation

`yoinkc fleet` merges inspection snapshots from multiple hosts serving the same role into a single fleet specification.

```bash
# Inspect each host
YOINKC_HOSTNAME=web-01 ./run-yoinkc.sh
YOINKC_HOSTNAME=web-02 ./run-yoinkc.sh
YOINKC_HOSTNAME=web-03 ./run-yoinkc.sh

# Collect and aggregate
mkdir web-servers && cp web-0*.tar.gz web-servers/
./run-yoinkc.sh fleet ./web-servers/ -p 80
```

The `-p` (prevalence threshold) controls inclusion. `-p 100` (default) means strict intersection — only items on every host. `-p 80` includes items on 80%+ of hosts. Items below threshold remain visible in the report but are excluded from the Containerfile.

The container wrapper (`run-yoinkc.sh fleet`) runs everything inside the yoinkc container — no Python or pip required on your workstation.

See [CLI Reference](docs/reference/cli.md#yoinkc-fleet) for the full flag list.

## Architect

`yoinkc architect` takes multiple refined fleet outputs and decomposes them into a layered bootc image hierarchy: a shared base image plus derived role-specific images.

```bash
mkdir refined-fleets
cp web-servers-refined.tar.gz db-servers-refined.tar.gz refined-fleets/
./run-yoinkc.sh architect ./refined-fleets/
```

The interactive web UI (default port 8643) lets you explore the proposed layer topology, move packages between layers, preview generated Containerfiles, and export the final set with an ordered build script.

See [CLI Reference](docs/reference/cli.md#yoinkc-architect) for flags.

## Installation

### Container (recommended)

The wrapper script handles everything — it installs podman if needed and pulls the pre-built image:

```bash
curl -fsSL -o run-yoinkc.sh https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh
chmod +x run-yoinkc.sh
```

The image is published to `ghcr.io/marrusl/yoinkc:latest` (multi-arch: amd64 + arm64).

### pipx

```bash
pipx install yoinkc
```

### From source

```bash
pip install -e .
```

Requirements: Python 3.11+, pydantic, jinja2. Podman or Docker for inspect and build. Installs the `yoinkc` CLI; `yoinkc-build` is a standalone script in the repo root.

| Variable | Effect |
|----------|--------|
| `YOINKC_IMAGE` | Override the container image (e.g. a local build or pinned tag) |
| `YOINKC_HOSTNAME` | Override the reported hostname |
| `YOINKC_DEBUG` | Set to `1` to enable debug logging |

## See Also

- [CLI Reference](docs/reference/cli.md) — complete flag tables for all subcommands
- [Architecture](docs/explanation/architecture.md) — how inspectors, renderers, and baseline subtraction work
- [Design Document](design.md) — full technical design and schema reference
- [driftify](https://github.com/marrusl/driftify) — companion tool for applying synthetic drift to test yoinkc end-to-end
- [bootc upstream](https://containers.github.io/bootc/) — bootc project documentation

## License

MIT — see [LICENSE](LICENSE).
