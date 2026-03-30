"""HTTP server for yoinkc architect interactive UI."""

from __future__ import annotations

import json
import logging
import socket
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import TYPE_CHECKING

import jinja2

from yoinkc.architect.analyzer import LayerTopology
from yoinkc.architect.export import export_topology, render_containerfile

if TYPE_CHECKING:
    pass

logger = logging.getLogger(__name__)

_DEFAULT_PORT = 8643


def _find_free_port(start: int = _DEFAULT_PORT, max_attempts: int = 20) -> int:
    """Return first free TCP port at or above *start*."""
    for port in range(start, start + max_attempts):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            try:
                s.bind(("127.0.0.1", port))
                return port
            except OSError:
                continue
    raise RuntimeError(f"No free port found in range {start}-{start + max_attempts}")


def create_handler(
    topology: LayerTopology,
    base_image: str,
    rendered_html: str,
) -> type[BaseHTTPRequestHandler]:
    """Create a request handler class with the topology bound."""

    class _Handler(BaseHTTPRequestHandler):
        _topology = topology
        _base_image = base_image
        _html = rendered_html

        def log_message(self, format, *args):
            logger.debug(format, *args)

        def do_GET(self) -> None:
            path = self.path.split("?")[0]
            if path in ("/", "/index.html"):
                self._send(200, self._html.encode(), "text/html; charset=utf-8")
            elif path == "/api/health":
                self._send_json(200, {"status": "ok"})
            elif path == "/api/topology":
                self._send_json(200, self._topology.to_dict())
            elif path == "/api/export":
                data = export_topology(self._topology, self._base_image)
                self._send(200, data, "application/gzip", {
                    "Content-Disposition": 'attachment; filename="architect-export.tar.gz"',
                })
            elif path.startswith("/api/preview/"):
                layer_name = path[len("/api/preview/"):]
                layer = self._topology.get_layer(layer_name)
                if layer is None:
                    self._send_json(404, {"error": f"Layer {layer_name!r} not found"})
                else:
                    content = render_containerfile(
                        layer.name, layer.parent, layer.packages, self._base_image,
                    )
                    self._send(200, content.encode(), "text/plain; charset=utf-8")
            else:
                self._send(404, b"Not found", "text/plain")

        def do_POST(self) -> None:
            path = self.path.split("?")[0]
            if path == "/api/move":
                try:
                    length = int(self.headers.get("Content-Length", 0))
                    body = json.loads(self.rfile.read(length))
                    self._topology.move_package(
                        body["package"], body["from"], body["to"],
                    )
                    self._send_json(200, self._topology.to_dict())
                except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
                    self._send_json(400, {"error": str(e)})
            elif path == "/api/copy":
                try:
                    length = int(self.headers.get("Content-Length", 0))
                    body = json.loads(self.rfile.read(length))
                    self._topology.copy_package(
                        body["package"], body["from"], body["to"],
                    )
                    self._send_json(200, self._topology.to_dict())
                except (ValueError, KeyError, TypeError, json.JSONDecodeError) as e:
                    self._send_json(400, {"error": str(e)})
            else:
                self._send(404, b"Not found", "text/plain")

        def _send(self, code: int, body: bytes, content_type: str, extra_headers: dict | None = None) -> None:
            self.send_response(code)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            for k, v in (extra_headers or {}).items():
                self.send_header(k, v)
            self.end_headers()
            self.wfile.write(body)

        def _send_json(self, code: int, data: dict) -> None:
            body = json.dumps(data).encode()
            self._send(code, body, "application/json")

    return _Handler


def start_server(
    topology: LayerTopology,
    base_image: str,
    template_dir: Path,
    patternfly_css: str,
    bind: str = "127.0.0.1",
    port: int = 0,
    open_browser: bool = True,
) -> tuple[int, HTTPServer]:
    """Create and return (port, server) without starting serve_forever."""
    # Render template
    env = jinja2.Environment(
        loader=jinja2.FileSystemLoader(str(template_dir)),
        autoescape=False,
    )
    template = env.get_template("architect/architect.html.j2")
    rendered_html = template.render(
        topology_json=json.dumps(topology.to_dict()),
        patternfly_css=patternfly_css,
    )

    handler_class = create_handler(topology, base_image, rendered_html)

    if port == 0:
        # Let OS pick
        httpd = HTTPServer((bind, 0), handler_class)
        actual_port = httpd.server_address[1]
    else:
        actual_port = _find_free_port(port)
        httpd = HTTPServer((bind, actual_port), handler_class)

    url = f"http://{bind}:{actual_port}"
    logger.info("Serving architect UI at %s", url)

    if open_browser:
        try:
            webbrowser.open(url)
        except Exception:
            pass  # best-effort

    return actual_port, httpd
