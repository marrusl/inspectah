# Tool Design: yoinkc

## 1. Overview & Principles

yoinkc inspects package-based RHEL, CentOS Stream, and Fedora hosts and produces bootc migration artifacts: a Containerfile, a config tree mirroring operator-modified files, human-readable reports (markdown audit report, interactive HTML report, secrets review), and a structured JSON inspection snapshot that preserves every finding for re-rendering and fleet aggregation.

**Baseline subtraction.** The tool's output should represent operator intent, not system state. Wherever possible, the tool subtracts base-image defaults from the host's current state so that only operator-added or operator-modified items appear in the Containerfile and reports. Packages are diffed against the base image package list, services against base image presets, timers and cron jobs against RPM ownership, and kernel/SELinux configs against shipped defaults. Items that exist identically in the base image are omitted — they'll already be there. Items that reflect hardware-specific or host-specific state (autoloaded kernel modules, DHCP interfaces) are flagged as deploy-time concerns rather than baked into the image.

**Pipeline concept.** The tool follows a three-phase pipeline: **inspect** (inspectors query the host and produce structured data), **schema** (findings are merged into an `InspectionSnapshot` Pydantic model), and **render** (renderers consume the snapshot and produce output artifacts). This separation means you can re-render from a saved snapshot without re-running against the host, and you can test renderers in isolation.

**Companion tools.** Three companion entry points complete the workflow:

- **`yoinkc refine`** serves an interactive UI for editing findings — toggling packages in or out, changing user migration strategies, excluding config files — and re-rendering the Containerfile live.
- **`yoinkc-build`** builds a bootc container image from yoinkc output, with automatic RHEL subscription cert handling for building on non-RHEL hosts.
- **`yoinkc fleet`** aggregates inspections from multiple hosts into a single fleet snapshot, producing a merged Containerfile and report with prevalence annotations.

## 2. Architecture

### Module Map

The tool is structured as a pipeline orchestrated by `pipeline.py`:

```
__main__.py  →  pipeline.run_pipeline()
                    ├── inspectors.run_all(host_root) → InspectionSnapshot
                    │     └── 11 inspector modules, each populating a section
                    ├── redact_snapshot(snapshot)
                    ├── renderers.run_all(snapshot, output_dir)
                    │     └── 6 renderers: containerfile, audit_report,
                    │        html_report, readme, kickstart, secrets_review
                    └── create_tarball() or copy to output_dir
```

`__main__.py` parses CLI arguments, runs preflight privilege checks, then calls `run_pipeline()` with two callables: `run_inspectors` (wrapping `inspectors.run_all()` with CLI args) and `run_renderers` (wrapping `renderers.run_all()`). The pipeline either runs inspectors or loads a previously saved snapshot (`--from-snapshot`), applies secret redaction, renders output, bundles RHEL subscription certs if present, and packages the result as a tarball or directory.

The central data structure is `InspectionSnapshot` (defined in `schema.py`), a Pydantic model with typed fields for each inspection category. Every inspector populates its section of the snapshot; every renderer reads the complete snapshot. The snapshot is serialized to `inspection-snapshot.json` in the output, enabling offline re-rendering and fleet aggregation.

### Runtime Model

A container run with `--pid=host --privileged --security-opt label=disable -v /:/host:ro` that inspects the host via `/host` and produces a hostname-stamped tarball. The user's current directory is mounted at `/output` (`-w /output -v $(pwd):/output`) so the tarball lands directly where the user ran the command. The container is a packaging convenience — the tool needs full read access to the host filesystem, so SELinux label enforcement is disabled. No `:z` volume flag — `--security-opt label=disable` makes it unnecessary, and relabeling the user's CWD would risk corrupting host SELinux contexts.

`--pid=host` and `--privileged` are required for baseline generation: the tool uses `nsenter -t 1 -m -u -i -n` to execute `podman` in the host's namespaces, querying the target bootc base image for its package list and systemd presets. Without these flags, the tool still runs but falls back to all-packages mode (no baseline subtraction). See [Baseline Generation](#baseline-generation) for details.

This is clean — no contamination of the source system, no installation required, and it naturally separates the tool from the thing being inspected.

During inspection, the tool emits styled progress output to stderr using ANSI colours and step counters (e.g., `── [1/11] Packages ──`, `── [2/11] Config files ──`). Colours are suppressed automatically when stderr is not a TTY. The renderer phase emits a `Rendering output…` / `Done.` pair.

The default output is a tarball (`HOSTNAME-YYYYMMDD-HHMMSS.tar.gz`) containing the Containerfile, config tree, reports, snapshot, and RHEL subscription certs (if present on the inspected host). Use `--output-dir` for unpacked directory output, which is required for `--validate` and `--push-to-github`.

The tool itself ships as a container: `ghcr.io/marrusl/yoinkc:latest`.

### Executor

Inspectors never call `subprocess` directly. All command execution goes through the `Executor` protocol (`executor.py`), which defines a callable taking a command list and optional `cwd`, returning a `RunResult` (stdout, stderr, returncode).

The default implementation, `subprocess_executor`, runs commands via `subprocess.run` with a 300-second timeout, capturing stdout and stderr. `make_executor(host_root)` creates an executor pre-configured with `host_root` as the default working directory.

This abstraction exists primarily for testability: test suites inject fixture-based executors that return canned output instead of running real commands, allowing inspectors to be tested without a live host or container privileges.

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
3. **Verify registry credentials.** For `registry.redhat.io` images, run `podman login --get-login` via nsenter to verify the host has valid credentials before attempting the pull. On failure, emit an actionable error instructing the operator to run `sudo podman login registry.redhat.io` or provide `--baseline-packages FILE`. This prevents cryptic pull failures and gives a clear fix.
4. **Query the base image package list.** Run `podman run --rm --cgroups=disabled <base-image> rpm -qa --queryformat '%{NAME}\n'` to get the concrete set of packages in the base image. Since the tool runs inside a container, it uses `nsenter -t 1 -m -u -i -n` to execute `podman` in the host's namespaces (this is why `--pid=host` and `--privileged` are required on the outer container). `--cgroups=disabled` is passed because the query runs inside an already-containerized environment (via nsenter): cgroup v2 delegation is typically unavailable in the outer container's cgroup namespace, and the short-lived `rpm -qa` command needs no resource limits or cgroup tracking. `query_packages()` now captures full NEVRA (`epoch:name-version-release.arch`) and returns `Dict[str, PackageEntry]` keyed by `name.arch` for multi-arch correctness. `load_baseline_packages_file()` auto-detects NEVRA vs names-only format; when names-only baseline files are used, version comparison is gracefully skipped. This is the authoritative baseline — it's the actual content of the image the Containerfile's `FROM` line references.
5. **Diff against host.** Compare the host's installed package names against the base image's package list. Packages on the host but not in the base image are "added" (go into `dnf install`). Packages in the base image but not on the host are "base-image-only" — they will be present after migration. These are noted in the audit report as "new from base image."
6. **Cache in the snapshot.** The base image package list is stored in `inspection-snapshot.json` so re-renders via `--from-snapshot` don't need to pull the image again.

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

### `/var` Handling

bootc's contract is that `/var` content from the image is written to disk at initial bootstrap, but is **never updated by subsequent image deployments**. It becomes fully mutable state from that point forward. This means you *can* seed `/var` with initial directory structures and default data in the image, but anything that lives there is the operator's responsibility to manage, back up, and migrate going forward — bootc won't touch it again.

This has practical implications for the tool's output:

- `tmpfiles.d` snippets are generated to create expected directory structures (these run on every boot and are the right mechanism for ensuring directories exist).
- Small, static seed data (e.g., default config databases) *can* be included in the image and will land in `/var` on first install, but this is flagged with a `# NOTE: only deployed on initial bootstrap, not updated by bootc` comment.
- Application databases, runtime state, and log directories with significant data are **not** embedded. These appear in the audit report's "Data Migration Plan" section with explicit notes that they need a separate migration strategy.

This is called out prominently in the audit report's "Data Migration Plan" section, comments in the Containerfile, and the README's deployment instructions.

## 3. CLI Reference

One entry point is registered in `pyproject.toml`:

```
yoinkc = "yoinkc.__main__:main"
```

Subcommands: `yoinkc inspect` (default), `yoinkc fleet`, `yoinkc refine`. The `fleet` and `refine` subcommands were previously separate entry points (`yoinkc-fleet`, `yoinkc-refine`) and are now integrated.

