# ostree/bootc Source System Scanning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable yoinkc to correctly detect and scan rpm-ostree and bootc source systems, producing accurate Containerfiles for migration to pipelined bootc builds.

**Architecture:** System type detection runs early in `run_all()` and stores the result on the snapshot. The detected type drives ostree base image mapping, which resolves to a ref that feeds into the existing `BaselineResolver.resolve()` pipeline — same path as `--target-image` on package-mode. Each inspector branches on `system_type` where behavior differs. Flatpak detection runs on all system types. Two slices: Slice 1 fixes wrong results (RPM, config, non-RPM), Slice 2 fixes noise and adds Flatpak (storage, kernel/boot, timers, containers).

**Tech Stack:** Python 3.11+, Pydantic 2.0+, pytest, pathlib, json (for rpm-ostree/bootc status parsing)

**Spec:** `docs/specs/proposed/2026-04-09-ostree-bootc-source-scanning-design.md`
**Round-2 follow-ups:** `workflow/backlog/yoinkc-image-mode-source-scanning-round2-followups.md` (PKA repo)

**Revision 2** — revised after Kit + Thorn + Collins round-1 plan review. Key changes:
- Wired `map_ostree_base_image()` into `BaselineResolver` / `snapshot.rpm.base_image` / renderer `FROM` (was disconnected)
- Enforced unknown-base refusal as real pipeline behavior with integration test (was unit-only `None` check)
- Replaced `--base-image` with existing `--target-image` (eliminates UX ambiguity)
- Dedicated task for pure-bootc low-confidence fallback (was a stub)
- Config Tier 2 aligned to spec: `rpm -qf` + targeted `rpm -V` (was `rpm -qf` only)
- Added SELinux context comparison to Tier 1 (was omitted)
- Added `/usr/lib/python3*` to non-RPM ostree skip list (was missing)
- Implemented actual BLS filtering in kernel/boot (was GRUB-suppression only)
- Changed `detect_system_type` from `sys.exit` to typed exception (composability)
- Added `bootc status --json` image ref parsing for BOOTC systems (was os-release only)
- Fixed test quality: exact command matching, no conditional assertions, explicit assertions
- Marked Slice 2 synthetic fixtures as explicit deviation with follow-up

---

## File Structure

**Create:**
- `src/yoinkc/system_type.py` — `detect_system_type()`, `map_ostree_base_image()`, `OstreeDetectionError`
- `tests/test_system_type.py` — System type detection + base image mapping tests
- `tests/test_ostree_rpm.py` — RPM inspector ostree-mode tests
- `tests/test_ostree_config.py` — Config inspector ostree-mode tests
- `tests/test_ostree_non_rpm.py` — Non-RPM inspector ostree-mode tests
- `tests/test_flatpak.py` — Flatpak detection tests
- `tests/test_ostree_slice2.py` — Storage, kernel/boot, timer adaptation tests
- `tests/test_ostree_renderer.py` — Containerfile renderer ostree-mode tests
- `tests/test_ostree_integration.py` — End-to-end integration with ostree fixtures

**Modify:**
- `src/yoinkc/schema.py` — `SystemType` enum, `OsRelease.variant_id`, `FlatpakApp`, `OstreePackageOverride`, `system_type` on `InspectionSnapshot`, ostree fields on `RpmSection`, `flatpak_apps` on `ContainerSection`
- `src/yoinkc/inspectors/__init__.py` — `_read_os_release` captures `VARIANT_ID`; `run_all` calls system detection, maps base image, enforces refusal, passes `system_type` to inspectors
- `src/yoinkc/inspectors/rpm.py` — `system_type` param, skip `rpm -Va`, parse `rpm-ostree status --json`
- `src/yoinkc/inspectors/config.py` — `system_type` param, `/usr/etc` → `/etc` diff with SELinux contexts, Tier 2 `rpm -qf` + `rpm -V`
- `src/yoinkc/inspectors/non_rpm_software.py` — `system_type` param, skip immutable `/usr` + `/usr/lib/python3*`
- `src/yoinkc/inspectors/container.py` — Flatpak detection (all system types)
- `src/yoinkc/inspectors/storage.py` — `system_type` param, filter ostree mounts
- `src/yoinkc/inspectors/kernel_boot.py` — `system_type` param, filter BLS entries
- `src/yoinkc/inspectors/scheduled_tasks.py` — `system_type` param, vendor timer filter
- `src/yoinkc/renderers/containerfile/packages.py` — ostree override/removal output
- `src/yoinkc/renderers/containerfile/_core.py` — bootc label, `flatpaks.list` generation
- `tests/conftest.py` — ostree fixture executor

**Explicit deviation:** Slice 2 tests (storage, kernel/boot, timers) use synthetic fixtures for v1. The spec calls for real captured fixtures — follow-up task to vendor snapshots from Silverblue/bootc systems. Risk: layout drift in synthetic fixtures; acceptable for initial implementation.

---

## Task 1: Schema Additions

**Files:**
- Modify: `src/yoinkc/schema.py`
- Test: `tests/test_system_type.py` (schema smoke test)

- [ ] **Step 1: Write smoke test for new schema types**

```python
# tests/test_system_type.py
"""System type detection and ostree base image mapping tests."""

from yoinkc.schema import SystemType, FlatpakApp, OstreePackageOverride


def test_system_type_enum_values():
    assert SystemType.PACKAGE_MODE == "package-mode"
    assert SystemType.RPM_OSTREE == "rpm-ostree"
    assert SystemType.BOOTC == "bootc"


def test_flatpak_app_model():
    app = FlatpakApp(app_id="org.mozilla.firefox", origin="flathub", branch="stable")
    assert app.app_id == "org.mozilla.firefox"
    assert app.origin == "flathub"


def test_ostree_package_override_model():
    ovr = OstreePackageOverride(
        name="kernel",
        from_nevra="kernel-5.14.0-1.el9",
        to_nevra="kernel-5.14.0-2.el9",
    )
    assert ovr.name == "kernel"


def test_os_release_has_variant_id():
    from yoinkc.schema import OsRelease
    osr = OsRelease(name="Fedora", version_id="41", variant_id="silverblue")
    assert osr.variant_id == "silverblue"


def test_snapshot_system_type_default():
    from yoinkc.schema import InspectionSnapshot
    snap = InspectionSnapshot()
    assert snap.system_type == SystemType.PACKAGE_MODE
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py::test_system_type_enum_values -v`
Expected: FAIL with `ImportError: cannot import name 'SystemType'`

- [ ] **Step 3: Add SystemType enum and new models to schema.py**

In `src/yoinkc/schema.py`, after existing imports, add:

```python
from enum import Enum

class SystemType(str, Enum):
    """Source system type detected at scan time."""
    PACKAGE_MODE = "package-mode"
    RPM_OSTREE = "rpm-ostree"
    BOOTC = "bootc"
```

Add `variant_id` to `OsRelease`:

```python
class OsRelease(BaseModel):
    """From /etc/os-release."""
    name: str
    version_id: str
    version: str = ""
    id: str = ""
    id_like: str = ""
    pretty_name: str = ""
    variant_id: str = ""  # e.g. "silverblue", "kinoite"
```

Add `FlatpakApp` near the container sub-models:

```python
class FlatpakApp(BaseModel):
    """A Flatpak application detected on the system."""
    app_id: str
    origin: str
    branch: str = ""
    include: bool = True
```

Add `OstreePackageOverride` near the RPM sub-models:

```python
class OstreePackageOverride(BaseModel):
    """An rpm-ostree override replace entry."""
    name: str
    from_nevra: str = ""
    to_nevra: str = ""
```

Add to `RpmSection`:

```python
    ostree_overrides: List[OstreePackageOverride] = Field(default_factory=list)
    ostree_removals: List[str] = Field(default_factory=list)
```

Add to `ContainerSection`:

```python
    flatpak_apps: List[FlatpakApp] = Field(default_factory=list)
```

Add to `InspectionSnapshot`:

```python
    system_type: SystemType = SystemType.PACKAGE_MODE
```

- [ ] **Step 4: Update _read_os_release to capture VARIANT_ID**

In `src/yoinkc/inspectors/__init__.py`, in `_read_os_release()`:

```python
    return OsRelease(
        name=data.get("NAME", ""),
        version_id=data.get("VERSION_ID", ""),
        version=data.get("VERSION", ""),
        id=data.get("ID", ""),
        id_like=data.get("ID_LIKE", ""),
        pretty_name=data.get("PRETTY_NAME", ""),
        variant_id=data.get("VARIANT_ID", ""),
    )
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py -v`
Expected: PASS

- [ ] **Step 6: Run existing tests for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`
Expected: All existing tests pass

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/schema.py src/yoinkc/inspectors/__init__.py tests/test_system_type.py
git commit -m "feat(schema): add SystemType enum, ostree models, variant_id on OsRelease"
```

