# Package Version Capture & Comparison — Design Spec

**Date:** 2026-03-15
**Status:** Approved
**Scope:** Capture full NEVRA from the baseline image, compare package versions
between host and base image, surface version drift (upgrades and downgrades)
in the HTML report and audit report.

## Motivation

Today, baseline subtraction is name-only: if a package exists in both the host
and the base image, it is silently filtered out regardless of version. This
hides two problems:

1. **Downgrades** — the host has a newer version (e.g., a security patch) that
   the base image does not. After migration, the package silently reverts to
   the older version. This is a regression risk.
2. **Upgrades** — the base image has a newer version than the host. Usually
   fine, but the operator should know about it.

Additionally, the HTML report currently shows added packages by name without
version information, even though `PackageEntry` already captures full NEVRA.

## Goals

1. **Version drift detection** — compare versions of packages present in both
   host and base image. Report downgrades prominently and upgrades
   informationally.
2. **Version display** — show version-release in the HTML report for added
   packages.

## Non-Goals

- No Containerfile changes (no version pinning in `dnf install`).
- No fleet-awareness for version changes (deferred to follow-up spec).
  `VersionChange` does not include a `fleet` field — this can be added
  when fleet-aware version comparison is designed.

## Schema Changes

### New Models

```python
class VersionChangeDirection(str, Enum):
    UPGRADE = "upgrade"      # base image has newer version than host
    DOWNGRADE = "downgrade"  # base image has older version than host

class VersionChange(BaseModel):
    name: str
    arch: str = ""           # e.g. "x86_64" — distinguishes multi-arch
    host_version: str        # e.g. "2.4.57-5.el9"
    base_version: str        # e.g. "2.4.53-11.el9"
    host_epoch: str = "0"
    base_epoch: str = "0"
    direction: VersionChangeDirection
```

### `RpmSection` Addition

```python
version_changes: List[VersionChange] = []
```

Defaults to empty — existing snapshots deserialize without issue.
Bump `SCHEMA_VERSION` to reflect the new capability.

### `base_image_only` Entries

Currently, packages in the base image but not on the host are constructed
with empty version/release/arch. With the baseline now carrying full NEVRA,
these entries should be populated from the baseline `PackageEntry`.

### Added Package Version Display

`PackageEntry` already has `version`, `release`, `epoch`. No schema changes
needed — renderers simply start displaying these fields.

## Baseline Changes

### `query_packages()` — NEVRA Capture

The queryformat changes from names-only to full NEVRA:

```python
# Before:
"rpm", "-qa", "--queryformat", r"%{NAME}\n"
# Returns: Set[str]

# After:
"rpm", "-qa", "--queryformat", r"%{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n"
# Returns: Dict[str, PackageEntry]  (keyed by name.arch)
```

The existing `_parse_nevr()` function handles this format. The return type
changes from `Set[str]` to `Dict[str, PackageEntry]`.

**Multi-arch handling:** Packages may be installed for multiple architectures
simultaneously (e.g., `glibc.x86_64` and `glibc.i686`). The dict is keyed
by `f"{name}.{arch}"` to avoid silently dropping one arch. Name-based set
operations (for the existing added/removed/matched logic) use a derived
`{p.name for p in baseline_packages.values()}` set.

### Caller Updates

All callers update type annotations from `Set[str]` to
`Dict[str, PackageEntry]`. Name-based set operations use `.keys()` or a
derived name set. This is a mechanical change — the callers are:

- `BaselineResolver.get_baseline_packages()` — return type changes
- `BaselineResolver.resolve()` — return type changes
- `load_baseline_packages_file()` — return type changes (auto-detect
  names vs NEVRA format, return `Dict[str, PackageEntry]` for NEVRA,
  `Dict[str, PackageEntry]` with empty version fields for names-only)
- `inspect()` in RPM inspector — where comparison happens
- `preflight_baseline` tuple — type changes from
  `Tuple[Optional[Set[str]], ...]` to
  `Tuple[Optional[Dict[str, PackageEntry]], ...]`
- `section.baseline_package_names` — remains `List[str]` (names only,
  extracted via `.keys()`). Full baseline NEVRA data is not stored on the
  schema — version changes are the computed output.

### `--baseline-packages` File

Graceful degradation:

- **Names only** (current format): version comparison skipped,
  `version_changes` stays empty. Existing behavior preserved.
- **NEVRA lines** (new format): parsed via `_parse_nevr()`, version
  comparison works.

Auto-detected: if a line contains epoch/version separators (`:` and `-`),
treat as NEVRA; otherwise treat as bare name.

## RPM Inspector — Version Comparison

