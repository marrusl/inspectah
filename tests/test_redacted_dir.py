# tests/test_redacted_dir.py
import tempfile
from pathlib import Path
from inspectah.schema import (
    InspectionSnapshot, ConfigSection, ConfigFileEntry, ConfigFileKind,
    RedactionFinding,
)
from inspectah.renderers.containerfile._config_tree import write_config_tree, write_redacted_dir


def _snapshot_with_excluded():
    snap = InspectionSnapshot(meta={})
    snap.config = ConfigSection(files=[
        ConfigFileEntry(path="/etc/cockpit/ws-certs.d/0-self-signed.key", kind=ConfigFileKind.UNOWNED, content="placeholder", include=False),
        ConfigFileEntry(path="/etc/pki/tls/private/server.key", kind=ConfigFileKind.UNOWNED, content="placeholder", include=False),
        ConfigFileEntry(path="/etc/app.conf", kind=ConfigFileKind.UNOWNED, content="normal config", include=True),
    ])
    snap.redactions = [
        RedactionFinding(path="/etc/cockpit/ws-certs.d/0-self-signed.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="regenerate"),
        RedactionFinding(path="/etc/pki/tls/private/server.key", source="file",
                        kind="excluded", pattern="EXCLUDED_PATH", remediation="provision"),
    ]
    return snap


def test_redacted_files_in_redacted_dir():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        key_file = out / "redacted" / "etc" / "cockpit" / "ws-certs.d" / "0-self-signed.key.REDACTED"
        assert key_file.exists()
        tls_file = out / "redacted" / "etc" / "pki" / "tls" / "private" / "server.key.REDACTED"
        assert tls_file.exists()


def test_redacted_files_not_in_config_dir():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_config_tree(snap, out)
        # Excluded files should NOT appear in config/
        cockpit_dir = out / "config" / "etc" / "cockpit"
        assert not cockpit_dir.exists()
        pki_key = out / "config" / "etc" / "pki" / "tls" / "private" / "server.key"
        assert not pki_key.exists()
        # Included file should appear
        assert (out / "config" / "etc" / "app.conf").exists()


def test_regenerate_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "cockpit" / "ws-certs.d" / "0-self-signed.key.REDACTED").read_text()
        assert "REDACTED by inspectah" in content
        assert "auto-generated credential" in content
        assert "no action needed" in content
        assert "/etc/cockpit/ws-certs.d/0-self-signed.key" in content


def test_provision_placeholder_content():
    snap = _snapshot_with_excluded()
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        content = (out / "redacted" / "etc" / "pki" / "tls" / "private" / "server.key.REDACTED").read_text()
        assert "REDACTED by inspectah" in content
        assert "sensitive file detected" in content
        assert "provision" in content
        assert "/etc/pki/tls/private/server.key" in content


def test_non_file_findings_no_redacted_file():
    snap = InspectionSnapshot(meta={})
    snap.config = ConfigSection(files=[])
    snap.redactions = [
        RedactionFinding(path="users:shadow/testuser", source="shadow",
                        kind="inline", pattern="SHADOW_HASH", remediation="value-removed"),
    ]
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        write_redacted_dir(snap, out)
        redacted = out / "redacted"
        if redacted.exists():
            assert not any(redacted.rglob("*"))
