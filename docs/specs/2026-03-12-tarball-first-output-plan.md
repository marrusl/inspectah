# Tarball-First Output Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make tarball the default output format for yoinkc, move entitlement cert bundling into yoinkc proper, and slim run-yoinkc.sh down to podman setup and launch.

**Architecture:** Renderers continue writing to a directory (now a temp dir). A new pipeline step bundles entitlement certs. A final step tars the temp dir into a `.tar.gz` and cleans up. `--output-dir` preserves the old directory behavior.

**Tech Stack:** Python 3.11+ (already required by pyproject.toml; runs in Fedora container), stdlib `tarfile`/`tempfile`/`shutil`/`socket`, pytest

**Spec:** `docs/specs/2026-03-12-tarball-first-output-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Create | `src/yoinkc/entitlement.py` | Detect and copy RHEL entitlement certs |
| Create | `src/yoinkc/packaging.py` | Create tarball from output directory; hostname/stamp helpers |
| Modify | `src/yoinkc/cli.py` | New flag structure: `-o`, `--output-dir`, `--no-entitlement`, mutual exclusivity |
| Modify | `src/yoinkc/pipeline.py` | Temp dir orchestration, entitlement bundling step, tarball/directory output modes |
| Modify | `src/yoinkc/__main__.py` | Wire new args, gate `--validate`/`--push-to-github` on `--output-dir` |
| Modify | `run-yoinkc.sh` | Remove cert bundling, tarball creation, simplify arg handling |
| Create | `tests/test_entitlement.py` | Tests for entitlement cert detection and bundling |
| Create | `tests/test_packaging.py` | Tests for tarball creation and hostname helpers |
| Modify | `tests/test_cli.py` | Update for new flags and validation rules |
| Modify | `tests/test_pipeline.py` | Update for new pipeline flow |
| Modify | `tests/test_integration.py` | Add tarball round-trip test |

---

## Chunk 1: Entitlement Cert Bundling Module

### Task 1: Entitlement cert detection and copying

**Files:**
- Create: `src/yoinkc/entitlement.py`
- Create: `tests/test_entitlement.py`

- [ ] **Step 1: Write failing tests for entitlement bundling**

Create `tests/test_entitlement.py`:

```python
"""Tests for entitlement cert detection and bundling."""

import tempfile
from pathlib import Path

import pytest

from yoinkc.entitlement import bundle_entitlement_certs


def _make_host_root_with_certs(root: Path) -> None:
    """Create a fake host root with entitlement certs and rhsm config."""
    ent_dir = root / "etc" / "pki" / "entitlement"
    ent_dir.mkdir(parents=True)
    (ent_dir / "123456.pem").write_text("cert-data")
    (ent_dir / "123456-key.pem").write_text("key-data")
    rhsm_dir = root / "etc" / "rhsm"
    rhsm_dir.mkdir(parents=True)
    (rhsm_dir / "rhsm.conf").write_text("[rhsm]\nbaseurl=https://cdn.redhat.com")


def test_bundles_certs_when_present():
    """Entitlement certs and rhsm dir are copied to output."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()
        _make_host_root_with_certs(host_root)

        bundle_entitlement_certs(host_root, output_dir)

        assert (output_dir / "entitlement" / "123456.pem").read_text() == "cert-data"
        assert (output_dir / "entitlement" / "123456-key.pem").read_text() == "key-data"
        assert (output_dir / "rhsm" / "rhsm.conf").exists()


def test_skips_silently_when_no_certs():
    """No error or output when entitlement certs do not exist."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        host_root.mkdir()
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_entitlement_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()
        assert not (output_dir / "rhsm").exists()


def test_skips_silently_when_host_root_missing():
    """No error when HOST_ROOT does not exist at all."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "nonexistent"
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_entitlement_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()


def test_copies_only_pem_files():
    """Only .pem files from the entitlement dir are copied, not other files."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        ent_dir = host_root / "etc" / "pki" / "entitlement"
        ent_dir.mkdir(parents=True)
        (ent_dir / "cert.pem").write_text("cert")
        (ent_dir / "README").write_text("ignore me")
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_entitlement_certs(host_root, output_dir)

        assert (output_dir / "entitlement" / "cert.pem").exists()
        assert not (output_dir / "entitlement" / "README").exists()


