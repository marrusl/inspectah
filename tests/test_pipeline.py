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
    snapshot = _make_snapshot()
    snap_path = tmp_path / "snap.json"
    save_snapshot(snapshot, snap_path)

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
    assert not tarball_path.exists()
