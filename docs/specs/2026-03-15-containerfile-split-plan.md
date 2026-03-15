# Containerfile Renderer Split — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development
> (if subagents available) or superpowers:executing-plans to implement this
> plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `renderers/containerfile.py` (1,293 lines) into a
`renderers/containerfile/` package of domain-specific modules.

**Architecture:** Pure structural refactor. Each domain section is extracted
into its own module exporting `section_lines(snapshot, **kwargs) -> list[str]`.
An orchestrator in `_core.py` calls sections in layer order and concatenates
results. Zero behavioral changes — existing test suite is the correctness
check.

**Tech Stack:** Python 3, pytest

**Spec:** `docs/specs/2026-03-15-containerfile-split-design.md`

---

## Extraction Pattern

Every domain module follows the same transformation. The original code mutates
a shared `lines` list via `lines.append(...)`. Each extracted module instead:

1. Creates a local `lines: list[str] = []`
2. Contains the same append logic (indentation adjusted)
3. Returns `lines`

Imports needed per module vary — check which schema types and helpers each
section references. All section modules import `InspectionSnapshot` from
`...schema`.

## Source Line Map

Reference for the original file `src/yoinkc/renderers/containerfile.py`.
Line ranges include the preamble (variable setup, guard conditions) that
precedes each `# ===` section marker — not just the marker itself.

| Lines     | Content                          | Destination          |
|-----------|----------------------------------|----------------------|
| 1-14      | Module docstring, imports        | split across modules |
| 15-62     | `_summarise_diff`                | `_helpers.py`        |
| 63-77     | `_SHELL_UNSAFE_RE`, `_sanitize_shell_value` | `_helpers.py` |
| 78-105    | `_KARGS_BOOTLOADER_*` constants  | `_helpers.py`        |
| 106-119   | `_is_bootloader_karg`            | `_helpers.py`        |
| 120-135   | `_operator_kargs`                | `_helpers.py`        |
| 136-141   | `_base_image_from_snapshot`      | `_helpers.py`        |
| 142-154   | `_dhcp_connection_paths`         | `_helpers.py`        |
| 155-411   | `_write_config_tree`             | `_config_tree.py`    |
| 412-430   | `_config_copy_roots`             | `_config_tree.py`    |
| 431-551   | `_config_inventory_comment`      | `_config_tree.py`    |
| 552-576   | `_render_containerfile_content` signature + pip classify | `_core.py` |
| 579-631   | Build stage + FROM + pip pre-install | `packages.py`    |
| 632-725   | Repos, GPG keys, dnf install     | `packages.py`        |
| 706-797   | Service enable/disable + drop-ins | `services.py`       |
| 793-811   | Firewall comment block           | `network.py`         |
| 809-851   | Scheduled tasks / timers         | `scheduled_tasks.py` |
| 847-881   | Config COPY + CA trust store     | `config.py`          |
| 882-1007  | Non-RPM software (pip, go, etc.) | `non_rpm_software.py`|
| 1008-1021 | Container workloads / quadlets   | `containers.py`      |
| 1023-1117 | Users and groups (4 strategies)  | `users_groups.py`    |
| 1118-1165 | Kernel config / kargs            | `kernel_boot.py`     |
| 1167-1215 | SELinux customizations           | `selinux.py`         |
| 1216-1268 | Network / kickstart note         | `network.py`         |
| 1270-1276 | tmpfiles.d epilogue              | `_core.py`           |
| 1277-1282 | Validate bootc compatibility     | `_core.py`           |
| 1283-1293 | `render()`                       | `_core.py`           |

