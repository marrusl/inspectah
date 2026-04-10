# RPM Preflight Check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Validate that all packages in the generated install list exist in the target repos before rendering the Containerfile, surfacing unavailable packages, direct-install RPMs, and unreachable repos as structured diagnostics.

**Architecture:** New `rpm_preflight.py` module runs a two-phase container-based check (bootstrap repo-providing packages, then `dnf repoquery --available`) after all inspectors complete (so it sees config, kernel_boot, and RPM data). Uses a persistent container (`podman run -d` + `podman exec`) so bootstrap state survives into the repoquery phase. Results are stored in the `InspectionSnapshot` as a `PreflightResult` and consumed by the renderer (to exclude unavailable packages and emit diagnostics) and architect (for fleet aggregation). A shared `resolve_install_set()` function ensures preflight and renderer operate on the same package list.

**Tech Stack:** Python 3.11+, Pydantic v2 (BaseModel), subprocess via Executor pattern, podman for container-based checks, pytest for testing.

**Spec:** `docs/specs/proposed/2026-04-09-rpm-preflight-check-design.md` (revision 3)

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `src/yoinkc/install_set.py` | Shared `resolve_install_set(snapshot) -> list[str]` function — single source of truth for the filtered package list that both preflight and renderer use |
| `src/yoinkc/rpm_preflight.py` | Package availability preflight module — runs container-based `dnf repoquery` check, produces `PreflightResult` |
| `tests/test_install_set.py` | Tests for `resolve_install_set()` |
| `tests/test_rpm_preflight.py` | Tests for the package availability preflight module |

### Modified files

| File | Changes |
|------|---------|
| `src/yoinkc/schema.py` | Add `PreflightResult`, `UnverifiablePackage`, `RepoStatus` models; add `preflight` field to `InspectionSnapshot`; add `repo_providing_packages` field to `RpmSection`; bump `SCHEMA_VERSION` to 11 |
| `src/yoinkc/inspectors/rpm.py` | Add repo-providing package detection (`rpm -qf /etc/yum.repos.d/*.repo`); classify direct-install RPMs |
| `src/yoinkc/cli.py` | Add `--skip-unavailable` flag to `_add_inspect_args` |
| `src/yoinkc/__main__.py` | Thread `skip_unavailable` arg through to `run_all()` |
| `src/yoinkc/inspectors/__init__.py` | Call `run_package_preflight()` after all inspectors complete, store result in snapshot |
| `src/yoinkc/pipeline.py` | Call `emit_preflight_diagnostics()` after secrets summary |
| `src/yoinkc/renderers/containerfile/packages.py` | Use `resolve_install_set()`, consume preflight results, exclude unavailable/direct-install, emit diagnostic block to stderr |
| `src/yoinkc/architect/analyzer.py` | Add preflight data to `FleetInput`, aggregate per base image |
| `tests/test_preflight.py` | Update `_make_inspect_args` with `skip_unavailable` default |

---

### Task 1: Schema — preflight models

**Files:**
- Modify: `src/yoinkc/schema.py:59-180` (RPM section area and root snapshot)
- Test: `tests/test_rpm_preflight.py` (new file)

- [ ] **Step 1: Write failing test for schema models**

Create `tests/test_rpm_preflight.py`:

```python
"""Tests for RPM preflight check: schema, install set, and preflight module."""

from yoinkc.schema import (
    InspectionSnapshot,
    PreflightResult,
    RepoStatus,
    RpmSection,
    UnverifiablePackage,
)


class TestPreflightSchema:
    def test_preflight_result_completed(self):
        result = PreflightResult(
            status="completed",
            available=["httpd", "nginx"],
            unavailable=["mcelog"],
            direct_install=["custom-agent"],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        assert result.status == "completed"
        assert result.status_reason is None
        assert result.unavailable == ["mcelog"]
        assert result.unverifiable == []
        assert result.repo_unreachable == []

    def test_preflight_result_partial_with_unverifiable(self):
        result = PreflightResult(
            status="partial",
            status_reason="repo-providing package epel-release unavailable",
            available=["httpd"],
            unavailable=["mcelog"],
            unverifiable=[
                UnverifiablePackage(
                    name="some-epel-pkg",
                    reason="repo-providing package epel-release unavailable",
                )
            ],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        assert result.status == "partial"
        assert len(result.unverifiable) == 1
        assert result.unverifiable[0].name == "some-epel-pkg"

    def test_preflight_result_skipped(self):
        result = PreflightResult(
            status="skipped",
            status_reason="user passed --skip-unavailable",
        )
        assert result.status == "skipped"
        assert result.available == []

    def test_preflight_result_failed(self):
        result = PreflightResult(
            status="failed",
            status_reason="base image could not be pulled",
        )
        assert result.status == "failed"

    def test_repo_status(self):
        rs = RepoStatus(
            repo_id="internal-mirror",
            repo_name="Internal Mirror",
            error="connection timed out",
            affected_packages=["internal-app", "internal-lib"],
        )
        assert rs.repo_id == "internal-mirror"
        assert len(rs.affected_packages) == 2

    def test_snapshot_has_preflight_field(self):
        snapshot = InspectionSnapshot()
        assert snapshot.preflight is not None
        assert snapshot.preflight.status == "skipped"

    def test_snapshot_preflight_roundtrip(self):
        """Preflight data survives JSON serialization/deserialization."""
        snapshot = InspectionSnapshot(
            preflight=PreflightResult(
                status="completed",
                available=["httpd"],
                unavailable=["mcelog"],
                direct_install=["custom-agent"],
                base_image="quay.io/fedora/fedora-bootc:44",
                repos_queried=["fedora"],
                timestamp="2026-04-09T17:00:00Z",
            )
        )
        json_str = snapshot.model_dump_json()
        loaded = InspectionSnapshot.model_validate_json(json_str)
        assert loaded.preflight.status == "completed"
        assert loaded.preflight.unavailable == ["mcelog"]
        assert loaded.preflight.direct_install == ["custom-agent"]

    def test_rpm_section_has_repo_providing_packages(self):
        section = RpmSection()
        assert section.repo_providing_packages == []

    def test_rpm_section_repo_providing_packages_roundtrip(self):
        section = RpmSection(repo_providing_packages=["epel-release", "rpmfusion-free-release"])
        data = section.model_dump()
        loaded = RpmSection.model_validate(data)
        assert loaded.repo_providing_packages == ["epel-release", "rpmfusion-free-release"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPreflightSchema -v`

Expected: FAIL — `PreflightResult`, `UnverifiablePackage`, `RepoStatus` not importable from `yoinkc.schema`.

- [ ] **Step 3: Implement schema models**

In `src/yoinkc/schema.py`, add after the `RpmVaEntry` class (around line 128) and before `OstreePackageOverride`:

```python
class UnverifiablePackage(BaseModel):
    """A package that could not be checked during preflight."""

    name: str
    reason: str  # e.g., "repo-providing package epel-release unavailable"


class RepoStatus(BaseModel):
    """Status of a repo that could not be queried during preflight."""

    repo_id: str
    repo_name: str
    error: str
    affected_packages: List[str] = Field(default_factory=list)


class PreflightResult(BaseModel):
    """Result of package availability check against target repos."""

    status: str = "skipped"  # "completed", "partial", "skipped", "failed"
    status_reason: Optional[str] = None
    available: List[str] = Field(default_factory=list)
    unavailable: List[str] = Field(default_factory=list)
    unverifiable: List[UnverifiablePackage] = Field(default_factory=list)
    direct_install: List[str] = Field(default_factory=list)
    repo_unreachable: List[RepoStatus] = Field(default_factory=list)
    base_image: str = ""
    repos_queried: List[str] = Field(default_factory=list)
    timestamp: str = ""
```

Add `repo_providing_packages` to `RpmSection` (after `duplicate_packages`):

```python
    repo_providing_packages: List[str] = Field(default_factory=list)
```

Add `preflight` field to `InspectionSnapshot` (after `users_groups`):

```python
    preflight: PreflightResult = Field(default_factory=PreflightResult)
```

Bump `SCHEMA_VERSION` from `10` to `11`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPreflightSchema -v`

Expected: All PASS.

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

Expected: Some tests that hardcode `SCHEMA_VERSION = 10` may break. Fix any snapshot version mismatches in test fixtures by updating `schema_version` values in fixture JSON files. The `load_snapshot` function in `pipeline.py` rejects snapshots with mismatched versions, so any fixture `.json` files need updating.

NOTE: Search for `"schema_version": 10` in `tests/fixtures/` and update to `11`. Also check tests that assert on `SCHEMA_VERSION` directly.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/schema.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): add PreflightResult schema models and snapshot field

Add PreflightResult, UnverifiablePackage, RepoStatus models to schema.
Add preflight field to InspectionSnapshot (defaults to skipped).
Add repo_providing_packages field to RpmSection.
Bump SCHEMA_VERSION to 11."
```

---

