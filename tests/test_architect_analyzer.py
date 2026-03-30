"""Tests for architect cross-fleet analyzer."""

import pytest

from yoinkc.architect.analyzer import (
    FleetInput,
    Layer,
    LayerTopology,
    analyze_fleets,
)


def _make_fleet(name: str, packages: list[str], host_count: int = 3) -> FleetInput:
    return FleetInput(name=name, packages=packages, configs=[], host_count=host_count)


class TestAnalyzeFleets:
    def test_single_fleet_no_base_extraction(self):
        fleets = [_make_fleet("web", ["httpd", "openssl", "bash"])]
        topo = analyze_fleets(fleets)
        # Single fleet should produce no base layer
        assert len(topo.layers) == 1
        assert topo.layers[0].name == "web"
        assert topo.layers[0].parent is None

    def test_two_fleets_common_packages_go_to_base(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base is not None
        assert set(base.packages) == {"openssl", "bash"}
        assert base.parent is None

    def test_two_fleets_exclusive_packages_go_to_derived(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        web = topo.get_layer("web")
        db = topo.get_layer("db")
        assert web is not None and "httpd" in web.packages
        assert db is not None and "postgresql" in db.packages
        assert web.parent == "base"
        assert db.parent == "base"

    def test_package_in_some_fleets_duplicated_to_each(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash", "curl"]),
            _make_fleet("db", ["postgresql", "openssl", "bash", "curl"]),
            _make_fleet("gpu", ["nvidia", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        # curl is in 2/3 fleets — NOT in base, duplicated to web and db
        assert "curl" not in base.packages
        assert "curl" in topo.get_layer("web").packages
        assert "curl" in topo.get_layer("db").packages

    def test_all_packages_shared_everything_in_base(self):
        fleets = [
            _make_fleet("web", ["openssl", "bash"]),
            _make_fleet("db", ["openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert set(base.packages) == {"openssl", "bash"}
        # Derived layers exist but are empty
        assert topo.get_layer("web").packages == []
        assert topo.get_layer("db").packages == []

    def test_no_overlap_empty_base(self):
        fleets = [
            _make_fleet("web", ["httpd"]),
            _make_fleet("db", ["postgresql"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base.packages == []

    def test_fan_out_computed(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
            _make_fleet("gpu", ["nvidia", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        base = topo.get_layer("base")
        assert base.fan_out == 3  # three derived layers

    def test_turbulence_computed_with_floor(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        web = topo.get_layer("web")
        # 1 package, fan_out=0 → formula gives 0, but floor is 1.0 for non-base
        assert web.turbulence >= 1.0


class TestMovePackage:
    def test_move_between_derived_layers(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        topo.move_package("httpd", "web", "db")
        assert "httpd" not in topo.get_layer("web").packages
        assert "httpd" in topo.get_layer("db").packages

    def test_move_from_base_broadcasts_to_all_derived(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        topo.move_package("openssl", "base", "web")
        assert "openssl" not in topo.get_layer("base").packages
        # Broadcast: openssl should be in ALL derived layers
        assert "openssl" in topo.get_layer("web").packages
        assert "openssl" in topo.get_layer("db").packages

    def test_move_updates_turbulence(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        old_turbulence = topo.get_layer("base").turbulence
        topo.move_package("openssl", "base", "web")
        new_turbulence = topo.get_layer("base").turbulence
        assert new_turbulence != old_turbulence  # should change since pkg count changed

    def test_move_nonexistent_package_raises(self):
        fleets = [
            _make_fleet("web", ["httpd", "bash"]),
            _make_fleet("db", ["postgresql", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        with pytest.raises(ValueError, match="not found"):
            topo.move_package("nonexistent", "base", "web")


class TestTopologyJson:
    def test_to_json_roundtrip(self):
        fleets = [
            _make_fleet("web", ["httpd", "openssl", "bash"]),
            _make_fleet("db", ["postgresql", "openssl", "bash"]),
        ]
        topo = analyze_fleets(fleets)
        data = topo.to_dict()
        assert "layers" in data
        assert "fleets" in data
        assert len(data["layers"]) == 3  # base + 2 derived
