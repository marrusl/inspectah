# Multi-Artifact Architect Decomposition

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Kit (via brainstorm with Mark, Birch input)

## Goal

Extend architect's layer decomposition beyond RPM packages to include all artifact types: config files, services, quadlets, scheduled tasks, non-RPM software, users/groups, network, SELinux, and kernel boot. Artifacts follow their parent package into the appropriate layer.

## Context

Architect currently decomposes only RPM packages across fleets into base and derived layers. But a real bootc image isn't just packages — it's configs, services, container workloads, cron jobs, and more. A base image with `httpd` in it but no `httpd.conf` or `systemctl enable httpd` is incomplete.

## Design

### Package-Follows Rule

**Configs and services follow their parent package.** If `httpd` is decomposed into the base layer, then:
- `/etc/httpd/conf/httpd.conf` → base layer `COPY`
- `systemctl enable httpd` → base layer `RUN`

If a config file has fleet-specific variants:
- The most-prevalent variant (or user-selected winner) goes in the layer that owns the package
- Derived layers can override with their variant via a subsequent `COPY`

If a config file is unowned (not associated with any RPM), it follows the same prevalence-based decomposition as packages: common across all fleets → base, unique to one fleet → derived.

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

To implement "config follows package," the decomposer needs to know which configs belong to which packages. This information is available:
- `ConfigFileEntry.kind == RPM_OWNED_MODIFIED` — the file is owned by an RPM. The owning package can be determined from the RPM database (already captured during inspection via `rpm -qf`).
- `ConfigFileEntry.kind == UNOWNED` — no parent package, decompose independently.
- `ConfigFileEntry.kind == ORPHANED` — parent package was removed, treat as unowned.

If the snapshot doesn't currently store the owning package name for RPM-owned configs, the decomposer falls back to prevalence-based decomposition for those files (safe fallback, slightly less optimal).

### Service-to-Package Association

Services follow packages via the unit file path:
- `httpd.service` → owned by `httpd` package (determinable from RPM database)
- If the owning package can't be determined, fall back to prevalence-based decomposition

### Architecture Changes

**`src/yoinkc/architect/analyzer.py`** — extend `compute_topology()` to process all artifact types, not just `rpm.packages_added`. For each artifact type:
1. Collect items across all fleet inputs
2. Determine which layer each item belongs to (parent-follows or prevalence-based)
3. Assign to layer

**`src/yoinkc/architect/models.py`** — extend the `Layer` dataclass to carry all artifact types, not just packages. Add fields for configs, services, quadlets, etc.

**`src/yoinkc/architect/export.py`** — extend Containerfile generation to emit directives for all artifact types in each layer.

**`src/yoinkc/templates/architect/`** — update the UI to show all artifact types per layer, not just packages. Group by type within each layer card.

### Variant Handling in Architect

When a config file has variants across fleets:
- If all fleets agree (same content) → goes in the owning package's layer as a single `COPY`
- If fleets disagree (different content) → the most-prevalent variant goes in the owning package's layer, each differing fleet's derived layer gets an override `COPY`
- If tied → flagged as unresolved (same "tied — compare & choose" treatment from refine)

### Backlogged Questions

- **Same package, different service enablement across fleets** — what if web-servers enable `httpd` but db-servers don't? Backlogged per Mark's direction. For now: if the package is in base, the service enablement goes in base too. Fleet-specific overrides are a follow-on.

## Scope

**In scope:**
- All artifact types in the decomposition algorithm
- Package-follows rule for RPM-owned configs and services
- Prevalence-based decomposition for unowned/independent artifacts
- Extended Layer model with all artifact fields
- Extended Containerfile export with all directives
- UI updates to show all artifact types per layer
- E2E tests for multi-artifact decomposition

**Out of scope:**
- Drag-and-drop for non-package artifacts
- Fleet-specific service enablement overrides (backlogged)
- Config variant resolution in architect (uses refine's resolution, carried through the snapshot)

## Files to Modify

- Modify: `src/yoinkc/architect/analyzer.py` — extend `compute_topology()`
- Modify: `src/yoinkc/architect/models.py` — extend `Layer` dataclass
- Modify: `src/yoinkc/architect/export.py` — extend Containerfile generation
- Modify: `src/yoinkc/templates/architect/_js.html.j2` — UI for all artifact types
- Modify: `src/yoinkc/templates/architect/*.html.j2` — layer card display
- Modify: `tests/e2e/generate-fixtures.py` — architect fixtures need configs/services
- New E2E tests for multi-artifact architect

## Testing

| Test | Assertion |
|------|-----------|
| RPM-owned config follows package to base | Config file owned by a base-layer package appears in base layer's COPY |
| Unowned config decomposes by prevalence | Unowned config present in all fleets → base layer |
| Service follows package | `systemctl enable httpd` in same layer as `httpd` package |
| Quadlet decomposes by prevalence | Quadlet in 2/3 fleets → base layer at majority threshold |
| Containerfile export includes all directives | Exported Containerfile has dnf install + COPY configs + systemctl + COPY quadlets |
| UI shows all artifact types per layer | Layer card displays packages, configs, services sections |
| Variant config gets override in derived | Fleet-specific config variant appears as COPY in derived layer |
