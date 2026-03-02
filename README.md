# yoinkc

Inspect package-based RHEL, CentOS Stream, and Fedora hosts and produce bootc image artifacts (Containerfile, config tree, audit report, etc.).

## Architecture

- **Inspectors** run against a host root (default `/host`) and produce structured JSON (the inspection snapshot).
- **Renderers** consume the snapshot and produce output artifacts (Containerfile, markdown report, HTML report, etc.).

## Usage

All renderers write to the **output directory**, which is created if it does not exist. Default: `./output`.

```bash
# Inspect host mounted at /host, write to default ./output
yoinkc

# Specify output directory
yoinkc --output-dir ./my-output
# or: yoinkc -o ./my-output

# Save snapshot only (no render)
yoinkc --inspect-only -o ./out

# Render from existing snapshot
yoinkc --from-snapshot ./out/inspection-snapshot.json -o ./rendered
```

## Development

```bash
pip install -e .
yoinkc --help
pytest
```

## Quick start

The fastest way to run yoinkc on a host. Downloads the run script and pulls the pre-built image from GHCR:

```bash
curl -fsSL https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh | sudo sh
```

Output goes to `./yoinkc-output` by default, plus a hostname-stamped tarball for easy collection. To specify a different output directory:

```bash
curl -fsSL -o run-yoinkc.sh https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh
sudo sh run-yoinkc.sh /path/to/output
```

> **Important:** `sudo` must wrap `sh`, not `curl`. The container requires rootful podman — if `sudo` only applies to the download, podman runs rootless and nsenter into host namespaces will fail.

## Container

The pre-built image is published to GHCR on every push to `main`:

```
ghcr.io/marrusl/yoinkc:latest
```

Multi-arch (amd64 + arm64). To run it directly:

```bash
sudo podman run --rm \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -v /:/host:ro \
  -v ./output:/output:z \
  ghcr.io/marrusl/yoinkc:latest --output-dir /output
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

After the run, the output directory contains the Containerfile, config tree, reports, and snapshot. You can then copy that directory off the host or push it to GitHub with `--push-to-github`. The HTML report (`report.html`) is **self-contained and portable**: all content is embedded, so you can share or archive that file alone.

---

## Inspectors

Each inspector examines one aspect of the host and contributes a section to the inspection snapshot.

### RPM / Packages

- Full package inventory via `rpm -qa` with epoch/version/release/arch
- Baseline from the target **bootc base image** — queries the image directly via `podman run` to get its package list, then diffs against installed packages to identify what the operator added
- Modified config detection via `rpm -Va` with verification flags
- Unowned file detection using bulk `rpm -qla` set subtraction (fast, avoids per-file lookups)
- `dnf history` analysis for packages that were installed then removed (orphaned configs)
- Repo file capture from `/etc/yum.repos.d/`
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
- Optional `--config-diffs`: retrieves RPM defaults from local cache or downloads from repos, generates unified diffs

### Network

- NetworkManager connection profiles classified as **static** (bake into image) or **DHCP** (kickstart at deploy)
- Firewalld zone parsing: services, ports, and rich rules from zone XML
- Firewalld direct rules from `direct.xml`
- `resolv.conf` provenance detection: systemd-resolved, NetworkManager-managed, or hand-edited
- `ip route` and `ip rule` capture with default rule filtering
- `/etc/hosts` additions and proxy settings
- Containerfile emits both COPY directives and `firewall-offline-cmd` equivalents for firewall rules
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

### SELinux

- Mode detection (enforcing/permissive/disabled)
- Custom module discovery via `semodule -l` cross-referenced with priority-400 module store
- Non-default boolean identification via `semanage boolean -l` current vs. default comparison
- Audit rules and `fcontext` capture

### Users & Groups

- Non-system users and groups (1000 <= UID/GID < 60000)
- Raw `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/gshadow` entry capture for append-based provisioning
- `/etc/subuid` and `/etc/subgid` for rootless container mappings
- SSH authorized key references (paths only, not key material)
- Sudoers rules capture with FIXME guidance in Containerfile
- Home directory detection

---

## Output Artifacts

```
output/
├── Containerfile                 # Layered image definition (cache-optimized layer order)
├── README.md                     # Build/deploy commands, FIXME checklist
├── audit-report.md               # Detailed findings with storage migration plan
├── report.html                   # Self-contained interactive HTML dashboard
├── secrets-review.md             # Redacted sensitive content for operator review
├── kickstart-suggestion.ks       # Deploy-time config (DHCP, DNS, credentials)
├── inspection-snapshot.json      # Raw structured data (re-renderable via --from-snapshot)
├── config/                       # Files to COPY into the image
│   ├── etc/                      # Mirrors /etc — modified configs, repos, firewall, timers
│   │   ├── httpd/conf/           # Modified RPM-owned configs
│   │   ├── firewalld/zones/      # Firewall zone definitions
│   │   ├── systemd/system/       # Local timer units (cron-converted + operator)
│   │   ├── sysctl.d/             # Non-default sysctl values
│   │   ├── modules-load.d/       # Kernel module configs
│   │   ├── tmpfiles.d/           # /var directory structure for bootc bootstrap
│   │   └── ...
│   ├── opt/                      # Non-RPM software (venvs, npm apps, binaries)
│   ├── usr/                      # Files under /usr/local
│   └── tmp/                      # User/group .append fragments (separate from etc/)
└── quadlet/                      # Container workload unit files
```

---

## CLI Reference

### Core Options

| Flag | Description |
|------|-------------|
| `--host-root PATH` | Root path for host inspection (default: `/host`) |
| `-o, --output-dir DIR` | Output directory for all artifacts (default: `./output`) |
| `--from-snapshot PATH` | Skip inspection; load snapshot from file and run renderers only |
| `--inspect-only` | Run inspectors and save snapshot; do not run renderers |

### Target Image

| Flag | Description |
|------|-------------|
| `--target-version VERSION` | Target bootc image version (e.g. `9.6`, `10.2`). Default: source host version, clamped to minimum bootc-supported release (9.6 for RHEL 9) |
| `--target-image IMAGE` | Full target bootc base image reference (e.g. `registry.redhat.io/rhel10/rhel-bootc:10.2`). Overrides `--target-version` and all automatic mapping |

### Inspection Options

| Flag | Description |
|------|-------------|
| `--baseline-packages FILE` | Path to a newline-separated package list for air-gapped environments where the base image cannot be queried via podman |
| `--config-diffs` | Generate line-by-line diffs for modified configs via `rpm2cpio` (retrieves from local cache or downloads from repos) |
| `--deep-binary-scan` | Full `strings` scan on unknown binaries with extended version pattern matching (slow) |
| `--query-podman` | Connect to podman to enumerate running containers with full inspect data |
| `--skip-preflight` | Skip container privilege checks (rootful, `--pid=host`, `--privileged`, SELinux) |

### Output Options

| Flag | Description |
|------|-------------|
| `--validate` | After generating output, run `podman build` to verify the Containerfile |
| `--push-to-github REPO` | Push output directory to a GitHub repository (e.g. `owner/repo`) |
| `--github-token TOKEN` | GitHub personal access token for repo creation (falls back to `GITHUB_TOKEN` env var) |
| `--public` | When creating a new GitHub repo, make it public (default: private) |
| `--yes` | Skip interactive confirmation prompts |

---

## Containerfile Layer Ordering

The generated Containerfile follows a deliberate layer order optimized for build cache efficiency — layers that change least frequently come first:

1. **Build stage** (conditional) — multi-stage build for pip packages with C extensions: installs build deps, compiles wheels
2. **Base image** — auto-detected from `/etc/os-release`, mapped to the corresponding bootc base image
3. **Repo files** — custom yum/dnf repositories
4. **Packages** — `dnf install` for packages added beyond the base image
5. **Services** — `systemctl enable/disable` based on base image preset diff
6. **Firewall** — zone XML files via `COPY` plus commented `firewall-offline-cmd` equivalents
7. **Scheduled tasks** — timer units (local + cron-converted with actual commands), at job FIXMEs
8. **Config files** — all captured configs via `COPY config/` with optional diff summaries
9. **Non-RPM software** — provenance-aware: `pip install` for pip, multi-stage for C extensions, `npm ci` for npm, FIXME for Go/Rust binaries, `git clone` comments for git repos
10. **Container workloads** — quadlet units
11. **Users & groups** — append-based provisioning (raw entries from `/etc/passwd`, `/etc/shadow`, etc.), sudoers FIXME, SSH key injection guidance
12. **Kernel** — sysctl overrides, module notes
13. **SELinux** — booleans, custom modules
14. **Network** — static connection profiles, `/etc/hosts` additions, proxy env vars, static route guidance
15. **tmpfiles.d** — transient file/directory setup

---

## Baseline Generation

The tool generates a package baseline by querying the target **bootc base image** directly. It detects the host OS from `/etc/os-release`, maps it to the corresponding base image, and runs `podman run --rm <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete package list. The diff against host packages produces exactly the `dnf install` list the Containerfile needs.