### Task 2: Shared resolve_install_set() function

**Files:**
- Create: `src/yoinkc/install_set.py`
- Test: `tests/test_install_set.py` (new file)

- [ ] **Step 1: Write failing tests for resolve_install_set**

Create `tests/test_install_set.py`:

```python
"""Tests for resolve_install_set — the shared package list that preflight and renderer use."""

from yoinkc.install_set import resolve_install_set
from yoinkc.schema import InspectionSnapshot, PackageEntry, PackageState, RpmSection


def _make_snapshot(
    packages=None,
    leaf_packages=None,
    auto_packages=None,
    no_baseline=False,
):
    """Build a minimal snapshot with RPM data for testing install set resolution."""
    entries = []
    for name, include in (packages or []):
        entries.append(PackageEntry(
            name=name,
            epoch="0",
            version="1.0",
            release="1.el9",
            arch="x86_64",
            state=PackageState.ADDED,
            include=include,
        ))
    section = RpmSection(
        packages_added=entries,
        leaf_packages=leaf_packages,
        auto_packages=auto_packages,
        no_baseline=no_baseline,
    )
    return InspectionSnapshot(rpm=section)


def test_basic_all_included():
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", True), ("rsync", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "nginx", "rsync"]


def test_exclude_filter():
    """Packages with include=False are excluded."""
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", False), ("rsync", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert "nginx" not in result
    assert sorted(result) == ["httpd", "rsync"]


def test_leaf_filter_with_baseline():
    """When baseline exists, only leaf packages are included."""
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", True), ("mod_ssl", True)],
        leaf_packages=["httpd", "nginx"],
        auto_packages=["mod_ssl"],
    )
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "nginx"]
    assert "mod_ssl" not in result


def test_no_baseline_includes_all():
    """Without baseline, all included packages are returned regardless of leaf/auto."""
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("nginx", True), ("mod_ssl", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert sorted(result) == ["httpd", "mod_ssl", "nginx"]


def test_shell_unsafe_names_excluded():
    """Package names with unsafe shell characters are excluded."""
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("bad;pkg", True), ("rsync", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert "bad;pkg" not in result
    assert sorted(result) == ["httpd", "rsync"]


def test_empty_rpm_section():
    """Snapshot with no RPM data returns empty list."""
    snapshot = InspectionSnapshot()
    result = resolve_install_set(snapshot)
    assert result == []


def test_no_packages_added():
    """RPM section with no packages_added returns empty list."""
    snapshot = InspectionSnapshot(rpm=RpmSection())
    result = resolve_install_set(snapshot)
    assert result == []


def test_deduplication():
    """Duplicate package names are deduplicated."""
    snapshot = _make_snapshot(
        packages=[("httpd", True), ("httpd", True), ("nginx", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert result.count("httpd") == 1


def test_result_is_sorted():
    """Result list is sorted alphabetically."""
    snapshot = _make_snapshot(
        packages=[("zsh", True), ("apache", True), ("mysql", True)],
        no_baseline=True,
    )
    result = resolve_install_set(snapshot)
    assert result == ["apache", "mysql", "zsh"]


def test_tuned_injected_when_active():
    """tuned is injected when snapshot has an active tuned profile."""
    from yoinkc.schema import KernelBootSection

    snapshot = _make_snapshot(
        packages=[("httpd", True)],
        no_baseline=True,
    )
    snapshot.kernel_boot = KernelBootSection(tuned_active="throughput-performance")
    result = resolve_install_set(snapshot)
    assert "tuned" in result


def test_tuned_not_duplicated():
    """tuned is not duplicated if already in the install set."""
    from yoinkc.schema import KernelBootSection

    snapshot = _make_snapshot(
        packages=[("httpd", True), ("tuned", True)],
        no_baseline=True,
    )
    snapshot.kernel_boot = KernelBootSection(tuned_active="throughput-performance")
    result = resolve_install_set(snapshot)
    assert result.count("tuned") == 1
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_install_set.py -v`

Expected: FAIL — `yoinkc.install_set` does not exist.

- [ ] **Step 3: Implement resolve_install_set**

Create `src/yoinkc/install_set.py`:

```python
"""Shared install-set resolution used by both preflight and the packages renderer.

resolve_install_set() applies the same filters the renderer uses:
  1. p.include filter (user exclusions)
  2. Leaf-package filter (when baseline exists, only explicit installs)
  3. Shell safety filter (reject names with shell metacharacters)
  4. Synthetic prerequisite injection (e.g., tuned)

Both the preflight module and the renderer call this function so they
always operate on the same package list.
"""

from typing import List

from .renderers.containerfile._helpers import _sanitize_shell_value, _TUNED_PROFILE_RE
from .schema import InspectionSnapshot


def resolve_install_set(snapshot: InspectionSnapshot) -> List[str]:
    """Return the sorted, deduplicated list of package names to install.

    This is the exact set the renderer will emit in the ``dnf install``
    line and the preflight module will validate against target repos.
    """
    rpm = snapshot.rpm
    if not rpm or not rpm.packages_added:
        return []

    # 1. Include filter
    included = [p for p in rpm.packages_added if p.include]
    raw_names = sorted(set(p.name for p in included))

    # 2. Shell safety filter
    safe_names = [n for n in raw_names if _sanitize_shell_value(n, "dnf install") is not None]

    # 3. Leaf filter (only when baseline exists)
    leaf_set = set(rpm.leaf_packages) if rpm.leaf_packages is not None else None
    if leaf_set is not None and not getattr(rpm, "no_baseline", False):
        included_name_set = set(raw_names)
        included_leaf_names = leaf_set & included_name_set
        result = sorted(n for n in safe_names if n in included_leaf_names)
    else:
        result = safe_names

    # 4. Synthetic prerequisite: tuned
    needs_tuned = bool(
        snapshot.kernel_boot and snapshot.kernel_boot.tuned_active
        and _TUNED_PROFILE_RE.match(snapshot.kernel_boot.tuned_active)
    )
    if needs_tuned and "tuned" not in result:
        result = sorted(result + ["tuned"])

    return result
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_install_set.py -v`

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/install_set.py tests/test_install_set.py
git commit -m "feat(preflight): add resolve_install_set shared function

Single source of truth for the filtered package list. Applies include,
leaf, and shell-safety filters. Used by both preflight and renderer."
```

---

### Task 3: Refactor renderer to use resolve_install_set()

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/packages.py:208-238`

- [ ] **Step 1: Run existing renderer tests to establish baseline**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_containerfile_output.py tests/test_plan_packages.py -v -q`

Expected: All PASS (establish baseline before refactor).

- [ ] **Step 2: Refactor section_lines to use resolve_install_set**

In `src/yoinkc/renderers/containerfile/packages.py`, replace the inline filtering block (approximately lines 210-238) with a call to `resolve_install_set`:

Replace this block inside `section_lines()`:

```python
        install_names: List[str] = []
        auto_count = 0
        if has_pkgs:
            included_pkgs = [p for p in rpm.packages_added if p.include]
            raw_names = sorted(set(p.name for p in included_pkgs))
            safe_names: List[str] = []
            for n in raw_names:
                if _sanitize_shell_value(n, "dnf install") is not None:
                    safe_names.append(n)
                else:
                    lines.append(f"# FIXME: package name contains unsafe characters, skipped: {n!r}")

            leaf_set = set(rpm.leaf_packages) if rpm.leaf_packages is not None else None
            dep_tree = rpm.leaf_dep_tree or {}
            if leaf_set is not None and not getattr(rpm, "no_baseline", False):
                included_name_set = set(raw_names)
                included_leaf_names = leaf_set & included_name_set
                install_names = [n for n in safe_names if n in included_leaf_names]
                if dep_tree:
                    remaining_auto: set = set()
                    for lf in included_leaf_names:
                        remaining_auto.update(dep_tree.get(lf, []))
                    auto_count = len(remaining_auto)
                else:
                    all_auto = set(rpm.auto_packages) if rpm.auto_packages else set()
                    auto_count = len(all_auto & included_name_set)
            else:
                install_names = safe_names
```

With:

```python
        from ...install_set import resolve_install_set

        install_names: List[str] = []
        auto_count = 0
        if has_pkgs:
            # FIXME comments for unsafe package names
            included_pkgs = [p for p in rpm.packages_added if p.include]
            for p in included_pkgs:
                if _sanitize_shell_value(p.name, "dnf install") is None:
                    lines.append(f"# FIXME: package name contains unsafe characters, skipped: {p.name!r}")

            install_names = resolve_install_set(snapshot)

            # Compute auto_count for the comment (resolve_install_set handles filtering)
            leaf_set = set(rpm.leaf_packages) if rpm.leaf_packages is not None else None
            dep_tree = rpm.leaf_dep_tree or {}
            if leaf_set is not None and not getattr(rpm, "no_baseline", False):
                included_name_set = set(p.name for p in included_pkgs)
                included_leaf_names = leaf_set & included_name_set
                if dep_tree:
                    remaining_auto: set = set()
                    for lf in included_leaf_names:
                        remaining_auto.update(dep_tree.get(lf, []))
                    auto_count = len(remaining_auto)
                else:
                    all_auto = set(rpm.auto_packages) if rpm.auto_packages else set()
                    auto_count = len(all_auto & included_name_set)
