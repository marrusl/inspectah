# Architect v2: Multi-Artifact Decomposition

**Date:** 2026-04-07 (revised 2026-04-08)
**Status:** Proposed
**Author:** Kit (Full-Stack Developer)
**Reviewers:** Fern, Ember, Thorn, Slate, Collins, Kit (round 2)
**Supersedes:** `2026-03-31-multi-artifact-architect-design.md`
**UX Spec:** `marks-inbox/reviews/2026-04-07-architect-v2-ux-spec.md` (Fern)
**Synthesis thread:** `comms/threads/2026-04-07-architect-v2-spec-review.md`

---

## Overview

Expand architect from package-only decomposition to multi-artifact decomposition. The architect will decompose 7 artifact types across fleets into a layered bootc image hierarchy: packages (existing) plus 6 new types -- configs, services, firewall rules, quadlets, users/groups, and sysctls.

The tied-changes feature connects artifacts semantically: moving a package moves its configs and services. Moving a config prompts to also move its owning package. This transforms architect from a package list manager into a system architecture tool.

## Context

### What exists today

Architect v1 decomposes **packages only**. The data flow:

1. Refined fleet tarballs land in a directory, each containing `inspection-snapshot.json`
2. `loader.py` extracts package NVRAs and config file paths into `FleetInput`
3. `analyzer.py` runs 100% cross-fleet prevalence: packages in ALL fleets go to base, remainder stays in derived layers
4. `server.py` serves an interactive web UI (PatternFly 6, Jinja2 + vanilla JS)
5. `export.py` generates a `.tar.gz` with Containerfiles per layer

Configs are loaded into `FleetInput.configs` and displayed read-only in the drawer, but cannot be moved/copied and are not decomposed across layers.

The inspection pipeline already captures all 6 new artifact types in `InspectionSnapshot`. The data exists -- architect just does not use it.

### What changes

1. `FleetInput` expands to carry 6 new artifact types as structured objects (not just strings)
2. `analyzer.py` decomposes all artifact types using prevalence + parent-follows rules
3. The drawer gets 7 flat tabs with move/copy support for every artifact type
4. Tied changes connect packages to their configs and services
5. `export.py` delegates to existing renderer modules via an adapter layer (not a parallel pipeline)
6. Config ownership enrichment runs by default during inspection

---

## Decisions Log

These decisions are settled. Not open questions.

| # | Decision | Rationale |
|---|----------|----------|
| 1 | **6 artifact types in scope:** configs, services, firewall rules, quadlets, users/groups, sysctls | Focused set per Mark. Defers SELinux, certs, storage, scheduled tasks to v3. |
| 2 | **Tied changes in scope** | Moving a package moves its configs and services. Moving a config prompts to move its owning package. Copying a package copies its tied configs/services. |
| 3 | **Confirmation strip pattern** | When moving a package with dependents, the row expands inline to show dependent artifacts with checkboxes. Zero-dependent packages move immediately. Always show the strip if any dependents exist, even if just 1. |
| 4 | **Reverse prompt pattern** | Moving an RPM-owned config shows "Also move owning package httpd? [Yes] [No, just this]". Unowned configs move immediately. Same pattern for services with `owning_package`. |
| 5 | **7 flat tabs** | Packages (existing) + Configs + Services + Firewall + Quadlets + Users/Groups + Sysctls. Packages is the default tab. Count badges per tab. |
| 6 | **Config filtering happens upstream, not in architect** | Architect shows whatever the refined fleet output contains. No "Show all defaults" toggle needed. Data is already curated by the refine step. |
| 7 | **Config ownership enrichment is default behavior** | `rpm -qf` lookup runs during normal inspection, populating `ConfigFileEntry.package`. Not gated behind `--config-diffs`. |
| 8 | **Driftify fixture changes in scope** | Fixtures need realistic multi-artifact data. |
| 9 | **Cross-tab "Related" indicator** | Package rows show "3 related: 2 configs, 1 service" as clickable link. Config/service rows show "Owned by: httpd" linking back. |
| 10 | **4-phase implementation** | Phase 1: tabs + data plumbing. Phase 2: move/copy + export for independent artifact types (quadlets, firewall, sysctls, unowned configs). Phase 3: package + owned-artifact moves with tied changes. Phase 4: related indicators + polish. |
| 11 | **Tied changes for copy-to: yes** | Copying a package to a sibling layer also copies its configs/services. Confirmation strip shown with same UX as move. |
| 12 | **Turbulence stays package-based** | Configs and services are COPY directives, not rebuild triggers. Adding them would dilute the metric. |
| 13 | **Renderer reuse via adapter** | Architect export delegates to existing `renderers/containerfile/` domain modules through an adapter layer. No parallel renderer pipeline. (See Export Architecture.) |
| 14 | **Artifact maturity tiers** | Artifact types ship at different maturity levels. Full-support types get decompose + move/copy + export. Visible-only types get decompose + display but export as comments/warnings. (See Artifact Maturity Tiers.) |
| 15 | **Users/groups display-only in v2** | Users/groups tab shows inspection data for operator awareness, but no Containerfile output is produced. The inspection pipeline does not yet classify service accounts reliably enough for safe export subsetting. SSH keys, sudoers, shadow data remain out of scope. (See Users/Groups Scope.) |

---

## Artifact Maturity Tiers

Not all artifact types ship at the same confidence level. Maturity tiers are **orthogonal** to the 4 implementation phases -- a Phase 1 tab can display a visible-only artifact, and a full-support artifact can gain move/copy in Phase 2. All 7 tabs exist from Phase 1; export maturity varies.

| Tier | Capability | Artifact Types |
|------|-----------|---------------|
| **Full support** | Decompose + move/copy + export directives | Packages, configs, services, quadlets |
| **Visible + decompose** | Show in tabs, decompose across layers, but export as `# REVIEW:` comments with warnings -- not executable directives | Firewall zones, sysctls |
| **Display-only** | Show in tab for inspection context, decompose across layers, but NO export -- not even comments in the Containerfile | Users/groups |

### Why this split

**Full-support types** have well-understood, deterministic export semantics:
- Packages: `RUN dnf install` (proven in v1)
- Configs: `COPY tree/ /` (existing renderer handles staged config trees)
- Services: `RUN systemctl enable/disable/mask` (existing renderer has ordering/timer logic)
- Quadlets: `COPY tree/ /` for `.container`/`.volume`/`.network` files (existing renderer handles this)

**Visible-only types** have export semantics that require human judgment:
- **Firewall zones:** Can embed environment-specific assumptions (site IPs, interface names). Zone XML files may carry secrets in rich rules. Export as `# REVIEW: firewall zone 'public' with services [http, https] -- verify before baking into image`.
- **Sysctls:** Most are benign (`vm.swappiness`), but some carry security implications (`kernel.randomize_va_space=0`, `net.ipv4.ip_forward=1`). Export as `# REVIEW: sysctl net.ipv4.ip_forward=1 -- verify security intent before baking`.

Visible-only types are still decomposed (prevalence-based placement), displayed in tabs with count badges, and support move/copy in the UI. The restriction is only on export: their Containerfile output is comments, not directives, until the type is promoted to full support in a future version.

**Display-only types** provide inspection context but do not export at all -- not even as comments in the Containerfile:
- **Users/groups:** The existing renderer (`users_groups.py`) handles strategy-aware provisioning (`sysusers.d`, `useradd`, `blueprint`, `kickstart`), but the current inspection pipeline does not capture service-account classification cleanly enough for architect to safely subset "exportable" users from the rest. Rather than claiming a safe subset that the pipeline does not actually support, v2 shows users/groups in their tab for visibility only. The tab displays strategy classification, UID/GID, and shell information so the operator understands what users exist, but no Containerfile output is produced. Promotion to visible-only or full-support is deferred until the inspection pipeline provides reliable service-account classification.

---

## Per-Artifact Identity and Equivalence Rules

When comparing artifacts across fleets for prevalence-based decomposition, each type needs a defined identity key. Two artifacts from different fleets are "the same" if they share the same identity key.

