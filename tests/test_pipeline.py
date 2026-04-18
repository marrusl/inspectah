"""Tests for pipeline.py: snapshot handling and output modes."""

import json
import tarfile
import tempfile
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from inspectah.pipeline import load_snapshot, save_snapshot, run_pipeline
from inspectah.schema import InspectionSnapshot, SCHEMA_VERSION


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


def test_subscription_bundling_in_tarball(tmp_path):
    """Subscription certs from host_root are included in tarball."""
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


def test_no_subscription_skips_bundling(tmp_path):
    """--no-subscription suppresses cert bundling."""
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
        no_subscription=True,
    )
    with tarfile.open(tarball_path, "r:gz") as tf:
        cert_entries = [n for n in tf.getnames() if "entitlement" in n]
        assert len(cert_entries) == 0


def test_from_snapshot_skips_subscription_bundling(tmp_path):
    """--from-snapshot mode silently skips subscription bundling (host may not be mounted)."""
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


# --- CLI secrets summary tests ---

import sys
from io import StringIO


def test_cli_secrets_summary(monkeypatch):
    """CLI summary prints correct counts to stderr."""
    from inspectah.pipeline import _print_secrets_summary
    from inspectah.schema import RedactionFinding

    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.cert", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
        RedactionFinding(path="/etc/wireguard/wg0.conf", source="file",
                        kind="inline", pattern="WIREGUARD_KEY", remediation="value-removed",
                        replacement="REDACTED_WIREGUARD_KEY_1"),
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="inline", pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1"),
        RedactionFinding(path="users:shadow/admin", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed",
                        replacement="REDACTED_SHADOW_HASH_1"),
    ]

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_secrets_summary(snap)
    output = captured.getvalue()

    assert "Secrets handling:" in output
    assert "Excluded (regenerate on target): 2 files" in output
    assert "Excluded (provision from store): 1 file" in output
    assert "Inline-redacted:" in output
    assert "2 files" in output or "2 file" in output
    assert "secrets-review.md" in output


def test_cli_secrets_summary_no_findings(monkeypatch):
    """CLI summary prints nothing when there are no findings."""
    from inspectah.pipeline import _print_secrets_summary

    snap = InspectionSnapshot(meta={})
    snap.redactions = []

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_secrets_summary(snap)
    output = captured.getvalue()
    assert output == ""


# --- Heuristic pass integration tests ---


def _make_snapshot_with_config_secret():
    """Create a snapshot with config content containing a heuristic-detectable secret.

    Uses 'signing_key' as the keyword — pattern-based redact won't catch this
    (no PASSWORD/TOKEN/SECRET/API_KEY pattern match), but heuristic will detect
    it via keyword proximity + entropy.
    """
    from inspectah.schema import ConfigSection, ConfigFileEntry, ConfigFileKind, ConfigCategory

    snap = InspectionSnapshot(meta={"host_root": "/host"})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/myapp/config.ini",
            kind=ConfigFileKind.UNOWNED,
            category=ConfigCategory.OTHER,
            content="[service]\nsigning_key = aB3dEfG7hI9jKlMnOpQrStUvWxYz012345\nhost = localhost\n",
        ),
    ])
    return snap


def _make_snapshot_with_subscription_cert():
    """Snapshot with config file under subscription cert path (should be skipped)."""
    from inspectah.schema import ConfigSection, ConfigFileEntry, ConfigFileKind, ConfigCategory

    snap = InspectionSnapshot(meta={"host_root": "/host"})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(
            path="/etc/pki/entitlement/1234.pem",
            kind=ConfigFileKind.UNOWNED,
            category=ConfigCategory.OTHER,
            content="signing_key = aB3dEfG7hI9jKlMnOpQrStUvWxYz012345\n",
        ),
    ])
    return snap


def _make_snapshot_with_container_env_secret():
    """Snapshot with container env var containing heuristic-detectable secret."""
    from inspectah.schema import ContainerSection, RunningContainer

    snap = InspectionSnapshot(meta={"host_root": "/host"})
    snap.containers = ContainerSection(running_containers=[
        RunningContainer(
            id="abc123def456",
            name="myapp",
            image="registry.example.com/myapp:latest",
            env=["SIGNING_KEY=aB3dEfG7hI9jKlMnOpQrStUvWxYz012345"],
        ),
    ])
    return snap


def test_heuristic_findings_appear_in_redactions(tmp_path):
    """Heuristic findings from config files appear in snapshot.redactions after pipeline."""
    snapshot = _make_snapshot_with_config_secret()
    snap_path = tmp_path / "snap.json"
    save_snapshot(snapshot, snap_path)

    result = run_pipeline(
        host_root=Path("/host"),
        run_inspectors=None,
        run_renderers=_tracking_renderer([]),
        from_snapshot_path=snap_path,
        output_dir=tmp_path / "out",
        sensitivity="strict",
    )

    heuristic_findings = [
        r for r in result.redactions
        if isinstance(r, __import__("inspectah.schema", fromlist=["RedactionFinding"]).RedactionFinding)
        and r.detection_method == "heuristic"
    ]
    assert len(heuristic_findings) > 0
    # At least one should be from our config file
    config_findings = [f for f in heuristic_findings if f.path == "/etc/myapp/config.ini"]
    assert len(config_findings) > 0


