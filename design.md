# Tool Design: yoinkc

## Runtime Model

A container run with `--pid=host --privileged --security-opt label=disable -v /:/host:ro` plus a writable output mount that inspects the host via `/host`. The container is a packaging convenience — the tool needs full read access to the host filesystem, so SELinux label enforcement is disabled.

`--pid=host` and `--privileged` are required for baseline generation: the tool uses `nsenter -t 1 -m -u -i -n` to execute `podman` in the host's namespaces, querying the target bootc base image for its package list and systemd presets. Without these flags, the tool still runs but falls back to all-packages mode (no baseline subtraction). See [Baseline Generation](#baseline-generation) for details.

This is clean — no contamination of the source system, no installation required, and it naturally separates the tool from the thing being inspected.

Output goes to a mounted volume that becomes either local files, a local git repo, or gets pushed to GitHub via the API.

The tool itself ships as a container: `ghcr.io/marrusl/yoinkc:latest`.

## Architecture

The tool is structured as a pipeline of **inspectors** that each produce structured JSON, fed into **renderers** that produce the output artifacts. That separation is important — it means you can re-render from a saved inspection snapshot without re-running against the host, and you can test renderers in isolation.

### Inspector Modules

#### RPM Inspector

`rpm -qa --queryformat` to get the full package list with epoch/version/release/arch, then diff against the package set from the target bootc base image (see [Baseline Generation](#baseline-generation)). Identifies: added packages (present on host but not in base image), removed packages (present in base image but not on host — rare but possible), and modified package configs via `rpm -Va`.

Also captures repo definitions from `/host/etc/yum.repos.d/` and `/host/etc/dnf/`.

Additionally checks `dnf history` for packages that were installed and later removed, since these may have left behind config files or state that still affects the system.

#### Service Inspector

Uses `systemctl --root=/host` as the preferred detection method for accurate service state (enabled, disabled, masked, static). This leverages systemd's own logic and avoids reimplementing preset evaluation. Falls back to filesystem-based scanning when `systemctl --root` is unavailable or returns errors (e.g., systemd version mismatches between the tool container and host): scans `.wants/` symlinks in `/host/etc/systemd/system/` for enabled services, symlinks to `/dev/null` for masked services, and parses `[Install]` sections in vendor units under `/host/usr/lib/systemd/system/` for disabled/static classification.

Diffs enabled/disabled/masked state against the defaults from systemd preset files in the base image.

#### Config Inspector

Three passes:

1. **RPM-owned files that have been modified** (from `rpm -Va`) — with optional diffs against RPM defaults (see below)
2. **Files in `/etc` not owned by any RPM** — these are the hand-placed configs that would otherwise be lost entirely
3. **Config files from packages that were installed and later removed** (cross-referenced with `dnf history` from the RPM inspector) — orphaned configs that may still affect system behavior

The second category is particularly important. These are the files that represent the actual identity of the system beyond its package set.

**Unowned file detection (optimized):**

Rather than running `rpm -qf` per-file across `/etc` (which is O(n) RPM database lookups and painfully slow on large systems), the inspector builds a set of all RPM-owned paths in a single pass via `rpm -qla` and then diffs that set against the actual filesystem listing. This reduces thousands of individual RPM queries to one bulk query plus a set subtraction — typically seconds instead of minutes.

A maintainable exclusion list filters out known system-generated files that are not operator-placed configs. The lists cover machine identity files, systemd state, PKI/subscription certs, SELinux policy store, PAM base configs, package manager state, installer artifacts, and more. See `src/yoinkc/inspectors/config.py` for the full current list.

The exclusion list is defined as two data structures (exact paths and glob patterns) at the top of the config inspector source, making it easy to extend as new false positives are discovered.

**Diff against RPM defaults (opt-in: `--config-diffs`):**

When enabled, for every modified RPM-owned config file, the inspector extracts the original package-shipped version via `rpm2cpio` + `cpio` from the installed RPM and produces a unified diff against the current file on disk. This means the output shows *what the operator actually changed* (e.g., "line 47: `MaxClients 256` → `MaxClients 1024`") rather than just "this file was modified."

Without this flag, modified RPM-owned configs are captured as full files with a note that they differ from the package default (based on `rpm -Va` output), but no diff is generated. The audit report and HTML report will show the `rpm -Va` verification flags (size, mtime, checksum, etc.) so operators still know *how* the file differs at a high level.

When `--config-diffs` is enabled:

RPM retrieval strategy:
1. Check the local dnf cache on the host (`/host/var/cache/dnf/`) — the original RPM is often still there.
2. If not cached, attempt to download the exact installed NEVRA from the configured repos (this requires network access from the tool container and working repo credentials).
3. If the RPM cannot be retrieved (offline host, repo no longer available, package from a decommissioned repo), fall back to capturing the full file with a `# NOTE: could not retrieve RPM default for diff — full file included` comment. The audit report lists these cases so the operator knows which files need manual comparison.

The diffs are stored in the inspection snapshot and rendered in both the markdown audit report (as fenced diff blocks) and the HTML report (as a side-by-side or unified diff view with syntax highlighting). This gives operators the context they need to decide whether each change is still relevant for the bootc target or can be dropped.

For the Containerfile, modified RPM-owned configs are always included as full-file COPYs. When `--config-diffs` is enabled, each COPY gets a comment summarizing the diff:

```dockerfile
# Modified from httpd-2.4.57 default:
#   - MaxClients: 256 → 1024
#   - ServerName: added (localhost:443)
#   - SSLProtocol: restricted to TLSv1.2+
# See audit-report.md or report.html for full diff
COPY config/etc/httpd/conf/httpd.conf /etc/httpd/conf/httpd.conf
```

This makes Containerfile review dramatically faster — operators can see at a glance whether a config change is intentional and relevant without having to manually diff each file.

#### Network Inspector

Captures the full network identity of the system:

- **NetworkManager profiles**: connection files from `/etc/NetworkManager/system-connections/` and `/etc/sysconfig/network-scripts/` (legacy). Classifies each as static config (method=manual, bake into image) vs. DHCP/dynamic (method=auto, defer to kickstart).
- **Firewall rules**: `firewalld` zones, services, rich rules, and direct rules. Parsed from zone XML files and `/etc/firewalld/direct.xml`. Exported as both the raw XML files and as `firewall-cmd` commands in the Containerfile.
- **Static routes and policy routing**: `/etc/sysconfig/network-scripts/route-*`, `/etc/iproute2/`, and `ip rule`/`ip route` output. Default policy routing tables (local, main, default) are filtered out.
- **DNS configuration**: `/etc/resolv.conf` provenance detection — symlink to `/run/systemd/resolve/` (systemd-resolved), "Generated by NetworkManager" comment (NM-managed), or plain file (hand-edited). `/etc/hosts` additions beyond localhost.
- **Proxy configuration**: system-wide proxy settings from `/etc/environment`, `/etc/profile.d/`, dnf proxy configs. DNF proxy configuration from `/etc/dnf/dnf.conf`.

The audit report and HTML report distinguish between network config baked into the image (firewall rules, static interface definitions) and config deferred to deployment time via kickstart (DHCP interfaces, site-specific DNS, hostname). The HTML report uses color-coded deployment labels (green for "bake into image", yellow for "kickstart at deploy"). The Containerfile COPYs static NM profiles and marks DHCP connections as kickstart-deferred with a FIXME. The kickstart suggestion file emits actual `network --bootproto=dhcp` directives.

#### Storage Inspector

Does **not** attempt to reproduce storage config in the image, but captures it comprehensively in the audit report for migration planning:

- **Mount points**: `/etc/fstab` entries, currently mounted filesystems.
- **Automount maps**: `/etc/auto.master` and `/etc/auto.*` files for automounted filesystems.
- **LVM layout**: volume groups, logical volumes, their sizes and mount points. LVM config files and profiles from `/etc/lvm/`.
- **NFS/CIFS mounts**: remote filesystem dependencies, including credential references. Extracts credential file references from fstab mount options (`credentials=`, `password_file=`).
- **Block device configuration**: multipath, iSCSI initiator config, device-mapper entries. Detects dm-crypt devices via `dmsetup table`.
- **`/var` directory scanning**: scans `/var/lib`, `/var/log`, `/var/data`, `/var/www`, `/var/opt` with size estimates and recommendations for the data migration plan.

The audit report gets a dedicated "Storage Migration Plan" section that maps each mount point to a recommended approach: image-embedded (for small, static data), PVC/volume mount (for application data), external storage service (for shared filesystems).

#### Scheduled Task Inspector

Captures and converts all scheduled execution:

- **Cron jobs**: `/etc/crontab`, `/etc/cron.d/*`, `/var/spool/cron/*` (per-user crontabs), `/etc/cron.{hourly,daily,weekly,monthly}/` scripts.
- **Systemd timers**: existing `.timer` units from both `/etc/systemd/system/` (local/operator) and `/usr/lib/systemd/system/` (vendor), with their associated `.service` units. Local and vendor timers are labeled separately in the output.
- **At jobs**: pending `at` jobs from `/var/spool/at/`, parsed to extract the scheduled command, user, and working directory.

For the Containerfile output, cron jobs are converted to systemd timer units (since bootc images should use systemd timers as the canonical scheduling mechanism). Each generated timer includes a comment with the original cron expression for reference. Local timers get COPY + `systemctl enable` directives, vendor timers get a comment noting they're already in the base image. At jobs get FIXME comments urging conversion to systemd timers. Jobs that can't be cleanly converted (e.g., per-user crontabs with environment variable dependencies) get a `# FIXME` comment and appear in the audit report.

#### Container Inspector

Discovers container workloads through a fast file-based scan:

1. **Quadlet units**: `/etc/containers/systemd/`, `/usr/share/containers/systemd/`, and `/etc/systemd/system/` — these are the primary source of truth and are always captured. Also scans user-level quadlets at `~/.config/containers/systemd/` for real users (UID 1000–59999) via targeted path lookup (no recursive home directory traversal). Parses `Image=` from the `[Container]` section (handles whitespace and case variations).
2. **Compose files**: podman-compose and docker-compose files found via search across `/opt`, `/srv`, `/etc`. Parses `image:` fields to extract image references per service.
3. **Container image references**: extracted from both quadlet units and compose files and displayed in the audit report.

**Live container query (opt-in: `--query-podman`):**

When enabled, runs `podman ps -a --format json` to enumerate containers, then `podman inspect` on each to capture mounts, network settings, ports, and environment variables. This captures runtime state that may not be reflected in the unit files (e.g., containers started manually, containers with runtime overrides). Without this flag, the inspector relies entirely on the file-based scan, which is faster and covers the vast majority of cases since production workloads should be defined in unit files.

Translates to quadlet in the output since that's the bootc-blessed pattern. Docker-compose files get a `# FIXME: converted from docker-compose, verify quadlet translation` comment.

#### Non-RPM Software Inspector

This is the hardest inspector and the one most likely to produce incomplete or incorrect results. The tool is honest about this — it defaults to `# FIXME` comments rather than guessing wrong.

**Detection approach:**

Scans `/opt`, `/srv`, and `/usr/local` (not `/home` — user home directories are not scanned to avoid noise). For FHS directories under `/usr/local` (`bin`, `sbin`, `libexec`, `lib`, `lib64`), files are enumerated and classified individually rather than treated as opaque directories. `/usr/local/lib` recurses one level into subdirectories.

Check for embedded package metadata (`package.json`, `*.dist-info`, `METADATA`, `setup.py`, installer logs in common locations). Check if directories have their own `.git` history (captures remote URL and current commit hash).

**Language-specific package managers:**

- **pip**: `pip list --path` against known venv and system paths. Discovers venvs by finding `pyvenv.cfg` under `/opt` and `/usr/local`. Captures `requirements.txt` or `pip freeze` output where possible. Flags venvs created with `--system-site-packages` (detected via `include-system-site-packages = true` in `pyvenv.cfg`) as needing manual review since their dependency resolution is entangled with system packages.
- **npm**: scan for `node_modules` with `package-lock.json` or `yarn.lock`. Captures the lockfile for reproducibility.
- **gem/bundler**: check for `Gemfile.lock`, system gem installs.

**Binary detection:**

For binaries without any package manager metadata, the tool runs a **fast classification pass** by default:

1. `file` command to identify binary type (ELF, script, etc.) and architecture.
2. For ELF binaries, `readelf -S` to check for `.note.go.buildid` section (Go) or `.rustc` debug section (Rust). `readelf -d` for dynamic linking info and shared library list. (Not `ldd`, which is slower and attempts to resolve.)
3. Check for a `--version` or `-V` flag by inspecting the binary's help text (`strings` limited to the first 4KB of the binary, not the full file — version strings in self-identifying binaries are almost always near the start).

This fast pass covers the common cases (Go/Rust identification, self-versioning binaries) without the cost of scanning entire large binaries. Requires `binutils` (for `readelf`) and `file` in the tool container.

**Deep binary scan (opt-in: `--deep-binary-scan`):**

When enabled, runs `strings` against the full binary content with regex matching against a conservative allowlist of version patterns (e.g., `X.Y.Z` preceded by `version`, `-v`, or the binary's own name). This is slow on large statically-linked binaries (a 200MB Go binary can take 10+ seconds) and has high false-positive potential, but may recover version info that the fast pass misses.

**Explicitly flagged as "manual intervention" categories:**

- Statically-linked Go and Rust binaries (increasingly common in `/usr/local/bin`, essentially opaque — no reliable way to determine provenance or version).
- Software installed from local wheels, private package indexes, or tarballs where the upstream source no longer exists.
- Anything installed via `curl | bash` or `wget && make install` with no manifest left behind.

For all unknown provenance software, the output is a `COPY` directive with a prominent `# FIXME: unknown provenance — determine upstream source and installation method` comment. The tool never guesses at an installation command it can't verify. Known-provenance items get real directives: `RUN pip install package==version` for pip packages, `COPY` for npm lockfiles, `# RUN git clone` comments for git-managed directories.

**Multi-stage build detection:**

If pip packages with C extensions are detected (identified by `.so` files in `*.dist-info` directories or build-dependency packages in the RPM list), the renderer offers a multi-stage Containerfile variant that separates the build environment from the final image.

#### Kernel/Boot Inspector

- `/proc/cmdline` and `/etc/default/grub` — kernel boot parameters
- Kernel modules: loaded (`lsmod`) diffed against defaults. Modules configured in `/etc/modules-load.d/` and `/usr/lib/modules-load.d/` are treated as expected. Modules loaded only as dependencies (non-empty `used_by` column) are filtered out. Only modules that are neither explicitly configured nor loaded as dependencies appear as non-default.
- Sysctl settings: reads runtime values from `/proc/sys/`, reads shipped defaults from `/usr/lib/sysctl.d/*.conf` (sorted by filename, matching systemd precedence), reads operator overrides from `/etc/sysctl.d/*.conf` and `/etc/sysctl.conf`. Only values where runtime differs from shipped default appear in output, with source attribution.
- Dracut configuration: `/etc/dracut.conf.d/`

#### SELinux/Security Inspector

- SELinux mode from `/host/etc/selinux/config`
- Custom policy modules: detected by scanning the priority-400 module store (`/host/etc/selinux/<type>/active/modules/400/`). Only modules at priority 400 (installed via `semodule -i`) are reported as custom.
- Boolean overrides: `chroot /host semanage boolean -l` returns current and default values in a single command. Non-default booleans are flagged. Falls back to reading `/host/sys/fs/selinux/booleans/*` (runtime values as current/pending pairs) if chroot fails.
- Audit rules from `/etc/audit/rules.d/`
- FIPS mode status
- Custom PAM configurations

#### User/Group Inspector

Non-system users and groups (1000 <= UID < 60000) from `/host/etc/passwd` and `/host/etc/group`. Captures raw entries from `passwd`, `shadow`, `gshadow`, `group`, `subuid`, and `subgid` for each qualifying user.

Captures: shell assignments, group memberships, sudoers rules, SSH authorized_keys references (flagged for manual handling, not copied).

For the Containerfile, generates `.append` fragment files (e.g., `config/tmp/passwd.append`) containing only non-system entries. These are written to `config/tmp/` rather than `config/etc/` so they are not swept up by the consolidated `COPY config/etc/ /etc/` — they need separate `COPY` + `cat >>` handling. The fragments are appended to the base image's files via `cat >> /etc/passwd` etc., preserving exact UIDs, encrypted passwords, and account flags. Home directories are created with correct ownership. Falls back to `useradd`/`groupadd` commands for snapshots without raw entries.

### Baseline Generation

The tool generates its package baseline by querying the target bootc base image directly. This answers the exact question the Containerfile needs to express: "what packages do I need to add to the base image to reproduce this system?"

**How it works:**

1. **Detect host identity.** Read `/host/etc/os-release` to determine distribution (RHEL, CentOS Stream, Fedora) and version.
2. **Select the target base image.** Map the detected host to the corresponding bootc base image:
   - RHEL 9.x → `registry.redhat.io/rhel9/rhel-bootc:{version}` (clamped to 9.6 minimum)
   - RHEL 10.x → `registry.redhat.io/rhel10/rhel-bootc:{version}`
   - CentOS Stream 9 → `quay.io/centos-bootc/centos-bootc:stream9`
   - CentOS Stream 10 → `quay.io/centos-bootc/centos-bootc:stream10`
   - Fedora → `quay.io/fedora/fedora-bootc:{major}` (clamped to 41 minimum)
3. **Query the base image package list.** Run `podman run --rm --cgroups=disabled <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete set of packages in the base image. Since the tool runs inside a container, it uses `nsenter -t 1 -m -u -i -n` to execute `podman` in the host's namespaces (this is why `--pid=host` and `--privileged` are required on the outer container). `--cgroups=disabled` avoids cgroup permission issues in the nsenter context. This is the authoritative baseline — it's the actual content of the image the Containerfile's `FROM` line references.
4. **Diff against host.** Compare the host's installed package names against the base image's package list. Packages on the host but not in the base image are "added" (go into `dnf install`). Packages in the base image but not on the host are "removed" (rare, noted in the audit report).
5. **Cache in the snapshot.** The base image package list is stored in `inspection-snapshot.json` so re-renders via `--from-snapshot` don't need to pull the image again.

**Source/target version separation:** The source host version can differ from the target image version. `--target-version` overrides the auto-detected version (e.g., migrating a RHEL 9.4 host to a 9.6 image), and `--target-image` overrides the base image reference entirely (useful for custom base images or testing). Cross-major migrations (e.g., RHEL 9 host → RHEL 10 image) produce a warning since package sets may differ significantly. RHEL images require `podman login registry.redhat.io` on the host before running the tool.

**Why this is better than comps XML:**

- **No dependency resolution needed.** Both sides are concrete package lists from real RPM databases. No comps XML parsing, no group resolution, no transitive dependency graphs.
- **No profile detection needed.** The baseline is the base image, not the install profile. It doesn't matter whether the host was originally installed as `@minimal` or `@server` — what matters is what's in the target image.
- **Directly answers the Containerfile question.** The diff produces exactly the `dnf install` list the Containerfile needs.
- **Always accurate.** The base image IS the baseline, by definition. No drift between a manifest and reality.
- **Works offline.** If the base image is already pulled (which it must be to build the Containerfile anyway), no network access is required for the baseline query.

**Service baseline:** Systemd preset files from the base image establish which services are enabled or disabled by default. These are queried from the same `podman run` or extracted from the image filesystem.

**Fallback:**

If the base image cannot be pulled or queried (air-gapped without a pre-pulled image, registry auth failure), the tool degrades gracefully:

```
WARNING: Could not query base image package list.
No baseline available — all installed packages will be included in the Containerfile.
To reduce image size, pull the base image first or provide a package list via --baseline-packages.
```

In this mode, the Containerfile includes all installed packages rather than just the delta. The tool still performs all other inspection — config files, services, containers, non-RPM software, etc. — so it remains useful even without a baseline. The `--baseline-packages` flag accepts a path to a newline-separated list of package names for environments where `podman run` isn't available but the package list can be obtained by other means.

The primary baseline is always the target bootc base image.

## Secret Handling

A dedicated redaction pass runs over all captured file contents **before any output is written or any git operations occur**. This ordering is critical — the redaction pass is not a separate stage that can be skipped, it's a gate that all content must pass through.

### Redaction Layers

**Pattern-based redaction:** Regex patterns for:
- API keys and tokens (AWS, GCP, Azure, GitHub, generic `API_KEY`/`TOKEN` patterns)
- Private key PEM blocks (`-----BEGIN.*PRIVATE KEY-----`)
- Password fields in config files (`password = ...`, `PASSWD=...`, `secret=...`)
- Connection strings with embedded credentials (JDBC, MongoDB, Redis, PostgreSQL URIs)
- Cloud provider credential files

Matched values are replaced with `REDACTED_<TYPE>_<hash>` where the hash is a truncated SHA-256 of the original value. This means you can tell redacted values apart without knowing them — useful for spotting "these three config files all use the same database password."

**Path-based exclusion:** These files are **never** included in content, only referenced in the audit report with a note that they need manual handling:
- `/etc/shadow`, `/etc/gshadow`
- `/etc/ssh/ssh_host_*` (host keys)
- `/etc/pki/` private keys
- TLS certificate private keys (`.key` files)
- Kerberos keytabs

**Flagging:** Anything redacted gets an entry in `secrets-review.md` listing:
- The file path
- The pattern that matched
- Line number (or "entire file" for excluded paths)
- Suggested remediation (e.g., "use a Kubernetes secret", "use a systemd credential", "inject via environment variable at deploy time")

### GitHub Push Guardrails

Pushing to GitHub means network egress from a container with read access to the entire host filesystem. Even with the redaction pass, this requires explicit safeguards:

1. **Explicit opt-in**: GitHub push is never a default. It requires `--push-to-github` with a repository target.
2. **Confirmation gate**: Before pushing, the tool prints a summary of what will be pushed — **total data size on disk**, file count, any `# FIXME` items, count of redacted values — and requires interactive confirmation (`--yes` to skip for automation, but this must be a conscious choice).
3. **Redaction verification**: The push step re-scans the entire output tree for known secret patterns as a second pass. If anything is found that the first pass missed, the push is aborted with an error.
4. **Private repo default**: If creating a new repo, it defaults to private. Creating a public repo requires `--public` flag.

## Output Artifacts

### Containerfile

Structured in a deliberate layer order to maximize cache efficiency:

```dockerfile
# === Base Image ===
FROM quay.io/centos-bootc/centos-bootc:stream9

# === Repos ===
COPY config/etc/yum.repos.d/ /etc/yum.repos.d/

# === Packages ===
RUN dnf install -y httpd postgresql-server ... && dnf clean all

# === Services ===
RUN systemctl enable httpd && systemctl disable kdump

# === Firewall ===
COPY config/etc/firewalld/ /etc/firewalld/

# === Scheduled Tasks (cron → systemd timers) ===
COPY config/etc/systemd/system/backup-daily.timer /etc/systemd/system/
RUN systemctl enable backup-daily.timer

# === Config Files (consolidated) ===
COPY config/etc/ /etc/

# === Non-RPM Software ===
RUN pip install flask==2.3.2
# FIXME: unknown provenance binary
COPY config/usr/local/bin/mytool /usr/local/bin/mytool

# === Containers (Quadlet) ===
COPY quadlet/ /etc/containers/systemd/

# === Users/Groups (from config/tmp/, not config/etc/) ===
COPY config/tmp/ /tmp/
RUN cat /tmp/passwd.append >> /etc/passwd && \
    cat /tmp/group.append >> /etc/group && \
    rm -f /tmp/*.append
RUN mkdir -p /home/appuser && chown 1001:1001 /home/appuser

# === Kernel ===
RUN rpm-ostree kargs --append=hugepagesz=2M

# === SELinux ===
COPY config/selinux/ /tmp/selinux/
RUN semodule -i /tmp/selinux/myapp.pp && rm -rf /tmp/selinux/
RUN setsebool -P httpd_can_network_connect on

# === Network ===
COPY config/etc/NetworkManager/system-connections/ /etc/NetworkManager/system-connections/
# FIXME: DHCP interfaces → kickstart

# === tmpfiles.d ===
COPY config/etc/tmpfiles.d/app-dirs.conf /etc/tmpfiles.d/
```

Each section has comments explaining what was detected and why it was included. `FIXME` comments mark anything that needs human review. See the README for the complete layer ordering documentation.

### Git Repo Layout

```
/
├── Containerfile
├── README.md                  # what was found, how to build, how to deploy
├── secrets-review.md          # everything redacted, needs manual handling
├── audit-report.md            # full human-readable findings
├── config/                    # files to COPY into the image
│   ├── etc/                   # mirrors /etc structure for modified configs
│   │   ├── firewalld/         # firewall zone/service definitions
│   │   ├── systemd/system/    # generated timer units (from cron conversion)
│   │   ├── tmpfiles.d/        # directory structure for /var
│   │   └── ...
│   ├── opt/                   # non-RPM software (where COPYed)
│   └── usr/
├── quadlet/                   # container unit files
├── kickstart-suggestion.ks     # suggested kickstart snippet for deploy-time settings
├── report.html                # interactive HTML report (open in browser)
└── inspection-snapshot.json   # raw inspector output, for re-rendering
```

### README.md

Includes:
- Summary of what was found on the source system (counts of packages, configs, services, FIXMEs, warnings)
- Exact `podman build` command to build the image
- Exact `bootc switch` or `bootc install` command to deploy (with the right flags for the detected scenario)
- List of `FIXME` items that need resolution before the image is production-ready
- Link to the audit report for full details

### Audit Report (audit-report.md)

A markdown document for version control and quick terminal reference. For the interactive view, see [HTML Report](#html-report-reporthtml) below.

Organized as:

1. **Executive Summary**: counts of packages added/removed, configs modified, containers found, secrets redacted, issues flagged. A clear triage: X items handled automatically, Y items handled with FIXME, Z items need manual intervention.

2. **Per-Inspector Sections** (each with tables):
   - RPM analysis (added, removed, modified packages)
   - Service state changes
   - Configuration changes (RPM-owned modified, unowned files)
   - Network configuration (what's baked vs. what should be kickstart)
   - Storage migration plan (mount points → recommended approach)
   - Scheduled tasks (existing timers by source, cron → timer conversions, at jobs)
   - Container workloads (quadlet units with images, compose files with service-to-image mappings)
   - Non-RPM software (compiled binaries with language/linking classification, venvs, git repos, unknown provenance items with confidence ratings)
   - Kernel and boot configuration (non-default modules, non-default sysctl values with source attribution)
   - SELinux customizations (custom modules, non-default booleans)
   - Users and groups

3. **Data Migration Plan** (`/var` problem): dedicated section listing everything found under `/var/lib`, `/var/log`, `/var/data` that looks like application state — databases, app data directories, log directories — with explicit notes on what can be seeded in the image (deployed only at initial bootstrap, never updated by bootc afterward) vs. what needs a separate migration strategy. The Containerfile generates `systemd-tmpfiles.d` snippets to ensure expected directory structures exist on every boot.

4. **Items Requiring Manual Intervention**: consolidated list pulled from all inspectors, prioritized by risk.

### HTML Report (report.html)

A single self-contained HTML file (inline CSS/JS, no external dependencies) that provides an interactive view of the inspection results. This is the primary artifact operators will use to understand what the tool found and what work remains.

**Layout:**

The report opens to a **dashboard view** with:

- **Status banner**: hostname, OS version, inspection timestamp, overall health with triage breakdown (X automatic, Y FIXME, Z manual).
- **Warning panel** (prominently placed, always visible at the top): all warnings, FIXMEs, and errors from the run, color-coded by severity (red for "needs manual intervention", amber for "handled with FIXME", blue for informational). Each warning is individually dismissable (dismissed warnings remain in the warnings tab). Dismiss All button collapses the panel. Warning count in status banner updates as warnings are dismissed.
- **Category cards**: one card per inspector area (Packages, Services, Config, Network, Storage, Scheduled Tasks, Containers, Non-RPM Software, Kernel/Boot, SELinux, Users/Groups). Each card shows a summary count and a status indicator (green checkmark if fully automated, amber if FIXMEs exist, red if manual intervention needed).

**Drill-down:**

Clicking any category card expands to the full detail view for that inspector:

- **Packages**: table of added, removed, and modified packages with version info.
- **Services**: table of state changes with columns for service name, current state, default state, and action taken in Containerfile.
- **Config files**: list of unowned and modified files. For modified RPM-owned files: if `--config-diffs` was used, shows the diff against the package default with syntax highlighting; otherwise, shows the full file with `rpm -Va` verification flags.
- **Network**: connections table with color-coded Deployment column (green "Bake into image" for static, yellow "Kickstart at deploy" for DHCP). Firewall zones with services, ports, rich rules, and direct rules. resolv.conf provenance with action guidance. Static routes and policy routing rules.
- **Non-RPM software**: compiled binaries with language badges (Go/Rust) and linking type. Python venvs with package lists and system-site-packages warnings. Git-managed directories with remote URLs. Unknown provenance items highlighted.
- **Secrets**: summary of all redacted items with file paths, pattern types, and remediation suggestions.

**Warnings section:**

Accessible both from the top-level warning panel and as a dedicated tab. Displays all warnings in a flat, searchable list with severity, source inspector, description, affected resource, and suggested action.

**Implementation:** Generated from `inspection-snapshot.json` by the renderer. The entire report is a single `.html` file (typically < 2MB) that can be opened in any browser, emailed, or served statically. No server required.

### Kickstart Suggestion File

A `kickstart-suggestion.ks` file containing suggested kickstart snippets for settings that belong at deploy time rather than in the image:
- DHCP network interfaces (`network --bootproto=dhcp ...`)
- Hostname (`network --hostname=...`)
- Site-specific DNS
- NFS mount credentials
- Any deployment-specific environment variables referenced in configs

This file is clearly marked as a **suggestion** — it needs to be reviewed and adapted for the target environment.

## The `/var` Problem — Explicitly Documented

bootc's contract is that `/var` content from the image is written to disk at initial bootstrap, but is **never updated by subsequent image deployments**. It becomes fully mutable state from that point forward. This means you *can* seed `/var` with initial directory structures and default data in the image, but anything that lives there is the operator's responsibility to manage, back up, and migrate going forward — bootc won't touch it again.

This has practical implications for the tool's output:

- `tmpfiles.d` snippets are generated to create expected directory structures (these run on every boot and are the right mechanism for ensuring directories exist).
- Small, static seed data (e.g., default config databases) *can* be included in the image and will land in `/var` on first install, but this is flagged with a `# NOTE: only deployed on initial bootstrap, not updated by bootc` comment.
- Application databases, runtime state, and log directories with significant data are **not** embedded. These appear in the audit report's "Data Migration Plan" section with explicit notes that they need a separate migration strategy.

This is called out prominently in:

1. The audit report's "Data Migration Plan" section
2. Comments in the Containerfile
3. The README's deployment instructions

## CLI Flags Summary

The default run is optimized for speed — it covers the vast majority of systems well without expensive operations. Opt-in flags enable deeper inspection at the cost of time and resources.

| Flag | Default | Effect |
|---|---|---|
| `--output-dir DIR` | `./output/` | Directory to write all output artifacts to. Created if it doesn't exist. |
| `--baseline-packages FILE` | off | Path to a newline-separated list of package names for air-gapped environments where the base image cannot be queried via podman. |
| `--validate` | off | After generating output, run `podman build` against the Containerfile to verify it builds successfully. Reports build errors with context so operators can fix issues before manual review. Requires `podman` on the host or in the tool container. |
| `--config-diffs` | off | Extract RPM defaults via `rpm2cpio` and generate line-by-line diffs for modified config files. Requires RPMs to be in local cache or downloadable from repos. |
| `--deep-binary-scan` | off | Run full `strings` scan on unknown binaries in `/opt` and `/usr/local` for version detection. Slow on large statically-linked binaries. |
| `--query-podman` | off | Connect to the podman socket to enumerate running containers and runtime state beyond what's in unit/compose files. |
| `--push-to-github REPO` | off | Push output to a GitHub repository. Requires confirmation (or `--yes`). Shows total data size before push. |
| `--public` | off | When creating a new GitHub repo, make it public instead of private. |
| `--yes` | off | Skip interactive confirmation prompts (for automation). |
| `--host-root PATH` | `/host` | Root path for host inspection. |
| `--from-snapshot PATH` | off | Skip inspection, load a previously saved snapshot and render only. |
| `--inspect-only` | off | Run inspectors only, save snapshot, skip renderers. |
| `--target-version VERSION` | auto-detected | Target bootc image version (clamped to minimum supported version per distro). |
| `--target-image IMAGE` | auto-selected | Full target base image reference (overrides `--target-version`). |
| `--skip-preflight` | off | Skip container privilege checks. |
| `--github-token TOKEN` | `GITHUB_TOKEN` env | GitHub PAT for repo creation (falls back to `GITHUB_TOKEN` environment variable). |

### Build Validation (`--validate`)

When enabled, the tool runs `podman build` against the generated Containerfile after all output artifacts are written. This catches a large class of errors before the operator spends time on manual review:

- Missing dependencies (a package referenced in `RUN dnf install` that doesn't exist in the configured repos)
- Broken COPY paths (a config file referenced in the Containerfile that wasn't written to the `config/` tree)
- Syntax errors in generated systemd units, timer files, or SELinux policy modules
- Base image pull failures (registry auth issues, wrong tag)

The build runs with `--no-cache` to ensure a clean test. On success, the tool reports the image ID and size. On failure, it captures the build log, appends a `build-errors.log` to the output, and adds a summary of failures to the HTML report's warning panel and the audit report.

The resulting image is not pushed or deployed — it's a local build test only. The operator is expected to review, refine, and rebuild before deployment.

Note: validation requires access to the host's podman (via `nsenter`, same mechanism as baseline generation). It also requires network access to pull the base image and install packages, so it won't work in fully air-gapped runs without a pre-pulled base image.

## Implementation Language

Python makes the most sense — it's available in UBI base images, has good libraries for all of this (`rpm` bindings, `GitPython`, `PyGithub`, `jinja2` for templating the Containerfile), and is readable enough that the heuristic logic in the non-RPM inspector is maintainable.

The tool container is based on Fedora and includes the inspection dependencies (`systemd` tools, `binutils` for `readelf`, `file` for binary type detection). `podman` is not installed in the tool container — it is accessed on the host via `nsenter` (see [Runtime Model](#runtime-model)). Target: `ghcr.io/marrusl/yoinkc:latest`.

## Future Work

The following are out of scope for the POC and v1 but represent the natural evolution of the tool:

**In-place migration.** The logical endpoint is a mode where the tool doesn't just generate artifacts — it applies them. The operational model is: run the tool against one representative host from a pool of identically-configured machines, generate and refine the Containerfile, build the image, then deploy that single image across the fleet via `bootc install-to-filesystem` or `system-reinstall-bootc`. The tool does not need to run against every host — that would produce a separate image per host, which defeats the purpose of image-based management. One image per role, deployed to many hosts, is the bootc model. Host-specific configuration (hostname, network, credentials) is applied at deploy time via kickstart or provisioning tooling. This is deliberately excluded from v1 because the tool's current value proposition is *safe and read-only* — it never touches the source system, which is what makes it trustworthy enough to run against production. The in-place migration mode should only be built once the read-only tool has been used across enough real systems to establish confidence in the accuracy of the generated Containerfiles.

**Fleet analysis mode.** For environments where hosts in the same role have drifted from each other over time, a mode that ingests multiple snapshots from the same nominal role, identifies the common base, and highlights per-host deviations. This helps operators decide which host is the most representative to use as the source for the golden image, and flags hosts that have diverged in ways that need reconciliation before fleet-wide deployment.

**Snapshot diffing and drift detection.** The structured inspection snapshot is independently valuable beyond migration. Diffing snapshots across hosts or across time enables configuration drift detection, compliance auditing, and fleet-wide inventory. A stable, well-documented snapshot schema is the foundation for this.

**Distribution support.** RHEL 9, RHEL 10, CentOS Stream 9, CentOS Stream 10, and Fedora are all supported. Future distro additions (e.g., RHEL 11) require only adding entries to the data-driven mapping tables in `baseline.py`.

**Enhanced cron-to-timer conversion.** Deeper semantic analysis of cron jobs to handle edge cases: `MAILTO` conversion to systemd journal notifications, `@reboot` entries mapped to oneshot services, `%` character handling, and environment variable inheritance differences.

**Config file semantic diffing.** Beyond line-level diffs (via `--config-diffs`), understanding the *meaning* of config changes — e.g., recognizing that a change to `MaxClients` in `httpd.conf` is a performance tuning decision vs. a change to `DocumentRoot` which is a structural decision. This would improve the audit report's guidance on which changes to keep vs. reconsider.
