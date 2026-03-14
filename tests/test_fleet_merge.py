# tests/test_fleet_merge.py
"""Tests for fleet merge engine."""

from yoinkc.schema import (
    InspectionSnapshot, RpmSection, PackageEntry, RepoFile,
    ServiceSection, ServiceStateChange, NetworkSection, FirewallZone,
    FleetPrevalence, OsRelease,
)


def _snap(hostname="web-01", **kwargs):
    """Helper to build a minimal snapshot."""
    return InspectionSnapshot(
        meta={"hostname": hostname},
        os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
        **kwargs,
    )


class TestMergePackages:
    def test_identical_packages_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].name == "httpd"
        assert merged.rpm.packages_added[0].fleet.count == 2
        assert merged.rpm.packages_added[0].fleet.total == 2
        assert merged.rpm.packages_added[0].include is True

    def test_different_packages_both_present(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        names = {p.name for p in merged.rpm.packages_added}
        assert names == {"httpd", "nginx"}
        # At 100% threshold, items on only 1/2 hosts are excluded
        for p in merged.rpm.packages_added:
            assert p.fleet.count == 1
            assert p.include is False

    def test_prevalence_threshold_50(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=50)
        # At 50%, 1/2 = 50% meets threshold
        for p in merged.rpm.packages_added:
            assert p.include is True

    def test_package_identity_by_name_not_version(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4.51", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4.53", release="2", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].fleet.count == 2


class TestMergeServices:
    def test_identical_services_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        sc = ServiceStateChange(
            unit="httpd.service", current_state="enabled",
            default_state="disabled", action="enable",
        )
        s1 = _snap("web-01", services=ServiceSection(state_changes=[sc]))
        s2 = _snap("web-02", services=ServiceSection(state_changes=[sc]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.services.state_changes) == 1
        assert merged.services.state_changes[0].fleet.count == 2

    def test_service_identity_includes_action(self):
        from yoinkc.fleet.merge import merge_snapshots
        sc_enable = ServiceStateChange(
            unit="httpd.service", current_state="enabled",
            default_state="disabled", action="enable",
        )
        sc_disable = ServiceStateChange(
            unit="httpd.service", current_state="disabled",
            default_state="enabled", action="disable",
        )
        s1 = _snap("web-01", services=ServiceSection(state_changes=[sc_enable]))
        s2 = _snap("web-02", services=ServiceSection(state_changes=[sc_disable]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.services.state_changes) == 2


class TestMergeFirewallZones:
    def test_identical_zones_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        z = FirewallZone(path="/etc/firewalld/zones/public.xml", name="public")
        s1 = _snap("web-01", network=NetworkSection(firewall_zones=[z]))
        s2 = _snap("web-02", network=NetworkSection(firewall_zones=[z]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.network.firewall_zones) == 1
        assert merged.network.firewall_zones[0].fleet.count == 2


class TestMergeFleetMeta:
    def test_fleet_meta_in_merged_snapshot(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=90)
        fleet_meta = merged.meta.get("fleet")
        assert fleet_meta is not None
        assert fleet_meta["total_hosts"] == 2
        assert fleet_meta["min_prevalence"] == 90
        assert set(fleet_meta["source_hosts"]) == {"web-01", "web-02"}

    def test_merged_hostname_synthetic(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=100, fleet_name="web-servers")
        assert merged.meta["hostname"] == "web-servers"


class TestMergeNoneSection:
    def test_one_snapshot_missing_rpm(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02")  # no rpm section
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm is not None
        assert len(merged.rpm.packages_added) == 1
        assert merged.rpm.packages_added[0].fleet.count == 1

    def test_all_snapshots_missing_section(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01")
        s2 = _snap("web-02")
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm is None
