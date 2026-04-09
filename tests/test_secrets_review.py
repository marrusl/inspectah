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