`yoinkc-build` is a standalone companion script (not yet a subcommand) — it builds a bootc container image from yoinkc output with automatic RHEL subscription cert handling (see [Building on Non-RHEL Hosts](#building-on-non-rhel-hosts)).

### `yoinkc` — Host Inspection

```
yoinkc [flags]
```

Inspects a RHEL/CentOS/Fedora host mounted at `--host-root` and produces a tarball (default) or directory of migration artifacts.

#### Core Flags

| Flag | Default | Effect |
|---|---|---|
| `--host-root PATH` | `/host` | Root path for host inspection. |
| `--skip-preflight` | off | Skip container privilege checks (rootful, `--pid=host`, `--privileged`, SELinux). |
| `--yes` | off | Skip interactive confirmation prompts (for automation). |

#### Output Flags

`-o` and `--output-dir` are mutually exclusive.

| Flag | Default | Effect |
|---|---|---|
| `-o FILE` | auto | Write tarball to FILE. Default: `HOSTNAME-TIMESTAMP.tar.gz` in the current directory. |
| `--output-dir DIR` | off | Write files to a directory instead of producing a tarball. Required for `--validate` and `--push-to-github`. |
| `--no-subscription` | off | Skip bundling RHEL subscription certs into the output. |

#### Target Image Flags

| Flag | Default | Effect |
|---|---|---|
| `--target-version VERSION` | auto-detected | Target bootc image version (e.g. 9.6, 10.2). Default: source host version, clamped to minimum bootc-supported release. |
| `--target-image IMAGE` | auto-selected | Full target bootc base image reference (e.g. `registry.redhat.io/rhel10/rhel-bootc:10.2`). Overrides `--target-version` and all automatic mapping. |

#### Baseline Flags

`--no-baseline` and `--baseline-packages` are mutually exclusive.

| Flag | Default | Effect |
|---|---|---|
| `--baseline-packages FILE` | off | Path to a newline-separated list of package names for air-gapped environments where the base image cannot be queried via podman. |
| `--no-baseline` | off | Run without base image comparison — all installed packages will be included in the Containerfile. Use when the base image cannot be queried and `--baseline-packages` is unavailable. |

#### Inspection Depth Flags

The default run is optimized for speed. These opt-in flags enable deeper inspection at the cost of time and resources.

| Flag | Default | Effect |
|---|---|---|
| `--config-diffs` | off | Extract RPM defaults via `rpm2cpio` and generate line-by-line diffs for modified config files. Requires RPMs to be in local cache or downloadable from repos. |
| `--deep-binary-scan` | off | Run full `strings` scan on unknown binaries in `/opt` and `/usr/local` for version detection. Slow on large statically-linked binaries. |
| `--query-podman` | off | Connect to the podman socket to enumerate running containers and runtime state beyond what's in unit/compose files. |
| `--user-strategy STRATEGY` | auto | Override user creation strategy for all users. Valid: `sysusers`, `blueprint`, `useradd`, `kickstart`. Default: auto-assigned per classification (service -> sysusers, human -> kickstart, ambiguous -> useradd). |

#### Snapshot Flags

`--from-snapshot` and `--inspect-only` are mutually exclusive.

| Flag | Default | Effect |
|---|---|---|
| `--from-snapshot PATH` | off | Skip inspection; load a previously saved snapshot from PATH and run renderers only. |
| `--inspect-only` | off | Run inspectors and save snapshot to output; do not run renderers. |

#### Build Validation (`--validate`)

| Flag | Default | Effect |
|---|---|---|
| `--validate` | off | After generating output, run `podman build` against the Containerfile to verify it builds successfully. Requires `--output-dir`. |

When enabled, the tool runs `podman build` against the generated Containerfile after all output artifacts are written. This catches a large class of errors before the operator spends time on manual review:

- Missing dependencies (a package referenced in `RUN dnf install` that doesn't exist in the configured repos)
- Broken COPY paths (a config file referenced in the Containerfile that wasn't written to the `config/` tree)
- Syntax errors in generated systemd units, timer files, or SELinux policy modules
- Base image pull failures (registry auth issues, wrong tag)

The build runs with `--no-cache` to ensure a clean test. On success, the tool reports the image ID and size. On failure, it captures the build log, appends a `build-errors.log` to the output, and adds a summary of failures to the HTML report's warning panel and the audit report.

The resulting image is not pushed or deployed — it's a local build test only. The operator is expected to review, refine, and rebuild before deployment.

Note: validation requires access to the host's podman (via `nsenter`, same mechanism as baseline generation). It also requires network access to pull the base image and install packages, so it won't work in fully air-gapped runs without a pre-pulled base image.

When podman is not installed, `--validate` reports failure with an explanatory warning rather than silently claiming success.

#### GitHub Push (`--push-to-github`)

| Flag | Default | Effect |
|---|---|---|
| `--push-to-github REPO` | off | Push output to a GitHub repository (e.g. `owner/repo`). Requires `--output-dir` and confirmation (or `--yes`). Shows total data size before push. |
| `--github-token TOKEN` | `GITHUB_TOKEN` env | GitHub personal access token for repo creation. Falls back to the `GITHUB_TOKEN` environment variable. |
| `--public` | off | When creating a new GitHub repo, make it public instead of private. |

### `yoinkc fleet` — Fleet Aggregation

```
yoinkc fleet <input_dir> [flags]
```

Merges multiple yoinkc inspection snapshots into a single fleet-level snapshot with prevalence metadata. Produces a tarball (default) containing a Containerfile, HTML report, and merged snapshot.

| Flag | Default | Effect |
|---|---|---|
| `input_dir` (positional) | required | Directory containing yoinkc tarballs (`.tar.gz`) and/or JSON snapshot files. |
| `-p`, `--min-prevalence PCT` | `100` | Include items present on >= PCT% of hosts. Range: 1-100. |
| `-o`, `--output FILE` | auto | Output path for tarball (or JSON with `--json-only`). Default: auto-named in CWD. |
| `--output-dir DIR` | off | Write rendered files to a directory instead of tarball. |
| `--json-only` | off | Write merged JSON only, skip rendering. |
| `--no-hosts` | off | Omit per-item host lists from fleet metadata. |

### Environment Variables

| Variable | Used by | Effect |
|---|---|---|
| `YOINKC_DEBUG` | `yoinkc` (Python), `run-yoinkc.sh` | When set, prints full Python tracebacks on error instead of a one-line summary. The wrapper script forwards it into the container as `YOINKC_DEBUG=1`. |
| `YOINKC_HOSTNAME` | `run-yoinkc.sh`, inspectors | Overrides hostname detection. The wrapper defaults to `$(hostnamectl hostname)` and falls back to `$(hostname -f)` before passing it into the container. Inside the container, the inspector reads this as the top-priority hostname source, then falls back to `/etc/hostname`, then `hostnamectl hostname`. |
| `YOINKC_IMAGE` | `run-yoinkc.sh` | Container image to use. Default: `ghcr.io/marrusl/yoinkc:latest`. |
| `YOINKC_HOST_CWD` | `run-yoinkc.sh`, pipeline | The host's working directory, passed into the container by the wrapper. Used by the pipeline to name output files relative to the host filesystem. |
| `YOINKC_EXCLUDE_PREREQS` | `run-yoinkc.sh`, RPM inspector | Space-separated list of packages that the wrapper installed as prerequisites (e.g. `podman`). The RPM inspector excludes these from the "operator-added" package list so they don't end up in the Containerfile. |
| `YOINKC_OUTPUT_DIR` | `run-yoinkc.sh fleet` | Destination directory for fleet output tarball. Default: current working directory. |
| `GITHUB_TOKEN` | `yoinkc` (Python) | Fallback for `--github-token` when pushing to GitHub. |

### Wrapper Scripts

#### `run-yoinkc.sh`

A self-contained shell script (POSIX `sh`) for running yoinkc on a host without any prior installation. It:

1. Installs `podman` via `dnf` or `yum` if not already present
2. Tracks just-installed packages in `YOINKC_EXCLUDE_PREREQS` so they are excluded from inspection results
3. Checks/prompts for `registry.redhat.io` login when using RHEL base images
4. Launches the yoinkc container with the required privileges (`--pid=host`, `--privileged`, `--security-opt label=disable`)
5. Forwards all extra arguments to the `yoinkc` entry point
6. Detects subcommands (`fleet`, `refine`) and routes accordingly — `run-yoinkc.sh fleet` maps to `yoinkc fleet`, `run-yoinkc.sh refine` maps to `yoinkc refine`

Usage:

```
curl -fsSL https://raw.githubusercontent.com/marrusl/yoinkc/main/run-yoinkc.sh | sudo sh
curl -fsSL ... | sudo YOINKC_HOSTNAME=webserver01 sh -s -- --inspect-only
./run-yoinkc.sh fleet ./snapshots/ -p 80
./run-yoinkc.sh refine hostname-*.tar.gz
```

## 4. Schema

The schema (`schema.py`) is the contract between inspectors and renderers. Inspectors produce data that conforms to it; renderers consume it; fleet merge operates on it. Every inspector writes into a typed section of the schema, every renderer reads the complete snapshot, and the JSON serialization of the schema (`inspection-snapshot.json`) is the durable artifact that enables offline re-rendering, fleet aggregation, and snapshot-based workflows like `--from-snapshot`.

### Schema Versioning

The `SCHEMA_VERSION` integer constant (currently **8**) is embedded in every serialized snapshot. When `pipeline.py:load_snapshot()` deserializes a snapshot, it compares the file's `schema_version` against the running code's `SCHEMA_VERSION`. A mismatch raises a `ValueError` with a clear message instructing the user to re-run the inspection. This prevents silent data corruption when the schema evolves — old snapshots cannot be loaded by new code or vice versa.

### Model Hierarchy

All models use **Pydantic v2** `BaseModel`, giving automatic validation on construction, `model_dump_json()` for serialization, and `model_validate()` for deserialization. The hierarchy has three layers:

1. **Root model — `InspectionSnapshot`.** Contains `schema_version`, `meta` (hostname, timestamp, profile), `os_release` (from `/etc/os-release`), `warnings`, `redactions`, and 11 optional section fields — one per inspector.

2. **Section models — one per inspector.** `RpmSection`, `ConfigSection`, `ServiceSection`, `NetworkSection`, `StorageSection`, `ScheduledTaskSection`, `ContainerSection`, `NonRpmSoftwareSection`, `KernelBootSection`, `SelinuxSection`, `UserGroupSection`. Each section contains lists of item models and any inspector-specific metadata (e.g., `RpmSection.baseline_package_names`, `RpmSection.leaf_packages`).

3. **Item models — the individual findings.** `PackageEntry`, `RepoFile`, `ConfigFileEntry`, `ServiceStateChange`, `SystemdDropIn`, `FirewallZone`, `CronJob`, `GeneratedTimerUnit`, `QuadletUnit`, `ComposeFile`, `VersionChange`, and others. Many item models carry an `include: bool` field that controls whether the item appears in the Containerfile (toggled by the interactive refine UI or fleet merge).

Three enums define controlled vocabularies: `PackageState` (`added`, `base_image_only`, `modified`), `ConfigFileKind` (`rpm_owned_modified`, `unowned`, `orphaned`), and `VersionChangeDirection` (`upgrade`, `downgrade`).

### Key Design Choices

- **Optional sections.** Every section field on `InspectionSnapshot` is `Optional[...Section] = None`. If an inspector did not run (e.g., `--skip-network`), its field is `None` rather than an empty section. Renderers check for `None` and skip the corresponding output.
- **Round-trip fidelity.** `model_dump_json()` and `model_validate()` round-trip cleanly — the serialized JSON is the single source of truth for a snapshot. The `model_config = {"extra": "ignore"}` setting on the root model ensures forward compatibility: fields added in newer schema versions are silently dropped when loading into older code (though the version check prevents this in practice).
- **Flat item lists.** Sections contain flat lists of item models rather than nested hierarchies. This makes fleet merge straightforward (union lists, deduplicate by key fields) and keeps Jinja2 templates simple (iterate and render).

### Fleet Metadata

Fleet-aggregated snapshots carry additional metadata at two levels:

- **`FleetMeta`** on `InspectionSnapshot.meta` — contains `source_hosts` (list of hostnames), `total_hosts`, and `min_prevalence` (the `--min-prevalence` threshold used during aggregation).

- **`FleetPrevalence`** on individual item models — contains `count` (how many hosts had this item), `total` (total hosts in the fleet), and `hosts` (list of hostnames). Exactly **10 item models** carry an `Optional[FleetPrevalence]` field: `PackageEntry`, `RepoFile`, `ConfigFileEntry`, `ServiceStateChange`, `SystemdDropIn`, `FirewallZone`, `CronJob`, `GeneratedTimerUnit`, `QuadletUnit`, and `ComposeFile`. These are the models that support deduplication and prevalence tracking during fleet merge. The `fleet` field is `None` in single-host snapshots and populated only by `yoinkc fleet`.

The source code (`schema.py`) is the authoritative reference for individual field names and types.

## 5. Inspectors

#### RPM Inspector

`rpm -qa --queryformat` to get the full package list with epoch/version/release/arch, then diff against the package set from the target bootc base image (see [Baseline Generation](#baseline-generation)). Identifies: added packages (present on host but not in base image), base-image-only packages (present in base image but not on host — will be present after migration, shown as "new from base image"), and modified package configs via `rpm -Va`.

**Version drift detection.** For packages present in both the host and the base image, the inspector compares epoch:version-release using a pure-Python implementation of RPM's `rpmvercmp` algorithm (tilde pre-release and caret post-release markers supported). Packages where the host has a newer version than the base image are flagged as **downgrades** (regression risk — the base image would silently revert to the older version). Packages where the base image is newer are flagged as **upgrades** (usually benign). Results are stored in `RpmSection.version_changes` as `VersionChange` entries. Downgrades generate a warning for the warnings panel. When the baseline comes from a `--baseline-packages` file in names-only format (no NEVRA data), version comparison is gracefully skipped.

Also captures repo definitions from `/host/etc/yum.repos.d/` and `/host/etc/dnf/`. GPG key files referenced by `gpgkey=file:///...` in repo configs are parsed and collected — the parser handles comma-separated URLs, INI-style continuation lines (indented lines following `gpgkey=`), and variable substitution (`$releasever`, `$releasever_major`, `$basearch` resolved from os-release and platform). `https://` URLs are skipped since dnf fetches those at build time. Collected key files are written to the config tree and COPYed into the image before `dnf install` so that package signature verification succeeds.

Additionally checks `dnf history` for packages that were installed and later removed, since these may have left behind config files or state that still affects the system.

After computing the added-packages list, the tool classifies each package as "leaf" (explicitly installed by the operator) or "auto" (pulled in as a dependency). Only leaf packages appear in the Containerfile `dnf install` line; auto packages are noted in a comment above the install directive.

Source repo tracking is populated per added package via `dnf repoquery --installed --queryformat "%{name} %{from_repo}\n"`, with `rpm -qi` as a fallback (checking both the "From repo" and "Repository" header fields). This data drives the repo-grouped package display in the HTML report and audit report, where leaf packages are organized by their source repository rather than shown as a flat list.

The primary classification method uses `dnf repoquery --userinstalled`, which queries dnf's own tracking of which packages were explicitly requested vs pulled in as dependencies. This correctly identifies packages like `git` as leaf even when other added packages happen to depend on them — something the graph-based approach gets wrong. When `--userinstalled` is unavailable (e.g., dnf not installed, or the query returns results that don't overlap with the added set), the tool falls back to dependency graph analysis: `dnf repoquery --requires --recursive` for transitive resolution, or `rpm -qR` + `rpm --whatprovides` if dnf repoquery is unavailable. In the graph-based fallback, packages are leaf if no other added package depends on them. If dependency resolution fails for a package, it is treated as a leaf — over-include rather than under-include.

Regardless of which method determines the leaf/auto split, a dependency graph is always built (via dnf repoquery or rpm) to power the per-leaf dependency tree view in the audit report, so operators can verify the classification. This typically reduces the `dnf install` list by 60–80%, producing a cleaner Containerfile that expresses intent rather than transitive closure.

**dnf5 compatibility.** All `--queryformat` strings used with `rpm` and `dnf repoquery` include an explicit `\n` delimiter. In dnf5 (Fedora 41+), `--queryformat` no longer appends a newline after each record automatically, so omitting the `\n` would produce concatenated output. The explicit delimiter works correctly on both dnf4 and dnf5.

#### Service Inspector

Uses `systemctl --root=/host` as the preferred detection method for accurate service state (enabled, disabled, masked, static). This leverages systemd's own logic and avoids reimplementing preset evaluation. Falls back to filesystem-based scanning when `systemctl --root` is unavailable or returns errors (e.g., systemd version mismatches between the tool container and host): scans `.wants/` symlinks in `/host/etc/systemd/system/` for enabled services, symlinks to `/dev/null` for masked services, and parses `[Install]` sections in vendor units under `/host/usr/lib/systemd/system/` for disabled/static classification.

Diffs enabled/disabled/masked state against the defaults from systemd preset files in the base image. Preset glob rules (e.g. `enable cloud-*`) are evaluated using `fnmatch` with first-match-wins semantics, matching `systemd-preset(5)` behaviour.

After unit-file scanning, the inspector also scans `/etc/systemd/system/` for drop-in override directories matching `*.service.d/`, `*.timer.d/`, and `*.socket.d/`. For each `.conf` file found, a `SystemdDropIn` entry is created with the parent unit name (derived by stripping the `.d` suffix) and the file content. Only admin overrides are scanned; vendor drop-ins under `/usr/lib/systemd/system/` ship with the base image and are not reported. The config inspector excludes these paths from the unowned-file list to avoid double-reporting. The Containerfile renderer writes included drop-ins to the config tree and references them in the consolidated COPY block; the audit report shows them in a "Systemd drop-in overrides" subsection under Services.

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

Quadlet unit files captured under `/etc/containers/systemd/` are excluded from the config file display in both the HTML report and audit report. They appear in the Config inspector's snapshot data and are written to the output, but their dedicated home is the Containers section where they are shown with image references and unit type context. The Containerfile renders them via `COPY quadlet/ /etc/containers/systemd/` rather than the consolidated `COPY config/etc/ /etc/` block, so writing them to the config tree would duplicate them in the image.

When custom CA certificates are found under `/etc/pki/ca-trust/source/anchors/` (captured as unowned configs), the Containerfile renderer emits `RUN update-ca-trust` immediately after the consolidated COPY block. Without this, copied certificates would not be added to the system trust store and TLS connections to internal services would silently fail.

**Config file categories:**

Every `ConfigFileEntry` carries a `category` field (a `ConfigCategory` enum) assigned at creation time by `classify_config_path()`. Classification is path-prefix matching — the same pattern as the exclusion list. Eight semantic categories are defined: `tmpfiles` (`/etc/tmpfiles.d/`), `environment` (`/etc/environment`, `/etc/profile.d/`), `audit` (`/etc/audit/rules.d/`), `library_path` (`/etc/ld.so.conf.d/`), `journal` (`/etc/systemd/journald.conf.d/`), `logrotate` (`/etc/logrotate.d/`), `automount` (`/etc/auto.master`, `/etc/auto.*`), and `sysctl` (`/etc/sysctl.d/`, `/etc/sysctl.conf`). Files matching none of these rules default to `other`. All three detection paths (RPM-owned modified, unowned, orphaned) assign the category at `ConfigFileEntry` construction.

Category is informational only — the Containerfile copies files regardless of category. In the HTML report, the config files table has a "Category" column with PF6 label styling that is sortable. No structural change to the table layout.

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
- **Firewall rules**: `firewalld` zones — services, ports, and rich rules parsed from zone XML files under `/etc/firewalld/zones/`. Direct rules parsed from `/etc/firewalld/direct.xml`. Exported as raw XML files via the config tree; `firewall-offline-cmd` equivalents are listed in the audit report.
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

Vendor systemd timers (those that ship with the base image, `source == "vendor"`) are excluded from the HTML report table and audit report listing — they require no operator action. A count note is shown in their place. RPM-owned cron jobs are similarly hidden from the display, replaced with a count note; the inspector already skips generating timer units from rpm-owned cron jobs.

#### Container Inspector

Discovers container workloads through a fast file-based scan:

1. **Quadlet units**: `/etc/containers/systemd/`, `/usr/share/containers/systemd/`, and `/etc/systemd/system/` — these are the primary source of truth and are always captured. Also scans user-level quadlets at `~/.config/containers/systemd/` for real users (UID 1000–59999) via targeted path lookup (no recursive home directory traversal). All six quadlet unit types are captured: `.container`, `.volume`, `.network`, `.kube`, `.image`, and `.build`. `Image=` is parsed from `.container` files; other types carry an empty image field. All unit files are copied to `quadlet/` and included via `COPY quadlet/ /etc/containers/systemd/`.
2. **Compose files**: podman-compose and docker-compose files found via search across `/opt`, `/srv`, `/etc`. Parses `image:` fields to extract image references per service.
3. **Container image references**: extracted from both quadlet units and compose files and displayed in the audit report.

**Live container query (opt-in: `--query-podman`):**

When enabled, runs `podman ps -a --format json` to enumerate containers, then `podman inspect` on each to capture mounts, network settings, ports, and environment variables. This captures runtime state that may not be reflected in the unit files (e.g., containers started manually, containers with runtime overrides). Without this flag, the inspector relies entirely on the file-based scan, which is faster and covers the vast majority of cases since production workloads should be defined in unit files.

Existing quadlet units are copied directly into the image. Compose files are noted in the Containerfile and audit report with a suggestion to convert to quadlet using `podlet` or manually — the tool does not attempt automatic conversion.

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
- Kernel modules: loaded (`lsmod`) diffed against defaults. Modules configured in `/etc/modules-load.d/` and `/usr/lib/modules-load.d/` are treated as expected. Modules loaded only as dependencies (non-empty `used_by` column) are filtered out. Only modules that are neither explicitly configured nor loaded as dependencies appear as non-default. The non-default module list is collected in the snapshot for reference, but the HTML report and audit report do not display it — hardware-autoloaded modules are host-specific and have no meaning for migration. Only explicitly configured modules (from modules-load.d, modprobe.d, and dracut.conf.d) appear in the report.
- Sysctl settings: reads runtime values from `/proc/sys/`, reads shipped defaults from `/usr/lib/sysctl.d/*.conf` (sorted by filename, matching systemd precedence), reads operator overrides from `/etc/sysctl.d/*.conf` and `/etc/sysctl.conf`. Only values where runtime differs from shipped default appear in output, with source attribution.
- Dracut configuration: `/etc/dracut.conf.d/`
- Tuned profiles: reads the active profile name from `/etc/tuned/active_profile` (falls back to `tuned-adm active` via executor). Scans `/etc/tuned/` for subdirectories containing a `tuned.conf` and captures them as custom profiles. Custom profiles are written to the config tree and the Containerfile emits `RUN tuned-adm profile <name>` in the Kernel section. Tuned profile files are excluded from the unowned-file list to avoid double-reporting.
- **System properties** (locale, timezone, alternatives): detected via file-based methods (container-compatible — no `localectl` or similar host commands). Locale is read from `{host_root}/etc/locale.conf` (`LANG=` line). Timezone is derived from the `{host_root}/etc/localtime` symlink target, stripping the `/usr/share/zoneinfo/` prefix. Alternatives are enumerated from symlinks in `{host_root}/etc/alternatives/`, with auto/manual status read from the corresponding file in `{host_root}/var/lib/alternatives/<name>` (first line is `auto` or `manual`). All detection is best-effort — missing files produce `None`/empty list, not errors. These fields (`locale`, `timezone`, `alternatives`) are added to `KernelBootSection` with optional/empty defaults so existing snapshots remain valid. Rendered in the Kernel/Boot tab: a description list for locale and timezone, and a table (Name, Path, Status) for alternatives. Both subsections are informational only — no include/exclude toggles, and the Containerfile is not affected (locale, timezone, and alternatives are host-level concerns, not image concerns).

#### SELinux/Security Inspector

- SELinux mode from `/host/etc/selinux/config`
- Custom policy modules: detected by scanning the priority-400 module store (`/host/etc/selinux/<type>/active/modules/400/`). Only modules at priority 400 (installed via `semodule -i`) are reported as custom.
- Boolean overrides: `chroot /host semanage boolean -l` returns current and default values in a single command. Non-default booleans are flagged. Falls back to reading `/host/sys/fs/selinux/booleans/*` (runtime values as current/pending pairs) if chroot fails.
- Custom fcontext rules: `chroot /host semanage fcontext -l -C` to capture operator-defined file context mappings. Falls back to reading `file_contexts.local` from the policy store if `semanage` is unavailable.
- Custom port label assignments: `chroot /host semanage port -l -C` to capture operator-defined SELinux port type associations (e.g., `ssh_port_t tcp 2222`).
- Audit rules from `/etc/audit/rules.d/` — filtered against RPM ownership. Only operator-added audit rule files (not owned by any installed RPM) are reported.
- FIPS mode status
- Custom PAM configurations — filtered against RPM ownership. Only PAM config files under `/etc/pam.d/` that are not owned by any installed RPM appear in the report. RPM-shipped defaults (the vast majority of `/etc/pam.d/` entries) are excluded.

#### User/Group Inspector

Non-system users and groups (1000 <= UID < 60000) from `/host/etc/passwd` and `/host/etc/group`. Captures raw entries from `passwd`, `shadow`, `gshadow`, `group`, `subuid`, and `subgid` for each qualifying user.

Captures: shell assignments, group memberships, sudoers rules, SSH authorized_keys references (flagged for manual handling, not copied).

##### User Provisioning Strategies

Discovered users are classified into three categories:

- **Service accounts**: nologin or `/bin/false` shell, home under `/var`, UID < 1000. These are daemon/application accounts.
- **Human users**: real shell (`/bin/bash`, `/bin/zsh`, etc.), home under `/home`, UID >= 1000. These are operator/developer accounts.
- **Ambiguous**: everything else — accounts that don't cleanly fit either pattern.

Each user is assigned a provisioning strategy:

| Strategy | Mechanism | When |
|---|---|---|
| `sysusers` | `systemd-sysusers` drop-in file under `/usr/lib/sysusers.d/`, applied at boot | Service accounts |
| `blueprint` | `bootc-image-builder` TOML user definition (`yoinkc-users.toml`) | Optional alternative for service accounts |
| `useradd` | Explicit `RUN useradd ...` in the Containerfile | Ambiguous accounts |
| `kickstart` | Deploy-time provisioning via kickstart `user --name=...` directives | Human users |
Default assignment: service → `sysusers`, human → `kickstart`, ambiguous → `useradd`. The `--user-strategy STRATEGY` flag overrides the strategy for all users (e.g., `--user-strategy useradd` to use explicit `useradd` commands for everything).

SSH authorized keys are never embedded in the image — they always appear as a `# FIXME: provision SSH keys via cloud-init, kickstart, or identity provider` comment. Baking SSH keys into an image is an anti-pattern for fleet management.

The generated README includes an opinionated strategy guide explaining the trade-offs between provisioning approaches.

## 6. Renderers

Renderers consume the complete `InspectionSnapshot` and produce the output artifacts that operators actually work with. Each renderer is a module in `src/yoinkc/renderers/` with a `render(snapshot, env, output_dir)` function that takes the snapshot, a Jinja2 `Environment`, and the output directory.

### Orchestration

`renderers/__init__.py:run_all()` is the single entry point. It creates the output directory, sets up the Jinja2 environment, and calls each renderer in sequence:

1. `render_containerfile` — writes `Containerfile` and `config/` tree
2. `render_audit_report` — writes `audit-report.md`
3. `render_html_report` — writes `report.html`
4. `render_readme` — writes `README.md`
5. `render_kickstart` — writes `kickstart-suggestion.ks`
6. `render_secrets_review` — writes `secrets-review.md`

Order matters: the Containerfile renderer runs first because subsequent renderers read from the generated `Containerfile` (e.g., extracting FIXME counts, embedding the Containerfile content in the HTML report).

### Jinja2 Setup

`run_all()` creates a single `Environment` with `FileSystemLoader` pointing to `src/yoinkc/templates/` and `autoescape=True`. This environment is passed to every renderer. The HTML report renderer adds a custom `fleet_color` filter for fleet prevalence color bars. If a renderer is called directly (e.g., in tests) without a loader, it sets one up from the package's templates directory.

### Containerfile Renderer (`renderers/containerfile/`)

Produces two outputs: the `Containerfile` itself and a `config/` directory tree containing all files referenced by `COPY` directives.

The renderer is a package of 14 modules. `_core.py` is the orchestrator — it calls domain modules in layer order and assembles their output. Each domain module exports a `section_lines(snapshot, ...) -> list[str]` function, making sections independently testable. Shared utilities live in `_helpers.py`; config-tree file-writing lives in `_config_tree.py`. Domain modules mirror the inspectors structure: `packages.py`, `services.py`, `config.py`, `scheduled_tasks.py`, `non_rpm_software.py`, `containers.py`, `users_groups.py`, `kernel_boot.py`, `selinux.py`, `network.py` (firewall + NM). `__init__.py` re-exports `render()` for a stable public interface.

**Layer ordering.** The Containerfile is structured in a deliberate layer order to maximize cache efficiency — layers that change infrequently appear first so that rebuilds after minor config changes don't invalidate the package installation layer:

```
repos → packages → services → firewall → scheduled tasks → configs →
non-RPM software → quadlets → users → kernel → SELinux → network → tmpfiles.d
```

Each section has comments explaining what was detected and why it was included. `# FIXME` comments mark anything that needs human review. Every generated Containerfile ends with `RUN bootc container lint`, which validates that the image is bootc-compatible at build time.

**Input sanitization.** All values interpolated into `RUN` shell commands are validated against a character allow-list (alphanumerics plus a controlled set of shell-safe characters). This is a safety net against corrupted snapshots producing malformed Containerfiles, not a security boundary — the data comes from RPM databases and systemd on an operator-controlled host.

**Tool package detection.** The Non-RPM Software section checks whether tool packages needed by non-RPM items (e.g., `npm`/`nodejs` for lockfile installs, `python3-pip` for pip requirements) are already in the `dnf install` block. If not, a prerequisite `RUN dnf install` is emitted before the non-RPM directives.

**Firewall handling.** Firewall zone XML files and direct rules are written to the config tree and included in the consolidated `COPY config/etc/ /etc/` block. The Containerfile section for firewall is comments-only, referencing the audit report for `firewall-offline-cmd` equivalents per zone.

**Config tree.** `write_config_tree()` (in `_config_tree.py`) writes all captured config files, repo files, GPG keys, firewall zone XMLs, systemd timer units, quadlet units, sysusers drop-ins, SELinux modules, and tuned profiles to `output_dir/config/`, preserving the original path hierarchy so that `COPY config/etc/ /etc/` places everything correctly.

### Audit Report Renderer (`audit_report.py`)

Produces `audit-report.md`, a markdown document for version control and quick terminal reference. Organized as:

1. **Executive Summary**: triage breakdown (automatic / FIXME / manual), counts of packages, configs, containers, redactions.
2. **Per-section findings**: RPM analysis (including version drift summary — downgrades with warning prefix, upgrades as informational — when version changes exist), service state changes, configuration changes, network configuration, storage migration plan, scheduled tasks, container workloads, non-RPM software, kernel/boot, SELinux, users/groups — each rendered only when the corresponding snapshot section is non-`None`.
3. **Data Migration Plan**: `/var` analysis with mount points mapped to recommended approaches (PVC, volume mount, tmpfiles.d).
4. **Environment-specific considerations**: advisory subsections rendered only when relevant (custom alternatives, raw nftables, complex network topologies, identity provider integration, NTP/chrony, rsyslog forwarding).
5. **Items requiring manual intervention**: consolidated list from all inspectors.

### HTML Report Renderer (`html_report.py` + `report.html.j2`)

Produces `report.html`, a single self-contained HTML file (inline CSS/JS, no external dependencies) that can be opened in any browser. The renderer builds a context dict from the snapshot and delegates to the Jinja2 template.

**Context building.** `_build_context()` pre-computes several derived data structures:
- Summary counts per section
- Triage breakdown (automatic/FIXME/manual) via `_triage.py`
- File browser tree HTML (pre-rendered `Markup`)
- Config file diffs (pre-rendered HTML with syntax highlighting)
- Container mount/network summaries
- Fleet variant grouping (for fleet-aggregated snapshots)
- Repo-grouped dependency tree entries with version display
- Embedded PatternFly 6 CSS (read from `templates/patternfly.css` and inlined)
- Embedded snapshot JSON (for the interactive refine UI)

**Template architecture.** `report.html.j2` is a ~66-line skeleton of Jinja2 `{% include %}` directives; the content lives in 29 partials under `templates/report/`. Infrastructure partials include macros (`_macros.html.j2`), CSS (`_css.html.j2`), JS (`_js.html.j2`), toolbar (`_toolbar.html.j2`), sidebar (`_sidebar.html.j2`), and summary (`_summary.html.j2`). Content sections map one-to-one to partials — packages, services, config, non-RPM software, containers, users/groups, scheduled jobs, kernel/boot, SELinux, secrets, network, storage, warnings, audit report, Containerfile preview, module streams, and version locks. UI overlays include the file editor (`_editor.html.j2`, `_editor_js.html.j2`), fleet banner (`_banner.html.j2`), compare modal (`_compare_modal.html.j2`), new file modal (`_new_file_modal.html.j2`), and file browser (`_file_browser.html.j2`). The packages partial includes an optional "Version Changes" subsection with PF6 color-coded labels (red for downgrades, blue for upgrades), shown only when `version_changes` is non-empty. The rendered output is unchanged: a single self-contained HTML file. The template uses PatternFly 6 CSS with custom overrides, and PF6's page structure: sidebar navigation with section links and a main content area with show/hide sections controlled by JavaScript.

**Fleet-aware rendering.** The template conditionally renders fleet-specific elements when `fleet_meta` is present in the context: a fleet banner showing host count and prevalence threshold, prevalence color bars (blue/gold/red) on individual items, fraction/percentage toggle, host list popovers, and content variant grouping for config files, quadlet units, and service drop-ins.

### README Renderer (`readme.py`)

Produces `README.md` with:
- Summary of findings (packages, configs, services, FIXMEs, warnings)
- Exact `podman build` command
- Exact `bootc switch` or `bootc install` command with the right flags for the detected scenario
- List of FIXME items requiring resolution
- Artifacts table describing each output file
- User provisioning strategy guide (opinionated explanation of sysusers vs. useradd vs. kickstart vs. blueprint trade-offs)

The README renderer reads back the generated Containerfile to extract FIXME items, creating a cross-reference between the two artifacts.

### Kickstart Renderer (`kickstart.py`)

Produces `kickstart-suggestion.ks` containing deploy-time provisioning suggestions:
- DHCP network interfaces (`network --bootproto=dhcp ...`)
- Hostname (`network --hostname=...`)
- Site-specific DNS
- NFS mount credentials
- User provisioning directives for kickstart-strategy users (`user --name=...`)
- Deployment-specific environment variables referenced in configs

The file is clearly marked as a **suggestion** — it needs review and adaptation for the target environment.

### Secrets Review Renderer (`secrets_review.py`)

Produces `secrets-review.md`, a standalone report listing all redacted items. Each entry includes the file path, the pattern that matched, the redacted line, and a suggested remediation approach (e.g., "use a Kubernetes secret", "use a systemd credential"). This complements the redaction machinery in Secret Handling (below) by giving operators a single checklist of items that need manual attention.

### Shared Triage Computation (`_triage.py`)

The `_triage` module provides two functions used by both the audit report and HTML report renderers:

- **`compute_triage(snapshot, output_dir)`** classifies all inspected items into three buckets: **automatic** (items the Containerfile handles without intervention), **FIXME** (items with `# FIXME` comments in the Containerfile that need review), and **manual** (warnings, redacted secrets, SSH key references). The FIXME count is computed by scanning the already-generated Containerfile for `# FIXME` comment lines — this is why the Containerfile renderer must run first.

- **`compute_triage_detail(snapshot, output_dir)`** returns a per-category breakdown for the readiness panel, with each entry carrying a label, count, target tab, and status. Only categories with non-zero counts are included.

Both functions filter by `.include` on item models, so excluded items (toggled off via the refine UI or fleet merge) don't inflate the counts.

## 7. Fleet Analysis

Inspecting one host at a time doesn't scale. When migrating a fleet of similar hosts (same OS, same base image, same role), the fleet analysis subsystem finds common ground — producing a single merged snapshot that captures what's shared across hosts and flags what varies.

### Architecture

The fleet pipeline is: **loader** → **merge engine** → **standard renderers**. The merge engine produces an `InspectionSnapshot` with fleet metadata attached, then feeds it through the same renderer pipeline used for single-host inspection. This means fleet output gets Containerfile generation, HTML reports, and all other renderers for free.

```
discover_snapshots()  →  validate_snapshots()  →  merge_snapshots()  →  run_pipeline()
     (loader.py)            (loader.py)             (merge.py)         (reuse existing)
```

The entry point is `yoinkc fleet <input_dir>`, implemented in `fleet/__main__.py`. It calls the loader, validates compatibility, merges, and delegates to `run_pipeline()` for rendering and tarball packaging.

### Loader (`fleet/loader.py`)

`discover_snapshots(input_dir)` scans a directory for `.tar.gz` tarballs and `.json` files, returning a list of `InspectionSnapshot` objects:

- **Tarballs:** extracts `inspection-snapshot.json` from the tar archive (supports both `<prefix>/inspection-snapshot.json` and bare `inspection-snapshot.json` paths). Invalid tarballs are skipped with a warning.
- **JSON files:** loaded directly via Pydantic's `InspectionSnapshot(**data)` constructor. Invalid JSON is skipped with a warning.
- Other file types are silently ignored. Files are processed in sorted order for deterministic output.

`validate_snapshots(snapshots)` enforces merge compatibility:

- Minimum 2 snapshots required.
- All snapshots must share the same `schema_version`.
- All must have `os_release` present, with matching `id` and `version_id`.
- All must have matching `base_image` (in their RPM sections).
- Duplicate hostnames produce a warning (re-inspected hosts are valid, not an error).

### Merge Engine (`fleet/merge.py`)

`merge_snapshots(snapshots, min_prevalence, fleet_name, include_hosts)` is the core algorithm. It builds a union of all items across hosts, counts how many hosts have each item, and applies a prevalence threshold to set `include` flags.

#### Identity Merge

Items that are matched by a simple identity key. The Containerfile installs packages by name and enables services by unit name, so content differences across hosts (e.g., different package versions) don't matter — the identity is what counts.

| Section | List | Identity Key |
|---|---|---|
| `rpm` | `packages_added` | `name` |
| `rpm` | `base_image_only` | `name` |
| `rpm` | `repo_files` | `path` |
| `rpm` | `gpg_keys` | `path` |
| `services` | `state_changes` | `unit` + `action` |
| `network` | `firewall_zones` | `name` |
| `scheduled_tasks` | `generated_timer_units` | `name` |
| `scheduled_tasks` | `cron_jobs` | `path` |

For identity-only items, the merged entry takes field values from the first host encountered. Each item gets a `FleetPrevalence` object recording count, total, and (optionally) host list.

#### Content Merge

Items where the same identity (path) can have different content across hosts. Each unique (identity, variant) pair becomes a separate entry in the merged snapshot, with its own prevalence count.

| Section | List | Identity Key | Variant Key |
|---|---|---|---|
| `config` | `files` | `path` | SHA-256 hash of `content` |
| `containers` | `quadlet_units` | `path` | SHA-256 hash of `content` |
| `services` | `drop_ins` | `path` | SHA-256 hash of `content` |
| `containers` | `compose_files` | `path` | Hash of sorted `(service, image)` tuples |

For example, if `/etc/httpd/conf/httpd.conf` exists in 3 content variants across 100 hosts, the merged snapshot has 3 `ConfigFileEntry` items for that path — one per variant, each with its own prevalence. The user sees all variants in the HTML report and can pick the right one.

#### Prevalence Threshold

The `-p` / `--min-prevalence` flag (integer 1-100, default 100) controls which items get `include: true`:

```
include = (count * 100) >= (min_prevalence * total)
```

At 100% (default), only items present on **every** host are included — a strict intersection. At lower values (e.g., `-p 90`), the tool absorbs fleet drift by including items on 90%+ of hosts. Below-threshold items remain in the snapshot with `include: false` — they're visible in the HTML report and can be re-included during refinement.

#### Per-Field Dispositions

Beyond identity and content merge, each field in each section has a specific disposition:

- **Merge:** identity or content merge with prevalence tracking (described above).
- **Deduplicate:** union by value, preserving first-seen order. Used for `dnf_history_removed`, `enabled_units`, `disabled_units`, `systemd_timers`, `sudoers_rules`.
- **Pass-through:** taken from the first snapshot (must be identical across all, enforced by validation). Used for `base_image`, `baseline_package_names`, `no_baseline`.
- **Omit:** set to empty/None in the merged snapshot. Used for host-specific data like `rpm_va`, `connections`, `static_routes`, `running_containers`, `ssh_authorized_keys_refs`, `passwd_entries`, `shadow_entries`.

**Special handling for leaf/auto packages:** `leaf_packages` and `auto_packages` are unioned (deduplicated) rather than omitted. If a package is leaf on any host, it's marked leaf in the fleet snapshot. This preserves dep-solving intelligence — the Containerfile can install leaf packages and let `dnf` pull in auto dependencies. `leaf_dep_tree` maps are also merged (union of all dependency mappings).

**Users/groups** are `List[dict]` (not typed Pydantic models), so they cannot receive a typed `fleet` field. They are deduplicated by `name` key, with prevalence stored as a plain `"fleet"` dict key injected into each entry.

**Warnings and redactions** are collected from all input snapshots and deduplicated.

**Sections omitted entirely** in the merged snapshot: `kernel_boot`, `selinux`, `non_rpm_software`, `storage`. These are host-specific and not meaningful in a fleet context.

#### Fleet Metadata

The merged snapshot carries `meta["fleet"]` with a `FleetMeta` structure:

- `host_count`: number of input snapshots
- `hostnames`: list of all hostnames
- `threshold`: the `--min-prevalence` value used
- `merged_at`: ISO 8601 timestamp

Individual items carry a `FleetPrevalence` field with `count`, `total`, and (unless `--no-hosts`) `hosts` list.

### Fleet UI

The HTML report template detects fleet data via `fleet_meta` in the template context and conditionally renders fleet-specific elements:

- **Fleet banner:** displayed at the top of the report showing host count, prevalence threshold, hostnames, and include/exclude summary counts.
- **Prevalence color bars:** each item gets a colored bar indicating prevalence. Blue for items above threshold (included), gold for items below threshold but on multiple hosts, red for items on very few hosts. The bar width is proportional to the prevalence percentage.
- **Fraction/percentage toggle:** a JavaScript toggle switches prevalence display between fraction form (e.g., "95/100") and percentage form (e.g., "95%").
- **Host list popovers:** clicking a prevalence bar shows a PatternFly 6 popover listing which specific hosts have that item (unless `--no-hosts` was used). The popover header shows the "N of M hosts" count and a split Copy button. The main Copy area copies the host list in the active format; the dropdown arrow expands a format menu (one per line, comma-separated, space-separated) with a blue checkmark (PF6 brand color) on the active choice. Clicking a format copies immediately in that format, updates the active choice, and closes the dropdown. The last-used format is remembered in a JS module-level variable for the duration of the page session (not persisted to `localStorage`). All interactive elements inside the popover call `e.stopPropagation()` to prevent the bar's click handler from tearing down the popover mid-action.
- **Content variant grouping:** for config files, quadlet units, and service drop-ins, items sharing the same path are grouped together with an expandable UI showing each content variant and its prevalence. Groups with a single item render as normal rows. Compose files render as separate cards (no grouping needed since cards are visually distinct).

In non-fleet mode, none of these elements appear — the template renders exactly as it does for single-host inspection.

### CLI (`fleet/cli.py`)

The `yoinkc fleet` subcommand accepts:

```
yoinkc fleet <input_dir> [-p PCT] [-o FILE] [--output-dir DIR] [--json-only] [--no-hosts]
```

| Flag | Default | Effect |
|---|---|---|
| `input_dir` | required | Directory containing `.tar.gz` and/or `.json` snapshots |
| `-p`, `--min-prevalence` | `100` | Include items on >= PCT% of hosts (range: 1-100) |
| `-o`, `--output` | auto | Output tarball path (or JSON path with `--json-only`) |
| `--output-dir` | off | Write rendered files to a directory instead of tarball |
| `--json-only` | off | Write merged JSON only, skip rendering |
| `--no-hosts` | off | Omit per-item host lists from fleet metadata |

Default output is a tarball containing Containerfile, HTML report, and merged snapshot — matching the single-host output format.

### Container Wrapper (`run-yoinkc.sh fleet`)

For zero-install workstation use, `run-yoinkc.sh fleet` runs the fleet aggregation inside the yoinkc container image. It:

1. Requires `podman` (does not auto-install).
2. Bind-mounts the input directory read-only and a temp directory for output.
3. Runs `yoinkc fleet` inside the container, passing through `-p`, `--json-only`, and `--no-hosts` flags.
4. Copies the result tarball to `YOINKC_OUTPUT_DIR` (default: current working directory).

Usage: `./run-yoinkc.sh fleet ./snapshots/ -p 80 --no-hosts`

## 8. Pipeline & Packaging

The pipeline orchestrator in `pipeline.py` ties inspection, redaction, rendering, and packaging into a single `run_pipeline()` call. Everything downstream of `run_pipeline()` is deterministic given the same snapshot — re-rendering from a saved snapshot produces identical output.

### Pipeline Flow

`run_pipeline()` accepts two modes of operation:

1. **From inspectors** (default): calls the inspector suite against `host_root`, producing an `InspectionSnapshot`. The snapshot is immediately passed through `redact_snapshot()` before any content is written to disk.
2. **From snapshot** (`--from-snapshot PATH`): loads a previously saved `inspection-snapshot.json` via `load_snapshot()`, validates the schema version, and re-runs `redact_snapshot()`. This mode enables offline re-rendering — copy a snapshot to a workstation, iterate on include/exclude choices, and re-render without access to the original host.

After the snapshot is ready, the pipeline branches:

- **`--inspect-only`**: saves the snapshot to `inspection-snapshot.json` in the working directory and returns immediately. No rendering, no packaging, no subscription cert bundling. Useful for capturing raw data to process later.
- **Normal flow**: renders all output artifacts into a temporary directory (snapshot JSON, Containerfile, reports, config tree), bundles subscription certificates (unless suppressed), and packages the result.

### Output Modes

Output is mutually exclusive among three modes:

- **Tarball (default)**: `create_tarball()` in `packaging.py` produces a `.tar.gz` with all entries under a `HOSTNAME-YYYYMMDD-HHMMSS/` prefix directory. The stamp is generated by `get_output_stamp()`, which resolves the hostname from the snapshot metadata (preferred), falling back to `{host_root}/etc/hostname`, then `socket.gethostname()`, then `"unknown"`. Characters unsafe for filenames are stripped by `sanitize_hostname()`. After writing the tarball, the pipeline prints next-step instructions: an `scp` command (using `YOINKC_HOST_CWD` if set), a `yoinkc refine` command, and a `yoinkc-build` command.
- **Explicit tarball path** (`--output-file PATH`): same as above but writes to the specified path instead of auto-generating the name.
- **Directory** (`--output-dir PATH`): copies rendered files directly to the target directory instead of creating a tarball. Useful for development and testing.

On error during output, the temporary directory is preserved and its path is printed to stderr so the operator can recover partial results.

### Tarball Contents

A typical tarball contains:

```
HOSTNAME-YYYYMMDD-HHMMSS/
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
│       └── lib/sysusers.d/    # systemd-sysusers drop-ins (sysusers strategy)
├── yoinkc-users.toml           # bootc-image-builder user definitions (blueprint strategy)
├── quadlet/                   # container unit files
├── kickstart-suggestion.ks     # suggested kickstart snippet for deploy-time settings
├── report.html                # interactive HTML report (open in browser)
├── inspection-snapshot.json   # raw inspector output, for re-rendering
├── entitlement/               # RHEL subscription .pem files (if bundled)
└── rhsm/                     # RHSM config tree (if bundled)
```

### Subscription Certificate Bundling

RHEL hosts require subscription certificates to install packages from `registry.redhat.io` base images. `bundle_subscription_certs()` in `subscription.py` copies these into the tarball so the Containerfile can be built on any host:

- `.pem` files from `{host_root}/etc/pki/entitlement/` (Red Hat's path) are copied to `entitlement/` in the output.
- The entire `{host_root}/etc/rhsm/` tree is copied to `rhsm/` in the output.
- Both steps silently skip if the source paths don't exist (non-RHEL hosts, or hosts without active subscriptions).
- Bundling is skipped in `--from-snapshot` mode (the host filesystem isn't mounted) and when `--no-subscription` is passed.

When building the Containerfile, `yoinkc-build` searches for certificates in a priority cascade: bundled in the yoinkc output (placed there by this step) → host-local (`/etc/pki/entitlement`) → `./entitlement/` in the current directory → the `YOINKC_ENTITLEMENT` environment variable. Found certificates are bind-mounted into the build container transparently. Certificate expiry is validated via `openssl x509 -checkend` so the operator gets a clear warning before a build fails due to stale credentials.

### Wrapper Scripts

`run-yoinkc.sh` is the single zero-install entry point. It detects subcommands (`fleet`, `refine`) via a case statement and routes accordingly:

**Default (inspect):**

1. Installs `podman` if missing (via `dnf` or `yum`).
2. Tracks just-installed tools in `YOINKC_EXCLUDE_PREREQS` so yoinkc can exclude them from the RPM output (they're tool prerequisites, not operator additions).
3. Checks/prompts for `registry.redhat.io` login when using Red Hat base images.
4. Runs the yoinkc container with `--privileged`, `--pid=host`, and the host root bind-mounted at `/host:ro`. Passes through `YOINKC_DEBUG`, `YOINKC_HOST_CWD`, and `YOINKC_HOSTNAME` (defaulting to `hostnamectl hostname`, then `hostname -f`).

**`run-yoinkc.sh fleet`:**

1. Requires `podman` (does not auto-install).
2. Bind-mounts the input directory read-only and a temp directory for output.
3. Runs `yoinkc fleet` inside the container, passing through `-p`, `--json-only`, and `--no-hosts` flags.
4. Copies the result tarball to `YOINKC_OUTPUT_DIR` (default: current working directory).

**`run-yoinkc.sh refine`:**

1. Runs `yoinkc refine` inside the container with port 8642 mapped.
2. Passes through `--no-browser`, `--port`, and the tarball path.

### Image Build

The `Containerfile` at the repository root defines the yoinkc tool image:

- **Base image**: `fedora:latest` — chosen because it's in the Red Hat family (compatible tooling and package ecosystem) but doesn't require a subscription, and works on both `amd64` and `aarch64`.
- **Dependencies**: `python3`, `python3-pip`, `systemd` (for systemd-related inspection), `binutils` and `file` (for binary analysis).
- **Install**: the yoinkc package is installed in editable mode (`pip install -e .`).
- **Entry point**: `yoinkc`. Subcommands `fleet` and `refine` are routed by `run-yoinkc.sh` via a case statement.

### Interactive Refinement

The HTML report embeds the full inspection snapshot and exposes include/exclude checkboxes on every package, service, config file, and other inspected item. Operators can deselect items they don't want in the output and click **Re-render** to generate a new Containerfile and updated artifacts without re-running the inspection.

**Workflow:**

1. **Inspect on host:** run yoinkc, collect the output tarball.
2. **Copy to workstation:** `scp target-host:~/hostname-*.tar.gz .`
3. **Start refine:** `./run-yoinkc.sh refine hostname-*.tar.gz` — runs `yoinkc refine` inside the container, serves the report at `http://localhost:8642`, and auto-opens the browser.
4. **Iterate in browser:** toggle items with checkboxes, click Re-render to apply changes, download a refined tarball when done.

**`yoinkc refine`** (`src/yoinkc/refine.py`) is an HTTP server module that:
- Extracts the tarball into a temporary directory and serves `report.html`
- Auto-opens the browser after the server is ready
- Handles re-render requests by calling `yoinkc inspect --from-snapshot --refine-mode` as a subprocess
- Serves the download tarball of the current output state
- Exposes `/api/health` for readiness detection (polled by the JavaScript UI on startup)
- Sets `Cache-Control: no-cache, no-store, must-revalidate` on all responses to prevent stale content after re-renders

**Interactive UI in the HTML report:**

The report embeds the full inspection snapshot as JSON. When served by the refine server, the JavaScript UI activates additional controls beyond the base include/exclude checkboxes:

Every inspected item has an include/exclude checkbox. Unchecking an item sets `include: false` in the embedded snapshot, and the dirty state is tracked client-side. Users and groups have per-row strategy dropdowns (one of `sysusers`, `useradd`, `blueprint`, `kickstart`) that modify the snapshot's strategy field. Apply-all buttons above the user and group tables allow batch strategy changes. Leaf packages cascade: unchecking a leaf package automatically unchecks its auto-dependency packages that aren't depended on by other included leaves.

The sticky footer toolbar reflects three states:

- **Dirty** — changes are pending. The Re-render button is highlighted and shows combined counts (e.g., "Re-render (2 edits, 3 toggles)"). The Download Snapshot button is also active, allowing the modified snapshot JSON to be saved without re-rendering.
- **Clean + helper** — no pending changes, refine server is running. The Re-render button is dimmed. The Download Tarball button is active, packaging the current output state as a `.tar.gz`.
- **Standalone** — report opened directly in a browser without the refine server. Include/exclude checkboxes are hidden, the toolbar is collapsed, and all interactive buttons are disabled. The report is still fully functional as a read-only dashboard.

On startup, the JavaScript probes `http://localhost:{port}/api/health` (with retries) to detect whether the refine server is running and whether re-rendering is available.

## 9. Secret Handling

A dedicated redaction pass runs over all captured file contents **before any output is written or any git operations occur**. This ordering is critical — the redaction pass is not a separate stage that can be skipped, it's a gate that all content must pass through.

### Redaction Layers

**Pattern-based redaction:** Regex patterns for:
- API keys and tokens (AWS, GCP, Azure, GitHub, generic `API_KEY`/`TOKEN` patterns)
- Private key PEM blocks (`-----BEGIN.*PRIVATE KEY-----`)
- Password fields in config files (`password = ...`, `PASSWD=...`, `secret=...`)
- Connection strings with embedded credentials (JDBC, MongoDB, Redis, PostgreSQL URIs)
- Cloud provider credential files
- Shadow entries: field 1 (password hash) is replaced with `REDACTED_SHADOW_HASH_<sha>` when it is a real hash (not `*`, `!`, `!!`, or empty)
- `passwd` GECOS fields (field 4) are scanned for embedded credentials

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
- Location: `"content"` for pattern matches within file text, `"field 2"` for shadow hash fields, or `"entire file"` for excluded paths
- Suggested remediation (e.g., "use a Kubernetes secret", "use a systemd credential", "inject via environment variable at deploy time")

### GitHub Push Guardrails

Pushing to GitHub means network egress from a container with read access to the entire host filesystem. Even with the redaction pass, this requires explicit safeguards:

1. **Explicit opt-in**: GitHub push is never a default. It requires `--push-to-github` with a repository target.
2. **Confirmation gate**: Before pushing, the tool prints a summary of what will be pushed — **total data size on disk**, file count, any `# FIXME` items, count of redacted values — and requires interactive confirmation (`--yes` to skip for automation, but this must be a conscious choice).
3. **Redaction verification**: The push step re-scans the entire output tree for known secret patterns as a second pass. If anything is found that the first pass missed, the push is aborted with an error.
4. **Private repo default**: If creating a new repo, it defaults to private. Creating a public repo requires `--public` flag.

## 10. Future Work & Backlog

### High Priority

**Cross-stream targeting.** The spec at `docs/specs/proposed/2026-03-13-cross-stream-targeting-design.md` is paused at "propose approaches" with 6 open questions. Most impactful unbuilt feature — unlocks RHEL 9→10 upgrades, CentOS→RHEL lateral moves, Fedora→CentOS targeting. Infrastructure partially ready: `baseline.py` has OS→image mapping, schema has `os_release` metadata.

**CI integration.** GitHub Actions workflow: driftify → yoinkc → validate output on CentOS Stream 9, 10, and Fedora. Catches regressions unit tests can't.

**Containerless re-rendering.** `yoinkc-render` entry point: takes snapshot JSON, produces artifacts using only Python. Rendering pipeline is already pure Python — mostly CLI/packaging exercise.

### Medium Priority

**Fleet-aware version comparison.** `VersionChange` does not currently carry a `fleet` field — version drift is per-host only. Fleet merge could aggregate version changes across hosts, showing which downgrades affect the entire fleet vs individual outliers.

**Fleet drift variations.** True within-profile variation in driftify for more realistic fleet test data.

**Enhanced cron-to-timer conversion.** `MAILTO` → journal notifications, `@reboot` handling, environment variable preservation.

**/var size estimation.** Use `du` via executor instead of Python file iteration.

### Lower Priority

**In-place migration.** Run yoinkc on one host, refine, apply across fleet.

**Snapshot diffing.** Compare snapshots across hosts or time for compliance auditing.

**Cross-family migration.** Ubuntu/Debian → RPM bootc. Separate project.

### Tech Debt

All identified tech debt items have been addressed: `containerfile.py` was split into the `renderers/containerfile/` package (14 modules), `report.html.j2` was split into 29 include partials under `templates/report/`, and the three large test files were split into 13 focused files plus a shared `conftest.py`.
