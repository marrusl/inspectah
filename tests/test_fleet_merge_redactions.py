"""Test that fleet merge handles RedactionFinding objects correctly."""
from yoinkc.schema import (
    InspectionSnapshot, OsRelease, RedactionFinding,
)
from yoinkc.fleet.merge import merge_snapshots


def _snap_with_redactions(hostname, redactions):
    snap = InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.6", pretty_name="RHEL 9.6"),
    )
    snap.redactions = redactions
    return snap


def test_merge_deduplicates_typed_findings():
    """Identical RedactionFinding objects across snapshots are deduplicated."""
    finding = RedactionFinding(
        path="/etc/pki/tls/private/server.key",
        source="file", kind="excluded", pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    snap_a = _snap_with_redactions("host-a", [finding])
    snap_b = _snap_with_redactions("host-b", [finding])
    merged = merge_snapshots([snap_a, snap_b])
    # Same finding on both hosts should deduplicate to one
    assert len(merged.redactions) == 1


def test_merge_keeps_different_typed_findings():
    """Different RedactionFinding objects are preserved."""
    f1 = RedactionFinding(
        path="/etc/pki/tls/private/server.key",
        source="file", kind="excluded", pattern="EXCLUDED_PATH",
        remediation="provision",
    )
    f2 = RedactionFinding(
        path="/etc/app.conf",
        source="file", kind="inline", pattern="PASSWORD",
        remediation="value-removed", replacement="REDACTED_PASSWORD_1",
    )
    snap_a = _snap_with_redactions("host-a", [f1])
    snap_b = _snap_with_redactions("host-b", [f2])
    merged = merge_snapshots([snap_a, snap_b])
    assert len(merged.redactions) == 2
