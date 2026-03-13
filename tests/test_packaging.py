"""Tests for tarball packaging and hostname/stamp helpers."""

import tarfile
import tempfile
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

import pytest

from yoinkc.packaging import (
    _resolve_hostname,
    create_tarball,
    get_output_stamp,
    sanitize_hostname,
)


def test_sanitize_hostname_simple():
    assert sanitize_hostname("webserver01") == "webserver01"


def test_sanitize_hostname_strips_unsafe_chars():
    assert sanitize_hostname("web/server:01") == "webserver01"


def test_sanitize_hostname_empty_fallback():
    assert sanitize_hostname("") == "unknown"


def test_sanitize_hostname_all_unsafe_fallback():
    assert sanitize_hostname("///") == "unknown"


def test_resolve_hostname_reads_host_root_etc_hostname():
    """host_root/etc/hostname takes priority over socket.gethostname()."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        (host_root / "etc").mkdir()
        (host_root / "etc" / "hostname").write_text("my-real-host\n")
        with patch("socket.gethostname", return_value="container-deadbeef"):
            assert _resolve_hostname(host_root) == "my-real-host"


def test_resolve_hostname_falls_back_to_socket_when_etc_hostname_missing():
    """Fall back to socket.gethostname() if host_root/etc/hostname is absent."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        # No etc/hostname created — directory exists but file does not
        (host_root / "etc").mkdir()
        with patch("socket.gethostname", return_value="socket-host"):
            assert _resolve_hostname(host_root) == "socket-host"


def test_resolve_hostname_falls_back_to_socket_when_no_host_root():
    """Without host_root, socket.gethostname() is used directly."""
    with patch("socket.gethostname", return_value="socket-host"):
        assert _resolve_hostname() == "socket-host"


def test_resolve_hostname_unknown_when_all_fail():
    """Return 'unknown' when both host_root file and socket fail."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        (host_root / "etc").mkdir()
        # etc/hostname is empty
        (host_root / "etc" / "hostname").write_text("")
        with patch("socket.gethostname", side_effect=OSError):
            assert _resolve_hostname(host_root) == "unknown"


def test_resolve_hostname_strips_newline_from_etc_hostname():
    """Only the first line of /etc/hostname is used, stripped of whitespace."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        (host_root / "etc").mkdir()
        (host_root / "etc" / "hostname").write_text("  myhost  \nextra-line\n")
        assert _resolve_hostname(host_root) == "myhost"


def test_get_output_stamp_uses_host_root():
    """get_output_stamp passes host_root through to hostname resolution."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        (host_root / "etc").mkdir()
        (host_root / "etc" / "hostname").write_text("prod-server")
        stamp = get_output_stamp(host_root=host_root)
        assert stamp.startswith("prod-server-")


def test_get_output_stamp_explicit_hostname_overrides_resolution():
    """Explicit hostname bypasses /etc/hostname and socket resolution."""
    with tempfile.TemporaryDirectory() as tmp:
        host_root = Path(tmp)
        (host_root / "etc").mkdir()
        (host_root / "etc" / "hostname").write_text("file-host")
        with patch("socket.gethostname", return_value="socket-host"):
            stamp = get_output_stamp(hostname="explicit-host", host_root=host_root)
        assert stamp.startswith("explicit-host-")


def test_get_output_stamp_explicit_hostname_sanitized():
    """Explicit hostname is sanitized before use."""
    stamp = get_output_stamp(hostname="my/host:name")
    assert stamp.startswith("myhostname-")


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
