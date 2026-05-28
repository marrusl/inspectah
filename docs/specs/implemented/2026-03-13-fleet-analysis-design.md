# Fleet-Level Migration Analysis — Aggregation

**Date:** 2026-03-13
**Status:** Proposed

## Problem

Migrating hosts one-by-one with a 1:1 host:image ratio is an anti-pattern
with bootc. For a fleet of 100 similar web servers, the user needs a single
golden image, not 100 Containerfiles. inspectah currently produces per-host
output with no way to find commonality across hosts.

This spec covers **fleet aggregation** — analyzing N inspection snapshots to
produce a merged snapshot representing the fleet. Fleet **collection**
(orchestrating inspectah across hosts and gathering tarballs) is a separate
sub-project with its own future spec.

## Decisions

### Output

A merged `InspectionSnapshot` with fleet metadata (prevalence counts, source
hosts) on each item. The merged snapshot is a valid snapshot that flows
through the existing inspectah pipeline: `inspectah --from-snapshot` for rendering,
`inspectah-refine` for interactive refinement.

Fleet metadata is embedded in the snapshot so downstream consumers can surface
it (e.g., "httpd: 98/100 hosts") without needing separate data files. No
fleet-specific UI for v1 — the existing inspectah-refine works as-is.

### Grouping

Pre-grouped by directory. The user places tarballs from hosts that serve the
same role into a single directory. The tool operates on one directory at a
time, producing one merged snapshot.

Auto-clustering (grouping hosts by package similarity) is out of scope.

### Prevalence Threshold

`--min-prevalence` / `-p` — integer 1-100, default 100. Items present on
fewer than this percentage of hosts get `include: false` in the merged
snapshot. At 100% (default), only items on ALL hosts are included — a strict
intersection. At lower values, the tool absorbs fleet drift (e.g., 90%
includes items on 90+ of 100 hosts).

Below-threshold items remain in the snapshot with `include: false`. The user
can see and re-include them during refinement.

### Tool

Separate entry point: `inspectah-fleet`. Same repo, same package, imports from
inspectah (schema, etc.). Registered as a console script in `pyproject.toml`
alongside `inspectah` and `inspectah-refine`.

Rationale: `inspectah` is associated with "run on a host, inspect the host"
inside a privileged container. `inspectah-fleet` runs on the admin's workstation,
reading snapshot files from a directory. Different runtime context warrants a
distinct entry point, paralleling how `inspectah-refine` is separate because
it's a live HTTP server.

### Scale

Target 100 hosts for v1. All snapshots loaded into memory. No
streaming/chunked processing. Performance optimization for larger fleets is
future work.

## Design

### Algorithm: Union with Prevalence Filtering

1. Load all snapshots from the input directory (tarballs and/or bare JSON).
2. For each section and list, build a union of all items keyed by an identity
   function.
3. For each unique item, count prevalence (how many hosts have it).
4. Apply `--min-prevalence` threshold: items at or above get `include: true`,
   below get `include: false`.
5. Attach `FleetPrevalence` metadata to each item.
6. Write the merged snapshot.

### Item Identity and Merge Rules

Each section type has an identity function that determines "same item" across
hosts. Items with the same identity are counted together for prevalence.

**Identity-only items** (matched by name/key; content differences across hosts
are not meaningful because the Containerfile installs by name, not version):

| Section | List | Identity Key |
|---------|------|-------------|
| `rpm` | `packages_added` | `name` |
| `rpm` | `base_image_only` | `name` |
| `rpm` | `repo_files` | `path` |
| `services` | `state_changes` | `unit` + `action` |
| `network` | `firewall_zones` | `name` |
| `scheduled_tasks` | `generated_timer_units` | `name` |
For identity-only items, the merged entry takes field values from the first
host encountered. Prevalence counts how many hosts have the item by identity.

**Users/groups** are `List[dict]` (not typed Pydantic models), so they cannot
receive a typed `fleet` field. They are deduplicated by `name` key
(`d["name"]`) and prevalence is stored as a plain `"fleet"` dict key injected
into each entry: `{"name": "appuser", ..., "fleet": {"count": 98, "total": 100}}`.
This is pragmatic — promoting users/groups to typed models is a separate
future change.