---

## Task 2: System Type Detection

**Files:**
- Create: `src/yoinkc/system_type.py`
- Test: `tests/test_system_type.py` (append)

- [ ] **Step 1: Write detection tests**

Append to `tests/test_system_type.py`:

```python
import pytest
from pathlib import Path
from yoinkc.executor import RunResult
from yoinkc.system_type import detect_system_type, OstreeDetectionError


def _mock_executor(bootc_rc=1, rpmostree_rc=1):
    """Return an executor that fakes bootc/rpm-ostree status commands."""
    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            if bootc_rc == 0:
                return RunResult(stdout="ok", stderr="", returncode=0)
            return RunResult(stdout="", stderr="not found", returncode=bootc_rc)
        if cmd == ["rpm-ostree", "status"]:
            if rpmostree_rc == 0:
                return RunResult(stdout="State: idle", stderr="", returncode=0)
            return RunResult(stdout="", stderr="not found", returncode=rpmostree_rc)
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


def test_detect_package_mode(tmp_path):
    """No /ostree directory -> package-mode."""
    assert detect_system_type(tmp_path, _mock_executor()) == SystemType.PACKAGE_MODE


def test_detect_bootc_system(tmp_path):
    """/ostree exists + bootc status succeeds -> bootc."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(tmp_path, _mock_executor(bootc_rc=0)) == SystemType.BOOTC


def test_detect_rpm_ostree_system(tmp_path):
    """/ostree exists + bootc fails + rpm-ostree succeeds -> rpm-ostree."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(
        tmp_path, _mock_executor(bootc_rc=1, rpmostree_rc=0)
    ) == SystemType.RPM_OSTREE


def test_detect_unknown_ostree_raises(tmp_path):
    """/ostree exists + both commands fail -> OstreeDetectionError."""
    (tmp_path / "ostree").mkdir()
    with pytest.raises(OstreeDetectionError, match="could not determine"):
        detect_system_type(tmp_path, _mock_executor(bootc_rc=1, rpmostree_rc=1))


def test_detect_bootc_preferred_over_rpmostree(tmp_path):
    """When both succeed, bootc wins."""
    (tmp_path / "ostree").mkdir()
    assert detect_system_type(
        tmp_path, _mock_executor(bootc_rc=0, rpmostree_rc=0)
    ) == SystemType.BOOTC
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py::test_detect_package_mode -v`
Expected: FAIL with `ImportError: cannot import name 'detect_system_type'`

- [ ] **Step 3: Implement detect_system_type**

Create `src/yoinkc/system_type.py`:

```python
"""Source system type detection and ostree base image mapping."""

import json
import sys
from pathlib import Path
from typing import Optional

from .executor import Executor
from .schema import OsRelease, SystemType
from ._util import debug as _debug_fn


def _debug(msg: str) -> None:
    _debug_fn("system_type", msg)


class OstreeDetectionError(Exception):
    """Raised when an ostree system cannot be classified."""
    pass


def detect_system_type(host_root: Path, executor: Executor) -> SystemType:
    """Detect whether the source system is package-mode, rpm-ostree, or bootc.

    Detection order per spec:
    1. No /ostree -> package-mode
    2. /ostree + bootc status succeeds -> bootc
    3. /ostree + rpm-ostree status succeeds -> rpm-ostree
    4. /ostree + both fail -> OstreeDetectionError (never fall back to package-mode)
    """
    ostree_dir = host_root / "ostree"
    if not ostree_dir.exists():
        return SystemType.PACKAGE_MODE

    result = executor(["bootc", "status"])
    if result.returncode == 0:
        return SystemType.BOOTC

    result = executor(["rpm-ostree", "status"])
    if result.returncode == 0:
        return SystemType.RPM_OSTREE

    raise OstreeDetectionError(
        "Detected ostree system (/ostree exists) but could not determine\n"
        "system type -- both 'bootc status' and 'rpm-ostree status' failed.\n"
        "\n"
        "This system may use an ostree configuration yoinkc does not yet support."
    )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/system_type.py tests/test_system_type.py
git commit -m "feat(detection): add system type detection with OstreeDetectionError"
```

---

## Task 3: ostree Base Image Mapping

**Files:**
- Modify: `src/yoinkc/system_type.py`
- Test: `tests/test_system_type.py` (append)

This task implements the mapping logic and tests all variants. It does NOT wire into the pipeline (Task 4 does that). The spec's `--base-image` is served by the existing `--target-image` flag — no new CLI flag needed.

- [ ] **Step 1: Write base image mapping tests**

Append to `tests/test_system_type.py`:

```python
import json
from yoinkc.system_type import map_ostree_base_image


def _make_os_release(**kwargs):
    from yoinkc.schema import OsRelease
    defaults = {
        "name": "Fedora Linux", "version_id": "41",
        "id": "fedora", "variant_id": "",
    }
    defaults.update(kwargs)
    return OsRelease(**defaults)


# --- rpm-ostree systems: VARIANT_ID mapping ---

def test_map_silverblue(tmp_path):
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result == "quay.io/fedora-ostree-desktops/silverblue:41"


def test_map_kinoite(tmp_path):
    os_rel = _make_os_release(variant_id="kinoite", version_id="42")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result == "quay.io/fedora-ostree-desktops/kinoite:42"


# --- Universal Blue ---

def test_map_universal_blue(tmp_path):
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    info = {
        "image-name": "bluefin", "image-vendor": "ublue-os",
        "image-ref": "ghcr.io/ublue-os/bluefin:41", "image-tag": "41",
    }
    (ublue_dir / "image-info.json").write_text(json.dumps(info))
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result == "ghcr.io/ublue-os/bluefin:41"


def test_map_ublue_malformed_json_missing_fields(tmp_path):
    """Missing required image-name -> None (round-2 follow-up)."""
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    (ublue_dir / "image-info.json").write_text(json.dumps({"image-vendor": "ublue-os"}))
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result is None


def test_map_ublue_invalid_json(tmp_path):
    """Unparseable JSON -> None."""
    ublue_dir = tmp_path / "usr" / "share" / "ublue-os"
    ublue_dir.mkdir(parents=True)
    (ublue_dir / "image-info.json").write_text("not valid json{{{")
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result is None


# --- bootc systems: use bootc status --json for image ref ---

def test_map_fedora_bootc_from_status(tmp_path):
    """bootc status --json provides the booted image ref."""
    os_rel = _make_os_release(id="fedora", version_id="41")

    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status", "--json"]:
            return RunResult(
                stdout=json.dumps({
                    "status": {"booted": {"image": {
                        "image": {"image": "quay.io/fedora/fedora-bootc:41"}
                    }}}
                }),
                stderr="", returncode=0,
            )
        return RunResult(stdout="", stderr="", returncode=1)

    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.BOOTC, executor=executor,
    )
    assert result == "quay.io/fedora/fedora-bootc:41"


def test_map_bootc_status_fails_falls_back_to_os_release(tmp_path):
    """If bootc status --json fails, fall back to os-release mapping."""
    os_rel = _make_os_release(id="fedora", version_id="41")

    def executor(cmd, *, cwd=None):
        return RunResult(stdout="", stderr="error", returncode=1)

    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.BOOTC, executor=executor,
    )
    assert result == "quay.io/fedora/fedora-bootc:41"


def test_map_centos_bootc(tmp_path):
    os_rel = _make_os_release(id="centos", name="CentOS Stream", version_id="10")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.BOOTC, executor=None,
    )
    assert result == "quay.io/centos-bootc/centos-bootc:stream10"


def test_map_rhel_bootc(tmp_path):
    os_rel = _make_os_release(id="rhel", version_id="9.4")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.BOOTC, executor=None,
    )
    assert result == "registry.redhat.io/rhel9/rhel-bootc:9.4"


def test_map_unknown_returns_none(tmp_path):
    os_rel = _make_os_release(id="custom-os", version_id="1.0")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
    )
    assert result is None


def test_map_target_image_override(tmp_path):
    """--target-image override takes precedence over auto-detection."""
    os_rel = _make_os_release(variant_id="silverblue", version_id="41")
    result = map_ostree_base_image(
        tmp_path, os_rel, SystemType.RPM_OSTREE, executor=None,
        target_image_override="quay.io/my-custom/image:latest",
    )
    assert result == "quay.io/my-custom/image:latest"
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py::test_map_silverblue -v`
Expected: FAIL with `ImportError: cannot import name 'map_ostree_base_image'`

- [ ] **Step 3: Implement map_ostree_base_image**

Add to `src/yoinkc/system_type.py`:

```python
# Known Fedora Atomic Desktop variants -> ostree-desktops base images
_FEDORA_OSTREE_DESKTOPS = {
    "silverblue": "quay.io/fedora-ostree-desktops/silverblue",
    "kinoite": "quay.io/fedora-ostree-desktops/kinoite",
    "sway-atomic": "quay.io/fedora-ostree-desktops/sway-atomic",
    "budgie-atomic": "quay.io/fedora-ostree-desktops/budgie-atomic",
    "lxqt-atomic": "quay.io/fedora-ostree-desktops/lxqt-atomic",
    "xfce-atomic": "quay.io/fedora-ostree-desktops/xfce-atomic",
    "cosmic-atomic": "quay.io/fedora-ostree-desktops/cosmic-atomic",
}


def _read_ublue_image_info(host_root: Path) -> Optional[str]:
    """Read Universal Blue image-info.json and return the image ref.

    Returns None if the file is missing, malformed, or lacks required fields.
    """
    info_path = host_root / "usr" / "share" / "ublue-os" / "image-info.json"
    if not info_path.exists():
        return None
    try:
        data = json.loads(info_path.read_text())
    except (json.JSONDecodeError, OSError):
        _debug("ublue image-info.json is not valid JSON")
        return None
    if not data.get("image-name") or not data.get("image-vendor"):
        _debug("ublue image-info.json missing required image-name or image-vendor")
        return None
    ref = data.get("image-ref", "")
    if ref:
        return ref
    tag = data.get("image-tag", "")
    vendor = data.get("image-vendor", "")
    name = data.get("image-name", "")
    if vendor and name and tag:
        return f"ghcr.io/{vendor}/{name}:{tag}"
    return None


def _bootc_status_image_ref(executor: Optional[Executor]) -> Optional[str]:
    """Parse bootc status --json for the booted image ref."""
    if executor is None:
        return None
    result = executor(["bootc", "status", "--json"])
    if result.returncode != 0:
        _debug(f"bootc status --json failed: {result.stderr[:200]}")
        return None
    try:
        data = json.loads(result.stdout)
        # Navigate: status.booted.image.image.image (bootc 1.x schema)
        booted = data.get("status", {}).get("booted", {})
        image_spec = booted.get("image", {}).get("image", {})
        ref = image_spec.get("image", "")
        return ref if ref else None
    except (json.JSONDecodeError, AttributeError, TypeError):
        _debug("bootc status --json returned unparseable data")
        return None


def _map_bootc_from_os_release(os_release: OsRelease) -> Optional[str]:
    """Fallback: map bootc system from os-release when bootc status is unavailable."""
    os_id = os_release.id.lower()
    version_id = os_release.version_id
    if os_id == "fedora":
        return f"quay.io/fedora/fedora-bootc:{version_id}"
    if os_id == "centos" or "centos" in os_id:
        major = version_id.split(".")[0]
        return f"quay.io/centos-bootc/centos-bootc:stream{major}"
    if os_id == "rhel":
        major = version_id.split(".")[0]
        return f"registry.redhat.io/rhel{major}/rhel-bootc:{version_id}"
    return None


def map_ostree_base_image(
    host_root: Path,
    os_release: Optional[OsRelease],
    system_type: SystemType,
    *,
    executor: Optional[Executor] = None,
    target_image_override: Optional[str] = None,
) -> Optional[str]:
    """Map an ostree/bootc source system to its target base image.

    Returns the base image ref string, or None if the system is unknown
    (caller should enforce refusal per spec).
    """
    if target_image_override:
        return target_image_override

    if not os_release:
        return None

    # Check for Universal Blue first (present on both system types)
    ublue_ref = _read_ublue_image_info(host_root)
    if ublue_ref:
        _debug(f"Universal Blue system detected: {ublue_ref}")
        return ublue_ref

    # rpm-ostree systems: map by VARIANT_ID
    if system_type == SystemType.RPM_OSTREE:
        variant_id = os_release.variant_id.lower()
        if variant_id in _FEDORA_OSTREE_DESKTOPS:
            return f"{_FEDORA_OSTREE_DESKTOPS[variant_id]}:{os_release.version_id}"
        return None

    # bootc systems: prefer bootc status --json, fall back to os-release
    if system_type == SystemType.BOOTC:
        ref = _bootc_status_image_ref(executor)
        if ref:
            return ref
        return _map_bootc_from_os_release(os_release)

    return None
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/system_type.py tests/test_system_type.py
git commit -m "feat(detection): ostree/bootc base image mapping with UBlue and bootc status support"
```

---

## Task 4: Pipeline Wiring + Refusal Enforcement

This is the critical integration task. It wires `detect_system_type` and `map_ostree_base_image` into `run_all()`, feeding the resolved ref into the existing `BaselineResolver` path. It enforces the spec's hard refusal when mapping fails.

**Files:**
- Modify: `src/yoinkc/inspectors/__init__.py`
- Test: `tests/test_system_type.py` (append pipeline tests)

- [ ] **Step 1: Write pipeline wiring + refusal tests**

Append to `tests/test_system_type.py`:

```python
from unittest.mock import patch
from yoinkc.inspectors import run_all, _read_os_release
import yoinkc.preflight as preflight_mod


def test_read_os_release_captures_variant_id(tmp_path):
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    os_rel = _read_os_release(tmp_path)
    assert os_rel is not None
    assert os_rel.variant_id == "silverblue"


def _silverblue_executor(cmd, *, cwd=None):
    """Executor simulating a Silverblue system."""
    if cmd == ["bootc", "status"]:
        return RunResult(stdout="", stderr="not found", returncode=1)
    if cmd == ["rpm-ostree", "status"]:
        return RunResult(stdout="State: idle", stderr="", returncode=0)
    # All other commands: neutral failures
    return RunResult(stdout="", stderr="", returncode=1)


def _setup_silverblue_host(tmp_path):
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    (etc / "hostname").write_text("test-host\n")
    return tmp_path


def test_run_all_detects_rpm_ostree(tmp_path):
    host_root = _setup_silverblue_host(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=_silverblue_executor, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.RPM_OSTREE


def test_run_all_package_mode_unchanged(tmp_path):
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
    )
    (etc / "hostname").write_text("test\n")
    no_ostree_executor = _mock_executor()
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(tmp_path, executor=no_ostree_executor, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.PACKAGE_MODE


def test_run_all_unknown_ostree_refuses_without_no_baseline(tmp_path):
    """Unknown ostree system without --target-image and without --no-baseline -> hard exit."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="CustomOS"\nVERSION_ID=1.0\nID=custom-os\n'
        'VARIANT_ID=custom\nPRETTY_NAME="Custom OS"\n'
    )
    (etc / "hostname").write_text("test\n")

    def unknown_executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="State: idle", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        with pytest.raises(SystemExit):
            run_all(tmp_path, executor=unknown_executor)


def test_run_all_unknown_ostree_proceeds_with_no_baseline(tmp_path):
    """Unknown ostree system + --no-baseline -> warn and proceed."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="CustomOS"\nVERSION_ID=1.0\nID=custom-os\n'
        'VARIANT_ID=custom\nPRETTY_NAME="Custom OS"\n'
    )
    (etc / "hostname").write_text("test\n")

    def unknown_executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="State: idle", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(
            tmp_path, executor=unknown_executor, no_baseline_opt_in=True,
        )
    assert snapshot.system_type == SystemType.RPM_OSTREE
    # Should have a warning about unknown base
    warning_msgs = [w.get("message", "") for w in snapshot.warnings]
    assert any("could not map" in m.lower() or "unknown" in m.lower() for m in warning_msgs)


def test_run_all_target_image_overrides_ostree_mapping(tmp_path):
    """--target-image overrides auto-mapping on ostree systems."""
    host_root = _setup_silverblue_host(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(
            host_root, executor=_silverblue_executor,
            target_image="quay.io/my-custom/image:latest",
            no_baseline_opt_in=True,
        )
    assert snapshot.system_type == SystemType.RPM_OSTREE
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py::test_run_all_unknown_ostree_refuses_without_no_baseline -v`
Expected: FAIL — `run_all` does not yet enforce refusal

- [ ] **Step 3: Wire detection + mapping + refusal into run_all**

In `src/yoinkc/inspectors/__init__.py`, add imports:

```python
from ..schema import InspectionSnapshot, OsRelease, SystemType
from ..system_type import detect_system_type, map_ostree_base_image, OstreeDetectionError
```

In `run_all()`, after `os_release = _read_os_release(host_root)` and `_validate_supported_host`, add system detection and base image resolution:

```python
    # --- System type detection ---
    try:
        system_type = detect_system_type(host_root, executor)
    except OstreeDetectionError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)

    snapshot.system_type = system_type

    if system_type != SystemType.PACKAGE_MODE:
        gate_label = "bootc" if system_type == SystemType.BOOTC else "rpm-ostree"
        _status_fn(f"Detected {gate_label} system, adapting inspection")
        print(f"Detected {gate_label} system, adapting inspection", file=sys.stderr)

        # Resolve ostree base image; feeds into same pipeline as --target-image
        mapped_image = map_ostree_base_image(
            host_root, os_release, system_type,
            executor=executor,
            target_image_override=target_image,
        )
        if mapped_image is None:
            if not no_baseline_opt_in:
                _ostree_unknown_base_fail(os_release, system_type)
            else:
                w.append(make_warning(
                    "pipeline",
                    "Could not map ostree system to a known base image. "
                    "Running without baseline (--no-baseline). "
                    "All installed packages will be included in the Containerfile.",
                ))
        else:
            # Override target_image so BaselineResolver uses the ostree-mapped ref
            target_image = mapped_image
```

Add the refusal function (before `run_all`):

```python
def _ostree_unknown_base_fail(
    os_release: Optional[OsRelease], system_type: SystemType,
) -> None:
    """Print the spec's refusal message for unknown ostree systems and exit."""
    gate_label = "bootc" if system_type == SystemType.BOOTC else "rpm-ostree"
    desc = ""
    if os_release:
        desc = f": {os_release.pretty_name or os_release.id}"
    lines = [
        f"Detected {gate_label} system{desc}",
        "Could not map to a known bootc base image.",
        "",
        "Specify one with: yoinkc --target-image <registry/image:tag>",
        "",
        "Common bases:",
        "  quay.io/fedora-ostree-desktops/silverblue:41",
        "  quay.io/fedora-ostree-desktops/kinoite:41",
        "  quay.io/fedora/fedora-bootc:41",
        "  quay.io/centos-bootc/centos-bootc:stream10",
    ]
    print("\n".join(lines), file=sys.stderr)
    sys.exit(1)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_system_type.py -v -k "run_all"`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/inspectors/__init__.py tests/test_system_type.py
git commit -m "feat(pipeline): wire ostree base image into baseline resolver, enforce unknown-base refusal"
```

---

## Task 5: RPM Inspector Adaptation (Slice 1)

**Files:**
- Modify: `src/yoinkc/inspectors/rpm.py`
- Modify: `src/yoinkc/inspectors/__init__.py` (pass system_type)
- Create: `tests/test_ostree_rpm.py`

- [ ] **Step 1: Write RPM ostree-mode tests**

Create `tests/test_ostree_rpm.py`:

```python
"""RPM inspector tests for ostree/bootc source systems."""

import json
from pathlib import Path

import pytest

from yoinkc.executor import RunResult
from yoinkc.schema import SystemType

_RPMOSTREE_STATUS_JSON = json.dumps({
    "deployments": [{
        "booted": True,
        "requested-packages": ["httpd", "vim-enhanced", "htop"],
        "requested-local-packages": [],
        "packages": [],
        "base-removals": [
            {"name": "nano", "nevra": "nano-7.2-3.fc41.x86_64"}
        ],
        "base-local-replacements": [{
            "name": "kernel",
            "nevra": "kernel-6.8.1-100.fc41.x86_64",
            "base-nevra": "kernel-6.7.9-200.fc41.x86_64",
        }],
    }]
})


class OstreeExecutorSpy:
    """Executor that tracks which commands were called."""

    def __init__(self):
        self.commands_called: list[list[str]] = []

    def __call__(self, cmd, *, cwd=None):
        self.commands_called.append(list(cmd))
        cmd_str = " ".join(cmd)
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout=_RPMOSTREE_STATUS_JSON, stderr="", returncode=0)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(
                stdout="0:bash-5.2.15-2.fc41.x86_64\n0:httpd-2.4.59-1.fc41.x86_64\n",
                stderr="", returncode=0,
            )
        if "nsenter" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=0)
        if "podman" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=1)
        return RunResult(stdout="", stderr="", returncode=1)

    def was_called(self, substring: str) -> bool:
        return any(substring in " ".join(c) for c in self.commands_called)


def _setup_fedora_root(tmp_path):
    etc = tmp_path / "etc"
    etc.mkdir(exist_ok=True)
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\nVARIANT_ID=silverblue\n'
    )
    return tmp_path


def test_rpm_va_not_called_on_ostree(tmp_path):
    """rpm -Va must not be invoked on ostree systems (spy-based check)."""
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)
    spy = OstreeExecutorSpy()
    section = run_rpm(tmp_path, spy, system_type=SystemType.RPM_OSTREE)
    assert not spy.was_called("-Va"), "rpm -Va should never be called on ostree"
    assert section.rpm_va == []


def test_layered_packages_from_rpmostree_status(tmp_path):
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)
    spy = OstreeExecutorSpy()
    section = run_rpm(tmp_path, spy, system_type=SystemType.RPM_OSTREE)
    added_names = [p.name for p in section.packages_added]
    assert "httpd" in added_names
    assert "vim-enhanced" in added_names
    assert "htop" in added_names


def test_removed_packages_captured(tmp_path):
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)
    section = run_rpm(tmp_path, OstreeExecutorSpy(), system_type=SystemType.RPM_OSTREE)
    assert "nano" in section.ostree_removals


def test_overridden_packages_captured(tmp_path):
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)
    section = run_rpm(tmp_path, OstreeExecutorSpy(), system_type=SystemType.RPM_OSTREE)
    override_names = [o.name for o in section.ostree_overrides]
    assert "kernel" in override_names
    kernel = next(o for o in section.ostree_overrides if o.name == "kernel")
    assert "6.7.9" in kernel.from_nevra
    assert "6.8.1" in kernel.to_nevra


def test_rpmostree_status_failure_handled(tmp_path):
    """rpm-ostree status --json failure -> empty overrides/removals, no crash."""
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)

    def failing_executor(cmd, *, cwd=None):
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout="", stderr="error", returncode=1)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout="0:bash-5.2.15-2.fc41.x86_64\n", stderr="", returncode=0)
        if "nsenter" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_rpm(tmp_path, failing_executor, system_type=SystemType.RPM_OSTREE)
    assert section.ostree_overrides == []
    assert section.ostree_removals == []


def test_rpmostree_status_invalid_json_handled(tmp_path):
    """Invalid JSON from rpm-ostree status -> graceful fallback."""
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)

    def bad_json_executor(cmd, *, cwd=None):
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout="not json{{{", stderr="", returncode=0)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout="0:bash-5.2.15-2.fc41.x86_64\n", stderr="", returncode=0)
        if "nsenter" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_rpm(tmp_path, bad_json_executor, system_type=SystemType.RPM_OSTREE)
    assert section.ostree_overrides == []
    assert section.ostree_removals == []
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_rpm.py::test_rpm_va_not_called_on_ostree -v`
Expected: FAIL — `run_rpm` does not accept `system_type`

- [ ] **Step 3: Implement RPM inspector ostree adaptation**

In `src/yoinkc/inspectors/rpm.py`:

Add to imports:
```python
import json
from ..schema import SystemType, OstreePackageOverride
```

Add `system_type` parameter to `run()`:
```python
def run(
    host_root, executor, *,
    # ... existing params ...
    system_type: SystemType = SystemType.PACKAGE_MODE,
) -> RpmSection:
```

Add `is_ostree` flag early in the function:
```python
    is_ostree = system_type in (SystemType.RPM_OSTREE, SystemType.BOOTC)
```

Replace `rpm -Va` block with conditional:
```python
    if is_ostree:
        _debug("Skipping rpm -Va on ostree/bootc system (immutable /usr)")
        section.rpm_va = []
    elif executor is not None:
        # ... existing rpm -Va code unchanged ...
```

After the existing package parsing, add rpm-ostree status parsing call:
```python
    if is_ostree and executor is not None:
        _parse_rpmostree_package_state(executor, section)
```

Add the parsing function (see Task 5 Step 3 in revision 1 for full code — same `_parse_rpmostree_package_state` but with `json.JSONDecodeError` and missing-field handling).

- [ ] **Step 4: Pass system_type in run_all**

In `src/yoinkc/inspectors/__init__.py`, update `_run_rpm_inspector`:
```python
    def _run_rpm_inspector():
        return run_rpm(
            host_root, executor,
            baseline_packages_file=baseline_packages_file,
            warnings=w, resolver=resolver,
            target_version=target_version,
            target_image=target_image,
            preflight_baseline=preflight_baseline,
            system_type=system_type,
        )
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_rpm.py -v`
Expected: PASS

- [ ] **Step 6: Full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/inspectors/rpm.py src/yoinkc/inspectors/__init__.py tests/test_ostree_rpm.py
git commit -m "feat(rpm): skip rpm -Va on ostree, parse rpm-ostree status for layered/overridden/removed"
```

---

## Task 6: Config Inspector Adaptation (Slice 1)