| Type | Identity Key | Equivalence Rule | Example |
|------|-------------|-----------------|---------|
| Package | NVRA string | Exact NVRA match | `httpd-2.4.57-5.el9.x86_64` |
| Config | File path | Same absolute path | `/etc/httpd/conf/httpd.conf` |
| Service | Unit name + action | Same unit AND same action | `httpd.service:enable` |
| Firewall zone | Zone name | Same zone name (content may differ) | `public` |
| Quadlet | File path | Same absolute path | `/etc/containers/systemd/app.container` |
| User/Group | `kind:name` | Same kind AND same name | `user:appuser` |
| Sysctl | Key name | Same sysctl key (value may differ) | `net.ipv4.ip_forward` |

### Variant handling (same identity, different content)

When two fleets have the same identity key but different content (e.g., same config path with different file contents, same sysctl key with different values, same firewall zone with different services/ports):

- **Prevalence still applies:** If ALL fleets have the artifact, it goes to base. The content difference is a **variant** that the operator must resolve, not a reason to split placement.
- **Variant flag:** The artifact gets an `approximate: true` flag in the topology JSON, surfaced as an approximation badge in the UI (see Approximation Badges below).
- **Export behavior:** Unresolved variants downgrade to `warn_only` for export regardless of their artifact type's maturity tier. They never enter `tree/`, never produce `COPY`/`RUN` directives — only a `# REVIEW: variant detected across fleets` comment in the Containerfile. The operator must resolve the variant (in refine or manually) before the artifact becomes exportable. See Export Safety Classification for the full boundary contract.

---

## Export Safety Classification

Per-artifact export policy. This table governs what `export.py` does for each artifact type. "Exportable" means the adapter emits executable Containerfile directives. "Warn-only" means comments with `# REVIEW:` prefix. "Deferred" means excluded from export entirely.

| Artifact Type | Classification | Export Behavior | Safety Notes |
|--------------|---------------|-----------------|-------------|
| Packages | **Exportable** | `RUN dnf install` | Proven in v1. No additional gates. |
| Configs (RPM-owned modified) | **Exportable** | `COPY tree/{path} {path}` | File content staged into `tree/` directory. Existing `_config_tree.py` renderer handles this. |
| Configs (unowned) | **Exportable** | `COPY tree/{path} {path}` | Same mechanism. Operator placed these files intentionally. |
| Configs (orphaned) | **Warn-only** | `# REVIEW: orphaned config {path}` | Package that owned this was removed. May be stale. |
| Services (enable/disable) | **Exportable** | `RUN systemctl enable/disable` | Existing `services.py` renderer handles ordering and timer dependencies. |
| Services (mask) | **Warn-only** | `# REVIEW: masked service {unit}` | Masking can hide security-relevant behavior. Operator should confirm intent. |
| Firewall zones | **Warn-only** | `# REVIEW: firewall zone '{name}'` | Can embed environment-specific IPs, interface names, credentials in rich rules. |
| Quadlets | **Exportable** | `COPY tree/{path} {path}` | File-based. But emit `# REVIEW: check for privileged settings` if the quadlet contains `Privileged=true`, `SecurityLabelDisable=true`, or `AddCapability=` directives. |
| Users/groups (all) | **Display-only** | Not exported | Shown in tab for inspection context. No Containerfile output (not even comments). Strategy classification displayed in the UI for operator awareness. Promotion to exportable deferred until inspection pipeline provides reliable service-account subsetting. |
| Sysctls (safe) | **Warn-only** | `# REVIEW: sysctl {key}={value}` | All sysctls exported as comments for now. Operator reviews, then can manually convert to `printf > /etc/sysctl.d/`. |
| Sysctls (security-sensitive) | **Warn-only + flag** | `# WARNING: security-sensitive sysctl {key}` | Denylist: `kernel.randomize_va_space`, `kernel.kptr_restrict`, `kernel.dmesg_restrict`, `kernel.yama.ptrace_scope`, `net.ipv4.conf.*.accept_redirects`, `selinux=0` (karg). Flagged with explicit warning. |

### Warn-only / deferred: bright-line negative contract

This is a hard boundary, not a guideline:

1. **No tree/ staging.** Artifacts classified `warn_only`, `deferred`, or `display_only` never enter the `tree/` staging directory. They are never arguments to `_config_tree.py`'s `write_config_tree()` or any file-writing helper.
2. **No COPY/RUN directives.** These artifacts never flow through `_config_tree.py`, `config.py`, `services.py`, `users_groups.py`, or any other renderer module in a way that produces `COPY` or `RUN` lines.
3. **Comment-only output.** In preview and export, warn_only artifacts emit only `# REVIEW:` or `# WARNING:` comment lines. Deferred artifacts emit only `# DEFERRED:` comment lines. Display-only artifacts produce no Containerfile output at all.
4. **Unresolved variants downgrade.** If a nominally full-support artifact has `approximate=True` (divergent content across fleets and the variant is unresolved by the operator), it downgrades to `warn_only` for export purposes. It does not enter `tree/`, does not produce `COPY`/`RUN` directives, and emits a `# REVIEW: variant detected` comment instead.

The adapter enforces this contract: `build_layer_render_input()` excludes warn_only, deferred, display_only, and unresolved-variant artifacts from `LayerRenderInput`. `render_layer_containerfile()` reads these excluded artifacts directly from the `Layer` to emit comment-only output.

### Kernel boot args (kargs)

Kernel boot arguments are NOT modeled as a separate artifact type in v2 (they are part of the `kernel_boot` section in the snapshot). If sysctls are present, the export emits a `# NOTE: kernel boot args detected -- see inspection report for kargs.d recommendations` comment. Full kargs export (via `bootc`-native `/usr/lib/bootc/kargs.d/`) is deferred to v3.

### Sysctl security denylist

The following sysctl keys trigger a `# WARNING` instead of a plain `# REVIEW` comment in export:

```python
SECURITY_SENSITIVE_SYSCTLS = {
    "kernel.randomize_va_space",
    "kernel.kptr_restrict",
    "kernel.dmesg_restrict",
    "kernel.yama.ptrace_scope",
    "kernel.perf_event_paranoid",
    "kernel.sysrq",
    "net.ipv4.conf.all.accept_redirects",
    "net.ipv4.conf.default.accept_redirects",
    "net.ipv6.conf.all.accept_redirects",
    "net.ipv6.conf.default.accept_redirects",
    "net.ipv4.conf.all.send_redirects",
    "net.ipv4.conf.all.accept_source_route",
    "net.ipv4.icmp_echo_ignore_all",
}
```

---

## Users/Groups Scope

v2 treats users/groups as **display-only**. The Users/Groups tab shows what the inspection pipeline captured so the operator has visibility, but architect does not produce any Containerfile output for users or groups -- not executable directives, not comments, nothing.

**Why not "service-account-safe" export?** The existing renderer (`renderers/containerfile/users_groups.py`) has sophisticated strategy-aware provisioning logic (sysusers, useradd, blueprint, kickstart). However, the current inspection pipeline does not classify users into these strategies reliably enough for architect to safely subset "definitely exportable service accounts" from "maybe-human, maybe-ambiguous users." Rather than promising a safe boundary that the data does not actually support, v2 defers all user/group export.

| User/Group State | v2 Behavior | Export |
|---------------------|------------|--------|
| **All users and groups** | Visible in tab, decompose across layers, move/copy in UI | **None.** No Containerfile output. |
| **SSH keys** | Not shown | Out of scope for v2. Sensitive credential data. |
| **Sudoers rules** | Not shown | Out of scope for v2. Privilege escalation data. |
| **Shadow/password data** | Never exported | Existing guardrail. Shadow entries are never included in snapshots. |

The `UserGroupInput` dataclass carries inspection metadata for display purposes:

```python
@dataclass
class UserGroupInput:
    """User or group entry for architect decomposition."""
    name: str
    kind: str  # "user" or "group"
    uid_or_gid: str = ""
    strategy: str = ""  # "sysusers", "useradd", "kickstart", "blueprint", "" -- for display only
    shell: str = ""  # login shell, for display
```

