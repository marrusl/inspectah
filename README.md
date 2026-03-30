# yoinkc

Inspect package-based RHEL, CentOS Stream, and Fedora hosts and produce bootc image artifacts (Containerfile, config tree, audit report, etc.).

## What is yoinkc

yoinkc inspects a running RHEL, CentOS Stream, or Fedora host and produces everything needed to rebuild it as a bootc container image. It figures out what you added to the base OS — packages, configs, services, users, cron jobs, container workloads — and generates only the delta. The output is a ready-to-build Containerfile, a config tree, an audit report, and an interactive HTML dashboard. Point it at a real server and get a real migration artifact, not a toy example.

## Workflow

Two paths depending on whether you're migrating one host or many:

```
  One host:    Inspect ────→ Refine ────→ Build
  Many hosts:  Inspect ────→ Fleet ─────→ Refine ────→ Architect ─────→ Build

  Refine, Fleet, and Architect are optional. Each step consumes and produces tarballs.

  Inspect   run-yoinkc.sh                       Scan host, produce tarball
  Fleet     run-yoinkc.sh fleet dir/ -p 80      Merge N hosts into one spec
  Refine    run-yoinkc.sh refine *.tar.gz       Edit findings in the browser
  Architect run-yoinkc.sh architect ./fleets/   Plan layer decomposition
  Build     yoinkc-build *.tar.gz tag           Build the bootc image
```

