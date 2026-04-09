# Architect v2: Multi-Artifact Decomposition -- Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand `yoinkc architect` from package-only decomposition to full multi-artifact decomposition covering packages, configs, services, firewall zones, quadlets, users/groups, and sysctls. 7-tab drawer UI with move/copy, tied-change semantics, and export via renderer adapter.

**Spec:** `docs/specs/proposed/2026-04-07-architect-v2-multi-artifact-design.md`

**UX Spec:** Review file at `marks-inbox/reviews/2026-04-07-architect-v2-ux-spec.md` (5-tab recommendation superseded by spec's 7-tab design)

**Project conventions:** `AGENTS.md` at workspace root. Conventional commits, `Assisted-by:` attribution, TDD, no AI slop. Two separate git repos: `yoinkc/` and `driftify/`.

---

## File Map

### Driftify (separate repo: `driftify/`)

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `driftify/driftify.py` | Expand architect fixture profiles with multi-artifact data |
| Modify | `driftify/tests/test_multi_fleet.py` | Tests for expanded fixture data |

### Yoinkc (repo: `yoinkc/`)

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src/yoinkc/inspectors/config.py` | Add `rpm -qf` enrichment on main path |
| Modify | `src/yoinkc/architect/analyzer.py` | Input dataclasses, Layer expansion, decomposition rules, move/copy, variant detection, export class |
| Modify | `src/yoinkc/architect/loader.py` | Extract all 6 artifact types from snapshots, build `fleet_tarball_paths` |
| Modify | `src/yoinkc/architect/server.py` | Generalize handlers, expand API, serve new tabs |
| Modify | `src/yoinkc/architect/export.py` | Delegate to render adapter instead of inline rendering |
| Create | `src/yoinkc/architect/render_adapter.py` | Adapter layer between architect and `renderers/containerfile/` |
| Modify | `src/yoinkc/templates/architect/architect.html.j2` | 7-tab drawer structure, confirmation strip, reverse prompt |
| Modify | `src/yoinkc/templates/architect/_js.html.j2` | Tab switching, panel renderers, tied-change UI, related indicators |
| Modify | `src/yoinkc/templates/architect/_css.html.j2` | Tab styles, badge styles, confirmation strip, highlight-fade |
| Create | `tests/architect/test_multi_artifact.py` | Decomposition, variant detection, export class tests |
| Create | `tests/architect/test_move_copy.py` | Move/copy for all types, tied changes, dependents |
| Create | `tests/architect/test_render_adapter.py` | Adapter contract tests |
| Modify | `tests/architect/test_analyzer.py` | Update existing tests for expanded dataclasses |
| Modify | `tests/e2e/generate-fixtures.py` | Multi-artifact fixture generation |

---

## Current State (what exists today)

Before reading the tasks, understand the baseline:

- **`analyzer.py`**: `FleetInput(name, packages: list[str], configs: list[str], host_count, base_image)`. `Layer(name, parent, packages: list[str], configs: list[str], fleets, fan_out, turbulence)`. `LayerTopology` with `move_package()`, `copy_package()`, `to_dict()`, `analyze_fleets()`. Configs are `list[str]` (path strings only). No services, firewall, quadlets, users, or sysctls.
- **`loader.py`**: `_snapshot_to_fleet_input()` reads `rpm.packages_added` for packages and `config.files[].path` for configs. Returns `FleetInput`.
- **`export.py`**: `export_topology()` generates tarball with inline `render_containerfile()` that produces `FROM` + `RUN dnf install` only. No config/service/quadlet export.
- **`server.py`**: Routes: `GET /`, `GET /api/topology`, `POST /api/move`, `POST /api/copy`, `GET /api/export`, `GET /api/health`, `GET /api/preview/<layer>`. Handlers: `_handle_pkg_operation()` calls `topology.move_package(pkg, from, to)` or `copy_package()`.
- **`schema.py`**: `ConfigFileEntry(path, kind: ConfigFileKind, category, content, rpm_va_flags, package: Optional[str], ...)`. `ServiceEntry` not a standalone schema class -- services are in `ServiceSection.state_changes` as `StateChange(unit, action, ...)`. `SysctlOverride(key, runtime, default, source)`. `FirewallZone(path, name, content, services, ports, rich_rules)`. `ContainerSection` has `quadlet_units`. User/group data in `UserGroupSection`.
- **`config.py` inspector**: `ConfigFileEntry.package` populated from `RpmVaEntry.package` only when `--config-diffs` is used. The `_rpm_owned_paths()` bulk query runs always, but per-file `rpm -qf` only runs in the diff path.

---

## Phase 0: Prerequisites (Inspection Pipeline + Fixtures)

### Task 0.1: Config ownership enrichment (`rpm -qf` on main path)

The `ConfigFileEntry.package` field is only populated when `--config-diffs` is enabled. Architect v2 needs this field always populated for RPM-owned-modified configs so the parent-follows decomposition rule works.

**Files:**
- Modify: `src/yoinkc/inspectors/config.py`

- [ ] **Step 1: Move `rpm -qf` lookup to the main code path**

  In `collect_config_files()` (or the equivalent main inspection function), after building the list of RPM-owned-modified files from `rpm -Va` results, add an `rpm -qf` call for each file to populate `ConfigFileEntry.package`. Currently, `RpmVaEntry.package` is only populated in the `--config-diffs` branch via `_get_rpm_original()`. The change:

  ```python
  # After building rpm_va entries, before creating ConfigFileEntry objects:
  # For each rpm_va entry that has no .package set, run rpm -qf
  for entry in section_rpm_va_entries:
      if entry.package is None and executor is not None:
          result = _run_rpm_query(executor, host_root, ["-qf", entry.path])
          if result.returncode == 0 and result.stdout.strip():
              entry.package = result.stdout.strip().split("\n")[0]
  ```

  The `_run_rpm_query()` helper already exists in `config.py`. Performance: `rpm -qf` is < 1ms per file, `rpm -Va` typically returns < 50 files. Negligible overhead.

- [ ] **Step 2: Write test for ownership enrichment**

  Add a test in `tests/inspectors/test_config.py` (or create it if it does not exist) that verifies `ConfigFileEntry.package` is populated for RPM-owned-modified files even without `--config-diffs`. Use a mock executor that returns known `rpm -qf` output.

**Test requirements:**
- Assert `ConfigFileEntry.package` is non-None for all RPM_OWNED_MODIFIED entries when executor is available
- Assert graceful handling when `rpm -qf` returns non-zero (package field stays None)

**Commit:** `feat(inspect): populate ConfigFileEntry.package via rpm -qf on main path`

---

### Task 0.2: Expand driftify architect fixtures with multi-artifact data

The existing architect fixtures (e.g., "three-role-overlap") only have packages and basic configs. Expand them to include all artifact types so Phase 1 has test data.

**Files:**
- Modify: `driftify/driftify.py` (separate repo at `/Users/mrussell/Work/bootc-migration/driftify/`)
- Modify: fixture generation code for architect profiles

- [ ] **Step 1: Add multi-artifact data to the "three-role-overlap" fixture profile**

  Each fleet's generated `inspection-snapshot.json` needs:

  **Configs** (in `config.files[]`):
  - Web fleet: `{path: "/etc/httpd/conf/httpd.conf", kind: "rpm_owned_modified", package: "httpd", category: "other"}`, `{path: "/etc/custom/web-app.conf", kind: "unowned", category: "other"}`
  - DB fleet: `{path: "/etc/postgresql/postgresql.conf", kind: "rpm_owned_modified", package: "postgresql-server", category: "other"}`
  - App fleet: `{path: "/etc/custom/app.conf", kind: "unowned", category: "other"}`
  - Shared across all 3: `{path: "/etc/security/limits.conf", kind: "rpm_owned_modified", package: "pam", category: "limits"}`, `{path: "/etc/sysconfig/network-scripts/ifcfg-eth0", kind: "unowned", category: "other"}`

  **Services** (in `services.state_changes[]`):
  - Web fleet: `{unit: "httpd.service", action: "enable", owning_package: "httpd"}`
  - DB fleet: `{unit: "postgresql.service", action: "enable", owning_package: "postgresql-server"}`
  - Shared: `{unit: "chronyd.service", action: "enable", owning_package: "chrony"}`, `{unit: "custom-agent.service", action: "enable"}` (no owning_package)

  **Firewall zones** (in `network.firewall_zones[]`):
  - Web fleet: `{name: "public", path: "/etc/firewalld/zones/public.xml", services: ["http", "https"], ports: ["8080/tcp"]}`
  - Shared: `{name: "public", path: "/etc/firewalld/zones/public.xml", services: ["ssh"]}` (variant: different services list)

  **Quadlets** (in `containers.quadlet_units[]`):
  - Shared: `{path: "/etc/containers/systemd/monitoring.container", name: "monitoring", image: "quay.io/prometheus/node-exporter:latest"}`
  - App fleet only: `{path: "/etc/containers/systemd/app.container", name: "app", image: "registry.example.com/myapp:v2"}`

  **Users/groups** (in `users_groups` section):
  - Shared: `{name: "deploy", kind: "user", uid: "5000", strategy: "sysusers", shell: "/sbin/nologin"}`
  - App fleet: `{name: "appuser", kind: "user", uid: "5001", strategy: "useradd", shell: "/sbin/nologin"}`
  - Web fleet: `{name: "admin", kind: "user", uid: "1001", strategy: "kickstart", shell: "/bin/bash"}` (human user -- deferred export)

  **Sysctls** (in `kernel_boot.sysctl_overrides[]`):
  - Shared: `{key: "net.ipv4.ip_forward", runtime: "1", source: "/etc/sysctl.d/99-forward.conf"}`
  - DB fleet: `{key: "vm.swappiness", runtime: "10", source: "/etc/sysctl.d/99-db.conf"}`
  - Shared: `{key: "kernel.randomize_va_space", runtime: "2", source: "/etc/sysctl.d/99-security.conf"}` (denylist test)

- [ ] **Step 2: Add a "tied-changes" fixture scenario**

  New fixture focusing on RPM-owned configs and package-service associations:
  - A package with multiple owned configs (for confirmation strip testing)
  - A package with zero owned configs (for fast-path testing)
  - An orphaned config (from a removed package)
  - An unowned config (for reverse prompt testing -- no prompt expected)

- [ ] **Step 3: Add an "export-safety" fixture scenario**

  New fixture with artifacts specifically designed to test export classification:
  - `kernel.randomize_va_space` sysctl (denylist -- `# WARNING`)
  - `kernel.kptr_restrict` sysctl (denylist -- `# WARNING`)
  - Orphaned config (warn_only)
  - Masked service (warn_only)
  - Human user with `/bin/bash` shell (display_only)
  - Service account with `/sbin/nologin` (display_only -- users/groups are all display_only in v2)
  - A config present in all fleets but with different content (variant/approximate -- downgrade to warn_only)

- [ ] **Step 4: Update existing driftify tests**

  Verify the fixture generation includes the new artifact data. Check that snapshot JSON round-trips correctly.

**Commit:** `feat(driftify): add multi-artifact data to architect fixture profiles`

---

### Quality Gate: Phase 0

**Invariants:**
- `ConfigFileEntry.package` populated for all RPM_OWNED_MODIFIED entries (when executor available)
- Fixture snapshots contain all 7 artifact types with valid structure matching `schema.py`
- Existing tests still pass (no regressions in config inspector)

**Edge cases:**
- `rpm -qf` returns non-zero (package not found -- field stays None)
- Fixture with zero configs/services/etc. (empty lists, not missing keys)
- Single-fleet fixture (no cross-fleet prevalence to compute)

**Regression risks:**
- Config inspector performance (negligible -- < 50 `rpm -qf` calls)
- Existing fixture consumers (e.g., refine, fleet) should ignore unknown keys

---

## Phase 1: Tabs + Data Plumbing

**Goal:** Multi-artifact data flows end-to-end. 7 tabs visible. All artifacts displayed read-only. No move/copy yet.

**Demo narrative:** "Architect now shows configs, services, firewall rules, quadlets, users, and sysctls -- not just packages."

### Task 1.1: Add input dataclasses to `analyzer.py`

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`

- [ ] **Step 1: Add 6 new input dataclasses**

  Add these dataclasses ABOVE `FleetInput` (they are used by it):

  ```python
  @dataclass
  class ConfigInput:
      """Config file for architect decomposition."""
      path: str
      kind: str  # "rpm_owned_modified", "unowned", "orphaned"
      package: str | None = None  # owning RPM, if known
      category: str = "other"
      source_fleet: str = ""  # fleet that sourced this file (for export provenance)
      source_ref: str = ""    # path within fleet tarball (for export file staging)
      _approximate: bool = False  # True if content differs across fleets

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
      _approximate: bool = False

  @dataclass
  class QuadletInput:
      """Quadlet unit file for architect decomposition."""
      path: str
      name: str
      image: str = ""
      source_fleet: str = ""
      source_ref: str = ""

  @dataclass
  class UserGroupInput:
      """User or group entry for architect decomposition."""
      name: str
      kind: str  # "user" or "group"
      uid_or_gid: str = ""
      strategy: str = ""  # "sysusers", "useradd", "kickstart", "blueprint", ""
      shell: str = ""

  @dataclass
  class SysctlInput:
      """Sysctl override for architect decomposition."""
      key: str
      value: str  # runtime value
      source: str = ""  # source file path
  ```

  These exactly match the spec's FleetInput Expansion section.

**Commit:** `feat(architect): add multi-artifact input dataclasses`

---

### Task 1.2: Expand `FleetInput` and `Layer` dataclasses

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`

- [ ] **Step 1: Expand `FleetInput`**

  Change from:
  ```python
  @dataclass
  class FleetInput:
      name: str
      packages: list[str]
      configs: list[str]          # currently list[str] (paths only)
      host_count: int = 0
      base_image: str = ""
  ```

  To:
  ```python
  @dataclass
  class FleetInput:
      name: str
      packages: list[str]
      configs: list[ConfigInput] = field(default_factory=list)    # was list[str]
      services: list[ServiceInput] = field(default_factory=list)
      firewall_zones: list[FirewallInput] = field(default_factory=list)
      quadlets: list[QuadletInput] = field(default_factory=list)
      users_groups: list[UserGroupInput] = field(default_factory=list)
      sysctls: list[SysctlInput] = field(default_factory=list)
      host_count: int = 0
      base_image: str = ""
  ```

  **Breaking change:** `configs` type changes from `list[str]` to `list[ConfigInput]`. All consumers of `FleetInput.configs` must be updated.

- [ ] **Step 2: Expand `Layer`**

  Change from:
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

  To:
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

  **Note:** `_recalc_turbulence()` stays package-based. Turbulence measures rebuild churn -- packages drive rebuilds, other artifact types are COPY directives.

- [ ] **Step 3: Update existing tests**

  Any test that constructs `FleetInput` with `configs=["path1", "path2"]` must change to `configs=[ConfigInput(path="path1", kind="unowned"), ...]`. Grep for `FleetInput(` in the test suite and update all call sites.

**Test requirements:**
- Existing analyzer tests pass with the new types
- New `Layer` fields default to empty lists (no breakage for code that only uses packages)

**Commit:** `feat(architect): expand FleetInput and Layer for multi-artifact types`

---

### Task 1.3: Update `analyze_fleets()` with multi-artifact decomposition

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`

- [ ] **Step 1: Add parent-follows decomposition for owned artifacts**

  After the existing package decomposition (which produces `base_layer.packages` and per-fleet `derived_layer.packages`), build a `pkg_layer` index mapping each package name to its assigned layer name. Then iterate owned artifacts:

  ```python
  # Build package -> layer index
  pkg_layer: dict[str, str] = {}
  for layer in [base_layer] + derived_layers:
      for pkg in layer.packages:
          # Extract package name from NVRA for lookup
          pkg_name = _nvra_to_name(pkg)
          pkg_layer[pkg_name] = layer.name

  # Assign RPM-owned configs: parent-follows rule
  for fleet in fleets:
      for config in fleet.configs:
          if config.kind == "rpm_owned_modified" and config.package:
              target = pkg_layer.get(config.package)
              if target:
                  get_layer(target).configs.append(config)
              else:
                  # Package not found in any layer -- fall back to fleet's derived layer
                  get_layer(fleet.name).configs.append(config)
          else:
              # Unowned/orphaned configs use prevalence rule (step 2)
              pass

  # Assign owned services: parent-follows rule
  for fleet in fleets:
      for service in fleet.services:
          if service.owning_package:
              target = pkg_layer.get(service.owning_package)
              if target:
                  get_layer(target).services.append(service)
              else:
                  get_layer(fleet.name).services.append(service)
          else:
              # Services without owner use prevalence rule (step 2)
              pass
  ```

  **Note:** `_nvra_to_name()` already exists in `export.py`. Move it to `analyzer.py` or a shared utils module so both can use it.

- [ ] **Step 2: Add prevalence-based decomposition for independent artifacts**

  For unowned/orphaned configs, quadlets, firewall zones, users/groups, sysctls, and services without `owning_package`: apply the same 100% cross-fleet prevalence rule used for packages.

  ```python
  # Collect identity keys across fleets
  def _artifact_identity(artifact, artifact_type):
      """Return the identity key for prevalence comparison."""
      if artifact_type == "config": return artifact.path
      if artifact_type == "service": return f"{artifact.unit}:{artifact.action}"
      if artifact_type == "firewall_zone": return artifact.name
      if artifact_type == "quadlet": return artifact.path
      if artifact_type == "user_group": return f"{artifact.kind}:{artifact.name}"
      if artifact_type == "sysctl": return artifact.key

  # For each independent artifact type, check if present in ALL fleets
  # If yes -> base layer. If no -> fleet's derived layer.
  for artifact_type in ["config", "service", "firewall_zone", "quadlet", "user_group", "sysctl"]:
      all_keys = defaultdict(set)  # identity -> set of fleet names
      fleet_artifacts = defaultdict(list)  # (fleet_name, identity) -> artifact

      for fleet in fleets:
          artifacts = _get_fleet_artifacts(fleet, artifact_type)  # helper
          for a in artifacts:
              key = _artifact_identity(a, artifact_type)
              all_keys[key].add(fleet.name)
              fleet_artifacts[(fleet.name, key)] = a

      fleet_names = {f.name for f in fleets}
      for key, present_in in all_keys.items():
          if present_in == fleet_names:
              # Present in ALL fleets -> base
              representative = fleet_artifacts[(next(iter(present_in)), key)]
              _get_layer_list(base_layer, artifact_type).append(representative)
          else:
              # Fleet-specific -> each fleet's derived layer
              for fname in present_in:
                  artifact = fleet_artifacts[(fname, key)]
                  _get_layer_list(get_layer(fname), artifact_type).append(artifact)
  ```

  Filter: only apply prevalence to configs where `kind != "rpm_owned_modified"` or `package is None` (the parent-follows configs were already assigned in step 1).

- [ ] **Step 3: Add variant detection**

  When the same identity key appears in multiple fleets, compare content to detect variants. If content differs, set `_approximate = True` on the artifact.

  ```python
  # During prevalence analysis, when collecting artifacts across fleets:
  # For configs: compare content field (if available) or diff_against_rpm
  # For firewall zones: compare services/ports lists
  # For sysctls: compare value field
  # If content differs -> set _approximate = True on the representative
  ```

  Variant detection applies to: configs (content), firewall zones (services/ports), sysctls (value). Packages, services, quadlets, and users/groups use identity-only equivalence (no variant concept).

- [ ] **Step 4: Add export class computation helpers**

  ```python
  # Export safety classification per the spec:
  _SYSCTL_DENYLIST = {
      "kernel.randomize_va_space",
      "kernel.kptr_restrict",
      "kernel.dmesg_restrict",
      "kernel.yama.ptrace_scope",
      "net.ipv4.conf.all.accept_redirects",
      "net.ipv4.conf.default.accept_redirects",
      "net.ipv6.conf.all.accept_redirects",
      "net.ipv6.conf.default.accept_redirects",
  }

  def _export_class(artifact, artifact_type: str) -> str:
      """Classify an artifact for export safety."""
      if artifact_type == "package":
          return "full"
      if artifact_type == "config":
          if artifact.kind == "orphaned":
              return "warn_only"
          if getattr(artifact, '_approximate', False):
              return "warn_only"
          return "full"  # rpm_owned_modified and unowned
      if artifact_type == "service":
          if artifact.action == "mask":
              return "warn_only"
          return "full"
      if artifact_type == "quadlet":
          return "full"
      if artifact_type == "firewall_zone":
          return "visible_only"
      if artifact_type == "sysctl":
          return "visible_only"
      if artifact_type == "user_group":
          return "display_only"
      return "warn_only"

  def _sysctl_is_denylisted(key: str) -> bool:
      return key in _SYSCTL_DENYLIST
  ```

- [ ] **Step 5: Write decomposition tests**

  Create `tests/architect/test_multi_artifact.py`:

  - Test parent-follows: config with `package="httpd"` lands in same layer as httpd NVRA
  - Test prevalence: unowned config in all fleets -> base; in one fleet -> derived
  - Test fallback: RPM-owned config with unknown package -> fleet's derived layer
  - Test variant detection: same config path in 2 fleets with different content -> `_approximate=True`
  - Test export class: each artifact type returns correct classification
  - Test sysctl denylist: `kernel.randomize_va_space` flagged
  - Test single-fleet: all artifacts go to that fleet's layer (no base layer prevalence)
  - Test empty artifact types: fleet with packages but no services -> empty `layer.services`

**Commit:** `feat(architect): multi-artifact decomposition with prevalence and parent-follows rules`

---

### Task 1.4: Expand `_snapshot_to_fleet_input()` in `loader.py`

**Files:**
- Modify: `src/yoinkc/architect/loader.py`

- [ ] **Step 1: Extract all 6 artifact types from snapshot JSON**

  Expand `_snapshot_to_fleet_input()` from extracting only packages and config paths to extracting all types. Reference `schema.py` for the snapshot structure:

  ```python
  def _snapshot_to_fleet_input(snapshot: dict, fleet_name: str) -> FleetInput:
      meta = snapshot.get("meta", {})
      hostname = meta.get("hostname", "unknown")
      fleet_meta = meta.get("fleet", {})
      host_count = fleet_meta.get("total_hosts", 1)  # FleetMeta.total_hosts in schema.py
      rpm = snapshot.get("rpm", {})
      base_image = rpm.get("base_image", "")  # RpmSection.base_image in schema.py

      # Packages (unchanged)
      rpm = snapshot.get("rpm", {})
      packages = [p["nvra"] for p in rpm.get("packages_added", []) if p.get("include", True)]

      # Configs
      config = snapshot.get("config", {})
      configs = [
          ConfigInput(
              path=f["path"],
              kind=f.get("kind", "unowned"),
              package=f.get("package"),
              category=f.get("category", "other"),
              source_fleet=fleet_name,
              source_ref=f"config{f['path']}",
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
              path=q.get("path", ""),
              name=q.get("name", ""),
              image=q.get("image", ""),
              source_fleet=fleet_name,
              source_ref=f"quadlet/{q.get('name', '')}",
          )
          for q in containers.get("quadlet_units", [])
          if q.get("include", True)
      ]

      # Users/groups
      ug = snapshot.get("users_groups", {})
      users_groups = []
      for u in ug.get("users", []):
          if u.get("include", True):
              users_groups.append(UserGroupInput(
                  name=u.get("name", ""),
                  kind="user",
                  uid_or_gid=str(u.get("uid", "")),
                  strategy=u.get("strategy", ""),
                  shell=u.get("shell", ""),
              ))
      for g in ug.get("groups", []):
          if g.get("include", True):
              users_groups.append(UserGroupInput(
                  name=g.get("name", ""),
                  kind="group",
                  uid_or_gid=str(g.get("gid", "")),
              ))

      # Sysctls (in KernelBootSection, not NetworkSection)
      kernel_boot = snapshot.get("kernel_boot", {})
      sysctls = [
          SysctlInput(
              key=s.get("key", ""),
              value=s.get("runtime", ""),
              source=s.get("source", ""),
          )
          for s in kernel_boot.get("sysctl_overrides", [])
          if s.get("include", True)
      ]

      return FleetInput(
          name=fleet_name,
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

  **Schema field cross-reference:**
  - `config.files[]` -> `ConfigFileEntry` in `schema.py` (fields: `path`, `kind`, `package`, `category`)
  - `services.state_changes[]` -> fields: `unit`, `action`, `owning_package`
  - `network.firewall_zones[]` -> `FirewallZone` in `schema.py` (fields: `name`, `path`, `services`, `ports`)
  - `containers.quadlet_units[]` -> fields: `path`, `name`, `image`
  - `users_groups.users[]` -> fields: `name`, `uid`, `strategy`, `shell`
  - `users_groups.groups[]` -> fields: `name`, `gid`
  - `kernel_boot.sysctl_overrides[]` -> `SysctlOverride` in `schema.py` (fields: `key`, `runtime`, `source`)

  **Note on sysctls location:** In `schema.py`, `SysctlOverride` lives in `KernelBootSection.sysctl_overrides`. The loader reads from `kernel_boot.sysctl_overrides` in the snapshot JSON.

- [ ] **Step 2: Build `fleet_tarball_paths` mapping**

  In `load_from_snapshots()`, after loading all fleet inputs, build a mapping of fleet name to tarball filesystem path:

  ```python
  def load_from_snapshots(input_dir: Path) -> tuple[list[FleetInput], dict[str, Path]]:
      """Load fleet inputs and build tarball path index."""
      fleets = []
      tarball_paths: dict[str, Path] = {}

      for path in sorted(input_dir.glob("*.tar.gz")):
          # ... existing loading code ...
          fleet_input = _snapshot_to_fleet_input(snapshot)
          fleets.append(fleet_input)
          tarball_paths[fleet_input.name] = path

      return fleets, tarball_paths
  ```

  **Breaking change:** Return type changes from `list[FleetInput]` to `tuple[list[FleetInput], dict[str, Path]]`. Update all callers (primarily `server.py` and any CLI entry point).

- [ ] **Step 3: Update loader import list**

  Add imports for the new dataclasses:
  ```python
  from yoinkc.architect.analyzer import (
      FleetInput, ConfigInput, ServiceInput, FirewallInput,
      QuadletInput, UserGroupInput, SysctlInput,
  )
  ```

- [ ] **Step 4: Write loader tests**

  Test with a fixture snapshot JSON (from the expanded driftify fixtures) that `_snapshot_to_fleet_input()` correctly populates all fields. Assert counts and field values for each artifact type. Test with missing sections (e.g., no `services` key in snapshot -> empty list).

**Commit:** `feat(architect): expand loader to extract all artifact types from snapshots`

---

### Task 1.5: Expand `LayerTopology.to_dict()` for multi-artifact JSON

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`

- [ ] **Step 1: Expand `to_dict()` to serialize all artifact types with badge fields**

  The current `to_dict()` returns packages as strings and configs as strings. Expand to serialize structured objects with export class and approximation metadata:

  ```python
  def to_dict(self) -> dict:
      return {
          "layers": [
              {
                  "name": l.name,
                  "parent": l.parent,
                  "packages": l.packages,
                  "configs": [
                      {
                          "path": c.path,
                          "kind": c.kind,
                          "owner_package": c.package,
                          "category": c.category,
                          "export_class": _export_class(c, "config"),
                          "approximate": c._approximate,
                      }
                      for c in l.configs
                  ],
                  "services": [
                      {
                          "unit": s.unit,
                          "action": s.action,
                          "owner_package": s.owning_package,
                          "export_class": _export_class(s, "service"),
                      }
                      for s in l.services
                  ],
                  "firewall_zones": [
                      {
                          "name": z.name,
                          "path": z.path,
                          "services": z.services,
                          "ports": z.ports,
                          "export_class": _export_class(z, "firewall_zone"),
                          "approximate": z._approximate,
                      }
                      for z in l.firewall_zones
                  ],
                  "quadlets": [
                      {
                          "path": q.path,
                          "name": q.name,
                          "image": q.image,
                          "export_class": _export_class(q, "quadlet"),
                      }
                      for q in l.quadlets
                  ],
                  "users_groups": [
                      {
                          "name": u.name,
                          "kind": u.kind,
                          "uid_or_gid": u.uid_or_gid,
                          "strategy": u.strategy,
                          "shell": u.shell,
                          "export_class": "display_only",
                      }
                      for u in l.users_groups
                  ],
                  "sysctls": [
                      {
                          "key": s.key,
                          "value": s.value,
                          "source": s.source,
                          "export_class": _export_class(s, "sysctl"),
                          "warning": "security-sensitive" if _sysctl_is_denylisted(s.key) else None,
                      }
                      for s in l.sysctls
                  ],
                  "fleets": l.fleets,
                  "fan_out": l.fan_out,
                  "turbulence": round(l.turbulence, 1),
              }
              for l in self.layers
          ],
          "fleets": [...],  # existing fleet info
          "fleet_tarball_paths": {
              k: str(v) for k, v in self.fleet_tarball_paths.items()
          } if hasattr(self, 'fleet_tarball_paths') else {},
      }
  ```

- [ ] **Step 2: Store `fleet_tarball_paths` on `LayerTopology`**

  Add `fleet_tarball_paths: dict[str, Path] = field(default_factory=dict)` to `LayerTopology`. Populated by `analyze_fleets()` or set by the caller after loading.

- [ ] **Step 3: Write serialization tests**

  Test that `to_dict()` output includes all 7 artifact types with correct field names. Test that `export_class` and `approximate` fields are present. Test round-trip: construct a topology with known data, serialize, verify JSON structure.

**Commit:** `feat(architect): expand topology JSON serialization for all artifact types`

---

### Task 1.6: Update `cli.py` and `server.py` for loader return type change

**Files:**
- Modify: `src/yoinkc/architect/cli.py` (primary call site for `load_refined_fleets()`)
- Modify: `src/yoinkc/architect/server.py` (rename handler, Phase 1 restrictions)

- [ ] **Step 1: Update loader call site in `cli.py`**

  `cli.py:_run_architect_inner()` calls `load_refined_fleets(input_dir)` which now returns `(fleets, tarball_paths)`. Update:

  ```python
  # Before:
  fleets = load_refined_fleets(input_dir)

  # After:
  fleets, tarball_paths = load_refined_fleets(input_dir)
  topology = analyze_fleets(fleets)
  topology.fleet_tarball_paths = tarball_paths
  ```

  Also update the `base_image` extraction (currently `fleets[0].base_image`) — this is unchanged in shape but verify it still works with the expanded `FleetInput`.

- [ ] **Step 2: Rename `_handle_pkg_operation` to `_handle_artifact_operation`**

  Rename and generalize. For Phase 1, only rename -- actual multi-type support comes in Phase 2. Add backward compatibility:

  ```python
  def _handle_artifact_operation(self, operation, content_length: int) -> None:
      body = json.loads(self.rfile.read(content_length))
      artifact_type = body.get("type", "package")
      if artifact_type != "package":
          self._send_json(400, {"error": f"Move/copy for '{artifact_type}' not yet supported"})
          return
      # For now, delegate to existing package move/copy
      operation(body["package"], body["from"], body["to"])
      self._send_json(200, self._topology.to_dict())
  ```

  Wire routes:
  ```python
  if path == "/api/move":
      self._handle_artifact_operation(self._topology.move_package, content_length)
  elif path == "/api/copy":
      self._handle_artifact_operation(self._topology.copy_package, content_length)
  ```

  **Backward compatibility:** If `type` is missing from body, defaults to `"package"`. If `"package"` key is present (old format), use it as `"artifact"`. Existing move/copy tests pass unchanged.

**Commit:** `refactor(architect): generalize server handler for multi-artifact operations`

---

### Task 1.7: Add 7-tab drawer UI (read-only)

**Files:**
- Modify: `src/yoinkc/templates/architect/architect.html.j2`
- Modify: `src/yoinkc/templates/architect/_js.html.j2`
- Modify: `src/yoinkc/templates/architect/_css.html.j2`

- [ ] **Step 1: Add tab bar HTML structure**

  In `architect.html.j2`, inside the drawer body (below the existing layer header), add a PF6 tabs component:

  ```html
  <div class="pf-v6-c-tabs" id="artifact-tabs">
    <ul class="pf-v6-c-tabs__list" role="tablist">
      <li class="pf-v6-c-tabs__item" role="presentation" data-tab="packages">
        <button class="pf-v6-c-tabs__link" id="tab-packages"
                role="tab" aria-selected="true"
                aria-controls="panel-packages">
          <span class="pf-v6-c-tabs__item-text">Packages</span>
          <span class="pf-v6-c-badge pf-m-read" id="badge-packages">0</span>
        </button>
      </li>
      <!-- Repeat for: configs, services, firewall, quadlets, users-groups, sysctls -->
    </ul>
  </div>
  <div id="panel-packages" class="artifact-panel" role="tabpanel" aria-labelledby="tab-packages"></div>
  <div id="panel-configs" class="artifact-panel" role="tabpanel" aria-labelledby="tab-configs" hidden></div>
  <!-- ... etc for all 7 panels -->
  ```

  Tab order: Packages, Configs, Services, Firewall, Quadlets, Users/Groups, Sysctls.

- [ ] **Step 2: Add tab switching JavaScript**

  In `_js.html.j2`:

  ```javascript
  var selectedTab = 'packages';

  function switchTab(tabName) {
    selectedTab = tabName;
    // Update tab bar active state
    document.querySelectorAll('.pf-v6-c-tabs__item').forEach(function(item) {
      var isActive = item.getAttribute('data-tab') === tabName;
      item.classList.toggle('pf-m-current', isActive);
      item.querySelector('[role="tab"]').setAttribute('aria-selected', isActive);
    });
    // Show/hide panels
    document.querySelectorAll('.artifact-panel').forEach(function(panel) {
      panel.hidden = panel.id !== 'panel-' + tabName;
    });
    renderTabPanel(tabName);
  }
  ```

  Wire tab click handlers in the render or init function.

- [ ] **Step 3: Implement read-only panel renderers**

  Add `renderTabPanel(tabName)` dispatch and individual panel functions:

  ```javascript
  function renderTabPanel(tabName) {
    var layer = getLayer(selectedLayer);
    if (!layer) return;
    var panel = document.getElementById('panel-' + tabName);
    if (!panel) return;

    switch (tabName) {
      case 'packages': renderPackagesPanel(panel, layer); break;
      case 'configs': renderConfigsPanel(panel, layer); break;
      case 'services': renderServicesPanel(panel, layer); break;
      case 'firewall': renderFirewallPanel(panel, layer); break;
      case 'quadlets': renderQuadletsPanel(panel, layer); break;
      case 'users-groups': renderUsersGroupsPanel(panel, layer); break;
      case 'sysctls': renderSysctlsPanel(panel, layer); break;
    }
  }
  ```

  **`renderPackagesPanel()`**: Refactor existing package rendering into this function. The existing drawer content moves inside the Packages tab panel. No behavioral change -- same move/copy buttons, same turbulence display.

  **`renderConfigsPanel()`**: List config rows with path, kind badge (RPM/unowned/orphaned), owner package (if any), export class badge. Read-only for Phase 1.

  **`renderServicesPanel()`**: List service rows with unit name, action badge (enable/disable/mask), owner package. Read-only.

  **`renderFirewallPanel()`**: List firewall zone rows with zone name, services list, ports list. Show "visible only" export class badge.

  **`renderQuadletsPanel()`**: List quadlet rows with path, name, image. Read-only.

  **`renderUsersGroupsPanel()`**: List user/group rows with name, kind, UID/GID, strategy, shell. Show "display only" export class badge.

  **`renderSysctlsPanel()`**: List sysctl rows with key, value, source file. Show "visible only" badge. Show "security-sensitive" warning for denylisted keys.

  Each panel function also renders the approximation badge (`~`) on artifacts where `approximate === true`.

- [ ] **Step 4: Update count badges**

  In `renderDrawer()` (or equivalent), update badge counts for each tab:

  ```javascript
  function updateBadgeCounts(layer) {
    document.getElementById('badge-packages').textContent = layer.packages.length;
    document.getElementById('badge-configs').textContent = layer.configs.length;
    document.getElementById('badge-services').textContent = layer.services.length;
    document.getElementById('badge-firewall').textContent = layer.firewall_zones.length;
    document.getElementById('badge-quadlets').textContent = layer.quadlets.length;
    document.getElementById('badge-users-groups').textContent = layer.users_groups.length;
    document.getElementById('badge-sysctls').textContent = layer.sysctls.length;
  }
  ```

- [ ] **Step 5: Add tab CSS**

  In `_css.html.j2`, add styles for:
  - Tab container spacing within the drawer
  - Badge styling for export class (full = green, visible_only = blue, display_only = gray, warn_only = orange)
  - Approximation badge (`~` indicator)
  - Per-panel row layout consistent with existing package rows

- [ ] **Step 6: Preserve tab selection across layer switches**

  When `selectLayer()` is called, re-render the current tab's panel (do not reset to Packages). The `selectedTab` state variable persists across layer changes.

**Test requirements:**
- Manual: All 7 tabs render with correct data from fixtures
- Manual: Tab switching works, badges update per layer
- Manual: Existing package functionality (move/copy) unaffected in Packages tab

**Commit:** `feat(architect): add 7-tab drawer UI with read-only artifact panels`

---

### Quality Gate: Phase 1

**Invariants:**
- All 7 artifact types flow from snapshot -> loader -> analyzer -> topology JSON -> UI tabs
- Package decomposition unchanged (same base/derived split as v1)
- Owned artifacts follow their package to the correct layer
- Independent artifacts follow prevalence rule
- Variant detection populates `_approximate` flag when content differs
- Export class computed correctly for all types

**Edge cases:**
- Single fleet: no base layer, all artifacts in derived
- Fleet with no configs/services/etc: tabs show "(0)" badge, empty panel
- Config with `package=None` and `kind=rpm_owned_modified`: falls back to prevalence
- Base image mismatch across fleets: existing base_image handling unchanged

**Regression risks:**
- `FleetInput.configs` type change (`list[str]` -> `list[ConfigInput]`) breaks callers
- `load_from_snapshots()` return type change breaks callers
- Existing package move/copy must still work in Packages tab

**Quality gate:** All existing tests pass. New decomposition tests pass. Manual demo: load a multi-artifact fixture, all 7 tabs populate, can still move packages.

---

## Phase 2: Move/Copy + Export (Independent Types)

**Goal:** Quadlets, firewall zones, sysctls, and unowned/orphaned configs can be moved, copied, and exported. Packages and owned artifacts are NOT movable yet (Phase 3).

**Demo narrative:** "Move a quadlet to a different layer. Export includes COPY directives for quadlet files."

### Task 2.1: Implement `move_artifact()` and `copy_artifact()` on `LayerTopology`

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`

- [ ] **Step 1: Add `move_artifact()` method**

  ```python
  def move_artifact(
      self,
      artifact_id: str,
      artifact_type: str,
      from_layer: str,
      to_layer: str,
      dependents: list[dict] | None = None,
  ) -> None:
      """Move an artifact between layers."""
      src = self._get_layer(from_layer)
      dst = self._get_layer(to_layer)
      if not src or not dst:
          raise ValueError(f"Layer not found: {from_layer} or {to_layer}")

      artifact_list_name = self._artifact_list_name(artifact_type)
      src_list = getattr(src, artifact_list_name)
      dst_list = getattr(dst, artifact_list_name)

      # Find and remove from source
      item = self._find_artifact(src_list, artifact_id, artifact_type)
      if item is None:
          raise ValueError(f"{artifact_type} '{artifact_id}' not found in {from_layer}")
      src_list.remove(item)

      # Handle base->derived broadcast: removing from base broadcasts to ALL derived
      if src.parent is None and dst.parent is not None:
          for layer in self.layers:
              if layer.parent == src.name and layer.name != dst.name:
                  getattr(layer, artifact_list_name).append(item)

      # Add to destination
      dst_list.append(item)

      # Recalculate turbulence (package-based only)
      if artifact_type == "package":
          src._recalc_turbulence()
          dst._recalc_turbulence()

      # Process dependents if provided (Phase 3 feature, stub for now)
      if dependents:
          for dep in dependents:
              self.move_artifact(
                  dep["id"], dep["type"],
                  from_layer, to_layer,
              )
  ```

- [ ] **Step 2: Add `copy_artifact()` method**

  Similar to `move_artifact()` but does not remove from source:

  ```python
  def copy_artifact(
      self,
      artifact_id: str,
      artifact_type: str,
      from_layer: str,
      to_layer: str,
      dependents: list[dict] | None = None,
  ) -> None:
      """Copy an artifact to another layer without removing from source."""
      src = self._get_layer(from_layer)
      dst = self._get_layer(to_layer)
      if not src or not dst:
          raise ValueError(f"Layer not found")

      artifact_list_name = self._artifact_list_name(artifact_type)
      src_list = getattr(src, artifact_list_name)

      item = self._find_artifact(src_list, artifact_id, artifact_type)
      if item is None:
          raise ValueError(f"{artifact_type} '{artifact_id}' not found in {from_layer}")

      # Deep copy to destination
      import copy
      getattr(dst, artifact_list_name).append(copy.deepcopy(item))

      if dependents:
          for dep in dependents:
              self.copy_artifact(dep["id"], dep["type"], from_layer, to_layer)
  ```

- [ ] **Step 3: Add helper methods**

  ```python
  @staticmethod
  def _artifact_list_name(artifact_type: str) -> str:
      """Map artifact type string to Layer field name."""
      return {
          "package": "packages",
          "config": "configs",
          "service": "services",
          "firewall_zone": "firewall_zones",
          "quadlet": "quadlets",
          "user_group": "users_groups",
          "sysctl": "sysctls",
      }[artifact_type]

  @staticmethod
  def _find_artifact(lst, artifact_id, artifact_type):
      """Find an artifact in a list by its identity key."""
      for item in lst:
          if artifact_type == "package" and item == artifact_id:
              return item
          elif artifact_type == "config" and item.path == artifact_id:
              return item
          elif artifact_type == "service" and item.unit == artifact_id:
              return item
          elif artifact_type == "firewall_zone" and item.name == artifact_id:
              return item
          elif artifact_type == "quadlet" and item.path == artifact_id:
              return item
          elif artifact_type == "user_group" and f"{item.kind}:{item.name}" == artifact_id:
              return item
          elif artifact_type == "sysctl" and item.key == artifact_id:
              return item
      return None
  ```

- [ ] **Step 4: Keep existing `move_package()` and `copy_package()` as backward-compatible wrappers**

  ```python
  def move_package(self, package: str, from_layer: str, to_layer: str) -> None:
      """Backward-compatible package move."""
      self.move_artifact(package, "package", from_layer, to_layer)

  def copy_package(self, package: str, from_layer: str, to_layer: str) -> None:
      """Backward-compatible package copy."""
      self.copy_artifact(package, "package", from_layer, to_layer)
  ```

**Commit:** `feat(architect): implement move_artifact and copy_artifact for all types`

---

### Task 2.2: Update server API for multi-type operations

**Files:**
- Modify: `src/yoinkc/architect/server.py`

- [ ] **Step 1: Expand `_handle_artifact_operation()` for Phase 2 types**

  ```python
  _PHASE2_TYPES = {"quadlet", "firewall_zone", "sysctl"}
  _PHASE2_INDEPENDENT_CONFIG_KINDS = {"unowned", "orphaned"}

  def _handle_artifact_operation(self, operation, content_length: int) -> None:
      body = json.loads(self.rfile.read(content_length))
      artifact_type = body.get("type", "package")
      artifact_id = body.get("artifact", body.get("package", ""))
      dependents = body.get("dependents", None)

      # Phase 2: only independent types allowed
      if artifact_type == "package":
          # Keep legacy package move working via backward compat
          operation(artifact_id, artifact_type, body["from"], body["to"])
      elif artifact_type in _PHASE2_TYPES:
          operation(artifact_id, artifact_type, body["from"], body["to"])
      elif artifact_type == "config":
          # Only unowned/orphaned configs movable in Phase 2
          # Server looks up config kind to validate
          layer = self._topology._get_layer(body["from"])
          config = self._topology._find_artifact(layer.configs, artifact_id, "config")
          if config and config.kind == "rpm_owned_modified" and config.package:
              self._send_json(400, {
                  "error": "RPM-owned config moves require tied-change support (Phase 3)"
              })
              return
          operation(artifact_id, artifact_type, body["from"], body["to"])
      elif artifact_type == "service":
          self._send_json(400, {
              "error": "Service moves require tied-change support (Phase 3)"
          })
          return
      else:
          self._send_json(400, {"error": f"Unknown artifact type: {artifact_type}"})
          return

      self._send_json(200, self._topology.to_dict())
  ```

- [ ] **Step 2: Write server tests**

  Test in `tests/architect/test_server.py` (update or create):
  - Move quadlet: 200, topology updated
  - Move sysctl: 200, topology updated
  - Move unowned config: 200, topology updated
  - Move RPM-owned config: 400, error message about Phase 3
  - Move service with owning_package: 400, error message about Phase 3
  - Move with missing `type` field: defaults to package, works as before
  - Backward compatibility: old-format body with `"package"` key still works

**Commit:** `feat(architect): expand server API for multi-type move/copy operations`

---

### Task 2.3: Enable move/copy buttons on independent-type tabs

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`

- [ ] **Step 1: Add move/copy UI to independent-type panels**

  For configs (unowned/orphaned), quadlets, firewall zones, and sysctls: add "Move up" button and "Copy to" dropdown (same pattern as existing package rows). The button handler calls the server with the `type` parameter:

  ```javascript
  function moveArtifact(artifactId, artifactType, fromLayer, toLayer) {
    fetch('/api/move', {
      method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({
        artifact: artifactId,
        type: artifactType,
        from: fromLayer,
        to: toLayer,
      }),
    })
    .then(function(r) { return r.json(); })
    .then(function(data) {
      if (data.error) { alert(data.error); return; }
      topology = data;
      render();
    });
  }
  ```

  For RPM-owned configs and owned services: rows are read-only in Phase 2 (no buttons). Show a "(tied to {package})" label instead.

- [ ] **Step 2: Add visual distinction for locked vs. movable rows**

  Configs panel: RPM-owned configs show a lock icon or "(tied)" label. Unowned/orphaned configs show move/copy buttons. This makes the Phase 2 boundary visible to the user.

  Services panel: Services with `owning_package` show "(tied to {package})" label. Services without `owning_package` could be movable, but the spec blocks all service moves until Phase 3. Show no buttons on services in Phase 2.

  Users/Groups panel: Display-only, no move/copy buttons ever (but decompose and display).

**Commit:** `feat(architect): enable move/copy buttons on independent artifact tabs`

---

### Task 2.4: Create render adapter (`render_adapter.py`)

**Files:**
- Create: `src/yoinkc/architect/render_adapter.py`

- [ ] **Step 1: Implement `LayerRenderInput` dataclass**

  ```python
  from dataclasses import dataclass, field
  from yoinkc.architect.analyzer import (
      Layer, ConfigInput, ServiceInput, QuadletInput,
  )

  @dataclass
  class LayerRenderInput:
      """Renderer-grade payload for a single architect layer.

      Contains only exportable artifacts. Nothing classified as
      warn_only, deferred, or display_only appears here.
      """
      packages: list[str] = field(default_factory=list)
      configs: list[ConfigInput] = field(default_factory=list)
      services: list[ServiceInput] = field(default_factory=list)
      quadlets: list[QuadletInput] = field(default_factory=list)
      config_source_paths: dict[str, str] = field(default_factory=dict)
      quadlet_source_paths: dict[str, str] = field(default_factory=dict)
  ```

- [ ] **Step 2: Implement `build_layer_render_input()`**

  ```python
  def build_layer_render_input(layer: Layer) -> LayerRenderInput:
      """Reshape architect Layer into renderer-grade payload.

      Filters to exportable artifacts only. Excludes:
      - Orphaned configs (warn_only)
      - Masked services (warn_only)
      - Artifacts with _approximate=True (unresolved variant -> warn_only)
      - Users/groups (display_only)
      - Firewall zones (visible_only)
      - Sysctls (visible_only)
      """
      exportable_configs = [
          c for c in layer.configs
          if c.kind != "orphaned"
          and not getattr(c, '_approximate', False)
      ]
      exportable_services = [
          s for s in layer.services
          if s.action != "mask"
      ]

      config_sources = {c.path: c.source_ref for c in exportable_configs if c.source_ref}
      quadlet_sources = {q.path: q.source_ref for q in layer.quadlets}

      return LayerRenderInput(
          packages=layer.packages,
          configs=exportable_configs,
          services=exportable_services,
          quadlets=layer.quadlets,
          config_source_paths=config_sources,
          quadlet_source_paths=quadlet_sources,
      )
  ```

- [ ] **Step 3: Implement `_build_layer_snapshot_shim()`**

  Build a minimal `InspectionSnapshot`-shaped dict from architect layer data that the existing renderer domain modules can consume via their `section_lines()` functions. This is the core adapter pattern — it reshapes architect data into the shape the renderers already expect, rather than reimplementing rendering logic.

  ```python
  def _build_layer_snapshot_shim(
      render_input: LayerRenderInput,
      base_image: str,
  ) -> dict:
      """Build a snapshot-shaped dict for existing renderer modules.

      The existing renderers (packages.section_lines, services.section_lines,
      config.section_lines, containers.section_lines) expect an
      InspectionSnapshot or compatible dict. This builds a minimal shim
      from architect layer data.

      IMPORTANT: Only exportable artifacts go in the shim. Warn-only,
      deferred, and display-only artifacts are never fed to renderers.
      """
      return {
          "rpm": {
              "packages_added": [
                  {"nvra": p, "name": _nvra_to_name(p), "include": True}
                  for p in render_input.packages
              ],
              "base_image": base_image,
          },
          "services": {
              "state_changes": [
                  {"unit": s.unit, "action": s.action, "include": True}
                  for s in render_input.services
              ],
          },
          "containers": {
              "quadlet_units": [
                  {"path": q.path, "name": q.name, "image": q.image, "include": True}
                  for q in render_input.quadlets
              ],
          },
          "config": {
              "files": [
                  {"path": c.path, "kind": c.kind, "package": c.package, "include": True}
                  for c in render_input.configs
              ],
          },
      }
  ```

- [ ] **Step 4: Implement `render_layer_containerfile()` using existing renderer modules**

  ```python
  from yoinkc.renderers.containerfile import (
      packages, services, config, containers,
  )

  def render_layer_containerfile(
      layer: Layer,
      parent: str | None,
      base_image: str,
  ) -> str:
      """Render a Containerfile by delegating to existing renderer modules.

      For full-support exportable artifacts: calls existing section_lines()
      functions through a snapshot-shaped shim.
      For warn_only/visible_only: emits comment-only lines directly.
      For display_only (users/groups): no Containerfile output.
      """
      render_input = build_layer_render_input(layer)
      shim = _build_layer_snapshot_shim(render_input, base_image)

      lines = []

      # FROM directive
      if parent is None:
          lines.append(f"FROM {base_image}")
      else:
          lines.append(f"FROM localhost/{parent}:latest")
      lines.append("")

      # Delegate to existing renderer modules for full-support types
      # Each module's section_lines() returns a list[str] of Containerfile lines
      # The shim provides the snapshot shape they expect
      snapshot_obj = _shim_to_snapshot(shim)  # convert dict to minimal Pydantic model
      lines += packages.section_lines(snapshot_obj, base=base_image)
      lines += services.section_lines(snapshot_obj)
      lines += containers.section_lines(snapshot_obj)  # quadlets
      # Config COPY is handled by stage_layer_files + config tree, not section_lines

      # === Advisory output (comment-only, never COPY/RUN) ===
      _emit_advisory_comments(lines, layer)

      # bootc lint
      lines.append("RUN bootc container lint")
      lines.append("")

      return "\n".join(lines)

  def _emit_advisory_comments(lines: list[str], layer: Layer) -> None:
      """Emit comment-only output for warn_only / visible_only artifacts.

      These NEVER produce COPY or RUN directives.
      """
      # Orphaned configs
      orphaned = [c for c in layer.configs if c.kind == "orphaned"]
      if orphaned:
          lines.append("# === Orphaned Configs (REVIEW) ===")
          for c in orphaned:
              lines.append(f"# REVIEW: orphaned config {c.path}")
          lines.append("")

      # Unresolved variants
      approximate = [c for c in layer.configs if getattr(c, '_approximate', False)]
      if approximate:
          lines.append("# === Variant Configs (REVIEW) ===")
          for c in approximate:
              lines.append(f"# REVIEW: variant detected for {c.path}")
          lines.append("")

      # Firewall zones (visible-only)
      if layer.firewall_zones:
          lines.append("# === Firewall Zones (manual review) ===")
          for z in layer.firewall_zones:
              lines.append(f"# REVIEW: zone '{z.name}'")
          lines.append("")

      # Sysctls (visible-only, with security warnings)
      if layer.sysctls:
          lines.append("# === Sysctls (manual review) ===")
          for s in layer.sysctls:
              prefix = "# WARNING:" if _sysctl_is_denylisted(s.key) else "# REVIEW:"
              lines.append(f"{prefix} sysctl {s.key}={s.value}")
          lines.append("")

      # Users/groups: display-only, NO output
  ```

  **Key difference from previous version:** The renderer modules (`packages.section_lines()`, `services.section_lines()`, `containers.section_lines()`) do the actual Containerfile generation. The adapter only builds the snapshot shim and emits advisory comments. This prevents renderer drift between architect export and the main yoinkc rendering path.

  **Implementation note:** `_shim_to_snapshot()` needs to construct a minimal `InspectionSnapshot` from the dict. The exact shape depends on which fields each `section_lines()` function actually accesses — read each renderer module to determine the minimum viable shim. Some modules may need additional fields (e.g., `packages.section_lines()` takes extra kwargs like `c_ext_pip` and `needs_multistage` — for architect export, these default to empty/False since we're only handling RPM packages).

- [ ] **Step 4: Implement `stage_layer_files()`**

  ```python
  import tarfile
  from pathlib import Path

  def stage_layer_files(
      layer: Layer,
      tar: tarfile.TarFile,
      layer_name: str,
      fleet_tarball_paths: dict[str, Path],
  ) -> None:
      """Stage config and quadlet files into the tarball's tree/ directory.

      Extracts actual file content from the source fleet tarballs
      using the provenance chain (source_fleet -> tarball path,
      source_ref -> path within tarball).
      """
      render_input = build_layer_render_input(layer)

      for dest_path, source_ref in render_input.config_source_paths.items():
          _stage_file(tar, layer_name, dest_path, source_ref, layer, fleet_tarball_paths)

      for dest_path, source_ref in render_input.quadlet_source_paths.items():
          _stage_file(tar, layer_name, dest_path, source_ref, layer, fleet_tarball_paths)
  ```

- [ ] **Step 5: Write adapter tests**

  Create `tests/architect/test_render_adapter.py`:
  - Test `build_layer_render_input()`: orphaned configs excluded, masked services excluded, approximate configs excluded, users/groups excluded, firewall zones excluded, sysctls excluded
  - Test `_build_layer_snapshot_shim()`: output has correct snapshot shape for renderer modules (rpm.packages_added, services.state_changes, containers.quadlet_units, config.files)
  - Test `render_layer_containerfile()` delegates to existing renderers: output matches what `packages.section_lines()`, `services.section_lines()`, `containers.section_lines()` would produce for the same data
  - Test advisory output: `# REVIEW:` comments for orphaned/variant/firewall/sysctls, `# WARNING:` for denylisted sysctls, no output for users/groups
  - Test export safety boundary: warn_only artifacts never produce COPY/RUN lines
  - Test empty layer: only FROM + bootc lint
  - Test renderer parity: adapter output for a known layer matches what the main renderer would produce for an equivalent snapshot

**Commit:** `feat(architect): create render adapter for Containerfile export`

---

### Task 2.5: Update `export.py` to use render adapter

**Files:**
- Modify: `src/yoinkc/architect/export.py`

- [ ] **Step 1: Replace inline rendering with adapter calls**

  ```python
  from yoinkc.architect.render_adapter import render_layer_containerfile, stage_layer_files

  def export_topology(topo: LayerTopology, base_image: str) -> bytes:
      buf = io.BytesIO()
      with tarfile.open(fileobj=buf, mode="w:gz") as tar:
          for layer in topo.layers:
              # Containerfile via adapter
              containerfile = render_layer_containerfile(
                  layer, layer.parent, base_image,
              )
              _add_string_to_tar(tar, f"{layer.name}/Containerfile", containerfile)

              # Stage file artifacts into tree/
              stage_layer_files(
                  layer, tar, layer.name,
                  topo.fleet_tarball_paths,
              )

          build_sh = _render_build_sh(topo, base_image)
          _add_string_to_tar(tar, "build.sh", build_sh)

      return buf.getvalue()
  ```

  Keep the existing `_nvra_to_name()`, `_render_build_sh()`, and `_add_string_to_tar()` helpers. The `render_containerfile()` function can remain for backward compatibility but is no longer called by `export_topology()`.

- [ ] **Step 2: Update preview endpoint**

  In `server.py`, the `/api/preview/<layer>` endpoint calls `render_containerfile()`. Update it to use `render_layer_containerfile()` instead:

  ```python
  # In do_GET, preview handler:
  from yoinkc.architect.render_adapter import render_layer_containerfile
  content = render_layer_containerfile(layer, layer.parent, self._base_image)
  ```

- [ ] **Step 3: Write export integration tests**

  Test that `export_topology()` with a multi-artifact topology produces:
  - A tarball with Containerfile per layer
  - Containerfiles contain package, config, service, quadlet sections
  - Containerfiles contain REVIEW comments for firewall/sysctl
  - No COPY/RUN for users/groups
  - `tree/` directory contains staged config and quadlet files (when source tarballs available)
  - `build.sh` unchanged

**Commit:** `feat(architect): integrate render adapter into export pipeline`

---

### Quality Gate: Phase 2

**Invariants:**
- Independent types (quadlets, firewall zones, sysctls, unowned configs) can be moved/copied
- Package move/copy still works (backward compatible)
- RPM-owned config and owned service moves return 400 with clear error
- Export produces correct Containerfile sections for all types
- Export safety boundary: no COPY/RUN for warn_only, visible_only, display_only artifacts
- Sysctl denylist produces `# WARNING` not just `# REVIEW`
- Approximate configs downgrade to `# REVIEW` comment in export

**Edge cases:**
- Move last artifact of a type from a layer: layer's list becomes empty
- Copy artifact that already exists in target: duplicate entry (spec allows this)
- Move from base to derived: broadcasts to all other derived layers
- Export with no configs/services: Containerfile has FROM + packages + bootc lint only

**Regression risks:**
- `render_containerfile()` still used by existing tests -- keep it available
- Export tarball structure change (tree/ directory added)
- Preview endpoint behavior change (now shows multi-artifact output)

**Quality gate:** All Phase 1 tests pass. Move/copy tests for independent types pass. Export tests verify correct Containerfile output. Server tests verify 400 rejection for tied types.

### Cross-Repo Fixture/Provenance Gate (before Phase 2 export tasks)

Before starting Task 2.4 (render adapter) and Task 2.5 (export), verify the driftify→yoinkc fixture contract is sound end-to-end. Export correctness depends on the provenance chain working — this gate must pass before any export code is written.

- [ ] **Gate 1: Tarball member layout matches `source_ref` assumptions.** For each driftify fixture tarball, verify that config files live at `config/{path}` and quadlet files live at `quadlet/{name}` — matching the `source_ref` format the loader populates.
- [ ] **Gate 2: One config round-trip.** Generate a fixture with a modified RPM-owned config → load via `load_refined_fleets()` → verify `ConfigInput.source_fleet` and `source_ref` are populated → call `stage_layer_files()` → verify the staged file matches the original fixture content byte-for-byte.
- [ ] **Gate 3: One quadlet round-trip.** Same as Gate 2 but for a quadlet unit file.
- [ ] **Gate 4: Fixture ownership decision.** Driftify tarballs are the canonical source — yoinkc consumes them directly via `load_refined_fleets()`. No separate `tests/e2e/generate-fixtures.py` is needed. If a fixture translation step is ever required, it becomes a concrete task at that time.

**Commit:** `test(architect): verify driftify fixture provenance round-trip`

---

## Phase 3: Packages, Owned Artifacts + Tied Changes

**Goal:** Packages and their owned configs/services become movable via the tied-change model. Confirmation strip and reverse prompt work.

**Demo narrative:** "Move httpd and its configs follow. The tool understands which things belong together."

### Task 3.1: Enable package and owned-artifact moves WITH tied-change validation

**Files:**
- Modify: `src/yoinkc/architect/server.py`
- Modify: `src/yoinkc/architect/analyzer.py` (add dependent validation)

**IMPORTANT:** Phase 2 restrictions on package/RPM-owned-config/owned-service moves stay in place until this task is COMPLETE — meaning the tied-change validation, confirmation strip UI, and reverse prompt all land together. Do NOT remove restrictions in a separate commit before the UX and validation exist.

- [ ] **Step 1: Add dependent validation to `move_artifact()` / `copy_artifact()`**

  Before removing server restrictions, add validation that enforces the tied-change contract:

  The analyzer's `move_artifact()` receives a `package_confirmed` flag from the server — it does NOT read from `body` directly:

  ```python
  def move_artifact(self, artifact_id, artifact_type, from_layer, to_layer,
                    dependents=None, package_confirmed=False):
      # For packages: require dependents parameter when owned configs/services exist
      if artifact_type == "package":
          owned = self._get_dependents(artifact_id, from_layer)
          if owned and dependents is None:
              raise ValueError(
                  f"Package '{artifact_id}' has {len(owned)} dependent artifacts. "
                  "Must provide 'dependents' list (use confirmation strip in UI)."
              )
          # Move dependents that were checked
          if dependents:
              for dep in dependents:
                  self._move_single(dep["artifact"], dep["type"],
                                    from_layer, to_layer)

      # For RPM-owned configs: validate owning package relationship
      if artifact_type == "config":
          config = self._find_config(artifact_id, from_layer)
          if config and config.package and not package_confirmed:
              raise ValueError(
                  f"Config '{artifact_id}' is owned by '{config.package}'. "
                  "Must confirm package intent (use reverse prompt in UI)."
              )
  ```

- [ ] **Step 2: Update server handler to pass signals through and remove restrictions**

  The server parses `body` and passes the relevant signals to the analyzer. Restrictions are removed in this same commit — safe because validation now exists:

  ```python
  def _handle_artifact_operation(self, operation, content_length: int) -> None:
      body = json.loads(self.rfile.read(content_length))
      artifact_type = body.get("type", "package")
      artifact_id = body.get("artifact", body.get("package", ""))
      dependents = body.get("dependents", None)
      package_confirmed = body.get("package_confirmed", False)

      try:
          operation(artifact_id, artifact_type, body["from"], body["to"],
                    dependents=dependents,
                    package_confirmed=package_confirmed)
      except ValueError as e:
          self._send_json(400, {"error": str(e)})
          return

      self._send_json(200, self._topology.to_dict())
  ```

- [ ] **Step 3: Add negative tests for validation**

  - Test: package move without `dependents` when owned configs exist → 400
  - Test: RPM-owned config move without `package_confirmed` → 400
  - Test: package move with `dependents` → all move together
  - Test: package move with some dependents unchecked → unchecked stay behind

- [ ] **Step 4: Update server tests**

  - Move package with dependents: 200, package and dependents move together
  - Move RPM-owned config: 200 (no longer blocked)
  - Move owned service: 200 (no longer blocked)

**Commit:** `feat(architect): enable tied-change moves for packages and owned artifacts`

---

### Task 3.2: Build `pkgDependents` index in frontend

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`

- [ ] **Step 1: Build dependents index from topology data**

  After topology loads (and after every update), build an index mapping each package to its dependent artifacts:

  ```javascript
  function buildPkgDependents(layer) {
    var deps = {};  // package_name -> [{type, id, label}, ...]

    // Configs owned by packages
    layer.configs.forEach(function(c) {
      if (c.kind === 'rpm_owned_modified' && c.owner_package) {
        if (!deps[c.owner_package]) deps[c.owner_package] = [];
        deps[c.owner_package].push({
          type: 'config', id: c.path,
          label: c.path + ' (config, RPM-owned)'
        });
      }
    });

    // Services owned by packages
    layer.services.forEach(function(s) {
      if (s.owner_package) {
        if (!deps[s.owner_package]) deps[s.owner_package] = [];
        deps[s.owner_package].push({
          type: 'service', id: s.unit,
          label: s.unit + ' (service, ' + s.action + ')'
        });
      }
    });

    return deps;
  }
  ```

  **Note:** Package name in the index is the short name (not NVRA). Use a JS equivalent of `_nvra_to_name()` when looking up packages.

- [ ] **Step 2: Add `nvraToName()` JavaScript helper**

  Port the existing Python `_nvra_to_name()` to JavaScript:

  ```javascript
  function nvraToName(nvra) {
    var arches = ['.x86_64', '.noarch', '.i686', '.aarch64', '.ppc64le', '.s390x'];
    for (var i = 0; i < arches.length; i++) {
      if (nvra.endsWith(arches[i])) {
        nvra = nvra.slice(0, -arches[i].length);
        break;
      }
    }
    nvra = nvra.replace(/-[^-]*$/, '');  // remove release
    nvra = nvra.replace(/-[^-]*$/, '');  // remove version
    return nvra;
  }
  ```

**Commit:** `feat(architect): build package dependents index for tied changes`

---

### Task 3.3: Implement confirmation strip (package move)

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`
- Modify: `src/yoinkc/templates/architect/_css.html.j2`

- [ ] **Step 1: Replace immediate move with confirmation flow**

  When user clicks "Move up" on a package in the Packages tab:

  1. Look up `pkgDependents[nvraToName(pkg)]`
  2. If dependents exist: expand the row to show confirmation strip with checkboxes
  3. If no dependents: execute move immediately (existing fast-path behavior)

  ```javascript
  var confirmingPkg = null;  // package NVRA currently in confirmation mode
  var confirmDeps = [];       // checked dependents

  function onPackageMoveClick(pkg, fromLayer, toLayer) {
    var pkgName = nvraToName(pkg);
    var deps = pkgDependents[pkgName] || [];
    if (deps.length === 0) {
      // Fast path: no dependents, move immediately
      moveArtifact(pkg, 'package', fromLayer, toLayer);
      return;
    }
    // Show confirmation strip
    confirmingPkg = pkg;
    confirmDeps = deps.map(function(d) { return {type: d.type, id: d.id, label: d.label, checked: true}; });
    render();
  }
  ```

- [ ] **Step 2: Render confirmation strip HTML**

  In `renderPackagesPanel()`, when `confirmingPkg` matches the current row, render the strip:

  ```javascript
  // Inside the package row rendering:
  if (confirmingPkg === pkg) {
    html += '<div class="confirmation-strip">';
    html += '<div class="confirmation-strip__header">Will also move:</div>';
    confirmDeps.forEach(function(d, i) {
      html += '<label class="confirmation-strip__item">';
      html += '<input type="checkbox" ' + (d.checked ? 'checked' : '') + ' data-dep-idx="' + i + '"> ';
      html += d.label;
      html += '</label>';
    });
    html += '<div class="confirmation-strip__actions">';
    html += '<button class="pf-v6-c-button pf-m-primary" onclick="confirmMove()">Confirm move</button>';
    html += '<button class="pf-v6-c-button pf-m-link" onclick="cancelConfirm()">Cancel</button>';
    html += '</div></div>';
  }
  ```

- [ ] **Step 3: Implement confirm and cancel handlers**

  ```javascript
  function confirmMove() {
    var checkedDeps = confirmDeps
      .filter(function(d) { return d.checked; })
      .map(function(d) { return {type: d.type, id: d.id}; });

    moveArtifact(confirmingPkg, 'package', selectedLayer, /* toLayer */,
                 checkedDeps.length > 0 ? checkedDeps : undefined);
    confirmingPkg = null;
    confirmDeps = [];
  }

  function cancelConfirm() {
    confirmingPkg = null;
    confirmDeps = [];
    render();
  }
  ```

  Update `moveArtifact()` to accept and send `dependents` in the request body.

- [ ] **Step 4: Add confirmation strip CSS**

  ```css
  .confirmation-strip {
    background: var(--pf-v6-global--BackgroundColor--200);
    border-top: 1px solid var(--pf-v6-global--BorderColor--100);
    padding: var(--pf-v6-global--spacer--sm) var(--pf-v6-global--spacer--md);
    margin-top: var(--pf-v6-global--spacer--xs);
  }
  .confirmation-strip__header {
    font-weight: var(--pf-v6-global--FontWeight--bold);
    margin-bottom: var(--pf-v6-global--spacer--xs);
  }
  .confirmation-strip__item {
    display: block;
    padding: 2px 0;
  }
  .confirmation-strip__actions {
    margin-top: var(--pf-v6-global--spacer--sm);
    display: flex;
    gap: var(--pf-v6-global--spacer--sm);
  }
  ```

**Commit:** `feat(architect): implement confirmation strip for package tied changes`

---

### Task 3.4: Implement reverse prompt (config/service move)

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`
- Modify: `src/yoinkc/templates/architect/_css.html.j2`

- [ ] **Step 1: Add reverse prompt for RPM-owned config move**

  When user clicks "Move up" on an RPM-owned config:

  1. Check if `config.owner_package` is set
  2. If yes: show prompt "This config is owned by {package}. Also move {package} and its other configs?"
  3. Two buttons: "Yes, move package too" / "No, move config only"

  ```javascript
  var reversePrompt = null;  // {configPath, ownerPackage, fromLayer, toLayer}

  function onConfigMoveClick(configPath, fromLayer, toLayer) {
    var layer = getLayer(fromLayer);
    var config = layer.configs.find(function(c) { return c.path === configPath; });
    if (config && config.kind === 'rpm_owned_modified' && config.owner_package) {
      reversePrompt = {
        artifactId: configPath, artifactType: 'config',
        ownerPackage: config.owner_package,
        fromLayer: fromLayer, toLayer: toLayer
      };
      render();
      return;
    }
    // Unowned/orphaned: move immediately
    moveArtifact(configPath, 'config', fromLayer, toLayer);
  }
  ```

- [ ] **Step 2: Render reverse prompt UI**

  ```javascript
  // In renderConfigsPanel(), when reversePrompt matches:
  if (reversePrompt && reversePrompt.artifactId === config.path) {
    html += '<div class="reverse-prompt">';
    html += 'This config is owned by <strong>' + reversePrompt.ownerPackage + '</strong>.';
    html += ' Also move the package and its other configs?';
    html += '<div class="reverse-prompt__actions">';
    html += '<button class="pf-v6-c-button pf-m-primary" onclick="reverseYes()">Yes, move package too</button>';
    html += '<button class="pf-v6-c-button pf-m-secondary" onclick="reverseNo()">No, move config only</button>';
    html += '<button class="pf-v6-c-button pf-m-link" onclick="reverseCancel()">Cancel</button>';
    html += '</div></div>';
  }
  ```

- [ ] **Step 3: Implement reverse prompt handlers**

  ```javascript
  function reverseYes() {
    // Move the owning package (which triggers its dependents via confirmation strip logic)
    // Find the package NVRA from the package name
    var layer = getLayer(reversePrompt.fromLayer);
    var pkgNvra = layer.packages.find(function(p) {
      return nvraToName(p) === reversePrompt.ownerPackage;
    });
    if (pkgNvra) {
      onPackageMoveClick(pkgNvra, reversePrompt.fromLayer, reversePrompt.toLayer);
    }
    reversePrompt = null;
  }

  function reverseNo() {
    // Move just the config, no package
    moveArtifact(reversePrompt.artifactId, 'config',
                 reversePrompt.fromLayer, reversePrompt.toLayer);
    reversePrompt = null;
  }

  function reverseCancel() {
    reversePrompt = null;
    render();
  }
  ```

- [ ] **Step 4: Add same pattern for owned services**

  Moving a service with `owning_package` set triggers the same reverse prompt:
  "This service is provided by {package}. Also move {package} and its configs?"

- [ ] **Step 5: Enable move/copy buttons on Packages, Configs (all kinds), and Services tabs**

  Remove the Phase 2 read-only restrictions on RPM-owned config rows and owned service rows. All artifact types now show move/copy buttons.

- [ ] **Step 6: Add reverse prompt CSS**

  ```css
  .reverse-prompt {
    background: var(--pf-v6-global--BackgroundColor--200);
    border: 1px solid var(--pf-v6-global--BorderColor--100);
    border-radius: var(--pf-v6-global--BorderRadius--sm);
    padding: var(--pf-v6-global--spacer--sm) var(--pf-v6-global--spacer--md);
    margin-top: var(--pf-v6-global--spacer--xs);
  }
  .reverse-prompt__actions {
    margin-top: var(--pf-v6-global--spacer--sm);
    display: flex;
    gap: var(--pf-v6-global--spacer--sm);
  }
  ```

**Test requirements:**
- Test confirmation strip: package with 2 configs and 1 service shows all 3 in strip
- Test confirmation strip: package with 0 dependents moves immediately
- Test reverse prompt: RPM-owned config shows prompt, unowned config does not
- Test reverse prompt: "Yes" triggers package move with all dependents
- Test reverse prompt: "No" moves config only
- Test cancel: both strip and prompt return to normal state

**Commit:** `feat(architect): implement reverse prompt for owned config/service moves`

---

### Task 3.5: Write tied-change move/copy tests

**Files:**
- Create/expand: `tests/architect/test_move_copy.py`

- [ ] **Step 1: Test package move with dependents**

  - Move package httpd from web-servers to base
  - Provide dependents: config `/etc/httpd/conf/httpd.conf` and service `httpd.service`
  - Assert: all three artifacts now in base layer
  - Assert: all three removed from web-servers layer

- [ ] **Step 2: Test package move without dependents (fast path)**

  - Move package with no owned configs/services
  - Assert: only the package moves, no side effects on configs/services

- [ ] **Step 3: Test package copy with dependents**

  - Copy package from base to derived
  - Assert: package and dependents exist in both layers

- [ ] **Step 4: Test config move with owner_package (reverse "No")**

  - Move config `/etc/httpd/conf/httpd.conf` from web-servers to base WITHOUT its package
  - Assert: config moves, httpd package stays in web-servers
  - This tests the "No, move config only" path

- [ ] **Step 5: Test config move with owner_package (reverse "Yes")**

  - Move config then trigger package move with dependents
  - This is a multi-step operation at the UI level, but at the API level it is just a package move with dependents

- [ ] **Step 6: Test base-to-derived broadcast with dependents**

  - Move package from base to derived-a
  - Assert: package broadcasts to derived-b, derived-c
  - Assert: dependents also broadcast correctly

**Commit:** `test(architect): add tied-change move/copy tests`

---

### Quality Gate: Phase 3

**Invariants:**
- All artifact types movable/copyable
- Package move with dependents: dependents follow atomically
- Reverse prompt: RPM-owned config/owned service triggers owner package prompt
- Unowned config/orphaned config: no prompt, direct move
- Service without owning_package: no prompt, direct move
- Base-to-derived broadcast works for all types including dependents

**Edge cases:**
- Package with 0 dependents: fast path, no confirmation strip
- Package with 10+ dependents: strip renders scrollable
- Move config whose owning package is not in the same layer: no reverse prompt (nothing to move)
- Uncheck all dependents in confirmation strip: only package moves
- Move a dependent config directly (after unchecking in strip): works independently

**Regression risks:**
- Backward-compatible package move (no `type` field) still works
- Existing keyboard shortcuts and UI interactions preserved
- Export still works correctly after tied-change moves

**Quality gate:** All Phase 1-2 tests pass. Tied-change tests pass. Confirmation strip and reverse prompt functional in manual testing.

---

## Phase 4: Related Indicators + Polish

**Goal:** Cross-tab visibility. Highlight animations. Keyboard navigation. UX polish.

**Demo narrative:** "Click '2 configs, 1 service' to see exactly what this package owns."

### Task 4.1: Add related-artifacts indicator on package rows

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`
- Modify: `src/yoinkc/templates/architect/_css.html.j2`

- [ ] **Step 1: Add related indicator to package rows**

  In `renderPackagesPanel()`, for each package that has dependents, show an inline indicator:

  ```javascript
  // After the package name and move buttons:
  var deps = pkgDependents[nvraToName(pkg)] || [];
  if (deps.length > 0) {
    var configCount = deps.filter(function(d) { return d.type === 'config'; }).length;
    var serviceCount = deps.filter(function(d) { return d.type === 'service'; }).length;
    var parts = [];
    if (configCount > 0) parts.push(configCount + ' config' + (configCount !== 1 ? 's' : ''));
    if (serviceCount > 0) parts.push(serviceCount + ' service' + (serviceCount !== 1 ? 's' : ''));
    html += '<button class="related-indicator" onclick="showRelated(\'' + nvraToName(pkg) + '\')">';
    html += parts.join(', ');
    html += '</button>';
  }
  ```

- [ ] **Step 2: Add "Owned by" indicator on config/service rows**

  In `renderConfigsPanel()` and `renderServicesPanel()`, for owned artifacts:

  ```javascript
  if (config.owner_package) {
    html += '<span class="owned-by-indicator" onclick="showOwner(\'' + config.owner_package + '\')">';
    html += 'Owned by ' + config.owner_package;
    html += '</span>';
  }
  ```

- [ ] **Step 3: Implement click-to-navigate**

  ```javascript
  function showRelated(packageName) {
    // Switch to Configs tab and highlight owned configs
    var layer = getLayer(selectedLayer);
    var ownedConfigs = layer.configs.filter(function(c) {
      return c.owner_package === packageName;
    });
    if (ownedConfigs.length > 0) {
      switchTab('configs');
      highlightArtifacts(ownedConfigs.map(function(c) { return c.path; }));
    }
  }

  function showOwner(packageName) {
    switchTab('packages');
    var layer = getLayer(selectedLayer);
    var pkg = layer.packages.find(function(p) { return nvraToName(p) === packageName; });
    if (pkg) highlightArtifacts([pkg]);
  }
  ```

**Commit:** `feat(architect): add related-artifacts indicators with cross-tab navigation`

---

### Task 4.2: Implement highlight-fade animation

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`
- Modify: `src/yoinkc/templates/architect/_css.html.j2`

- [ ] **Step 1: Add highlight class and animation**

  ```css
  @keyframes highlight-fade {
    from { background-color: var(--pf-v6-global--palette--gold-50); }
    to { background-color: transparent; }
  }
  .artifact-highlight {
    animation: highlight-fade 2s ease-out;
  }
  ```

- [ ] **Step 2: Implement `highlightArtifacts()` function**

  ```javascript
  function highlightArtifacts(ids) {
    // Wait for re-render, then find and highlight matching rows
    requestAnimationFrame(function() {
      ids.forEach(function(id) {
        var row = document.querySelector('[data-artifact-id="' + CSS.escape(id) + '"]');
        if (row) {
          row.classList.add('artifact-highlight');
          row.scrollIntoView({behavior: 'smooth', block: 'nearest'});
          row.addEventListener('animationend', function() {
            row.classList.remove('artifact-highlight');
          }, {once: true});
        }
      });
    });
  }
  ```

  Add `data-artifact-id` attributes to all artifact rows in the panel renderers.

**Commit:** `feat(architect): add highlight-fade animation for cross-tab navigation`

---

### Task 4.3: Add keyboard navigation

**Files:**
- Modify: `src/yoinkc/templates/architect/_js.html.j2`

- [ ] **Step 1: Tab switching with arrow keys**

  Per the UX spec section 6:

  ```javascript
  document.getElementById('artifact-tabs').addEventListener('keydown', function(e) {
    var tabs = ['packages', 'configs', 'services', 'firewall', 'quadlets', 'users-groups', 'sysctls'];
    var currentIdx = tabs.indexOf(selectedTab);
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      switchTab(tabs[(currentIdx + 1) % tabs.length]);
      document.getElementById('tab-' + selectedTab).focus();
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      switchTab(tabs[(currentIdx - 1 + tabs.length) % tabs.length]);
      document.getElementById('tab-' + selectedTab).focus();
    }
  });
  ```

- [ ] **Step 2: Within-tab row navigation with arrow keys**

  ```javascript
  // In each panel: Up/Down arrow moves focus between rows
  // Enter on a focused row opens the move/copy menu
  ```

- [ ] **Step 3: Focus management on move/copy**

  After a move/copy operation completes and the UI re-renders:
  - If confirmation strip was shown, focus moves to the next row
  - If reverse prompt was shown, focus returns to the row that triggered it

- [ ] **Step 4: Escape key handling**

  - In confirmation strip: Escape cancels (calls `cancelConfirm()`)
  - In reverse prompt: Escape cancels (calls `reverseCancel()`)
  - In tab navigation: Escape does nothing (stop propagation to drawer)

**Commit:** `feat(architect): add keyboard navigation for tabs and artifact rows`

---

### Task 4.4: Polish and cleanup

**Files:**
- Modify: `src/yoinkc/templates/architect/_css.html.j2`
- Modify: `src/yoinkc/templates/architect/_js.html.j2`

- [ ] **Step 1: Add "Back to [tab]" breadcrumb**

  After clicking a related indicator to navigate to another tab, show a small breadcrumb link to return to the source tab:

  ```javascript
  var previousTab = null;

  function showRelated(packageName) {
    previousTab = selectedTab;
    switchTab('configs');
    // ... highlight logic ...
  }

  // Render breadcrumb if previousTab is set:
  // "< Back to Packages"
  ```

- [ ] **Step 2: Visual polish**

  - Badge hover states
  - Consistent spacing between tab panels
  - Empty state messaging: "No configs in this layer" instead of blank panel
  - Loading state during move/copy operations (disable buttons, show spinner)

- [ ] **Step 3: Clean up any Phase 1-3 TODO comments**

  Search for TODO, FIXME, and temporary Phase restriction comments. Remove or resolve them.

**Commit:** `style(architect): polish UI, empty states, and breadcrumb navigation`

---

### Quality Gate: Phase 4

**Invariants:**
- Related indicators show correct counts (match `pkgDependents` index)
- Click-to-navigate switches tab and highlights correct rows
- Highlight fades after 2s
- Keyboard navigation: Left/Right cycles tabs, Up/Down cycles rows
- Escape cancels confirmation strip and reverse prompt
- "Back to [tab]" breadcrumb works

**Edge cases:**
- Package with 0 dependents: no related indicator shown
- Package in base layer with dependents spread across derived layers: indicator shows count for current layer only
- Navigate to a tab with 0 items: empty state shown, no crash
- Rapid tab switching: no stale highlights

**Regression risks:**
- Focus management changes could break existing Escape handling in drawer
- Animation CSS could conflict with PF6 defaults

**Quality gate:** Full manual walkthrough. All automated tests pass. Demo narrative complete: "Click '2 configs, 1 service' to see exactly what this package owns."

---

## Summary: Commit Sequence

| # | Commit Message | Phase |
|---|---------------|-------|
| 1 | `feat(inspect): populate ConfigFileEntry.package via rpm -qf on main path` | 0 |
| 2 | `feat(driftify): add multi-artifact data to architect fixture profiles` | 0 |
| 3 | `feat(architect): add multi-artifact input dataclasses` | 1 |
| 4 | `feat(architect): expand FleetInput and Layer for multi-artifact types` | 1 |
| 5 | `feat(architect): multi-artifact decomposition with prevalence and parent-follows rules` | 1 |
| 6 | `feat(architect): expand loader to extract all artifact types from snapshots` | 1 |
| 7 | `feat(architect): expand topology JSON serialization for all artifact types` | 1 |
| 8 | `refactor(architect): generalize server handler for multi-artifact operations` | 1 |
| 9 | `feat(architect): add 7-tab drawer UI with read-only artifact panels` | 1 |
| 10 | `feat(architect): implement move_artifact and copy_artifact for all types` | 2 |
| 11 | `feat(architect): expand server API for multi-type move/copy operations` | 2 |
| 12 | `feat(architect): enable move/copy buttons on independent artifact tabs` | 2 |
| 13 | `feat(architect): create render adapter for Containerfile export` | 2 |
| 14 | `feat(architect): integrate render adapter into export pipeline` | 2 |
| 15 | `feat(architect): enable tied-change moves for packages and owned artifacts` | 3 |
| 16 | `feat(architect): build package dependents index for tied changes` | 3 |
| 17 | `feat(architect): implement confirmation strip for package tied changes` | 3 |
| 18 | `feat(architect): implement reverse prompt for owned config/service moves` | 3 |
| 19 | `test(architect): add tied-change move/copy tests` | 3 |
| 20 | `feat(architect): add related-artifacts indicators with cross-tab navigation` | 4 |
| 21 | `feat(architect): add highlight-fade animation for cross-tab navigation` | 4 |
| 22 | `feat(architect): add keyboard navigation for tabs and artifact rows` | 4 |
| 23 | `style(architect): polish UI, empty states, and breadcrumb navigation` | 4 |

---

## Cross-Phase Test File Summary

| File | Created/Modified | Tests |
|------|-----------------|-------|
| `tests/inspectors/test_config.py` | Modified | rpm -qf enrichment |
| `tests/architect/test_analyzer.py` | Modified | Updated for new dataclass shapes |
| `tests/architect/test_multi_artifact.py` | Created | Decomposition, prevalence, parent-follows, variants, export class |
| `tests/architect/test_move_copy.py` | Created | All-type move/copy, tied changes, dependents, broadcast |
| `tests/architect/test_render_adapter.py` | Created | Adapter contract, export safety boundary, Containerfile output |
| `tests/architect/test_server.py` | Modified | Multi-type API, backward compat, Phase restrictions |
| `tests/e2e/generate-fixtures.py` | Modified | Multi-artifact fixture generation |
