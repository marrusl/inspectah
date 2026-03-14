# Fleet Aggregation (`yoinkc-fleet`) — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `yoinkc-fleet aggregate` — a CLI tool that merges N yoinkc inspection snapshots into a single fleet-representative snapshot with prevalence metadata.

**Architecture:** A new `yoinkc.fleet` package containing: schema additions (`FleetPrevalence`, `FleetMeta`), a snapshot loader that reads tarballs/JSON from a directory, section-specific merge functions keyed by identity/variant rules, and an argparse-based CLI registered as a `yoinkc-fleet` console script. The merge engine produces a valid `InspectionSnapshot` that flows through the existing pipeline.

**Tech Stack:** Python 3.11+, Pydantic v2, argparse, pytest. No new dependencies.

**Spec:** `docs/specs/2026-03-13-fleet-analysis-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/yoinkc/schema.py` | Add `FleetPrevalence`, `FleetMeta` models; add `fleet` field to 10 item models |
| `src/yoinkc/fleet/__init__.py` | Package init — exports `aggregate_snapshots` |
| `src/yoinkc/fleet/loader.py` | Discover and load snapshots from a directory of tarballs/JSON files |
| `src/yoinkc/fleet/merge.py` | Core merge engine — union-with-prevalence, identity functions, per-section merge |
| `src/yoinkc/fleet/cli.py` | argparse CLI for `yoinkc-fleet aggregate` |
| `src/yoinkc/fleet/__main__.py` | Entry point: `def main()` calling cli + merge |
| `tests/test_fleet_schema.py` | Tests for FleetPrevalence/FleetMeta schema additions |
| `tests/test_fleet_loader.py` | Tests for tarball/JSON discovery and loading |
| `tests/test_fleet_merge.py` | Tests for merge engine — identity, prevalence, threshold, variants |
| `tests/test_fleet_cli.py` | Tests for CLI argument parsing and end-to-end |
| `pyproject.toml` | Add `yoinkc-fleet` console script entry point |

---

## Chunk 1: Schema Additions and Snapshot Loader

### Task 1: Add FleetPrevalence and FleetMeta to schema

**Files:**
- Modify: `src/yoinkc/schema.py`
- Create: `tests/test_fleet_schema.py`

- [ ] **Step 1: Write failing tests for FleetPrevalence and FleetMeta**

```python
# tests/test_fleet_schema.py
"""Tests for fleet-related schema additions."""

import json
from yoinkc.schema import FleetPrevalence, FleetMeta


class TestFleetPrevalence:
    def test_basic_construction(self):
        fp = FleetPrevalence(count=98, total=100)
        assert fp.count == 98
        assert fp.total == 100
        assert fp.hosts == []

    def test_with_hosts(self):
        fp = FleetPrevalence(count=2, total=100, hosts=["web-01", "web-02"])
        assert fp.hosts == ["web-01", "web-02"]

    def test_serialization_roundtrip(self):
        fp = FleetPrevalence(count=50, total=100, hosts=["a", "b"])
        data = json.loads(fp.model_dump_json())
        fp2 = FleetPrevalence(**data)
        assert fp2.count == fp.count
        assert fp2.hosts == fp.hosts


class TestFleetMeta:
    def test_basic_construction(self):
        fm = FleetMeta(
            source_hosts=["web-01", "web-02"],
            total_hosts=2,
            min_prevalence=90,
        )
        assert fm.total_hosts == 2
        assert fm.min_prevalence == 90

    def test_serialization_roundtrip(self):
        fm = FleetMeta(
            source_hosts=["a", "b", "c"],
            total_hosts=3,
            min_prevalence=100,
        )
        data = json.loads(fm.model_dump_json())
        fm2 = FleetMeta(**data)
        assert fm2.source_hosts == ["a", "b", "c"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_schema.py -v`

Expected: FAIL — `FleetPrevalence` and `FleetMeta` not defined yet.

- [ ] **Step 3: Add FleetPrevalence and FleetMeta models to schema.py**

In `src/yoinkc/schema.py`, add after the `OsRelease` class (before the RPM Inspector section):

```python
# --- Fleet metadata (set by yoinkc-fleet aggregate) ---


class FleetPrevalence(BaseModel):
    """Fleet prevalence metadata for a merged snapshot item."""

    count: int
    total: int
    hosts: List[str] = Field(default_factory=list)


class FleetMeta(BaseModel):
    """Fleet-level metadata for a merged snapshot."""

    source_hosts: List[str]
    total_hosts: int
    min_prevalence: int
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_schema.py -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/schema.py tests/test_fleet_schema.py
git commit -m "feat(schema): Add FleetPrevalence and FleetMeta models

Assisted-by: Cursor (<model>)"
```

### Task 2: Add `fleet` field to item models

**Files:**
- Modify: `src/yoinkc/schema.py`
- Modify: `tests/test_fleet_schema.py`

- [ ] **Step 1: Write failing tests for fleet field on item models**

Append to `tests/test_fleet_schema.py`:

```python
from yoinkc.schema import (
    PackageEntry, RepoFile, ConfigFileEntry, ServiceStateChange,
    SystemdDropIn, FirewallZone, GeneratedTimerUnit, QuadletUnit,
    ComposeFile, CronJob, FleetPrevalence,
)


class TestFleetFieldOnModels:
    """Every item model that supports include should accept an optional fleet field."""

    def test_package_entry_fleet_default_none(self):
        p = PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64")
        assert p.fleet is None

    def test_package_entry_with_fleet(self):
        fp = FleetPrevalence(count=98, total=100)
        p = PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64", fleet=fp)
        assert p.fleet.count == 98

    def test_config_file_entry_fleet(self):
        fp = FleetPrevalence(count=50, total=100)
        c = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", fleet=fp)
        assert c.fleet.count == 50

    def test_all_models_accept_fleet_none(self):
        """Verify fleet field exists and defaults to None on all target models."""
        models_with_defaults = [
            PackageEntry(name="x", version="1", release="1", arch="x86_64"),
            RepoFile(path="/etc/yum.repos.d/test.repo"),
            ConfigFileEntry(path="/etc/test.conf", kind="unowned"),
            ServiceStateChange(unit="test.service", current_state="enabled",
                             default_state="disabled", action="enable"),
            SystemdDropIn(unit="test.service", path="etc/systemd/system/test.service.d/override.conf"),
            FirewallZone(path="/etc/firewalld/zones/public.xml", name="public"),
            GeneratedTimerUnit(name="test-timer"),
            QuadletUnit(path="/etc/containers/systemd/test.container", name="test"),
            ComposeFile(path="/opt/app/docker-compose.yml"),
            CronJob(path="/etc/cron.d/test", source="cron.d"),
        ]
        for model in models_with_defaults:
            assert model.fleet is None, f"{type(model).__name__}.fleet should default to None"

    def test_fleet_survives_json_roundtrip(self):
        """Fleet data should survive serialization and deserialization."""
        import json
        fp = FleetPrevalence(count=5, total=10, hosts=["h1", "h2"])
        p = PackageEntry(name="vim", version="9", release="1", arch="x86_64", fleet=fp)
        data = json.loads(p.model_dump_json())
        p2 = PackageEntry(**data)
        assert p2.fleet is not None
        assert p2.fleet.count == 5
        assert p2.fleet.hosts == ["h1", "h2"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_schema.py::TestFleetFieldOnModels -v`

Expected: FAIL — `fleet` field not defined on models yet.

- [ ] **Step 3: Add `fleet` field to all 10 item models**

In `src/yoinkc/schema.py`, add `fleet: Optional[FleetPrevalence] = None` as the last field in each of these models: `PackageEntry`, `RepoFile`, `ConfigFileEntry`, `ServiceStateChange`, `SystemdDropIn`, `FirewallZone`, `GeneratedTimerUnit`, `QuadletUnit`, `ComposeFile`, `CronJob`.

Example for `PackageEntry`:

```python
class PackageEntry(BaseModel):
    """Single package from rpm -qa or baseline diff."""

    name: str
    epoch: str = "0"
    version: str
    release: str
    arch: str
    state: PackageState = PackageState.ADDED
    include: bool = True
    source_repo: str = ""
    fleet: Optional[FleetPrevalence] = None
```

Repeat the same pattern (add `fleet: Optional[FleetPrevalence] = None` as the last field) for the other 9 models.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_schema.py -v`

Expected: PASS

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass. The new field defaults to `None`, so existing code is unaffected.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/schema.py tests/test_fleet_schema.py
git commit -m "feat(schema): Add fleet field to 10 item models for prevalence metadata

Assisted-by: Cursor (<model>)"
```

### Task 3: Snapshot loader — discover and load from directory

**Files:**
- Create: `src/yoinkc/fleet/__init__.py`
- Create: `src/yoinkc/fleet/loader.py`
- Create: `tests/test_fleet_loader.py`

- [ ] **Step 1: Create the fleet package**

Create `src/yoinkc/fleet/__init__.py`:

```python
"""Fleet aggregation — merge N inspection snapshots into one."""
```

- [ ] **Step 2: Write failing tests for the loader**

```python
# tests/test_fleet_loader.py
"""Tests for fleet snapshot loader."""

import json
import tarfile
import tempfile
from pathlib import Path

import pytest

from yoinkc.schema import InspectionSnapshot, OsRelease, SCHEMA_VERSION


class TestDiscoverSnapshots:
    """Test snapshot discovery from a directory."""

    def test_finds_json_files(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        (tmp_path / "web-01.json").write_text(snap.model_dump_json())
        (tmp_path / "web-02.json").write_text(snap.model_dump_json())
        results = discover_snapshots(tmp_path)
        assert len(results) == 2

    def test_finds_tarballs(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        tarball_path = tmp_path / "web-01.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            data = snap.model_dump_json().encode()
            import io
            info = tarfile.TarInfo(name="inspection-snapshot.json")
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        results = discover_snapshots(tmp_path)
        assert len(results) == 1

    def test_empty_directory_returns_empty(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_non_snapshot_files(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        (tmp_path / "readme.txt").write_text("not a snapshot")
        (tmp_path / "data.csv").write_text("a,b,c")
        results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_fleet_snapshot_output(self, tmp_path):
        """Prevent re-runs from ingesting previous output as input."""
        from yoinkc.fleet.loader import discover_snapshots
        snap = InspectionSnapshot(meta={"hostname": "web-01"})
        (tmp_path / "web-01.json").write_text(snap.model_dump_json())
        (tmp_path / "fleet-snapshot.json").write_text(snap.model_dump_json())
        results = discover_snapshots(tmp_path)
        assert len(results) == 1  # fleet-snapshot.json excluded

    def test_skips_invalid_json_with_warning(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        (tmp_path / "bad.json").write_text("not valid json {{{")
        results = discover_snapshots(tmp_path)
        assert results == []

    def test_skips_tarball_without_snapshot_json(self, tmp_path):
        from yoinkc.fleet.loader import discover_snapshots
        tarball_path = tmp_path / "empty.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            import io
            info = tarfile.TarInfo(name="Containerfile")
            data = b"FROM scratch"
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        results = discover_snapshots(tmp_path)
        assert results == []


class TestValidateSnapshots:
    """Test input validation across loaded snapshots."""

    def _make_snap(self, hostname="web-01", os_id="rhel", os_version="9.4",
                   base_image="quay.io/centos-bootc/centos-bootc:stream9"):
        from yoinkc.schema import RpmSection
        return InspectionSnapshot(
            meta={"hostname": hostname},
            os_release=OsRelease(name="RHEL", version_id=os_version, id=os_id),
            rpm=RpmSection(base_image=base_image),
        )

    def test_valid_snapshots_pass(self):
        from yoinkc.fleet.loader import validate_snapshots
        snaps = [self._make_snap("web-01"), self._make_snap("web-02")]
        validate_snapshots(snaps)  # should not raise

    def test_schema_version_mismatch_raises(self):
        from yoinkc.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01")
        s2 = self._make_snap("web-02")
        s2.schema_version = 999
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_os_release_mismatch_raises(self):
        from yoinkc.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01", os_id="rhel")
        s2 = self._make_snap("web-02", os_id="centos")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_base_image_mismatch_raises(self):
        from yoinkc.fleet.loader import validate_snapshots
        s1 = self._make_snap("web-01", base_image="image-a")
        s2 = self._make_snap("web-02", base_image="image-b")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])

    def test_fewer_than_two_raises(self):
        from yoinkc.fleet.loader import validate_snapshots
        with pytest.raises(SystemExit):
            validate_snapshots([self._make_snap()])

    def test_duplicate_hostnames_warns(self):
        from yoinkc.fleet.loader import validate_snapshots
        snaps = [self._make_snap("web-01"), self._make_snap("web-01")]
        with pytest.warns(UserWarning, match="Duplicate hostname"):
            validate_snapshots(snaps)  # warns but does not error

    def test_missing_os_release_raises(self):
        from yoinkc.fleet.loader import validate_snapshots
        s1 = InspectionSnapshot(meta={"hostname": "web-01"})
        s2 = self._make_snap("web-02")
        with pytest.raises(SystemExit):
            validate_snapshots([s1, s2])
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_loader.py -v`

