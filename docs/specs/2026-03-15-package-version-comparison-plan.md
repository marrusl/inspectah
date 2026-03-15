# Package Version Capture & Comparison — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture full NEVRA from the baseline image, compare package versions between host and base, surface version drift (upgrades/downgrades) in the HTML and audit reports.

**Architecture:** Change `query_packages()` and `load_baseline_packages_file()` to return `Dict[str, PackageEntry]` keyed by `name.arch`. Add EVR comparison logic in the RPM inspector to populate a new `version_changes` list on `RpmSection`. Renderers display a new "Version Changes" subsection and downgrade warnings. Pure-Python `rpmvercmp` implementation.

**Tech Stack:** Python, Pydantic models, Jinja2 templates, pytest.

**Spec:** `docs/specs/2026-03-15-package-version-comparison-design.md`

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/yoinkc/schema.py` | New `VersionChangeDirection` enum, `VersionChange` model, `version_changes` field on `RpmSection`, bump `SCHEMA_VERSION` |
| `src/yoinkc/inspectors/rpm.py` | `_compare_evr()` / `_rpmvercmp()` functions, version comparison step in `run()`, populate `base_image_only` with full NEVRA |
| `src/yoinkc/baseline.py` | Change `query_packages()` return from `Set[str]` to `Dict[str, PackageEntry]`, change `load_baseline_packages_file()` to auto-detect NEVRA vs names-only format |
| `src/yoinkc/templates/report/_packages.html.j2` | New "Version Changes" subsection, version column on dependency tree |
| `src/yoinkc/templates/report/_audit_report.html.j2` | Version drift summary in Packages section |
| `src/yoinkc/templates/report/_warnings.html.j2` | (No changes — warnings are data-driven, added by inspector) |
| `src/yoinkc/renderers/html_report.py` | Pass `version_changes` data through context |
| `tests/test_inspector_rpm.py` | EVR comparison tests, version change detection tests |
| `tests/test_baseline.py` | NEVRA baseline parsing tests, format auto-detection |
| `tests/test_plan_packages.py` | HTML/audit report rendering tests for version changes |
| `tests/fixtures/base_image_packages_nevra.txt` | New fixture: NEVRA-format baseline |
| `tests/conftest.py` | Update fixture executor to return NEVRA-format baseline |

---

## Chunk 1: Schema & EVR Comparison

### Task 1: Add schema models for version changes

**Files:**
- Modify: `src/yoinkc/schema.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_plan_packages.py`:

```python
class TestVersionChangeSchema:
    """VersionChange model and RpmSection.version_changes field."""

    def test_version_change_model(self):
        from yoinkc.schema import VersionChange, VersionChangeDirection
        vc = VersionChange(
            name="httpd",
            arch="x86_64",
            host_version="2.4.57-5.el9",
            base_version="2.4.53-11.el9",
            host_epoch="0",
            base_epoch="0",
            direction=VersionChangeDirection.DOWNGRADE,
        )
        assert vc.name == "httpd"
        assert vc.direction == VersionChangeDirection.DOWNGRADE
        d = vc.model_dump()
        assert d["direction"] == "downgrade"
        vc2 = VersionChange.model_validate(d)
        assert vc2.direction == VersionChangeDirection.DOWNGRADE

    def test_version_changes_on_rpm_section(self):
        from yoinkc.schema import RpmSection, VersionChange, VersionChangeDirection
        section = RpmSection()
        assert section.version_changes == []
        section.version_changes.append(VersionChange(
            name="curl",
            arch="x86_64",
            host_version="7.76.1-29.el9",
            base_version="7.76.1-26.el9",
            direction=VersionChangeDirection.DOWNGRADE,
        ))
        assert len(section.version_changes) == 1

    def test_version_changes_empty_by_default_roundtrip(self):
        """Existing snapshots without version_changes deserialize correctly."""
        from yoinkc.schema import RpmSection
        data = {"packages_added": [], "base_image_only": []}
        section = RpmSection.model_validate(data)
        assert section.version_changes == []

    def test_schema_version_bumped(self):
        from yoinkc.schema import SCHEMA_VERSION
        assert SCHEMA_VERSION >= 7
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangeSchema -v`

Expected: FAIL — `VersionChange` and `VersionChangeDirection` do not exist, `SCHEMA_VERSION` is 6.

- [ ] **Step 3: Add the schema models**

In `src/yoinkc/schema.py`, after the `PackageState` enum (line 53), add:

```python
class VersionChangeDirection(str, Enum):
    UPGRADE = "upgrade"      # base image has newer version than host
    DOWNGRADE = "downgrade"  # base image has older version than host


class VersionChange(BaseModel):
    """A package whose version differs between host and base image."""

    name: str
    arch: str = ""
    host_version: str        # e.g. "2.4.57-5.el9"
    base_version: str        # e.g. "2.4.53-11.el9"
    host_epoch: str = "0"
    base_epoch: str = "0"
    direction: VersionChangeDirection
```

Add to `RpmSection` (after `dnf_history_removed`, around line 96):

```python
    version_changes: List["VersionChange"] = Field(default_factory=list)
```

Bump `SCHEMA_VERSION` from 6 to 7 (line 493).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangeSchema -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(schema): add VersionChange model and version_changes field

Add VersionChangeDirection enum (upgrade/downgrade) and VersionChange
model to schema.  Add version_changes list to RpmSection with empty
default for backward compatibility.  Bump SCHEMA_VERSION to 7.

Assisted-by: Claude Code"
```

---

### Task 2: Implement pure-Python rpmvercmp and _compare_evr

**Files:**
- Modify: `src/yoinkc/inspectors/rpm.py`
- Modify: `tests/test_inspector_rpm.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_inspector_rpm.py`:

```python
import pytest
from yoinkc.inspectors.rpm import _rpmvercmp, _compare_evr
from yoinkc.schema import PackageEntry


class TestRpmvercmp:
    """Pure-Python rpmvercmp algorithm tests."""

    def test_equal(self):
        assert _rpmvercmp("1.0", "1.0") == 0

    def test_numeric_greater(self):
        assert _rpmvercmp("1.1", "1.0") > 0

    def test_numeric_less(self):
        assert _rpmvercmp("1.0", "1.1") < 0

    def test_longer_numeric(self):
        assert _rpmvercmp("1.0.1", "1.0") > 0

    def test_alpha_comparison(self):
        assert _rpmvercmp("1.0a", "1.0b") < 0

    def test_numeric_beats_alpha(self):
        assert _rpmvercmp("1.1", "1.a") > 0

    def test_leading_zeros(self):
        assert _rpmvercmp("01", "1") == 0

    def test_tilde_sorts_before_everything(self):
        """Tilde versions (pre-release) sort before non-tilde."""
        assert _rpmvercmp("1.0~rc1", "1.0") < 0

    def test_tilde_both(self):
        assert _rpmvercmp("1.0~rc1", "1.0~rc2") < 0

    def test_caret_sorts_after(self):
        """Caret versions (post-release snapshots) sort after the base."""
        assert _rpmvercmp("1.0^git1", "1.0") > 0

    def test_caret_both(self):
        assert _rpmvercmp("1.0^git1", "1.0^git2") < 0

    def test_tilde_before_caret(self):
        assert _rpmvercmp("1.0~rc1", "1.0^git1") < 0

    def test_real_world_el9(self):
        assert _rpmvercmp("5.2.15", "5.1.8") > 0

    def test_release_comparison(self):
        assert _rpmvercmp("2.el9", "1.el9") > 0

    def test_empty_equal(self):
        assert _rpmvercmp("", "") == 0

    def test_one_empty(self):
        assert _rpmvercmp("1.0", "") > 0
        assert _rpmvercmp("", "1.0") < 0


class TestCompareEvr:
    """EVR comparison combining epoch, version, release."""

    def _pkg(self, epoch="0", version="1.0", release="1.el9"):
        return PackageEntry(name="x", epoch=epoch, version=version,
                            release=release, arch="x86_64")

    def test_equal(self):
        assert _compare_evr(self._pkg(), self._pkg()) == 0

    def test_epoch_wins(self):
        """Higher epoch always wins regardless of version."""
        a = self._pkg(epoch="1", version="1.0")
        b = self._pkg(epoch="0", version="99.0")
        assert _compare_evr(a, b) > 0

    def test_version_diff(self):
        a = self._pkg(version="2.4.57")
        b = self._pkg(version="2.4.53")
        assert _compare_evr(a, b) > 0

    def test_release_diff(self):
        a = self._pkg(release="5.el9")
        b = self._pkg(release="3.el9")
        assert _compare_evr(a, b) > 0

    def test_version_then_release(self):
        """Same version, different release — release breaks the tie."""
        a = self._pkg(version="2.4.57", release="5.el9")
        b = self._pkg(version="2.4.57", release="3.el9")
        assert _compare_evr(a, b) > 0

    def test_epoch_none_treated_as_zero(self):
        a = self._pkg(epoch="0")
        b = self._pkg(epoch="0")
        assert _compare_evr(a, b) == 0
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_inspector_rpm.py::TestRpmvercmp tests/test_inspector_rpm.py::TestCompareEvr -v`

Expected: FAIL — `_rpmvercmp` and `_compare_evr` not defined.

- [ ] **Step 3: Implement _rpmvercmp and _compare_evr**

In `src/yoinkc/inspectors/rpm.py`, after the `_parse_rpm_va` function (after line 121), add:

```python
# ---------------------------------------------------------------------------
# RPM version comparison (pure-Python rpmvercmp implementation)
# ---------------------------------------------------------------------------

def _rpmvercmp(a: str, b: str) -> int:
    """Compare two RPM version/release strings using the rpmvercmp algorithm.

    Returns negative if a < b, 0 if equal, positive if a > b.
    Handles ~(pre-release) and ^(post-release snapshot) markers.
    """
    if a == b:
        return 0

    i, j = 0, 0
    while i < len(a) or j < len(b):
        # Skip non-alphanumeric, non-tilde, non-caret characters
        while i < len(a) and not a[i].isalnum() and a[i] not in ("~", "^"):
            i += 1
        while j < len(b) and not b[j].isalnum() and b[j] not in ("~", "^"):
            j += 1

        # Handle end of string
        if i >= len(a) and j >= len(b):
            return 0
        if i >= len(a):
            return -1 if b[j] == "^" else (1 if b[j] == "~" else -1)
        if j >= len(b):
            return 1 if a[i] == "^" else (-1 if a[i] == "~" else 1)

        # Tilde handling: ~ sorts before everything (pre-release)
        if a[i] == "~":
            if b[j] != "~":
                return -1
            i += 1
            j += 1
            continue
        if b[j] == "~":
            return 1

        # Caret handling: ^ sorts after empty but before any alphanumeric
        if a[i] == "^":
            if b[j] != "^":
                return 1 if j >= len(b) or not b[j].isalnum() else -1
            i += 1
            j += 1
            continue
        if b[j] == "^":
            return -1 if i >= len(a) or not a[i].isalnum() else 1

        # Extract contiguous digit or alpha segment
        if a[i].isdigit():
            # Numeric segment
            si = i
            while i < len(a) and a[i].isdigit():
                i += 1
            seg_a = a[si:i]

            sj = j
            if j < len(b) and b[j].isdigit():
                while j < len(b) and b[j].isdigit():
                    j += 1
                seg_b = b[sj:j]
            else:
                # Numeric > alpha
                return 1

            # Compare numerically (strip leading zeros)
            na = int(seg_a) if seg_a else 0
            nb = int(seg_b) if seg_b else 0
            if na != nb:
                return 1 if na > nb else -1
        else:
            # Alpha segment
            si = i
            while i < len(a) and a[i].isalpha():
                i += 1
            seg_a = a[si:i]

            sj = j
            if j < len(b) and b[j].isalpha():
                while j < len(b) and b[j].isalpha():
                    j += 1
                seg_b = b[sj:j]
            else:
                # Alpha < numeric
                return -1

            if seg_a != seg_b:
                return 1 if seg_a > seg_b else -1

    return 0


def _compare_evr(host_pkg: "PackageEntry", base_pkg: "PackageEntry") -> int:
    """Compare epoch:version-release between two PackageEntry objects.

    Returns negative if host < base, 0 if equal, positive if host > base.
    Pure-Python implementation of RPM's EVR comparison (see lib/rpmvercmp.c).
    """
    h_epoch = int(host_pkg.epoch or "0")
    b_epoch = int(base_pkg.epoch or "0")
    if h_epoch != b_epoch:
        return 1 if h_epoch > b_epoch else -1

    vc = _rpmvercmp(host_pkg.version, base_pkg.version)
    if vc != 0:
        return vc

    return _rpmvercmp(host_pkg.release, base_pkg.release)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_inspector_rpm.py::TestRpmvercmp tests/test_inspector_rpm.py::TestCompareEvr -v`

