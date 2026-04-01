# Multi-Artifact Architect Decomposition

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark, Birch input)

## Goal

Extend architect's layer decomposition beyond RPM packages to include all artifact types: config files, services, quadlets, scheduled tasks, non-RPM software, users/groups, network, SELinux, and kernel boot. Artifacts follow their parent package into the appropriate layer.

## Context

Architect currently decomposes only RPM packages across fleets into base and derived layers. But a real bootc image isn't just packages — it's configs, services, container workloads, cron jobs, and more. A base image with `httpd` in it but no `httpd.conf` or `systemctl enable httpd` is incomplete.

## Prerequisites

### Phase 0: Expand FleetInput and Loader Plumbing

Before the analyzer can decompose multi-artifact data, the data must actually reach it. Today, `FleetInput` (defined in `src/yoinkc/architect/analyzer.py`) carries only `name`, `packages`, `configs`, `host_count`, and `base_image`. The loader function `_snapshot_to_fleet_input()` (in `src/yoinkc/architect/loader.py`) only extracts `rpm.packages_added` and `config.files` paths from the snapshot.

**Required work:**

1. **Expand `FleetInput`** to carry all artifact types:
   - `services: list[ServiceInput]` — service state changes (unit, action, owning_package)
   - `drop_ins: list[str]` — systemd drop-in paths
   - `quadlets: list[str]` — quadlet unit paths
   - `compose_files: list[str]` — compose file paths
   - `cron_jobs: list[str]` — cron job identifiers
   - `timers: list[str]` — systemd timer identifiers
   - `non_rpm_software: list[str]` — pip packages, standalone binaries
   - `users: list[str]` — non-default user names
   - `groups: list[str]` — non-default group names
   - `firewall_zones: list[str]` — firewall zone configs
   - `network_connections: list[str]` — NM connection paths
   - `selinux_ports: list[str]` — SELinux port label rules
   - `kernel_boot_args: list[str]` — kernel boot arguments

2. **Expand `_snapshot_to_fleet_input()`** to extract from the snapshot:
   - `snapshot.services.state_changes` → services (filter to non-unchanged)
   - `snapshot.services.drop_ins` → drop-in paths
   - `snapshot.containers.quadlet_units` → quadlet paths
   - `snapshot.containers.compose_files` → compose paths
   - `snapshot.scheduled_tasks.cron_jobs` → cron identifiers
   - `snapshot.scheduled_tasks.systemd_timers` → timer identifiers
   - `snapshot.non_rpm_software` → pip packages, binaries
   - `snapshot.users_groups.users` → user names
   - `snapshot.users_groups.groups` → group names
   - `snapshot.network.firewall_zones` → firewall configs
   - `snapshot.network.connections` → NM connection paths
   - `snapshot.selinux.port_labels` → SELinux rules (field name TBD — verify against schema)
   - `snapshot.kernel_boot.kargs` → kernel arguments (field name TBD — verify against schema)

This is a hard prerequisite for all subsequent phases. Without it, the analyzer has no multi-artifact data to decompose.

### Phase 0b: Enrich Config Ownership Data

The config inspector does **not** currently populate RPM ownership on the main path. Specifically:
- `RpmVaEntry.package` is always `None` — set in `_parse_rpm_va()` (in `src/yoinkc/inspectors/rpm.py`) without an `rpm -qf` lookup.
- `ConfigFileEntry.package` inherits this `None` value from `entry.package`.
- The `_get_owning_package()` function (which does call `rpm -qf`) is only invoked in the `--config-diffs` code path, not during normal inspection.

For the "config follows package" rule to work, the owning package must be known. **Required work:**

- **Option A (preferred):** Enrich `_parse_rpm_va()` or the config inspector's main path to call `rpm -qf` for each RPM-owned modified file and populate `ConfigFileEntry.package`. This adds subprocess calls during inspection but is the cleanest data path.
- **Option B:** Add a fallback enrichment step in the architect pipeline — when `ConfigFileEntry.package` is `None` for an `RPM_OWNED_MODIFIED` file, attempt to determine ownership from the package list (e.g., match config paths against known RPM file lists).

Until ownership is populated, the decomposer must fall back to prevalence-based decomposition for RPM-owned configs where `package` is `None`. This is a safe but suboptimal fallback.

## Design

### Package-Follows Rule

**Configs and services follow their parent package.** If `httpd` is decomposed into the base layer, then:
- `/etc/httpd/conf/httpd.conf` → base layer `COPY`
- `systemctl enable httpd` → base layer `RUN`