**Content-bearing items** (same path but different content = different
variants, each with its own prevalence):

| Section | List | Identity Key | Variant Key |
|---------|------|-------------|-------------|
| `config` | `files` | `path` | `content` hash |
| `containers` | `quadlet_units` | `path` | `content` hash |
| `containers` | `compose_files` | `path` | `(service, image)` tuples, sorted, then hashed |
| `services` | `drop_ins` | `path` | `content` hash |

For content-bearing items, the merged snapshot contains one entry per unique
(identity, variant) pair. Each variant gets its own `FleetPrevalence`. For
example, if `/etc/httpd/conf/httpd.conf` exists in 3 content variants across
100 hosts, the merged snapshot has 3 `ConfigFileEntry` items for that path:
version A (95 hosts, `include: true` at 90% threshold), version B (3 hosts,
`include: false`), version C (2 hosts, `include: false`). The user sees all
variants in refinement and picks the right one.

### Per-Field Disposition

Every list field in every section is accounted for below. Disposition is one
of: **merge** (identity/content rules above), **deduplicate** (union by
value), **pass-through** (take from first snapshot), or **omit** (set to
empty/None in merged snapshot).

**`rpm` section:**

| Field | Disposition |
|-------|-------------|
| `packages_added` | merge (by `name`) |
| `base_image_only` | merge (by `name`) |
| `repo_files` | merge (by `path`) |
| `gpg_keys` | merge (by `path`) |
| `rpm_va` | omit — host-specific verification output |
| `dnf_history_removed` | deduplicate — union of all removed package names |
| `leaf_packages` | deduplicate — union (if leaf on any host, leaf in fleet) |
| `auto_packages` | deduplicate — union (if auto on any host, auto in fleet) |
| `leaf_dep_tree` | merge — union of all dep mappings (leaf→[auto deps]) |
| `base_image` | pass-through (require identical, error if mismatched) |
| `baseline_package_names` | pass-through |
| `no_baseline` | pass-through |

**`config` section:**

| Field | Disposition |
|-------|-------------|
| `files` | merge (by `path`, content variants) |

**`services` section:**

| Field | Disposition |
|-------|-------------|
| `state_changes` | merge (by `unit` + `action`) |
| `drop_ins` | merge (by `path`, content variants) |
| `enabled_units` | deduplicate — union of all unit names |
| `disabled_units` | deduplicate — union of all unit names |

**`network` section:**

| Field | Disposition |
|-------|-------------|
| `firewall_zones` | merge (by `name`) |
| `firewall_direct_rules` | omit — low priority for v1 |
| `connections` | omit — host-specific NM connections |
| `static_routes` | omit — host-specific |
| `ip_routes`, `ip_rules` | omit — host-specific |
| `resolv_provenance` | omit |
| `hosts_additions` | omit |
| `proxy` | omit |

**`scheduled_tasks` section:**

| Field | Disposition |
|-------|-------------|
| `generated_timer_units` | merge (by `name`) |
| `cron_jobs` | merge (by `path`) |
| `systemd_timers` | deduplicate — union by `name` |
| `at_jobs` | omit — ephemeral, host-specific |

**`containers` section:**

| Field | Disposition |
|-------|-------------|
| `quadlet_units` | merge (by `path`, content variants) |
| `compose_files` | merge (by `path`, content variants) |
| `running_containers` | omit — ephemeral |

**`users_groups` section:**

| Field | Disposition |
|-------|-------------|
| `users` | deduplicate by `name` key (fleet info as dict key) |
| `groups` | deduplicate by `name` key (fleet info as dict key) |
| `sudoers_rules` | deduplicate — union of all rules |
| `ssh_authorized_keys_refs` | omit — host-specific, security-sensitive |
| `passwd_entries` | omit — host-specific UID assignments |
| `shadow_entries` | omit — host-specific, security-sensitive |
| `group_entries` | omit — host-specific GID assignments |
| `gshadow_entries` | omit |
| `subuid_entries`, `subgid_entries` | omit |