def test_bundles_rhsm_without_entitlement():
    """rhsm dir is bundled even if entitlement certs are absent."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp) / "host"
        rhsm_dir = host_root / "etc" / "rhsm"
        rhsm_dir.mkdir(parents=True)
        (rhsm_dir / "rhsm.conf").write_text("[rhsm]")
        output_dir = Path(tmp) / "output"
        output_dir.mkdir()

        bundle_entitlement_certs(host_root, output_dir)

        assert not (output_dir / "entitlement").exists()
        assert (output_dir / "rhsm" / "rhsm.conf").exists()
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_entitlement.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'yoinkc.entitlement'`

- [ ] **Step 3: Implement entitlement module**

Create `src/yoinkc/entitlement.py`:

```python
"""Detect and bundle RHEL entitlement certs into the output directory."""

import shutil
from pathlib import Path


def bundle_entitlement_certs(host_root: Path, output_dir: Path) -> None:
    """Copy entitlement certs and rhsm config from host_root into output_dir.

    Silently skips if host_root does not exist or certs are not found.
    """
    if not host_root.is_dir():
        return

    # Copy .pem files from /etc/pki/entitlement/
    ent_src = host_root / "etc" / "pki" / "entitlement"
    if ent_src.is_dir():
        pems = list(ent_src.glob("*.pem"))
        if pems:
            ent_dst = output_dir / "entitlement"
            ent_dst.mkdir(exist_ok=True)
            for pem in pems:
                shutil.copy2(pem, ent_dst / pem.name)

    # Copy /etc/rhsm/ tree
    rhsm_src = host_root / "etc" / "rhsm"
    if rhsm_src.is_dir():
        shutil.copytree(rhsm_src, output_dir / "rhsm")
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_entitlement.py -v`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
cd yoinkc
git add src/yoinkc/entitlement.py tests/test_entitlement.py
git commit -m "feat: add entitlement cert bundling module

Move RHEL entitlement cert detection and bundling from run-yoinkc.sh
into yoinkc proper. Detects certs at {HOST_ROOT}/etc/pki/entitlement/
and rhsm config at {HOST_ROOT}/etc/rhsm/, silently skipping if absent.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Chunk 2: Tarball Packaging Module

### Task 2: Tarball creation and hostname helpers

**Files:**
- Create: `src/yoinkc/packaging.py`
- Create: `tests/test_packaging.py`

- [ ] **Step 1: Write failing tests for packaging**

Create `tests/test_packaging.py`:

```python
"""Tests for tarball packaging and hostname/stamp helpers."""

import tarfile
import tempfile
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

import pytest

from yoinkc.packaging import create_tarball, get_output_stamp, sanitize_hostname


def test_sanitize_hostname_simple():
    assert sanitize_hostname("webserver01") == "webserver01"


def test_sanitize_hostname_strips_unsafe_chars():
    assert sanitize_hostname("web/server:01") == "webserver01"


def test_sanitize_hostname_empty_fallback():
    assert sanitize_hostname("") == "unknown"


def test_sanitize_hostname_all_unsafe_fallback():
    assert sanitize_hostname("///") == "unknown"


def test_get_output_stamp_format():
    """Stamp matches HOSTNAME-YYYYMMDD-HHMMSS format."""
    stamp = get_output_stamp()
    parts = stamp.rsplit("-", 2)
    assert len(parts) == 3
    # Date part should be 8 digits
    assert len(parts[1]) == 8 and parts[1].isdigit()
    # Time part should be 6 digits
    assert len(parts[2]) == 6 and parts[2].isdigit()


def test_create_tarball_produces_valid_tar_gz():
    """Tarball contains all files from the source directory under a prefix dir."""
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "src"
        src.mkdir()
        (src / "Containerfile").write_text("FROM fedora:latest")
        config = src / "config" / "etc"
        config.mkdir(parents=True)
        (config / "hosts").write_text("127.0.0.1 localhost")

        tarball_path = Path(tmp) / "output.tar.gz"
        create_tarball(src, tarball_path, prefix="test-host-20260312-120000")

        assert tarball_path.exists()
        with tarfile.open(tarball_path, "r:gz") as tf:
            names = tf.getnames()
            assert "test-host-20260312-120000/Containerfile" in names
            assert "test-host-20260312-120000/config/etc/hosts" in names


def test_create_tarball_contents_match_source():
    """File contents inside the tarball match the source."""
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "src"
        src.mkdir()
        (src / "file.txt").write_text("hello world")

        tarball_path = Path(tmp) / "out.tar.gz"
        create_tarball(src, tarball_path, prefix="stamp")

        with tarfile.open(tarball_path, "r:gz") as tf:
            member = tf.getmember("stamp/file.txt")
            content = tf.extractfile(member).read().decode()
            assert content == "hello world"


def test_create_tarball_raises_on_write_failure():
    """Tarball creation raises if the output path is not writable."""
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "src"
        src.mkdir()
        (src / "f.txt").write_text("x")

        bad_path = Path("/nonexistent/dir/out.tar.gz")
        with pytest.raises(OSError):
            create_tarball(src, bad_path, prefix="stamp")
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_packaging.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'yoinkc.packaging'`

- [ ] **Step 3: Implement packaging module**

Create `src/yoinkc/packaging.py`:

```python
"""Create tarball from rendered output directory."""

import re
import socket
import tarfile
from datetime import datetime
from pathlib import Path


def sanitize_hostname(hostname: str) -> str:
    """Remove characters unsafe for filenames; fall back to 'unknown'."""
    cleaned = re.sub(r"[^\w.-]", "", hostname)
    return cleaned or "unknown"


def _resolve_hostname() -> str:
    """Hostname with fallback chain: socket → /etc/hostname → 'unknown'."""
    try:
        name = socket.gethostname()
        if name:
            return name
    except OSError:
        pass
    try:
        name = Path("/etc/hostname").read_text().strip()
        if name:
            return name
    except OSError:
        pass
    return "unknown"


def get_output_stamp() -> str:
    """Return 'HOSTNAME-YYYYMMDD-HHMMSS' stamp for tarball naming."""
    hostname = sanitize_hostname(_resolve_hostname())
    now = datetime.now().strftime("%Y%m%d-%H%M%S")
    return f"{hostname}-{now}"


def create_tarball(source_dir: Path, tarball_path: Path, prefix: str) -> None:
    """Create a gzipped tarball from source_dir with all entries under prefix/.

    Raises OSError if the tarball cannot be written.
    """
    with tarfile.open(tarball_path, "w:gz") as tf:
        for item in sorted(source_dir.rglob("*")):
            arcname = f"{prefix}/{item.relative_to(source_dir)}"
            tf.add(item, arcname=arcname, recursive=False)
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_packaging.py -v`
Expected: All 7 tests PASS

- [ ] **Step 5: Commit**

```bash
cd yoinkc
git add src/yoinkc/packaging.py tests/test_packaging.py
git commit -m "feat: add tarball packaging module

Create tarball from rendered output directory with HOSTNAME-TIMESTAMP
prefix. Uses Python's tarfile module — no external tar dependency.
Includes hostname resolution with fallback chain and sanitization.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Chunk 3: CLI Changes

### Task 3: Restructure CLI flags

**Files:**
- Modify: `src/yoinkc/cli.py`
- Modify: `tests/test_cli.py`

- [ ] **Step 1: Write failing tests for new CLI behavior**

Add to `tests/test_cli.py` (keep existing tests but update them for the
new flag structure). The following tests need to change or be added:

Update `test_defaults` — `args.output_dir` is now `None` by default
(tarball mode), and there's no `-o` mapping to output_dir anymore:

```python
def test_defaults():
    args = parse_args([])
    assert args.host_root == Path("/host")
    assert args.output_file is None
    assert args.output_dir is None
    assert args.no_entitlement is False
    assert args.from_snapshot is None
    assert args.inspect_only is False
    assert args.baseline_packages is None
    assert args.no_baseline is False
    assert args.config_diffs is False
    assert args.deep_binary_scan is False
    assert args.query_podman is False
    assert args.validate is False
    assert args.push_to_github is None
    assert args.public is False
    assert args.yes is False
```

Add new tests:

```python
def test_output_file_short_flag():
    """'-o' sets the tarball output path."""
    args = parse_args(["-o", "/tmp/out.tar.gz"])
    assert args.output_file == Path("/tmp/out.tar.gz")
    assert args.output_dir is None


def test_output_dir_long_flag():
    """'--output-dir' sets directory output mode."""
    args = parse_args(["--output-dir", "/tmp/outdir"])
    assert args.output_dir == Path("/tmp/outdir")
    assert args.output_file is None


def test_output_file_and_output_dir_mutually_exclusive():
    """-o and --output-dir together must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["-o", "/tmp/out.tar.gz", "--output-dir", "/tmp/outdir"])