The loader populates `strategy` and `shell` from the snapshot's user provisioning data. These fields are used for display in the tab (showing the operator what strategy was detected) but are NOT used for export classification -- all users/groups have `export_class: "display_only"` regardless of strategy.

---

## Approximation Badges

When the decomposition is approximate -- unknown config ownership, divergent service enablement across fleets, variant content for the same identity key -- the UI must surface that state visibly. Silent approximation erodes operator trust.

### Badge states

| Badge | Meaning | Appears on | Trigger |
|-------|---------|-----------|---------|
| **Approximate** | Content differs across source fleets | Any artifact row | Same identity key, different content in 2+ fleets |
| **Ownership unknown** | RPM-owned-modified config with `package=None` | Config rows | `kind == "rpm_owned_modified"` and `package is None` |
| **Deferred export** | Will not be exported as executable directive | Any artifact row | Artifact classification is warn-only or deferred |
| **Display only** | Shown for inspection context; no export at all | Users/groups rows | Artifact classification is display_only |
| **Security review** | Contains security-sensitive settings | Quadlet rows, sysctl rows | Quadlet has privileged flags, or sysctl key is on denylist |

### Topology JSON additions

Each artifact in the topology JSON gains optional badge fields:

```javascript
{
  "path": "/etc/httpd/conf/httpd.conf",
  "kind": "rpm_owned_modified",
  "owner_package": "httpd",
  "category": "other",
  "approximate": false,        // true if variant detected
  "export_class": "exportable" // "exportable", "warn_only", "deferred", "display_only"
}
```

The `export_class` field is computed at decomposition time based on the Export Safety Classification table. The frontend reads it to render the appropriate badge.

### Rendering

Badges use PatternFly 6 `pf-v6-c-label` component:
- `pf-m-orange` for Approximate
- `pf-m-gold` for Ownership unknown
- `pf-m-grey` for Deferred export
- `pf-m-red` for Security review

Badges appear inline after the artifact name/path, before the action buttons. They are always visible -- not hidden behind a hover or expand. See Fern's UX spec section 7 for the full state inventory and interaction details.

---

## Backend Changes

### FleetInput Expansion

The `FleetInput` dataclass in `analyzer.py` expands from simple string lists to structured artifact objects. This is the core data pipeline change.

**Current:**
```python
@dataclass
class FleetInput:
    name: str
    packages: list[str]
    configs: list[str]
    host_count: int = 0
    base_image: str = ""
```

**New:**
```python
@dataclass
class ConfigInput:
    """Config file for architect decomposition."""
    path: str
    kind: str  # "rpm_owned_modified", "unowned", "orphaned"
    package: str | None = None  # owning RPM, if known
    category: str = "other"
    source_fleet: str = ""  # fleet name that provided this artifact (e.g., "web-servers")
    source_ref: str = ""  # path within fleet tarball (e.g., "config/etc/httpd/conf/httpd.conf")

@dataclass
class ServiceInput:
    """Service state change for architect decomposition."""
    unit: str
    action: str  # "enable", "disable", "mask"
    owning_package: str | None = None

@dataclass
class FirewallInput:
    """Firewall zone for architect decomposition."""
    name: str
    path: str
    services: list[str] = field(default_factory=list)
    ports: list[str] = field(default_factory=list)

@dataclass
class QuadletInput:
    """Quadlet unit file for architect decomposition."""
    path: str
    name: str
    image: str = ""
    source_fleet: str = ""  # fleet name that provided this artifact (e.g., "web-servers")
    source_ref: str = ""  # path within fleet tarball (e.g., "quadlet/app.container")

@dataclass
class UserGroupInput:
    """User or group entry for architect decomposition."""
    name: str
    kind: str  # "user" or "group"
    uid_or_gid: str = ""
    strategy: str = ""  # "sysusers", "useradd", "kickstart", "blueprint", ""
    shell: str = ""  # login shell, for classification

@dataclass
class SysctlInput:
    """Sysctl override for architect decomposition."""
    key: str
    value: str  # runtime value
    source: str = ""  # source file path

@dataclass
class FleetInput:
    name: str
    packages: list[str]
    configs: list[ConfigInput] = field(default_factory=list)
    services: list[ServiceInput] = field(default_factory=list)
    firewall_zones: list[FirewallInput] = field(default_factory=list)
    quadlets: list[QuadletInput] = field(default_factory=list)
    users_groups: list[UserGroupInput] = field(default_factory=list)
    sysctls: list[SysctlInput] = field(default_factory=list)
    host_count: int = 0
    base_image: str = ""
```

### Layer Expansion

The `Layer` dataclass stores decomposed artifacts per layer:

**Current:**
```python
@dataclass
class Layer:
    name: str
    parent: str | None
    packages: list[str] = field(default_factory=list)
    configs: list[str] = field(default_factory=list)
    fleets: list[str] = field(default_factory=list)
    fan_out: int = 0
    turbulence: float = 0.0
```

**New:**
```python
@dataclass
class Layer:
    name: str
    parent: str | None
    packages: list[str] = field(default_factory=list)
    configs: list[ConfigInput] = field(default_factory=list)
    services: list[ServiceInput] = field(default_factory=list)
    firewall_zones: list[FirewallInput] = field(default_factory=list)
    quadlets: list[QuadletInput] = field(default_factory=list)
    users_groups: list[UserGroupInput] = field(default_factory=list)
    sysctls: list[SysctlInput] = field(default_factory=list)
    fleets: list[str] = field(default_factory=list)
    fan_out: int = 0
    turbulence: float = 0.0
```

### Decomposition Rules

The `analyze_fleets()` function applies two decomposition strategies:

**1. Parent-follows rule (for owned artifacts):**
- RPM-owned configs (`ConfigInput.package` is set) follow their owning package to whatever layer it lands in.
- Services with `owning_package` follow their owning package.
- If `ConfigInput.package` is `None` for an `rpm_owned_modified` config, fall back to prevalence.

**2. Prevalence rule (for independent artifacts):**
- Unowned/orphaned configs, firewall zones, quadlets, users/groups, sysctls: 100% cross-fleet prevalence. Present in ALL fleets -> base layer. Otherwise -> derived layer.
- Services without `owning_package`: prevalence-based, keyed on `unit:action`.

**Algorithm sketch:**
```python
def analyze_fleets(fleets: list[FleetInput]) -> LayerTopology:
    # 1. Decompose packages (unchanged from v1)
    #    -> base_packages, per-fleet derived_packages

    # 2. Build package-to-layer index
    #    pkg_layer: dict[str, str]  (package_name -> layer_name)

    # 3. Decompose owned artifacts (parent-follows)
    #    For each fleet:
    #      For each config with config.package:
    #        target_layer = pkg_layer.get(config.package, fleet_layer)
    #        Assign config to target_layer
    #      For each service with service.owning_package:
    #        target_layer = pkg_layer.get(service.owning_package, fleet_layer)
    #        Assign service to target_layer

    # 4. Decompose unowned artifacts (prevalence)
    #    For each artifact type (unowned configs, firewall, quadlets, etc.):
    #      Build cross-fleet index: identity_key -> set[fleet_names]
    #      If present in ALL fleets -> base layer
    #      Otherwise -> derived layer

    # 5. Detect variants (same identity key, different content)
    #    For each artifact placed in base:
    #      If content differs across source fleets:
    #        Mark artifact with approximate=True
```

The parent-follows rule runs AFTER package decomposition, so it knows which layer each package landed in.

### Topology Serialization

`LayerTopology.to_dict()` expands to include structured artifact data per layer, including badge fields:

```python
def to_dict(self) -> dict:
    return {
        "layers": [
            {
                "name": l.name,
                "parent": l.parent,
                "packages": l.packages,
                "configs": [
                    {"path": c.path, "kind": c.kind,
                     "owner_package": c.package, "category": c.category,
                     "approximate": getattr(c, '_approximate', False),
                     "export_class": _config_export_class(c)}
                    for c in l.configs
                ],
                "services": [
                    {"unit": s.unit, "action": s.action,
                     "owner_package": s.owning_package,
                     "approximate": getattr(s, '_approximate', False),
                     "export_class": _service_export_class(s)}
                    for s in l.services
                ],
                "firewall_zones": [
                    {"name": f.name, "path": f.path,
                     "services": f.services, "ports": f.ports,
                     "approximate": getattr(f, '_approximate', False),
                     "export_class": "warn_only"}
                    for f in l.firewall_zones
                ],
                "quadlets": [
                    {"path": q.path, "name": q.name, "image": q.image,
                     "approximate": getattr(q, '_approximate', False),
                     "export_class": _quadlet_export_class(q)}
                    for q in l.quadlets
                ],
                "users_groups": [
                    {"name": ug.name, "kind": ug.kind,
                     "uid_or_gid": ug.uid_or_gid,
                     "strategy": ug.strategy,
                     "approximate": getattr(ug, '_approximate', False),
                     "export_class": _user_export_class(ug)}
                    for ug in l.users_groups
                ],
                "sysctls": [
                    {"key": s.key, "value": s.value, "source": s.source,
                     "approximate": getattr(s, '_approximate', False),
                     "export_class": _sysctl_export_class(s)}
                    for s in l.sysctls
                ],
                "fleets": l.fleets,
                "fan_out": l.fan_out,
                "turbulence": round(l.turbulence, 1),
            }
            for l in self.layers
        ],
        "fleets": [
            {"name": f.name, "host_count": f.host_count,
             "total_packages": f.total_packages}
            for f in self.fleets
        ],
    }
```

**Export class helper functions:**

```python
def _config_export_class(c: ConfigInput) -> str:
    if c.kind == "orphaned":
        return "warn_only"
    return "exportable"

def _service_export_class(s: ServiceInput) -> str:
    if s.action == "mask":
        return "warn_only"
    return "exportable"

def _quadlet_export_class(q: QuadletInput) -> str:
    # Will be refined during implementation to check quadlet content
    # for privileged flags. Default: exportable.
    return "exportable"

def _user_export_class(ug: UserGroupInput) -> str:
    # All users/groups are display-only in v2 -- no export of any kind
    return "display_only"

def _sysctl_export_class(s: SysctlInput) -> str:
    # All sysctls are warn_only in v2
    return "warn_only"
```

### Move/Copy Operations

Generalize `move_package()` and `copy_package()` to handle all artifact types:

```python
def move_artifact(
    self,
    artifact_id: str,
    artifact_type: str,  # "package", "config", "service", "firewall_zone",
                         # "quadlet", "user_group", "sysctl"
    from_layer: str,
    to_layer: str,
    dependents: list[dict] | None = None,
) -> None:
    """Move an artifact between layers, optionally with dependents.

    Base -> derived broadcast: same semantics as packages. Removing from
    base broadcasts the artifact to ALL derived layers.

    Dependents: when provided, each dependent is also moved in the same
    operation. Used by the tied-changes feature.
    """

def copy_artifact(
    self,
    artifact_id: str,
    artifact_type: str,
    from_layer: str,
    to_layer: str,
    dependents: list[dict] | None = None,
) -> None:
    """Copy an artifact to another layer without removing from source."""
```

**Artifact identity keys** (matches the Per-Artifact Identity table above):

| Type | Identity key field | Lookup method |
|------|-------------------|---------------|
| package | NVRA string | Match in `layer.packages` list |
| config | `path` | Match `ConfigInput.path` |
| service | `unit:action` | Match `ServiceInput.unit` (action is part of identity for prevalence, but move targets the unit) |
| firewall_zone | `name` | Match `FirewallInput.name` |
| quadlet | `path` | Match `QuadletInput.path` |
| user_group | `kind:name` | Match `UserGroupInput.kind` + `UserGroupInput.name` |
| sysctl | `key` | Match `SysctlInput.key` |

The existing `move_package()` and `copy_package()` methods remain as backward-compatible wrappers that call `move_artifact(type="package")`.

### Loader Changes

`_snapshot_to_fleet_input()` in `loader.py` expands to extract all artifact types from the snapshot:

```python
def _snapshot_to_fleet_input(snapshot: dict) -> FleetInput:
    # Packages (unchanged)
    rpm = snapshot.get("rpm", {})
    packages = [...]

    # Configs
    config = snapshot.get("config", {})
    configs = [
        ConfigInput(
            path=f["path"],
            kind=f.get("kind", "unowned"),
            package=f.get("package"),
            category=f.get("category", "other"),
            source_fleet=fleet_name,
            source_ref=f"config{f['path']}",  # maps to tarball's config/ tree
        )
        for f in config.get("files", [])
        if f.get("path") and f.get("include", True)
    ]

    # Services (non-unchanged only)
    services_section = snapshot.get("services", {})
    services = [
        ServiceInput(
            unit=s["unit"],
            action=s.get("action", "unchanged"),
            owning_package=s.get("owning_package"),
        )
        for s in services_section.get("state_changes", [])
        if s.get("action", "unchanged") != "unchanged"
    ]

    # Firewall zones
    network = snapshot.get("network", {})
    firewall_zones = [
        FirewallInput(
            name=z["name"],
            path=z.get("path", ""),
            services=z.get("services", []),
            ports=z.get("ports", []),
        )
        for z in network.get("firewall_zones", [])
        if z.get("include", True)
    ]

    # Quadlets
    containers = snapshot.get("containers", {})
    quadlets = [
        QuadletInput(
            path=q["path"],
            name=q["name"],
            image=q.get("image", ""),
            source_fleet=fleet_name,
            source_ref=f"quadlet/{q['name']}",  # maps to tarball's quadlet/ dir
        )
        for q in containers.get("quadlet_units", [])
        if q.get("include", True)
    ]

    # Users and groups (with strategy classification)
    ug_section = snapshot.get("users_groups", {})
    users_groups = [
        UserGroupInput(
            name=u["name"], kind="user",
            uid_or_gid=str(u.get("uid", "")),
            strategy=u.get("strategy", ""),
            shell=u.get("shell", ""),
        )
        for u in ug_section.get("users", [])
    ] + [
        UserGroupInput(
            name=g["name"], kind="group",
            uid_or_gid=str(g.get("gid", "")),
        )
        for g in ug_section.get("groups", [])
    ]

    # Sysctls
    kernel_boot = snapshot.get("kernel_boot", {})
    sysctls = [
        SysctlInput(
            key=s["key"],
            value=s.get("runtime", ""),
            source=s.get("source", ""),
        )
        for s in kernel_boot.get("sysctl_overrides", [])
        if s.get("include", True)
    ]

    return FleetInput(
        name=hostname,
        packages=packages,
        configs=configs,
        services=services,
        firewall_zones=firewall_zones,
        quadlets=quadlets,
        users_groups=users_groups,
        sysctls=sysctls,
        host_count=host_count,
        base_image=base_image,
    )
```

### Export Architecture: Renderer Adapter

**Decision:** Architect export delegates to existing `renderers/containerfile/` domain modules. It does NOT build a parallel renderer pipeline.

The existing containerfile renderer package has domain modules that each accept an `InspectionSnapshot` (Pydantic model) and return `list[str]` via a `section_lines(snapshot) -> list[str]` function:
- `packages.py` -- `RUN dnf install` (also accepts `config_dir`, `min_prevalence`)
- `services.py` -- `RUN systemctl enable/disable` (handles timer deps, orphan-unit filtering, ordering)
- `config.py` -- `COPY` directives for staged config trees (accepts `config_dir`, `dhcp_paths`, `has_repos`, etc.)
- `containers.py` -- quadlet file handling
- `users_groups.py` -- strategy-aware user provisioning: groups by strategy (`sysusers`, `useradd`, `blueprint`, `kickstart`), emits `COPY sysusers.d/`, `RUN useradd`, or `# FIXME` comments accordingly
- `kernel_boot.py` -- `bootc`-native `kargs.d` handling
- `network.py` -- firewall zone XML, NM connections
- `_config_tree.py` -- `write_config_tree(snapshot, output_dir)` stages files into `output_dir/config/`; `config_copy_roots(config_dir)` returns the top-level subdirs to `COPY`
- `_helpers.py` -- shared utilities (`_sanitize_shell_value`, `_dhcp_connection_paths`, `_operator_kargs`)

