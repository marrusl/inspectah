"""Tests for architect Containerfile tree export."""

import io
import tarfile
import pytest

from yoinkc.architect.analyzer import FleetInput, Layer, LayerTopology, analyze_fleets
from yoinkc.architect.export import export_topology


def _make_topology() -> LayerTopology:
    fleets = [
        FleetInput(name="web", packages=["httpd", "openssl", "bash"], configs=["/etc/httpd/httpd.conf"]),
        FleetInput(name="db", packages=["postgresql", "openssl", "bash"], configs=["/etc/pg/pg.conf"]),
    ]
    return analyze_fleets(fleets)


class TestExportTopology:
    def test_returns_bytes(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        assert isinstance(data, bytes)
        assert len(data) > 0

    def test_is_valid_tarball(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert len(names) > 0

    def test_contains_base_containerfile(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            assert "base/Containerfile" in tar.getnames()

    def test_contains_derived_containerfiles(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            names = tar.getnames()
            assert "web/Containerfile" in names
            assert "db/Containerfile" in names

    def test_base_containerfile_has_from_upstream(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("base/Containerfile").read().decode()
            assert "FROM registry.redhat.io/rhel9/rhel-bootc:9.4" in content

    def test_derived_containerfile_has_from_base(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("web/Containerfile").read().decode()
            assert "FROM localhost/base:latest" in content

    def test_base_containerfile_has_dnf_install(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("base/Containerfile").read().decode()
            assert "dnf install -y" in content
            assert "openssl" in content
            assert "bash" in content

    def test_contains_build_sh(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            assert "build.sh" in tar.getnames()
            content = tar.extractfile("build.sh").read().decode()
            assert "localhost/base:latest" in content
            assert "localhost/web:latest" in content

    def test_build_sh_builds_base_first(self):
        topo = _make_topology()
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("build.sh").read().decode()
            base_pos = content.index("localhost/base:latest")
            web_pos = content.index("localhost/web:latest")
            assert base_pos < web_pos

    def test_empty_layer_no_dnf_line(self):
        fleets = [
            FleetInput(name="web", packages=["openssl"], configs=[]),
            FleetInput(name="db", packages=["openssl"], configs=[]),
        ]
        topo = analyze_fleets(fleets)
        # Both packages go to base, derived layers are empty
        data = export_topology(topo, base_image="registry.redhat.io/rhel9/rhel-bootc:9.4")
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            content = tar.extractfile("web/Containerfile").read().decode()
            assert "dnf install" not in content