def test_no_entitlement_flag():
    args = parse_args(["--no-entitlement"])
    assert args.no_entitlement is True


def test_validate_requires_output_dir():
    """--validate without --output-dir must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--validate"])


def test_push_to_github_requires_output_dir():
    """--push-to-github without --output-dir must be rejected."""
    with pytest.raises(SystemExit):
        parse_args(["--push-to-github", "owner/repo"])


def test_validate_with_output_dir_accepted():
    args = parse_args(["--output-dir", "/tmp/out", "--validate"])
    assert args.validate is True
    assert args.output_dir == Path("/tmp/out")


def test_push_to_github_with_output_dir_accepted():
    args = parse_args(["--output-dir", "/tmp/out", "--push-to-github", "owner/repo"])
    assert args.push_to_github == "owner/repo"
```

Update `test_from_snapshot_flags` — remove `args.output_dir` default
assertion (now `None` by default) and remove `--output-dir` reference
from `test_inspect_only_flags`:

```python
def test_from_snapshot_flags():
    """Flags compatible with --from-snapshot parse correctly."""
    args = parse_args([
        "--host-root", "/mnt/host",
        "--output-dir", "/tmp/out",
        "--from-snapshot", "/tmp/snap.json",
        "--validate",
        "--push-to-github", "owner/repo",
        "--public",
        "--yes",
    ])
    assert args.host_root == Path("/mnt/host")
    assert args.output_dir == Path("/tmp/out")
    assert args.from_snapshot == Path("/tmp/snap.json")
    assert args.inspect_only is False
    assert args.validate is True
    assert args.push_to_github == "owner/repo"
    assert args.public is True
    assert args.yes is True
```

Update `test_inspect_only_flags`:

```python
def test_inspect_only_flags():
    """Flags compatible with --inspect-only parse correctly."""
    args = parse_args([
        "--host-root", "/mnt/host",
        "--inspect-only",
        "--baseline-packages", "/tmp/pkgs.txt",
        "--config-diffs",
        "--deep-binary-scan",
        "--query-podman",
    ])
    assert args.host_root == Path("/mnt/host")
    assert args.from_snapshot is None
    assert args.inspect_only is True
    assert args.baseline_packages == Path("/tmp/pkgs.txt")
    assert args.config_diffs is True
    assert args.deep_binary_scan is True
    assert args.query_podman is True
```

Update `test_main_git_init_failure_returns_error` — already uses
`--output-dir`, no change needed.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_cli.py -v`
Expected: Multiple failures — new attrs don't exist, `-o` still maps
to `output_dir`, new validation not in place.

- [ ] **Step 3: Implement CLI changes**

Replace the `-o`/`--output-dir` argument definition in
`src/yoinkc/cli.py` (lines 21-28) with:

```python
    # Output mode: tarball (default) or directory
    output_group = parser.add_mutually_exclusive_group()
    output_group.add_argument(
        "-o",
        dest="output_file",
        type=Path,
        metavar="FILE",
        help="Write tarball to FILE (default: HOSTNAME-TIMESTAMP.tar.gz in cwd)",
    )
    output_group.add_argument(
        "--output-dir",
        dest="output_dir",
        type=Path,
        metavar="DIR",
        help="Write files to a directory instead of producing a tarball",
    )
    parser.add_argument(
        "--no-entitlement",
        action="store_true",
        help="Skip bundling RHEL entitlement certs into the output",
    )
```

Add validation after existing checks (before `return args`):

```python
    if (args.validate or args.push_to_github) and args.output_dir is None:
        parser.error(
            "--validate and --push-to-github require --output-dir "
            "(directory output mode)"
        )
```

- [ ] **Step 4: Run CLI tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_cli.py -v`
Expected: All tests PASS

- [ ] **Step 5: Run full test suite — expect some failures in other test files**

Run: `cd yoinkc && python -m pytest --tb=short 2>&1 | tail -20`
Expected: Some tests in `test_pipeline.py`, `test_integration.py`, or
other files may fail because they reference `args.output_dir` with the
old default. That's expected — we fix those in later tasks.

- [ ] **Step 6: Commit**

```bash
cd yoinkc
git add src/yoinkc/cli.py tests/test_cli.py
git commit -m "feat(cli): restructure output flags for tarball-first mode

Split -o (now tarball output path) from --output-dir (directory mode).
Add --no-entitlement flag. Validate that --validate and --push-to-github
require --output-dir.

Breaking change: -o no longer means --output-dir.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Chunk 4: Pipeline and Main Refactor

### Task 4: Refactor pipeline for temp dir and tarball output

**Files:**
- Modify: `src/yoinkc/pipeline.py`
- Modify: `src/yoinkc/__main__.py`
- Modify: `tests/test_pipeline.py`
- Modify: `tests/test_cli.py` (if any main() tests need updating)

- [ ] **Step 1: Write failing tests for new pipeline behavior**

Replace `tests/test_pipeline.py` entirely:

```python
"""Tests for pipeline.py: snapshot handling and output modes."""

import json
import tarfile
import tempfile
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from yoinkc.pipeline import load_snapshot, save_snapshot, run_pipeline
from yoinkc.schema import InspectionSnapshot, SCHEMA_VERSION


def test_load_snapshot_version_mismatch_raises():
    """Loading a snapshot with a different schema version must raise ValueError."""
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "snap.json"
        p.write_text(json.dumps({"schema_version": 999}))
        with pytest.raises(ValueError, match="different version"):
            load_snapshot(p)


def test_load_snapshot_current_version_succeeds():
    """Loading a snapshot at the current schema version must succeed."""
    snapshot = InspectionSnapshot(meta={"host_root": "/host"})
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / "snap.json"
        save_snapshot(snapshot, p)
        loaded = load_snapshot(p)
    assert loaded.schema_version == SCHEMA_VERSION


def _make_snapshot() -> InspectionSnapshot:
    return InspectionSnapshot(meta={"host_root": "/host"})


def _noop_inspectors(host_root: Path) -> InspectionSnapshot:
    return _make_snapshot()


def _tracking_renderer(calls: list):
    """Return a renderer callable that records its arguments."""
    def renderer(snapshot, output_dir):
        calls.append(output_dir)
        (output_dir / "Containerfile").write_text("FROM fedora:latest")
    return renderer


def test_tarball_mode_produces_tar_gz():
    """Default tarball mode produces a .tar.gz file."""
    render_calls = []
    with tempfile.TemporaryDirectory() as tmp:
        tarball_path = Path(tmp) / "out.tar.gz"
        run_pipeline(
            host_root=Path("/host"),
            run_inspectors=_noop_inspectors,
            run_renderers=_tracking_renderer(render_calls),
            output_file=tarball_path,
        )
        assert tarball_path.exists()
        with tarfile.open(tarball_path, "r:gz") as tf:
            names = tf.getnames()
            containerfiles = [n for n in names if n.endswith("Containerfile")]
            assert len(containerfiles) == 1


def test_tarball_mode_cleans_up_temp_dir():
    """Temp directory is removed after tarball is created."""
    render_calls = []
    with tempfile.TemporaryDirectory() as tmp:
        tarball_path = Path(tmp) / "out.tar.gz"
        run_pipeline(
            host_root=Path("/host"),
            run_inspectors=_noop_inspectors,
            run_renderers=_tracking_renderer(render_calls),
            output_file=tarball_path,
        )
        # The renderer was called with a temp dir that should now be gone
        assert len(render_calls) == 1
        assert not render_calls[0].exists()


def test_output_dir_mode_writes_directory():
    """--output-dir mode writes files to the specified directory."""
    with tempfile.TemporaryDirectory() as tmp:
        out_dir = Path(tmp) / "output"
        run_pipeline(
            host_root=Path("/host"),
            run_inspectors=_noop_inspectors,
            run_renderers=_tracking_renderer([]),
            output_dir=out_dir,
        )
        assert out_dir.is_dir()
        assert (out_dir / "Containerfile").exists()
        assert (out_dir / "inspection-snapshot.json").exists()


def test_inspect_only_saves_snapshot_to_cwd():
    """--inspect-only writes snapshot to cwd, no renderers, no tarball."""
    renderer = MagicMock()
    with tempfile.TemporaryDirectory() as tmp:
        cwd = Path(tmp)
        run_pipeline(
            host_root=Path("/host"),
            run_inspectors=_noop_inspectors,
            run_renderers=renderer,
            inspect_only=True,
            cwd=cwd,
        )
        assert (cwd / "inspection-snapshot.json").exists()
        renderer.assert_not_called()


def test_entitlement_bundling_in_tarball(tmp_path):
    """Entitlement certs from host_root are included in tarball."""
    # Create fake host with certs
    host_root = tmp_path / "host"
    ent_dir = host_root / "etc" / "pki" / "entitlement"
    ent_dir.mkdir(parents=True)
    (ent_dir / "cert.pem").write_text("certdata")

    tarball_path = tmp_path / "out.tar.gz"
    run_pipeline(
        host_root=host_root,
        run_inspectors=lambda hr: _make_snapshot(),
        run_renderers=_tracking_renderer([]),
        output_file=tarball_path,
    )
    with tarfile.open(tarball_path, "r:gz") as tf:
        cert_entries = [n for n in tf.getnames() if "entitlement/cert.pem" in n]
        assert len(cert_entries) == 1


def test_no_entitlement_skips_bundling(tmp_path):
    """--no-entitlement suppresses cert bundling."""
    host_root = tmp_path / "host"
    ent_dir = host_root / "etc" / "pki" / "entitlement"
    ent_dir.mkdir(parents=True)
    (ent_dir / "cert.pem").write_text("certdata")

    tarball_path = tmp_path / "out.tar.gz"
    run_pipeline(
        host_root=host_root,
        run_inspectors=lambda hr: _make_snapshot(),
        run_renderers=_tracking_renderer([]),
        output_file=tarball_path,
        no_entitlement=True,
    )
    with tarfile.open(tarball_path, "r:gz") as tf:
        cert_entries = [n for n in tf.getnames() if "entitlement" in n]
        assert len(cert_entries) == 0


def test_from_snapshot_skips_entitlement_bundling(tmp_path):
    """--from-snapshot mode silently skips entitlement bundling (host may not be mounted)."""
    # Create a snapshot file
    snapshot = _make_snapshot()
    snap_path = tmp_path / "snap.json"
    from yoinkc.pipeline import save_snapshot
    save_snapshot(snapshot, snap_path)

    # Create fake host with certs (should NOT be bundled)
    host_root = tmp_path / "host"
    ent_dir = host_root / "etc" / "pki" / "entitlement"
    ent_dir.mkdir(parents=True)
    (ent_dir / "cert.pem").write_text("certdata")

    tarball_path = tmp_path / "out.tar.gz"
    run_pipeline(
        host_root=host_root,
        run_inspectors=_noop_inspectors,
        run_renderers=_tracking_renderer([]),
        from_snapshot_path=snap_path,
        output_file=tarball_path,
    )
    with tarfile.open(tarball_path, "r:gz") as tf:
        cert_entries = [n for n in tf.getnames() if "entitlement" in n]
        assert len(cert_entries) == 0


def test_default_tarball_name_in_cwd(tmp_path):
    """When no -o or --output-dir is given, tarball is written to CWD."""
    run_pipeline(
        host_root=Path("/host"),
        run_inspectors=_noop_inspectors,
        run_renderers=_tracking_renderer([]),
        cwd=tmp_path,
    )
    tarballs = list(tmp_path.glob("*.tar.gz"))
    assert len(tarballs) == 1
    assert tarballs[0].name.endswith(".tar.gz")


def test_error_preserves_temp_dir(tmp_path):
    """If rendering fails, temp dir is preserved and error message includes its path."""
    def failing_renderer(snapshot, output_dir):
        (output_dir / "partial.txt").write_text("partial")
        raise RuntimeError("render failed")

    tarball_path = tmp_path / "out.tar.gz"
    with pytest.raises(RuntimeError, match="render failed"):
        run_pipeline(
            host_root=Path("/host"),
            run_inspectors=_noop_inspectors,
            run_renderers=failing_renderer,
            output_file=tarball_path,
        )
    # Tarball should NOT exist
    assert not tarball_path.exists()
    # But the partial output should be recoverable somewhere in /tmp
    # (we can't easily check the exact path, but verify the error propagates)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd yoinkc && python -m pytest tests/test_pipeline.py -v`
Expected: FAIL — `run_pipeline()` signature doesn't match new args

- [ ] **Step 3: Implement pipeline changes**

Replace `src/yoinkc/pipeline.py`:

```python
"""
Pipeline orchestrator: run inspectors (or load snapshot), redact, optionally
bundle entitlement certs, then produce a tarball or write to a directory.
"""

import json
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Callable, Optional

from .entitlement import bundle_entitlement_certs
from .packaging import create_tarball, get_output_stamp
from .redact import redact_snapshot
from .schema import InspectionSnapshot, SCHEMA_VERSION


def load_snapshot(path: Path) -> InspectionSnapshot:
    """Load and deserialize an inspection snapshot from JSON."""
    data = json.loads(path.read_text())
    file_version = data.get("schema_version", 1)
    if file_version != SCHEMA_VERSION:
        raise ValueError(
            f"Snapshot was created by a different version of yoinkc "
            f"(schema v{file_version}, expected v{SCHEMA_VERSION}). "
            f"Re-run the inspection to generate a new snapshot."
        )
    return InspectionSnapshot.model_validate(data)


def save_snapshot(snapshot: InspectionSnapshot, path: Path) -> None:
    """Serialize snapshot to JSON."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(snapshot.model_dump_json(indent=2))