[Inspect](#inspect) | [Refine](#refine) | [Fleet](#fleet) | [Architect](#architect) | [Build](#build)

---

## Inspect

Run yoinkc on any supported host. The wrapper script installs podman if needed, pulls the pre-built image, and runs the inspection. A hostname-stamped tarball appears in your current directory:

```bash
curl -fsSL https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh | sudo sh
```

That's it. The tarball (e.g. `webserver01-20260312-143000.tar.gz`) contains everything: Containerfile, config tree, reports, snapshot, and RHEL subscription certs (if present). Pass it to `yoinkc refine` for interactive editing or `yoinkc-build` to build the image.

All yoinkc flags pass through the wrapper:

```bash
curl -fsSL -o run-yoinkc.sh https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh
sudo sh run-yoinkc.sh --config-diffs --no-baseline
```

Environment variables for customization:

| Variable | Effect |
|----------|--------|
| `YOINKC_IMAGE` | Override the container image (e.g. a local build or pinned tag) |
| `YOINKC_HOSTNAME` | Override the reported hostname (default: `hostnamectl hostname`, falling back to `hostname -f`) |
| `YOINKC_DEBUG` | Set to `1` to enable debug logging to stderr |

> **Important:** `sudo` must wrap `sh`, not `curl`. The container requires rootful podman — if `sudo` only applies to the download, podman runs rootless and nsenter into host namespaces will fail.

> **RHEL hosts:** The base image on `registry.redhat.io` requires authentication. Run `sudo podman login registry.redhat.io` on the host before running yoinkc, or use `--baseline-packages FILE` as an alternative. CentOS Stream and Fedora images are on public registries and need no authentication.

### Native install (RPM/Homebrew)

If installed via `dnf` or `brew`, run inspect directly:

```bash
sudo yoinkc inspect
```

`fleet` and `refine` do not require root:

```bash
yoinkc fleet ./tarballs/
yoinkc refine ./fleet-output.tar.gz
```

---

## Refine

`yoinkc refine` serves a yoinkc output tarball over HTTP, enabling the HTML report's interactive editing UI. Toggle packages, repos, and config files in the browser and immediately see an updated Containerfile — without touching the inspected host again.

1. Copy the tarball from the target host to your workstation:
   ```bash
   scp target-host:~/hostname-*.tar.gz .
   ```
2. Start the refine server via the wrapper script:
   ```bash
   ./run-yoinkc.sh refine hostname-*.tar.gz
   ```
   This runs `yoinkc refine` inside the container with port 8642 mapped. The browser opens automatically.

Toggle any inspected item (packages, config files, services, repos), adjust user strategies, click Re-render to update the Containerfile, and download the updated tarball when done.

**Direct usage** (when installed via pip):

```bash
yoinkc refine hostname-*.tar.gz
yoinkc refine hostname-*.tar.gz --no-browser --port 9000
```

The server serves the report at `http://localhost:8642` (or the next available port) and auto-opens the browser. Press Ctrl+C to stop.

---

## Build

`yoinkc-build` wraps `podman build` (or `docker build`) with automatic RHEL subscription cert handling and image tagging. Point it at a yoinkc tarball or output directory:

```bash
./yoinkc-build hostname-20260312-143000.tar.gz my-bootc-image:latest
./yoinkc-build ./output-dir/ my-bootc-image:v1.0
```

For RHEL base images, subscription certificates are auto-detected and bind-mounted into the build. On non-RHEL hosts without certs, the build proceeds with a warning.

Push directly after building:

```bash
./yoinkc-build hostname-20260312-143000.tar.gz my-bootc-image:v1.0 --push registry.example.com/my-bootc-image:v1.0
```

Use `--no-cache` for a clean rebuild without layer caching.

**Requirements:** Python 3.9+ (stdlib only). Podman or Docker.

---

## Fleet

`yoinkc fleet` aggregates inspection snapshots from multiple hosts serving the same role into a single fleet snapshot. Building one image per host quickly becomes unmanageable. Fleet analysis finds the common ground across your fleet and produces one shared image spec.

1. Run yoinkc on each host (however you like — manually, Ansible, scripts):
   ```bash
   YOINKC_HOSTNAME=web-01 ./run-yoinkc.sh
   YOINKC_HOSTNAME=web-02 ./run-yoinkc.sh
   YOINKC_HOSTNAME=web-03 ./run-yoinkc.sh
   ```

2. Collect tarballs into a directory on your workstation and run the fleet wrapper:
   ```bash
   mkdir web-servers && cp web-0*.tar.gz web-servers/
   ./run-yoinkc.sh fleet ./web-servers/ -p 80
   ```

The fleet tarball contains a Containerfile, HTML report, and snapshot — same structure as single-host output.

**Prevalence threshold (`-p`):** Controls what gets included. `-p 100` (default) means strict intersection — only items on every host. `-p 80` includes items on 80%+ of hosts. Items below threshold are still visible in the report (as unchecked), just not included in the Containerfile.

**Container wrapper:** `run-yoinkc.sh fleet` runs `yoinkc fleet` inside the yoinkc container — no Python/pip/venv needed on your workstation. Just podman and the script.

| Variable | Effect |
|----------|--------|
| `YOINKC_IMAGE` | Override the container image |
| `YOINKC_OUTPUT_DIR` | Output directory for the fleet tarball (default: CWD) |

**Direct usage** (when installed via pip):

```bash
yoinkc fleet ./web-servers/ -p 80
yoinkc fleet ./web-servers/ --json-only -o merged.json
yoinkc fleet ./web-servers/ --output-dir ./fleet-output/
```

| Flag | Description |
|------|-------------|
| `-p`, `--min-prevalence` | Include items on >= N% of hosts (1-100, default: 100) |
| `-o`, `--output` | Output tarball path (default: `<dir-name>-TIMESTAMP.tar.gz` in CWD) |
| `--output-dir` | Write rendered files to a directory instead of tarball |
| `--json-only` | Write merged snapshot JSON only, skip rendering |
| `--no-hosts` | Omit per-item host lists from fleet metadata |

---

## Architect

`yoinkc architect` takes multiple refined fleet outputs and helps decompose them into a layered bootc image hierarchy: a base image plus derived role-specific or hardware-specific images. It launches an interactive web UI for exploring and adjusting the proposed layer topology.

**When to use architect:** You have 2+ fleets (e.g., web servers, database servers, GPU nodes) and you want to identify common packages that should go into a shared base layer, keeping role-specific packages in derived layers.

```bash
# Place refined fleet tarballs in a directory
mkdir refined-fleets
cp web-servers-refined.tar.gz db-servers-refined.tar.gz gpu-nodes-refined.tar.gz refined-fleets/

# Launch architect — accepts a directory or a tarball bundle
./run-yoinkc.sh architect ./refined-fleets/
./run-yoinkc.sh architect refined-fleets.tar.gz
```

Architect serves an interactive UI on `http://localhost:8643`. The UI shows:
- **Sidebar:** All loaded fleets with host/package counts
- **Center:** Layer topology tree (base + derived layers) with package counts and metrics (hover for explanations)
- **Drawer:** Packages in the selected layer with move/copy controls
- **Preview:** Click "View" on any layer card to see its generated Containerfile

Architect initially proposes all 100%-prevalent packages (shared by all fleets) go into the base layer, with role-specific packages in derived layers. Use "Move up ↑" to shift packages from derived layers to the parent layer, or "Copy to →" to duplicate a package across sibling layers. Exported Containerfiles use bare package names (e.g., `vim`) for maximum compatibility across minor version boundaries.

When ready, click **Export Containerfiles** to download a tarball containing a `Containerfile` for each layer, plus a `build.sh` script with ordered build commands.

**Direct usage** (when installed via pip):

```bash
yoinkc architect ./refined-fleets/
yoinkc architect refined-fleets.tar.gz
```

---

## Output Artifacts

The default output is a tarball (`hostname-YYYYMMDD-HHMMSS.tar.gz`) containing:

```
hostname-20260312-143000.tar.gz
└── hostname-20260312-143000/
    ├── Containerfile                 # Layered image definition (cache-optimized layer order)
    ├── README.md                     # Build/deploy commands, FIXME checklist
    ├── audit-report.md               # Detailed findings with storage migration plan, version drift summary
    ├── report.html                   # Self-contained interactive HTML dashboard with Version Changes table
    ├── secrets-review.md             # Redacted sensitive content for operator review
    ├── kickstart-suggestion.ks       # Deploy-time config (conditional)
    ├── inspection-snapshot.json      # Raw structured data (re-renderable via --from-snapshot)
    ├── config/                       # Files to COPY into the image
    │   ├── etc/                      # Mirrors /etc — modified configs, repos, firewall, timers
    │   ├── opt/                      # Non-RPM software (venvs, npm apps, binaries)
    │   ├── usr/                      # Files under /usr/local
    │   └── usr/lib/sysusers.d/       # systemd-sysusers conf (sysusers strategy)
    ├── quadlet/                      # Container workload unit files (conditional)
    ├── yoinkc-users.toml             # bootc-image-builder user config (conditional)
    ├── entitlement/                  # RHEL subscription certs (conditional, RHEL only)
    └── rhsm/                         # RHEL subscription manager config (conditional, RHEL only)
```

Use `--output-dir` to get unpacked directory output instead.

---

## CLI Reference

### `yoinkc` (inspect)

#### Core Options

| Flag | Description |
|------|-------------|
| `--host-root PATH` | Root path for host inspection (default: `/host`) |
| `-o FILE` | Write tarball to FILE (default: `HOSTNAME-TIMESTAMP.tar.gz` in current directory) |
| `--output-dir DIR` | Write files to a directory instead of producing a tarball. Mutually exclusive with `-o`. |
| `--no-subscription` | Skip bundling RHEL subscription certs into the output |
| `--from-snapshot PATH` | Skip inspection; load snapshot from file and run renderers only. Mutually exclusive with `--inspect-only`. |
| `--inspect-only` | Run inspectors and save snapshot to current directory; do not run renderers. Mutually exclusive with `--from-snapshot`. |

#### Target Image

| Flag | Description |
|------|-------------|
| `--target-version VERSION` | Target bootc image version (e.g. `9.6`, `10.2`). Default: source host version, clamped to minimum bootc-supported release (9.6 for RHEL 9) |
| `--target-image IMAGE` | Full target bootc base image reference (e.g. `registry.redhat.io/rhel10/rhel-bootc:10.2`). Overrides `--target-version` and all automatic mapping |

#### Inspection Options

| Flag | Description |
|------|-------------|
| `--baseline-packages FILE` | Path to a newline-separated package list for air-gapped environments where the base image cannot be queried via podman |
| `--config-diffs` | Generate line-by-line diffs for modified configs via `rpm2cpio` (retrieves from local cache or downloads from repos) |
| `--deep-binary-scan` | Full `strings` scan on unknown binaries with extended version pattern matching (slow) |
| `--query-podman` | Connect to podman to enumerate running containers with full inspect data |
| `--user-strategy STRATEGY` | Override user creation strategy for all users. Valid: `sysusers`, `blueprint`, `useradd`, `kickstart` |
| `--skip-preflight` | Skip container privilege checks (rootful, `--pid=host`, `--privileged`, SELinux) |

#### Output Options

| Flag | Description |
|------|-------------|
| `--validate` | After generating output, run `podman build` to verify the Containerfile. Requires `--output-dir`. |
| `--push-to-github REPO` | Push output directory to a GitHub repository (e.g. `owner/repo`). Requires `--output-dir`. |
| `--github-token TOKEN` | GitHub personal access token for repo creation (falls back to `GITHUB_TOKEN` env var) |
| `--public` | When creating a new GitHub repo, make it public (default: private) |
| `--yes` | Skip interactive confirmation prompts |

### `yoinkc refine`

| Flag | Description |
|------|-------------|
| `--no-browser` | Don't auto-open the browser on startup |
| `--port PORT` | Listen port (default: 8642, falls back to next available) |

### `yoinkc fleet`

| Flag | Description |
|------|-------------|
| `-p`, `--min-prevalence` | Include items on >= N% of hosts (1-100, default: 100) |
| `-o`, `--output` | Output tarball path (default: `<dir-name>-TIMESTAMP.tar.gz` in CWD) |
| `--output-dir` | Write rendered files to a directory instead of tarball |
| `--json-only` | Write merged snapshot JSON only, skip rendering |
| `--no-hosts` | Omit per-item host lists from fleet metadata |

### `yoinkc architect`

| Argument | Description |
|----------|-------------|
| `input` | Directory containing refined fleet tarballs (`.tar.gz`), or a tarball bundle of tarballs |

Launches an HTTP server on port 8643 with an interactive web UI for layer topology planning. If a tarball is provided, it's automatically extracted to a temporary directory. Press Ctrl+C to stop.

> `yoinkc-build` is a standalone companion script, not a subcommand. Its usage is covered in [Build](#build).

---

## How It Works

### Architecture

- **Inspectors** run against a host root (default `/host`) and produce structured JSON (the inspection snapshot).
- **Renderers** consume the snapshot and produce output artifacts (Containerfile, markdown report, HTML report, etc.).

A core design principle is **baseline subtraction**: wherever possible, the tool subtracts base-image defaults from the host's current state so that only operator-added or operator-modified items appear in the output. Packages are diffed against the base image package list, services against base image presets, timers and cron jobs against RPM ownership, and kernel/SELinux configs against shipped defaults. Items that exist identically in the base image are omitted — they'll already be there.

Three subcommands and one companion tool complete the workflow:

- **`yoinkc refine`** serves an interactive UI for editing findings — toggling packages in or out, changing user migration strategies, excluding config files — and re-rendering the Containerfile live. See [Refine](#refine).
- **`yoinkc fleet`** aggregates inspections from multiple hosts into a single fleet snapshot, producing a merged Containerfile and report with prevalence annotations. See [Fleet](#fleet).
- **`yoinkc architect`** takes multiple refined fleets and proposes a layered image topology (base + derived layers), with an interactive web UI for adjusting the decomposition. See [Architect](#architect).
- **`yoinkc-build`** builds a bootc container image from yoinkc output, with automatic RHEL subscription cert handling for building on non-RHEL hosts. See [Build](#build).

### Refine UI internals

Every inspected item (packages, config files, services, repos, etc.) has an include/exclude checkbox. Users and groups have per-row strategy dropdowns (`sysusers`, `useradd`, `blueprint`, `kickstart`) with apply-all buttons for batch changes. The sticky footer toolbar reflects three states: **dirty** (changes pending, Re-render button highlighted), **clean + helper** (no pending changes, tarball download available), and **standalone** (report opened without the refine server — checkboxes hidden, toolbar collapsed). Clicking Re-render sends the modified snapshot to the server, which runs a fresh render and replaces the page with the updated report. The Download Tarball button packages the current output state for transfer.

### Build cert handling

For RHEL base images (`registry.redhat.io`), `yoinkc-build` searches for subscription certificates in this order: bundled in the yoinkc output, host-local (`/etc/pki/entitlement`), current directory (`./entitlement/`), or `YOINKC_ENTITLEMENT` env var. Certs are bind-mounted into the build via `-v`. On a RHEL host with a valid subscription, cert access is handled by podman natively. Found certificates are validated via `openssl x509 -checkend` — the operator gets an expiry warning before a build fails due to stale credentials. On non-RHEL hosts, if no certs are found the build proceeds with a warning — the operator may have a Satellite or local mirror configured.

### Fleet report features

The fleet HTML report includes fleet-specific UI: a summary banner, prevalence color bars on every item (showing how many hosts have it), click-to-toggle fraction/percentage display, host list popovers with a split Copy button (one-per-line, comma-separated, or space-separated formats), and grouped content variants for config files with differences across hosts.

### Inspectors

Each inspector examines one aspect of the host and contributes a section to the inspection snapshot.

#### RPM / Packages

- Full package inventory via `rpm -qa` with epoch/version/release/arch
- Baseline from the target **bootc base image** — queries the image directly via `podman run` to get its package list, then diffs against installed packages to identify what the operator added
- **Version drift detection**: compares package versions between host and base image. Downgrades (host has newer version that would be reverted) are flagged as warnings; upgrades (base image is newer) are noted as informational. Gracefully skipped when using names-only baseline files.
- Leaf/auto classification: `dnf repoquery --userinstalled` identifies packages the operator explicitly installed vs those pulled in as dependencies. Only leaf packages appear in the Containerfile's `dnf install` line. Falls back to dependency graph analysis (`dnf repoquery --recursive` or `rpm -qR`) when `--userinstalled` is unavailable. This is more accurate than pure graph-based classification — it correctly handles packages like `git` that the operator installed but which other added packages also depend on.
- Source repo tracking per package via `dnf repoquery --installed`, with repo-grouped display in the HTML report and audit report
- GPG key handling: parses `gpgkey=file:///...` from repo files (including INI-style continuation lines), resolves `$releasever` and `$basearch` variables, and COPYs key files into the image before `dnf install`
- Modified config detection via `rpm -Va` with verification flags
- Unowned file detection using bulk `rpm -qla` set subtraction (fast, avoids per-file lookups)
- `dnf history` analysis for packages that were installed then removed (orphaned configs)
- Repo file capture from `/etc/yum.repos.d/` and `/etc/dnf/`
- Optional line-by-line diffs against RPM defaults (`--config-diffs`) with syntax-highlighted rendering in the HTML report

#### Services

- Enabled/disabled/masked unit state from `systemctl list-unit-files` with filesystem-based fallback
- Diff against **base image** systemd preset defaults (queried from the target bootc image)
- State change actions generated for the Containerfile (`systemctl enable`/`disable`)

#### Configuration Files

- RPM-owned modified files (from `rpm -Va`)
- Unowned files in `/etc` (hand-placed configs) with extensible exclusion list for system-generated artifacts
- Orphaned configs from removed packages
- Sensitive content detection and automatic redaction
- Semantic categories assigned by path (tmpfiles, environment, audit, library_path, journal, logrotate, automount, sysctl) — displayed as a sortable "Category" column in the HTML report
- Optional `--config-diffs`: retrieves RPM defaults from local cache or downloads from repos, generates unified diffs

#### Network

- NetworkManager connection profiles classified as **static** (bake into image) or **DHCP** (kickstart at deploy)
- Firewalld zone parsing: services, ports, and rich rules from zone XML
- Firewalld direct rules from `direct.xml`
- `resolv.conf` provenance detection: systemd-resolved, NetworkManager-managed, or hand-edited
- `ip route` and `ip rule` capture with default rule filtering
- `/etc/hosts` additions and proxy settings
- Containerfile COPYs zone XML files; `firewall-offline-cmd` equivalents are documented in the audit report
- Static route file detection with FIXME guidance in both Containerfile and kickstart (translate to NM connection properties)
- Proxy env vars and `/etc/hosts` additions rendered in both Containerfile and kickstart

#### Storage

- `/etc/fstab` parsing with **migration recommendations** per mount point (image-embedded, PVC/volume, external storage, swap, tmpfs)
- LVM layout detection

#### Scheduled Tasks

- Cron jobs from `/etc/cron.d`, `/etc/crontab`, periodic dirs, and user spool
- Automatic cron-to-systemd timer conversion with **actual command extraction** into `ExecStart`
- Existing systemd timer scanning from `/etc/systemd/system` (local) and `/usr/lib/systemd/system` (vendor) with `OnCalendar` and `ExecStart` extraction
- `at` job parsing: extracts actual command, user, and working directory from spool files
- Display filtering: vendor systemd timers (shipped with the base image) are hidden from reports since they require no operator action. RPM-owned cron jobs are similarly excluded.

#### Containers

- Quadlet `.container` unit discovery with `Image=` reference extraction
- Compose file discovery with per-service `image:` field parsing (no PyYAML dependency)
- Optional live container enumeration via `podman ps` + `podman inspect` (`--query-podman`): captures mounts, network settings, ports, and environment variables

#### Non-RPM Software

- **readelf-based binary classification**: detects Go (`.note.go.buildid`), Rust (`.rustc`), and C/C++ binaries with static/dynamic linking and shared library enumeration
- **pip C extension detection**: identifies packages with `.so` files via RECORD inspection; triggers multi-stage Containerfile build
- **Python venv detection**: discovers venvs via `pyvenv.cfg`, flags `--system-site-packages`, scans dist-info and `pip list --path` for package inventories
- **pip dist-info scanning**: system-level pip packages with name and version
- **npm/yarn/gem lockfile detection**: captures lockfiles for reproducible installs
- **Git repository detection**: captures remote URL, branch, and commit hash for directories under `/opt` and `/usr/local`
- Optional deep binary strings scan for version extraction (`--deep-binary-scan`) with extended patterns for Go, Rust, OpenSSL, Java, Node, Python, and build metadata

#### Kernel & Boot

- `/proc/cmdline` and GRUB defaults
- `lsmod` parsing with module classification: default (from `modules-load.d`), configured, dependency, or non-default
- Runtime sysctl values from `/proc/sys` diffed against shipped defaults in `/usr/lib/sysctl.d` with source attribution
- `modules-load.d`, `modprobe.d`, and `dracut.conf.d` config capture and COPY into image
- Locale, timezone, and alternatives detection via file-based methods (container-compatible): rendered in the Kernel/Boot tab as a system properties description list and alternatives table

#### SELinux

- Mode detection (enforcing/permissive/disabled)
- Custom module discovery via `semodule -l` cross-referenced with priority-400 module store
- Non-default boolean identification via `semanage boolean -l` current vs. default comparison
- Audit rules and `fcontext` capture

#### Users & Groups

- Non-system users and groups (1000 <= UID/GID < 60000)
- Raw `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow` entry capture
- Strategy-aware provisioning: each user is assigned one of four strategies based on account type — `sysusers` (service accounts, created at boot via systemd-sysusers), `useradd` (ambiguous accounts, explicit `RUN useradd` in Containerfile), `kickstart` (human users, deferred to deploy time), or `blueprint` (bootc-image-builder TOML). Override with `--user-strategy` to apply a single strategy to all users.
- `/etc/subuid` and `/etc/subgid` for rootless container mappings
- SSH authorized key references (paths only, not key material — never baked into image)
- Sudoers rules capture with FIXME guidance in Containerfile
- Home directory detection

### Containerfile Layer Ordering

The generated Containerfile follows a deliberate layer order optimized for build cache efficiency — layers that change least frequently come first:

1. **Build stage** (conditional) — multi-stage build for pip packages with C extensions: installs build deps, compiles wheels
2. **Base image** — auto-detected from `/etc/os-release`, mapped to the corresponding bootc base image
3. **Repo files** — GPG key files COPYed first (for signature verification), then custom yum/dnf repositories
4. **Packages** — `dnf install` for leaf packages added beyond the base image (auto-dependencies omitted, resolved by dnf)
5. **Services** — `systemctl enable/disable` based on base image preset diff
6. **Firewall** — zone XML files and direct rules via `COPY`; `firewall-offline-cmd` equivalents documented in the audit report
7. **Scheduled tasks** — timer units (local + cron-converted with actual commands), at job FIXMEs
8. **Config files** — all captured configs via `COPY config/` with optional diff summaries
9. **Non-RPM software** — tool package prerequisites installed first (e.g. `nodejs`/`python3-pip`) when needed but not already in the package set; then provenance-aware directives: `pip install` for pip, multi-stage for C extensions, `npm ci` for npm, FIXME for Go/Rust binaries, `git clone` comments for git repos
10. **Container workloads** — quadlet units
11. **Users & groups** — strategy-aware provisioning: `sysusers` for service accounts, `useradd` for ambiguous, `kickstart` for human users, `blueprint` for bootc-image-builder TOML
12. **Kernel** — sysctl overrides, kargs.d TOML drop-in, module configs, tuned profiles
13. **SELinux** — booleans, custom modules, port labels, fcontext rules
14. **Network** — static connection profiles, `/etc/hosts` additions, proxy env vars, static route guidance
15. **tmpfiles.d** — transient file/directory setup
16. **bootc container lint** — validates the generated image is bootc-compatible

### Baseline Generation

The tool generates a package baseline by querying the target **bootc base image** directly. It detects the host OS from `/etc/os-release`, maps it to the corresponding base image, and runs `podman run --rm <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete package list. The diff against host packages produces exactly the `dnf install` list the Containerfile needs.

**Supported OS → base image mappings:**

| Source Host | Target Base Image | Notes |
|-------------|-------------------|-------|
| RHEL 9.x | `registry.redhat.io/rhel9/rhel-bootc:{version}` | Version clamped to 9.6 minimum (first bootc release) |
| RHEL 10.x | `registry.redhat.io/rhel10/rhel-bootc:{version}` | |
| CentOS Stream 9 | `quay.io/centos-bootc/centos-bootc:stream9` | |
| CentOS Stream 10 | `quay.io/centos-bootc/centos-bootc:stream10` | |
| Fedora | `quay.io/fedora/fedora-bootc:{major}` | Version clamped to 41 minimum |

**Source/target version separation:** The source host (what you're inspecting) and the target image (your Containerfile's FROM line) can differ. A RHEL 9.4 host auto-targets `rhel-bootc:9.6` (the minimum bootc release). Override with `--target-version 9.8` or `--target-image` for full control. Cross-major-version migrations (e.g. RHEL 9 → 10) produce a prominent warning since package names, services, and config formats may differ.

**RHEL registry authentication:** RHEL base images on `registry.redhat.io` require authentication. The tool checks for credentials before attempting to pull and will exit with instructions if credentials are missing. Run `sudo podman login registry.redhat.io` on the host before running yoinkc, or use `--baseline-packages FILE` as an alternative. CentOS Stream and Fedora images are on public registries and need no authentication.

When running inside a container, the tool uses `nsenter` to execute `podman` in the host's namespaces. This requires `sudo`, `--pid=host`, and `--privileged` on the outer container (see the run command above). Before attempting `nsenter`, the tool runs a fast probe to detect rootless containers and missing capabilities, and provides specific guidance if the probe fails.

**Fallback behavior:**

- **Base image queryable** — accurate package diff, only truly operator-added packages appear in the Containerfile
- **Base image not available** (not pulled, auth failure, or `--skip-preflight` used without proper flags) — enters "all-packages mode" where every installed package is treated as operator-added (no baseline subtraction), with a clear warning in the reports
- **Air-gapped environments** — use `--baseline-packages FILE` to provide a newline-separated list of package names, bypassing the podman query

The resolved baseline (including the base image package list) is cached in the inspection snapshot, so `--from-snapshot` re-renders work without network access or podman.

---

## Advanced Usage

### Running directly

The pre-built image is published to GHCR on every push to `main`:

```
ghcr.io/marrusl/yoinkc:latest
```

Multi-arch (amd64 + arm64). To run it directly without the wrapper script:

```bash
sudo podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -w /output \
  -v /:/host:ro \
  -v "$(pwd):/output" \
  ghcr.io/marrusl/yoinkc:latest
```

To build locally instead:

```bash
podman build -t yoinkc .
```

> **Required flags:** The container must run with **rootful podman** (`sudo`), `--pid=host`, `--privileged`, and `--security-opt label=disable`. The tool performs a preflight check on startup and will exit with a clear error if any of these are missing. Use `--skip-preflight` to bypass the check if needed.
>
> | Flag | Why |
> |------|-----|
> | `sudo` (rootful) | nsenter into host namespaces requires real `CAP_SYS_ADMIN` — rootless podman runs in a user namespace where this is impossible |
> | `--pid=host` | Exposes the host PID namespace so the tool can reach PID 1 and run `podman` on the host via `nsenter` |
> | `--privileged` | Grants the full capability set including `CAP_SYS_ADMIN` for `nsenter` and broad filesystem access |
> | `--security-opt label=disable` | Disables SELinux label enforcement so the container can read all host paths |
>
> The tool needs broad read access across the host filesystem — the container is a packaging convenience, not a security boundary.

After the run, a hostname-stamped tarball appears in your current directory. Extract it to inspect the contents, or pass it directly to `yoinkc refine` or `yoinkc-build`. Use `--output-dir DIR` instead if you prefer unpacked directory output (required for `--validate` and `--push-to-github`). The HTML report (`report.html`) is **self-contained and portable**: all content is embedded, so you can share or archive that file alone.

### Development

```bash
pip install -e .
yoinkc --help
pytest
```

#### Usage (when installed directly)

```bash
# Inspect host mounted at /host, produce tarball in current directory
yoinkc

# Write tarball to a specific path
yoinkc -o /tmp/migration.tar.gz

# Write to a directory instead of tarball
yoinkc --output-dir ./my-output

# Save snapshot only (no render)
yoinkc --inspect-only

# Render from existing snapshot
yoinkc --from-snapshot ./inspection-snapshot.json

# Skip bundling RHEL subscription certs (e.g. when sharing publicly)
yoinkc --no-subscription
```

---

## License

MIT — see [LICENSE](LICENSE).
