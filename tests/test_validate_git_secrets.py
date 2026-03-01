"""Tests for validate.py, git_github.py, and redact.scan_directory_for_secrets."""

import tempfile
from pathlib import Path

from yoinkc.validate import _append_build_failure_to_reports, run_validate
from yoinkc.redact import scan_directory_for_secrets
from yoinkc.git_github import output_stats


# ---------------------------------------------------------------------------
# validate.py
# ---------------------------------------------------------------------------

def test_run_validate_no_containerfile():
    """When no Containerfile exists, validate returns True (nothing to do)."""
    with tempfile.TemporaryDirectory() as tmp:
        assert run_validate(Path(tmp)) is True


def test_append_build_failure_to_audit_report():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "audit-report.md").write_text("# Report\n\nSome content.\n")
        _append_build_failure_to_reports(d, "Error: package xyz not found")
        text = (d / "audit-report.md").read_text()
        assert "Build validation failed" in text
        assert "package xyz not found" in text


def test_append_build_failure_to_html_report():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "report.html").write_text("<html><body></body></html>")
        _append_build_failure_to_reports(d, "Error: missing dep")
        html = (d / "report.html").read_text()
        assert "Build validation failed" in html
        assert "missing dep" in html


def test_append_build_failure_escapes_html():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "report.html").write_text("<html><body></body></html>")
        _append_build_failure_to_reports(d, '<script>alert("xss")</script>')
        html = (d / "report.html").read_text()
        assert "<script>" not in html
        assert "&lt;script&gt;" in html


def test_append_build_failure_no_reports():
    """Gracefully handles missing report files."""
    with tempfile.TemporaryDirectory() as tmp:
        _append_build_failure_to_reports(Path(tmp), "some error")


# ---------------------------------------------------------------------------
# redact.scan_directory_for_secrets
# ---------------------------------------------------------------------------

def test_scan_clean_directory():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "config.txt").write_text("some_setting=value\nmode=production\n")
        (d / "notes.md").write_text("# Notes\n\nNothing secret here.\n")
        assert scan_directory_for_secrets(d) is None


def test_scan_detects_api_key():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "clean.txt").write_text("hello world\n")
        (d / "config.env").write_text("API_KEY=TESTKEY_not_real_xxxxxxxxxxxxxxxxxxxx\n")
        result = scan_directory_for_secrets(d)
        assert result is not None
        assert "config.env" in result


def test_scan_detects_private_key():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "server.key").write_text(
            "-----BEGIN RSA PRIVATE KEY-----\n"
            "MIIEowIBAAKCAQEA...\n"
            "-----END RSA PRIVATE KEY-----\n"
        )
        result = scan_directory_for_secrets(d)
        assert result is not None


def test_scan_detects_password():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "db.conf").write_text("password=supersecret123\n")
        result = scan_directory_for_secrets(d)
        assert result is not None


def test_scan_skips_git_directory():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        git_dir = d / ".git" / "objects"
        git_dir.mkdir(parents=True)
        (git_dir / "secret.txt").write_text("password=leaked\n")
        (d / "clean.txt").write_text("safe content\n")
        assert scan_directory_for_secrets(d) is None


def test_scan_handles_binary_files():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "binary.bin").write_bytes(b"\x00\x01\x02\xff\xfe")
        assert scan_directory_for_secrets(d) is None


# ---------------------------------------------------------------------------
# git_github.output_stats
# ---------------------------------------------------------------------------

def test_output_stats_counts():
    with tempfile.TemporaryDirectory() as tmp:
        d = Path(tmp)
        (d / "Containerfile").write_text("FROM base\n# FIXME: check this\nRUN echo\n")
        (d / "audit-report.md").write_text("# Report\n\n2 FIXME items\n\nAnother FIXME.\n")
        sub = d / "config" / "etc"
        sub.mkdir(parents=True)
        (sub / "foo.conf").write_text("key=value\n")
        total, count, fixmes = output_stats(d)
        assert count == 3
        assert fixmes == 3
        assert total > 0


def test_output_stats_empty_dir():
    with tempfile.TemporaryDirectory() as tmp:
        total, count, fixmes = output_stats(Path(tmp))
        assert count == 0
        assert total == 0
        assert fixmes == 0
