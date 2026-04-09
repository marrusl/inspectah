"""End-to-end integration tests for heuristic secrets safety net."""
import tempfile
from pathlib import Path
from jinja2 import Environment

from yoinkc.pipeline import run_pipeline, save_snapshot
from yoinkc.schema import (
    InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)
from yoinkc.renderers.secrets_review import render as render_secrets_review
from yoinkc.renderers.containerfile._core import _secrets_comment_lines


def _full_snapshot():
    """Build a snapshot with various secret types for integration testing."""
    return InspectionSnapshot(
        meta={"hostname": "test-host"},
        config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/wireguard/wg0.conf",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="[Interface]\nPrivateKey = aB3cD4eF5gH6iJ7kL8mN9oP0qR1sT2uV3wX4yZ5A=\n"),
            ConfigFileEntry(path="/etc/shadow",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="root:$y$abc:19700:::\n"),
            ConfigFileEntry(path="/etc/myapp/config.ini",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="db_password = aR9xk!mQ2pL7bN4cKzW5tY\nsession_timeout = 3600\n"),
            ConfigFileEntry(path="/etc/pki/entitlement/12345.pem",
                kind=ConfigFileKind.UNOWNED, include=True,
                content="-----BEGIN RSA PRIVATE KEY-----\nfake\n-----END RSA PRIVATE KEY-----\n"),
        ]),
    )


def test_strict_mode_full_pipeline():
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            sensitivity="strict",
        )
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    pattern = [f for f in findings if f.detection_method == "pattern"]
    assert len(pattern) >= 1
    excluded = [f for f in findings if f.detection_method == "excluded_path"]
    assert len(excluded) >= 1
    # Subscription cert paths are excluded from the heuristic pass,
    # but the pattern pass still detects PRIVATE_KEY content.
    sub_heuristic = [f for f in findings
                     if ("entitlement" in f.path or "rhsm" in f.path)
                     and f.detection_method == "heuristic"]
    assert len(sub_heuristic) == 0


def test_moderate_mode_full_pipeline():
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            sensitivity="moderate",
        )
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    heuristic = [f for f in findings if f.detection_method == "heuristic"]
    for f in heuristic:
        assert f.kind == "flagged", f"Moderate mode: heuristic should be flagged: {f}"


def test_no_redaction_mode_full_pipeline():
    snap = _full_snapshot()
    with tempfile.TemporaryDirectory() as tmp:
        snap_path = Path(tmp) / "inspection-snapshot.json"
        save_snapshot(snap, snap_path)
        result = run_pipeline(
            host_root=Path("/nonexistent"),
            run_inspectors=None,
            run_renderers=lambda s, d: None,
            from_snapshot_path=snap_path,
            output_dir=Path(tmp) / "output",
            no_redaction=True,
        )
    wg_file = [f for f in result.config.files if "wireguard" in f.path]
    if wg_file:
        assert "PrivateKey" in wg_file[0].content
    findings = [r for r in result.redactions if isinstance(r, RedactionFinding)]
    assert len(findings) >= 1


def test_secrets_review_three_tables():
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/shadow", source="file", kind="excluded",
                        pattern="EXCLUDED_PATH", remediation="provision",
                        detection_method="excluded_path"),
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/myapp/config.ini", source="file", kind="inline",
                        pattern="db_password", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_2", detection_method="heuristic",
                        confidence="high"),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render_secrets_review(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "## Excluded Files" in content
    assert "## Inline Redactions" in content
    assert "## Flagged for Review" in content
    assert "Detection" in content


def test_containerfile_comments_full_spectrum():
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/shadow", source="file", kind="excluded",
                        pattern="EXCLUDED_PATH", remediation="provision",
                        detection_method="excluded_path"),
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="HEURISTIC", remediation="",
                        detection_method="heuristic", confidence="low"),
    ]
    lines = _secrets_comment_lines(snap)
    text = "\n".join(lines)
    assert "Excluded secrets" in text
    assert "Inline-redacted" in text
    assert "flagged for review" in text.lower()
    assert "secrets-review.md" in text
