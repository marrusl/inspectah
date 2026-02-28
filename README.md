# yoinkc

Yoink the essentials from package-based CentOS/RHEL hosts and produce bootc image artifacts (Containerfile, config tree, audit report, etc.).

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

## Container

Build the tool image:

```bash
podman build -t yoinkc .
```

Run it against a host. **Typically you run the container on the host you are inspecting**, so both the host root and the output directory are bind-mounted from that same host. The tool reads the host via `/host` and writes artifacts to `--output-dir`; with the mount below, those artifacts end up on the host at `./output`.

```bash
sudo podman run --rm \
  --pid=host \
  --security-opt label=disable \
  -v /:/host:ro \
  -v ./output:/output:z \
  yoinkc --output-dir /output
```

> **Note:** `--pid=host` allows the tool to reach the host's `podman` (via `nsenter`) for querying the bootc base image package list. Without it, baseline generation falls back to all-packages mode. `--security-opt label=disable` turns off SELinux label enforcement. The tool needs broad read access across the host filesystem — the container is a packaging convenience, not a security boundary.

After the run, `./output` on the host contains the Containerfile, config tree, reports, and snapshot. You can then copy that directory off the host or push it to GitHub with `--push-to-github`. The HTML report (`report.html`) is **self-contained and portable**: all content is embedded, so you can share or archive that file alone.

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
- Static routes, proxy env vars, and `/etc/hosts` additions rendered in both Containerfile and kickstart

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
- `modules-load.d`, `modprobe.d`, and dracut configuration capture

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

| File | Description |
|------|-------------|
| `Containerfile` | Layered image definition with correct ordering for cache efficiency |
| `config/` | Config file tree preserving original paths, ready for `COPY` |
| `quadlet/` | Quadlet unit files for container workloads |
| `audit-report.md` | Detailed markdown report with triage breakdown and storage migration plan |
| `report.html` | Self-contained interactive HTML dashboard with syntax-highlighted config diffs |
| `README.md` | Build instructions, `podman build` command, `bootc switch` command, FIXME checklist |
| `kickstart-suggestion.ks` | Kickstart fragment for deploy-time config (DHCP, static routes, NFS credentials, proxy) |
| `secrets-review.md` | List of redacted sensitive content for operator review |
| `inspection-snapshot.json` | Full structured snapshot (re-renderable with `--from-snapshot`) |

---

## CLI Reference

### Core Options

| Flag | Description |
|------|-------------|
| `--host-root PATH` | Root path for host inspection (default: `/host`) |
| `-o, --output-dir DIR` | Output directory for all artifacts (default: `./output`) |
| `--from-snapshot PATH` | Skip inspection; load snapshot from file and run renderers only |
| `--inspect-only` | Run inspectors and save snapshot; do not run renderers |

### Inspection Options

| Flag | Description |
|------|-------------|
| `--baseline-packages FILE` | Path to a newline-separated package list for air-gapped environments where the base image cannot be queried via podman |
| `--config-diffs` | Generate line-by-line diffs for modified configs via `rpm2cpio` (retrieves from local cache or downloads from repos) |
| `--deep-binary-scan` | Full `strings` scan on unknown binaries with extended version pattern matching (slow) |
| `--query-podman` | Connect to podman to enumerate running containers with full inspect data |

### Output Options

| Flag | Description |
|------|-------------|
| `--validate` | After generating output, run `podman build` to verify the Containerfile |
| `--push-to-github REPO` | Push output directory to a GitHub repository (e.g. `owner/repo`) |
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

The tool generates a package baseline by querying the target **bootc base image** directly. It detects the host OS from `/etc/os-release`, maps it to the corresponding base image (RHEL 9.x to `registry.redhat.io/rhel9/rhel-bootc:9.x`, CentOS Stream 9 to `quay.io/centos-bootc/centos-bootc:stream9`), and runs `podman run --rm <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete package list. The diff against host packages produces exactly the `dnf install` list the Containerfile needs.

When running inside a container, the tool uses `nsenter` to execute `podman` in the host's namespaces. This requires `--pid=host` on the outer container (see the run command above). If `--pid=host` is not set, the tool falls back gracefully to all-packages mode.

**Fallback behavior:**

- **Base image queryable** — accurate package diff, only truly operator-added packages appear in the Containerfile
- **Base image not available** (no `--pid=host`, or base image not pulled) — enters "all-packages mode" where every installed package is treated as operator-added (no baseline subtraction), with a clear warning in the reports
- **Air-gapped environments** — use `--baseline-packages FILE` to provide a newline-separated list of package names, bypassing the podman query

The resolved baseline (including the base image package list) is cached in the inspection snapshot, so `--from-snapshot` re-renders work without network access or podman.
