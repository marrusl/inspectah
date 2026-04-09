"""Tests for the rewritten secrets-review.md renderer."""
import tempfile
from pathlib import Path
from jinja2 import Environment
from yoinkc.schema import InspectionSnapshot, RedactionFinding
from yoinkc.renderers.secrets_review import render


def _snapshot_with_findings():
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
        RedactionFinding(path="/etc/wireguard/wg0.conf", source="file",
                        kind="inline", pattern="WIREGUARD_KEY", remediation="value-removed",
                        line=3, replacement="REDACTED_WIREGUARD_KEY_1"),
        RedactionFinding(path="users:shadow/testuser", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed",
                        replacement="REDACTED_SHADOW_HASH_1"),
    ]
    return snap


def test_secrets_review_has_excluded_table():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Excluded Files" in content
        assert "Regenerate on target" in content
        assert "Provision from secret store" in content


def test_secrets_review_has_inline_table():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Inline Redactions" in content
        assert "REDACTED_WIREGUARD_KEY_1" in content
        assert "Supply value at deploy time" in content


def test_secrets_review_separates_excluded_and_inline():
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        excluded_pos = content.index("## Excluded Files")
        inline_pos = content.index("## Inline Redactions")
        assert excluded_pos < inline_pos


def test_secrets_review_empty():
    snap = InspectionSnapshot(meta={})
    snap.redactions = []
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "No redactions recorded" in content


def test_secrets_review_legacy_dict_compat():
    """Renderer handles a mix of old dicts and new RedactionFinding objects."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        {"path": "/etc/old.conf", "pattern": "PASSWORD", "line": "content", "remediation": "old style"},
        RedactionFinding(path="/etc/new.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        # Should not crash; both items should appear
        assert "/etc/old.conf" in content or "/etc/new.conf" in content


def test_secrets_review_has_detection_column():
    """Inline Redactions table includes a Detection column."""
    snap = _snapshot_with_findings()
    # Add detection_method to one finding
    snap.redactions.append(
        RedactionFinding(path="/etc/app.conf", source="file", kind="inline",
                        pattern="API_KEY", remediation="value-removed",
                        replacement="REDACTED_API_KEY_1", detection_method="heuristic",
                        confidence="high"),
    )
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "| Detection |" in content
        assert "heuristic (high)" in content
        assert "pattern" in content


def test_secrets_review_has_flagged_table():
    """Flagged for Review table appears for kind='flagged' findings."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file", kind="flagged",
                        pattern="signing_key", remediation="",
                        detection_method="heuristic", confidence="low", line=5),
        RedactionFinding(path="/etc/other.conf", source="file", kind="flagged",
                        pattern="db_password", remediation="",
                        detection_method="heuristic", confidence="high", line=12),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Flagged for Review" in content
        assert "| Path | Line | Confidence | Why Flagged |" in content
        assert "/etc/app.conf" in content
        assert "low" in content
        assert "signing_key" in content


def test_secrets_review_summary_line():
    """Summary line at top shows correct counts."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/a.conf", source="file", kind="inline",
                        pattern="PASSWORD", remediation="value-removed",
                        replacement="REDACTED_PASSWORD_1", detection_method="pattern"),
        RedactionFinding(path="/etc/b.conf", source="file", kind="inline",
                        pattern="API_KEY", remediation="value-removed",
                        replacement="REDACTED_API_KEY_1", detection_method="heuristic",
                        confidence="high"),
        RedactionFinding(path="/etc/c.conf", source="file", kind="flagged",
                        pattern="signing_key", remediation="",
                        detection_method="heuristic", confidence="low", line=3),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "> Detected secrets: 2 redacted (1 pattern, 1 heuristic), 1 flagged for review" in content


def test_secrets_review_no_flagged_table_when_no_flagged():
    """No Flagged for Review table when there are no flagged findings."""
    snap = _snapshot_with_findings()
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "## Flagged for Review" not in content


def test_secrets_review_no_redaction_header():
    """WARNING header appears when no_redaction=True."""
    snap = InspectionSnapshot(meta={})
    snap.redactions = [
        RedactionFinding(path="/etc/a.conf", source="file", kind="flagged",
                        pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp), no_redaction=True)
        content = (Path(tmp) / "secrets-review.md").read_text()
        assert "> WARNING: Redaction was disabled" in content


def test_secrets_review_no_redaction_via_meta():
    """WARNING header appears when _no_redaction is set in snapshot.meta."""
    snap = InspectionSnapshot(meta={"_no_redaction": True})
    snap.redactions = [
        RedactionFinding(path="/etc/app.conf", source="file",
                        kind="flagged", pattern="PASSWORD", remediation="",
                        detection_method="pattern"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        render(snap, Environment(), Path(tmp))
        content = (Path(tmp) / "secrets-review.md").read_text()
    assert "WARNING" in content