**Sections omitted entirely** (set to `None` in merged snapshot):

- `storage` — host-specific hardware (fstab, mounts, LVM)
- `kernel_boot` — host-specific (cmdline, grub, sysctl overrides)
- `selinux` — low priority for v1
- `non_rpm_software` — complex identity (path-based, many subtypes)

**Top-level lists:**

| Field | Disposition |
|-------|-------------|
| `warnings` | deduplicate by `(source, message)` tuple |
| `redactions` | deduplicate by `(source, message)` tuple |

### Fleet Metadata Format

**`FleetPrevalence` model** — attached to each item in the merged snapshot:

```python
class FleetPrevalence(BaseModel):
    """Fleet prevalence metadata for a merged snapshot item."""
    count: int          # how many hosts have this item
    total: int          # total hosts in the fleet
    hosts: List[str] = Field(default_factory=list)  # hostnames (optional)
```

Added as an optional field to item models that support `include`:

```python
class PackageEntry(BaseModel):
    # ... existing fields ...
    fleet: Optional[FleetPrevalence] = None
```

The field is `None` for single-host snapshots (no behavioral change to
existing code). When present, renderers and the refine UI can display
prevalence information.

Models that gain the `fleet` field: `PackageEntry`, `RepoFile`,
`ConfigFileEntry`, `ServiceStateChange`, `SystemdDropIn`, `FirewallZone`,
`GeneratedTimerUnit`, `QuadletUnit`, `ComposeFile`, `CronJob`.

**`FleetMeta` model** — fleet-level metadata stored in `meta["fleet"]`:

```python
class FleetMeta(BaseModel):
    """Fleet-level metadata for a merged snapshot."""
    source_hosts: List[str]     # hostnames from each input snapshot
    total_hosts: int
    min_prevalence: int         # threshold percentage used
```

Stored as `snapshot.meta["fleet"] = fleet_meta.model_dump()`. The `meta` dict
is already used for pipeline metadata (hostname, timestamp, profile), so
fleet info fits naturally.

**Schema version:** Adding optional fields to existing models is additive.
Existing snapshots are unaffected (`fleet` defaults to `None`). No schema
version bump required.

### Input Validation