If a config file has fleet-specific variants:
- The most-prevalent variant (or user-selected winner) goes in the layer that owns the package
- Derived layers can override with their variant via a subsequent `COPY`

If a config file is unowned (not associated with any RPM), it follows the same prevalence-based decomposition as packages: common across all fleets → base, unique to one fleet → derived.

### Prevalence Rule

The current analyzer uses a **100% cross-fleet prevalence** rule: an artifact must appear in every fleet to be placed in the base layer. All other artifacts stay in their fleet's derived layer. This spec preserves that rule for all artifact types.

A configurable threshold (e.g., majority >50%) is explicitly out of scope for this phase. If a threshold mechanism is desired later, it would require:
- A threshold parameter on `analyze_fleets()`
- Changing the prevalence check from `f_set == all_fleet_names` to `len(f_set) / len(all_fleet_names) >= threshold`
- UI controls for the threshold value
- Updated test assertions

### Artifact Type Mapping

| Artifact Type | Decomposition Rule | Containerfile Directive |
|--------------|-------------------|----------------------|
| RPM packages | Prevalence-based (existing) | `RUN dnf install` |
| Config files (RPM-owned) | Follows parent package | `COPY` |
| Config files (unowned) | Prevalence-based (like packages) | `COPY` |
| Services | Follows parent package | `RUN systemctl enable/disable` |
| Service drop-ins | Follows parent service | `COPY` |
| Quadlet units | Prevalence-based | `COPY` to quadlet path |
| Compose files | Prevalence-based | `COPY` |
| Scheduled tasks (cron) | Prevalence-based | `COPY` |
| Scheduled tasks (timers) | Prevalence-based | `COPY` |
| Non-RPM software (pip) | Prevalence-based | `RUN pip install` |
| Non-RPM software (binaries) | Prevalence-based | `COPY` |
| Users/Groups | Prevalence-based | `RUN useradd/groupadd` |
| Network (firewall) | Prevalence-based | Firewall zone config |
| Network (connections) | Prevalence-based | `COPY` |
| SELinux port labels | Prevalence-based | `RUN semanage port` |
| Kernel boot args | Prevalence-based | `COPY` kargs.d drop-in |

### Package-to-Config Association

To implement "config follows package," the decomposer needs to know which configs belong to which packages. This information is modeled in the schema but not reliably populated today (see Phase 0b above):
- `ConfigFileEntry.kind == RPM_OWNED_MODIFIED` — the file is owned by an RPM. The owning package should be in `ConfigFileEntry.package`, but this field is currently `None` on the main inspection path because `RpmVaEntry.package` is not populated via `rpm -qf`.
- `ConfigFileEntry.kind == UNOWNED` — no parent package, decompose independently.
- `ConfigFileEntry.kind == ORPHANED` — parent package was removed, treat as unowned.

When `ConfigFileEntry.package` is `None` for an `RPM_OWNED_MODIFIED` file, the decomposer falls back to prevalence-based decomposition for that file (safe fallback, slightly less optimal).

### Service-to-Package Association

Services follow packages via `ServiceStateChange.owning_package`, which is populated by the service inspector's `_enrich_owning_packages()` function using `rpm -qf`. This field is reliably populated for non-unchanged state changes.
- If `owning_package` is set → service follows that package's layer
- If `owning_package` is `None` → fall back to prevalence-based decomposition

### Architecture Changes

**`src/yoinkc/architect/analyzer.py`** — extend `analyze_fleets()` to process all artifact types, not just `packages`. The `FleetInput` dataclass, `Layer` dataclass, and `LayerTopology` class all live in this file. For each artifact type:
1. Collect items across all fleet inputs
2. Determine which layer each item belongs to (parent-follows or prevalence-based)
3. Assign to layer

The `Layer` dataclass must be extended with fields for all artifact types (services, quadlets, cron_jobs, timers, non_rpm_software, users, groups, firewall_zones, network_connections, selinux_ports, kernel_boot_args). Currently it has only `packages` and `configs`.

**`src/yoinkc/architect/loader.py`** — expand `_snapshot_to_fleet_input()` to extract all artifact types from the snapshot (see Phase 0).

**`src/yoinkc/architect/export.py`** — extend `render_containerfile()` to emit directives for all artifact types in each layer. Currently it only emits `RUN dnf install` for packages.

**`src/yoinkc/templates/architect/`** — update the UI to show all artifact types per layer, not just packages. Group by type within each layer card.

### Variant Handling in Architect