#### Adapter contract: three functions

The adapter lives in a new file `src/yoinkc/architect/render_adapter.py` and exposes three public functions:

```python
@dataclass
class LayerRenderInput:
    """Renderer-grade payload for a single architect layer.

    Contains only exportable artifacts -- nothing classified as
    warn_only or deferred appears here. Those are handled separately
    as comment-only output by render_layer_containerfile().

    Fields carry the exact data the domain modules need, reshaped
    from architect's Layer dataclasses into the Pydantic-model-shaped
    dicts that section_lines() expects, plus stable source references
    (file paths, unit names) for staging.
    """
    packages: list[str]                # NVRA strings for dnf install
    configs: list[ConfigInput]         # exportable configs only (not orphaned)
    services: list[ServiceInput]       # exportable services only (not masked)
    quadlets: list[QuadletInput]       # all quadlets (full-support tier)
    # Source references for file staging
    config_source_paths: dict[str, str]  # dest_path -> source content or ref
    quadlet_source_paths: dict[str, str] # dest_path -> source content or ref


def build_layer_render_input(layer: Layer) -> LayerRenderInput:
    """Reshape architect Layer state into renderer-grade payload.

    Filters to exportable artifacts only. Copies stable source
    references for file staging. Does NOT:
    - Infer missing safety metadata (e.g., guess a strategy for
      a user with no classification)
    - Pick executable output for unresolved variants (approximate=True
      artifacts downgrade to warn_only in the Containerfile)
    - Materialize warn_only or deferred artifacts into the render input
    """


def render_layer_containerfile(
    render_input: LayerRenderInput,
    parent: str | None,
    base_image: str,
    layer: Layer,  # full layer, needed for warn_only/deferred comment emission
) -> str:
    """Render a Containerfile for a single architect layer.

    For exportable artifacts in render_input, delegates to existing
    renderer domain modules (building minimal InspectionSnapshot-shaped
    dicts as adapter shims).

    For warn_only and deferred artifacts (read from layer, not from
    render_input), emits ONLY comment lines:
    - # REVIEW: ... for warn_only artifacts
    - # WARNING: ... for security-flagged warn_only artifacts
    - # DEFERRED: ... for deferred artifacts

    These comments never flow through _config_tree.py, never produce
    COPY or RUN directives, and never enter tree/.
    """


def stage_layer_files(
    render_input: LayerRenderInput,
    staging_dir: Path,
) -> None:
    """Populate a layer-specific staging directory (tree/) with file
    artifacts from the render input.

    Calls _config_tree.py helpers for config files and quadlet unit
    files. Only processes artifacts present in render_input (i.e.,
    exportable artifacts). warn_only and deferred artifacts are never
    staged.
    """
```

#### Negative boundary (what the adapter must NOT do)

The adapter reshapes architect state into existing renderer inputs. It must NOT:

1. **Infer missing safety metadata.** If a user has no `strategy` classification, the adapter does not guess one -- it excludes the user from `LayerRenderInput` and emits a `# REVIEW:` comment instead.
2. **Pick executable output for unresolved variants.** If an artifact has `approximate=True` (divergent content across fleets), it is excluded from `LayerRenderInput` and emitted as a `# REVIEW:` comment noting the variant, even if the artifact type is otherwise full-support.
3. **Materialize warn_only or deferred artifacts.** These never enter `LayerRenderInput`, never flow through `_config_tree.py` or any `COPY`/`RUN`-producing helper.

#### Why an adapter, not direct calls

The domain modules expect `InspectionSnapshot` sections (Pydantic models). Architect layers are dataclasses. The adapter translates between these shapes by building minimal snapshot-shaped dicts as shims. This keeps the domain modules pure -- they do not need to know about architect's data model.

#### File artifact provenance (where staged bytes come from)

Exportable configs and quadlets require actual file content to stage into `tree/` for `COPY` directives. The source of truth for this content is the **fleet's refined inspection snapshot tarball**, which contains the original `config/` directory tree with all captured files preserving their original path hierarchy.

**Provenance chain:**

1. **Loader stage:** `_snapshot_to_fleet_input()` reads each fleet tarball. The loader already knows the fleet name and the tarball's filesystem path. When building `ConfigInput` and `QuadletInput` objects, it populates two fields:
   - `source_fleet`: the fleet name (e.g., `"web-servers"`) — identifies which tarball to reopen
   - `source_ref`: the path within that tarball (e.g., `"config/etc/httpd/conf/httpd.conf"`) — identifies which file to extract
2. **Fleet tarball index:** The loader also builds a `fleet_tarball_paths: dict[str, Path]` mapping fleet name → filesystem path of the tarball. This is stored on `LayerTopology` so the export stage can resolve `source_fleet` back to a tarball path without re-discovering files.
3. **After decomposition/mutation:** When an artifact moves between layers, both `source_fleet` and `source_ref` travel with it. These always point back to the original fleet tarball, which is immutable. Moving httpd's config from the web-servers layer to base does not change its provenance — the bytes still come from the web-servers tarball.
4. **`build_layer_render_input()`** populates `config_source_paths` and `quadlet_source_paths` dicts by mapping each artifact's destination path to a `(source_fleet, source_ref)` tuple.
5. **`stage_layer_files()`** resolves each `source_fleet` via `fleet_tarball_paths` to get the tarball filesystem path, extracts the file at `source_ref`, and writes it to the layer's `tree/` directory via `_config_tree.py` helpers. Only artifacts present in `LayerRenderInput` are staged — warn_only, deferred, display_only, and unresolved-variant artifacts are never read or staged.

**Key invariant:** File content is never stored in memory in the topology or `Layer` dataclasses. The topology carries identity, ownership, classification, and provenance metadata. Actual bytes are read from the source tarball only at export/staging time. The `(source_fleet, source_ref)` pair is always sufficient to locate the exact file — no ambient state or re-discovery needed.

### Server API Changes

Expand the API to support artifact-type-aware operations:

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Serve the HTML UI |
| GET | `/api/topology` | Current topology state as JSON (expanded with all artifact types, badge fields) |
| POST | `/api/move` | Move an artifact. Body: `{"artifact": "...", "type": "package", "from": "...", "to": "...", "dependents": [...]}` |
| POST | `/api/copy` | Copy an artifact. Same body shape as move. |
| GET | `/api/preview/<layer>` | Containerfile preview for a layer (via render adapter) |
| GET | `/api/export` | Generate and return tarball |
| GET | `/api/health` | Health check |

**Backward compatibility:** If `type` is missing from the move/copy body, assume `"package"`. Existing tests continue to pass.

**Handler change:**
```python
def _handle_artifact_operation(self, operation, content_length: int) -> None:
    body = json.loads(self.rfile.read(content_length))
    artifact_type = body.get("type", "package")
    dependents = body.get("dependents", None)
    operation(
        body["artifact"], artifact_type,
        body["from"], body["to"],
        dependents=dependents,
    )
    self._send_json(200, self._topology.to_dict())
```

**Preview endpoint:** `/api/preview/<layer>` calls `render_layer_containerfile()` via the adapter. The response is plain text (Containerfile content), same as today. The preview now includes all artifact directives and comments.

---

## Frontend Changes

The full UX interaction design is in Fern's spec at `marks-inbox/reviews/2026-04-07-architect-v2-ux-spec.md`. This section covers the key patterns to implement; refer to her spec for the complete state inventory (section 7) and keyboard navigation model (section 6).

### Tab Structure

7 flat tabs in the drawer, one per artifact type:

```
+----------+---------+----------+----------+----------+-------------+---------+
| Packages | Configs | Services | Firewall | Quadlets | Users/Groups| Sysctls |
| (47)     | (12)    | (8)      | (4)      | (3)      | (6)         | (5)     |
+----------+---------+----------+----------+----------+-------------+---------+
```

