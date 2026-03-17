# DNF Module Streams and Version Locks Detection

**Date:** 2026-03-16
**Priority:** P0 — Functional correctness
**Status:** Proposed
**Source:** Gap audit items #1 (module streams) and #2 (version locks)

---

## Problem

yoinkc does not detect DNF module streams or version lock pins. Both cause silent wrong-version installs in the generated Containerfile:

- **Module streams:** If the host has `postgresql:15` enabled and the Containerfile runs `dnf install postgresql-server`, DNF resolves to the default stream (e.g., `postgresql:13`) instead of the intended stream 15. The generated image silently runs the wrong version.
- **Version locks:** If the host has `curl` pinned to `7.76.1-26.el9`, the Containerfile's `dnf install curl` pulls the latest available version. The pin — which likely exists for a reason — is silently ignored.

Neither is detected by any inspector. Neither has a schema field. The RPM inspector's `_collect_repo_files()` reads `/etc/dnf/` but only captures `.repo` and `.conf` files, not `.module` files or versionlock lists.

---

## Design

### Schema Changes

New models added to `schema.py`:

```python
class EnabledModuleStream(BaseModel):
    module_name: str          # e.g., "postgresql"
    stream: str               # e.g., "15"
    profiles: List[str] = []  # e.g., ["server"] — from .module file, optional
    include: bool = True      # triage toggle
    baseline_match: bool = False  # True if base image has same module:stream
    fleet: Optional[FleetPrevalence] = None

class VersionLockEntry(BaseModel):
    raw_pattern: str          # the NEVRA pattern as written in the file
    name: str                 # parsed package name
    epoch: int = 0            # epoch (0 if absent from pattern)
    version: str              # e.g., "7.76.1"
    release: str              # e.g., "26.el9"
    arch: str                 # e.g., "x86_64", "*"
    include: bool = True      # triage toggle
    fleet: Optional[FleetPrevalence] = None
```

New fields on `RpmSection`:

- `module_streams: List[EnabledModuleStream] = []`
- `version_locks: List[VersionLockEntry] = []`
- `module_stream_conflicts: List[str] = []` — warning messages like `"postgresql: host=15, base_image=13"`
- `baseline_module_streams: Optional[Dict[str, str]] = None` — cached base image module streams (`module_name → stream`), parallel to `baseline_package_names`. Enables `--from-snapshot` reruns without re-querying the base image.
- `versionlock_command_output: Optional[str] = None` — raw `dnf versionlock list` output, stored for system properties use.

SCHEMA_VERSION bumps to `9.1` (additive, non-breaking).

### Inspector — Module Streams

New method `_collect_module_streams(executor, host_root)` in `rpm.py`:

1. Glob `/host/etc/dnf/modules.d/*.module` files.
2. Each file is INI format. Parse with `configparser`. Sections are module names (e.g., `[postgresql]`) containing `name=`, `stream=`, `profiles=`, `state=` fields.
3. Only capture sections where `state=enabled`. Skip `state=disabled` (explicitly turned off) and `state=installed` (stream was set by installing a module profile — the packages are already captured by the RPM inspector; the stream enablement is what matters and `installed` implies `enabled`). Actually: `state=installed` means the stream IS enabled AND a profile was installed — treat it the same as `enabled` since the stream needs to be enabled in the image.
4. Build `EnabledModuleStream` per enabled/installed section.
5. If the directory doesn't exist or is empty, result is an empty list.

No command fallback needed — the files are the source of truth. This is confirmed by insights-core's `DnfModules` parser which is literally `class DnfModules(IniConfigFile): pass`.

**Note:** The existing `_collect_repo_files()` method reads `/etc/dnf/` and captures files there, but it targets `.repo` and `.conf` extensions. Module files live under `/etc/dnf/modules.d/` (a subdirectory) with `.module` extension. The implementer should verify that `_collect_repo_files` does not double-capture `.module` files; if it does, add an exclusion for `modules.d/` in that method.

### Inspector — Version Locks

New method `_collect_version_locks(executor, host_root)` in `rpm.py`:

1. **File parse (primary):** Read `/host/etc/dnf/plugins/versionlock.list`, fall back to `/host/etc/yum/pluginconf.d/versionlock.list`. Parse non-comment, non-empty lines as NEVRA patterns. Each line becomes a `VersionLockEntry` with parsed `name`, `evr`, `arch` fields.
2. **Command output (secondary):** Run `dnf versionlock list` via executor. Store raw output on the snapshot for future system properties use. The file parse populates the schema; the command output is evidence.

**NEVRA parsing:** Versionlock files contain one pattern per line in varying formats:
- Full NEVRA: `1:curl-7.76.1-26.el9.x86_64` (with epoch prefix)
- No epoch: `curl-7.76.1-26.el9.x86_64` (assume epoch 0)
- Wildcard arch: `curl-7.76.1-26.el9.*`

Parse strategy: split arch after the last `.`. If the remaining string contains `:`, split epoch before the first `:`. Then split name from version-release at the boundary where a `-` is followed by a digit (standard RPM heuristic). Store epoch as `int` (default 0), version and release as separate `str` fields — consistent with `PackageEntry` and needed for correct Containerfile output (epoch must be included in `dnf install` when non-zero: `dnf install 1:curl-7.76.1-26.el9`).

Both methods called from the main `inspect()` flow alongside existing RPM collection, after `_collect_repo_files()`.

### Baseline Comparison — Module Streams

Extend `BaselineResolver` with `query_module_streams(image)`:

1. Run `podman run --rm <image> dnf module list --enabled --quiet` (or parse `/etc/dnf/modules.d/*.module` inside the container — same file parse approach).
2. Return `Dict[str, str]` mapping `module_name → stream`.
3. Called alongside `query_packages()` during baseline resolution. Cached on the snapshot.