When a config file has variants across fleets:
- If all fleets agree (same content) → goes in the owning package's layer as a single `COPY`
- If fleets disagree (different content) → the most-prevalent variant goes in the owning package's layer, each differing fleet's derived layer gets an override `COPY`
- If tied → flagged as unresolved (same "tied — compare & choose" treatment from refine)

### Package Move/Copy and Attached Artifacts

**Backlogged.** When a package is moved between layers via `move_package()` or `copy_package()` (drag-and-drop in the UI), attached artifacts (owned configs, enabled services) do not currently follow. These methods only operate on the `packages` list.

Implementing "dependents follow" requires:
- Building a reverse index from package → owned configs and services
- Updating `move_package()` and `copy_package()` to also move/copy dependent artifacts
- Handling edge cases (e.g., a config owned by a package that is in base but has fleet-specific variants in derived)

Until this is implemented, manual package moves may leave orphaned configs/services in the wrong layer. Users should be aware that moving a package does not automatically move its associated artifacts. This is acknowledged as a follow-on to the initial multi-artifact decomposition.

### Known Limitations

- **Same package, different service enablement across fleets** — this is a known limitation, not an open question. If `httpd` is in base because all fleets have it, but only web-servers enable the service, the current design puts the enablement in base too. Fleet-specific service enablement overrides require per-fleet service state tracking in the `Layer` model, which is deferred. This was a deliberate scoping decision per Mark's direction.

## Scope

**In scope:**
- Phase 0: Expand `FleetInput` and `_snapshot_to_fleet_input()` to carry all artifact types
- Phase 0b: Enrich config ownership data so `ConfigFileEntry.package` is populated on the main inspection path
- All artifact types in the decomposition algorithm (using 100% cross-fleet prevalence)
- Package-follows rule for RPM-owned configs (when ownership is known) and services
- Prevalence-based decomposition for unowned/independent artifacts
- Extended `Layer` dataclass with all artifact fields (in `analyzer.py`)
- Extended Containerfile export with all directives
- UI updates to show all artifact types per layer
- E2E tests for multi-artifact decomposition

**Out of scope:**
- Configurable prevalence threshold (majority, percentage, etc.)
- Package move/copy carrying attached artifacts (backlogged — see above)
- Drag-and-drop for non-package artifacts
- Fleet-specific service enablement overrides (known limitation — see above)
- Config variant resolution in architect (uses refine's resolution, carried through the snapshot)

## Files to Modify

- Modify: `src/yoinkc/architect/analyzer.py` — extend `FleetInput`, `Layer`, `LayerTopology.to_dict()`, and `analyze_fleets()` for all artifact types
- Modify: `src/yoinkc/architect/loader.py` — extend `_snapshot_to_fleet_input()` to extract all artifact types from snapshot
- Modify: `src/yoinkc/architect/export.py` — extend `render_containerfile()` for all artifact directives
- Modify: `src/yoinkc/inspectors/config.py` or `src/yoinkc/inspectors/rpm.py` — enrich RPM ownership data on the main path (Phase 0b)
- Modify: `src/yoinkc/templates/architect/_js.html.j2` — UI for all artifact types
- Modify: `src/yoinkc/templates/architect/architect.html.j2` — layer card display
- Modify: `tests/e2e/generate-fixtures.py` — architect fixtures need services, quadlets, etc.
- New E2E tests for multi-artifact architect

## Testing

| Test | Assertion |
|------|-----------|
| RPM-owned config follows package to base | Config file with known owning package in base layer appears in base layer's COPY |
| Unowned config decomposes by prevalence | Unowned config present in all fleets → base layer |
| RPM-owned config with unknown owner falls back | Config where `package` is `None` decomposes by prevalence, not parent-follows |
| Service follows package | `systemctl enable httpd` in same layer as `httpd` package (requires `owning_package` set) |
| Quadlet decomposes by prevalence (100%) | Quadlet in all fleets → base layer; quadlet in 2/3 fleets → derived layer (not base) |
| Containerfile export includes all directives | Exported Containerfile has dnf install + COPY configs + systemctl + COPY quadlets |
| UI shows all artifact types per layer | Layer card displays packages, configs, services sections |
| Variant config gets override in derived | Fleet-specific config variant appears as COPY in derived layer |
| FleetInput carries all artifact types | `_snapshot_to_fleet_input()` populates services, quadlets, cron, etc. from snapshot |
| Phase 0b enrichment works | After enrichment, `ConfigFileEntry.package` is non-None for RPM-owned modified files |