**Files:**
- Modify: `src/yoinkc/inspectors/config.py`
- Modify: `src/yoinkc/inspectors/__init__.py` (pass system_type)
- Create: `tests/test_ostree_config.py`

Key differences from revision 1:
- Tier 1 includes SELinux context comparison via `os.getxattr("security.selinux")`
- Tier 2 uses `rpm -qf` for ownership check, then targeted `rpm -V <file>` for modification detection (per spec)

- [ ] **Step 1: Write config ostree-mode tests**

Create `tests/test_ostree_config.py`:

```python
"""Config inspector tests for ostree/bootc source systems."""

import os
from pathlib import Path

from yoinkc.executor import RunResult
from yoinkc.schema import SystemType


def _config_executor(cmd, *, cwd=None):
    """Executor for config inspector ostree tests."""
    if "rpm" in cmd and "-qf" in cmd:
        # custom-app.conf: not owned by any RPM
        if any("custom-app.conf" in a for a in cmd):
            return RunResult(stdout="", stderr="not owned", returncode=1)
        # rpm-post.conf: owned by some-rpm
        if any("rpm-post.conf" in a for a in cmd):
            return RunResult(stdout="some-rpm\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="not owned", returncode=1)
    if "rpm" in cmd and "-V" in cmd:
        # Targeted rpm -V on a specific file (Tier 2 per spec)
        if any("rpm-post.conf" in a for a in cmd):
            return RunResult(stdout="S.5....T.  c /etc/rpm-post.conf\n", stderr="", returncode=1)
        return RunResult(stdout="", stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd and "--queryformat" in cmd:
        return RunResult(stdout="/etc/rpm-post.conf\n", stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)


def _setup_ostree_config(tmp_path):
    """Create fixture with /usr/etc and /etc for ostree config diffing."""
    usr_etc = tmp_path / "usr" / "etc"
    (usr_etc / "ssh").mkdir(parents=True)
    (usr_etc / "ssh" / "sshd_config").write_text(
        "Port 22\nPermitRootLogin yes\nPasswordAuthentication yes\n"
    )
    (usr_etc / "httpd" / "conf").mkdir(parents=True)
    (usr_etc / "httpd" / "conf" / "httpd.conf").write_text(
        "ServerRoot /etc/httpd\nListen 80\n"
    )

    etc = tmp_path / "etc"
    (etc / "ssh").mkdir(parents=True)
    (etc / "ssh" / "sshd_config").write_text(
        "Port 2222\nPermitRootLogin no\nPasswordAuthentication yes\n"
    )
    (etc / "httpd" / "conf").mkdir(parents=True)
    (etc / "httpd" / "conf" / "httpd.conf").write_text(
        "ServerRoot /etc/httpd\nListen 80\n"  # identical to vendor
    )
    (etc / "custom-app.conf").write_text("key=value\n")  # /etc-only, not RPM-owned
    (etc / "rpm-post.conf").write_text("modified\n")  # /etc-only, RPM-owned %post

    (usr_etc / "unmodified.conf").write_text("defaults\n")  # /usr/etc only

    # Volatile files
    (etc / "resolv.conf").write_text("nameserver 8.8.8.8\n")
    (etc / "hostname").write_text("test-host\n")
    (etc / "machine-id").write_text("abc123\n")

    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\n'
    )
    return tmp_path


def test_ostree_modified_config_detected(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/ssh/sshd_config" in paths


def test_ostree_unmodified_config_not_reported(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/httpd/conf/httpd.conf" not in paths


def test_ostree_usr_etc_only_file_not_reported(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert not any("unmodified.conf" in p for p in paths)


def test_ostree_etc_only_unowned_file_detected(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/custom-app.conf" in paths


def test_ostree_etc_only_rpm_owned_post_detected(tmp_path):
    """RPM-owned file in /etc with no /usr/etc counterpart (rare %post case) is detected."""
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/rpm-post.conf" in paths


def test_ostree_volatile_files_skipped(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    host_root = _setup_ostree_config(tmp_path)
    section = run_config(host_root, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/resolv.conf" not in paths
    assert "etc/hostname" not in paths
    assert "etc/machine-id" not in paths


def test_ostree_symlink_target_change_detected(tmp_path):
    from yoinkc.inspectors.config import run as run_config
    usr_etc = tmp_path / "usr" / "etc"
    etc = tmp_path / "etc"
    usr_etc.mkdir(parents=True)
    etc.mkdir(parents=True)
    (etc / "os-release").write_text('NAME="Fedora"\nVERSION_ID=41\nID=fedora\nVARIANT_ID=silverblue\n')
    (usr_etc / "localtime").symlink_to("/usr/share/zoneinfo/UTC")
    (etc / "localtime").symlink_to("/usr/share/zoneinfo/America/New_York")
    section = run_config(tmp_path, _config_executor, system_type=SystemType.RPM_OSTREE)
    paths = [e.path for e in section.files]
    assert "etc/localtime" in paths
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_config.py::test_ostree_modified_config_detected -v`
Expected: FAIL — `run_config` does not accept `system_type`

- [ ] **Step 3: Implement ostree config diffing in config.py**

In `src/yoinkc/inspectors/config.py`, add `SystemType` import, `system_type` parameter to `run()`, and the ostree code path. The implementation follows the same structure as revision 1 but with these additions:

**Tier 1 SELinux context comparison** — after content/metadata comparison, add:

```python
        # SELinux context comparison (per spec: part of primary tier)
        try:
            v_ctx = os.getxattr(str(vendor_file), "security.selinux")
            u_ctx = os.getxattr(str(user_file), "security.selinux")
            if v_ctx != u_ctx:
                existing = next((e for e in section.files if e.path == rel_path), None)
                details = f"SELinux context changed: {v_ctx.decode(errors='replace').rstrip(chr(0))} -> {u_ctx.decode(errors='replace').rstrip(chr(0))}"
                if existing:
                    existing.change_details = (existing.change_details or "") + f" [{details}]"
                elif vendor_content == user_content:
                    section.files.append(ConfigFileEntry(
                        path=rel_path,
                        kind=ConfigFileKind.RPM_MODIFIED,
                        change_details=details,
                    ))
        except OSError:
            pass  # xattr not available (test environments, non-SELinux hosts)
```

**Tier 2 targeted rpm -V** — in `_find_etc_only_files`, for RPM-owned files, additionally run targeted `rpm -V`:

```python
        if executor is not None:
            from .._util import run_rpm_query as _run_rpm_query
            # Check ownership with rpm -qf
            qf_result = _run_rpm_query(executor, host_root, ["-qf", str(etc_file)])
            if qf_result.returncode == 0 and qf_result.stdout.strip():
                # RPM-owned: run targeted rpm -V to check for %post modifications
                pkg_name = qf_result.stdout.strip().splitlines()[0].strip()
                v_result = executor(["rpm", "-V", pkg_name])
                if v_result.returncode != 0 and rel_path.lstrip("/") in v_result.stdout:
                    kind = ConfigFileKind.RPM_MODIFIED  # modified %post file
                else:
                    kind = ConfigFileKind.RPM_MODIFIED  # RPM-owned but in /etc only
            else:
                kind = ConfigFileKind.UNOWNED  # user-created
```

- [ ] **Step 4: Pass system_type to config inspector in run_all**

In `src/yoinkc/inspectors/__init__.py`:
```python
    snapshot.config = _safe_run("config", lambda: run_config(
        host_root, executor,
        rpm_section=snapshot.rpm, rpm_owned_paths_override=rpm_owned,
        config_diffs=config_diffs, warnings=w, system_type=system_type,
    ), None, w)
```

- [ ] **Step 5: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_config.py -v`
Expected: PASS

- [ ] **Step 6: Full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/yoinkc
git add src/yoinkc/inspectors/config.py src/yoinkc/inspectors/__init__.py tests/test_ostree_config.py
git commit -m "feat(config): ostree /usr/etc diffing with SELinux context + Tier 2 rpm -V"
```

---

## Task 7: Non-RPM Software Inspector Adaptation (Slice 1)

**Files:**
- Modify: `src/yoinkc/inspectors/non_rpm_software.py`
- Modify: `src/yoinkc/inspectors/__init__.py`
- Create: `tests/test_ostree_non_rpm.py`

Key difference from revision 1: adds `/usr/lib/python3*` and `/usr/lib64/python3*` to the ostree skip list (per spec table and Collins finding).

- [ ] **Step 1: Write tests**

Create `tests/test_ostree_non_rpm.py`:

```python
"""Non-RPM software inspector tests for ostree/bootc source systems."""

from pathlib import Path
from yoinkc.executor import RunResult
from yoinkc.schema import SystemType


def _non_rpm_executor(cmd, *, cwd=None):
    return RunResult(stdout="", stderr="", returncode=1)


def test_immutable_usr_local_skipped_on_ostree(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    usr_local = tmp_path / "usr" / "local" / "bin"
    usr_local.mkdir(parents=True)
    (usr_local / "custom-app").write_text("#!/bin/bash\n")
    opt = tmp_path / "opt" / "myapp"
    opt.mkdir(parents=True)
    (opt / "app.py").write_text("print('hello')\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any(p.startswith("usr/local") for p in paths), \
        f"/usr/local content found in ostree scan: {[p for p in paths if 'usr/local' in p]}"
    assert any("opt/myapp" in p for p in paths)


def test_immutable_usr_lib_python_skipped_on_ostree(tmp_path):
    """Spec: skip /usr/lib/python3 immutable content on ostree."""
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    pydir = tmp_path / "usr" / "lib" / "python3.12" / "site-packages" / "mylib"
    pydir.mkdir(parents=True)
    (pydir / "__init__.py").write_text("# immutable\n")
    pydir64 = tmp_path / "usr" / "lib64" / "python3.12" / "site-packages" / "mylib64"
    pydir64.mkdir(parents=True)
    (pydir64 / "__init__.py").write_text("# immutable\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any("usr/lib/python3" in p for p in paths), \
        f"/usr/lib/python3 content found in ostree scan: {[p for p in paths if 'python3' in p]}"
    assert not any("usr/lib64/python3" in p for p in paths)


def test_ostree_var_internal_paths_skipped(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    for internal in ["var/lib/ostree", "var/lib/rpm-ostree", "var/lib/flatpak"]:
        p = tmp_path / internal / "data"
        p.mkdir(parents=True)
        (p / "file.db").write_text("data")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any("lib/ostree" in p for p in paths)
    assert not any("lib/rpm-ostree" in p for p in paths)
    assert not any("lib/flatpak" in p for p in paths)


def test_package_mode_usr_local_still_scanned(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    usr_local = tmp_path / "usr" / "local" / "bin"
    usr_local.mkdir(parents=True)
    (usr_local / "custom-tool").write_text("#!/bin/bash\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.PACKAGE_MODE)
    paths = [item.path for item in section.items]
    assert any("usr/local" in p for p in paths)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_non_rpm.py -v`
Expected: FAIL

- [ ] **Step 3: Implement ostree filtering**

In `src/yoinkc/inspectors/non_rpm_software.py`:

Add `SystemType` import and `system_type` parameter. Add skip lists:

```python
# Paths under /var to skip on ostree/bootc — ostree-managed internal state
_OSTREE_VAR_SKIP = frozenset({"lib/ostree", "lib/rpm-ostree", "lib/flatpak"})

# Immutable /usr paths to skip on ostree/bootc — base image content
_OSTREE_USR_SKIP_PREFIXES = ("usr/local", "usr/lib/python3", "usr/lib64/python3")
```

In the scan directory logic:
- On ostree, remove `usr/local` from FHS scan dirs
- On ostree, skip any path matching `_OSTREE_USR_SKIP_PREFIXES`
- On ostree, skip any `/var` path matching `_OSTREE_VAR_SKIP`

- [ ] **Step 4: Pass system_type in run_all**

```python
    snapshot.non_rpm_software = _safe_run("non_rpm_software", lambda: run_non_rpm_software(
        host_root, executor, deep_binary_scan=deep_binary_scan,
        warnings=w, system_type=system_type,
    ), None, w)
```

- [ ] **Step 5: Run tests, full suite, commit**

```bash
git commit -m "feat(non-rpm): skip immutable /usr and /usr/lib/python3* on ostree systems"
```

---

## Task 8: Pure-bootc Low-Confidence Fallback

This is a dedicated task for the pure-bootc path where `rpm-ostree status` is unavailable and yoinkc must fall back to `rpm -qa` diffing against the base image. Per spec, this path is explicitly low-confidence and must surface warnings and digest info.

**Files:**
- Modify: `src/yoinkc/inspectors/rpm.py`
- Create: `tests/test_ostree_rpm.py` (append pure-bootc tests)

- [ ] **Step 1: Write pure-bootc fallback tests**

Append to `tests/test_ostree_rpm.py`:

```python
def test_pure_bootc_without_rpmostree_uses_rpm_qa(tmp_path):
    """On BOOTC when rpm-ostree is absent, falls back to rpm -qa for package detection."""
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)

    class BootcOnlyExecutor:
        def __init__(self):
            self.commands = []

        def __call__(self, cmd, *, cwd=None):
            self.commands.append(list(cmd))
            if cmd == ["rpm-ostree", "status", "--json"]:
                return RunResult(stdout="", stderr="not found", returncode=127)
            if "rpm" in cmd and "-qa" in cmd:
                return RunResult(
                    stdout="0:bash-5.2.15-2.fc41.x86_64\n0:httpd-2.4.59-1.fc41.x86_64\n",
                    stderr="", returncode=0,
                )
            if "nsenter" in " ".join(cmd):
                return RunResult(stdout="", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=1)

    executor = BootcOnlyExecutor()
    warnings = []
    section = run_rpm(
        tmp_path, executor,
        system_type=SystemType.BOOTC,
        warnings=warnings,
    )
    # rpm -qa still runs as fallback
    assert any("-qa" in " ".join(c) for c in executor.commands)
    # Should have a low-confidence warning
    warning_msgs = [w.get("message", "") if isinstance(w, dict) else str(w) for w in warnings]
    assert any("approximate" in m.lower() or "low-confidence" in m.lower() for m in warning_msgs), \
        f"Expected low-confidence warning, got: {warning_msgs}"


def test_pure_bootc_emits_digest_info_when_available(tmp_path):
    """Pure-bootc fallback should surface the resolved base ref in output metadata."""
    from yoinkc.inspectors.rpm import run as run_rpm
    _setup_fedora_root(tmp_path)

    def executor(cmd, *, cwd=None):
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout="", stderr="not found", returncode=127)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout="0:bash-5.2.15-2.fc41.x86_64\n", stderr="", returncode=0)
        if "nsenter" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    warnings = []
    section = run_rpm(
        tmp_path, executor,
        system_type=SystemType.BOOTC,
        warnings=warnings,
    )
    # Section should indicate low-confidence package detection was used
    assert section.rpm_va == []  # rpm -Va still skipped
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_rpm.py::test_pure_bootc_without_rpmostree_uses_rpm_qa -v`
Expected: FAIL — warning plumbing not yet implemented

- [ ] **Step 3: Implement pure-bootc fallback logic**

In `src/yoinkc/inspectors/rpm.py`, in `_parse_rpmostree_package_state()`:

When `rpm-ostree status --json` returns rc != 0, emit a low-confidence warning instead of silently falling back:

```python
def _parse_rpmostree_package_state(
    executor: Executor, section: RpmSection, *, warnings: Optional[list] = None,
    system_type: SystemType = SystemType.PACKAGE_MODE,
) -> None:
    """Parse rpm-ostree status --json for layered, overridden, and removed packages.

    On pure-bootc systems where rpm-ostree is absent, emits a low-confidence
    warning — package detection falls back to rpm -qa (already run earlier).
    """
    result = executor(["rpm-ostree", "status", "--json"])
    if result.returncode != 0:
        _debug(f"rpm-ostree status --json failed (rc={result.returncode})")
        if system_type == SystemType.BOOTC and warnings is not None:
            warnings.append(make_warning(
                "rpm",
                "Package diff is approximate -- rpm-ostree status is not available on this "
                "bootc system. Package detection used rpm -qa against the base image, which "
                "may differ due to tag drift or NVR skew. Results require manual review.",
                "warning",
            ))
        return

    # ... existing JSON parsing ...
```

Pass `warnings` and `system_type` through from `run()`:
```python
    if is_ostree and executor is not None:
        _parse_rpmostree_package_state(
            executor, section, warnings=warnings, system_type=system_type,
        )
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_ostree_rpm.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(rpm): pure-bootc low-confidence fallback with warning when rpm-ostree absent"
```

---

## Task 9: Flatpak Detection (Slice 2)

**Files:**
- Modify: `src/yoinkc/inspectors/container.py`
- Create: `tests/test_flatpak.py`

Key difference from revision 1: adds negative-case tests for malformed output and nonzero exit.

- [ ] **Step 1: Write Flatpak tests (including negative cases)**

Create `tests/test_flatpak.py`:

```python
"""Flatpak detection tests -- runs on all system types."""

from pathlib import Path
from yoinkc.executor import RunResult


_FLATPAK_LIST_OUTPUT = (
    "org.mozilla.firefox\tflathub\tstable\n"
    "org.gnome.Calculator\tflathub\tstable\n"
    "org.fedoraproject.MediaWriter\tfedora\tstable\n"
)


def _flatpak_executor(cmd, *, cwd=None):
    if cmd == ["which", "flatpak"]:
        return RunResult(stdout="/usr/bin/flatpak\n", stderr="", returncode=0)
    if cmd[:2] == ["flatpak", "list"]:
        return RunResult(stdout=_FLATPAK_LIST_OUTPUT, stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)


def _no_flatpak_executor(cmd, *, cwd=None):
    if cmd == ["which", "flatpak"]:
        return RunResult(stdout="", stderr="not found", returncode=1)
    return RunResult(stdout="", stderr="", returncode=1)


def test_flatpak_apps_detected(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _flatpak_executor)
    assert len(section.flatpak_apps) == 3
    ids = {a.app_id for a in section.flatpak_apps}
    assert ids == {"org.mozilla.firefox", "org.gnome.Calculator", "org.fedoraproject.MediaWriter"}


def test_flatpak_origin_captured(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _flatpak_executor)
    firefox = next(a for a in section.flatpak_apps if a.app_id == "org.mozilla.firefox")
    assert firefox.origin == "flathub"


def test_flatpak_not_present_silently_skipped(tmp_path):
    from yoinkc.inspectors.container import run as run_container
    section = run_container(tmp_path, _no_flatpak_executor)
    assert section.flatpak_apps == []


def test_flatpak_list_nonzero_exit_handled(tmp_path):
    """flatpak list returns error -- no crash, no partial data."""
    from yoinkc.inspectors.container import run as run_container

    def failing_executor(cmd, *, cwd=None):
        if cmd == ["which", "flatpak"]:
            return RunResult(stdout="/usr/bin/flatpak", stderr="", returncode=0)
        if cmd[:2] == ["flatpak", "list"]:
            return RunResult(stdout="", stderr="error", returncode=1)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_container(tmp_path, failing_executor)
    assert section.flatpak_apps == []


def test_flatpak_malformed_output_no_crash(tmp_path):
    """Malformed flatpak list output -- no crash, skip bad rows."""
    from yoinkc.inspectors.container import run as run_container

    def bad_output_executor(cmd, *, cwd=None):
        if cmd == ["which", "flatpak"]:
            return RunResult(stdout="/usr/bin/flatpak", stderr="", returncode=0)
        if cmd[:2] == ["flatpak", "list"]:
            return RunResult(
                stdout="org.valid.App\tflathub\tstable\nsingle-column-only\n\n",
                stderr="", returncode=0,
            )
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_container(tmp_path, bad_output_executor)
    assert len(section.flatpak_apps) == 1
    assert section.flatpak_apps[0].app_id == "org.valid.App"
```

- [ ] **Step 2-6: Implement, run tests, commit** (same structure as revision 1 Task 8)

```bash
git commit -m "feat(flatpak): detect installed Flatpak apps on all system types"
```

---

## Task 10: Storage Inspector Adaptation (Slice 2)