After host module streams are collected and baseline is resolved:

- **Match:** Host stream equals base image stream → `baseline_match = True`. Containerfile skips this one.
- **Missing:** Host stream not in base image → `baseline_match = False`. Containerfile emits `dnf module enable`.
- **Conflict:** Same module, different stream → populate `module_stream_conflicts` warning list + `baseline_match = False`.

When `--no-baseline` is used, all streams get `baseline_match = False` (safe default — emit all enables, potentially redundant).

Version locks have no baseline comparison — they are pure host-side intent.

### Containerfile Renderer

In `packages.py`'s `section_lines()`, new blocks in this order:

**1. Module stream enables** — after repo COPYs, before `dnf install`:

```dockerfile
# --- Enabled DNF module streams ---
RUN dnf module enable -y postgresql:15 nginx:1.24
# WARNING: base image has postgresql:13, overriding to postgresql:15
```

Rules:
- Only emit streams where `include=True` and `baseline_match=False`.
- Single `RUN dnf module enable -y` line with all streams (space-separated `name:stream` pairs), sorted alphabetically.
- Conflicting streams get an inline comment with the warning.
- If no streams to emit, skip the block entirely.

**2. Version lock FIXMEs** — above the `dnf install` line (single-host mode):

```dockerfile
# FIXME: The following packages were version-locked on the source system.
# dnf install will pull the latest available version instead.
#   curl-7.76.1-26.el9.x86_64
#   openssl-3.0.7-24.el9.x86_64
```

Only emit entries where `include=True`.

**3. Version lock preserve** — in fleet mode, when a version lock survives the prevalence threshold:

```dockerfile
RUN dnf install -y sudo-1.9.5p2-10.el9
RUN dnf versionlock add sudo-1.9.5p2-10.el9
```

Instead of a FIXME, the locked version is pinned in the install line and the lock is persisted. Locks below threshold or with conflicting versions across the fleet still get FIXMEs.

### HTML Report

New card in the packages tab, between repos and the dependency tree:

**"DNF Module Streams" card:**

| Column | Content |
|--------|---------|
| Module | module_name |
| Stream | stream value |
| Profiles | comma-joined profiles (if any) |
| Status | "Enabled" green badge if `baseline_match`, "Needs enable" blue badge if not |
| Include | triage checkbox |
| Fleet | prevalence cell (fleet mode only) |

Conflict rows get a warning icon + tooltip: "Base image has stream X".

Same `data-snap-section="rpm"` / `data-snap-list="module_streams"` / `data-snap-index` pattern for editor integration and triage.

**"Version-Locked Packages" card:**

| Column | Content |
|--------|---------|
| Package | name |
| Locked Version | formatted as `epoch:version-release` (epoch omitted when 0) |
| Arch | arch |
| Include | triage checkbox |
| Fleet | prevalence cell (fleet mode only) |

### Audit Report

In `_audit_rpm()`:

- Module streams: `"- Module Streams: N enabled (M need enable in image)"`
- Version locks: `"- Version Locks: N packages pinned"`
- Module stream conflicts are added to the snapshot-level `warnings` list (the existing top-level list used by other inspectors) AND rendered in the audit markdown warnings section.

### Fleet Merge

Both lists follow the existing fleet merge pattern:

**Module streams** — merge key is `(module_name, stream)`. Same module+stream across hosts → one entry with prevalence count. Different streams for the same module → separate entries (variants). Profiles are unioned across hosts.

**Version locks** — merge key is `(name, arch)` to correctly handle multi-arch locks (e.g., `curl.x86_64` and `curl.i686` are separate lock entries). Same package+arch locked to the same version → one entry with prevalence. Same package+arch locked to different versions → separate entries with their own prevalence (variant dimension is version). Fleet Containerfile behavior:
- Most prevalent version above threshold → pin in install line + `dnf versionlock add`
- Multiple versions above threshold → pick highest prevalence; tie-break by `rpmvercmp` (newer wins)
- No version above threshold → FIXME comment

### Sidebar Triage

Both lists contribute to the packages tab triage count, filtered by `.include` as usual.

---

## Testing

### driftify additions needed

- Enable a non-default module stream: `dnf module enable postgresql:15` or `dnf module enable nodejs:18` (reliably available in RHEL/CentOS 9 repos; avoid `nginx:mainline` which may not exist in all repo configurations)
- Add a versionlock pin: `dnf install dnf-plugin-versionlock && dnf versionlock add curl`

### Unit tests

- Module file INI parsing (well-formed, empty, missing directory, multiple modules, disabled state)
- Versionlock file parsing (NEVRA patterns, comments, empty lines, both file paths)
- Baseline comparison (match, missing, conflict)
- Containerfile output (module enables, FIXME comments, fleet preserve mode)
- Fleet merge (same stream, different streams, same lock, different lock versions)
- Triage filtering (include/exclude affects Containerfile output)

---

## References

- Gap audit: `yoinkc-gap-audit.md` items #1 and #2
- insights-core `parsers/dnf_modules.py` (confirms INI format)
- leapp `GetEnabledModules` actor (libdnf API approach — not used, file parse preferred)
- convert2rhel `pkghandler.py` (versionlock file path — uses YUM path with DNF symlink)

---

## Out of Scope

- Module stream *installation* (profiles) in Containerfile — yoinkc already handles individual package installs; `dnf module enable` is sufficient
- Automatic versionlock reproduction in single-host mode — operator decision, surfaced as FIXME
- Fleet package version variant picking (separate future spec — different problem shape)
