"""Integration test for architect: fixture → load → analyze → export."""

import io
import json
import tarfile
import pytest
from pathlib import Path

from yoinkc.architect.loader import load_refined_fleets
from yoinkc.architect.analyzer import analyze_fleets
from yoinkc.architect.export import export_topology


@pytest.fixture()
def three_fleet_dir(tmp_path):
    """Create mock refined fleet tarballs for three fleets."""
    shared = [f"shared-pkg-{i}-1.0-1.el9.x86_64" for i in range(20)]

    fleet_data = {
        "web-servers": shared + ["httpd-2.4-1.el9.x86_64", "mod_ssl-2.4-1.el9.x86_64"],
        "db-servers": shared + ["postgresql-15-1.el9.x86_64", "pgaudit-1.7-1.el9.x86_64"],
        "app-servers": shared + ["python3-3.11-1.el9.x86_64", "gunicorn-21-1.el9.x86_64"],
    }

    for fleet_name, packages in fleet_data.items():
        snapshot = {
            "schema_version": 6,
            "meta": {
                "hostname": fleet_name,
                "fleet": {"source_hosts": [f"{fleet_name}-01"], "total_hosts": 3},
            },
            "os_release": {"name": "RHEL", "version_id": "9.4", "id": "rhel"},
            "rpm": {
                "base_image": "registry.redhat.io/rhel9/rhel-bootc:9.4",
                "packages_added": [
                    {"name": p.split("-")[0], "nvra": p, "source": "dnf"}
                    for p in packages
                ],
            },
            "config": {"files": []},
        }
        snap_json = json.dumps(snapshot).encode()
        tarball_path = tmp_path / f"{fleet_name}.tar.gz"
        with tarfile.open(tarball_path, "w:gz") as tar:
            info = tarfile.TarInfo(name="inspection-snapshot.json")
            info.size = len(snap_json)
            tar.addfile(info, io.BytesIO(snap_json))

    return tmp_path


class TestEndToEnd:
    def test_load_analyze_export(self, three_fleet_dir):
        # Load
        fleets = load_refined_fleets(three_fleet_dir)
        assert len(fleets) == 3

        # Analyze
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base is not None
        assert len(base.packages) == 20  # shared packages
        assert base.fan_out == 3

        # Each derived layer has 2 exclusive packages
        for name in ("web-servers", "db-servers", "app-servers"):
            layer = topo.get_layer(name)
            assert layer is not None
            assert len(layer.packages) == 2
            assert layer.parent == "base"

        # Move a package from base to web
        topo.move_package(base.packages[0], "base", "web-servers")
        assert len(base.packages) == 19
        # Broadcast: all derived layers get the package
        for name in ("web-servers", "db-servers", "app-servers"):
            layer = topo.get_layer(name)
            assert len(layer.packages) == 3  # 2 original + 1 moved

        # Export
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert "base/Containerfile" in names
            assert "web-servers/Containerfile" in names
            assert "db-servers/Containerfile" in names
            assert "app-servers/Containerfile" in names
            assert "build.sh" in names

            # Verify base Containerfile
            base_cf = tar.extractfile("base/Containerfile").read().decode()
            assert "FROM registry.redhat.io/rhel9/rhel-bootc:9.4" in base_cf
            assert "dnf install" in base_cf
            # Verify bare package names (not NVRAs) are used
            assert "shared-pkg-1" in base_cf  # pkg-0 was moved, so use pkg-1
            assert "shared-pkg-1-1.0-1.el9.x86_64" not in base_cf

            # Verify derived references base
            web_cf = tar.extractfile("web-servers/Containerfile").read().decode()
            assert "FROM localhost/base:latest" in web_cf

            # Verify build.sh order
            build = tar.extractfile("build.sh").read().decode()
            assert build.index("localhost/base:latest") < build.index("localhost/web-servers:latest")
