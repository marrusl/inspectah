# Architecture

This document covers inspectah internals: inspectors, renderers, baseline subtraction, Containerfile layer ordering, and build cert handling. For usage, see the [README](../../README.md). For the full technical design, see [design.md](../../design.md).

## Overview

- **Inspectors** run against a host root (default `/host`) and produce structured JSON (the inspection snapshot).
- **Renderers** consume the snapshot and produce output artifacts (Containerfile, markdown report, HTML report, etc.).

A core design principle is **baseline subtraction**: wherever possible, the tool subtracts base-image defaults from the host's current state so that only operator-added or operator-modified items appear in the output. Packages are diffed against the base image package list, services against base image presets, timers and cron jobs against RPM ownership, and kernel/SELinux configs against shipped defaults. Items that exist identically in the base image are omitted — they'll already be there.

Three subcommands and one companion tool complete the workflow:

- **`inspectah refine`** serves an interactive UI for editing findings — toggling packages in or out, changing user migration strategies, excluding config files — and re-rendering the Containerfile live.
- **`inspectah fleet`** aggregates inspections from multiple hosts into a single fleet snapshot, producing a merged Containerfile and report with prevalence annotations.
- **`inspectah architect`** takes multiple refined fleets and proposes a layered image topology (base + derived layers), with an interactive web UI for adjusting the decomposition.
- **`inspectah-build`** builds a bootc container image from inspectah output, with automatic RHEL subscription cert handling.

## Refine UI Internals

Every inspected item (packages, config files, services, repos, etc.) has an include/exclude checkbox. Users and groups have per-row strategy dropdowns (`sysusers`, `useradd`, `blueprint`, `kickstart`) with apply-all buttons for batch changes. The sticky footer toolbar reflects three states: **dirty** (changes pending, Re-render button highlighted), **clean + helper** (no pending changes, tarball download available), and **standalone** (report opened without the refine server — checkboxes hidden, toolbar collapsed). Clicking Re-render sends the modified snapshot to the server, which runs a fresh render and replaces the page with the updated report. The Download Tarball button packages the current output state for transfer.

## Build Cert Handling

For RHEL base images (`registry.redhat.io`), `inspectah-build` searches for subscription certificates in this order: bundled in the inspectah output, host-local (`/etc/pki/entitlement`), current directory (`./entitlement/`), or `INSPECTAH_ENTITLEMENT` env var. Certs are bind-mounted into the build via `-v`. On a RHEL host with a valid subscription, cert access is handled by podman natively. Found certificates are validated via `openssl x509 -checkend` — the operator gets an expiry warning before a build fails due to stale credentials. On non-RHEL hosts, if no certs are found the build proceeds with a warning — the operator may have a Satellite or local mirror configured.

## Fleet Report Features

The fleet HTML report includes fleet-specific UI: a summary banner, prevalence color bars on every item (showing how many hosts have it), click-to-toggle fraction/percentage display, host list popovers with a split Copy button (one-per-line, comma-separated, or space-separated formats), and grouped content variants for config files with differences across hosts.

## Inspectors

Each inspector examines one aspect of the host and contributes a section to the inspection snapshot.

### RPM / Packages

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

### Services

- Enabled/disabled/masked unit state from `systemctl list-unit-files` with filesystem-based fallback
- Diff against **base image** systemd preset defaults (queried from the target bootc image)
- State change actions generated for the Containerfile (`systemctl enable`/`disable`)

### Configuration Files

- RPM-owned modified files (from `rpm -Va`)
- Unowned files in `/etc` (hand-placed configs) with extensible exclusion list for system-generated artifacts
- Orphaned configs from removed packages
- Sensitive content detection and automatic redaction
- Semantic categories assigned by path (tmpfiles, environment, audit, library_path, journal, logrotate, automount, sysctl) — displayed as a sortable "Category" column in the HTML report
- Optional `--config-diffs`: retrieves RPM defaults from local cache or downloads from repos, generates unified diffs