- **Tab order:** Frequency of interaction. Packages first (primary decomposition unit), configs second ("what did this package bring?"), services third ("what runs at boot?"), then firewall, quadlets, users/groups, sysctls.
- **Count badges:** Actionable item count for the selected layer. PatternFly 6 `pf-v6-c-badge`.
- **Default tab:** Packages, preserved across layer selections.
- **Implementation:** PF6 `pf-v6-c-tabs` with `role="tablist"`, `aria-selected`. Each tab panel is a function: `renderPackagesPanel()`, `renderConfigsPanel()`, etc.

### Approximation Badges in the Drawer

Each artifact row may display badges based on the `approximate` and `export_class` fields from the topology JSON:

```
+-- /etc/httpd/conf/httpd.conf --- [Approximate] --- [Move up] ---+
|  Owned by: httpd                                                 |
+------------------------------------------------------------------+

+-- net.ipv4.ip_forward --- [Warn-only] [Security review] ---------+
|  Value: 1                                                        |
+------------------------------------------------------------------+
```

Badge rendering logic in `renderArtifactRow()`:
```javascript
function renderBadges(artifact) {
    var badges = '';
    if (artifact.approximate) {
        badges += '<span class="pf-v6-c-label pf-m-orange">Approximate</span> ';
    }
    if (artifact.export_class === 'warn_only') {
        badges += '<span class="pf-v6-c-label pf-m-gold">Warn-only export</span> ';
    } else if (artifact.export_class === 'deferred') {
        badges += '<span class="pf-v6-c-label pf-m-grey">Deferred</span> ';
    } else if (artifact.export_class === 'display_only') {
        badges += '<span class="pf-v6-c-label pf-m-blue">Display only</span> ';
    }
    // Security review badge for sysctls on denylist
    if (artifact.security_flag) {
        badges += '<span class="pf-v6-c-label pf-m-red">Security review</span> ';
    }
    return badges;
}
```

Badges are always visible, not hidden behind hover. They appear inline after the artifact name/path, before the action buttons.

### Confirmation Strip (Tied Changes -- Package Move)

When a package has dependent configs or services, clicking "Move up" or selecting a copy-to target expands the row inline:

```
+-- httpd -------- [Move up] -----------------------------------+
|                                                                |
|  Will also move:                                               |
|    [x] /etc/httpd/conf/httpd.conf    (config, RPM-owned)      |
|    [x] /etc/httpd/conf.d/ssl.conf    (config, RPM-owned)      |
|    [x] httpd.service                 (service, enable)         |
|                                                                |
|  [Confirm move]  [Cancel]                                      |
+----------------------------------------------------------------+
```

- Checkboxes default to checked. Unchecking excludes the item from the move.
- Zero-dependent packages move immediately (no strip).
- Always show the strip if any dependents exist, even if just 1.
- No modal dialog. Inline expansion keeps the user in spatial context.

### Reverse Prompt (Config/Service Move)

When moving an RPM-owned config:
```
+-- /etc/httpd/conf/httpd.conf --- [Move up] -------------------+
|                                                                |
|  This config is owned by httpd.                                |
|  Also move httpd and its other configs?  [Yes] [No, just this] |
+----------------------------------------------------------------+
```

- **"Yes":** Switches to the package's confirmation strip, showing all dependents.
- **"No, just this":** Moves only the config.
- **Unowned configs:** Move immediately, no prompt.

Same pattern for services with `owning_package`.

### Related Artifacts Indicator

On package rows:
```
+-- httpd -------- [Move up] [Copy to v] ---+
|  2 configs, 1 service                     |
+-------------------------------------------+
```

- Clickable link. Navigates to Configs tab with related items highlighted (2-second fade, `@keyframes highlight-fade`).
- "Back to Packages" breadcrumb at top of target tab panel.

On config/service rows:
```
+-- /etc/httpd/conf/httpd.conf ----------+
|  Owned by: httpd                       |
+-----------------------------------------+
```

- Clicking navigates to Packages tab with `httpd` highlighted.

### Per-Artifact-Type Move Semantics

| Artifact Type       | Move up | Copy to | Has dependents? | Confirmation strip | Reverse prompt |
|---------------------|---------|---------|------------------|--------------------|----------------|
| Package             | Yes     | Yes     | Yes (configs, services) | Yes (if dependents) | N/A |
| Config (RPM-owned)  | Yes     | Yes     | No               | No                 | Yes            |
| Config (unowned)    | Yes     | Yes     | No               | No                 | No             |
| Config (orphaned)   | Yes     | Yes     | No               | No                 | No             |
| Service (owned)     | Yes     | Yes     | No               | No                 | Yes            |
| Service (unowned)   | Yes     | Yes     | No               | No                 | No             |
| Firewall zone       | Yes     | Yes     | No               | No                 | No             |
| Quadlet             | Yes     | Yes     | No               | No                 | No             |
| User/Group          | Yes     | Yes     | No               | No                 | No             |
| Sysctl              | Yes     | Yes     | No               | No                 | No             |

All types support the base -> derived broadcast on move-up (removing from base copies to ALL derived layers).

### Edge Cases

- **Package already in target but config is not:** The confirmation strip shows the config but grays out the package row with "(already in {target})". Only the config moves.
- **Multiple packages own configs in the same move batch:** Each package gets its own confirmation strip. Sequential, not parallel -- one at a time.
- **Circular dependency detection:** Not possible in the current data model (packages own configs, not the reverse). If future types introduce bidirectional ownership, the system should detect and warn rather than loop.

### JavaScript Changes

**New state variables:**
```javascript
var activeTab = 'packages';
var confirmingMove = null;   // {artifact, type, from, to, dependents}
var pkgDependents = {};      // built from topology data per layer
```

**`renderDrawer()` refactored:**
1. Render tab bar with active tab highlighted and count badges
2. Call `renderTabPanel(activeTab)` which dispatches to type-specific renderers

**New functions:**
- `buildDependentsIndex(layer)` -- populates `pkgDependents` from layer's configs/services by `owner_package`
- `renderConfirmationStrip(artifact, from, to, dependents)` -- inline confirmation UI
- `renderReversePrompt(artifact, ownerPackage, from, to)` -- "also move owner?" prompt
- `executeMove(artifact, type, from, to, dependents)` -- API call with dependents
- `switchToTab(tabName, highlightIds)` -- tab switch with optional highlight animation
- `renderBadges(artifact)` -- approximation/export/security badge rendering

**Event delegation expansion:** The drawer's click handler gains cases for tab clicks, confirmation checkboxes, confirm/cancel buttons, reverse prompt yes/no, and related-artifacts links.

### CSS Additions

Minimal additions to `_css.html.j2`:
- Confirmation strip: expandable row, checkbox list, background highlight
- Reverse prompt: inline prompt with button pair
- Related-artifacts indicator: subtle secondary text, clickable
- Highlight fade: `@keyframes highlight-fade { from { background: rgba(43,154,243,0.2); } to { background: transparent; } }`
- Tab count badges: PF6 `pf-v6-c-badge`
- Approximation badges: PF6 `pf-v6-c-label` with color modifiers (`pf-m-orange`, `pf-m-gold`, `pf-m-grey`, `pf-m-red`, `pf-m-blue` for display-only)

---

## Inspection Pipeline Changes

### Config Ownership Enrichment

**Decision:** `rpm -qf` lookup runs on the main inspection path, not gated behind `--config-diffs`.

**Current state:** `ConfigFileEntry.package` is populated only when `--config-diffs` is used, because `RpmVaEntry.package` relies on an `rpm -qf` call that only happens in that mode.

**Change:** Move the `rpm -qf` lookup to always run for files detected by `rpm -Va`. This populates `ConfigFileEntry.package` for all RPM_OWNED_MODIFIED configs regardless of flags.

**Files affected:**
- `src/yoinkc/inspectors/config.py` or `src/yoinkc/inspectors/rpm.py` -- add `rpm -qf` for each `rpm -Va` path on the main path
- Performance note: `rpm -qf` is fast (< 1ms per file), and `rpm -Va` typically returns fewer than 50 files. Total overhead: negligible.

**Schema impact:** None. `ConfigFileEntry.package` already exists as `Optional[str]`. We are just populating it more consistently.

---

## Driftify Fixture Changes

