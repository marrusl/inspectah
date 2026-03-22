"""
Tests for the yoinkc.refine module.

Covers tarball extraction, validation, HTTP server routes, and the
re-render subprocess pipeline.
"""

from __future__ import annotations

import io
import json
import socket
import subprocess as sp
import tarfile
import threading
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from yoinkc.refine import (
    _DEFAULT_PORT,
    _Handler,
    _build_tarball,
    _count_excluded,
    _extract_tarball,
    _find_free_port,
    _re_render,
    _validate_output_dir,
)


# ---------------------------------------------------------------------------
# Helpers to build minimal tarballs for testing
# ---------------------------------------------------------------------------

def _make_tarball(files: dict[str, str], dest: Path) -> Path:
    """Write a tar.gz at *dest* containing *files* (name -> content)."""
    with tarfile.open(dest, "w:gz") as tf:
        for name, content in files.items():
            data = content.encode()
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
    return dest


def _minimal_tarball(tmp_path: Path, extra: dict | None = None) -> Path:
    files = {
        "report.html": "<html><body>yoinkc report</body></html>",
        "inspection-snapshot.json": json.dumps({"schema_version": 5}),
    }
    if extra:
        files.update(extra)
    return _make_tarball(files, tmp_path / "test.tar.gz")


# ---------------------------------------------------------------------------
# Port finding
# ---------------------------------------------------------------------------

class TestFindFreePort:
    def test_returns_integer(self):
        port = _find_free_port()
        assert isinstance(port, int)
        assert port > 0

    def test_port_is_bindable(self):
        port = _find_free_port()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind(("127.0.0.1", port))  # should not raise

    def test_increments_when_busy(self):
        start = 59900
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied:
            occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            occupied.bind(("127.0.0.1", start))
            occupied.listen(1)
            port = _find_free_port(start=start, max_attempts=10)
            assert port > start


# ---------------------------------------------------------------------------
# Tarball helpers
# ---------------------------------------------------------------------------

