# inspectah

Scans a running RHEL, CentOS Stream, or Fedora system and generates
everything needed to rebuild it as a
[bootc](https://containers.github.io/bootc/) image: a Containerfile,
config file tree, audit report, and deployment fragments. Run it on one
host or aggregate a fleet.

## Quick Start

```bash
# Install
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah

# Scan
sudo inspectah scan

# Review findings in the browser, then build
inspectah refine hostname-*.tar.gz
podman build -t my-image -f Containerfile .
```

## Installation

### RPM (Fedora / RHEL / CentOS Stream)

```bash
sudo dnf copr enable marrusl/inspectah
sudo dnf install inspectah
```

### From source

```bash
git clone https://github.com/marrusl/inspectah.git
cd inspectah
cargo build --release -p inspectah-cli
sudo install target/release/inspectah /usr/local/bin/
```

Requires Rust 1.80+ and a C linker.

## What It Does

inspectah runs a four-stage pipeline:

1. **Scan** -- collects RPMs, config files, services, users, repos,
   secrets, and more from the running system.
2. **Classify** -- compares findings against a baseline image to
   separate what the base already provides from what was added.
3. **Triage** -- marks each finding as include, exclude, or review
   based on heuristics.
4. **Render** -- writes migration artifacts: Containerfile, config
   tree, audit report, HTML dashboard, kickstart, and secrets review.

inspectah does not build images. It produces the inputs for
`podman build` (or any OCI-compatible builder).

### Fleet mode

When multiple hosts serve the same role, `inspectah fleet` merges their
scans into one specification:

```bash
sudo inspectah scan -o web-01.tar.gz
sudo inspectah scan -o web-02.tar.gz
inspectah fleet aggregate web-01.tar.gz web-02.tar.gz
```

Use `inspectah fleet init <dir>` to generate a TOML manifest for
per-host overrides and prevalence thresholds.

## Output

Each scan produces a tarball:

```
hostname-20260527-143000.tar.gz
└── hostname-20260527-143000/
    ├── Containerfile               # Image build definition
    ├── README.md                   # Build commands and FIXME checklist
    ├── audit-report.md             # Findings, storage plan, version drift
    ├── report.html                 # Interactive HTML dashboard
    ├── secrets-review.md           # Redacted sensitive content for review
    ├── kickstart-suggestion.ks     # Deploy-time settings
    ├── inspection-snapshot.json    # Structured data (re-renderable)
    ├── config/                     # Files to COPY into the image
    │   ├── etc/                    #   Modified configs, repos, timers
    │   ├── opt/                    #   Non-RPM software
    │   └── usr/                    #   Files under /usr/local
    ├── env-files/                  # .env files (conditional)
    ├── quadlet/                    # Container workload units (conditional)
    ├── inspectah-users.toml        # User/group config for bootc-image-builder (conditional)
    ├── inspectah-users.ks          # User/group kickstart fragment (conditional)
    ├── entitlement/                # RHEL subscription certs (conditional)
    └── rhsm/                       # Subscription manager config (conditional)
```

Use `--inspect-only` to write a JSON snapshot without the full tarball.

## Commands

| Command | Description |
|---------|-------------|
| `scan` | Scan the system and produce a migration tarball |
| `refine <tarball>` | Open a browser UI to review and edit findings |
| `fleet init <dir>` | Generate a fleet manifest from a directory of tarballs |
| `fleet aggregate [inputs]` | Merge host tarballs into a fleet specification |
| `version` | Print version, commit, and build date |

Run `inspectah <command> --help` for the full flag list.

## Documentation

Full docs: [marrusl.github.io/inspectah](https://marrusl.github.io/inspectah/)

See also [driftify](https://github.com/marrusl/driftify), a companion
tool that applies synthetic drift to test inspectah end-to-end.

## License

[MIT](LICENSE)
