# Containerfile Renderer Split — Design Spec

**Date:** 2026-03-15
**Status:** Approved
**Scope:** Split `renderers/containerfile.py` (1,293 lines) into a package of
domain-specific modules mirroring the inspectors structure.

## Motivation

`containerfile.py` is the largest source file in yoinkc. It renders every
Containerfile section — packages, services, config, firewall, SELinux, etc. —
in a single 730-line function (`_render_containerfile_content`) plus ~550 lines
of helpers. Cross-stream targeting (next on the backlog) will add conditional
logic per target distro, making this harder to maintain. Splitting by domain
now creates clean seams for that work.

## Approach

Convert `renderers/containerfile.py` into a `renderers/containerfile/` package.
Each domain gets its own module mirroring the inspectors package structure.
Section modules return `list[str]` (Containerfile lines) rather than mutating a
shared list, making each section independently testable.

## Package Layout

```
renderers/containerfile/
├── __init__.py          # re-exports render()
├── _core.py             # orchestrator: render(), _render_containerfile_content()
├── _helpers.py          # shared utilities (6 functions + associated constants)
├── _config_tree.py      # file-writing: write_config_tree, config_copy_roots,
│                        #   config_inventory_comment
├── packages.py          # build stage, FROM, repos, GPG keys, dnf install
├── services.py          # systemctl enable, drop-in overrides
├── config.py            # consolidated COPY, CA trust store, diff comments
├── scheduled_tasks.py   # timers, cron conversion, at jobs
├── non_rpm_software.py  # pip (pure), go binaries, standalone installs
├── containers.py        # quadlet files, container workloads
├── users_groups.py      # sysusers/useradd/blueprint/kickstart strategies
├── kernel_boot.py       # kargs, kernel modules, sysctl
├── selinux.py           # SELinux booleans, modules, custom policy
└── network.py           # firewall (zones + direct rules) + NM/kickstart note
```

### Design Decisions

- **Firewall goes in `network.py`** — firewall data lives under
  `snapshot.network` in the schema, matching the inspectors structure. The
  firewall section is ~14 lines of comments.
- **No `storage.py`** — there is no Containerfile storage section today
  (storage is config-tree only).
- **Multi-stage build detection** (pip C-extension scan) stays in `_core.py` —
  it is structural, affecting the FROM line and stage ordering, not
  domain-specific. The inline pip classification logic (currently L563-576) is
  extracted into a small `_classify_pip(snapshot)` helper within `_core.py`.
- **`tmpfiles.d` and `validate bootc compatibility`** stay in `_core.py` as
  small epilogue blocks.

## Module Interfaces

Each domain module exports a single function:

```python
def section_lines(snapshot: InspectionSnapshot, **kwargs) -> list[str]:
    """Return Containerfile lines for this domain, or [] if nothing to emit."""
```

### Extra Arguments by Module

| Module              | Extra args                                          | Why                                          |
|---------------------|-----------------------------------------------------|----------------------------------------------|
| `packages.py`       | `base: str`, `c_ext_pip: list`, `needs_multistage`  | FROM line, builder stage, pip pre-install    |
| `config.py`         | `output_dir: Path`, `dhcp_paths: set`               | reads config tree to determine COPY roots    |
| `non_rpm_software`  | `pure_pip: list`, `needs_multistage: bool`          | pip install lines differ by stage type       |
| `network.py`        | `firewall_only: bool`                               | firewall vs network/kickstart note           |

All other modules (`services`, `scheduled_tasks`, `containers`, `users_groups`,
`kernel_boot`, `selinux`) need only `snapshot`.

### `_helpers.py` Exports

| Function                    | Used by                |
|-----------------------------|------------------------|
| `_summarise_diff`           | `_config_tree.py`      |
| `_sanitize_shell_value`     | `packages.py`, `services.py`, `kernel_boot.py`, `selinux.py` |
| `_base_image_from_snapshot` | `_core.py`             |
| `_dhcp_connection_paths`    | `_core.py`             |
| `_is_bootloader_karg`       | `kernel_boot.py`       |
| `_operator_kargs`           | `kernel_boot.py`       |