Expected: FAIL — `yoinkc.fleet.loader` module does not exist yet.

- [ ] **Step 4: Implement the loader**

Create `src/yoinkc/fleet/loader.py`:

```python
"""Discover and load inspection snapshots from a directory of tarballs/JSON files."""

import io
import json
import sys
import tarfile
import warnings
from pathlib import Path

from ..schema import InspectionSnapshot


def _load_from_json(path: Path) -> InspectionSnapshot | None:
    """Load a snapshot from a bare JSON file. Returns None on failure."""
    try:
        data = json.loads(path.read_text())
        return InspectionSnapshot(**data)
    except Exception as exc:
        warnings.warn(f"Skipping invalid JSON {path.name}: {exc}", stacklevel=2)
        return None


def _load_from_tarball(path: Path) -> InspectionSnapshot | None:
    """Extract inspection-snapshot.json from a tarball. Returns None on failure."""
    try:
        with tarfile.open(path, "r:gz") as tar:
            for member in tar.getmembers():
                if member.name.endswith("inspection-snapshot.json"):
                    f = tar.extractfile(member)
                    if f is None:
                        continue
                    data = json.loads(f.read())
                    return InspectionSnapshot(**data)
        warnings.warn(
            f"Skipping tarball {path.name}: no inspection-snapshot.json found",
            stacklevel=2,
        )
        return None
    except Exception as exc:
        warnings.warn(f"Skipping tarball {path.name}: {exc}", stacklevel=2)
        return None


def discover_snapshots(input_dir: Path) -> list[InspectionSnapshot]:
    """Scan a directory for tarballs and JSON files, return loaded snapshots.

    Invalid files are skipped with a warning.
    """
    snapshots: list[InspectionSnapshot] = []
    for path in sorted(input_dir.iterdir()):
        if path.name == "fleet-snapshot.json":
            continue  # skip previous output to prevent self-contamination
        if path.suffix == ".gz" and path.name.endswith(".tar.gz"):
            snap = _load_from_tarball(path)
        elif path.suffix == ".json":
            snap = _load_from_json(path)
        else:
            continue
        if snap is not None:
            snapshots.append(snap)
    return snapshots


def validate_snapshots(snapshots: list[InspectionSnapshot]) -> None:
    """Validate that all snapshots are compatible for merging.

    Checks: minimum count, schema version, os_release, base_image.
    Exits with error message on failure.
    """
    if len(snapshots) < 2:
        print(f"Error: Need at least 2 snapshots, found {len(snapshots)}.", file=sys.stderr)
        sys.exit(1)

    # Schema version
    versions = {s.schema_version for s in snapshots}
    if len(versions) > 1:
        print(f"Error: Schema version mismatch: {versions}", file=sys.stderr)
        sys.exit(1)

    # Duplicate hostnames (warn, not error)
    hostnames = [s.meta.get("hostname", "") for s in snapshots]
    seen = set()
    for h in hostnames:
        if h in seen:
            warnings.warn(f"Duplicate hostname: {h}", stacklevel=2)
        seen.add(h)

    # os_release — require present on all snapshots
    for s in snapshots:
        if not s.os_release:
            hostname = s.meta.get("hostname", "unknown")
            print(f"Error: Snapshot from {hostname} has no os_release.", file=sys.stderr)
            sys.exit(1)

    os_ids = {s.os_release.id for s in snapshots}
    if len(os_ids) > 1:
        print(f"Error: os_release.id mismatch: {os_ids}", file=sys.stderr)
        sys.exit(1)
    os_versions = {s.os_release.version_id for s in snapshots}
    if len(os_versions) > 1:
        print(f"Error: os_release.version_id mismatch: {os_versions}", file=sys.stderr)
        sys.exit(1)

    # base_image
    base_images = set()
    for s in snapshots:
        if s.rpm and s.rpm.base_image:
            base_images.add(s.rpm.base_image)
    if len(base_images) > 1:
        print(f"Error: rpm.base_image mismatch: {base_images}", file=sys.stderr)
        sys.exit(1)
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_loader.py -v`

Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/fleet/__init__.py src/yoinkc/fleet/loader.py tests/test_fleet_loader.py
git commit -m "feat(fleet): Add snapshot loader with tarball/JSON discovery and validation

