"""
End-to-end integration tests: fixtures → inspectors → serialize → deserialize → renderers.

Test 1: Full pipeline with all fixtures; verify every output file is written and non-empty.
Test 2: Load snapshot via --from-snapshot path and run only renderers; verify identical output.
"""

import tempfile
from pathlib import Path

from yoinkc.executor import Executor, RunResult
from yoinkc.inspectors import run_all as run_all_inspectors
from yoinkc.pipeline import load_snapshot, save_snapshot
from yoinkc.redact import redact_snapshot
from yoinkc.renderers import run_all as run_all_renderers

FIXTURES = Path(__file__).parent / "fixtures"

EXPECTED_OUTPUT_FILES = [
    "Containerfile",
    "audit-report.md",
    "report.html",
    "README.md",
    "secrets-review.md",
    "kickstart-suggestion.ks",
]
EXPECTED_OUTPUT_DIRS = ["config"]
SNAPSHOT_FILENAME = "inspection-snapshot.json"


def _fixture_executor(cmd, cwd=None):
    """Executor that returns fixture file content for known commands."""
    cmd_str = " ".join(cmd)
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    if "podman" in cmd and "login" in cmd and "--get-login" in cmd:
        return RunResult(stdout="testuser\n", stderr="", returncode=0)
    if "podman" in cmd and "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "base_image_packages.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-Va" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "list" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "info" in cmd and "4" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-ql" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
    if "systemctl" in cmd and "list-unit-files" in cmd:
        return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
    return RunResult(stdout="", stderr="unknown command", returncode=1)


def _run_full_pipeline(output_dir: Path) -> Path:
    """Run all inspectors (with fixtures), redact, save snapshot, run renderers."""
    host_root = FIXTURES / "host_etc"
    executor: Executor = _fixture_executor
    snapshot = run_all_inspectors(
        host_root,
        executor=executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    snapshot = redact_snapshot(snapshot)
    output_dir.mkdir(parents=True, exist_ok=True)
    snapshot_path = output_dir / SNAPSHOT_FILENAME
    save_snapshot(snapshot, snapshot_path)
    run_all_renderers(snapshot, output_dir)
    return snapshot_path


def _verify_all_output_files_written_and_non_empty(output_dir: Path) -> None:
    for name in EXPECTED_OUTPUT_FILES:
        path = output_dir / name
        assert path.exists(), f"Expected output file missing: {name}"
        content = path.read_text()
        assert len(content.strip()) > 0, f"Expected output file non-empty: {name}"
    for name in EXPECTED_OUTPUT_DIRS:
        path = output_dir / name
        assert path.is_dir(), f"Expected output dir missing: {name}"


def _collect_output_file_paths(output_dir: Path):
    for name in EXPECTED_OUTPUT_FILES:
        yield name, True
    for name in EXPECTED_OUTPUT_DIRS:
        d = output_dir / name
        if d.is_dir():
            for p in sorted(d.rglob("*")):
                if p.is_file():
                    yield p.relative_to(output_dir).as_posix(), True


def test_full_pipeline_fixtures_end_to_end():
    """Full pipeline: fixtures → inspectors → serialize → deserialize → renderers."""
    host_root = FIXTURES / "host_etc"
    executor: Executor = _fixture_executor

    snapshot = run_all_inspectors(
        host_root,
        executor=executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    snapshot = redact_snapshot(snapshot)

    with tempfile.TemporaryDirectory() as tmp:
        output_dir = Path(tmp)
        snapshot_path = output_dir / SNAPSHOT_FILENAME
        save_snapshot(snapshot, snapshot_path)
        assert snapshot_path.exists()
        assert snapshot_path.stat().st_size > 0

        loaded = load_snapshot(snapshot_path)
        run_all_renderers(loaded, output_dir)
        _verify_all_output_files_written_and_non_empty(output_dir)


def test_from_snapshot_produces_identical_output():
    """--from-snapshot produces identical output to a full pipeline run."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        dir_first = root / "first"
        dir_second = root / "second"

        snapshot_path = _run_full_pipeline(dir_first)
        _verify_all_output_files_written_and_non_empty(dir_first)

        loaded = load_snapshot(snapshot_path)
        loaded = redact_snapshot(loaded)
        dir_second.mkdir(parents=True, exist_ok=True)
        run_all_renderers(loaded, dir_second)

        for rel_path, _ in _collect_output_file_paths(dir_first):
            p1 = dir_first / rel_path
            p2 = dir_second / rel_path
            assert p1.is_file(), f"First run missing file: {rel_path}"
            assert p2.exists(), f"Second run (from-snapshot) missing: {rel_path}"
            assert p2.is_file(), f"Second run path not file: {rel_path}"
            c1 = p1.read_text()
            c2 = p2.read_text()
            assert c1 == c2, f"Output differs for {rel_path}"

        _verify_all_output_files_written_and_non_empty(dir_second)