**Note:** Domain sections in the original share a single `lines` list, so
boundary lines overlap slightly (e.g., one section's trailing `lines.append("")`
is adjacent to the next section's preamble). The implementer should include each
section's guard condition and variable setup, then end at the trailing blank
line append. When in doubt, read the original and follow the `# N.` numbered
comment markers.

## Test Import Map

| File                        | Line | Import                                          | Action         |
|-----------------------------|------|-------------------------------------------------|----------------|
| `test_plan_items.py`        | 49   | `from yoinkc.renderers.containerfile import render as render_containerfile` | No change (re-exported) |
| `test_plan_items.py`        | 865  | `from yoinkc.renderers.containerfile import _sanitize_shell_value` | Change to `from yoinkc.renderers.containerfile._helpers import _sanitize_shell_value` |
| `test_plan_items.py`        | 921  | `from yoinkc.renderers.containerfile import render` | No change |
| `test_plan_items.py`        | 945  | `from yoinkc.renderers.containerfile import render` | No change |
| `test_renderer_outputs.py`  | 27   | `from yoinkc.renderers.containerfile import render as render_containerfile` | No change |
| `test_renderer_outputs.py`  | 1135, 1184, 1343, 1368, 1396 | same | No change |

---

## Chunk 1: Infrastructure and Domain Modules

### Task 1: Create package directory

**Files:**
- Create: `src/yoinkc/renderers/containerfile/` (directory)

- [ ] **Step 1: Create the package directory**

```bash
mkdir -p src/yoinkc/renderers/containerfile
```

---

### Task 2: Create `_helpers.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/_helpers.py`
- Source: `containerfile.py` L15-154

- [ ] **Step 1: Create `_helpers.py`**

Extract these functions and their associated constants into the new module:

```python
"""Shared helpers for the containerfile renderer package."""

import re
from pathlib import Path
from typing import List, Optional

from ...schema import InspectionSnapshot
```

Functions to extract (preserve exact implementation):
- `_summarise_diff(diff_text: str) -> List[str]` (L15-62)
- `_SHELL_UNSAFE_RE` constant (L63)
- `_sanitize_shell_value(value: str, context: str) -> Optional[str]` (L66-77)
- `_KARGS_BOOTLOADER_EXACT` frozenset (L84-87)
- `_KARGS_BOOTLOADER_PREFIXES` tuple (L89-105)
- `_is_bootloader_karg(karg: str) -> bool` (L106-119)
- `_operator_kargs(cmdline: str) -> List[str]` (L120-135)
- `_base_image_from_snapshot(snapshot: InspectionSnapshot) -> str` (L136-141)
- `_dhcp_connection_paths(snapshot: InspectionSnapshot) -> set` (L142-154)

No transformation needed — these are standalone functions, copy as-is.

---

### Task 3: Create `_config_tree.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/_config_tree.py`
- Source: `containerfile.py` L155-551

- [ ] **Step 1: Create `_config_tree.py`**

```python
"""Config tree file-writing and inventory comment generation."""

from pathlib import Path
from typing import List

from ...schema import ConfigFileKind, InspectionSnapshot
from .._triage import _QUADLET_PREFIX
from ._helpers import _summarise_diff
```

Functions to extract (preserve exact implementation):
- `write_config_tree(snapshot, output_dir) -> None` (L155-411) — rename from
  `_write_config_tree`, drop leading underscore since it's now a module-level
  public function within the package
- `config_copy_roots(config_dir: Path) -> list` (L412-430) — rename from
  `_config_copy_roots`
- `config_inventory_comment(snapshot, dhcp_paths) -> list` (L431-551) — rename
  from `_config_inventory_comment`

Note: `_config_inventory_comment` calls `_summarise_diff` — import it from
`._helpers`.

---

### Task 4: Create `packages.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/packages.py`
- Source: `containerfile.py` L579-725

- [ ] **Step 1: Create `packages.py`**

```python
"""Containerfile section: packages (repos, GPG keys, dnf install)."""

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    base: str,
    c_ext_pip: list,
    needs_multistage: bool,
) -> list[str]:
    """Return Containerfile lines for build stage, FROM, repos, and packages."""
    lines: list[str] = []
    # ... extract L579-725, adjusting indentation
    return lines
```

Extraction notes:
- The build stage block (L579-587) and FROM / base image (L588-613) come first
- Pip pre-install block (L614-631) follows
- Repos section (L632-656) including GPG keys
- Package installation (L657-725)
- Variables `base`, `c_ext_pip`, `needs_multistage` are passed as keyword args
  (they were computed in the orchestrator's prep phase)
- `_sanitize_shell_value` is used at L663 for dnf package name sanitization

---

### Task 5: Create `services.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/services.py`
- Source: `containerfile.py` L706-797

- [ ] **Step 1: Create `services.py`**

```python
"""Containerfile section: service enablement and systemd drop-in overrides."""

from pathlib import Path

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for services and drop-ins."""
    lines: list[str] = []
    # ... extract L706-797 (sections 3 + 3b), adjusting indentation
    return lines
```

Extraction notes:
- Includes the `_config_tree_units` set computation (L706-725)
- Includes `_unit_installable` local helper (it's a closure, keep as-is)
- Uses `_sanitize_shell_value` at L751, L753
- Drop-in overrides block (L779-797) included

---

### Task 6: Create `network.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/network.py`
- Source: `containerfile.py` L793-811 (firewall) and L1216-1268 (network)

- [ ] **Step 1: Create `network.py`**

```python
"""Containerfile section: firewall configuration and network/kickstart notes."""

from ...schema import InspectionSnapshot


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    firewall_only: bool,
) -> list[str]:
    """Return firewall or network lines depending on firewall_only flag."""
    if firewall_only:
        return _firewall_lines(snapshot)
    return _network_lines(snapshot)


def _firewall_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Firewall comment block."""
    lines: list[str] = []
    # ... extract L793-811 (section 4)
    # Include fw_zones/fw_direct filtering from L795-796
    return lines


def _network_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Network / kickstart note."""
    lines: list[str] = []
    # ... extract L1216-1268 (section 12)
    return lines
```

Extraction notes:
- Called twice by orchestrator: once with `firewall_only=True` (early), once
  with `firewall_only=False` (late)
- `dhcp_paths` is NOT needed — the network section filters DHCP vs static
  connections directly from `snapshot.network.connections` by checking
  `c.method == "dhcp"` / `c.method == "static"` (L1218-1219)
- Firewall section references `snapshot.network.firewall_zones` and
  `snapshot.network.firewall_direct_rules` — these are computed before the
  section in the original (around L793-797). Extract that computation into
  `_firewall_lines`.

---

### Task 7: Create `scheduled_tasks.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/scheduled_tasks.py`
- Source: `containerfile.py` L809-851

- [ ] **Step 1: Create `scheduled_tasks.py`**

```python
"""Containerfile section: scheduled tasks (timers, cron, at jobs)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for scheduled tasks."""
    lines: list[str] = []
    # ... extract L809-851 (section 5)
    return lines
```

---

### Task 8: Create `config.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/config.py`
- Source: `containerfile.py` L847-881

- [ ] **Step 1: Create `config.py`**

```python
"""Containerfile section: configuration files (consolidated COPY)."""

from pathlib import Path

from ...schema import InspectionSnapshot
from ._config_tree import config_copy_roots, config_inventory_comment
from ._helpers import _dhcp_connection_paths


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    output_dir: Path,
    dhcp_paths: set,
) -> list[str]:
    """Return Containerfile lines for config COPY and CA trust store."""
    lines: list[str] = []
    # ... extract L847-881 (section 6 + CA trust)
    return lines
```

Extraction notes:
- Calls `config_inventory_comment(snapshot, dhcp_paths)` and
  `config_copy_roots(output_dir / "config")`
- Includes the diff comment block and consolidated COPY line
- Includes CA trust store block (L875-881)

---

### Task 9: Create `non_rpm_software.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/non_rpm_software.py`
- Source: `containerfile.py` L882-1011

- [ ] **Step 1: Create `non_rpm_software.py`**

```python
"""Containerfile section: non-RPM software (pip, go, standalone)."""

from ...schema import InspectionSnapshot


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    pure_pip: list,
    needs_multistage: bool,
) -> list[str]:
    """Return Containerfile lines for non-RPM software."""
    lines: list[str] = []
    # ... extract L882-1011 (section 7)
    return lines
```

Extraction notes:
- Largest domain section (~130 lines)
- Handles pip dist-info, go binaries, standalone binaries, and unknown
  provenance items
- `pure_pip` and `needs_multistage` passed from orchestrator (computed during
  pip classification)

---

### Task 10: Create `containers.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/containers.py`
- Source: `containerfile.py` L1008-1021

- [ ] **Step 1: Create `containers.py`**

```python
"""Containerfile section: container workloads (quadlets, compose)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for container workloads."""
    lines: list[str] = []
    # ... extract L1008-1021 (section 8)
    # Include the included_quadlets/included_compose filtering (L1008-1010)
    # before the section marker
    return lines
```

---

### Task 11: Create `users_groups.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/users_groups.py`
- Source: `containerfile.py` L1023-1117

- [ ] **Step 1: Create `users_groups.py`**

```python
"""Containerfile section: users and groups (strategy-aware rendering)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for users and groups."""
    lines: list[str] = []
    # ... extract L1023-1117 (section 9)
    # Include ug/included_users setup (L1023-1025) before section marker
    return lines
```

Extraction notes:
- Includes `ug = snapshot.users_groups` and `_included_users` filtering
  (L1023-1025) before the section marker
- Contains 4 strategy blocks: sysusers (L1035-1042), useradd (L1043-1082),
  blueprint (L1083-1088), kickstart (L1089-1117)
- Does NOT use `_sanitize_shell_value` (review corrected this)

---

### Task 12: Create `kernel_boot.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/kernel_boot.py`
- Source: `containerfile.py` L1118-1165

- [ ] **Step 1: Create `kernel_boot.py`**

```python
"""Containerfile section: kernel configuration and boot arguments."""

from ...schema import InspectionSnapshot
from ._helpers import _is_bootloader_karg, _operator_kargs, _sanitize_shell_value


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for kernel config."""
    lines: list[str] = []
    # ... extract L1118-1165 (section 10)
    # Include kb/has_kernel guard (L1118-1125) before section marker
    return lines
```

Extraction notes:
- Includes `kb = snapshot.kernel_boot` and `has_kernel` guard (L1118-1125)
  before the section marker
- Uses `_is_bootloader_karg`, `_operator_kargs`, and `_sanitize_shell_value`
  from helpers
- Includes kargs.d drop-in block (L1135-1160) and tuned-adm (L1160-1165)

---

### Task 13: Create `selinux.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/selinux.py`
- Source: `containerfile.py` L1167-1215

- [ ] **Step 1: Create `selinux.py`**

```python
"""Containerfile section: SELinux customizations."""

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for SELinux customizations."""
    lines: list[str] = []
    # ... extract L1167-1215 (section 11)
    # Include has_selinux guard (L1167-1172) before section marker
    return lines
```

Extraction notes:
- Includes `has_selinux` guard computation (L1167-1172) before the section
  marker at L1174
- Uses `_sanitize_shell_value` extensively (L1186-1206) for setsebool,
  semanage fcontext, semanage port
- ~50 lines

---

### Task 14: Create `_core.py` (orchestrator)

**Files:**
- Create: `src/yoinkc/renderers/containerfile/_core.py`
- Source: `containerfile.py` L552-576 (pip classify), L1271-1293 (epilogue +
  render)

- [ ] **Step 1: Create `_core.py`**

```python
"""Containerfile renderer orchestrator."""

from pathlib import Path

from jinja2 import Environment

from ...schema import InspectionSnapshot
from ._helpers import _base_image_from_snapshot, _dhcp_connection_paths
from ._config_tree import write_config_tree
from . import (
    config,
    containers,
    kernel_boot,
    network,
    non_rpm_software,
    packages,
    scheduled_tasks,
    selinux,
    services,
    users_groups,
)


def _classify_pip(snapshot: InspectionSnapshot) -> tuple[list, list]:
    """Classify pip packages into C-extension and pure lists."""
    c_ext_pip: list = []
    pure_pip: list = []
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        for item in snapshot.non_rpm_software.items:
            if not item.include:
                continue
            if item.method == "pip dist-info" and item.version:
                if item.has_c_extensions:
                    c_ext_pip.append((item.name, item.version))
                else:
                    pure_pip.append((item.name, item.version))
    return c_ext_pip, pure_pip


def _tmpfiles_lines() -> list[str]:
    """Epilogue: tmpfiles.d comment block."""
    return [
        "# === tmpfiles.d for /var structure ===",
        "# Directories created on every boot; /var is not updated by bootc after bootstrap.",
        "# tmpfiles.d/yoinkc-var.conf included in COPY config/etc/ above",
        "",
    ]


def _validate_lines() -> list[str]:
    """Epilogue: bootc validation."""
    return [
        "# === Validate bootc compatibility ===",
        "RUN bootc container lint",
    ]


def _render_containerfile_content(
    snapshot: InspectionSnapshot, output_dir: Path
) -> str:
    """Build Containerfile content from snapshot."""
    base = _base_image_from_snapshot(snapshot)
    c_ext_pip, pure_pip = _classify_pip(snapshot)
    needs_multistage = bool(c_ext_pip)
    dhcp_paths = _dhcp_connection_paths(snapshot)

    lines: list[str] = []

    # Layer order matches design doc for cache efficiency
    lines += packages.section_lines(
        snapshot, base=base, c_ext_pip=c_ext_pip,
        needs_multistage=needs_multistage,
    )
    lines += services.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=True)
    lines += scheduled_tasks.section_lines(snapshot)
    lines += config.section_lines(
        snapshot, output_dir=output_dir, dhcp_paths=dhcp_paths,
    )
    lines += non_rpm_software.section_lines(
        snapshot, pure_pip=pure_pip, needs_multistage=needs_multistage,
    )
    lines += containers.section_lines(snapshot)
    lines += users_groups.section_lines(snapshot)
    lines += kernel_boot.section_lines(snapshot)
    lines += selinux.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=False)

    # Epilogue
    lines += _tmpfiles_lines()
    lines += _validate_lines()

    return "\n".join(lines)


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    """Write Containerfile and config/ tree to output_dir."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    write_config_tree(snapshot, output_dir)
    content = _render_containerfile_content(snapshot, output_dir)
    (output_dir / "Containerfile").write_text(content)
```

**Important:** Check the original `render()` (L1283-1293) to see if the
`"\n".join(lines)` includes a trailing newline. The original returns
`"\n".join(lines)` without `+ "\n"` at L1282. Match this exactly.

---

### Task 15: Create `__init__.py`

**Files:**
- Create: `src/yoinkc/renderers/containerfile/__init__.py`

- [ ] **Step 1: Create `__init__.py`**

```python
"""Containerfile renderer: produces Containerfile and config/ tree from snapshot."""

from ._core import render

__all__ = ["render"]
```

---

### Task 16: Delete old monolith

**Files:**
- Delete: `src/yoinkc/renderers/containerfile.py`

- [ ] **Step 1: Verify the old file path**

The old file is at `src/yoinkc/renderers/containerfile.py`. It must be deleted
AFTER the package is created (Python would be confused by both a module and
package with the same name, but the package takes precedence — still, keeping
the dead file is confusing).

```bash
rm src/yoinkc/renderers/containerfile.py
```

---

### Task 17: Update test imports

**Files:**
- Modify: `tests/test_plan_items.py:865`

- [ ] **Step 1: Update `_sanitize_shell_value` import**

Change line 865 from:
```python
from yoinkc.renderers.containerfile import _sanitize_shell_value
```
to:
```python
from yoinkc.renderers.containerfile._helpers import _sanitize_shell_value
```

This is the only import that needs changing. All other test imports use
`render` which is re-exported from `__init__.py`.

---

### Task 18: Run full test suite and commit

- [ ] **Step 1: Run the full test suite**

```bash
cd /path/to/yoinkc
python -m pytest tests/ -v
```

Expected: all tests pass. If any fail, the output diff will show which section
was extracted incorrectly. Fix and re-run.

- [ ] **Step 2: Verify output identity**

Run a quick smoke test to confirm the rendered Containerfile is byte-identical:

```bash
python -m pytest tests/test_renderer_outputs.py -v
```

These tests compare rendered output against expected results. If they pass, the
refactor preserved behavior.

- [ ] **Step 3: Commit**

```bash
git add -A src/yoinkc/renderers/containerfile/ tests/test_plan_items.py
git rm src/yoinkc/renderers/containerfile.py
git commit -m "refactor(renderer): split containerfile.py into domain-specific package

Convert renderers/containerfile.py (1,293 lines) into a renderers/containerfile/
package with 14 modules mirroring the inspectors structure. Each domain section
returns list[str] for independent testability.

No behavioral changes — existing test suite verifies output identity.

Prepares clean seams for cross-stream targeting work.

Assisted-by: Claude <noreply@anthropic.com>"
```