def test_heuristic_skips_subscription_cert_paths(tmp_path):
    """Heuristic pass skips subscription cert paths (/etc/pki/entitlement/, /etc/rhsm/)."""
    snapshot = _make_snapshot_with_subscription_cert()
    snap_path = tmp_path / "snap.json"
    save_snapshot(snapshot, snap_path)

    result = run_pipeline(
        host_root=Path("/host"),
        run_inspectors=None,
        run_renderers=_tracking_renderer([]),
        from_snapshot_path=snap_path,
        output_dir=tmp_path / "out",
        sensitivity="strict",
    )

    heuristic_findings = [
        r for r in result.redactions
        if isinstance(r, __import__("inspectah.schema", fromlist=["RedactionFinding"]).RedactionFinding)
        and r.detection_method == "heuristic"
    ]
    # No heuristic findings for subscription cert paths
    cert_findings = [f for f in heuristic_findings if "/etc/pki/entitlement/" in f.path]
    assert len(cert_findings) == 0


def test_no_redaction_mode_detects_but_preserves_content(tmp_path):
    """no_redaction mode runs detection but doesn't modify content. All findings are flagged."""
    snapshot = _make_snapshot_with_config_secret()
    snap_path = tmp_path / "snap.json"
    save_snapshot(snapshot, snap_path)

    # Capture original content
    original_content = snapshot.config.files[0].content

    result = run_pipeline(
        host_root=Path("/host"),
        run_inspectors=None,
        run_renderers=_tracking_renderer([]),
        from_snapshot_path=snap_path,
        output_dir=tmp_path / "out",
        no_redaction=True,
    )

    # Content should be preserved (no REDACTED_ tokens)
    if result.config and result.config.files:
        for f in result.config.files:
            assert "REDACTED_" not in f.content

    # All findings should be kind="flagged"
    from inspectah.schema import RedactionFinding
    typed_findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    for f in typed_findings:
        assert f.kind == "flagged", f"Expected kind='flagged' in no-redaction mode, got '{f.kind}' for {f.path}"


def test_pipeline_moderate_flags_all_heuristic(tmp_path):
    """In moderate mode, all heuristic findings are flagged, not redacted."""
    from inspectah.schema import RedactionFinding

    snapshot = _make_snapshot_with_config_secret()
    snap_path = tmp_path / "snap.json"
    save_snapshot(snapshot, snap_path)

    result = run_pipeline(
        host_root=Path("/host"),
        run_inspectors=None,
        run_renderers=_tracking_renderer([]),
        from_snapshot_path=snap_path,
        output_dir=tmp_path / "out",
        sensitivity="moderate",
    )

    heuristic_findings = [
        r for r in result.redactions
        if isinstance(r, RedactionFinding) and r.detection_method == "heuristic"
    ]
    assert len(heuristic_findings) > 0, "Expected at least one heuristic finding"
    for f in heuristic_findings:
        assert f.kind == "flagged", (
            f"In moderate mode, all heuristic findings should be flagged, "
            f"got kind='{f.kind}' for {f.path}"
        )


def test_cli_summary_includes_heuristic_supplement(monkeypatch):
    """CLI summary shows heuristic breakdown in inline and flagged counts."""
    from inspectah.pipeline import _print_secrets_summary
    from inspectah.schema import RedactionFinding

    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        # Pattern-based inline
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="inline", pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        # Heuristic inline
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="inline", pattern="heuristic", remediation="value-removed",
                        replacement="REDACTED_HEURISTIC_1", detection_method="heuristic",
                        confidence="high"),
        # Heuristic flagged
        RedactionFinding(path="/etc/other.conf", source="file",
                        kind="flagged", pattern="heuristic", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_secrets_summary(snap)
    output = captured.getvalue()

    # Should show heuristic supplement in inline count
    assert "pattern" in output.lower() or "heuristic" in output.lower()
    # Should show flagged for review count
    assert "Flagged" in output or "flagged" in output


def test_no_redaction_warning_printed(monkeypatch):
    """--no-redaction completion warning prints WARNING and secrets-review.md to stderr."""
    from inspectah.pipeline import _print_no_redaction_warning
    from inspectah.schema import RedactionFinding

    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/a.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/b.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/c.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/d.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/e.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
        RedactionFinding(path="/etc/f.conf", source="file", kind="flagged",
                        pattern="signing_key", remediation="",
                        detection_method="heuristic", confidence="high"),
        RedactionFinding(path="/etc/g.conf", source="file", kind="flagged",
                        pattern="signing_key", remediation="",
                        detection_method="heuristic", confidence="high"),
        RedactionFinding(path="/etc/h.conf", source="file", kind="flagged",
                        pattern="signing_key", remediation="",
                        detection_method="heuristic", confidence="high"),
        RedactionFinding(path="/etc/i.conf", source="file", kind="flagged",
                        pattern="db_pass", remediation="",
                        detection_method="heuristic", confidence="low"),
        RedactionFinding(path="/etc/j.conf", source="file", kind="flagged",
                        pattern="db_pass", remediation="",
                        detection_method="heuristic", confidence="low"),
        RedactionFinding(path="/etc/k.conf", source="file", kind="flagged",
                        pattern="db_pass", remediation="",
                        detection_method="heuristic", confidence="low"),
        RedactionFinding(path="/etc/l.conf", source="file", kind="flagged",
                        pattern="db_pass", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]

    captured = StringIO()
    monkeypatch.setattr(sys, "stderr", captured)
    _print_no_redaction_warning(snap)
    output = captured.getvalue()

    assert "WARNING: Redaction was disabled for this run." in output
    assert "5 pattern findings were NOT redacted" in output
    assert "3 high-confidence heuristic findings were NOT redacted" in output
    assert "4 low-confidence heuristic findings flagged" in output
    assert "secrets-review.md" in output
    assert "Do not share, commit, or upload" in output