All input snapshots must agree on:
- `schema_version` — error if any differ
- `os_release.id` and `os_release.version_id` — error if any differ (the
  merged snapshot's `FROM` line depends on consistent OS identity)
- `rpm.base_image` — error if any differ (baseline package comparison depends
  on a consistent base image)

The merged snapshot takes `os_release` and `rpm.base_image` /
`rpm.baseline_package_names` from the first snapshot.

### Merged Snapshot Metadata

The merged snapshot's `meta` dict is constructed as:
- `meta["hostname"]` — set to the input directory basename (e.g.,
  `"web-servers"` for `./web-servers/`)
- `meta["timestamp"]` — ISO 8601 timestamp of the merge operation
- `meta["fleet"]` — `FleetMeta.model_dump()` (see Fleet Metadata Format)
- Other keys (`profile`, etc.) — omitted

### Error Handling

- Input directory does not exist or is empty → error message, exit 1
- Fewer than 2 snapshots found → error message, exit 1
- Tarball missing `inspection-snapshot.json` → skip with warning, continue
- JSON file fails Pydantic validation → skip with warning, continue
- Schema version mismatch → error message listing mismatched files, exit 1
- os_release mismatch → error message listing mismatched values, exit 1
- base_image mismatch → error message listing mismatched values, exit 1
- Duplicate hostnames → warning (not an error — re-inspected hosts are valid)

If skipping invalid inputs reduces the count below 2, exit with error.

### CLI Interface

```
inspectah-fleet aggregate <input-dir> [options]
```

**Arguments:**
- `<input-dir>` — directory containing inspectah tarballs (`.tar.gz`) and/or
  bare `inspection-snapshot.json` files.

**Options:**
- `--min-prevalence` / `-p` — integer 1-100, default 100.
- `-o` / `--output` — output path for merged snapshot JSON. Default:
  `<input-dir>/fleet-snapshot.json`.
- `--no-hosts` — omit per-item `hosts` lists from fleet metadata (just
  `count` and `total`). For privacy or large fleets.

**Input discovery:** scans `<input-dir>` for `.tar.gz` files (extracts
`inspection-snapshot.json` from each) and `.json` files (parsed directly).
Minimum 2 snapshots required. Validates all snapshots share the same
`schema_version`.

**Output:** a single `inspection-snapshot.json` written to the output path.
The merged snapshot has `meta["fleet"]` set and per-item `fleet` fields
populated.

### Pipeline Compatibility

The merged snapshot has some sections set to `None` (storage, kernel_boot,
selinux, non_rpm_software). The existing renderers already handle `None`
sections gracefully — each section is guarded by `if snapshot.section:` checks.
No prerequisite changes needed.

`inspectah --from-snapshot` accepts a bare JSON file path (not just tarballs).
The merged snapshot is a valid `InspectionSnapshot` and flows through the
existing pipeline without modification.

### Workflow

```
1. Run inspectah on each host → one tarball per host
2. Collect tarballs onto workstation: mkdir web-servers/ && scp ...
3. inspectah-fleet aggregate ./web-servers/ -p 90 -o merged.json
4. inspectah --from-snapshot merged.json    # render Containerfile
   — or —
   inspectah-refine merged.json            # interactive refinement
```

## Scope

**In scope (v1):**
- RPM packages (`packages_added`, `base_image_only`)
- Repo files
- Config files (with content variants)
- Services (`state_changes`, `drop_ins`)
- Firewall zones
- Scheduled tasks (`generated_timer_units`)
- Quadlet units (with content variants)
- Compose files (with content variants)
- Users/groups (deduplicated by name)
- `FleetPrevalence` and `FleetMeta` Pydantic models in schema
- `inspectah-fleet` CLI tool with argparse
- Warnings/redactions (deduplicated by message)

**Out of scope:**
- Fleet collection (SSH/Ansible orchestration)
- Fleet-specific refinement UI
- Layered image hierarchy (common base + role-specific layers)
- Auto-clustering by package similarity
- Non-RPM software, storage, kernel/boot, SELinux aggregation
- Running containers aggregation
- Scale beyond ~100 hosts

## Testing

**Unit tests (pytest):**
- Merge two snapshots with identical packages → one entry, prevalence 2/2
- Merge two snapshots with different packages → two entries, prevalence 1/2
  each, include based on threshold
- Config file with content variants → separate entries per variant
- `--min-prevalence 50` includes items on 1/2 hosts; 100 excludes them
- Identity functions produce correct keys for each section type
- Tarball extraction finds `inspection-snapshot.json`
- FleetPrevalence and FleetMeta serialize/deserialize correctly
- Minimum 2 snapshots enforced
- Single snapshot → error
- All snapshots identical → merged equals original, all prevalence N/N
- Completely disjoint snapshots → nothing in common, all `include: false`
  at threshold 100
- Snapshot with missing/None section → merge proceeds, section omitted
- os_release mismatch → error
- base_image mismatch → error

**Integration tests:**
- End-to-end: directory of test tarballs → merged snapshot → validate against
  schema

## Future Work

- **Fleet collection:** `inspectah-collect` tool or Ansible playbook to push
  inspectah to hosts and gather tarballs automatically.
- **Fleet-aware refinement UI:** surface prevalence data in inspectah-refine
  (e.g., "98/100 hosts" badges, prevalence threshold slider, outlier
  highlighting).
- **Layered image hierarchy:** analyze multiple role groups to produce a
  common base image and role-specific `FROM`-layers. Natural v2 once
  single-group aggregation works.
- **Additional sections:** SELinux, non-RPM software, kernel/boot as demand
  warrants.
- **Scale optimization:** streaming/chunked processing for 1000+ host fleets.
- **Summary/dry-run mode:** preview fleet composition before merging (e.g.,
  "100 snapshots, 342 unique packages, 12 config variants").
