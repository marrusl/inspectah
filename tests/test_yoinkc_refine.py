"""
Tests for the yoinkc-refine helper script.

The script lives at the repo root as a standalone Python file (not a package).
We import it by path using importlib so no package structure is needed.
"""

from __future__ import annotations

import importlib.util
import io
import json
import os
import socket
import sys
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

# ---------------------------------------------------------------------------
# Import the script as a module by absolute path
# ---------------------------------------------------------------------------

_SCRIPT_PATH = Path(__file__).resolve().parent.parent / "yoinkc-refine"


def _load_refine():
    """Import yoinkc-refine as a module (handles hyphens in name, no .py extension)."""
    from importlib.machinery import SourceFileLoader
    loader = SourceFileLoader("yoinkc_refine", str(_SCRIPT_PATH))
    spec = importlib.util.spec_from_loader("yoinkc_refine", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


@pytest.fixture(scope="module")
def refine():
    return _load_refine()


# ---------------------------------------------------------------------------
# Helpers to build minimal tarballs for testing
# ---------------------------------------------------------------------------

def _make_tarball(files: dict[str, str], dest: Path) -> Path:
    """Write a tar.gz at *dest* containing *files* (name → content)."""
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
    def test_returns_integer(self, refine):
        port = refine._find_free_port()
        assert isinstance(port, int)
        assert port > 0

    def test_port_is_bindable(self, refine):
        port = refine._find_free_port()
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind(("127.0.0.1", port))  # should not raise

    def test_increments_when_busy(self, refine):
        start = 59900
        # Occupy start port
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied:
            occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            occupied.bind(("127.0.0.1", start))
            occupied.listen(1)
            port = refine._find_free_port(start=start, max_attempts=10)
            assert port > start


# ---------------------------------------------------------------------------
# Tarball helpers
# ---------------------------------------------------------------------------

class TestExtractAndValidate:
    def test_extracts_files(self, refine, tmp_path):
        tb = _minimal_tarball(tmp_path)
        out = tmp_path / "out"
        out.mkdir()
        refine._extract_tarball(tb, out)
        assert (out / "report.html").exists()
        assert (out / "inspection-snapshot.json").exists()

    def test_validate_passes_when_files_present(self, refine, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        (out / "inspection-snapshot.json").write_text("{}")
        refine._validate_output_dir(out)  # should not raise or exit

    def test_validate_fails_with_system_exit_on_missing(self, refine, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        # inspection-snapshot.json is missing
        with pytest.raises(SystemExit):
            refine._validate_output_dir(out)

    def test_validate_unwraps_single_subdirectory(self, refine, tmp_path):
        """Tarballs that wrap contents in one dir are transparently unwrapped."""
        # Build a tarball with a wrapper dir
        files = {
            "wrapper/report.html": "<html/>",
            "wrapper/inspection-snapshot.json": "{}",
        }
        tb = _make_tarball(files, tmp_path / "wrapped.tar.gz")
        out = tmp_path / "out"
        out.mkdir()
        refine._extract_tarball(tb, out)
        refine._validate_output_dir(out)
        # After unwrap, files should be directly in out/
        assert (out / "report.html").exists()

    def test_extract_prevents_path_traversal(self, refine, tmp_path):
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
        refine._extract_tarball(tb, out)
        assert not escape_target.exists()


# ---------------------------------------------------------------------------
# Tarball builder
# ---------------------------------------------------------------------------

class TestBuildTarball:
    def test_returns_valid_targz(self, refine, tmp_path):
        out = tmp_path / "out"
        out.mkdir()
        (out / "report.html").write_text("<html/>")
        (out / "data.json").write_text("{}")
        data = refine._build_tarball(out)
        assert isinstance(data, bytes)
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tf:
            names = tf.getnames()
        assert "report.html" in names
        assert "data.json" in names


# ---------------------------------------------------------------------------
# Runtime detection
# ---------------------------------------------------------------------------

class TestFindRuntime:
    def test_finds_podman_when_available(self, refine):
        with patch("shutil.which", side_effect=lambda x: "/usr/bin/podman" if x == "podman" else None):
            assert refine._find_runtime() == "podman"

    def test_falls_back_to_docker(self, refine):
        with patch("shutil.which", side_effect=lambda x: "/usr/bin/docker" if x == "docker" else None):
            assert refine._find_runtime() == "docker"

    def test_returns_none_when_neither_found(self, refine):
        with patch("shutil.which", return_value=None):
            assert refine._find_runtime() is None


# ---------------------------------------------------------------------------
# Count excluded
# ---------------------------------------------------------------------------

class TestCountExcluded:
    def test_counts_false_include_fields(self, refine):
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
        assert refine._count_excluded(snapshot) == 3

    def test_returns_zero_when_all_included(self, refine):
        snapshot = {
            "rpm": {"packages_added": [{"name": "httpd", "include": True}]},
        }
        assert refine._count_excluded(snapshot) == 0

    def test_ignores_non_list_fields(self, refine):
        snapshot = {
            "meta": {"hostname": "myhost"},
            "schema_version": 5,
        }
        assert refine._count_excluded(snapshot) == 0


# ---------------------------------------------------------------------------
# Image pre-pull
# ---------------------------------------------------------------------------

class TestEnsureImage:
    def test_pull_success_logs_done(self, refine, capsys):
        with patch("subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=0)
            refine._ensure_image("podman")
        out = capsys.readouterr().out
        assert "done" in out
        assert mock_run.call_count == 1  # only pull, no check needed

    def test_pull_failure_with_cache_logs_cached(self, refine, capsys):
        results = [MagicMock(returncode=1), MagicMock(returncode=0)]
        with patch("subprocess.run", side_effect=results):
            refine._ensure_image("podman")
        out = capsys.readouterr().out
        assert "failed" in out
        assert "cached" in out

    def test_pull_failure_no_cache_warns(self, refine, capsys):
        with patch("subprocess.run", return_value=MagicMock(returncode=1)):
            refine._ensure_image("podman")
        captured = capsys.readouterr()
        combined = captured.out + captured.err
        assert "failed" in combined
        assert "Re-render will not be available" in combined


# ---------------------------------------------------------------------------
# Re-render via container
# ---------------------------------------------------------------------------

class TestReRender:
    def _make_output_dir(self, tmp_path: Path) -> Path:
        d = tmp_path / "output"
        d.mkdir()
        (d / "report.html").write_text("<html>old</html>")
        (d / "inspection-snapshot.json").write_text("{}")
        return d

    def test_no_runtime_returns_error(self, refine, tmp_path):
        with patch.object(refine, "_find_runtime", return_value=None):
            ok, msg = refine._re_render(b'{"schema_version": 5}', self._make_output_dir(tmp_path))
        assert ok is False
        assert "podman" in msg.lower() or "docker" in msg.lower()

    def test_successful_rerender_replaces_output(self, refine, tmp_path):
        output_dir = self._make_output_dir(tmp_path)
        new_html = "<html>new</html>"

        def _fake_run(cmd, **kwargs):
            # Write new output as if the container did it
            new_out = Path(cmd[cmd.index("-v", 4) + 1].split(":")[0])
            (new_out / "report.html").write_text(new_html)
            (new_out / "inspection-snapshot.json").write_text("{}")
            result = MagicMock()
            result.returncode = 0
            result.stdout = ""
            result.stderr = ""
            return result

        with patch.object(refine, "_find_runtime", return_value="podman"), \
             patch("subprocess.run", side_effect=_fake_run):
            ok, html = refine._re_render(b'{"schema_version": 5}', output_dir)

        assert ok is True
        assert new_html in html
        assert (output_dir / "report.html").read_text() == new_html

    def test_container_failure_returns_log(self, refine, tmp_path):
        output_dir = self._make_output_dir(tmp_path)

        def _fail(*args, **kwargs):
            r = MagicMock()
            r.returncode = 1
            r.stdout = "some stdout"
            r.stderr = "some error"
            return r

        with patch.object(refine, "_find_runtime", return_value="podman"), \
             patch("subprocess.run", side_effect=_fail):
            ok, msg = refine._re_render(b"{}", output_dir)

        assert ok is False
        assert "some error" in msg or "some stdout" in msg

    def test_falls_back_to_docker(self, refine, tmp_path):
        """When podman is unavailable but docker is, docker is used."""
        output_dir = self._make_output_dir(tmp_path)
        called_with: list[str] = []

        def _fake_which(name: str) -> str | None:
            return None if name == "podman" else f"/usr/bin/{name}"

        def _fake_run(cmd, **kwargs):
            called_with.append(cmd[0])
            new_out = Path(cmd[cmd.index("-v", 4) + 1].split(":")[0])
            (new_out / "report.html").write_text("<html>docker</html>")
            (new_out / "inspection-snapshot.json").write_text("{}")
            r = MagicMock()
            r.returncode = 0
            r.stdout = r.stderr = ""
            return r

        with patch("shutil.which", side_effect=_fake_which), \
             patch("subprocess.run", side_effect=_fake_run):
            ok, _ = refine._re_render(b"{}", output_dir)

        assert ok is True
        assert called_with[0] == "docker"


# ---------------------------------------------------------------------------
# HTTP server (live server in a background thread)
# ---------------------------------------------------------------------------

@pytest.fixture()
def live_server(refine, tmp_path):
    """Spin up the HTTP server in a background thread; yield (url, output_dir)."""
    from http.server import HTTPServer

    output_dir = tmp_path / "output"
    output_dir.mkdir()
    (output_dir / "report.html").write_text("<html>report</html>")
    (output_dir / "inspection-snapshot.json").write_text(
        json.dumps({"schema_version": 5, "rpm": None})
    )
    (output_dir / "extra.txt").write_text("extra content")

    port = refine._find_free_port(start=59200)
    # Inject output_dir into handler
    refine._Handler.output_dir = output_dir  # type: ignore[attr-defined]
    server = HTTPServer(("127.0.0.1", port), refine._Handler)

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

    def test_get_health_returns_ok(self, live_server, refine):
        url, _ = live_server
        refine._Handler.re_render_available = True
        status, body, _ = _get(url + "/api/health")
        assert status == 200
        data = json.loads(body)
        assert data["status"] == "ok"
        assert data["re_render"] is True

    def test_get_health_re_render_false_when_no_runtime(self, live_server, refine):
        url, _ = live_server
        refine._Handler.re_render_available = False
        try:
            status, body, _ = _get(url + "/api/health")
            assert status == 200
            data = json.loads(body)
            assert data["status"] == "ok"
            assert data["re_render"] is False
        finally:
            refine._Handler.re_render_available = True

    def test_cors_headers_on_all_responses(self, live_server):
        url, _ = live_server
        _, _, headers = _get(url + "/api/health")
        assert headers.get("Access-Control-Allow-Origin") == "*"

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
        # Verify it's a valid tar.gz
        with tarfile.open(fileobj=io.BytesIO(body), mode="r:gz") as tf:
            names = tf.getnames()
        assert "report.html" in names

    def test_post_rerender_no_runtime_returns_500(self, live_server, refine):
        url, _ = live_server
        with patch.object(refine, "_find_runtime", return_value=None):
            status, body, _ = _post(url + "/api/re-render", b'{"schema_version": 5}')
        assert status == 500

    def test_post_rerender_with_podman_mock(self, live_server, refine):
        url, output_dir = live_server
        new_html = "<html>refreshed</html>"

        def _fake_run(cmd, **kwargs):
            new_out = Path(cmd[cmd.index("-v", 4) + 1].split(":")[0])
            (new_out / "report.html").write_text(new_html)
            (new_out / "inspection-snapshot.json").write_text("{}")
            r = MagicMock()
            r.returncode = 0
            r.stdout = r.stderr = ""
            return r

        with patch.object(refine, "_find_runtime", return_value="podman"), \
             patch("subprocess.run", side_effect=_fake_run):
            status, body, headers = _post(url + "/api/re-render", b'{"schema_version": 5}')

        assert status == 200
        assert new_html.encode() in body
        assert "html" in headers.get("Content-Type", "")

    def test_post_rerender_wrapper_format(self, live_server, refine):
        """Re-render accepts the {"snapshot": ..., "original": ...} wrapper."""
        url, output_dir = live_server
        new_html = "<html>wrapper-rerender</html>"

        def _fake_run(cmd, **kwargs):
            for i, arg in enumerate(cmd):
                if arg == "-v" and ":/output" in cmd[i + 1]:
                    new_out = Path(cmd[i + 1].split(":")[0])
                    break
            (new_out / "report.html").write_text(new_html)
            (new_out / "inspection-snapshot.json").write_text("{}")
            r = MagicMock()
            r.returncode = 0
            r.stdout = r.stderr = ""
            return r

        wrapper = json.dumps({
            "snapshot": {"schema_version": 5},
            "original": {"schema_version": 5, "meta": {"hostname": "orig"}},
        }).encode()

        with patch.object(refine, "_find_runtime", return_value="podman"), \
             patch("subprocess.run", side_effect=_fake_run):
            status, body, headers = _post(url + "/api/re-render", wrapper)

        assert status == 200
        assert new_html.encode() in body

    def test_post_rerender_empty_body_returns_400(self, live_server):
        url, _ = live_server
        status, _, _ = _post(url + "/api/re-render", b"")
        assert status == 400

    def test_post_rerender_invalid_json_returns_400(self, live_server):
        url, _ = live_server
        status, _, _ = _post(url + "/api/re-render", b"not-json")
        assert status == 400