class TestExtractAndValidate:
    def test_extracts_files(self, tmp_path):
        tb = _minimal_tarball(tmp_path)
        out = tmp_path / "out"
        out.mkdir()
        _extract_tarball(tb, out)
        assert (out / "report.html").exists()
        assert (out / "inspection-snapshot.json").exists()

    def test_validate_passes_when_files_present(self, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        (out / "inspection-snapshot.json").write_text("{}")
        _validate_output_dir(out)  # should not raise or exit

    def test_validate_fails_with_system_exit_on_missing(self, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        # inspection-snapshot.json is missing
        with pytest.raises(SystemExit):
            _validate_output_dir(out)

    def test_validate_unwraps_single_subdirectory(self, tmp_path):
        """Tarballs that wrap contents in one dir are transparently unwrapped."""
        files = {
            "wrapper/report.html": "<html/>",
            "wrapper/inspection-snapshot.json": "{}",
        }
        tb = _make_tarball(files, tmp_path / "wrapped.tar.gz")
        out = tmp_path / "out"
        out.mkdir()
        _extract_tarball(tb, out)
        _validate_output_dir(out)
        assert (out / "report.html").exists()

    def test_extract_prevents_path_traversal(self, tmp_path):
        """Entries with '..' in their path must not escape the destination."""
        escape_target = tmp_path / "escape.txt"
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w:gz") as tf:
            data = b"injected"
            info = tarfile.TarInfo(name="../escape.txt")
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
        buf.seek(0)
        tb = tmp_path / "evil.tar.gz"
        tb.write_bytes(buf.getvalue())
        out = tmp_path / "out"
        out.mkdir()
        _extract_tarball(tb, out)
        assert not escape_target.exists()


# ---------------------------------------------------------------------------
# Tarball builder
# ---------------------------------------------------------------------------

class TestBuildTarball:
    def test_returns_valid_targz(self, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        (out / "data.json").write_text("{}")
        data = _build_tarball(out)
        assert isinstance(data, bytes)
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tf:
            names = tf.getnames()
        assert "report.html" in names
        assert "data.json" in names


# ---------------------------------------------------------------------------
# Count excluded
# ---------------------------------------------------------------------------

class TestCountExcluded:
    def test_counts_false_include_fields(self):
        snapshot = {
            "rpm": {
                "packages_added": [
                    {"name": "httpd", "include": True},
                    {"name": "nginx", "include": False},
                    {"name": "curl", "include": False},
                ],
            },
            "config": {
                "files": [
                    {"path": "/etc/foo", "include": False},
                ],
            },
        }
        assert _count_excluded(snapshot) == 3

    def test_returns_zero_when_all_included(self):
        snapshot = {
            "rpm": {"packages_added": [{"name": "httpd", "include": True}]},
        }
        assert _count_excluded(snapshot) == 0

    def test_ignores_non_list_fields(self):
        snapshot = {
            "meta": {"hostname": "myhost"},
            "schema_version": 5,
        }
        assert _count_excluded(snapshot) == 0


# ---------------------------------------------------------------------------
# Re-render via subprocess (calls `yoinkc inspect --from-snapshot`)
# ---------------------------------------------------------------------------

class TestReRender:
    def _make_output_dir(self, tmp_path: Path) -> Path:
        d = tmp_path / "output"
        d.mkdir()
        (d / "report.html").write_text("<html>old</html>")
        (d / "inspection-snapshot.json").write_text("{}")
        return d

    def test_successful_rerender_updates_report(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)
        new_html = "<html>new</html>"

        def _fake_run(cmd, **kwargs):
            idx = cmd.index("--output-dir")
            out_path = Path(cmd[idx + 1])
            (out_path / "report.html").write_text(new_html)
            (out_path / "inspection-snapshot.json").write_text("{}")
            r = MagicMock()
            r.returncode = 0
            r.stdout = ""
            r.stderr = ""
            return r

        with patch("subprocess.run", side_effect=_fake_run):
            ok, html = _re_render(b'{"schema_version": 5}', output_dir)

        assert ok is True
        assert new_html in html
        assert (output_dir / "report.html").read_text() == new_html

    def test_subprocess_failure_returns_log(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)

        def _fail(*args, **kwargs):
            r = MagicMock()
            r.returncode = 1
            r.stdout = "some stdout"
            r.stderr = "some error"
            return r

        with patch("subprocess.run", side_effect=_fail):
            ok, msg = _re_render(b"{}", output_dir)

        assert ok is False
        assert "some error" in msg or "some stdout" in msg

    def test_timeout_returns_error(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)

        with patch("subprocess.run", side_effect=sp.TimeoutExpired("yoinkc", 300)):
            ok, msg = _re_render(b"{}", output_dir)

        assert ok is False
        assert "timeout" in msg.lower() or "300" in msg

    def test_command_not_found_returns_error(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)

        with patch("subprocess.run", side_effect=FileNotFoundError("yoinkc")):
            ok, msg = _re_render(b"{}", output_dir)

        assert ok is False
        assert "not found" in msg.lower() or "yoinkc" in msg.lower()

    def test_passes_original_snapshot(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)
        captured_cmd: list[str] = []

        def _fake_run(cmd, **kwargs):
            captured_cmd.extend(cmd)
            idx = cmd.index("--output-dir")
            out_path = Path(cmd[idx + 1])
            (out_path / "report.html").write_text("<html/>")
            (out_path / "inspection-snapshot.json").write_text("{}")
            r = MagicMock()
            r.returncode = 0
            r.stdout = r.stderr = ""
            return r

        snap = b'{"schema_version": 5}'
        orig = b'{"schema_version": 5, "meta": {"hostname": "orig"}}'
        with patch("subprocess.run", side_effect=_fake_run):
            ok, _ = _re_render(snap, output_dir, original_data=orig)

        assert ok is True
        assert "--original-snapshot" in captured_cmd
        assert "--refine-mode" in captured_cmd
        assert "--from-snapshot" in captured_cmd


# ---------------------------------------------------------------------------
# HTTP server (live server in a background thread)
# ---------------------------------------------------------------------------

@pytest.fixture()
def live_server(tmp_path):
    """Spin up the HTTP server in a background thread; yield (url, output_dir)."""
    from http.server import HTTPServer

    output_dir = tmp_path / "output"
    output_dir.mkdir()
    (output_dir / "report.html").write_text("<html>report</html>")
    (output_dir / "inspection-snapshot.json").write_text(
        json.dumps({"schema_version": 5, "rpm": None})
    )
    (output_dir / "extra.txt").write_text("extra content")

    port = _find_free_port(start=59200)
    _Handler.output_dir = output_dir  # type: ignore[attr-defined]
    _Handler.re_render_available = True  # type: ignore[attr-defined]
    server = HTTPServer(("127.0.0.1", port), _Handler)

    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    yield f"http://127.0.0.1:{port}", output_dir

    server.shutdown()
    server.server_close()


def _get(url: str, *, timeout: float = 5.0) -> tuple[int, bytes, dict]:
    """Return (status_code, body_bytes, headers_dict) for a GET request."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return r.status, r.read(), dict(r.headers)
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read(), {}


def _post(url: str, body: bytes, content_type: str = "application/json", *, timeout: float = 10.0):
    req = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": content_type},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, r.read(), dict(r.headers)
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read(), {}


class TestServer:
    def test_get_root_serves_report_html(self, live_server):
        url, _ = live_server
        status, body, _ = _get(url + "/")
        assert status == 200
        assert b"<html>report</html>" in body

    def test_get_health_returns_ok(self, live_server):
        url, _ = live_server
        status, body, _ = _get(url + "/api/health")
        assert status == 200
        data = json.loads(body)
        assert data["status"] == "ok"
        assert data["re_render"] is True

    def test_cors_headers_on_all_responses(self, live_server):
        url, _ = live_server
        _, _, headers = _get(url + "/api/health")
        assert headers.get("Access-Control-Allow-Origin") == "*"

    def test_cache_control_prevents_stale_responses(self, live_server):
        url, _ = live_server
        _, _, headers = _get(url + "/")
        cc = headers.get("Cache-Control", "")
        assert "no-cache" in cc
        assert "no-store" in cc

    def test_get_snapshot_serves_json(self, live_server):
        url, _ = live_server
        status, body, headers = _get(url + "/snapshot")
        assert status == 200
        assert "json" in headers.get("Content-Type", "")
        data = json.loads(body)
        assert "schema_version" in data

    def test_get_static_file(self, live_server):
        url, _ = live_server
        status, body, _ = _get(url + "/extra.txt")
        assert status == 200
        assert b"extra content" in body

    def test_get_unknown_returns_404(self, live_server):
        url, _ = live_server
        status, _, _ = _get(url + "/does-not-exist")
        assert status == 404

    def test_get_tarball_returns_targz(self, live_server):
        url, _ = live_server
        status, body, headers = _get(url + "/api/tarball")
        assert status == 200
        assert "gzip" in headers.get("Content-Type", "")
        cd = headers.get("Content-Disposition", "")
        assert "yoinkc-refined-" in cd
        assert ".tar.gz" in cd
        with tarfile.open(fileobj=io.BytesIO(body), mode="r:gz") as tf:
            names = tf.getnames()
        assert "report.html" in names

    def test_post_rerender_success(self, live_server):
        url, output_dir = live_server
        new_html = "<html>refreshed</html>"

        with patch("yoinkc.refine._re_render", return_value=(True, new_html)):
            status, body, headers = _post(url + "/api/re-render", b'{"schema_version": 5}')

        assert status == 200
        assert new_html.encode() in body
        assert "html" in headers.get("Content-Type", "")

    def test_post_rerender_failure_returns_500(self, live_server):
        url, _ = live_server

        with patch("yoinkc.refine._re_render", return_value=(False, "Re-render failed")):
            status, body, _ = _post(url + "/api/re-render", b'{"schema_version": 5}')

        assert status == 500

    def test_post_rerender_wrapper_format(self, live_server):
        """Re-render accepts the {"snapshot": ..., "original": ...} wrapper."""
        url, output_dir = live_server
        new_html = "<html>wrapper-rerender</html>"

        with patch("yoinkc.refine._re_render", return_value=(True, new_html)) as mock_rr:
            wrapper = json.dumps({
                "snapshot": {"schema_version": 5},
                "original": {"schema_version": 5, "meta": {"hostname": "orig"}},
            }).encode()
            status, body, headers = _post(url + "/api/re-render", wrapper)

        assert status == 200
        assert new_html.encode() in body
        # Verify original_data was passed through
        call_args = mock_rr.call_args
        assert call_args[0][2] is not None  # third positional = original_data

    def test_post_rerender_empty_body_returns_400(self, live_server):
        url, _ = live_server
        status, _, _ = _post(url + "/api/re-render", b"")
        assert status == 400

    def test_post_rerender_invalid_json_returns_400(self, live_server):
        url, _ = live_server
        status, _, _ = _post(url + "/api/re-render", b"not-json")
        assert status == 400