The `tests/e2e/generate-fixtures.py` script builds `InspectionSnapshot` objects. The existing architect fixtures (`fixtures/architect-topology/`) only populate `rpm.packages_added` and `config.files`. They need realistic multi-artifact data.

### Required Fixture Data

Each architect fixture fleet needs:

1. **Configs:** Mix of RPM-owned-modified (with `.package` set) and unowned configs. Some shared across fleets (prevalence -> base), some fleet-specific.
   - Example: `/etc/httpd/conf/httpd.conf` (RPM-owned, package=httpd) in web fleet
   - Example: `/etc/custom/app.conf` (unowned) in app fleet

2. **Services:** State changes with `owning_package` set where applicable.
   - Example: `httpd.service` (action=enable, owning_package=httpd) in web fleet
   - Example: `postgresql.service` (action=enable, owning_package=postgresql) in db fleet
   - Example: `custom-agent.service` (action=enable, owning_package=None) shared across fleets

3. **Firewall zones:** Zone configs with services and ports.
   - Example: `public` zone (services=["http", "https"], ports=["8080/tcp"]) in web fleet
   - Example: `public` zone (services=["ssh"]) shared across all fleets (variant: different services)

4. **Quadlets:** Container unit files.
   - Example: `/etc/containers/systemd/monitoring.container` shared across fleets
   - Example: `/etc/containers/systemd/app.container` in app fleet only

5. **Users/groups:** Non-default users and groups with strategy classification.
   - Example: `appuser` (user, strategy=useradd, shell=/sbin/nologin) in app fleet
   - Example: `deploy` (user, strategy=sysusers, shell=/sbin/nologin) shared across fleets
   - Example: `admin` (user, strategy=kickstart, shell=/bin/bash) -- deferred export test

6. **Sysctls:** Sysctl overrides, including security-sensitive ones.
   - Example: `net.ipv4.ip_forward=1` shared across fleets (security-sensitive)
   - Example: `vm.swappiness=10` in db fleet only (benign)
   - Example: `kernel.randomize_va_space=2` in one fleet (denylist test)

### Fixture Scenarios

**Existing "three-role-overlap" fixture -- expand:**
- 3 fleets (web, db, app) with overlapping packages
- Add configs, services, firewall zones, quadlets, users, sysctls per above
- Expected: some artifacts land in base (shared across all 3), rest in derived
- Include at least one variant (same firewall zone name, different services) for approximation badge testing

**Existing "hardware-split" fixture -- expand:**
- 2 fleets with hardware-specific divergence
- Add hardware-correlated artifacts (GPU-specific sysctls, different quadlet images)

**New "tied-changes" fixture:**
- Focus on RPM-owned configs and package-service associations
- Packages with multiple owned configs for confirmation strip testing
- Packages with zero owned configs for fast-path testing
- Unowned configs for reverse prompt testing (no prompt expected)
- Service with `owning_package` for reverse-prompt-on-service testing

**New "export-safety" fixture:**
- Any user (service account or human) verifies `display_only` export class
- Human user with real shell confirms no Containerfile output
- Orphaned config (warn-only test)
- Masked service (warn-only test)
- Security-sensitive sysctl on denylist (warning flag test)
- Quadlet with `Privileged=true` (security review badge test)

---

## Phased Implementation Plan

### Phase 1: Tabs + Data Plumbing

**Goal:** Multi-artifact data flows end-to-end. Tabs visible. All 7 artifact types displayed read-only with badges. No move/copy yet.

**Maturity tiers activated:** All types visible. Badge rendering active. Export not yet connected.

**Backend:**
- Add input dataclasses (`ConfigInput`, `ServiceInput`, etc.) to `analyzer.py`
- Expand `FleetInput` and `Layer` with new fields
- Update `analyze_fleets()` with prevalence + parent-follows for all types
- Add variant detection (populate `_approximate` flag)
- Add export class computation helpers
- Expand `_snapshot_to_fleet_input()` in `loader.py`
- Expand `to_dict()` in `LayerTopology` with badge fields

**Frontend:**
- Add 7-tab bar to drawer
- Implement `renderTabPanel()` dispatch
- Implement read-only panel renderers for each type
- Count badges
- Approximation and export-class badge rendering

**Fixtures:**
- Expand existing architect fixtures with multi-artifact data
- Add "export-safety" fixture

**Server:**
- Expand `/api/topology` response to include all artifact types with badge fields

**Demo narrative:** "Architect now shows configs, services, firewall rules, quadlets, users, and sysctls -- not just packages. Badges tell you which ones need review."

### Phase 2: Move/Copy + Export (independent artifact types)

**Goal:** Artifact types that do NOT have tied-change semantics can be moved, copied, and exported. This means quadlets, firewall zones, sysctls, and unowned/orphaned configs. Packages, RPM-owned configs, and owned services are NOT movable yet -- they require the tied-change model from Phase 3 to avoid incoherent layer states (e.g., a config landing in a layer without its owning package).

**Why this boundary:** Moving a package without its owned configs, or moving an RPM-owned config without prompting for its package, produces architecturally wrong results. The safety model for these types IS the tied-change model. Exposing move/copy for them before Phase 3 would create an interim milestone where users can produce broken decompositions.

**Backend:**
- Implement `move_artifact()` and `copy_artifact()` on `LayerTopology` for independent types
- Update server API to accept `type` parameter
- Backward-compatible: `type` defaults to `"package"` (but package moves return 400 until Phase 3)
- Server rejects move/copy for `package`, `config` (RPM-owned), and `service` (with `owning_package`) with a clear error message: "Package/owned-artifact moves require tied-change support (Phase 3)"

**Export:**
- Create `render_adapter.py` with adapter layer to existing renderer modules
- Implement `build_layer_render_input()`, `render_layer_containerfile()`, `stage_layer_files()`
- Full-support types: delegate to domain modules for executable directives
- Visible-only types: emit `# REVIEW:` / `# WARNING:` comments
- Display-only types (users/groups): no Containerfile output
- Wire `/api/preview/<layer>` to use render adapter
- Expand `export_topology()` to populate `tree/` with file artifacts (exportable only)

**Frontend:**
- Add move-up and copy-to buttons to tabs for independent types (quadlets, firewall, sysctls, unowned configs)
- Package, RPM-owned config, and owned service tabs show move/copy buttons as disabled with tooltip: "Available in Phase 3 with tied-change support"
- Reuse existing button/dropdown patterns from packages

**Fixtures:**
- Add "tied-changes" fixture (data needed for Phase 3 testing, but fixture available now)

**Demo narrative:** "You can rearrange independent artifacts -- move a firewall zone to base, copy a quadlet to another fleet. Export produces real Containerfiles with safety comments. Package and owned-artifact moves come next with tied-change support."

### Phase 3: Packages, Owned Artifacts + Tied Changes

**Goal:** Packages and their owned configs/services become movable, with the tied-change model ensuring semantic coherence. The confirmation strip and reverse prompt work. This is the phase that unlocks the full move/copy surface for all artifact types.

**Backend:**
- Enable `move_artifact()` and `copy_artifact()` for packages, RPM-owned configs, and owned services
- `move_artifact()` and `copy_artifact()` accept `dependents` parameter
- Process dependents in the same operation
- Remove the Phase 2 server rejection for package/owned-artifact moves

**Frontend:**
- Enable move/copy buttons on package, RPM-owned config, and owned service tabs
- Build `pkgDependents` index from topology data
- Implement confirmation strip (package move shows owned configs/services with checkboxes)
- Implement reverse prompt (RPM-owned config move prompts "also move owning package?")
- Event handlers for checkboxes, confirm/cancel, yes/no

**Demo narrative:** "Move httpd and its configs follow. The tool understands which things belong together."

### Phase 4: Related Indicators + Polish

**Goal:** Cross-tab visibility. Polish. Keyboard nav.

