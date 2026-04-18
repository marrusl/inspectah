"""
Interactive refinement server for inspectah output.

Extracts a inspectah output tarball, serves the HTML report over HTTP,
and handles live re-rendering via the ``inspectah inspect --from-snapshot``
subprocess pipeline.

Usage (via CLI):
    inspectah refine output-tarball.tar.gz [--no-browser] [--port PORT]
"""

from __future__ import annotations

import io
import json
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.request
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

_DEFAULT_PORT = 8642
_REQUIRED_FILES = ("report.html", "inspection-snapshot.json")
_LOG_PREFIX = "inspectah refine"
_CACHE_CONTROL = "no-cache, no-store, must-revalidate"
_PRAGMA = "no-cache"
_EXPIRES = "0"


# ---------------------------------------------------------------------------
# Logging helpers
# ---------------------------------------------------------------------------

def _log(msg: str) -> None:
    print(f"{_LOG_PREFIX}: {msg}", flush=True)


def _err(msg: str) -> None:
    print(f"{_LOG_PREFIX}: error: {msg}", file=sys.stderr, flush=True)


# ---------------------------------------------------------------------------
# Port finding
# ---------------------------------------------------------------------------

def _find_free_port(start: int = _DEFAULT_PORT, max_attempts: int = 20) -> int:
    """Return the first free TCP port at or above *start*."""
    for port in range(start, start + max_attempts):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                s.bind(("127.0.0.1", port))
                return port
            except OSError:
                continue
    raise RuntimeError(f"No free port found in range {start}\u2013{start + max_attempts - 1}")


# ---------------------------------------------------------------------------
# Tarball helpers
# ---------------------------------------------------------------------------

def _extract_tarball(tarball_path: Path, dest: Path) -> None:
    """Extract *tarball_path* into *dest*."""
    with tarfile.open(tarball_path, "r:gz") as tf:
        members = []
        for m in tf.getmembers():
            safe_name = "/".join(
                part for part in m.name.replace("\\", "/").split("/")
                if part and part != ".."
            )
            if not safe_name:
                continue
            m.name = safe_name
            members.append(m)
        try:
            tf.extractall(dest, members=members, filter='data')  # noqa: S202
        except TypeError:
            tf.extractall(dest, members=members)  # noqa: S202


def _validate_output_dir(d: Path) -> None:
    """Raise SystemExit if required files are missing from *d*."""
    missing = [f for f in _REQUIRED_FILES if not (d / f).exists()]
    if missing:
        subdirs = [p for p in d.iterdir() if p.is_dir()]
        if len(subdirs) == 1:
            inner = subdirs[0]
            missing = [f for f in _REQUIRED_FILES if not (inner / f).exists()]
            if not missing:
                for item in inner.iterdir():
                    shutil.move(str(item), str(d / item.name))
                inner.rmdir()
                return
        _err(f"tarball is missing required file(s): {', '.join(missing)}")
        sys.exit(1)


# ---------------------------------------------------------------------------
# Excluded-item counter
# ---------------------------------------------------------------------------

def _count_excluded(snapshot: dict) -> int:
    """Count items where include==False across the snapshot."""
    count = 0
    for section_val in snapshot.values():
        if not isinstance(section_val, dict):
            continue
        for list_val in section_val.values():
            if isinstance(list_val, list):
                for item in list_val:
                    if isinstance(item, dict) and item.get("include") is False:
                        count += 1
    return count


# ---------------------------------------------------------------------------
# Re-render via subprocess
# ---------------------------------------------------------------------------

def _re_render(
    snapshot_data: bytes,
    output_dir: Path,
    original_data: bytes | None = None,
) -> tuple[bool, dict | str]:
    """
    Re-render by calling ``inspectah inspect --from-snapshot``.

    Returns (success, result).  On success the second element is a dict
    with keys ``html``, ``snapshot``, and ``containerfile``; on failure
    it is the error text.
    """
    with tempfile.TemporaryDirectory(prefix="inspectah-rerender-") as tmp:
        tmp_path = Path(tmp)
        snap_file = tmp_path / "snapshot.json"
        snap_file.write_bytes(snapshot_data)
        new_output = tmp_path / "output"
        new_output.mkdir()

        cmd = [
            "inspectah", "inspect",
            "--from-snapshot", str(snap_file),
            "--output-dir", str(new_output),
            "--refine-mode",
        ]
        if original_data:
            orig_file = tmp_path / "original-snapshot.json"
            orig_file.write_bytes(original_data)
            cmd += ["--original-snapshot", str(orig_file)]

        _log(f"re-rendering: {' '.join(cmd)}")
        t0 = time.monotonic()
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=300,
            )
        except FileNotFoundError as exc:
            return False, f"inspectah command not found: {exc}"
        except subprocess.TimeoutExpired:
            return False, "Re-render timed out after 300 seconds."

        elapsed = time.monotonic() - t0

        if result.returncode != 0:
            log = (result.stdout or "") + (result.stderr or "")
            return False, f"Re-render failed (exit {result.returncode}):\n{log}"

        new_html = new_output / "report.html"
        if not new_html.exists():
            return False, "Re-render ran but did not produce report.html"

        for item in list(output_dir.iterdir()):
            if item.is_dir():
                shutil.rmtree(item)
            else:
                item.unlink()
        for item in new_output.iterdir():
            dest = output_dir / item.name
            if item.is_dir():
                shutil.copytree(item, dest)
            else:
                shutil.copy2(item, dest)

        _log(f"done in {elapsed:.1f}s, serving updated report")

        # Build JSON result with all three pieces the client needs
        html_content = (output_dir / "report.html").read_text()
        snapshot_path = output_dir / "inspection-snapshot.json"
        snapshot_json = json.loads(snapshot_path.read_text()) if snapshot_path.exists() else {}
        containerfile_path = output_dir / "Containerfile"
        containerfile_text = containerfile_path.read_text() if containerfile_path.exists() else ""

        return True, {
            "html": html_content,
            "snapshot": snapshot_json,
            "containerfile": containerfile_text,
        }