Module-level constants travel with their associated functions:
- `_SHELL_UNSAFE_RE` → `_helpers.py` (used by `_sanitize_shell_value`)
- `_KARGS_BOOTLOADER_EXACT`, `_KARGS_BOOTLOADER_PREFIXES` → `_helpers.py`
  (used by `_is_bootloader_karg`)

### `_config_tree.py` Exports

| Function                   | Used by      |
|----------------------------|--------------|
| `write_config_tree`        | `_core.py`   |
| `config_copy_roots`        | `config.py`  |
| `config_inventory_comment` | `config.py`  |

`_config_tree.py` imports `_QUADLET_PREFIX` from `renderers._triage` (the
existing cross-module dependency) and `ConfigFileKind` from the schema.

## Orchestrator Design

`_core.py` contains a slimmed-down `_render_containerfile_content()` (~60
lines) that:

1. **Preps** shared context — base image, pip classification, DHCP paths
2. **Collects** lines by calling each section module in layer order
3. **Appends** small epilogue blocks (tmpfiles.d, bootc validation)

```python
def _render_containerfile_content(snapshot: InspectionSnapshot, output_dir: Path) -> str:
    base = _base_image_from_snapshot(snapshot)
    c_ext_pip, pure_pip = _classify_pip(snapshot)
    needs_multistage = bool(c_ext_pip)
    dhcp_paths = _dhcp_connection_paths(snapshot)

    lines: list[str] = []

    # Layer order matches design doc for cache efficiency
    lines += packages.section_lines(snapshot, base=base, c_ext_pip=c_ext_pip,
                                     needs_multistage=needs_multistage)
    lines += services.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=True)
    lines += scheduled_tasks.section_lines(snapshot)
    lines += config.section_lines(snapshot, output_dir=output_dir, dhcp_paths=dhcp_paths)
    lines += non_rpm_software.section_lines(snapshot, pure_pip=pure_pip,
                                             needs_multistage=needs_multistage)
    lines += containers.section_lines(snapshot)
    lines += users_groups.section_lines(snapshot)
    lines += kernel_boot.section_lines(snapshot)
    lines += selinux.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=False)

    # Epilogue
    lines += _tmpfiles_lines()
    lines += _validate_lines()

    return "\n".join(lines)
```

`render()` is unchanged: calls `write_config_tree()` then
`_render_containerfile_content()`, writes the result.

`__init__.py` is:
```python
from ._core import render
```

### `network.py` Called Twice

`network.section_lines()` is called twice — once with `firewall_only=True`
(early in layer order, between services and scheduled tasks) and once with
`firewall_only=False` (end, for the network/kickstart note). This matches the
current code where firewall and network are separated by ~400 lines.

## Backward Compatibility

Zero breaking changes:

- `from .containerfile import render as render_containerfile` in
  `renderers/__init__.py` continues to work — the package `__init__.py`
  re-exports `render()`.
- The `render()` signature is unchanged: `(snapshot, env, output_dir) -> None`.
- All output is byte-identical — pure structural refactor, no behavioral
  changes.

## Testing Impact

- **`test_renderer_outputs.py`** tests rendered output files (Containerfile
  content, config tree). Since output doesn't change, these tests are the
  primary correctness verification.
- **`test_plan_items.py`** tests internal logic (cron conversion, sanitize
  shell value, user strategies). One known import to update:
  `test_plan_items.py` L865 imports `_sanitize_shell_value` from
  `yoinkc.renderers.containerfile` — after the split this becomes
  `yoinkc.renderers.containerfile._helpers`. Any other internal imports
  discovered during implementation follow the same pattern.
- **No new tests** needed for the refactor — the existing suite is the
  correctness check.

## Migration Strategy

Single atomic commit:

1. Delete `renderers/containerfile.py`
2. Create `renderers/containerfile/` package with all modules
3. Update any test imports that reference moved internal functions

Splitting across commits would leave broken intermediate states.
