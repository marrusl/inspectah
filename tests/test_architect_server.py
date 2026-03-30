"""Tests for architect HTTP server API."""

import json
import threading
import urllib.request
import urllib.error
import pytest
from pathlib import Path

from yoinkc.architect.analyzer import FleetInput, analyze_fleets
from yoinkc.architect.server import create_handler, start_server


def _make_topology():
    fleets = [
        FleetInput(name="web", packages=["httpd", "openssl", "bash"], configs=["/etc/httpd/httpd.conf"]),
        FleetInput(name="db", packages=["postgresql", "openssl", "bash"], configs=["/etc/pg/pg.conf"]),
    ]
    return analyze_fleets(fleets)


@pytest.fixture()
def server_url(tmp_path):
    """Start architect server on a free port, yield URL, stop after test."""
    topo = _make_topology()
    port, httpd = start_server(
        topo,
        base_image="registry.redhat.io/rhel9/rhel-bootc:9.4",
        template_dir=Path(__file__).resolve().parent.parent / "src" / "yoinkc" / "templates",
        patternfly_css="/* test */",
        bind="127.0.0.1",
        port=0,  # let OS pick a free port
        open_browser=False,
    )
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{port}"
    httpd.shutdown()


class TestHealthEndpoint:
    def test_health_returns_ok(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/health")
        assert resp.status == 200
        data = json.loads(resp.read())
        assert data["status"] == "ok"


class TestTopologyEndpoint:
    def test_returns_topology(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/topology")
        assert resp.status == 200
        data = json.loads(resp.read())
        assert "layers" in data
        assert "fleets" in data
        assert len(data["layers"]) == 3  # base + web + db

    def test_layers_have_expected_fields(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/topology")
        data = json.loads(resp.read())
        layer = data["layers"][0]
        assert "name" in layer
        assert "packages" in layer
        assert "fan_out" in layer
        assert "turbulence" in layer


class TestMoveEndpoint:
    def test_move_package_between_layers(self, server_url):
        body = json.dumps({"package": "httpd", "from": "web", "to": "db"}).encode()
        req = urllib.request.Request(
            f"{server_url}/api/move",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        resp = urllib.request.urlopen(req)
        assert resp.status == 200
        data = json.loads(resp.read())
        # Response should include updated topology
        assert "layers" in data
        db_layer = next(l for l in data["layers"] if l["name"] == "db")
        assert "httpd" in db_layer["packages"]

    def test_move_nonexistent_returns_400(self, server_url):
        body = json.dumps({"package": "fake", "from": "web", "to": "db"}).encode()
        req = urllib.request.Request(
            f"{server_url}/api/move",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with pytest.raises(urllib.error.HTTPError) as exc_info:
            urllib.request.urlopen(req)
        assert exc_info.value.code == 400


    def test_move_malformed_json_returns_400(self, server_url):
        req = urllib.request.Request(
            f"{server_url}/api/move",
            data=b"not json",
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with pytest.raises(urllib.error.HTTPError) as exc_info:
            urllib.request.urlopen(req)
        assert exc_info.value.code == 400


class TestExportEndpoint:
    def test_export_returns_tarball(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/api/export")
        assert resp.status == 200
        assert resp.headers["Content-Type"] == "application/gzip"
        data = resp.read()
        assert len(data) > 0


class TestIndexEndpoint:
    def test_index_returns_html(self, server_url):
        resp = urllib.request.urlopen(f"{server_url}/")
        assert resp.status == 200
        content = resp.read().decode()
        assert "yoinkc Architect" in content