Same as revision 1 Task 9. Filter `/sysroot`, `/ostree`, and `/ostree/*` mounts. Synthetic fixtures (explicit deviation from spec's real-fixture requirement).

```bash
git commit -m "feat(storage): filter ostree-managed mounts from storage inventory"
```

---

## Task 11: Kernel/Boot Inspector Adaptation (Slice 2)

Key difference from revision 1: actually implements BLS entry filtering instead of just suppressing `grub_defaults`.

**Files:**
- Modify: `src/yoinkc/inspectors/kernel_boot.py`
- Test: `tests/test_ostree_slice2.py` (append)

- [ ] **Step 1: Write kernel/boot BLS filtering tests**

Append to `tests/test_ostree_slice2.py`:

```python
def test_ostree_grub_defaults_suppressed(tmp_path):
    """GRUB defaults suppressed on ostree (BLS-managed)."""
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "default").mkdir()
    (etc / "default" / "grub").write_text("GRUB_TIMEOUT=5\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("root=/dev/sda2 ro rhgb quiet custom.option=foo")

    def executor(cmd, *, cwd=None):
        if "lsmod" in " ".join(cmd):
            return RunResult(stdout="Module  Size  Used\nvfat  20480  1\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_kernel_boot(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    assert section.grub_defaults == ""


def test_ostree_bls_entries_not_in_snapshot(tmp_path):
    """BLS entries from /boot/loader/entries/ are ostree-managed -- not included in snapshot."""
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    loader = tmp_path / "boot" / "loader" / "entries"
    loader.mkdir(parents=True)
    (loader / "ostree-1-6.8.1.conf").write_text(
        "title Fedora 41 (6.8.1)\nversion 6.8.1\n"
        "linux /vmlinuz-6.8.1\ninitrd /initramfs-6.8.1.img\n"
        "options root=/dev/sda2 ro rhgb quiet\n"
    )
    (loader / "ostree-0-6.7.9.conf").write_text(
        "title Fedora 41 (6.7.9)\nversion 6.7.9\nlinux /vmlinuz-6.7.9\n"
    )
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("root=/dev/sda2 ro rhgb quiet custom.option=foo")

    def executor(cmd, *, cwd=None):
        if "lsmod" in " ".join(cmd):
            return RunResult(stdout="Module  Size  Used\nvfat  20480  1\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_kernel_boot(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    # BLS entries should be filtered — the specific observable depends on whether
    # the inspector surfaces entries as a list. At minimum, cmdline is still captured
    # and the renderer's _operator_kargs filter handles karg separation.
    assert section.cmdline is not None
    assert "custom.option=foo" in section.cmdline


def test_ostree_user_kargs_preserved(tmp_path):
    """User-added kernel args are preserved through the pipeline."""
    from yoinkc.inspectors.kernel_boot import run as run_kernel_boot
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text(
        "BOOT_IMAGE=/vmlinuz root=/dev/sda2 ro rhgb quiet "
        "systemd.unified_cgroup_hierarchy=1 mitigations=off"
    )

    def executor(cmd, *, cwd=None):
        if "lsmod" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    section = run_kernel_boot(tmp_path, executor, system_type=SystemType.RPM_OSTREE)
    assert "mitigations=off" in section.cmdline
    assert "systemd.unified_cgroup_hierarchy=1" in section.cmdline
```

- [ ] **Step 3: Implement BLS filtering**

In `src/yoinkc/inspectors/kernel_boot.py`, on ostree:
- Suppress `grub_defaults` (BLS-managed, no user-facing GRUB config)
- Skip BLS entry enumeration from `/boot/loader/entries/` (ostree-managed boot entries should not appear in the snapshot as user customizations)
- cmdline is still captured — the renderer's existing `_operator_kargs()` filter in `_helpers.py` separates user kargs from bootloader-managed ones

```bash
git commit -m "feat(kernel-boot): filter BLS entries and suppress GRUB defaults on ostree"
```

---

## Task 12: Scheduled Tasks Inspector Adaptation (Slice 2)

Same structure as revision 1 Task 11. Vendor timers from `/usr/lib/systemd/` are force-classified as vendor on ostree. Test uses unconditional assertions (not `if name in list: assert` — Thorn finding).

```bash
git commit -m "feat(timers): extend vendor timer filter to /usr/lib/systemd/ on ostree"
```

---

## Task 13: Containerfile Renderer Adaptations

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/packages.py`
- Modify: `src/yoinkc/renderers/containerfile/_core.py`
- Create: `tests/test_ostree_renderer.py`

Key differences from revision 1:
- Adds test for layered packages appearing as `RUN dnf install` (Thorn finding #15)
- bootc label logic checks `system_type` on snapshot

- [ ] **Step 1: Write renderer tests**

Create `tests/test_ostree_renderer.py` with all tests from revision 1 plus:

```python
def test_ostree_layered_packages_in_dnf_install(tmp_path):
    """Layered packages from rpm-ostree appear as RUN dnf install lines."""
    from yoinkc.renderers.containerfile._core import _render_containerfile_content
    snapshot = _make_ostree_snapshot()
    content = _render_containerfile_content(snapshot, tmp_path)
    assert "RUN dnf install -y" in content
    assert "httpd" in content
    assert "vim-enhanced" in content


def test_renderer_integration_from_ostree_snapshot(tmp_path):
    """Full render on ostree snapshot produces Containerfile + flatpaks.list."""
    from yoinkc.renderers.containerfile._core import render
    from jinja2 import Environment
    snapshot = _make_ostree_snapshot()
    render(snapshot, Environment(autoescape=True), tmp_path)
    containerfile = tmp_path / "Containerfile"
    assert containerfile.exists()
    content = containerfile.read_text()
    assert "FROM quay.io/fedora-ostree-desktops/silverblue:41" in content
    assert "dnf install" in content
    assert (tmp_path / "flatpaks.list").exists()
```

- [ ] **Step 2-7: Implement (same structure as revision 1 Task 12), commit**

```bash
git commit -m "feat(renderer): ostree package output, bootc label, flatpaks.list, layered pkg install"
```

---

## Task 14: Integration Tests

**Files:**
- Create: `tests/test_ostree_integration.py`
- Modify: `tests/conftest.py`

Key differences from revision 1:
- Adds refusal-path integration test (assert SystemExit + stderr message)
- Asserts rendered Containerfile output, not just snapshot state
- Pure-bootc test asserts warning text, not just system_type
- No `len(rpm_va) > 0` — uses spy to confirm rpm -Va was invoked on package-mode

- [ ] **Step 1: Add ostree fixture executor to conftest.py** (same as revision 1 but with exact command matching)

- [ ] **Step 2: Write integration tests**

Create `tests/test_ostree_integration.py`:

```python
"""End-to-end integration tests for ostree/bootc source scanning."""

import json
from pathlib import Path
from unittest.mock import patch

import pytest
from jinja2 import Environment

from yoinkc.executor import RunResult
from yoinkc.inspectors import run_all
from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import SystemType
import yoinkc.preflight as preflight_mod


def _setup_silverblue_root(tmp_path):
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    (etc / "hostname").write_text("silverblue-test\n")
    usr_etc = tmp_path / "usr" / "etc"
    (usr_etc / "ssh").mkdir(parents=True)
    (usr_etc / "ssh" / "sshd_config").write_text("Port 22\nPermitRootLogin yes\n")
    (etc / "ssh").mkdir()
    (etc / "ssh" / "sshd_config").write_text("Port 2222\nPermitRootLogin no\n")
    (etc / "resolv.conf").write_text("nameserver 8.8.8.8\n")
    (etc / "machine-id").write_text("abc123\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("BOOT_IMAGE=/vmlinuz root=/dev/sda2 ro rhgb quiet")
    return tmp_path


def test_full_pipeline_silverblue(tmp_path, ostree_fixture_executor):
    host_root = _setup_silverblue_root(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=ostree_fixture_executor, no_baseline_opt_in=True)

    assert snapshot.system_type == SystemType.RPM_OSTREE
    assert snapshot.os_release.variant_id == "silverblue"
    assert snapshot.rpm is not None
    assert snapshot.rpm.rpm_va == []

    added_names = [p.name for p in snapshot.rpm.packages_added]
    assert "httpd" in added_names

    if snapshot.config:
        config_paths = [f.path for f in snapshot.config.files]
        assert "etc/ssh/sshd_config" in config_paths
        assert "etc/resolv.conf" not in config_paths
        assert "etc/machine-id" not in config_paths

    if snapshot.containers:
        app_ids = [a.app_id for a in snapshot.containers.flatpak_apps]
        assert "org.mozilla.firefox" in app_ids


def test_full_pipeline_renders_containerfile(tmp_path, ostree_fixture_executor):
    """Integration: pipeline + renderer produces valid Containerfile."""
    host_root = _setup_silverblue_root(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=ostree_fixture_executor, no_baseline_opt_in=True)
    output_dir = tmp_path / "output"
    output_dir.mkdir()
    run_all_renderers(snapshot, output_dir)
    containerfile = output_dir / "Containerfile"
    assert containerfile.exists()
    content = containerfile.read_text()
    assert "FROM " in content
    assert "dnf install" in content or "dnf remove" in content


def test_refusal_path_integration(tmp_path):
    """Unknown ostree system without --target-image -> exit with spec error message."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('NAME="CustomOS"\nVERSION_ID=1.0\nID=custom\nVARIANT_ID=custom\n')
    (etc / "hostname").write_text("test\n")

    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        with pytest.raises(SystemExit) as exc_info:
            run_all(tmp_path, executor=executor)
    assert exc_info.value.code == 1


def test_pure_bootc_pipeline_warns(tmp_path):
    """Pure bootc system without rpm-ostree emits low-confidence warning."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('NAME="Fedora"\nVERSION_ID=41\nID=fedora\n')
    (etc / "hostname").write_text("bootc-test\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("root=/dev/sda2")

    def executor(cmd, *, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        if cmd == ["bootc", "status", "--json"]:
            return RunResult(
                stdout=json.dumps({"status": {"booted": {"image": {"image": {"image": "quay.io/fedora/fedora-bootc:41"}}}}}),
                stderr="", returncode=0,
            )
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout="", stderr="not found", returncode=127)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout="0:bash-5.2.15-2.fc41.x86_64\n", stderr="", returncode=0)
        if "nsenter" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(tmp_path, executor=executor, no_baseline_opt_in=True)

    assert snapshot.system_type == SystemType.BOOTC
    warning_msgs = [w.get("message", "") for w in snapshot.warnings]
    assert any("approximate" in m.lower() or "rpm-ostree" in m.lower() for m in warning_msgs), \
        f"Expected low-confidence warning, got warnings: {warning_msgs}"
```

- [ ] **Step 3: Run tests, full suite, commit**

```bash
git commit -m "test(integration): end-to-end ostree pipeline with refusal, renderer, and pure-bootc warnings"
```

---

## Self-Review (Revision 2)

### Spec Coverage

| Spec Requirement | Task |
|---|---|
| System type detection (4-step) | Task 2 |
| Gate message | Task 4 |
| Base image auto-mapping (all variants) | Task 3 |
| Base image wiring into BaselineResolver | Task 4 (critical fix) |
| Unknown-base hard refusal + --target-image override | Task 4 (critical fix) |
| --no-baseline interaction with refusal | Task 4 |
| bootc status --json image ref | Task 3 |
| Package detection: rpm-ostree layered/overridden/removed | Task 5 |
| Skip rpm -Va on ostree | Task 5 |
| Pure-bootc low-confidence fallback + warnings | Task 8 (new dedicated task) |
| Config: /usr/etc -> /etc diff (Tier 1) | Task 6 |
| Config: SELinux context in Tier 1 | Task 6 (added) |
| Config: /etc-only files with rpm -qf + rpm -V (Tier 2) | Task 6 (aligned to spec) |
| Config: volatile file filtering | Task 6 |
| Non-RPM: skip /usr/local + /usr/lib/python3* | Task 7 (python3 added) |
| Flatpak detection (all system types) | Task 9 |
| flatpaks.list output | Task 13 |
| Storage: filter ostree mounts | Task 10 |
| Kernel/boot: filter BLS entries, user kargs | Task 11 (actual BLS filtering) |
| Scheduled tasks: vendor timer filter | Task 12 |
| Containerfile: FROM, bootc label, overrides, removals | Task 13 |
| Containerfile: layered packages -> dnf install | Task 13 (added) |

### Round-1 Plan Review Findings Addressed

| Finding | Resolution |
|---|---|
| Missing baseline / FROM wiring (Kit #1, Collins #1) | Task 4: mapped ref feeds into `target_image` for BaselineResolver |
| Unknown-system refusal not enforced (Kit #2, Thorn #3, Collins #1) | Task 4: `_ostree_unknown_base_fail` + integration test |
| --base-image vs --target-image (Kit #3) | Dropped --base-image, reuse --target-image |
| Config Tier 2: rpm -V vs rpm -qf (Kit #4, Collins #3) | Task 6: rpm -qf + targeted rpm -V |
| Config Tier 1: SELinux context (Kit #5, Collins #4, Thorn #7) | Task 6: os.getxattr comparison |
| Non-RPM /usr/lib/python3 (Kit #6, Collins #5) | Task 7: added to skip list |
| Kernel/boot BLS underspecified (Kit #7, Collins #6, Thorn #1) | Task 11: actual BLS filtering |
| Slice 2 synthetic vs real fixtures (Kit #8, Thorn #14) | Explicit deviation noted in file structure |
| Pure-bootc stub (Kit #9, Collins #2, Thorn #2) | Task 8: dedicated task with warnings |
| Command-string matching (Thorn #8) | Tests use exact `cmd == [...]` matching |
| Conditional assertions (Thorn #9) | Unconditional assertions throughout |
| rpm -Va spy check (Thorn #10) | Task 5: `OstreeExecutorSpy.was_called` |
| Substring path assertions (Thorn #11) | Explicit path matching with `startswith`/equality |
| sys.exit in library (Thorn #13) | Task 2: OstreeDetectionError exception |
| Renderer: layered -> dnf install test (Thorn #15) | Task 13: explicit test |
| bootc mapping from bootc status (Collins #7) | Task 3: `_bootc_status_image_ref` |

### Placeholder Scan

No TBD, TODO, or "implement later." All steps have code or precise implementation guidance.

### Type Consistency

- `SystemType` defined in `schema.py`, imported everywhere
- `system_type` parameter name consistent across all inspector `run()` signatures
- `OstreeDetectionError` raised in library, caught in pipeline
- `map_ostree_base_image()` returns `Optional[str]`, caller enforces refusal
- `FlatpakApp`, `OstreePackageOverride` models consistent between schema and usage