### Network

- NetworkManager connection profiles classified as **static** (bake into image) or **DHCP** (kickstart at deploy)
- Firewalld zone parsing: services, ports, and rich rules from zone XML
- Firewalld direct rules from `direct.xml`
- `resolv.conf` provenance detection: systemd-resolved, NetworkManager-managed, or hand-edited
- `ip route` and `ip rule` capture with default rule filtering
- `/etc/hosts` additions and proxy settings
- Containerfile COPYs zone XML files; `firewall-offline-cmd` equivalents are documented in the audit report
- Static route file detection with FIXME guidance in both Containerfile and kickstart (translate to NM connection properties)
- Proxy env vars and `/etc/hosts` additions rendered in both Containerfile and kickstart

### Storage

- `/etc/fstab` parsing with **migration recommendations** per mount point (image-embedded, PVC/volume, external storage, swap, tmpfs)
- LVM layout detection

### Scheduled Tasks

- Cron jobs from `/etc/cron.d`, `/etc/crontab`, periodic dirs, and user spool
- Automatic cron-to-systemd timer conversion with **actual command extraction** into `ExecStart`
- Existing systemd timer scanning from `/etc/systemd/system` (local) and `/usr/lib/systemd/system` (vendor) with `OnCalendar` and `ExecStart` extraction
- `at` job parsing: extracts actual command, user, and working directory from spool files
- Display filtering: vendor systemd timers (shipped with the base image) are hidden from reports since they require no operator action. RPM-owned cron jobs are similarly excluded.

### Containers

- Quadlet `.container` unit discovery with `Image=` reference extraction
- Compose file discovery with per-service `image:` field parsing (no PyYAML dependency)
- Optional live container enumeration via `podman ps` + `podman inspect` (`--query-podman`): captures mounts, network settings, ports, and environment variables

### Non-RPM Software

- **readelf-based binary classification**: detects Go (`.note.go.buildid`), Rust (`.rustc`), and C/C++ binaries with static/dynamic linking and shared library enumeration
- **pip C extension detection**: identifies packages with `.so` files via RECORD inspection; triggers multi-stage Containerfile build
- **Python venv detection**: discovers venvs via `pyvenv.cfg`, flags `--system-site-packages`, scans dist-info and `pip list --path` for package inventories
- **pip dist-info scanning**: system-level pip packages with name and version
- **npm/yarn/gem lockfile detection**: captures lockfiles for reproducible installs
- **Git repository detection**: captures remote URL, branch, and commit hash for directories under `/opt` and `/usr/local`
- Optional deep binary strings scan for version extraction (`--deep-binary-scan`) with extended patterns for Go, Rust, OpenSSL, Java, Node, Python, and build metadata

### Kernel & Boot

- `/proc/cmdline` and GRUB defaults
- `lsmod` parsing with module classification: default (from `modules-load.d`), configured, dependency, or non-default
- Runtime sysctl values from `/proc/sys` diffed against shipped defaults in `/usr/lib/sysctl.d` with source attribution
- `modules-load.d`, `modprobe.d`, and `dracut.conf.d` config capture and COPY into image
- Locale, timezone, and alternatives detection via file-based methods (container-compatible): rendered in the Kernel/Boot tab as a system properties description list and alternatives table

### SELinux

- Mode detection (enforcing/permissive/disabled)
- Custom module discovery via `semodule -l` cross-referenced with priority-400 module store
- Non-default boolean identification via `semanage boolean -l` current vs. default comparison
- Audit rules and `fcontext` capture

### Users & Groups