```

- [ ] **Step 3: Run existing tests to verify no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_containerfile_output.py tests/test_plan_packages.py tests/test_plan_containerfile.py -v -q`

Expected: All PASS — output should be identical.

- [ ] **Step 4: Commit**

```bash
git add src/yoinkc/renderers/containerfile/packages.py
git commit -m "refactor(renderer): use resolve_install_set for package filtering

Replace inline filtering with shared resolve_install_set() call.
Ensures preflight and renderer always operate on the same package list."
```

---

### Task 4: RPM inspector — repo-providing package detection

**Files:**
- Modify: `src/yoinkc/inspectors/rpm.py:1230-1232` (after repo file collection)
- Test: `tests/test_rpm_preflight.py` (add to existing file)

- [ ] **Step 1: Write failing test**

Append to `tests/test_rpm_preflight.py`:

```python
from pathlib import Path
from yoinkc.executor import RunResult


class TestRepoProvidingPackages:
    def test_detects_repo_providing_packages(self, host_root, fixture_executor):
        """Packages that own .repo files in /etc/yum.repos.d/ are detected."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        # Fixture has repo files; executor returns epel-release as owner
        def executor(cmd, cwd=None):
            cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
            if "rpm" in cmd_str and "-qf" in cmd_str and "yum.repos.d" in cmd_str:
                return RunResult(
                    stdout="epel-release\nepel-release\nrpmfusion-free-release\n",
                    stderr="",
                    returncode=0,
                )
            return fixture_executor(cmd, cwd=cwd)

        result = _detect_repo_providing_packages(executor, host_root)
        assert "epel-release" in result
        assert "rpmfusion-free-release" in result

    def test_no_repo_files(self, tmp_path):
        """When no repo files exist, returns empty list."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        def executor(cmd, cwd=None):
            return RunResult(stdout="", stderr="", returncode=1)

        result = _detect_repo_providing_packages(executor, tmp_path)
        assert result == []

    def test_rpm_qf_failure_returns_empty(self, host_root):
        """When rpm -qf fails, returns empty list gracefully."""
        from yoinkc.inspectors.rpm import _detect_repo_providing_packages

        def executor(cmd, cwd=None):
            return RunResult(stdout="", stderr="error", returncode=1)

        result = _detect_repo_providing_packages(executor, host_root)
        assert result == []
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestRepoProvidingPackages -v`

Expected: FAIL — `_detect_repo_providing_packages` not importable.

- [ ] **Step 3: Implement _detect_repo_providing_packages**

In `src/yoinkc/inspectors/rpm.py`, add after the `_collect_gpg_keys` function (around line 500):

```python
def _detect_repo_providing_packages(
    executor: "Executor",
    host_root: Path,
) -> List[str]:
    """Detect packages that own .repo files in /etc/yum.repos.d/.

    These are repo-providing packages (e.g., epel-release) that need
    to be bootstrapped in the preflight container before checking
    availability of packages from their repos.
    """
    repo_dir = host_root / "etc" / "yum.repos.d"
    if not repo_dir.is_dir():
        return []

    repo_files = [f for f in repo_dir.iterdir() if f.suffix == ".repo" and f.is_file()]
    if not repo_files:
        return []

    # Use --dbpath for the same reason as the rest of the inspector:
    # the container's rpm binary may use a different default dbpath.
    dbpath = detect_rpmdb_path(host_root, relative=True)
    cmd = ["rpm", "--root", str(host_root), "--dbpath", dbpath, "-qf", "--queryformat", "%{NAME}\n"]
    cmd += [str(f) for f in repo_files]

    _debug(f"detecting repo-providing packages: {len(repo_files)} repo files")
    result = executor(cmd)
    if result.returncode != 0:
        _debug(f"rpm -qf failed (rc={result.returncode}): {result.stderr[:200]}")
        # Partial results are still useful — some files may be owned, others not
        if not result.stdout.strip():
            return []

    owners: set = set()
    for line in result.stdout.splitlines():
        name = line.strip()
        # rpm -qf returns "file ... is not owned by any package" for unowned files
        if name and "is not owned by" not in name:
            owners.add(name)

    _debug(f"repo-providing packages: {sorted(owners)}")
    return sorted(owners)
```

Then in the `run()` function, after step 5 (repo files collection, around line 1232), add:

```python
    # 5-rpp) Repo-providing packages
    if executor is not None:
        section.repo_providing_packages = _detect_repo_providing_packages(executor, host_root)
        _debug(f"repo-providing packages: {section.repo_providing_packages}")
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestRepoProvidingPackages -v`

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/inspectors/rpm.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): detect repo-providing packages in RPM inspector

Identify packages that own .repo files in /etc/yum.repos.d/ via rpm -qf.
These packages (e.g., epel-release) need to be bootstrapped in the
preflight container before checking availability of their repo's packages."
```

---

### Task 5: CLI — add --skip-unavailable flag

**Files:**
- Modify: `src/yoinkc/cli.py:175-180` (after `--no-redaction` flag)
- Modify: `tests/test_preflight.py` (update `_make_inspect_args` defaults)
- Test: `tests/test_cli.py`

- [ ] **Step 1: Write failing test**

Add to `tests/test_cli.py` (find the appropriate location for new parse_args tests — the file already tests CLI parsing):

```python
def test_skip_unavailable_flag():
    """--skip-unavailable is parsed correctly."""
    from yoinkc.cli import parse_args
    args = parse_args(["inspect", "--skip-unavailable"])
    assert args.skip_unavailable is True


def test_skip_unavailable_default_false():
    """--skip-unavailable defaults to False."""
    from yoinkc.cli import parse_args
    args = parse_args(["inspect"])
    assert args.skip_unavailable is False
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_cli.py::test_skip_unavailable_flag -v`

Expected: FAIL — `skip_unavailable` attribute not on args.

- [ ] **Step 3: Add --skip-unavailable flag**

In `src/yoinkc/cli.py`, in `_add_inspect_args()`, add after the `--no-redaction` argument (around line 179):

```python
    parser.add_argument(
        "--skip-unavailable",
        action="store_true",
        help="Skip the package availability preflight check. All packages "
             "included in the Containerfile without validation.",
    )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_cli.py -v -q`

Expected: All PASS.

- [ ] **Step 5: Update test helpers**

In `tests/test_preflight.py`, update `_make_inspect_args` to include `skip_unavailable=False` in the defaults:

```python
    args = argparse.Namespace(
        # ... existing fields ...
        skip_unavailable=False,
        # ... rest ...
    )
```

- [ ] **Step 6: Run full suite to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

Expected: All PASS.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/cli.py tests/test_cli.py tests/test_preflight.py
git commit -m "feat(preflight): add --skip-unavailable CLI flag

Skips the package availability preflight check. All packages are
included in the Containerfile without validation."
```

---

### Task 6: Core preflight module — run_package_preflight()

**Files:**
- Create: `src/yoinkc/rpm_preflight.py`
- Test: `tests/test_rpm_preflight.py` (append to existing)

This is the largest task. The module runs a two-phase container check:
- Phase 1: Bootstrap repo-providing packages
- Phase 2: `dnf repoquery --available` against the full install set

- [ ] **Step 1: Write failing tests for the preflight module**

Append to `tests/test_rpm_preflight.py`:

```python
from datetime import datetime, timezone
from unittest.mock import patch, MagicMock

from yoinkc.rpm_preflight import run_package_preflight


def _make_preflight_snapshot(
    packages=None,
    base_image="quay.io/fedora/fedora-bootc:44",
    repo_providing_packages=None,
):
    """Build a snapshot suitable for preflight testing."""
    entries = []
    for name in (packages or []):
        entries.append(PackageEntry(
            name=name,
            epoch="0",
            version="1.0",
            release="1.fc44",
            arch="x86_64",
            state=PackageState.ADDED,
            include=True,
            source_repo="baseos",
        ))
    section = RpmSection(
        packages_added=entries,
        no_baseline=True,
        base_image=base_image,
        repo_providing_packages=repo_providing_packages or [],
    )
    return InspectionSnapshot(rpm=section)


def _make_preflight_executor(
    repoquery_stdout="",
    repoquery_rc=0,
    repoquery_stderr="",
    pull_rc=0,
    bootstrap_rc=0,
    repos_stdout="fedora\nupdates\n",
):
    """Build a mock executor for preflight subprocess calls.

    repoquery_stdout should contain plain package names (one per line),
    matching the ``--queryformat "%{name}"`` output format.
    """
    def executor(cmd, cwd=None):
        cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
        # podman pull
        if "podman" in cmd_str and "pull" in cmd_str:
            return RunResult(stdout="", stderr="" if pull_rc == 0 else "pull failed", returncode=pull_rc)
        # podman run with dnf install (bootstrap)
        if "podman" in cmd_str and "run" in cmd_str and "dnf install" in cmd_str:
            return RunResult(stdout="", stderr="" if bootstrap_rc == 0 else "install failed", returncode=bootstrap_rc)
        # podman run with dnf repoquery
        if "podman" in cmd_str and "run" in cmd_str and "repoquery" in cmd_str:
            return RunResult(stdout=repoquery_stdout, stderr=repoquery_stderr, returncode=repoquery_rc)
        # podman run with dnf repolist
        if "podman" in cmd_str and "run" in cmd_str and "repolist" in cmd_str:
            return RunResult(stdout=repos_stdout, stderr="", returncode=0)
        # podman rm
        if "podman" in cmd_str and "rm" in cmd_str:
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="unknown cmd", returncode=1)
    return executor


class TestPackagePreflight:
    def test_all_available(self):
        """All packages found in repos -> status completed, no unavailable."""
        snapshot = _make_preflight_snapshot(packages=["httpd", "nginx"])
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\nnginx\n",
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert result.status == "completed"
        assert sorted(result.available) == ["httpd", "nginx"]
        assert result.unavailable == []

    def test_some_unavailable(self):
        """Some packages missing -> status completed, lists unavailable."""
        snapshot = _make_preflight_snapshot(packages=["httpd", "mcelog", "nginx"])
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\nnginx\n",
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert result.status == "completed"
        assert result.unavailable == ["mcelog"]
        assert sorted(result.available) == ["httpd", "nginx"]

    def test_base_image_pull_fails(self):
        """Pull failure -> status failed."""
        snapshot = _make_preflight_snapshot(packages=["httpd"])
        executor = _make_preflight_executor(pull_rc=1)
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert result.status == "failed"
        assert "pull" in result.status_reason.lower()

    def test_direct_install_excluded(self):
        """Packages with no source_repo are classified as direct_install."""
        entries = [
            PackageEntry(
                name="httpd", epoch="0", version="1.0", release="1",
                arch="x86_64", source_repo="baseos",
            ),
            PackageEntry(
                name="custom-agent", epoch="0", version="1.0", release="1",
                arch="x86_64", source_repo="",
            ),
            PackageEntry(
                name="local-tool", epoch="0", version="1.0", release="1",
                arch="x86_64", source_repo="(none)",
            ),
        ]
        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=entries,
                no_baseline=True,
                base_image="quay.io/fedora/fedora-bootc:44",
            )
        )
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\n",
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert sorted(result.direct_install) == ["custom-agent", "local-tool"]
        # Direct installs are NOT in the available/unavailable lists
        assert "custom-agent" not in result.available
        assert "custom-agent" not in result.unavailable

    def test_repo_provider_bootstrap_failure_classifies_correctly(self):
        """Bootstrap failure: base-repo packages stay unavailable,
        provider-dependent packages become unverifiable."""
        from yoinkc.schema import RepoFile

        snapshot = _make_preflight_snapshot(
            packages=["httpd", "some-epel-pkg"],
            repo_providing_packages=["epel-release"],
        )
        # httpd is from baseos, some-epel-pkg is from epel
        snapshot.rpm.packages_added[0].source_repo = "baseos"
        snapshot.rpm.packages_added[1].source_repo = "epel"
        # Add a non-default repo file so _provider_repo_ids can map epel-release -> epel
        snapshot.rpm.repo_files = [
            RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nname=EPEL\n", is_default_repo=False),
        ]
        executor = _make_preflight_executor(
            bootstrap_rc=1,
            repoquery_stdout="httpd\n",  # httpd found in base repos
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert result.status == "partial"
        assert "epel-release" in result.status_reason
        # httpd is found -> available (not unavailable, not unverifiable)
        assert "httpd" in result.available
        # some-epel-pkg not found, but its source_repo matches the failed provider's repo
        # -> unverifiable (NOT unavailable)
        unverifiable_names = [uv.name for uv in result.unverifiable]
        assert "some-epel-pkg" in unverifiable_names
        assert "some-epel-pkg" not in result.unavailable

    def test_repo_unreachable_detected(self):
        """Unreachable repos are detected from dnf stderr and reported."""
        snapshot = _make_preflight_snapshot(packages=["httpd", "internal-app"])
        executor = _make_preflight_executor(
            repoquery_stdout="httpd\n",
            repoquery_stderr="Failed to synchronize cache for repo 'internal-mirror': connection timed out\n",
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=executor,
        )
        assert result.status == "partial"
        assert len(result.repo_unreachable) == 1
        assert result.repo_unreachable[0].repo_id == "internal-mirror"

    def test_empty_install_set_returns_completed(self):
        """No packages to check -> completed with empty lists."""
        snapshot = InspectionSnapshot(
            rpm=RpmSection(base_image="quay.io/fedora/fedora-bootc:44")
        )
        result = run_package_preflight(
            snapshot=snapshot,
            executor=_make_preflight_executor(),
        )
        assert result.status == "completed"
        assert result.available == []

    def test_no_base_image_returns_failed(self):
        """No base image -> failed."""
        snapshot = _make_preflight_snapshot(packages=["httpd"])
        snapshot.rpm.base_image = None
        result = run_package_preflight(
            snapshot=snapshot,
            executor=_make_preflight_executor(),
        )
        assert result.status == "failed"
        assert "base image" in result.status_reason.lower()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPackagePreflight -v`

Expected: FAIL — `yoinkc.rpm_preflight` does not exist.

- [ ] **Step 3: Implement run_package_preflight**

Create `src/yoinkc/rpm_preflight.py`:

```python
"""
RPM package availability preflight check.

Validates that packages in the install set exist in the target base image's
repos before rendering the Containerfile. Runs a two-phase check inside a
temporary container:
  Phase 1: Bootstrap repo-providing packages (e.g., epel-release)
  Phase 2: dnf repoquery --available to check package existence

Results are stored as PreflightResult in the InspectionSnapshot.
"""

import configparser
import shutil
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import List, Optional

from ._util import debug as _debug_fn
from .executor import Executor
from .install_set import resolve_install_set
from .schema import (
    InspectionSnapshot,
    PreflightResult,
    RepoStatus,
    UnverifiablePackage,
)

_DIRECT_INSTALL_REPOS = frozenset({"", "(none)", "commandline", "(commandline)", "installed"})

# DNF stderr patterns that indicate a repo failed to sync/download metadata.
_REPO_FAILURE_PATTERNS = (
    "Failed to synchronize cache for repo",
    "Failed to download metadata for repo",
    "Cannot download repomd.xml",
    "Errors during downloading metadata for repository",
)


def _debug(msg: str) -> None:
    _debug_fn("rpm_preflight", msg)


def _stage_config_tree(snapshot: InspectionSnapshot) -> Optional[Path]:
    """Stage snapshot repo files, GPG keys, and dnf config to a temp directory.

    Returns the temp directory path (caller must clean up), or None if
    the snapshot has no custom config to stage.

    The layout mirrors what the renderer's ``write_config_tree`` produces:
      staging/etc/yum.repos.d/*.repo
      staging/etc/pki/rpm-gpg/RPM-GPG-KEY-*
      staging/etc/dnf/...
    """
    has_repos = snapshot.rpm and snapshot.rpm.repo_files and any(r.include for r in snapshot.rpm.repo_files)
    has_gpg = snapshot.rpm and snapshot.rpm.gpg_keys and any(k.include for k in snapshot.rpm.gpg_keys)
    # dnf config files live in snapshot.config with paths starting "etc/dnf/"
    has_dnf_conf = (
        snapshot.config and snapshot.config.files
        and any(f.include and f.path.startswith("etc/dnf/") for f in snapshot.config.files)
    )

    if not has_repos and not has_gpg and not has_dnf_conf:
        return None

    staging = Path(tempfile.mkdtemp(prefix="yoinkc-preflight-"))

    if has_repos:
        for repo in snapshot.rpm.repo_files:
            if not repo.include or not repo.path:
                continue
            dest = staging / repo.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(repo.content or "")

    if has_gpg:
        for key in snapshot.rpm.gpg_keys:
            if not key.include or not key.path:
                continue
            dest = staging / key.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(key.content or "")

    if has_dnf_conf:
        for f in snapshot.config.files:
            if not f.include or not f.path.startswith("etc/dnf/"):
                continue
            dest = staging / f.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(f.content or "")

    return staging


def _classify_direct_installs(snapshot: InspectionSnapshot) -> tuple[list[str], list[str]]:
    """Split install set into (repo_packages, direct_install_packages).

    Direct-install packages have no repo origin (source_repo is empty,
    "(none)", "commandline", etc.) and cannot be installed from a repo.

    Synthetic packages injected by resolve_install_set() (e.g., tuned)
    that are not in packages_added are treated as repo packages — they
    must be validated by repoquery, not skipped as direct-install.
    """
    if not snapshot.rpm or not snapshot.rpm.packages_added:
        return [], []

    install_set = set(resolve_install_set(snapshot))
    direct: list[str] = []
    repo_pkgs: list[str] = []

    # Build a lookup of source_repo by package name (only real packages)
    source_repos = {}
    for p in snapshot.rpm.packages_added:
        if p.name not in source_repos:
            source_repos[p.name] = p.source_repo

    for name in sorted(install_set):
        if name not in source_repos:
            # Synthetic injection (e.g., tuned) — not in packages_added,
            # so no source_repo to check. Treat as repo package so
            # repoquery validates it.
            repo_pkgs.append(name)
        elif source_repos[name].strip().lower() in _DIRECT_INSTALL_REPOS:
            direct.append(name)
        else:
            repo_pkgs.append(name)

    return repo_pkgs, direct


def _provider_repo_ids(snapshot: InspectionSnapshot) -> dict[str, set[str]]:
    """Map each repo-providing package to the repo IDs it provides.

    Parses [reponame] headers from .repo files owned by each provider.
    Returns {provider_pkg_name: {repo_id, ...}}.
    """
    if not snapshot.rpm:
        return {}

    # Build mapping: .repo file path -> owning package
    # We know repo_providing_packages and repo_files; cross-reference
    # by checking which repo files are non-default (user-added).
    result: dict[str, set[str]] = {}
    for repo_file in (snapshot.rpm.repo_files or []):
        if repo_file.is_default_repo:
            continue  # Base-image repo, not from a provider package
        # Parse repo IDs from the .repo content
        repo_ids: set[str] = set()
        if repo_file.content:
            parser = configparser.ConfigParser()
            try:
                parser.read_string(repo_file.content)
                repo_ids = set(parser.sections())
            except configparser.Error:
                pass
        if repo_ids:
            # Attribute to all repo-providing packages (conservative)
            # In practice, each .repo file is owned by one provider
            for provider in (snapshot.rpm.repo_providing_packages or []):
                result.setdefault(provider, set()).update(repo_ids)
    return result


def _detect_unreachable_repos(stderr: str) -> list[RepoStatus]:
    """Parse dnf stderr for repo failure messages.

    Returns a list of RepoStatus for repos that could not be queried.
    """
    unreachable: list[RepoStatus] = []
    seen: set[str] = set()
    for line in stderr.splitlines():
        for pattern in _REPO_FAILURE_PATTERNS:
            if pattern in line:
                # Extract repo ID — typically quoted or at end of message
                # e.g., "Failed to synchronize cache for repo 'epel'"
                repo_id = ""
                if "'" in line:
                    parts = line.split("'")
                    if len(parts) >= 2:
                        repo_id = parts[1]
                elif '"' in line:
                    parts = line.split('"')
                    if len(parts) >= 2:
                        repo_id = parts[1]
                if repo_id and repo_id not in seen:
                    seen.add(repo_id)
                    unreachable.append(RepoStatus(
                        repo_id=repo_id,
                        repo_name=repo_id,
                        error=line.strip(),
                    ))
                break
    return unreachable


def run_package_preflight(
    *,
    snapshot: InspectionSnapshot,
    executor: Executor,
) -> PreflightResult:
    """Run the package availability preflight check.

    Parameters
    ----------
    snapshot : InspectionSnapshot
        Snapshot with RPM data populated (packages_added, leaf_packages, etc.).
    executor : Executor
        Command executor (real or mock).

    Returns
    -------
    PreflightResult
        Structured result with available/unavailable/unverifiable packages.
    """
    timestamp = datetime.now(timezone.utc).isoformat()

    # Check prerequisites
    base_image = snapshot.rpm.base_image if snapshot.rpm else None
    if not base_image:
        return PreflightResult(
            status="failed",
            status_reason="No base image configured — cannot run preflight check",
            timestamp=timestamp,
        )

    # Classify direct installs vs repo packages
    repo_packages, direct_installs = _classify_direct_installs(snapshot)
    _debug(f"install set: {len(repo_packages)} repo packages, {len(direct_installs)} direct installs")

    if not repo_packages:
        return PreflightResult(
            status="completed",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Pull the base image
    pull_result = executor(["podman", "pull", "-q", base_image])
    if pull_result.returncode != 0:
        return PreflightResult(
            status="failed",
            status_reason=f"Base image {base_image} could not be pulled: {pull_result.stderr.strip()[:200]}",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Stage custom repo/GPG/dnf config from snapshot
    staging_dir = _stage_config_tree(snapshot)

    try:
        return _run_checks(
            snapshot=snapshot,
            executor=executor,
            base_image=base_image,
            repo_packages=repo_packages,
            direct_installs=direct_installs,
            staging_dir=staging_dir,
            timestamp=timestamp,
        )
    finally:
        if staging_dir:
            shutil.rmtree(staging_dir, ignore_errors=True)


def _run_checks(
    *,
    snapshot: InspectionSnapshot,
    executor: Executor,
    base_image: str,
    repo_packages: list[str],
    direct_installs: list[str],
    staging_dir: Optional[Path],
    timestamp: str,
) -> PreflightResult:
    """Run the two-phase check (extracted for staging_dir cleanup)."""

    # Build the podman run command with volume mounts for custom repos
    run_base = ["podman", "run", "--rm"]

    if staging_dir:
        repo_dir = staging_dir / "etc" / "yum.repos.d"
        gpg_dir = staging_dir / "etc" / "pki" / "rpm-gpg"
        dnf_dir = staging_dir / "etc" / "dnf"
        if repo_dir.is_dir():
            run_base += ["-v", f"{repo_dir}:/etc/yum.repos.d/:Z"]
        if gpg_dir.is_dir():
            run_base += ["-v", f"{gpg_dir}:/etc/pki/rpm-gpg/:Z"]
        if dnf_dir.is_dir():
            run_base += ["-v", f"{dnf_dir}:/etc/dnf/:Z"]

    # Phase 1: Bootstrap repo-providing packages
    repo_providers = snapshot.rpm.repo_providing_packages if snapshot.rpm else []
    unverifiable: list[UnverifiablePackage] = []
    bootstrap_failed_providers: set[str] = set()

    if repo_providers:
        _debug(f"phase 1: bootstrapping {repo_providers}")
        bootstrap_cmd = run_base + [base_image, "dnf", "install", "-y"] + list(repo_providers)
        bootstrap_result = executor(bootstrap_cmd)
        if bootstrap_result.returncode != 0:
            _debug(f"repo-provider bootstrap failed: {bootstrap_result.stderr[:200]}")
            bootstrap_failed_providers = set(repo_providers)

    # Phase 2: Check availability via dnf repoquery
    # Use --queryformat "%{name}\n" for unambiguous name extraction.
    # Avoids NEVRA parsing pitfalls with hyphenated package names.
    _debug(f"phase 2: checking {len(repo_packages)} packages")

    repoquery_cmd = run_base + [
        base_image, "dnf", "repoquery", "--available",
        "--queryformat", "%{name}",
    ] + repo_packages

    repoquery_result = executor(repoquery_cmd)

    # Detect unreachable repos from stderr
    repo_unreachable = _detect_unreachable_repos(repoquery_result.stderr or "")

    if repoquery_result.returncode != 0 and not repoquery_result.stdout.strip():
        # Total failure — no results at all
        if repo_unreachable:
            # Some repos failed — report partial, not failed
            return PreflightResult(
                status="partial",
                status_reason=f"{len(repo_unreachable)} repo(s) unreachable",
                direct_install=direct_installs,
                repo_unreachable=repo_unreachable,
                base_image=base_image,
                timestamp=timestamp,
            )
        return PreflightResult(
            status="failed",
            status_reason=f"dnf repoquery failed: {repoquery_result.stderr.strip()[:200]}",
            direct_install=direct_installs,
            base_image=base_image,
            timestamp=timestamp,
        )

    # Parse results — each line is a plain package name
    found_names: set[str] = set()
    for line in repoquery_result.stdout.splitlines():
        name = line.strip()
        if name:
            found_names.add(name)

    available = sorted(n for n in repo_packages if n in found_names)
    not_found = sorted(n for n in repo_packages if n not in found_names)

    # Classify not-found packages: unavailable vs unverifiable.
    # If repo-provider bootstrap failed, packages whose source_repo
    # matches the failed provider's repos are unverifiable (we couldn't
    # check their repos). Packages from base repos that weren't found
    # are genuinely unavailable.
    unavailable: list[str] = []
    if bootstrap_failed_providers and not_found:
        provider_repos = _provider_repo_ids(snapshot)
        failed_repo_ids: set[str] = set()
        for provider in bootstrap_failed_providers:
            failed_repo_ids.update(provider_repos.get(provider, set()))

        # Build source_repo lookup
        source_repos = {}
        for p in snapshot.rpm.packages_added:
            if p.name not in source_repos:
                source_repos[p.name] = p.source_repo

        for pkg in not_found:
            pkg_source = source_repos.get(pkg, "").strip().lower()
            if pkg_source in failed_repo_ids or pkg_source in {p.lower() for p in failed_repo_ids}:
                unverifiable.append(UnverifiablePackage(
                    name=pkg,
                    reason=f"repo-providing package(s) {', '.join(sorted(bootstrap_failed_providers))} unavailable",
                ))
            else:
                unavailable.append(pkg)
    else:
        unavailable = not_found

    # Populate affected_packages on unreachable repos
    if repo_unreachable:
        source_repos = {}
        for p in snapshot.rpm.packages_added:
            if p.name not in source_repos:
                source_repos[p.name] = p.source_repo
        unreachable_ids = {r.repo_id for r in repo_unreachable}
        for repo_status in repo_unreachable:
            repo_status.affected_packages = sorted(
                name for name in repo_packages
                if source_repos.get(name, "") == repo_status.repo_id
            )

    # Query available repo IDs
    repolist_cmd = run_base + [base_image, "dnf", "repolist", "--quiet"]
    repolist_result = executor(repolist_cmd)
    repos_queried = []
    if repolist_result.returncode == 0:
        for line in repolist_result.stdout.splitlines():
            repo_id = line.strip().split()[0] if line.strip() else ""
            if repo_id:
                repos_queried.append(repo_id)

    # Determine status
    if unverifiable or repo_unreachable:
        status = "partial"
        reasons = []
        if bootstrap_failed_providers:
            reasons.append(
                f"repo-providing package(s) {', '.join(sorted(bootstrap_failed_providers))} "
                f"unavailable; {len(unverifiable)} package(s) unverifiable"
            )
        if repo_unreachable:
            reasons.append(f"{len(repo_unreachable)} repo(s) unreachable")
        status_reason = "; ".join(reasons) if reasons else None
    else:
        status = "completed"
        status_reason = None

    return PreflightResult(
        status=status,
        status_reason=status_reason,
        available=available,
        unavailable=unavailable,
        unverifiable=unverifiable,
        direct_install=direct_installs,
        repo_unreachable=repo_unreachable,
        base_image=base_image,
        repos_queried=repos_queried,
        timestamp=timestamp,
    )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPackagePreflight -v`

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/rpm_preflight.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): implement run_package_preflight module