Expected: PASS

- [ ] **Step 5: Run full test suite to check for regressions**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(rpm): add pure-Python rpmvercmp and EVR comparison

Implement _rpmvercmp() following RPM's segment-by-segment algorithm
with tilde (pre-release) and caret (post-release) support.  Add
_compare_evr() for full epoch:version-release comparison.

Assisted-by: Claude Code"
```

---

## Chunk 2: Baseline NEVRA Capture

### Task 3: Create NEVRA baseline fixture

**Files:**
- Create: `tests/fixtures/base_image_packages_nevra.txt`

- [ ] **Step 1: Create the NEVRA-format fixture file**

This file mirrors `base_image_packages.txt` but in NEVRA format. Some packages will have **different versions** from the host's `rpm_qa_output.txt` to exercise upgrade/downgrade detection.

```
0:acl-2.3.1-4.el9.x86_64
0:audit-libs-3.0.7-104.el9.x86_64
0:bash-5.1.8-9.el9.x86_64
0:coreutils-8.32-35.el9.x86_64
0:curl-7.76.1-29.el9.x86_64
0:cyrus-sasl-lib-2.1.27-21.el9.x86_64
0:dbus-libs-1.12.20-8.el9.x86_64
0:expat-2.5.0-2.el9.x86_64
0:filesystem-3.16-2.el9.x86_64
0:glibc-2.34-100.el9.x86_64
0:gmp-6.2.0-13.el9.x86_64
0:grep-3.6-5.el9.x86_64
0:krb5-libs-1.21.1-2.el9.x86_64
0:libcap-2.48-9.el9.x86_64
0:libdb-5.3.28-54.el9.x86_64
0:libffi-3.4.2-8.el9.x86_64
0:libgcc-11.4.1-3.el9.x86_64
0:libselinux-3.6-1.el9.x86_64
0:libxml2-2.9.13-6.el9.x86_64
0:ncurses-libs-6.2-10.el9.x86_64
0:openssl-libs-3.0.7-28.el9.x86_64
0:pcre2-10.40-6.el9.x86_64
0:procps-ng-3.3.17-14.el9.x86_64
0:redhat-release-9.4-0.5.el9.x86_64
0:sed-4.8-9.el9.x86_64
0:setup-2.13.7-10.el9.noarch
0:shadow-utils-4.9-9.el9.x86_64
0:systemd-libs-252-32.el9.x86_64
0:util-linux-core-2.37.4-18.el9.x86_64
0:zlib-1.2.11-41.el9.x86_64
0:bash-completion-2.11-5.el9.noarch
0:vim-minimal-8.2.2637-20.el9.x86_64
0:tar-1.34-6.el9.x86_64
0:policycoreutils-3.6-2.1.el9.x86_64
0:dnf-4.14.0-9.el9.noarch
0:rpm-4.16.1.3-27.el9.x86_64
0:sudo-1.9.5p2-10.el9.x86_64
```

Note: The versions here should roughly match what the base image would have. The important thing is that some differ from the host's `rpm_qa_output.txt` so that `version_changes` is exercised. We'll verify this against the actual fixture content next.

- [ ] **Step 2: Check rpm_qa_output.txt to identify version differences**

Read `tests/fixtures/rpm_qa_output.txt` and compare `bash` version between host and baseline. The host fixture has `bash-5.2.15-2.el9` while the baseline NEVRA fixture above has `bash-5.1.8-9.el9` — this creates a **downgrade** (host is newer, base image has older).

- [ ] **Step 3: Commit**

```bash
cd yoinkc && git add -A && git commit -m "test: add NEVRA-format baseline fixture

Add base_image_packages_nevra.txt with full epoch:name-version-release.arch
format for testing version comparison.  bash version intentionally differs
from host fixture to exercise downgrade detection.

Assisted-by: Claude Code"
```

---

### Task 4: Update load_baseline_packages_file to auto-detect NEVRA

**Files:**
- Modify: `src/yoinkc/baseline.py`
- Modify: `tests/test_baseline.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_baseline.py`:

```python
from yoinkc.schema import PackageEntry