# ---------------------------------------------------------------------------
# Tarball builder
# ---------------------------------------------------------------------------

def _build_tarball(output_dir: Path) -> bytes:
    """Return a tar.gz of *output_dir* as bytes."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tf:
        for item in sorted(output_dir.rglob("*")):
            arcname = item.relative_to(output_dir)
            tf.add(item, arcname=str(arcname))
    return buf.getvalue()


# ---------------------------------------------------------------------------
# HTTP request handler
# ---------------------------------------------------------------------------

class _Handler(BaseHTTPRequestHandler):
    """HTTP handler for inspectah refine routes."""

    output_dir: Path
    re_render_available: bool = True

    # ------------------------------------------------------------------
    # Low-level helpers
    # ------------------------------------------------------------------

    def log_message(self, fmt: str, *args: object) -> None:  # noqa: N802
        pass  # suppress default access log noise

    def _send_shared_headers(self) -> None:
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Cache-Control", _CACHE_CONTROL)
        self.send_header("Pragma", _PRAGMA)
        self.send_header("Expires", _EXPIRES)

    def _send(
        self,
        code: int,
        body: bytes | str,
        content_type: str = "text/plain; charset=utf-8",
        extra_headers: dict[str, str] | None = None,
    ) -> None:
        if isinstance(body, str):
            body = body.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self._send_shared_headers()
        for name, value in (extra_headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def _send_file(self, path: Path, content_type: str = "application/octet-stream") -> None:
        try:
            data = path.read_bytes()
        except OSError as exc:
            self._send(404, f"File not found: {exc}")
            return
        self._send(200, data, content_type)

    def _read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length", 0))
        if length:
            return self.rfile.read(length)
        return b""

    # ------------------------------------------------------------------
    # Routing
    # ------------------------------------------------------------------

    def do_GET(self) -> None:  # noqa: N802
        path = self.path.split("?")[0]
        output_dir = self.__class__.output_dir

        if path in ("/", "/index.html"):
            self._send_file(output_dir / "report.html", "text/html; charset=utf-8")

        elif path == "/api/health":
            self._send(
                200,
                json.dumps({"status": "ok", "re_render": self.__class__.re_render_available}),
                "application/json",
            )

        elif path == "/snapshot":
            self._send_file(
                output_dir / "inspection-snapshot.json",
                "application/json",
            )

        elif path == "/api/tarball":
            timestamp = time.strftime("%Y%m%d-%H%M%S")
            filename = f"inspectah-refined-{timestamp}.tar.gz"
            data = _build_tarball(output_dir)
            self._send(
                200,
                data,
                "application/gzip",
                extra_headers={"Content-Disposition": f'attachment; filename="{filename}"'},
            )
            _log(f"tarball downloaded: {filename}")

        else:
            rel = path.lstrip("/")
            candidate = output_dir / rel
            if candidate.is_file():
                self._send_file(candidate)
            else:
                self._send(404, f"Not found: {path}")

    def do_POST(self) -> None:  # noqa: N802
        path = self.path.split("?")[0]
        output_dir = self.__class__.output_dir

        if path == "/api/re-render":
            body = self._read_body()
            if not body:
                self._send(400, "Empty request body")
                return
            try:
                payload = json.loads(body)
            except json.JSONDecodeError as exc:
                self._send(400, f"Invalid JSON: {exc}")
                return

            if isinstance(payload, dict) and "snapshot" in payload and "original" in payload:
                snapshot_data = payload["snapshot"]
                original_data = payload["original"]
            else:
                snapshot_data = payload
                original_data = None

            excluded = _count_excluded(snapshot_data)
            _log(f"re-rendering... ({excluded} item{'s' if excluded != 1 else ''} excluded)")

            snapshot_bytes = json.dumps(snapshot_data).encode()
            original_bytes = json.dumps(original_data).encode() if original_data else None
            ok, result = _re_render(snapshot_bytes, output_dir, original_bytes)
            if ok:
                self._send(200, json.dumps(result), "application/json")
            else:
                self._send(500, result)
        else:
            self._send(404, f"Not found: {path}")

    def do_OPTIONS(self) -> None:  # noqa: N802
        self.send_response(200)
        self._send_shared_headers()
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()


# ---------------------------------------------------------------------------
# Browser opener
# ---------------------------------------------------------------------------

def _open_browser(url: str) -> None:
    """Open *url* in the default browser, platform-appropriately."""
    try:
        if sys.platform == "darwin":
            subprocess.run(["open", url], check=False)
        elif sys.platform == "win32":
            subprocess.run(["start", url], shell=True, check=False)
        else:
            subprocess.run(["xdg-open", url], check=False)
    except Exception:
        pass


def _healthcheck_url(bind: str, port: int) -> str:
    """Return the local healthcheck URL for the refine server."""
    if bind == "0.0.0.0":
        health_host = "127.0.0.1"
    elif bind == "::":
        health_host = "::1"
    else:
        health_host = bind
    if ":" in health_host and not health_host.startswith("["):
        health_host = f"[{health_host}]"
    return f"http://{health_host}:{port}/api/health"


def _wait_for_server_ready(
    bind: str,
    port: int,
    *,
    attempts: int = 50,
    delay: float = 0.1,
    timeout: float = 0.5,
) -> bool:
    """Poll the refine health endpoint until it responds or attempts are exhausted."""
    health_url = _healthcheck_url(bind, port)
    for _ in range(attempts):
        try:
            with urllib.request.urlopen(health_url, timeout=timeout) as response:
                if response.status == 200:
                    return True
        except (OSError, TimeoutError, urllib.error.URLError):
            time.sleep(delay)
    return False


# ---------------------------------------------------------------------------
# Entry point (called from __main__.py)
# ---------------------------------------------------------------------------

def run_refine(args) -> int:
    """
    Main entry point for ``inspectah refine``.

    *args* is the argparse Namespace with ``tarball``, ``no_browser``,
    and ``port`` attributes.
    """
    tarball = args.tarball
    if not tarball.exists():
        _err(f"file not found: {tarball}")
        return 1
    if not tarfile.is_tarfile(tarball):
        _err(f"not a valid tar.gz file: {tarball}")
        return 1

    tmpdir = tempfile.mkdtemp(prefix="inspectah-refine-")
    output_dir = Path(tmpdir)
    server: HTTPServer | None = None
    server_thread: threading.Thread | None = None

    def _cleanup(*_: object) -> None:
        raise KeyboardInterrupt

    signal.signal(signal.SIGINT, _cleanup)
    signal.signal(signal.SIGTERM, _cleanup)

    try:
        try:
            _log(f"extracting {tarball} \u2026")
            _extract_tarball(tarball, output_dir)
        except Exception as exc:
            _err(f"failed to extract tarball: {exc}")
            return 1
        _validate_output_dir(output_dir)

        file_count = sum(1 for _ in output_dir.rglob("*") if _.is_file())

        snapshot_path = output_dir / "inspection-snapshot.json"
        host_label = ""
        try:
            snap = json.loads(snapshot_path.read_text())
            host_label = (snap.get("meta") or {}).get("hostname", "")
        except Exception:
            pass

        # Initial re-render with --refine-mode so the served report has the
        # editor UI from the first page load.
        re_render_available = True
        if snapshot_path.exists():
            _log("re-rendering with editor UI enabled\u2026")
            snap_bytes = snapshot_path.read_bytes()
            ok, result = _re_render(snap_bytes, output_dir, original_data=snap_bytes)
            if ok:
                _log("editor UI ready")
            else:
                re_render_available = False
                _log(f"warning: initial re-render failed, serving static report: {result[:200]}")

        port = args.port
        if port == _DEFAULT_PORT:
            try:
                port = _find_free_port(start=port)
            except RuntimeError as exc:
                _err(str(exc))
                return 1

        _Handler.output_dir = output_dir  # type: ignore[attr-defined]
        _Handler.re_render_available = re_render_available  # type: ignore[attr-defined]

        bind = getattr(args, "bind", "127.0.0.1")
        server = HTTPServer((bind, port), _Handler)
        cache_buster = int(time.time())
        url = f"http://localhost:{port}?t={cache_buster}"

        src_name = tarball.name
        summary = f"{file_count} files from {src_name}" + (f"  [{host_label}]" if host_label else "")
        rule = "\u2500" * 42
        _log(f"extracted {summary}")
        _log(rule)
        _log(f"  Report: {url}")
        _log(rule)
        _log("edit items in the browser, then click Re-render")
        _log("Ctrl+C to stop")

        if args.no_browser:
            server.serve_forever()
        else:
            server_thread = threading.Thread(target=server.serve_forever, daemon=True)
            server_thread.start()
            if _wait_for_server_ready(bind, port):
                _open_browser(url)
            else:
                _log("warning: refine server did not become ready in time; not opening browser automatically")
            server_thread.join()
        return 0
    except KeyboardInterrupt:
        return 0
    finally:
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        if server_thread is not None and server is not None:
            server.shutdown()
            server_thread.join()
        if server is not None:
            server.server_close()
        shutil.rmtree(tmpdir, ignore_errors=True)