- Non-system users and groups (1000 <= UID/GID < 60000)
- Raw `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow` entry capture
- Strategy-aware provisioning: each user is assigned one of four strategies based on account type — `sysusers` (service accounts, created at boot via systemd-sysusers), `useradd` (ambiguous accounts, explicit `RUN useradd` in Containerfile), `kickstart` (human users, deferred to deploy time), or `blueprint` (bootc-image-builder TOML). Override with `--user-strategy` to apply a single strategy to all users.
- `/etc/subuid` and `/etc/subgid` for rootless container mappings
- SSH authorized key references (paths only, not key material — never baked into image)
- Sudoers rules capture with FIXME guidance in Containerfile
- Home directory detection

## Containerfile Layer Ordering

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

## Baseline Generation

The tool generates a package baseline by querying the target **bootc base image** directly. It detects the host OS from `/etc/os-release`, maps it to the corresponding base image, and runs `podman run --rm <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete package list. The diff against host packages produces exactly the `dnf install` list the Containerfile needs.

**Supported OS to base image mappings:**

| Source Host | Target Base Image | Notes |
|-------------|-------------------|-------|
| RHEL 9.x | `registry.redhat.io/rhel9/rhel-bootc:{version}` | Version clamped to 9.6 minimum (first bootc release) |
| RHEL 10.x | `registry.redhat.io/rhel10/rhel-bootc:{version}` | |
| CentOS Stream 9 | `quay.io/centos-bootc/centos-bootc:stream9` | |
| CentOS Stream 10 | `quay.io/centos-bootc/centos-bootc:stream10` | |
| Fedora | `quay.io/fedora/fedora-bootc:{major}` | Version clamped to 41 minimum |

**Source/target version separation:** The source host (what you're inspecting) and the target image (your Containerfile's FROM line) can differ. A RHEL 9.4 host auto-targets `rhel-bootc:9.6` (the minimum bootc release). Override with `--target-version 9.8` or `--target-image` for full control. Cross-major-version migrations (e.g. RHEL 9 to 10) produce a prominent warning since package names, services, and config formats may differ.

**RHEL registry authentication:** RHEL base images on `registry.redhat.io` require authentication. The tool checks for credentials before attempting to pull and will exit with instructions if credentials are missing. Run `sudo podman login registry.redhat.io` on the host before running inspectah, or use `--baseline-packages FILE` as an alternative. CentOS Stream and Fedora images are on public registries and need no authentication.

When running inside a container, the tool uses `nsenter` to execute `podman` in the host's namespaces. This requires `sudo`, `--pid=host`, and `--privileged` on the outer container. Before attempting `nsenter`, the tool runs a fast probe to detect rootless containers and missing capabilities, and provides specific guidance if the probe fails.

**Fallback behavior:**

- **Base image queryable** — accurate package diff, only truly operator-added packages appear in the Containerfile
- **Base image not available** (not pulled, auth failure, or `--skip-preflight` used without proper flags) — enters "all-packages mode" where every installed package is treated as operator-added (no baseline subtraction), with a clear warning in the reports
- **Air-gapped environments** — use `--baseline-packages FILE` to provide a newline-separated list of package names, bypassing the podman query

The resolved baseline (including the base image package list) is cached in the inspection snapshot, so `--from-snapshot` re-renders work without network access or podman.

## Running the Container Directly

The pre-built image is published to GHCR on every push to `main`:

```
ghcr.io/marrusl/inspectah:latest
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
  ghcr.io/marrusl/inspectah:latest
```

To build locally:

```bash
podman build -t inspectah .
```

**Required flags:**

| Flag | Why |
|------|-----|
| `sudo` (rootful) | nsenter into host namespaces requires real `CAP_SYS_ADMIN` — rootless podman runs in a user namespace where this is impossible |
| `--pid=host` | Exposes the host PID namespace so the tool can reach PID 1 and run `podman` on the host via `nsenter` |
| `--privileged` | Grants the full capability set including `CAP_SYS_ADMIN` for `nsenter` and broad filesystem access |
| `--security-opt label=disable` | Disables SELinux label enforcement so the container can read all host paths |

The tool needs broad read access across the host filesystem — the container is a packaging convenience, not a security boundary.