class TestBaselineNevraFormat:
    """Auto-detection of NEVRA vs names-only baseline files."""

    def test_load_nevra_format(self):
        """NEVRA lines are parsed into Dict[str, PackageEntry]."""
        result = load_baseline_packages_file(FIXTURES / "base_image_packages_nevra.txt")
        assert result is not None
        assert isinstance(result, dict)
        # Should be keyed by name.arch
        assert "bash.x86_64" in result
        pkg = result["bash.x86_64"]
        assert isinstance(pkg, PackageEntry)
        assert pkg.name == "bash"
        assert pkg.version == "5.1.8"
        assert pkg.release == "9.el9"
        assert pkg.arch == "x86_64"

    def test_load_names_only_format(self):
        """Names-only files return Dict[str, PackageEntry] with empty version fields."""
        result = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        assert result is not None
        assert isinstance(result, dict)
        assert "bash" in result
        pkg = result["bash"]
        assert isinstance(pkg, PackageEntry)
        assert pkg.name == "bash"
        assert pkg.version == ""
        assert pkg.arch == ""

    def test_load_names_only_name_set(self):
        """Names-only result can produce a name set for backward compat."""
        result = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        assert result is not None
        name_set = {p.name for p in result.values()}
        assert "bash" in name_set
        assert "glibc" in name_set
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_baseline.py::TestBaselineNevraFormat -v`

Expected: FAIL — `load_baseline_packages_file` currently returns `Set[str]`.

- [ ] **Step 3: Update load_baseline_packages_file**

In `src/yoinkc/baseline.py`, update the import to include `PackageEntry`:

```python
from typing import Dict, List, Optional, Set, Tuple
```

At the top of the file, add the import (after the existing imports, around line 20):

```python
from .schema import PackageEntry
```

Replace `load_baseline_packages_file` (lines 106-119) with:

```python
def load_baseline_packages_file(path: Path) -> Optional[Dict[str, "PackageEntry"]]:
    """Read a baseline package list from *path*.

    Auto-detects format:
    - NEVRA lines (epoch:name-version-release.arch) → Dict keyed by name.arch
    - Names-only lines → Dict keyed by name, with empty version fields
    """
    from .inspectors.rpm import _parse_nevr

    path = Path(path)
    if not path.exists():
        _debug(f"baseline packages file not found: {path}")
        return None
    try:
        text = path.read_text()
    except (PermissionError, OSError) as exc:
        _debug(f"cannot read baseline packages file: {exc}")
        return None

    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines:
        _debug("baseline packages file is empty")
        return None

    # Auto-detect: if the first non-empty line contains ":" and "-", treat as NEVRA
    is_nevra = ":" in lines[0] and "-" in lines[0]

    result: Dict[str, PackageEntry] = {}
    if is_nevra:
        for line in lines:
            pkg = _parse_nevr(line)
            if pkg:
                key = f"{pkg.name}.{pkg.arch}"
                result[key] = pkg
        _debug(f"loaded {len(result)} baseline packages (NEVRA format) from {path}")
    else:
        for line in lines:
            result[line] = PackageEntry(
                name=line, epoch="0", version="", release="", arch="",
            )
        _debug(f"loaded {len(result)} baseline package names from {path}")

    return result
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_baseline.py::TestBaselineNevraFormat -v`

Expected: PASS

- [ ] **Step 5: Do NOT run full suite yet** — other callers still expect `Set[str]`. That's fixed in the next task.

- [ ] **Step 6: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(baseline): auto-detect NEVRA vs names-only baseline files

load_baseline_packages_file() now returns Dict[str, PackageEntry].
NEVRA lines are parsed via _parse_nevr() and keyed by name.arch.
Names-only lines produce PackageEntry with empty version fields.
Format auto-detected by presence of ':' and '-' in first line.

Assisted-by: Claude Code"
```

---

### Task 5: Update query_packages to return NEVRA dict

**Files:**
- Modify: `src/yoinkc/baseline.py`
- Modify: `tests/test_baseline.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_baseline.py`:

```python
@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_query_packages_returns_nevra_dict(_mock_userns):
    """query_packages() returns Dict[str, PackageEntry] with full NEVRA."""
    nevra_output = (
        "0:bash-5.1.8-9.el9.x86_64\n"
        "0:glibc-2.34-100.el9.x86_64\n"
        "(none):setup-2.13.7-10.el9.noarch\n"
    )

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=nevra_output, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    result = resolver.query_packages("test-image:latest")
    assert result is not None
    assert isinstance(result, dict)
    assert "bash.x86_64" in result
    assert result["bash.x86_64"].version == "5.1.8"
    assert "setup.noarch" in result
    assert result["setup.noarch"].epoch == "0"  # (none) → "0"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd yoinkc && python -m pytest tests/test_baseline.py::test_query_packages_returns_nevra_dict -v`

Expected: FAIL — `query_packages()` still returns `Set[str]`.

- [ ] **Step 3: Update query_packages**

In `src/yoinkc/baseline.py`, update `BaselineResolver.query_packages()` (lines 296-324):

```python
    def query_packages(self, base_image: str) -> Optional[Dict[str, "PackageEntry"]]:
        """Run ``podman run --rm <base_image> rpm -qa`` via nsenter.

        Pulls the image first if it is not already cached, so progress is
        visible to the user.  Returns a dict of ``name.arch → PackageEntry``,
        or None on failure.
        """
        from .inspectors.rpm import _parse_nevr, RPM_QA_QUERYFORMAT

        if not self._check_registry_auth(base_image):
            return None
        if not self.pull_image(base_image):
            return None
        cmd = [
            "podman", "run", "--rm", "--cgroups=disabled", base_image,
            "rpm", "-qa", "--queryformat", RPM_QA_QUERYFORMAT + r"\n",
        ]
        _debug(f"querying base image: {' '.join(cmd)}")
        result = self._run_on_host(cmd)
        if result is None:
            return None
        if result.returncode != 0:
            _debug(f"podman run failed (rc={result.returncode}): "
                   f"{result.stderr.strip()[:800]}")
            return None
        packages: Dict[str, "PackageEntry"] = {}
        for line in result.stdout.splitlines():
            line = line.strip()
            if not line:
                continue
            pkg = _parse_nevr(line)
            if pkg:
                key = f"{pkg.name}.{pkg.arch}"
                packages[key] = pkg
        _debug(f"base image has {len(packages)} packages")
        return packages
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd yoinkc && python -m pytest tests/test_baseline.py::test_query_packages_returns_nevra_dict -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(baseline): query_packages returns NEVRA dict

Change query_packages() to use full NEVRA queryformat and return
Dict[str, PackageEntry] keyed by name.arch instead of Set[str].

Assisted-by: Claude Code"
```

---

### Task 6: Update all callers to handle Dict return type

**Files:**
- Modify: `src/yoinkc/baseline.py` — `resolve()` and `get_baseline_packages()` return types
- Modify: `src/yoinkc/inspectors/rpm.py` — `run()` function baseline handling
- Modify: `tests/conftest.py` — fixture executor returns NEVRA format

- [ ] **Step 1: Update type annotations in baseline.py**

Update the return type annotations on `resolve()` and `get_baseline_packages()` from `Tuple[Optional[Set[str]], ...]` to `Tuple[Optional[Dict[str, PackageEntry]], ...]`. The `Set` import can be removed if no longer used. Ensure the `Dict` import is present.