Assisted-by: Cursor (<model>)"
```

---

## Chunk 2: Merge Engine

### Task 4: Identity-only merge functions

**Files:**
- Create: `src/yoinkc/fleet/merge.py`
- Create: `tests/test_fleet_merge.py`

- [ ] **Step 1: Write failing tests for identity-only merge**

```python
# tests/test_fleet_merge.py
"""Tests for fleet merge engine."""

from yoinkc.schema import (
    InspectionSnapshot, RpmSection, PackageEntry, RepoFile,
    ServiceSection, ServiceStateChange, NetworkSection, FirewallZone,
    ScheduledTaskSection, GeneratedTimerUnit, CronJob,
    FleetPrevalence, OsRelease,
)


def _snap(hostname="web-01", **kwargs):
    """Helper to build a minimal snapshot."""
    return InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
        **kwargs,
    )


class TestMergePackages:
    def test_identical_packages_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].name == "httpd"
        assert merged.rpm.packages_added[0].fleet.count == 2
        assert merged.rpm.packages_added[0].fleet.total == 2
        assert merged.rpm.packages_added[0].include is True

    def test_different_packages_both_present(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        names = {p.name for p in merged.rpm.packages_added}
        assert names == {"httpd", "nginx"}
        # At 100% threshold, items on only 1/2 hosts are excluded
        for p in merged.rpm.packages_added:
            assert p.fleet.count == 1
            assert p.include is False

    def test_prevalence_threshold_50(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=50)
        # At 50%, 1/2 = 50% meets threshold
        for p in merged.rpm.packages_added:
            assert p.include is True

    def test_package_identity_by_name_not_version(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4.51", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4.53", release="2", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].fleet.count == 2


class TestMergeServices:
    def test_identical_services_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        sc = ServiceStateChange(
            unit="httpd.service", current_state="enabled",
            default_state="disabled", action="enable",
        )
        s1 = _snap("web-01", services=ServiceSection(state_changes=[sc]))
        s2 = _snap("web-02", services=ServiceSection(state_changes=[sc]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.services.state_changes) == 1
        assert merged.services.state_changes[0].fleet.count == 2

    def test_service_identity_includes_action(self):
        from yoinkc.fleet.merge import merge_snapshots
        sc_enable = ServiceStateChange(
            unit="httpd.service", current_state="enabled",
            default_state="disabled", action="enable",
        )
        sc_disable = ServiceStateChange(
            unit="httpd.service", current_state="disabled",
            default_state="enabled", action="disable",
        )
        s1 = _snap("web-01", services=ServiceSection(state_changes=[sc_enable]))
        s2 = _snap("web-02", services=ServiceSection(state_changes=[sc_disable]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.services.state_changes) == 2


class TestMergeFirewallZones:
    def test_identical_zones_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        z = FirewallZone(path="/etc/firewalld/zones/public.xml", name="public")
        s1 = _snap("web-01", network=NetworkSection(firewall_zones=[z]))
        s2 = _snap("web-02", network=NetworkSection(firewall_zones=[z]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.network.firewall_zones) == 1
        assert merged.network.firewall_zones[0].fleet.count == 2


class TestMergeFleetMeta:
    def test_fleet_meta_in_merged_snapshot(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=90)
        fleet_meta = merged.meta.get("fleet")
        assert fleet_meta is not None
        assert fleet_meta["total_hosts"] == 2
        assert fleet_meta["min_prevalence"] == 90
        assert set(fleet_meta["source_hosts"]) == {"web-01", "web-02"}

    def test_merged_hostname_synthetic(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=100, fleet_name="web-servers")
        assert merged.meta["hostname"] == "web-servers"


class TestMergeNoneSection:
    def test_one_snapshot_missing_rpm(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02")  # no rpm section
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm is not None
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].fleet.count == 1

    def test_all_snapshots_missing_section(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm is None
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_merge.py -v`

Expected: FAIL — `yoinkc.fleet.merge` module does not exist.

- [ ] **Step 3: Implement the merge engine**

Create `src/yoinkc/fleet/merge.py`:

```python
"""Core fleet merge engine — union with prevalence filtering."""

import hashlib
from collections import defaultdict
from datetime import datetime, timezone

from ..schema import (
    InspectionSnapshot, FleetPrevalence, FleetMeta,
    RpmSection, ConfigSection, ServiceSection, NetworkSection,
    ScheduledTaskSection, ContainerSection, UserGroupSection,
    PackageEntry, RepoFile, ConfigFileEntry,
    ServiceStateChange, SystemdDropIn,
    FirewallZone, GeneratedTimerUnit, CronJob,
    QuadletUnit, ComposeFile,
)


def _prevalence_include(count: int, total: int, min_prevalence: int) -> bool:
    """Return True if count/total meets the min_prevalence threshold."""
    return (count * 100) >= (min_prevalence * total)


def _content_hash(text: str) -> str:
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def _merge_identity_items(
    all_items: list[list],
    key_fn,
    total: int,
    min_prevalence: int,
    host_names: list[str],
) -> list:
    """Merge items keyed by identity only (not content)."""
    seen: dict[str, dict] = {}  # key -> {"item": model, "hosts": [hostname]}
    for snapshot_idx, items in enumerate(all_items):
        hostname = host_names[snapshot_idx]
        for item in items:
            k = key_fn(item)
            if k not in seen:
                seen[k] = {"item": item, "hosts": [hostname]}
            else:
                seen[k]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"].model_copy()
        count = len(entry["hosts"])
        item.fleet = FleetPrevalence(count=count, total=total, hosts=entry["hosts"])
        if hasattr(item, "include"):
            item.include = _prevalence_include(count, total, min_prevalence)
        result.append(item)
    return result


def _merge_content_items(
    all_items: list[list],
    identity_fn,
    variant_fn,
    total: int,
    min_prevalence: int,
    host_names: list[str],
) -> list:
    """Merge items with content variants — each (identity, variant) pair is separate."""
    seen: dict[tuple[str, str], dict] = {}
    for snapshot_idx, items in enumerate(all_items):
        hostname = host_names[snapshot_idx]
        for item in items:
            ik = identity_fn(item)
            vk = variant_fn(item)
            key = (ik, vk)
            if key not in seen:
                seen[key] = {"item": item, "hosts": [hostname]}
            else:
                seen[key]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"].model_copy()
        count = len(entry["hosts"])
        item.fleet = FleetPrevalence(count=count, total=total, hosts=entry["hosts"])
        if hasattr(item, "include"):
            item.include = _prevalence_include(count, total, min_prevalence)
        result.append(item)
    return result


def _deduplicate_strings(all_lists: list[list[str]]) -> list[str]:
    """Union of string lists, preserving first-seen order."""
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            if item not in seen:
                seen.add(item)
                result.append(item)
    return result


def _deduplicate_dicts(
    all_lists: list[list[dict]],
    key_field: str,
    total: int,
    host_names: list[str],
) -> list[dict]:
    """Deduplicate dicts by a key field, inject fleet prevalence as dict key."""
    seen: dict[str, dict] = {}
    for snapshot_idx, items in enumerate(all_lists):
        hostname = host_names[snapshot_idx]
        for item in items:
            k = item.get(key_field, "")
            if k not in seen:
                seen[k] = {"item": dict(item), "hosts": [hostname]}
            else:
                seen[k]["hosts"].append(hostname)

    result = []
    for entry in seen.values():
        item = entry["item"]
        item["fleet"] = {"count": len(entry["hosts"]), "total": total}
        result.append(item)
    return result


def _deduplicate_warning_dicts(all_lists: list[list[dict]]) -> list[dict]:
    """Deduplicate warning/redaction dicts by (source, message) tuple."""
    seen = set()
    result = []
    for items in all_lists:
        for item in items:
            key = (item.get("source", ""), item.get("message", ""))
            if key not in seen:
                seen.add(key)
                result.append(item)
    return result


def _collect_section_lists(snapshots, section_attr, list_attr):
    """Collect a list field from each snapshot's section, returning [] for missing."""
    result = []
    for s in snapshots:
        section = getattr(s, section_attr, None)
        if section is not None:
            result.append(getattr(section, list_attr, []) or [])
        else:
            result.append([])
    return result


def merge_snapshots(
    snapshots: list[InspectionSnapshot],
    min_prevalence: int = 100,
    fleet_name: str = "fleet-merged",
    include_hosts: bool = True,
) -> InspectionSnapshot:
    """Merge N snapshots into a single fleet snapshot with prevalence metadata."""
    total = len(snapshots)
    host_names = [s.meta.get("hostname", f"host-{i}") for i, s in enumerate(snapshots)]

    # --- RPM ---
    rpm_section = None
    has_rpm = any(s.rpm for s in snapshots)
    if has_rpm:
        packages_added = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "packages_added"),
            key_fn=lambda p: p.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        base_image_only = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "base_image_only"),
            key_fn=lambda p: p.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        repo_files = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "repo_files"),
            key_fn=lambda r: r.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        gpg_keys = _merge_identity_items(
            _collect_section_lists(snapshots, "rpm", "gpg_keys"),
            key_fn=lambda r: r.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        dnf_removed = _deduplicate_strings(
            _collect_section_lists(snapshots, "rpm", "dnf_history_removed")
        )
        # Pass-through fields from first snapshot with rpm
        first_rpm = next(s.rpm for s in snapshots if s.rpm)
        rpm_section = RpmSection(
            packages_added=packages_added,
            base_image_only=base_image_only,
            repo_files=repo_files,
            gpg_keys=gpg_keys,
            dnf_history_removed=dnf_removed,
            base_image=first_rpm.base_image,
            baseline_package_names=first_rpm.baseline_package_names,
            no_baseline=first_rpm.no_baseline,
        )

    # --- Config ---
    config_section = None
    has_config = any(s.config for s in snapshots)
    if has_config:
        files = _merge_content_items(
            _collect_section_lists(snapshots, "config", "files"),
            identity_fn=lambda f: f.path,
            variant_fn=lambda f: _content_hash(f.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        config_section = ConfigSection(files=files)

    # --- Services ---
    services_section = None
    has_services = any(s.services for s in snapshots)
    if has_services:
        state_changes = _merge_identity_items(
            _collect_section_lists(snapshots, "services", "state_changes"),
            key_fn=lambda sc: f"{sc.unit}:{sc.action}",
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        drop_ins = _merge_content_items(
            _collect_section_lists(snapshots, "services", "drop_ins"),
            identity_fn=lambda d: d.path,
            variant_fn=lambda d: _content_hash(d.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        enabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "enabled_units")
        )
        disabled_units = _deduplicate_strings(
            _collect_section_lists(snapshots, "services", "disabled_units")
        )
        services_section = ServiceSection(
            state_changes=state_changes,
            drop_ins=drop_ins,
            enabled_units=enabled_units,
            disabled_units=disabled_units,
        )

    # --- Network (firewall zones only) ---
    network_section = None
    has_network = any(s.network for s in snapshots)
    if has_network:
        firewall_zones = _merge_identity_items(
            _collect_section_lists(snapshots, "network", "firewall_zones"),
            key_fn=lambda z: z.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        network_section = NetworkSection(firewall_zones=firewall_zones)

    # --- Scheduled Tasks ---
    sched_section = None
    has_sched = any(s.scheduled_tasks for s in snapshots)
    if has_sched:
        gen_timers = _merge_identity_items(
            _collect_section_lists(snapshots, "scheduled_tasks", "generated_timer_units"),
            key_fn=lambda t: t.name,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        cron_jobs = _merge_identity_items(
            _collect_section_lists(snapshots, "scheduled_tasks", "cron_jobs"),
            key_fn=lambda c: c.path,
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        systemd_timers_all = _collect_section_lists(snapshots, "scheduled_tasks", "systemd_timers")
        # Deduplicate systemd timers by name — these are model objects, convert via first-seen
        timer_seen: dict[str, object] = {}
        for items in systemd_timers_all:
            for t in items:
                if t.name not in timer_seen:
                    timer_seen[t.name] = t
        sched_section = ScheduledTaskSection(
            generated_timer_units=gen_timers,
            cron_jobs=cron_jobs,
            systemd_timers=list(timer_seen.values()),
        )

    # --- Containers ---
    containers_section = None
    has_containers = any(s.containers for s in snapshots)
    if has_containers:
        quadlet_units = _merge_content_items(
            _collect_section_lists(snapshots, "containers", "quadlet_units"),
            identity_fn=lambda q: q.path,
            variant_fn=lambda q: _content_hash(q.content),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        compose_files = _merge_content_items(
            _collect_section_lists(snapshots, "containers", "compose_files"),
            identity_fn=lambda c: c.path,
            variant_fn=lambda c: _content_hash(
                str(sorted((img.service, img.image) for img in c.images))
            ),
            total=total, min_prevalence=min_prevalence, host_names=host_names,
        )
        containers_section = ContainerSection(
            quadlet_units=quadlet_units,
            compose_files=compose_files,
        )

    # --- Users/Groups ---
    ug_section = None
    has_ug = any(s.users_groups for s in snapshots)
    if has_ug:
        users = _deduplicate_dicts(
            _collect_section_lists(snapshots, "users_groups", "users"),
            key_field="name", total=total, host_names=host_names,
        )
        groups = _deduplicate_dicts(
            _collect_section_lists(snapshots, "users_groups", "groups"),
            key_field="name", total=total, host_names=host_names,
        )
        sudoers = _deduplicate_strings(
            _collect_section_lists(snapshots, "users_groups", "sudoers_rules")
        )
        ug_section = UserGroupSection(
            users=users,
            groups=groups,
            sudoers_rules=sudoers,
        )

    # --- Warnings / Redactions ---
    warnings_merged = _deduplicate_warning_dicts(
        [s.warnings for s in snapshots]
    )
    redactions_merged = _deduplicate_warning_dicts(
        [s.redactions for s in snapshots]
    )

    # --- Fleet metadata ---
    fleet_meta = FleetMeta(
        source_hosts=host_names,
        total_hosts=total,
        min_prevalence=min_prevalence,
    )

    # --- Build merged snapshot ---
    first = snapshots[0]
    merged = InspectionSnapshot(
        meta={
            "hostname": fleet_name,
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "fleet": fleet_meta.model_dump(),
        },
        os_release=first.os_release,
        rpm=rpm_section,
        config=config_section,
        services=services_section,
        network=network_section,
        scheduled_tasks=sched_section,
        containers=containers_section,
        users_groups=ug_section,
        warnings=warnings_merged,
        redactions=redactions_merged,
        # Omitted sections: storage, kernel_boot, selinux, non_rpm_software
    )

    if not include_hosts:
        _strip_host_lists(merged)

    return merged


def _strip_host_lists(snapshot: InspectionSnapshot) -> None:
    """Remove per-item host lists from fleet metadata (privacy mode)."""
    for section_name in ["rpm", "config", "services", "network",
                         "scheduled_tasks", "containers"]:
        section = getattr(snapshot, section_name, None)
        if section is None:
            continue
        for field_name in section.model_fields:
            items = getattr(section, field_name, None)
            if not isinstance(items, list):
                continue
            for item in items:
                if hasattr(item, "fleet") and item.fleet is not None:
                    item.fleet.hosts = []
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_merge.py -v`

Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/fleet/merge.py tests/test_fleet_merge.py
git commit -m "feat(fleet): Implement merge engine with identity/content merge and prevalence filtering

Assisted-by: Cursor (<model>)"
```

### Task 5: Content-variant merge tests

**Files:**
- Modify: `tests/test_fleet_merge.py`

- [ ] **Step 1: Add tests for content-bearing item variants**

Append to `tests/test_fleet_merge.py`:

```python
from yoinkc.schema import (
    ConfigSection, ConfigFileEntry, ContainerSection,
    QuadletUnit, ComposeFile, ComposeService,
    SystemdDropIn,
)


class TestMergeConfigVariants:
    def test_identical_config_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        f = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 80")
        s1 = _snap("web-01", config=ConfigSection(files=[f]))
        s2 = _snap("web-02", config=ConfigSection(files=[f]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.config.files) == 1
        assert merged.config.files[0].fleet.count == 2

    def test_different_content_produces_variants(self):
        from yoinkc.fleet.merge import merge_snapshots
        f1 = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 80")
        f2 = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 8080")
        s1 = _snap("web-01", config=ConfigSection(files=[f1]))
        s2 = _snap("web-02", config=ConfigSection(files=[f2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.config.files) == 2
        paths = {f.path for f in merged.config.files}
        assert paths == {"/etc/httpd/conf/httpd.conf"}
        for f in merged.config.files:
            assert f.fleet.count == 1
            assert f.include is False  # 1/2 < 100%

    def test_majority_variant_included_at_threshold(self):
        from yoinkc.fleet.merge import merge_snapshots
        f_majority = ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="version=A")
        f_outlier = ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="version=B")
        snaps = [_snap(f"web-{i:02d}", config=ConfigSection(files=[f_majority])) for i in range(9)]
        snaps.append(_snap("web-09", config=ConfigSection(files=[f_outlier])))
        merged = merge_snapshots(snaps, min_prevalence=90)
        included = [f for f in merged.config.files if f.include]
        excluded = [f for f in merged.config.files if not f.include]
        assert len(included) == 1
        assert included[0].content == "version=A"
        assert len(excluded) == 1
        assert excluded[0].content == "version=B"


class TestMergeQuadletVariants:
    def test_quadlet_content_variants(self):
        from yoinkc.fleet.merge import merge_snapshots
        q1 = QuadletUnit(path="/etc/containers/systemd/app.container", name="app", content="[Container]\nImage=v1")
        q2 = QuadletUnit(path="/etc/containers/systemd/app.container", name="app", content="[Container]\nImage=v2")
        s1 = _snap("web-01", containers=ContainerSection(quadlet_units=[q1]))
        s2 = _snap("web-02", containers=ContainerSection(quadlet_units=[q2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.containers.quadlet_units) == 2


class TestMergeUsersGroups:
    def test_users_deduplicated_by_name(self):
        from yoinkc.fleet.merge import merge_snapshots
        ug1 = UserGroupSection(users=[{"name": "appuser", "uid": 1000}])
        ug2 = UserGroupSection(users=[{"name": "appuser", "uid": 1000}])
        s1 = _snap("web-01", users_groups=ug1)
        s2 = _snap("web-02", users_groups=ug2)
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.users_groups.users) == 1
        assert merged.users_groups.users[0]["fleet"]["count"] == 2


class TestMergeWarnings:
    def test_warnings_deduplicated(self):
        from yoinkc.fleet.merge import merge_snapshots
        w = {"source": "rpm", "message": "package conflict detected"}
        s1 = _snap("web-01")
        s1.warnings = [w]
        s2 = _snap("web-02")
        s2.warnings = [w, {"source": "config", "message": "orphaned file"}]
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.warnings) == 2


class TestNoHostsMode:
    def test_strip_host_lists(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100, include_hosts=False)
        assert merged.rpm.packages_added[0].fleet.hosts == []
        assert merged.rpm.packages_added[0].fleet.count == 2
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_merge.py -v`

Expected: PASS (the merge engine from Task 4 already handles these cases).

- [ ] **Step 3: Commit**

```bash
git add tests/test_fleet_merge.py
git commit -m "test(fleet): Add content-variant, users/groups, warnings, and no-hosts merge tests

Assisted-by: Cursor (<model>)"
```

---

## Chunk 3: CLI, Entry Point, and Integration

### Task 6: CLI argument parsing

**Files:**
- Create: `src/yoinkc/fleet/cli.py`
- Create: `tests/test_fleet_cli.py`

- [ ] **Step 1: Write failing tests for CLI parsing**

```python
# tests/test_fleet_cli.py
"""Tests for yoinkc-fleet CLI."""

import pytest


class TestFleetCliParsing:
    def test_aggregate_requires_input_dir(self):
        from yoinkc.fleet.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args([])

    def test_aggregate_basic(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path)])
        assert args.input_dir == tmp_path
        assert args.min_prevalence == 100
        assert args.no_hosts is False

    def test_aggregate_with_prevalence(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "-p", "80"])
        assert args.min_prevalence == 80

    def test_aggregate_with_output(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "-o", "/tmp/merged.json"])
        assert str(args.output) == "/tmp/merged.json"

    def test_aggregate_no_hosts_flag(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        args = parse_args(["aggregate", str(tmp_path), "--no-hosts"])
        assert args.no_hosts is True

    def test_prevalence_out_of_range(self, tmp_path):
        from yoinkc.fleet.cli import parse_args
        with pytest.raises(SystemExit):
            parse_args(["aggregate", str(tmp_path), "-p", "0"])
        with pytest.raises(SystemExit):
            parse_args(["aggregate", str(tmp_path), "-p", "101"])
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_cli.py -v`

Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement CLI parsing**

Create `src/yoinkc/fleet/cli.py`:

```python
"""CLI for yoinkc-fleet."""

import argparse
from pathlib import Path
from typing import Optional


def parse_args(argv: Optional[list[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="yoinkc-fleet",
        description="Fleet-level analysis of yoinkc inspection snapshots.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    agg = sub.add_parser("aggregate", help="Merge N snapshots into a fleet snapshot")
    agg.add_argument(
        "input_dir",
        type=Path,
        help="Directory containing yoinkc tarballs (.tar.gz) and/or JSON snapshot files",
    )
    agg.add_argument(
        "-p", "--min-prevalence",
        type=int,
        default=100,
        metavar="PCT",
        help="Include items present on >= PCT%% of hosts (default: 100)",
    )
    agg.add_argument(
        "-o", "--output",
        type=Path,
        default=None,
        metavar="FILE",
        help="Output path for merged snapshot (default: <input-dir>/fleet-snapshot.json)",
    )
    agg.add_argument(
        "--no-hosts",
        action="store_true",
        help="Omit per-item host lists from fleet metadata",
    )

    args = parser.parse_args(argv)

    if hasattr(args, "min_prevalence"):
        if not (1 <= args.min_prevalence <= 100):
            parser.error("--min-prevalence must be between 1 and 100")

    return args
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_fleet_cli.py -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/fleet/cli.py tests/test_fleet_cli.py
git commit -m "feat(fleet): Add CLI argument parsing for yoinkc-fleet aggregate

Assisted-by: Cursor (<model>)"
```

### Task 7: Entry point and integration

**Files:**
- Create: `src/yoinkc/fleet/__main__.py`
- Modify: `src/yoinkc/fleet/__init__.py`
- Modify: `pyproject.toml`
- Modify: `tests/test_fleet_cli.py`

- [ ] **Step 1: Write failing end-to-end test**

Append to `tests/test_fleet_cli.py`:

```python
import json
import tarfile
import io
from pathlib import Path
from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry


def _make_tarball(tmp_path, hostname, packages):
    """Create a test tarball with the given packages."""
    snap = InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
        rpm=RpmSection(
            packages_added=[
                PackageEntry(name=n, version="1.0", release="1", arch="x86_64")
                for n in packages
            ],
            base_image="quay.io/centos-bootc/centos-bootc:stream9",
        ),
    )
    tarball_path = tmp_path / f"{hostname}.tar.gz"
    with tarfile.open(tarball_path, "w:gz") as tar:
        data = snap.model_dump_json().encode()
        info = tarfile.TarInfo(name="inspection-snapshot.json")
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
    return tarball_path


class TestFleetEndToEnd:
    def test_aggregate_produces_valid_snapshot(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd", "php"])
        _make_tarball(tmp_path, "web-02", ["httpd", "mod_ssl"])

        output = tmp_path / "merged.json"
        exit_code = main(["aggregate", str(tmp_path), "-o", str(output), "-p", "50"])
        assert exit_code == 0

        data = json.loads(output.read_text())
        snap = InspectionSnapshot(**data)
        assert snap.meta["fleet"]["total_hosts"] == 2
        pkg_names = {p.name for p in snap.rpm.packages_added}
        assert "httpd" in pkg_names
        assert "php" in pkg_names
        assert "mod_ssl" in pkg_names

        httpd = next(p for p in snap.rpm.packages_added if p.name == "httpd")
        assert httpd.fleet.count == 2
        assert httpd.include is True

    def test_aggregate_default_output_path(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd"])
        _make_tarball(tmp_path, "web-02", ["httpd"])

        exit_code = main(["aggregate", str(tmp_path)])
        assert exit_code == 0
        assert (tmp_path / "fleet-snapshot.json").exists()

    def test_aggregate_fewer_than_two_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        _make_tarball(tmp_path, "web-01", ["httpd"])
        with pytest.raises(SystemExit):
            main(["aggregate", str(tmp_path)])

    def test_aggregate_empty_dir_exits(self, tmp_path):
        from yoinkc.fleet.__main__ import main
        with pytest.raises(SystemExit):
            main(["aggregate", str(tmp_path)])
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_fleet_cli.py::TestFleetEndToEnd -v`

Expected: FAIL — `__main__` module does not exist.

- [ ] **Step 3: Implement entry point**

Create `src/yoinkc/fleet/__main__.py`:

```python
"""Entry point for yoinkc-fleet CLI."""

import json
import sys
from pathlib import Path
from typing import Optional

from .cli import parse_args
from .loader import discover_snapshots, validate_snapshots
from .merge import merge_snapshots


def main(argv: Optional[list[str]] = None) -> int:
    args = parse_args(argv)

    input_dir: Path = args.input_dir
    if not input_dir.is_dir():
        print(f"Error: {input_dir} is not a directory.", file=sys.stderr)
        sys.exit(1)

    snapshots = discover_snapshots(input_dir)
    if len(snapshots) < 2:
        print(
            f"Error: Need at least 2 snapshots, found {len(snapshots)} in {input_dir}.",
            file=sys.stderr,
        )
        sys.exit(1)

    validate_snapshots(snapshots)

    fleet_name = input_dir.resolve().name
    merged = merge_snapshots(
        snapshots,
        min_prevalence=args.min_prevalence,
        fleet_name=fleet_name,
        include_hosts=not args.no_hosts,
    )

    output_path = args.output or (input_dir / "fleet-snapshot.json")
    output_path.write_text(merged.model_dump_json(indent=2))
    print(f"Fleet snapshot written to {output_path}")
    print(f"  {len(snapshots)} hosts merged, threshold {args.min_prevalence}%")

    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Update `__init__.py` to export `merge_snapshots`**

Update `src/yoinkc/fleet/__init__.py`:

```python
"""Fleet aggregation — merge N inspection snapshots into one."""

from .merge import merge_snapshots

__all__ = ["merge_snapshots"]
```

- [ ] **Step 5: Register `yoinkc-fleet` console script in pyproject.toml**

In `pyproject.toml`, change:

```toml
[project.scripts]
yoinkc = "yoinkc.__main__:main"
```

To:

```toml
[project.scripts]
yoinkc = "yoinkc.__main__:main"
yoinkc-fleet = "yoinkc.fleet.__main__:main"
```

- [ ] **Step 6: Run end-to-end tests**

Run: `cd yoinkc && python -m pytest tests/test_fleet_cli.py -v`

Expected: PASS

- [ ] **Step 7: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/yoinkc/fleet/__init__.py src/yoinkc/fleet/__main__.py pyproject.toml tests/test_fleet_cli.py
git commit -m "feat(fleet): Add yoinkc-fleet entry point with end-to-end aggregate command

Assisted-by: Cursor (<model>)"
```

---

## Manual Verification Checklist

After implementation, verify on your workstation:

- [ ] `pip install -e .` installs the `yoinkc-fleet` command
- [ ] `yoinkc-fleet --help` shows usage
- [ ] `yoinkc-fleet aggregate --help` shows aggregate options
- [ ] Create 2+ test tarballs (from real yoinkc runs or test fixtures), place in a directory
- [ ] `yoinkc-fleet aggregate ./test-dir/ -p 90 -o merged.json` produces valid output
- [ ] `python -c "from yoinkc.schema import InspectionSnapshot; import json; s = InspectionSnapshot(**json.load(open('merged.json'))); print(s.meta['fleet'])"` shows fleet metadata
- [ ] `yoinkc --from-snapshot merged.json --output-dir /tmp/fleet-test` renders a Containerfile from the merged snapshot
- [ ] Merged snapshot opened in `yoinkc-refine` shows items with fleet prevalence data in the JSON (even though the UI doesn't render it specially yet)