def run_pipeline(
    *,
    host_root: Path,
    run_inspectors: Callable[[Path], InspectionSnapshot],
    run_renderers: Callable[[InspectionSnapshot, Path], None],
    from_snapshot_path: Optional[Path] = None,
    inspect_only: bool = False,
    output_file: Optional[Path] = None,
    output_dir: Optional[Path] = None,
    no_entitlement: bool = False,
    cwd: Optional[Path] = None,
) -> InspectionSnapshot:
    """Run the yoinkc pipeline.

    Output modes (mutually exclusive):
    - output_file: write tarball to this path
    - output_dir: write files to this directory
    - neither: write tarball to CWD with auto-generated name

    inspect_only: save snapshot to CWD and exit early.
    cwd: override working directory for default output paths (testing).
    """
    working_dir = cwd or Path.cwd()

    # Load or build the snapshot
    if from_snapshot_path is not None:
        snapshot = load_snapshot(from_snapshot_path)
        snapshot = redact_snapshot(snapshot)
    else:
        snapshot = run_inspectors(host_root)
        snapshot = redact_snapshot(snapshot)

    # --inspect-only: save snapshot and return
    if inspect_only:
        save_snapshot(snapshot, working_dir / "inspection-snapshot.json")
        return snapshot

    # Render into a temp directory
    tmp_dir = Path(tempfile.mkdtemp(prefix="yoinkc-"))
    try:
        save_snapshot(snapshot, tmp_dir / "inspection-snapshot.json")
        run_renderers(snapshot, tmp_dir)

        # Bundle entitlement certs (skip in --from-snapshot mode where
        # host filesystem may not be mounted)
        if not no_entitlement and from_snapshot_path is None:
            bundle_entitlement_certs(host_root, tmp_dir)

        # Output: tarball or directory
        if output_dir is not None:
            # Directory mode
            output_dir.mkdir(parents=True, exist_ok=True)
            for item in tmp_dir.iterdir():
                dest = output_dir / item.name
                if item.is_dir():
                    shutil.copytree(item, dest, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, dest)
        else:
            # Tarball mode
            stamp = get_output_stamp()
            if output_file is None:
                output_file = working_dir / f"{stamp}.tar.gz"
            create_tarball(tmp_dir, output_file, prefix=stamp)
            print(f"Output: {output_file}")
    except Exception:
        print(
            f"Error during output. Rendered files preserved at: {tmp_dir}",
            file=sys.stderr,
        )
        raise
    else:
        shutil.rmtree(tmp_dir)

    return snapshot