In `resolve()` (line 360), change signature:
```python
    def resolve(
        self,
        host_root: Path,
        os_id: str,
        version_id: str,
        baseline_packages_file: Optional[Path] = None,
        target_version: Optional[str] = None,
        target_image: Optional[str] = None,
    ) -> Tuple[Optional[Dict[str, "PackageEntry"]], Optional[str], bool]:
```

In `get_baseline_packages()` (line 388), change signature:
```python
    def get_baseline_packages(
        self,
        host_root: Path,
        os_id: str,
        version_id: str,
        baseline_packages_file: Optional[Path] = None,
        target_version: Optional[str] = None,
    ) -> Tuple[Optional[Dict[str, "PackageEntry"]], Optional[str], bool]:
```

- [ ] **Step 2: Update rpm.py run() to handle Dict baseline**

In `src/yoinkc/inspectors/rpm.py`, the `run()` function (line 569) needs updates:

1. Change type annotations. The `preflight_baseline` parameter type becomes:
```python
    preflight_baseline: Optional[Tuple[Optional[Dict[str, "PackageEntry"]], Optional[str], bool]] = None,
```

Add `Dict` to the `from typing import` line at top of file.

2. In the baseline comparison block (around lines 651-693), update the name-based set operations to derive names from the dict:

Replace the block starting at line 661 (`if baseline_names is not None and not section.no_baseline:`) — note that `baseline_names` should now be renamed to `baseline_packages` (a dict), and we derive `baseline_name_set` from it:

```python
    if installed:
        installed_names = {p.name for p in installed}
        _debug(f"installed package count: {len(installed_names)}")
        _prereq_exclude: Set[str] = set()
        _prereq_raw = os.environ.get("YOINKC_EXCLUDE_PREREQS", "").split()
        if _prereq_raw:
            _prereq_exclude = set(_prereq_raw)
            _debug(f"YOINKC_EXCLUDE_PREREQS: will exclude tool prerequisites: {sorted(_prereq_exclude)}")
        if baseline_packages is not None and not section.no_baseline:
            # Derive name set for existing name-based subtraction logic
            baseline_name_set = {p.name for p in baseline_packages.values()}
            added_names = installed_names - baseline_name_set
            if _prereq_exclude:
                _excluded = added_names & _prereq_exclude
                if _excluded:
                    _debug(f"excluded tool prerequisites from added set: {sorted(_excluded)}")
                    added_names -= _excluded
            base_only_names = baseline_name_set - installed_names
            matched_names = installed_names & baseline_name_set
            _debug(f"baseline has {len(baseline_name_set)} names, "
                   f"installed has {len(installed_names)} names")
            _debug(f"matched={len(matched_names)}, "
                   f"added (installed-baseline, after prereq exclusion)={len(added_names)}, "
                   f"base-image-only (baseline-installed)={len(base_only_names)}")
            section.baseline_package_names = sorted(baseline_name_set)
            for p in installed:
                if p.name in added_names:
                    p.state = PackageState.ADDED
                    section.packages_added.append(p)
            # Populate base_image_only with full NEVRA from baseline when available
            baseline_by_name = {}
            for bp in baseline_packages.values():
                if bp.name not in baseline_by_name:
                    baseline_by_name[bp.name] = bp
            for name in sorted(base_only_names):
                bio_pkg = baseline_by_name.get(name)
                if bio_pkg:
                    section.base_image_only.append(
                        PackageEntry(name=bio_pkg.name, epoch=bio_pkg.epoch,
                                     version=bio_pkg.version, release=bio_pkg.release,
                                     arch=bio_pkg.arch, state=PackageState.BASE_IMAGE_ONLY)
                    )
                else:
                    section.base_image_only.append(
                        PackageEntry(name=name, epoch="0", version="", release="",
                                     arch="noarch", state=PackageState.BASE_IMAGE_ONLY)
                    )

            # --- Version comparison for matched packages ---
            _has_nevra = any(p.version for p in baseline_packages.values())
            if _has_nevra:
                from ..schema import VersionChange, VersionChangeDirection
                installed_by_key = {f"{p.name}.{p.arch}": p for p in installed}
                matched_keys = installed_by_key.keys() & baseline_packages.keys()
                for key in sorted(matched_keys):
                    host_pkg = installed_by_key[key]
                    base_pkg = baseline_packages[key]
                    cmp = _compare_evr(host_pkg, base_pkg)
                    if cmp != 0:
                        direction = (VersionChangeDirection.DOWNGRADE if cmp > 0
                                     else VersionChangeDirection.UPGRADE)
                        section.version_changes.append(VersionChange(
                            name=host_pkg.name,
                            arch=host_pkg.arch,
                            host_version=f"{host_pkg.version}-{host_pkg.release}",
                            base_version=f"{base_pkg.version}-{base_pkg.release}",
                            host_epoch=host_pkg.epoch,
                            base_epoch=base_pkg.epoch,
                            direction=direction,
                        ))
                if section.version_changes:
                    n_down = sum(1 for vc in section.version_changes
                                 if vc.direction == VersionChangeDirection.DOWNGRADE)
                    n_up = len(section.version_changes) - n_down
                    _debug(f"version changes: {n_down} downgrades, {n_up} upgrades")
                    # Sort: downgrades first, then upgrades, alphabetical within
                    section.version_changes.sort(
                        key=lambda vc: (0 if vc.direction == VersionChangeDirection.DOWNGRADE else 1, vc.name)
                    )
        else:
            section.baseline_package_names = None
            for p in installed:
                if p.name not in _prereq_exclude:
                    p.state = PackageState.ADDED
                    section.packages_added.append(p)
            if _prereq_exclude:
                _skipped = [p.name for p in installed if p.name in _prereq_exclude]
                if _skipped:
                    _debug(f"(no-baseline) excluded tool prerequisites: {sorted(_skipped)}")
```

Also rename `baseline_names` → `baseline_packages` throughout the `run()` function (the variable that holds the dict, around lines 610-650).

