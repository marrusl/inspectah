"""
Tests for the inspectah.refine module.

Covers tarball extraction, validation, HTTP server routes, and the
re-render subprocess pipeline.
"""

from __future__ import annotations

import io
import json
import signal
import socket
import subprocess as sp
import tarfile
import threading
import urllib.error
import urllib.request
from argparse import Namespace
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from inspectah.refine import (
    _DEFAULT_PORT,
    _Handler,
    _build_tarball,
    _count_excluded,
    _extract_tarball,
    _find_free_port,
    _re_render,
    _validate_output_dir,
    _wait_for_server_ready,
    run_refine,
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
        "report.html": "<html><body>inspectah report</body></html>",
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
# Re-render via subprocess (calls `inspectah inspect --from-snapshot`)
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
        new_snapshot = {"schema_version": 5, "rpm": None}

        def _fake_run(cmd, **kwargs):
            idx = cmd.index("--output-dir")
            out_path = Path(cmd[idx + 1])
            (out_path / "report.html").write_text(new_html)
            (out_path / "inspection-snapshot.json").write_text(json.dumps(new_snapshot))
            (out_path / "Containerfile").write_text("FROM rhel:9")
            r = MagicMock()
            r.returncode = 0
            r.stdout = ""
            r.stderr = ""
            return r

        with patch("subprocess.run", side_effect=_fake_run):
            ok, result = _re_render(b'{"schema_version": 5}', output_dir)

        assert ok is True
        assert isinstance(result, dict)
        assert new_html in result["html"]
        assert result["snapshot"] == new_snapshot
        assert result["containerfile"] == "FROM rhel:9"
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

        with patch("subprocess.run", side_effect=sp.TimeoutExpired("inspectah", 300)):
            ok, msg = _re_render(b"{}", output_dir)

        assert ok is False
        assert "timeout" in msg.lower() or "300" in msg

    def test_command_not_found_returns_error(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)

        with patch("subprocess.run", side_effect=FileNotFoundError("inspectah")):
            ok, msg = _re_render(b"{}", output_dir)

        assert ok is False
        assert "not found" in msg.lower() or "inspectah" in msg.lower()

    def test_passes_original_snapshot(self, tmp_path):
        output_dir = self._make_output_dir(tmp_path)
        captured_cmd: list[str] = []

        def _fake_run(cmd, **kwargs):
            captured_cmd.extend(cmd)
            idx = cmd.index("--output-dir")
            out_path = Path(cmd[idx + 1])
            (out_path / "report.html").write_text("<html/>")
            (out_path / "inspection-snapshot.json").write_text('{"schema_version": 5}')
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

class TestHandlerSend:
    def test_send_sets_no_cache_headers(self):
        handler = object.__new__(_Handler)
        handler.wfile = io.BytesIO()
        headers: list[tuple[str, str]] = []
        handler.send_response = MagicMock()
        handler.send_header = MagicMock(side_effect=lambda name, value: headers.append((name, value)))
        handler.end_headers = MagicMock()

        handler._send(200, "ok")

        header_map = dict(headers)
        assert header_map["Cache-Control"] == "no-cache, no-store, must-revalidate"
        assert header_map["Pragma"] == "no-cache"
        assert header_map["Expires"] == "0"
        assert handler.wfile.getvalue() == b"ok"


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
        assert headers.get("Pragma") == "no-cache"
        assert headers.get("Expires") == "0"

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
        assert headers.get("Cache-Control") == "no-cache, no-store, must-revalidate"
        assert headers.get("Pragma") == "no-cache"
        assert headers.get("Expires") == "0"
        cd = headers.get("Content-Disposition", "")
        assert "inspectah-refined-" in cd
        assert ".tar.gz" in cd
        with tarfile.open(fileobj=io.BytesIO(body), mode="r:gz") as tf:
            names = tf.getnames()
        assert "report.html" in names

    def test_post_rerender_success(self, live_server):
        url, output_dir = live_server
        result_dict = {
            "html": "<html>refreshed</html>",
            "snapshot": {"schema_version": 5},
            "containerfile": "FROM rhel:9",
        }

        with patch("inspectah.refine._re_render", return_value=(True, result_dict)):
            status, body, headers = _post(url + "/api/re-render", b'{"schema_version": 5}')

        assert status == 200
        data = json.loads(body)
        assert data["html"] == "<html>refreshed</html>"
        assert data["snapshot"] == {"schema_version": 5}
        assert data["containerfile"] == "FROM rhel:9"
        assert "json" in headers.get("Content-Type", "")

    def test_post_rerender_failure_returns_500(self, live_server):
        url, _ = live_server

        with patch("inspectah.refine._re_render", return_value=(False, "Re-render failed")):
            status, body, _ = _post(url + "/api/re-render", b'{"schema_version": 5}')

        assert status == 500

    def test_post_rerender_wrapper_format(self, live_server):
        """Re-render accepts the {"snapshot": ..., "original": ...} wrapper."""
        url, output_dir = live_server
        result_dict = {
            "html": "<html>wrapper-rerender</html>",
            "snapshot": {"schema_version": 5},
            "containerfile": "FROM rhel:9",
        }

        with patch("inspectah.refine._re_render", return_value=(True, result_dict)) as mock_rr:
            wrapper = json.dumps({
                "snapshot": {"schema_version": 5},
                "original": {"schema_version": 5, "meta": {"hostname": "orig"}},
            }).encode()
            status, body, headers = _post(url + "/api/re-render", wrapper)

        assert status == 200
        data = json.loads(body)
        assert data["html"] == "<html>wrapper-rerender</html>"
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


class TestRunRefine:
    def test_signal_handlers_raise_keyboard_interrupt_for_centralized_cleanup(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        registered_handlers = []
        server = MagicMock()
        server.serve_forever.side_effect = KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=True,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        def _capture_handler(_sig, handler):
            registered_handlers.append(handler)

        with (
            patch("inspectah.refine.signal.signal", side_effect=_capture_handler),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
        ):
            result = run_refine(args)

        assert result == 0
        assert len(registered_handlers) == 4
        # First two are the cleanup handlers (SIGINT, SIGTERM)
        for handler in registered_handlers[:2]:
            with pytest.raises(KeyboardInterrupt):
                handler()
        # Last two are SIG_IGN (cleanup-phase ignore)
        assert registered_handlers[2] == signal.SIG_IGN
        assert registered_handlers[3] == signal.SIG_IGN

    def test_waits_for_server_health_before_opening_browser(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        server = MagicMock()
        events: list[tuple[str, str]] = []

        class _FakeResponse:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

            def read(self):
                return b'{"status":"ok"}'

        class _FakeThread:
            def __init__(self, target=None, daemon=None):
                self.target = target
                self.daemon = daemon
                self.join_calls = 0

            def start(self):
                events.append(("thread", "start"))

            def join(self):
                self.join_calls += 1
                events.append(("thread", f"join-{self.join_calls}"))
                if self.join_calls == 1:
                    raise KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=False,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        def _fake_urlopen(url, timeout=0.0):
            events.append(("health", url))
            return _FakeResponse()

        def _fake_open_browser(url):
            events.append(("browser", url))

        with (
            patch("inspectah.refine.signal.signal"),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
            patch("inspectah.refine.time.time", return_value=1_700_000_000),
            patch("inspectah.refine.threading.Thread", _FakeThread),
            patch("inspectah.refine.urllib.request.urlopen", side_effect=_fake_urlopen),
            patch("inspectah.refine._open_browser", side_effect=_fake_open_browser),
        ):
            result = run_refine(args)

        assert result == 0
        assert events == [
            ("thread", "start"),
            ("health", "http://127.0.0.1:8765/api/health"),
            ("browser", "http://localhost:8765?t=1700000000"),
            ("thread", "join-1"),
            ("thread", "join-2"),
        ]

    def test_wait_for_server_ready_brackets_ipv6_host(self):
        class _FakeResponse:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

        with patch("inspectah.refine.urllib.request.urlopen", return_value=_FakeResponse()) as mock_urlopen:
            assert _wait_for_server_ready("::1", 8765, attempts=1)

        mock_urlopen.assert_called_once_with("http://[::1]:8765/api/health", timeout=0.5)

    def test_wait_for_server_ready_uses_ipv6_loopback_for_ipv6_wildcard_bind(self):
        class _FakeResponse:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                return False

        with patch("inspectah.refine.urllib.request.urlopen", return_value=_FakeResponse()) as mock_urlopen:
            assert _wait_for_server_ready("::", 8765, attempts=1)

        mock_urlopen.assert_called_once_with("http://[::1]:8765/api/health", timeout=0.5)

    def test_opens_browser_with_cache_busting_query(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        server = MagicMock()

        class _FakeThread:
            def __init__(self, target=None, daemon=None):
                self.target = target
                self.daemon = daemon
                self.join_calls = 0

            def start(self):
                return None

            def join(self):
                self.join_calls += 1
                if self.join_calls == 1:
                    raise KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=False,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        with (
            patch("inspectah.refine.signal.signal"),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
            patch("inspectah.refine.time.time", return_value=1_700_000_000),
            patch("inspectah.refine._wait_for_server_ready", return_value=True),
            patch("inspectah.refine.threading.Thread", _FakeThread),
            patch("inspectah.refine._open_browser") as mock_open_browser,
        ):
            result = run_refine(args)

        assert result == 0
        mock_open_browser.assert_called_once_with("http://localhost:8765?t=1700000000")

    def test_no_browser_skips_readiness_wait_and_browser_open(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        server = MagicMock()
        server.serve_forever.side_effect = KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=True,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        with (
            patch("inspectah.refine.signal.signal"),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
            patch("inspectah.refine._wait_for_server_ready") as mock_wait_ready,
            patch("inspectah.refine._open_browser") as mock_open_browser,
        ):
            result = run_refine(args)

        assert result == 0
        mock_wait_ready.assert_not_called()
        mock_open_browser.assert_not_called()

    def test_timeout_wait_logs_warning_and_skips_browser_open(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        server = MagicMock()

        class _FakeThread:
            def __init__(self, target=None, daemon=None):
                self.target = target
                self.daemon = daemon
                self.join_calls = 0

            def start(self):
                return None

            def join(self):
                self.join_calls += 1
                if self.join_calls == 1:
                    raise KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=False,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        with (
            patch("inspectah.refine.signal.signal"),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
            patch("inspectah.refine._wait_for_server_ready", return_value=False),
            patch("inspectah.refine.threading.Thread", _FakeThread),
            patch("inspectah.refine._open_browser") as mock_open_browser,
            patch("inspectah.refine._log") as mock_log,
        ):
            result = run_refine(args)

        assert result == 0
        mock_open_browser.assert_not_called()
        mock_log.assert_any_call("warning: refine server did not become ready in time; not opening browser automatically")

    def test_interrupt_shutdown_joins_thread_before_closing_server(self, tmp_path):
        tarball = _minimal_tarball(tmp_path)
        events: list[tuple[str, str]] = []

        server = MagicMock()
        server.shutdown.side_effect = lambda: events.append(("server", "shutdown"))
        server.server_close.side_effect = lambda: events.append(("server", "close"))

        class _FakeThread:
            def __init__(self, target=None, daemon=None):
                self.target = target
                self.daemon = daemon
                self.join_calls = 0

            def start(self):
                events.append(("thread", "start"))

            def join(self):
                self.join_calls += 1
                events.append(("thread", f"join-{self.join_calls}"))
                if self.join_calls == 1:
                    raise KeyboardInterrupt

        args = Namespace(
            tarball=tarball,
            no_browser=False,
            port=_DEFAULT_PORT,
            bind="127.0.0.1",
        )

        with (
            patch("inspectah.refine.signal.signal"),
            patch("inspectah.refine._find_free_port", return_value=8765),
            patch("inspectah.refine._re_render", return_value=(True, "<html>ready</html>")),
            patch("inspectah.refine.HTTPServer", return_value=server),
            patch("inspectah.refine._wait_for_server_ready", return_value=True),
            patch("inspectah.refine.threading.Thread", _FakeThread),
            patch("inspectah.refine._open_browser"),
        ):
            result = run_refine(args)

        assert result == 0
        assert events == [
            ("thread", "start"),
            ("thread", "join-1"),
            ("server", "shutdown"),
            ("thread", "join-2"),
            ("server", "close"),
        ]