**Frontend:**
- Related-artifacts indicator on package rows
- "Owned by" indicator on config/service rows
- Click-to-navigate with highlight fade animation
- "Back to [tab]" breadcrumb
- Keyboard navigation (per Fern's spec section 6)

**Demo narrative:** Complete experience. "Click '2 configs, 1 service' to see exactly what this package owns."

---

## Files to Modify

### Python (Backend)

| File | Changes |
|------|--------|
| `src/yoinkc/architect/analyzer.py` | Add input dataclasses (with `source_fleet`/`source_ref` on `ConfigInput`/`QuadletInput`), expand `FleetInput`, `Layer`, `LayerTopology` (add `fleet_tarball_paths`), `to_dict()`, `analyze_fleets()`. Add `move_artifact()`, `copy_artifact()`. Add variant detection. Add export class helpers. |
| `src/yoinkc/architect/loader.py` | Expand `_snapshot_to_fleet_input()` to extract all 6 artifact types from snapshot. Populate `source_fleet` and `source_ref` for configs/quadlets. Build `fleet_tarball_paths` mapping. Populate `strategy` and `shell` for users. |
| `src/yoinkc/architect/render_adapter.py` | **New file.** Adapter layer between architect `Layer` data and existing `renderers/containerfile/` domain modules. |
| `src/yoinkc/architect/export.py` | Replace inline rendering with calls to `render_adapter.render_layer_containerfile()`. Expand `export_topology()` for file artifacts in `tree/`. |
| `src/yoinkc/architect/server.py` | Generalize `_handle_pkg_operation()` to `_handle_artifact_operation()`. Accept `type` and `dependents` in request body. Wire `/api/preview` to render adapter. |
| `src/yoinkc/inspectors/config.py` | Run `rpm -qf` on the main inspection path to populate `ConfigFileEntry.package`. |

### Templates (Frontend)

| File | Changes |
|------|--------|
| `src/yoinkc/templates/architect/architect.html.j2` | Add tab bar markup to drawer. |
| `src/yoinkc/templates/architect/_js.html.j2` | Tab switching, per-type panel renderers, badge rendering, confirmation strip, reverse prompt, related indicators, highlight animation. |
| `src/yoinkc/templates/architect/_css.html.j2` | Confirmation strip, reverse prompt, highlight fade, tab badge styles, approximation badge styles. |

### Tests / Fixtures

| File | Changes |
|------|--------|
| `tests/e2e/generate-fixtures.py` | Add multi-artifact data to architect fixtures. Add "tied-changes" and "export-safety" fixtures. |
| `tests/` (new test files) | Unit tests for multi-artifact decomposition, parent-follows rule, move/copy with dependents, export safety classification, render adapter, variant detection. Server tests for new API shape. |

---

## Testing

### Unit Tests -- Decomposition

- **Prevalence decomposition per type:** Given fleets with overlapping configs/services/sysctls/firewall zones/quadlets/users, verify correct base vs. derived placement for each type.
- **Parent-follows:** Given packages in base with RPM-owned configs, verify configs land in the same layer as their package.
- **Parent-follows fallback:** When `ConfigInput.package` is `None` for an `rpm_owned_modified` config, verify fallback to prevalence.
- **Single fleet:** No base extraction. All artifacts in one layer. No crashes.
- **Variant detection:** Same config path in 2 fleets with different `kind` or ownership. Verify `approximate=True` flag set on the base-layer artifact.
- **Service identity:** Same unit enabled in fleet A, disabled in fleet B. Verify they are treated as different artifacts (`httpd.service:enable` vs `httpd.service:disable`).

### Unit Tests -- Move/Copy

- **Move artifact per type:** Verify move for each of the 7 artifact types. Verify base -> derived broadcast.
- **Move with dependents:** Move a package with 3 dependent configs. Verify all move. Move with 1 unchecked dependent. Verify excluded item stays.
- **Copy with dependents:** Copy a package to a sibling. Verify dependents also copied.
- **Edge case -- package already in target:** Config's owning package is already in the target layer. Only config moves.
- **Invariant: no orphaned owned artifacts after move.** After any move/copy, verify that every config with `owner_package=X` is in the same layer as package X (unless the user explicitly unchecked it).
- **Invariant: no duplicate artifacts.** After any move/copy, verify no layer contains the same artifact identity key twice.

### Unit Tests -- Export Safety

- **Export classification per type:** Verify each artifact type gets the correct `export_class` based on the Export Safety Classification table.
- **Sysctl denylist:** Verify `kernel.randomize_va_space` gets `# WARNING` prefix, not just `# REVIEW`.
- **User classification:** Verify all users/groups get `display_only` regardless of strategy or shell.
- **Orphaned config:** Verify `warn_only` export class.
- **Masked service:** Verify `warn_only` export class.

### Unit Tests -- Render Adapter

- **Adapter produces valid Containerfile:** For a layer with all artifact types, verify the adapter output is valid Containerfile syntax.
- **Full-support directives present:** Packages produce `RUN dnf install`, configs produce `COPY tree/`, services produce `RUN systemctl`, quadlets produce `COPY tree/`.
- **Visible-only types produce comments:** Firewall zones produce `# REVIEW:`, sysctls produce `# REVIEW:` or `# WARNING:`.
- **Display-only types produce nothing:** Users/groups do not appear in Containerfile output at all -- no directives, no comments.
- **Adapter parity with renderer:** For a known fixture, verify the adapter produces the same `RUN systemctl enable` line as calling `services.section_lines()` directly.

### Integration Tests

- Load fixture tarballs -> analyze -> verify topology structure -> export -> verify Containerfile contains all directive types and comments.
- Verify `tree/` directory contains expected config files, quadlet files.
- Verify `build.sh` ordering.

### Server Tests

- POST `/api/move` with `type: "config"` and no `dependents` -- single config move.
- POST `/api/move` with `type: "package"` and `dependents` list -- tied move.
- POST `/api/move` without `type` field -- backward compat, assumes package.
- GET `/api/topology` -- verify response JSON includes all artifact types with badge fields.
- GET `/api/preview/<layer>` -- verify response includes directives and comments.
- **API/preview/export parity:** After a move, verify that `/api/topology`, `/api/preview`, and `/api/export` all reflect the same state.

### E2E Tests

- Expand Playwright tests to verify tab rendering, count badges, badge rendering, and move operations across tabs.
- Verify confirmation strip appears for package with dependents.
- Verify reverse prompt appears for RPM-owned config move.

---

## Out of Scope / Deferred

- **Additional artifact types beyond 6:** SELinux policies, certificates/TLS, storage mounts, cron jobs, NM connections, non-RPM software, kernel modules, systemd drop-ins. These are captured by the inspection pipeline but not decomposed in architect v2. The `FleetInput` structure is extensible -- adding a new type is mechanical once the pattern is established.
- **Configurable prevalence threshold:** 100% cross-fleet prevalence only. Majority/percentage thresholds are future work.
- **Config diff viewer in architect:** Viewing the actual diff of a modified config within the architect UI. The data exists (`ConfigFileEntry.diff_against_rpm`) but displaying it is a UX project on its own.
- **Fleet-specific service overrides:** A service enabled in one fleet but disabled in another within the same layer. Known limitation -- the current model stores one `action` per service per layer. The identity key (`unit:action`) treats enable and disable as separate artifacts for prevalence, which avoids the worst case (silently baking an enable into base when only one fleet enables it). But same-unit-different-action-in-same-layer remains unsupported.
- **Drag-and-drop for non-package artifacts:** Currently only packages support drag-and-drop repositioning in the tree. Extending to other types is Phase 5 at earliest.
- **"Show all configs" toggle:** Removed from scope. Config filtering happens at the refine/fleet level upstream. Architect shows whatever the refined fleet output contains.
- **Full kernel boot args export:** Kernel args via `bootc`-native `/usr/lib/bootc/kargs.d/` deferred to v3. v2 only flags kargs in export comments.
- **Promotion of visible-only / display-only to full-support:** Firewall and sysctls (visible-only) can be promoted to full-support once export semantics are validated. Users/groups (display-only) require reliable service-account classification from the inspection pipeline before promotion to visible-only or full-support. The infrastructure (adapter, badges, testing) supports these promotions as configuration changes, not structural ones.
- **Human user provisioning in images:** SSH keys, sudoers rules, password/shadow data are never baked into images. Human users are deploy-time provisioning only (kickstart, cloud-init). This is a security boundary, not a v2 limitation.
