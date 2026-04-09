# tests/test_containerfile_secrets_comments.py
import tempfile
from pathlib import Path
from yoinkc.schema import (
    InspectionSnapshot, OsRelease, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)
from yoinkc.renderers.containerfile._core import _render_containerfile_content


def _snapshot_with_secrets():
    """Build a snapshot with both excluded and inline-redacted findings."""
    snap = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = [
        RedactionFinding(
            path="/etc/cockpit/ws-certs.d/0-self-signed.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="regenerate",
        ),
        RedactionFinding(
            path="/etc/pki/tls/private/server.key",
            source="file", kind="excluded", pattern="EXCLUDED_PATH",
            remediation="provision",
        ),
        RedactionFinding(
            path="/etc/wireguard/wg0.conf",
            source="file", kind="inline", pattern="WIREGUARD_KEY",
            remediation="value-removed", replacement="REDACTED_WIREGUARD_KEY_1",
        ),
        RedactionFinding(
            path="users:shadow/testuser",
            source="shadow", kind="inline", pattern="SHADOW_HASH",
            remediation="value-removed", replacement="REDACTED_SHADOW_HASH_1",
        ),
    ]
    return snap


def test_containerfile_has_excluded_comment_block():
    """Containerfile should list excluded secrets grouped by remediation."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Excluded secrets (not in this image)" in content
    assert "Regenerate on target" in content
    assert "cockpit" in content
    assert "Provision from secret store" in content
    assert "server.key" in content


def test_containerfile_has_inline_comment_block():
    """Containerfile should list inline-redacted values (file-backed only)."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Inline-redacted values" in content
    assert "wireguard" in content or "wg0.conf" in content
    assert "REDACTED_WIREGUARD_KEY_1" in content
    # Shadow (non-file-backed) should NOT appear in Containerfile comments
    assert "REDACTED_SHADOW_HASH_1" not in content


def test_containerfile_no_comments_when_no_redactions():
    """No comment blocks if no redactions."""
    snap = InspectionSnapshot(
        meta={"host_root": "/host"},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = []
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "Excluded secrets" not in content
    assert "Inline-redacted" not in content


def test_containerfile_flagged_note_present():
    """Flagged-note appears with correct count when flagged findings exist."""
    snap = _snapshot_with_secrets()
    snap.redactions.append(
        RedactionFinding(
            path="/etc/myapp/config.ini", source="file", kind="flagged",
            pattern="signing_key", remediation="",
            detection_method="heuristic", confidence="low", line=5,
        ),
    )
    snap.redactions.append(
        RedactionFinding(
            path="/etc/myapp/other.ini", source="file", kind="flagged",
            pattern="db_password", remediation="",
            detection_method="heuristic", confidence="high", line=12,
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "# Note: 2 additional values were flagged for review but not redacted." in content
    assert "# See secrets-review.md for details." in content


def test_containerfile_no_flagged_note_when_no_flagged():
    """No flagged-note when there are no flagged findings."""
    snap = _snapshot_with_secrets()
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "flagged for review" not in content


def test_containerfile_heuristic_inline_in_inline_block():
    """Heuristic inline findings appear in the inline block alongside pattern ones."""
    snap = _snapshot_with_secrets()
    snap.redactions.append(
        RedactionFinding(
            path="/etc/myapp/config.ini", source="file", kind="inline",
            pattern="signing_key", remediation="value-removed",
            replacement="REDACTED_SIGNING_KEY_1",
            detection_method="heuristic", confidence="high",
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        content = _render_containerfile_content(snap, Path(tmp))
    assert "REDACTED_SIGNING_KEY_1" in content
    assert "Inline-redacted values" in content
    # Both the pattern-based and heuristic inline should be in the same block
    assert "REDACTED_WIREGUARD_KEY_1" in content
    assert "REDACTED_SIGNING_KEY_1" in content