**Supported OS → base image mappings:**

| Source Host | Target Base Image | Notes |
|-------------|-------------------|-------|
| RHEL 9.x | `registry.redhat.io/rhel9/rhel-bootc:{version}` | Version clamped to 9.6 minimum (first bootc release) |
| RHEL 10.x | `registry.redhat.io/rhel10/rhel-bootc:{version}` | |
| CentOS Stream 9 | `quay.io/centos-bootc/centos-bootc:stream9` | |
| CentOS Stream 10 | `quay.io/centos-bootc/centos-bootc:stream10` | |
| Fedora | `quay.io/fedora/fedora-bootc:{major}` | Any Fedora version |

**Source/target version separation:** The source host (what you're inspecting) and the target image (your Containerfile's FROM line) can differ. A RHEL 9.4 host auto-targets `rhel-bootc:9.6` (the minimum bootc release). Override with `--target-version 9.8` or `--target-image` for full control. Cross-major-version migrations (e.g. RHEL 9 → 10) produce a prominent warning since package names, services, and config formats may differ.

**RHEL registry authentication:** RHEL base images on `registry.redhat.io` require authentication. The tool checks for credentials before attempting to pull and will exit with instructions if credentials are missing. Run `sudo podman login registry.redhat.io` on the host before running yoinkc, or use `--baseline-packages FILE` as an alternative. CentOS Stream and Fedora images are on public registries and need no authentication.

When running inside a container, the tool uses `nsenter` to execute `podman` in the host's namespaces. This requires `sudo`, `--pid=host`, and `--privileged` on the outer container (see the run command above). Before attempting `nsenter`, the tool runs a fast probe to detect rootless containers and missing capabilities, and provides specific guidance if the probe fails.

**Fallback behavior:**

- **Base image queryable** — accurate package diff, only truly operator-added packages appear in the Containerfile
- **Base image not available** (not pulled, auth failure, or `--skip-preflight` used without proper flags) — enters "all-packages mode" where every installed package is treated as operator-added (no baseline subtraction), with a clear warning in the reports
- **Air-gapped environments** — use `--baseline-packages FILE` to provide a newline-separated list of package names, bypassing the podman query

The resolved baseline (including the base image package list) is cached in the inspection snapshot, so `--from-snapshot` re-renders work without network access or podman.