```

- [ ] **Step 4: Run pipeline tests to verify they pass**

Run: `cd yoinkc && python -m pytest tests/test_pipeline.py -v`
Expected: All tests PASS

- [ ] **Step 5: Update `__main__.py` for new pipeline interface**

Replace `src/yoinkc/__main__.py`:

```python
"""
CLI entry point. Parses args and delegates to pipeline.
"""

import os
import sys
import traceback
from pathlib import Path
from typing import Optional

from .cli import parse_args
from .pipeline import run_pipeline
from .schema import InspectionSnapshot


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
    )


def _run_renderers(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Run all renderers."""
    from .renderers import run_all

    run_all(snapshot, output_dir)


def main(argv: Optional[list] = None) -> int:
    args = parse_args(argv)

    # Preflight: bail out early if container privileges are missing.
    if (
        args.from_snapshot is None
        and str(args.host_root) != "/"
        and not args.skip_preflight
    ):
        from .preflight import check_container_privileges
        errors = check_container_privileges()
        if errors:
            print("ERROR: container privilege checks failed:\n", file=sys.stderr)
            for err in errors:
                print(f"  • {err}", file=sys.stderr)
            print(
                "\nRun with the required flags, e.g.:\n"
                "  sudo podman run --rm --pid=host --privileged "
                "--security-opt label=disable \\\n"
                "    -v /:/host:ro yoinkc\n"
                "\nOr use --skip-preflight to bypass these checks.",
                file=sys.stderr,
            )
            return 1

    try:
        def run_inspectors(host_root: Path):
            return _run_inspectors(host_root, args)

        snapshot = run_pipeline(
            host_root=args.host_root,
            run_inspectors=run_inspectors,
            run_renderers=_run_renderers,
            from_snapshot_path=args.from_snapshot,
            inspect_only=args.inspect_only,
            output_file=args.output_file,
            output_dir=args.output_dir,
            no_entitlement=args.no_entitlement,
        )
        # --validate and --push-to-github require --output-dir (enforced by CLI)
        if args.output_dir and not args.inspect_only:
            if args.validate:
                from .validate import run_validate
                run_validate(args.output_dir)
            if args.push_to_github:
                from .git_github import init_git_repo, add_and_commit, push_to_github, output_stats
                if not init_git_repo(args.output_dir):
                    print(
                        "Error: failed to initialise git repository in output directory. "
                        "GitPython may not be installed — try: pip install 'yoinkc[github]'",
                        file=sys.stderr,
                    )
                    return 1
                if not add_and_commit(args.output_dir):
                    print(
                        "Error: failed to commit output files to git repository.",
                        file=sys.stderr,
                    )
                    return 1
                size, file_count, fixme_count = output_stats(args.output_dir)
                err = push_to_github(
                    args.output_dir,
                    args.push_to_github,
                    create_private=not args.public,
                    skip_confirmation=args.yes,
                    total_size_bytes=size,
                    file_count=file_count,
                    fixme_count=fixme_count,
                    redaction_count=len(snapshot.redactions),
                    github_token=args.github_token,
                )
                if err:
                    print(f"GitHub push failed: {err}", file=sys.stderr)
                    return 1
        return 0
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        if os.environ.get("YOINKC_DEBUG"):
            traceback.print_exc()
        else:
            print("Set YOINKC_DEBUG=1 for the full traceback.", file=sys.stderr)
        return 1
```

- [ ] **Step 6: Update test_cli.py main() tests for new interface**

In `tests/test_cli.py`, update `test_main_git_init_failure_returns_error`
to include `--skip-preflight` (already present) and ensure `--output-dir`
is used (already is). No change needed if the test already uses
`--output-dir`.

Also update `test_main_exception_prints_hint` and
`test_main_exception_prints_traceback_in_debug_mode` if they break
due to the new pipeline signature. These tests mock `run_pipeline`,
so they should still work as long as the mock accepts the new kwargs.

- [ ] **Step 7: Run full test suite**

Run: `cd yoinkc && python -m pytest --tb=short -v`
Expected: All CLI and pipeline tests PASS. Integration tests may still
fail — that's addressed in the next task.

- [ ] **Step 8: Commit**

```bash
cd yoinkc
git add src/yoinkc/pipeline.py src/yoinkc/__main__.py tests/test_pipeline.py tests/test_cli.py
git commit -m "feat: refactor pipeline for tarball-first output

Pipeline now renders to a temp directory, optionally bundles entitlement
certs, then either creates a tarball (default) or copies to --output-dir.

--inspect-only saves snapshot to CWD as plain JSON.
--validate and --push-to-github are gated on --output-dir.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

---

## Chunk 5: Integration Tests and run-yoinkc.sh

### Task 5: Update integration tests for tarball output

**Files:**
- Modify: `tests/test_integration.py`

- [ ] **Step 1: Add tarball round-trip integration test**

Add to `tests/test_integration.py`:

```python
def test_tarball_output_contains_all_expected_files():
    """Tarball mode produces a valid .tar.gz with all expected output files."""
    import tarfile

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        # Run full pipeline to a directory first to get rendered output
        dir_out = root / "dir_out"
        _run_full_pipeline(dir_out)

        # Now test tarball creation via packaging module
        from yoinkc.packaging import create_tarball
        tarball_path = root / "test-output.tar.gz"
        create_tarball(dir_out, tarball_path, prefix="testhost-20260312-120000")

        assert tarball_path.exists()
        with tarfile.open(tarball_path, "r:gz") as tf:
            names = tf.getnames()
            prefix = "testhost-20260312-120000"
            for expected in EXPECTED_OUTPUT_FILES:
                assert f"{prefix}/{expected}" in names, f"Missing {expected} in tarball"
            assert any(f"{prefix}/config/" in n for n in names), "Missing config/ in tarball"
            assert f"{prefix}/{SNAPSHOT_FILENAME}" in names, "Missing snapshot in tarball"
```

- [ ] **Step 2: Run integration tests**

Run: `cd yoinkc && python -m pytest tests/test_integration.py -v`
Expected: All tests PASS

- [ ] **Step 3: Commit**

```bash
cd yoinkc
git add tests/test_integration.py
git commit -m "test: add tarball round-trip integration test

Verify that tarball output contains all expected files from a full
pipeline run.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

### Task 6: Slim down run-yoinkc.sh

**Files:**
- Modify: `run-yoinkc.sh`

- [ ] **Step 1: Rewrite run-yoinkc.sh**

Replace `run-yoinkc.sh` with the slimmed-down version. Keep the
podman install logic, registry login checks, and the podman run
invocation. Remove everything after the `podman run` line (cert
bundling, tarball creation). Remove `tar` from the install list.
Remove `--no-entitlement` stripping. Remove `OUTPUT_DIR` handling.

The first positional argument is no longer consumed as output dir —
all arguments pass through to yoinkc.

```sh
#!/bin/sh
set -eu

IMAGE="${YOINKC_IMAGE:-ghcr.io/marrusl/yoinkc:latest}"

_need_install=""
if ! command -v podman >/dev/null 2>&1; then
    _need_install="podman"
fi
if [ -n "$_need_install" ]; then
    echo "Installing missing tools:${_need_install}" >&2
    if command -v dnf >/dev/null 2>&1; then
        dnf install -y $_need_install
    elif command -v yum >/dev/null 2>&1; then
        yum install -y $_need_install
    else
        echo "ERROR: missing${_need_install} and no supported package manager found." >&2
        exit 1
    fi
fi

# Track whether podman was just installed for the registry login check.
_podman_just_installed=false
case " ${_need_install} " in
  *" podman "*) _podman_just_installed=true ;;
esac

# Expose just-installed tools to yoinkc so it can exclude them from
# the RPM output (they're tool prerequisites, not operator additions).
if [ -n "$_need_install" ]; then
  YOINKC_EXCLUDE_PREREQS="${_need_install# }"
  export YOINKC_EXCLUDE_PREREQS
fi

echo "Image: $IMAGE"

# Registry login checks for registry.redhat.io
_check_rh_login() {
  if ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
    echo "" >&2
    echo "ERROR: You are not logged in to registry.redhat.io." >&2
    echo "" >&2
    echo "  Run:  sudo podman login registry.redhat.io" >&2
    echo "" >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    echo "  Free developer account: https://developers.redhat.com" >&2
    echo "" >&2
    exit 1
  fi
}

_prompt_rh_login_fresh() {
  if [ -t 0 ]; then
    printf '\nyoinkc needs access to registry.redhat.io for the RHEL base image.\nLet'\''s log in now:\n\n' >&2
    if podman login registry.redhat.io; then
      return 0
    fi
    echo "" >&2
    echo "ERROR: podman login failed." >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    exit 1
  else
    echo "" >&2
    echo "ERROR: podman was just installed and has no credentials for registry.redhat.io." >&2
    echo "" >&2
    echo "  Run:  sudo podman login registry.redhat.io" >&2
    echo "  Then re-run this script." >&2
    echo "" >&2
    echo "  Use your Red Hat account (https://access.redhat.com)." >&2
    echo "  Free developer account: https://developers.redhat.com" >&2
    echo "" >&2
    exit 1
  fi
}

case "$IMAGE" in
  registry.redhat.io/*)
    if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
      _prompt_rh_login_fresh
    else
      _check_rh_login
    fi
    ;;
  *)
    if [ -f /etc/redhat-release ] && grep -qi "red hat" /etc/redhat-release 2>/dev/null; then
      if $_podman_just_installed && ! podman login --get-login registry.redhat.io >/dev/null 2>&1; then
        _prompt_rh_login_fresh
      else
        _check_rh_login
      fi
    fi
    ;;
esac

echo "=== Running yoinkc ==="
podman run --rm --pull=always \
  --pid=host \
  --privileged \
  --security-opt label=disable \
  -w /output \
  ${YOINKC_DEBUG:+-e YOINKC_DEBUG=1} \
  ${YOINKC_EXCLUDE_PREREQS:+--env YOINKC_EXCLUDE_PREREQS} \
  -v /:/host:ro \
  -v "$(pwd):/output" \
  "$IMAGE" "$@"
echo "=== Done ==="
```

- [ ] **Step 2: Verify the script is syntactically valid**

Run: `cd yoinkc && sh -n run-yoinkc.sh`
Expected: No output (no syntax errors)

- [ ] **Step 3: Commit**

```bash
cd yoinkc
git add run-yoinkc.sh
git commit -m "refactor: slim run-yoinkc.sh to podman setup and launch

Remove tarball creation, entitlement cert bundling, output directory
handling, tar installation, and --no-entitlement flag stripping. These
are now handled by yoinkc itself.

All user arguments pass through directly. The user's CWD is mounted at
/output and -w /output sets the container working directory so the
tarball lands in the user's directory.

Drop :z volume flag — --security-opt label=disable makes it unnecessary
and relabeling the user's CWD risks corrupting host SELinux contexts.

Part of tarball-first output refactor.
See docs/specs/2026-03-12-tarball-first-output-design.md

Assisted-by: Claude Code (Opus 4.6)"
```

### Task 7: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cd yoinkc && python -m pytest -v`
Expected: All tests PASS

- [ ] **Step 2: Verify no regressions in existing functionality**

Run: `cd yoinkc && python -m pytest tests/test_renderer_outputs.py tests/test_inspectors.py tests/test_redact.py -v`
Expected: All PASS — these tests don't touch CLI/pipeline

- [ ] **Step 3: Commit if any fixups were needed**

Only commit if step 1 or 2 revealed issues that needed fixing.

```bash
cd yoinkc
git add -u
git commit -m "fix: address test failures from tarball-first refactor

Assisted-by: Claude Code (Opus 4.6)"
```