3. Update the inline baseline resolution branches (lines 620-649) that currently assign to `baseline_names` — they now call `load_baseline_packages_file` (which returns a dict) and `resolver.query_packages` (which returns a dict). The variable name changes from `baseline_names` to `baseline_packages`.

- [ ] **Step 3: Update conftest.py fixture executor**

In `tests/conftest.py`, update the `_fixture_executor` function (line 108). The podman rpm query now needs to return NEVRA format instead of names-only:

Change line 118:
```python
    if "podman" in cmd and "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "base_image_packages_nevra.txt").read_text(), stderr="", returncode=0)
```

Also update `_make_executor` (line 36) — the `pkg_list` parameter content now needs to be the NEVRA fixture when baseline is enabled:

Change line 76:
```python
    pkg_list = (FIXTURES / "base_image_packages_nevra.txt").read_text() if with_baseline else None
```

And in `_make_executor`, the podman rpm branch (line 47) already passes through `pkg_list` — that content will now be NEVRA format.

- [ ] **Step 4: Run the full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All tests pass. Some existing tests may need minor adjustments — for example, `test_resolver_with_podman` in `test_baseline.py` asserts `"bash" in names` but names is now a dict. Update those assertions:

In `test_baseline.py::test_resolver_with_podman`, change:
```python
    assert "bash" in names  # → check dict keys
```
to:
```python
    assert any(p.name == "bash" for p in names.values())
```

Similarly update `test_get_baseline_with_file`, `test_resolve_target_image_with_file`, `test_resolve_delegates_to_get_baseline_packages`, and `test_resolve_target_image_with_executor`.

In `test_inspector_rpm.py`, `test_rpm_inspector_with_baseline_file` uses the names-only `base_image_packages.txt` — this should still work since `load_baseline_packages_file` auto-detects format.

- [ ] **Step 5: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(rpm): version comparison for matched packages

Update all baseline callers from Set[str] to Dict[str, PackageEntry].
Add version comparison step in RPM inspector: matched packages compared
via _compare_evr(), differences recorded as VersionChange entries.
base_image_only entries now populated with full NEVRA when available.
Fixture executor updated to return NEVRA-format baseline.

Assisted-by: Claude Code"
```

---

### Task 7: Add version change detection integration tests

**Files:**
- Modify: `tests/test_inspector_rpm.py`

- [ ] **Step 1: Write the integration tests**

Add to `tests/test_inspector_rpm.py`:

```python
class TestVersionChangeDetection:
    """Integration tests for version change detection in RPM inspector."""

    def test_version_changes_populated(self, host_root, fixture_executor):
        """With NEVRA baseline, version_changes should be populated for drifted packages."""
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.schema import VersionChangeDirection

        # The NEVRA baseline fixture has bash 5.1.8-9.el9 while host has 5.2.15-2.el9
        # This should be detected as a downgrade (host is newer → base downgrades it)
        section = run_rpm(host_root, fixture_executor)
        assert section.version_changes is not None

        # Find bash in version_changes
        bash_changes = [vc for vc in section.version_changes if vc.name == "bash"]
        assert len(bash_changes) == 1, f"Expected bash version change, got: {[vc.name for vc in section.version_changes]}"
        vc = bash_changes[0]
        assert vc.direction == VersionChangeDirection.DOWNGRADE
        assert "5.2.15" in vc.host_version
        assert "5.1.8" in vc.base_version

    def test_no_version_changes_with_names_only_baseline(self, host_root, fixture_executor):
        """With names-only baseline file, version_changes should be empty."""
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.baseline import load_baseline_packages_file
        baseline_pkgs = load_baseline_packages_file(FIXTURES / "base_image_packages.txt")
        preflight = (baseline_pkgs, "test-image:latest", False)
        section = run_rpm(host_root, fixture_executor, preflight_baseline=preflight)
        assert section.version_changes == []

    def test_version_changes_sorted_downgrades_first(self, host_root, fixture_executor):
        """Downgrades sort before upgrades."""
        from yoinkc.inspectors.rpm import run as run_rpm
        from yoinkc.schema import VersionChangeDirection
        section = run_rpm(host_root, fixture_executor)
        if len(section.version_changes) >= 2:
            directions = [vc.direction for vc in section.version_changes]
            downgrade_indices = [i for i, d in enumerate(directions) if d == VersionChangeDirection.DOWNGRADE]
            upgrade_indices = [i for i, d in enumerate(directions) if d == VersionChangeDirection.UPGRADE]
            if downgrade_indices and upgrade_indices:
                assert max(downgrade_indices) < min(upgrade_indices)

    def test_base_image_only_has_nevra(self, host_root, fixture_executor):
        """base_image_only entries should have version/release from NEVRA baseline."""
        from yoinkc.inspectors.rpm import run as run_rpm
        section = run_rpm(host_root, fixture_executor)
        # base_image_only entries come from packages in baseline but not on host
        # With NEVRA baseline, they should have version info
        for bio in section.base_image_only:
            # Some may still lack version if they came from names-only fallback
            # but with NEVRA baseline, they should have version
            if bio.version:
                assert bio.release, f"Package {bio.name} has version but no release"
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_inspector_rpm.py::TestVersionChangeDetection -v`

Expected: PASS (these test the code from the previous task).

- [ ] **Step 3: Commit**

```bash
cd yoinkc && git add -A && git commit -m "test: add version change detection integration tests

Verify downgrade/upgrade detection, sorting order, NEVRA population
of base_image_only entries, and names-only graceful degradation.