Two-phase container-based availability check:
  Phase 1: Bootstrap repo-providing packages
  Phase 2: dnf repoquery --available
Classifies direct-install RPMs, handles partial/failed status."
```

---

### Task 7: Integration — call preflight from run_all()

**Files:**
- Modify: `src/yoinkc/inspectors/__init__.py:359-371` (after RPM inspector step)
- Modify: `src/yoinkc/__main__.py:34-48` (thread skip_unavailable)

- [ ] **Step 1: Write failing integration test**

Append to `tests/test_rpm_preflight.py`:

```python
class TestPreflightIntegration:
    def test_skip_unavailable_sets_skipped(self, fixture_executor, host_root):
        """When skip_unavailable=True, snapshot.preflight.status is 'skipped'."""
        from yoinkc.inspectors import run_all

        snapshot = run_all(
            host_root,
            executor=fixture_executor,
            no_baseline_opt_in=True,
            skip_unavailable=True,
        )
        assert snapshot.preflight.status == "skipped"
        assert "skip-unavailable" in snapshot.preflight.status_reason
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPreflightIntegration -v`

Expected: FAIL — `run_all()` does not accept `skip_unavailable`.

- [ ] **Step 3: Thread skip_unavailable through the stack**

**In `src/yoinkc/inspectors/__init__.py`**, add `skip_unavailable` parameter to `run_all()`:

At the function signature (line 219), add `skip_unavailable: bool = False`:

```python
def run_all(
    host_root: Path,
    executor: Optional[Executor] = None,
    config_diffs: bool = False,
    deep_binary_scan: bool = False,
    query_podman: bool = False,
    baseline_packages_file: Optional[Path] = None,
    target_version: Optional[str] = None,
    target_image: Optional[str] = None,
    user_strategy: Optional[str] = None,
    no_baseline_opt_in: bool = False,
    skip_unavailable: bool = False,
) -> InspectionSnapshot:
```

After the RPM inspector step (after line 371, after post-inspector baseline fallback), add the preflight call:

```python
    # 1.5) Package availability preflight
    if skip_unavailable:
        from ..schema import PreflightResult
        snapshot.preflight = PreflightResult(
            status="skipped",
            status_reason="user passed --skip-unavailable",
        )
    elif snapshot.rpm and snapshot.rpm.base_image and executor is not None:
        _section_banner("Package preflight", 1, _TOTAL_STEPS)
        from ..rpm_preflight import run_package_preflight
        try:
            snapshot.preflight = run_package_preflight(
                snapshot=snapshot,
                executor=executor,
            )
        except Exception as exc:
            from ..schema import PreflightResult
            snapshot.preflight = PreflightResult(
                status="failed",
                status_reason=f"Preflight check failed: {exc}",
            )
            print(f"WARNING: package preflight failed: {exc}", file=sys.stderr)
```

**In `src/yoinkc/__main__.py`**, thread `skip_unavailable` to `_run_inspectors`:

In `_run_inspectors()` (line 34), add the parameter:

```python
def _run_inspectors(host_root: Path, args) -> InspectionSnapshot:
    """Run all inspectors and merge into one snapshot."""
    from .inspectors import run_all

    return run_all(
        host_root,
        config_diffs=args.config_diffs,
        deep_binary_scan=args.deep_binary_scan,
        query_podman=args.query_podman,
        baseline_packages_file=args.baseline_packages,
        target_version=args.target_version,
        target_image=args.target_image,
        user_strategy=args.user_strategy,
        no_baseline_opt_in=args.no_baseline,
        skip_unavailable=getattr(args, "skip_unavailable", False),
    )
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestPreflightIntegration -v`

Expected: PASS.

- [ ] **Step 5: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

Expected: All PASS. Existing tests should not be affected because `skip_unavailable` defaults to `False` and the preflight runs only when `base_image` is set and executor is available. In tests using fixture executors, the podman pull command will fail (fixture executor doesn't handle it), so preflight will return `failed` status — which is fine for existing tests.

NOTE: If existing tests break because the preflight tries to run podman commands the fixture executor doesn't handle, the fix is to ensure the preflight gracefully handles the fixture executor's "unknown command" returns (returncode=1, which triggers `status="failed"`). The `try/except` wrapper ensures it never crashes.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/inspectors/__init__.py src/yoinkc/__main__.py
git commit -m "feat(preflight): integrate package preflight into inspection pipeline

Call run_package_preflight after RPM inspector completes. Respects
--skip-unavailable flag. Catches exceptions gracefully — a failed
preflight never blocks inspection."
```

---

### Task 8: Renderer — consume preflight results

**Files:**
- Modify: `src/yoinkc/renderers/containerfile/packages.py`
- Modify: `src/yoinkc/pipeline.py`
- Test: `tests/test_rpm_preflight.py` (append)

- [ ] **Step 1: Write failing tests for renderer preflight consumption**

Append to `tests/test_rpm_preflight.py`:

```python
from yoinkc.renderers.containerfile.packages import section_lines


class TestRendererPreflightConsumption:
    def _make_renderer_snapshot(self, packages, unavailable=None, direct_install=None, unverifiable=None):
        """Build a snapshot with preflight data for renderer testing."""
        entries = []
        for name in packages:
            entries.append(PackageEntry(
                name=name, epoch="0", version="1.0", release="1",
                arch="x86_64", state=PackageState.ADDED, include=True,
                source_repo="baseos",
            ))
        section = RpmSection(
            packages_added=entries,
            no_baseline=True,
            base_image="quay.io/fedora/fedora-bootc:44",
        )
        preflight = PreflightResult(
            status="completed",
            available=[p for p in packages if p not in (unavailable or []) and p not in (direct_install or [])],
            unavailable=unavailable or [],
            direct_install=direct_install or [],
            unverifiable=[UnverifiablePackage(name=n, reason="test") for n in (unverifiable or [])],
            base_image="quay.io/fedora/fedora-bootc:44",
            repos_queried=["fedora", "updates"],
            timestamp="2026-04-09T17:00:00Z",
        )
        return InspectionSnapshot(rpm=section, preflight=preflight)

    def test_unavailable_excluded_from_dnf_install(self):
        """Unavailable packages are NOT in the dnf install line."""
        snapshot = self._make_renderer_snapshot(
            packages=["httpd", "mcelog", "nginx"],
            unavailable=["mcelog"],
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        # Find the actual dnf install block lines (indented package names)
        install_block = [l.strip() for l in lines if l.startswith("    ") and not l.strip().startswith("#") and not l.strip().startswith("&&")]
        install_names_in_output = [l.rstrip(" \\") for l in install_block]
        assert "mcelog" not in install_names_in_output
        assert "httpd" in install_names_in_output
        assert "nginx" in install_names_in_output

    def test_direct_install_excluded_from_dnf_install(self):
        """Direct-install RPMs are NOT in the dnf install line."""
        snapshot = self._make_renderer_snapshot(
            packages=["httpd", "custom-agent"],
            direct_install=["custom-agent"],
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        install_block = [l.strip().rstrip(" \\") for l in lines if l.startswith("    ") and not l.strip().startswith("#") and not l.strip().startswith("&&")]
        assert "custom-agent" not in install_block

    def test_skipped_preflight_includes_all(self):
        """With preflight skipped, all packages are included."""
        entries = [
            PackageEntry(
                name=n, epoch="0", version="1.0", release="1",
                arch="x86_64", state=PackageState.ADDED, include=True,
            )
            for n in ["httpd", "mcelog", "nginx"]
        ]
        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=entries,
                no_baseline=True,
                base_image="quay.io/fedora/fedora-bootc:44",
            ),
            preflight=PreflightResult(
                status="skipped",
                status_reason="user passed --skip-unavailable",
            ),
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        joined = "\n".join(lines)
        assert "httpd" in joined
        assert "mcelog" in joined
        assert "nginx" in joined
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestRendererPreflightConsumption -v`

Expected: FAIL — renderer doesn't yet consume preflight data.

- [ ] **Step 3: Modify renderer to consume preflight results**

In `src/yoinkc/renderers/containerfile/packages.py`, after computing `install_names` via `resolve_install_set(snapshot)`, add preflight filtering:

```python
            install_names = resolve_install_set(snapshot)

            # Apply preflight filtering (exclude unavailable and direct-install)
            preflight = snapshot.preflight
            if preflight.status in ("completed", "partial"):
                exclude_set = set(preflight.unavailable) | set(preflight.direct_install)
                if exclude_set:
                    install_names = [n for n in install_names if n not in exclude_set]
```

- [ ] **Step 4: Add diagnostic block emission**

Add `emit_preflight_diagnostics` function to `src/yoinkc/renderers/containerfile/packages.py`:

```python
import sys


def emit_preflight_diagnostics(snapshot: InspectionSnapshot) -> None:
    """Emit the preflight diagnostic block to stderr.

    Called after generating Containerfile lines. Only emits when
    preflight ran and found issues worth reporting.
    """
    preflight = snapshot.preflight
    if preflight.status == "skipped":
        return
    if preflight.status == "failed":
        print(f"\nPreflight check failed: {preflight.status_reason}", file=sys.stderr)
        return

    has_issues = (
        preflight.direct_install
        or preflight.unavailable
        or preflight.unverifiable
        or preflight.repo_unreachable
    )
    if not has_issues:
        return

    print("\n=== Package Availability Report ===\n", file=sys.stderr)

    excluded_count = 0

    if preflight.direct_install:
        print("NOT IN ANY REPO (installed directly via rpm — cannot be installed from repos):", file=sys.stderr)
        for pkg in sorted(preflight.direct_install):
            print(f"  {pkg}", file=sys.stderr)
        excluded_count += len(preflight.direct_install)
        print("", file=sys.stderr)

    if preflight.unavailable:
        print("UNAVAILABLE in target repos:", file=sys.stderr)
        for pkg in sorted(preflight.unavailable):
            print(f"  {pkg}", file=sys.stderr)
        excluded_count += len(preflight.unavailable)
        print("", file=sys.stderr)

    if preflight.unverifiable:
        print("UNVERIFIABLE (could not check — included in Containerfile but not validated):", file=sys.stderr)
        for uv in preflight.unverifiable:
            print(f"  {uv.name} ({uv.reason})", file=sys.stderr)
        print("", file=sys.stderr)

    if preflight.repo_unreachable:
        print("REPO UNREACHABLE (could not verify — packages from these repos not validated):", file=sys.stderr)
        for repo in preflight.repo_unreachable:
            print(f"  {repo.repo_id} (error: {repo.error})", file=sys.stderr)
            if repo.affected_packages:
                print(f"    Packages from this repo: {', '.join(sorted(repo.affected_packages))}", file=sys.stderr)
        print("", file=sys.stderr)

    # Footer
    if excluded_count:
        print(f"{excluded_count} packages excluded from Containerfile.", file=sys.stderr)
    unverifiable_count = len(preflight.unverifiable)
    if unverifiable_count:
        s = "s" if unverifiable_count != 1 else ""
        print(f"{unverifiable_count} package{s} unverifiable (included but not validated).", file=sys.stderr)
    repo_unreachable_pkg_count = sum(len(r.affected_packages) for r in preflight.repo_unreachable)
    if repo_unreachable_pkg_count:
        print(f"{repo_unreachable_pkg_count} packages from unreachable repos (included but not validated).", file=sys.stderr)
    print(f"Preflight status: {preflight.status.upper()} — use --skip-unavailable to skip all checks.", file=sys.stderr)
    print("===", file=sys.stderr)
```

In `src/yoinkc/pipeline.py`, call `emit_preflight_diagnostics` after `_print_secrets_summary(snapshot)` (around line 440):

```python
        from .renderers.containerfile.packages import emit_preflight_diagnostics
        emit_preflight_diagnostics(snapshot)
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestRendererPreflightConsumption -v`

Expected: All PASS.

- [ ] **Step 6: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

Expected: All PASS. Existing tests have `preflight.status == "skipped"` by default, so the renderer won't filter anything.

- [ ] **Step 7: Commit**

```bash
git add src/yoinkc/renderers/containerfile/packages.py src/yoinkc/pipeline.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): renderer excludes unavailable packages, emits diagnostics

Packages marked unavailable or direct-install by preflight are excluded
from the dnf install line. Diagnostic block emitted to stderr with
categorized package status and footer summary."
```

---

### Task 9: Architect — fleet preflight data plumbing

> **Scope note:** This task and Task 10 deliver preflight data extraction and plumbing
> into the architect's `FleetInput` data model. The per-base-image aggregation views
> described in the spec (prevalence-sorted unavailable packages, per-base-image grouping,
> layer decomposition awareness) are a follow-up — they consume this data but are not
> part of this branch.

**Files:**
- Modify: `src/yoinkc/architect/analyzer.py`
- Test: `tests/test_rpm_preflight.py` (append)

- [ ] **Step 1: Write failing test**

Append to `tests/test_rpm_preflight.py`:

```python
from yoinkc.architect.analyzer import FleetInput


class TestArchitectPreflightAggregation:
    def test_fleet_input_has_preflight_fields(self):
        """FleetInput includes preflight data for aggregation."""
        fi = FleetInput(
            name="fleet-1",
            packages=["httpd"],
            configs=[],
            unavailable_packages=["mcelog"],
            direct_install_packages=["custom-agent"],
            preflight_status="completed",
            base_image="quay.io/fedora/fedora-bootc:44",
        )
        assert fi.unavailable_packages == ["mcelog"]
        assert fi.direct_install_packages == ["custom-agent"]
        assert fi.preflight_status == "completed"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestArchitectPreflightAggregation -v`

Expected: FAIL — `FleetInput` does not have `unavailable_packages`.

- [ ] **Step 3: Add preflight fields to FleetInput**

In `src/yoinkc/architect/analyzer.py`, update `FleetInput`:

```python
@dataclass
class FleetInput:
    """Simplified fleet data for analysis."""

    name: str
    packages: list[str]
    configs: list[str]
    host_count: int = 0
    base_image: str = ""
    unavailable_packages: list[str] = field(default_factory=list)
    direct_install_packages: list[str] = field(default_factory=list)
    unverifiable_packages: list[str] = field(default_factory=list)
    preflight_status: str = "skipped"
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestArchitectPreflightAggregation tests/test_architect_analyzer.py -v -q`

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add src/yoinkc/architect/analyzer.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): add preflight data to architect FleetInput

