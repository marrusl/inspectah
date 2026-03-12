"""Tests for tarball packaging and hostname/stamp helpers."""

import tarfile
import tempfile
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

import pytest

from yoinkc.packaging import create_tarball, get_output_stamp, sanitize_hostname


def test_sanitize_hostname_simple():
    assert sanitize_hostname("webserver01") == "webserver01"


def test_sanitize_hostname_strips_unsafe_chars():
    assert sanitize_hostname("web/server:01") == "webserver01"


def test_sanitize_hostname_empty_fallback():
    assert sanitize_hostname("") == "unknown"


def test_sanitize_hostname_all_unsafe_fallback():
    assert sanitize_hostname("///") == "unknown"


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