Assisted-by: Claude Code"
```

---

## Chunk 3: Renderer Changes

### Task 8: Add downgrade warnings to inspector output

**Files:**
- Modify: `src/yoinkc/inspectors/rpm.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_inspector_rpm.py`:

```python
def test_downgrade_warning_generated(host_root, fixture_executor):
    """Downgrades should produce a warning in the warnings list."""
    from yoinkc.inspectors.rpm import run as run_rpm
    warnings = []
    section = run_rpm(host_root, fixture_executor, warnings=warnings)
    downgrade_count = sum(1 for vc in section.version_changes
                         if vc.direction.value == "downgrade")
    assert downgrade_count > 0, "Expected at least one downgrade from NEVRA fixture"
    warning_msgs = [w.get("message", "") for w in warnings]
    assert any("downgraded" in m for m in warning_msgs), \
        f"Expected downgrade warning, got: {warning_msgs}"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd yoinkc && python -m pytest tests/test_inspector_rpm.py::test_downgrade_warning_generated -v`

Expected: FAIL — no downgrade warning generated yet.

- [ ] **Step 3: Add warning generation**

In `src/yoinkc/inspectors/rpm.py`, in the version comparison block (after the version_changes sort, inside the `if section.version_changes:` block), add:

```python
                    if n_down > 0 and warnings is not None:
                        warnings.append(make_warning(
                            "rpm",
                            f"{n_down} package(s) will be downgraded by the base image — "
                            "review the Version Changes section.",
                            "warning",
                        ))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd yoinkc && python -m pytest tests/test_inspector_rpm.py::test_downgrade_warning_generated -v`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(rpm): emit warning for package downgrades

When the base image would downgrade packages from the host, emit a
warning with the count for the warnings panel.

Assisted-by: Claude Code"
```

---

### Task 9: Add Version Changes subsection to HTML report

**Files:**
- Modify: `src/yoinkc/templates/report/_packages.html.j2`
- Modify: `src/yoinkc/renderers/html_report.py`
- Modify: `tests/test_plan_packages.py`

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_plan_packages.py`:

```python
class TestVersionChangesHtmlReport:
    """Version Changes subsection in the HTML packages tab."""

    def _render_html(self, snapshot):
        """Helper: render HTML report and return the HTML string."""
        from yoinkc.renderers.html_report import render as render_html
        from jinja2 import Environment, FileSystemLoader
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            (tmp_path / "Containerfile").write_text("FROM test")
            templates_dir = Path(__file__).parent.parent / "src" / "yoinkc" / "templates"
            env = Environment(loader=FileSystemLoader(str(templates_dir)), autoescape=True)
            render_html(snapshot, env, tmp_path)
            return (tmp_path / "report.html").read_text()

    def test_version_changes_table_present(self):
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, RpmSection, PackageEntry,
            VersionChange, VersionChangeDirection,
        )
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                    VersionChange(
                        name="curl", arch="x86_64",
                        host_version="7.76.1-26.el9", base_version="7.76.1-29.el9",
                        direction=VersionChangeDirection.UPGRADE,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        html = self._render_html(snapshot)
        assert "Version Changes" in html
        assert "bash" in html
        assert "5.2.15-2.el9" in html
        assert "5.1.8-9.el9" in html
        assert "downgrade" in html.lower()
        assert "upgrade" in html.lower()

    def test_version_column_on_dependency_tree(self):
        """Leaf package entries in the dependency tree should show version-release."""
        from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
                leaf_dep_tree={"httpd": []},
            ),
        )
        html = self._render_html(snapshot)
        assert "2.4.57-5.el9" in html

    def test_version_changes_absent_when_empty(self):
        from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        html = self._render_html(snapshot)
        assert "Version Changes" not in html
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangesHtmlReport -v`

Expected: FAIL — "Version Changes" not in rendered HTML.

- [ ] **Step 3: Add version column to dependency tree and Version Changes subsection**

In `src/yoinkc/templates/report/_packages.html.j2`:

**3a.** In the repo-grouped dependency tree table (around line 54), add a Version column header after "Leaf Package":

Change:
```jinja2
<thead><tr><th class="pf-v6-c-table__check" scope="col"></th><th scope="col">Leaf Package</th><th scope="col">Dependencies</th>
```
To:
```jinja2
<thead><tr><th class="pf-v6-c-table__check" scope="col"></th><th scope="col">Leaf Package</th><th scope="col">Version</th><th scope="col">Dependencies</th>
```

And in the data rows, after the `<td><strong>{{ entry.name }}</strong></td>` cell, add:
```jinja2
        <td class="text-sm">{{ entry.version }}</td>
```

This requires passing `version` through the `repo_groups` entries in `html_report.py` — in `_build_context()`, where `repo_groups` entries are built (around line 551), add:
```python
                "version": f"{pkg.version}-{pkg.release}" if pkg and pkg.version else "",