FleetInput now carries unavailable_packages, direct_install_packages,
unverifiable_packages, and preflight_status for fleet-level aggregation."
```

---

### Task 10: Architect loader — populate preflight data from snapshots

**Files:**
- Modify: `src/yoinkc/architect/loader.py`
- Test: `tests/test_rpm_preflight.py` (append)

- [ ] **Step 1: Read the architect loader to understand existing API**

Read `src/yoinkc/architect/loader.py` to find where `FleetInput` objects are constructed from snapshots. Identify the exact function and line where preflight data should be extracted.

- [ ] **Step 2: Write failing test**

Append to `tests/test_rpm_preflight.py` (adjust based on actual loader API found in step 1):

```python
class TestArchitectLoaderPreflight:
    def test_loader_extracts_preflight(self, tmp_path):
        """Architect loader populates FleetInput with preflight data from snapshot."""
        from yoinkc.architect.loader import load_fleet_inputs  # adjust to actual API

        snapshot_data = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="1.0",
                                 release="1", arch="x86_64"),
                ],
                base_image="quay.io/fedora/fedora-bootc:44",
                no_baseline=True,
            ),
            preflight=PreflightResult(
                status="completed",
                available=["httpd"],
                unavailable=["mcelog"],
                direct_install=["custom-agent"],
                base_image="quay.io/fedora/fedora-bootc:44",
                repos_queried=["fedora"],
                timestamp="2026-04-09T17:00:00Z",
            ),
        )
        fleet_dir = tmp_path / "fleet-1"
        fleet_dir.mkdir()
        (fleet_dir / "inspection-snapshot.json").write_text(
            snapshot_data.model_dump_json(indent=2)
        )

        inputs = load_fleet_inputs([fleet_dir])  # adjust to actual API
        assert len(inputs) == 1
        assert inputs[0].unavailable_packages == ["mcelog"]
        assert inputs[0].direct_install_packages == ["custom-agent"]
        assert inputs[0].preflight_status == "completed"
```

NOTE: The `load_fleet_inputs` function may not exist — adjust the test to match the actual architect loader API discovered in step 1. The key point is that wherever `FleetInput` is constructed, preflight data from `snapshot.preflight` should be populated.

- [ ] **Step 3: Implement preflight extraction in architect loader**

In the function that constructs `FleetInput` from snapshots, add:

```python
    unavailable_packages=list(snapshot.preflight.unavailable) if snapshot.preflight else [],
    direct_install_packages=list(snapshot.preflight.direct_install) if snapshot.preflight else [],
    unverifiable_packages=[uv.name for uv in snapshot.preflight.unverifiable] if snapshot.preflight else [],
    preflight_status=snapshot.preflight.status if snapshot.preflight else "skipped",
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py::TestArchitectLoaderPreflight tests/test_architect_loader.py -v -q`

Expected: All PASS.

- [ ] **Step 5: Run full architect test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_architect_*.py -v -q`

Expected: All PASS.

- [ ] **Step 6: Commit**

```bash
git add src/yoinkc/architect/loader.py tests/test_rpm_preflight.py
git commit -m "feat(preflight): architect loader extracts preflight data from snapshots

Populates FleetInput with unavailable_packages, direct_install_packages,
and preflight_status from snapshot.preflight field."
```

---

### Task 11: End-to-end integration test

**Files:**
- Test: `tests/test_rpm_preflight.py` (append)

- [ ] **Step 1: Write end-to-end tests**

Append to `tests/test_rpm_preflight.py`:

```python
class TestEndToEnd:
    def test_preflight_roundtrip_via_snapshot(self, tmp_path):
        """Preflight data survives: inspect -> save snapshot -> load -> render."""
        from yoinkc.pipeline import save_snapshot, load_snapshot

        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="1.0",
                                 release="1", arch="x86_64", source_repo="baseos"),
                    PackageEntry(name="mcelog", epoch="0", version="1.0",
                                 release="1", arch="x86_64", source_repo="baseos"),
                ],
                base_image="quay.io/fedora/fedora-bootc:44",
                no_baseline=True,
            ),
            preflight=PreflightResult(
                status="completed",
                available=["httpd"],
                unavailable=["mcelog"],
                base_image="quay.io/fedora/fedora-bootc:44",
                repos_queried=["fedora"],
                timestamp="2026-04-09T17:00:00Z",
            ),
        )

        # Save and reload
        path = tmp_path / "snapshot.json"
        save_snapshot(snapshot, path)
        loaded = load_snapshot(path)

        assert loaded.preflight.status == "completed"
        assert loaded.preflight.unavailable == ["mcelog"]
        assert loaded.preflight.available == ["httpd"]

        # Render — mcelog should be excluded
        lines = section_lines(
            loaded, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        install_block = [l.strip().rstrip(" \\") for l in lines if l.startswith("    ") and not l.strip().startswith("#") and not l.strip().startswith("&&")]
        assert "mcelog" not in install_block
        assert "httpd" in install_block

    def test_skip_unavailable_preserves_all_packages(self):
        """With skipped preflight, renderer includes all packages."""
        snapshot = InspectionSnapshot(
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="1.0",
                                 release="1", arch="x86_64"),
                    PackageEntry(name="mcelog", epoch="0", version="1.0",
                                 release="1", arch="x86_64"),
                ],
                base_image="quay.io/fedora/fedora-bootc:44",
                no_baseline=True,
            ),
            preflight=PreflightResult(
                status="skipped",
                status_reason="user passed --skip-unavailable",
            ),
        )
        lines = section_lines(
            snapshot, base="quay.io/fedora/fedora-bootc:44",
            c_ext_pip=[], needs_multistage=False,
        )
        joined = "\n".join(lines)
        assert "httpd" in joined
        assert "mcelog" in joined
```

- [ ] **Step 2: Run all preflight tests**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/test_rpm_preflight.py -v`

Expected: All PASS.

- [ ] **Step 3: Run the full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/yoinkc && python -m pytest tests/ -x -q`

Expected: All PASS.

- [ ] **Step 4: Commit**

```bash
git add tests/test_rpm_preflight.py
git commit -m "test(preflight): add end-to-end roundtrip and skip-unavailable tests

Verifies preflight data survives snapshot save/load cycle and that
the renderer correctly excludes unavailable packages (or includes
all when preflight is skipped)."
```

---

## Implementation Notes

- **Naming:** The new module is `rpm_preflight.py` (not added to `preflight.py`) because the existing `preflight.py` handles container privilege checks — a completely different concern. The spec's "New module: `src/yoinkc/preflight.py`" predates the existing file.

- **Repoquery format:** Uses `--queryformat "%{name}"` to output plain package names, one per line. This avoids all NEVRA parsing ambiguity with hyphenated package names like `xorg-x11-server-Xvfb` or `compat-libstdc++-33`. Parsing is trivial: each non-empty line is a name.

- **Config staging:** During live inspection, custom repo/GPG/dnf configs exist in the snapshot's `RpmSection.repo_files`, `RpmSection.gpg_keys`, and `ConfigSection.files` — not on a filesystem. The preflight module stages these to a temporary directory (`_stage_config_tree`), mounts it into the container, and cleans up afterward. This ensures preflight sees the same repo set the rendered Containerfile will configure.

- **Bootstrap-failure classification:** When a repo-providing package (e.g., `epel-release`) fails to bootstrap, only packages whose `source_repo` matches a repo ID from that provider are marked `unverifiable`. Packages from base repos that weren't found stay `unavailable`. The mapping from provider package to repo IDs is derived by parsing `[section]` headers from the provider's `.repo` files in the snapshot.

- **Repo unreachable detection:** Parses dnf stderr for known repo failure patterns ("Failed to synchronize cache for repo", etc.) to populate `repo_unreachable`. Each unreachable repo gets its `affected_packages` populated by matching `source_repo` on packages in the install set.

- **tuned parity:** `resolve_install_set()` includes the synthetic `tuned` injection (when `kernel_boot.tuned_active` matches a valid profile), so preflight validates the exact same final install set the renderer emits.

- **Architect scope:** Tasks 9-10 deliver data plumbing only (preflight fields on `FleetInput`, loader extraction from snapshots). Per-base-image aggregation views, prevalence sorting, and layer decomposition awareness are follow-up work that consumes this data.

- **Executor pattern:** The preflight module uses the same `Executor` protocol as inspectors. In tests, the mock executor handles podman commands. In production, `subprocess_executor` runs real commands.

- **Schema version bump:** `SCHEMA_VERSION` goes from 10 to 11. This means old snapshots can't be loaded with `load_snapshot()` — they'll fail the version check. This is the existing behavior and is correct.

- **`_TOTAL_STEPS` in `run_all()`:** When adding the preflight step, consider incrementing `_TOTAL_STEPS` from 11 to 12 and adjusting the step numbers in `_section_banner` calls. Or insert the preflight between steps 1 and 2 without changing the numbering (it's a sub-step of package inspection).

- **Test fixture JSON files:** Any `.json` fixture files containing `"schema_version": 10` need updating to `11` to pass `load_snapshot` validation.

- **Diagnostic block placement:** The diagnostic block is emitted to stderr from `pipeline.py` after `_print_secrets_summary()`. This keeps it consolidated and separate from the Containerfile output.

- **Repo merge semantics:** The spec notes that bind-mounting custom repos replaces the base image's repos. The implementation should merge (copy base image repos into a staging dir, overlay custom repos). This is a detail for the `run_package_preflight` implementation — the podman command can use `podman create` + `podman cp` to extract base repos, merge, then mount. The initial implementation can use the simpler bind-mount approach with a TODO for merge semantics if testing reveals issues.

- **argv-based subprocess invocation:** Per the spec's implementation notes, all `podman` and `dnf` commands use argv lists (`["podman", "run", ...]`), never shell strings. The Executor protocol enforces this pattern.