After the existing name-based subtraction, a new step compares versions for
matched packages (packages present in both host and base image):

```python
installed_by_key = {f"{p.name}.{p.arch}": p for p in installed}
for key in matched_keys:
    host_pkg = installed_by_key[key]
    base_pkg = baseline_packages[key]
    cmp = _compare_evr(host_pkg, base_pkg)
    if cmp != 0:
        # cmp > 0 means host is newer → base image downgrades it
        # cmp < 0 means base is newer → base image upgrades it
        direction = DOWNGRADE if cmp > 0 else UPGRADE
        section.version_changes.append(VersionChange(
            name=host_pkg.name,
            host_version=f"{host_pkg.version}-{host_pkg.release}",
            base_version=f"{base_pkg.version}-{base_pkg.release}",
            host_epoch=host_pkg.epoch,
            base_epoch=base_pkg.epoch,
            direction=direction,
        ))
```

**`matched_names` vs `matched_keys`:** The existing added/removed/matched
logic continues to use name-based sets (`matched_names = installed_names &
baseline_name_set`) for backward compatibility. Version comparison uses a
separate `matched_keys = installed_by_key.keys() & baseline_packages.keys()`
computed from `name.arch` keys, ensuring multi-arch packages are compared
against the correct counterpart. These are independent — `matched_names`
drives the existing subtraction, `matched_keys` drives version comparison.

### EVR Comparison

`_compare_evr()` implements RPM's epoch-version-release comparison:

1. **Epoch** compared numerically (higher wins)
2. **Version** and **release** compared segment-by-segment using RPM's
   `rpmvercmp` algorithm (numeric segments as integers, alpha segments
   lexicographically)

Implementation strategy:
- Use `rpm.labelCompare()` from the `rpm` Python bindings if available
  (likely present since yoinkc runs on RPM-based hosts)
- Pure-Python fallback implementing `rpmvercmp` for testing and portability
- The `rpmvercmp` algorithm is well-documented and stable

Packages with identical epoch:version-release are silently ignored — they
do not appear in `version_changes`.

## Renderer Changes

### HTML Report — Packages Section

**Version display on added packages:** Existing leaf/auto/unclassified tables
gain a Version column showing `version-release` for each `PackageEntry`.

**New "Version Changes" subsection:** Appears after the existing package
tables when `version_changes` is non-empty.

- Table columns: Package, Host Version, Base Image Version, Direction
- Downgrades get a warning badge/color (PF6 warning/danger status)
- Upgrades get an informational badge (PF6 info/blue status)
- Sorted: downgrades first, then upgrades, alphabetical within each group

### HTML Report — Warnings Panel

Downgrade entries added when downgrades exist:

> "N package(s) will be downgraded by the base image"

Links to the Version Changes subsection in the packages section. Upgrades
do NOT appear in the warnings panel.

### HTML Report — Audit Report Section

Version drift summary in the Packages subsection:

- Downgrades listed with warning prefix and full version comparison
- Upgrades listed as informational with version comparison
- Both show `host_version → base_version`

### No Containerfile Changes

Per design decision — version drift is an awareness tool, not an
auto-remediation feature.

## Backward Compatibility

- **Schema:** `version_changes` defaults to `[]`. Existing snapshots
  deserialize without issue.
- **Baseline return type:** Internal breaking change. All callers update
  from `Set[str]` to `Dict[str, PackageEntry]`. Few callers, mechanical
  change.
- **`--baseline-packages` file:** Auto-detect format. Names-only continues
  to work (no version comparison, same behavior as today).
- **Fleet:** Unaffected. `version_changes` is empty in merged snapshots
  until fleet-aware version comparison is implemented (separate spec).

## Testing

- **EVR comparison:** Unit tests for the pure-Python `rpmvercmp` — epoch
  trumps version, numeric vs alpha segments, edge cases (missing epoch,
  missing release).
- **Version change detection:** Upgrade, downgrade, and same-version
  (ignored) scenarios.
- **Baseline NEVRA parsing:** `_parse_nevr()` with the new queryformat.
- **`--baseline-packages` format detection:** Names-only graceful
  degradation vs NEVRA parsing.
- **HTML report rendering:** Version Changes subsection appears when
  version_changes is non-empty, absent when empty. Downgrade styling.
  Version column on added packages.
- **Audit report:** Version drift summary with correct direction labels.
- **Warnings panel:** Downgrade count and link. No upgrades in warnings.
- **Snapshot roundtrip:** VersionChange entries survive JSON
  serialization/deserialization. `baseline_package_names` remains names-only
  (no version data stored). Version comparison is not available from
  `--from-snapshot` reloads (acceptable — comparison requires the live
  baseline query).