```

Similarly update the flat fallback table (around line 85) to include a Version column.

**3b.** Before the closing `{% endcall %}` (line 169), add the Version Changes subsection:

```jinja2

  {# ── Version Changes ──────────────────────────────────────────────────── #}
  {%- if snapshot.rpm and snapshot.rpm.version_changes %}
  <div class="pf-v6-c-card" id="pkg-version-changes">
    <div class="pf-v6-c-card__header"><div class="pf-v6-c-card__title"><h3>Version Changes ({{ snapshot.rpm.version_changes|length }})</h3></div></div>
    <div class="pf-v6-c-card__body">
    <p class="text-sm">Packages present on both host and base image with different versions. Downgrades may indicate regressions.</p>
    <table class="pf-v6-c-table" id="pkg-version-table">
      <thead><tr><th scope="col">Package</th><th scope="col">Arch</th><th scope="col">Host Version</th><th scope="col">Base Image Version</th><th scope="col">Direction</th></tr></thead>
      <tbody>
      {%- for vc in snapshot.rpm.version_changes %}
      <tr>
        <td>{{ vc.name }}</td>
        <td>{{ vc.arch }}</td>
        <td>{% if vc.host_epoch != "0" %}{{ vc.host_epoch }}:{% endif %}{{ vc.host_version }}</td>
        <td>{% if vc.base_epoch != "0" %}{{ vc.base_epoch }}:{% endif %}{{ vc.base_version }}</td>
        <td>
          {%- if vc.direction.value == "downgrade" %}
          <span class="pf-v6-c-label pf-m-compact pf-m-red"><span class="pf-v6-c-label__content">downgrade</span></span>
          {%- else %}
          <span class="pf-v6-c-label pf-m-compact pf-m-blue"><span class="pf-v6-c-label__content">upgrade</span></span>
          {%- endif %}
        </td>
      </tr>
      {%- endfor %}
      </tbody>
    </table>
    </div>
  </div>
  {%- endif %}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangesHtmlReport -v`

Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(report): add Version Changes subsection to packages tab

Show upgrade/downgrade table with PF6 labels when version_changes is
non-empty.  Downgrades use red labels, upgrades use blue.

Assisted-by: Claude Code"
```

---

### Task 10: Add version drift to audit report

**Files:**
- Modify: `src/yoinkc/renderers/audit_report.py`
- Modify: `src/yoinkc/templates/report/_audit_report.html.j2`
- Modify: `tests/test_plan_packages.py`

- [ ] **Step 1: Write the failing test**

Add to `tests/test_plan_packages.py`:

```python
class TestVersionChangesAuditReport:
    """Version drift summary in the audit report."""

    def test_audit_report_shows_version_drift(self):
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, RpmSection, PackageEntry,
            VersionChange, VersionChangeDirection,
        )
        from yoinkc.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Version" in report
        assert "bash" in report
        assert "downgrade" in report.lower()

    def test_audit_report_no_version_drift_when_empty(self):
        from yoinkc.schema import InspectionSnapshot, OsRelease, RpmSection, PackageEntry
        from yoinkc.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4.57", release="5.el9", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Version Changes" not in report
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangesAuditReport -v`

Expected: FAIL — audit report doesn't mention version changes.

- [ ] **Step 3: Add version drift to audit report renderer**

Read `src/yoinkc/renderers/audit_report.py` to understand how the audit report markdown is generated. Add a "Version Changes" subsection under the Packages section, after the "In target image only" listing:

```python
    # Version changes
    if snapshot.rpm and snapshot.rpm.version_changes:
        lines.append("")
        lines.append("### Version Changes")
        lines.append("")
        downgrades = [vc for vc in snapshot.rpm.version_changes
                      if vc.direction.value == "downgrade"]
        upgrades = [vc for vc in snapshot.rpm.version_changes
                    if vc.direction.value == "upgrade"]
        if downgrades:
            lines.append(f"**{len(downgrades)} downgrade(s)** — base image has older version than host:")
            lines.append("")
            for vc in downgrades:
                lines.append(f"- [WARNING] **{vc.name}** ({vc.arch}): {vc.host_version} → {vc.base_version}")
            lines.append("")
        if upgrades:
            lines.append(f"**{len(upgrades)} upgrade(s)** — base image has newer version than host:")
            lines.append("")
            for vc in upgrades:
                lines.append(f"- {vc.name} ({vc.arch}): {vc.host_version} → {vc.base_version}")
            lines.append("")
```

- [ ] **Step 4: Add version drift to audit report HTML template**

In `src/yoinkc/templates/report/_audit_report.html.j2`, inside the Packages `<details>` block (before the closing `</details>` at line 58), add:

```jinja2
  {%- if snapshot.rpm and snapshot.rpm.version_changes %}
  <h4>Version Changes ({{ snapshot.rpm.version_changes|length }})</h4>
  <table class="pf-v6-c-table"><thead><tr><th scope="col">Package</th><th scope="col">Host Version</th><th scope="col">Base Version</th><th scope="col">Direction</th></tr></thead><tbody>
    {%- for vc in snapshot.rpm.version_changes[:30] %}
    <tr><td>{{ vc.name }}</td><td>{{ vc.host_version }}</td><td>{{ vc.base_version }}</td><td>{{ vc.direction.value }}</td></tr>
    {%- endfor %}
    {%- if snapshot.rpm.version_changes|length > 30 %}
    <tr><td colspan="4"><em>+{{ snapshot.rpm.version_changes|length - 30 }} more — see Packages tab</em></td></tr>
    {%- endif %}
  </tbody></table>
  {%- endif %}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangesAuditReport -v`

Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All pass.

- [ ] **Step 7: Commit**

```bash
cd yoinkc && git add -A && git commit -m "feat(report): add version drift to audit report

Show downgrades with warning prefix and upgrades as informational
in both the markdown audit report and the HTML audit tab.

Assisted-by: Claude Code"
```

---

### Task 11: Final integration test and cleanup

**Files:**
- Modify: `tests/test_plan_packages.py` (or existing renderer test if needed)

- [ ] **Step 1: Write a full roundtrip test**

Add to `tests/test_plan_packages.py`:

```python
class TestVersionChangeRoundtrip:
    """Snapshot roundtrip for version_changes."""

    def test_version_changes_survive_json_roundtrip(self):
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, RpmSection,
            VersionChange, VersionChangeDirection,
        )
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                version_changes=[
                    VersionChange(
                        name="bash", arch="x86_64",
                        host_version="5.2.15-2.el9", base_version="5.1.8-9.el9",
                        direction=VersionChangeDirection.DOWNGRADE,
                    ),
                ],
            ),
        )
        json_str = snapshot.model_dump_json()
        loaded = InspectionSnapshot.model_validate_json(json_str)
        assert len(loaded.rpm.version_changes) == 1
        assert loaded.rpm.version_changes[0].name == "bash"
        assert loaded.rpm.version_changes[0].direction == VersionChangeDirection.DOWNGRADE
```

- [ ] **Step 2: Run test**

Run: `cd yoinkc && python -m pytest tests/test_plan_packages.py::TestVersionChangeRoundtrip -v`

Expected: PASS

- [ ] **Step 3: Run full test suite one final time**

Run: `cd yoinkc && python -m pytest -x -q`

Expected: All pass.

- [ ] **Step 4: Commit**

```bash
cd yoinkc && git add -A && git commit -m "test: add version change JSON roundtrip test

Verify VersionChange entries survive serialization/deserialization
in InspectionSnapshot.

Assisted-by: Claude Code"
```

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 — Schema & EVR | Tasks 1-2 | `VersionChange` model, `_rpmvercmp`, `_compare_evr` |
| 2 — Baseline NEVRA | Tasks 3-7 | NEVRA fixture, `load_baseline_packages_file` auto-detect, `query_packages` NEVRA return, caller updates, version change detection |
| 3 — Renderers | Tasks 8-11 | Downgrade warnings, HTML Version Changes table, audit report version drift, roundtrip test |

Total commits: ~9, each small and independently testable.
