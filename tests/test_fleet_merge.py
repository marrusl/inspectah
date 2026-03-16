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


from yoinkc.schema import (
    ConfigSection, ConfigFileEntry, ContainerSection,
    QuadletUnit, ComposeFile, ComposeService,
    SystemdDropIn, UserGroupSection,
)


class TestMergeConfigVariants:
    def test_identical_config_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        f = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 80")
        s1 = _snap("web-01", config=ConfigSection(files=[f]))
        s2 = _snap("web-02", config=ConfigSection(files=[f]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.config.files) == 1
        assert merged.config.files[0].fleet.count == 2

    def test_different_content_produces_variants(self):
        from yoinkc.fleet.merge import merge_snapshots
        f1 = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 80")
        f2 = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", content="Listen 8080")
        s1 = _snap("web-01", config=ConfigSection(files=[f1]))
        s2 = _snap("web-02", config=ConfigSection(files=[f2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.config.files) == 2
        paths = {f.path for f in merged.config.files}
        assert paths == {"/etc/httpd/conf/httpd.conf"}
        for f in merged.config.files:
            assert f.fleet.count == 1
            assert f.include is False  # 1/2 < 100%

    def test_majority_variant_included_at_threshold(self):
        from yoinkc.fleet.merge import merge_snapshots
        f_majority = ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="version=A")
        f_outlier = ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="version=B")
        snaps = [_snap(f"web-{i:02d}", config=ConfigSection(files=[f_majority])) for i in range(9)]
        snaps.append(_snap("web-09", config=ConfigSection(files=[f_outlier])))
        merged = merge_snapshots(snaps, min_prevalence=90)
        included = [f for f in merged.config.files if f.include]
        excluded = [f for f in merged.config.files if not f.include]
        assert len(included) == 1
        assert included[0].content == "version=A"
        assert len(excluded) == 1
        assert excluded[0].content == "version=B"


class TestMergeQuadletVariants:
    def test_quadlet_content_variants(self):
        from yoinkc.fleet.merge import merge_snapshots
        q1 = QuadletUnit(path="/etc/containers/systemd/app.container", name="app", content="[Container]\nImage=v1")
        q2 = QuadletUnit(path="/etc/containers/systemd/app.container", name="app", content="[Container]\nImage=v2")
        s1 = _snap("web-01", containers=ContainerSection(quadlet_units=[q1]))
        s2 = _snap("web-02", containers=ContainerSection(quadlet_units=[q2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.containers.quadlet_units) == 2


class TestMergeUsersGroups:
    def test_users_deduplicated_by_name(self):
        from yoinkc.fleet.merge import merge_snapshots
        ug1 = UserGroupSection(users=[{"name": "appuser", "uid": 1000}])
        ug2 = UserGroupSection(users=[{"name": "appuser", "uid": 1000}])
        s1 = _snap("web-01", users_groups=ug1)
        s2 = _snap("web-02", users_groups=ug2)
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.users_groups.users) == 1
        assert merged.users_groups.users[0]["fleet"]["count"] == 2


class TestMergeWarnings:
    def test_warnings_deduplicated(self):
        from yoinkc.fleet.merge import merge_snapshots
        w = {"source": "rpm", "message": "package conflict detected"}
        s1 = _snap("web-01")
        s1.warnings = [w]
        s2 = _snap("web-02")
        s2.warnings = [w, {"source": "config", "message": "orphaned file"}]
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.warnings) == 2


class TestMergeLeafAutoPackages:
    def test_leaf_packages_unioned(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
            leaf_packages=["httpd"],
            auto_packages=["apr", "apr-util"],
        ))
        s2 = _snap("web-02", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
            leaf_packages=["httpd", "nginx"],
            auto_packages=["apr"],
        ))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert set(merged.rpm.leaf_packages) == {"httpd", "nginx"}
        assert set(merged.rpm.auto_packages) == {"apr", "apr-util"}

    def test_leaf_fields_none_when_no_snapshots_have_them(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
        ))
        s2 = _snap("web-02", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
        ))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm.leaf_packages is None
        assert merged.rpm.auto_packages is None
        assert merged.rpm.leaf_dep_tree is None

    def test_leaf_fields_preserved_when_mixed_with_none(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
            leaf_packages=["httpd"],
            auto_packages=["apr"],
            leaf_dep_tree={"httpd": ["apr"]},
        ))
        s2 = _snap("web-02", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
        ))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.rpm.leaf_packages == ["httpd"]
        assert merged.rpm.auto_packages == ["apr"]
        assert merged.rpm.leaf_dep_tree == {"httpd": ["apr"]}

    def test_leaf_dep_tree_merged(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
            leaf_dep_tree={"httpd": ["apr", "apr-util"]},
        ))
        s2 = _snap("web-02", rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64")],
            leaf_dep_tree={"httpd": ["apr", "mailcap"], "nginx": ["openssl"]},
        ))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert "httpd" in merged.rpm.leaf_dep_tree
        assert set(merged.rpm.leaf_dep_tree["httpd"]) == {"apr", "apr-util", "mailcap"}
        assert merged.rpm.leaf_dep_tree["nginx"] == ["openssl"]


from yoinkc.schema import NonRpmItem, NonRpmSoftwareSection


class TestMergeNonRpmSoftware:
    def test_non_rpm_items_merged_by_path(self):
        """Non-RPM items with same path across hosts are deduplicated."""
        from yoinkc.fleet.merge import merge_snapshots
        item = NonRpmItem(path="/opt/app/bin/myapp", name="myapp", method="elf")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.non_rpm_software is not None
        assert len(merged.non_rpm_software.items) == 1
        assert merged.non_rpm_software.items[0].fleet.count == 2
        assert merged.non_rpm_software.items[0].fleet.total == 2
        assert merged.non_rpm_software.items[0].include is True

    def test_non_rpm_different_items_both_preserved(self):
        """Different non-RPM items on different hosts are both in merged output."""
        from yoinkc.fleet.merge import merge_snapshots
        i1 = NonRpmItem(path="/opt/app1/bin/app1", name="app1", method="elf")
        i2 = NonRpmItem(path="/opt/app2/bin/app2", name="app2", method="pip")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[i1]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[i2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.non_rpm_software.items) == 2
        paths = {i.path for i in merged.non_rpm_software.items}
        assert paths == {"/opt/app1/bin/app1", "/opt/app2/bin/app2"}

    def test_non_rpm_prevalence_threshold(self):
        """Items below min_prevalence get include=False."""
        from yoinkc.fleet.merge import merge_snapshots
        item_common = NonRpmItem(path="/opt/common/bin/app", name="common", method="elf")
        item_rare = NonRpmItem(path="/opt/rare/bin/app", name="rare", method="elf")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item_common, item_rare]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item_common]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        by_path = {i.path: i for i in merged.non_rpm_software.items}
        assert by_path["/opt/common/bin/app"].include is True
        assert by_path["/opt/rare/bin/app"].include is False

    def test_non_rpm_env_files_content_variants(self):
        """env_files with same path but different content produce variants."""
        from yoinkc.fleet.merge import merge_snapshots
        ef1 = ConfigFileEntry(path="/opt/app/.env", kind="unowned", content="DB_HOST=db1.example.com")
        ef2 = ConfigFileEntry(path="/opt/app/.env", kind="unowned", content="DB_HOST=db2.example.com")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef1]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef2]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.non_rpm_software.env_files) == 2
        for ef in merged.non_rpm_software.env_files:
            assert ef.fleet.count == 1
            assert ef.include is False

    def test_non_rpm_env_files_identical_deduped(self):
        """env_files with same path and content are deduplicated with correct prevalence."""
        from yoinkc.fleet.merge import merge_snapshots
        ef = ConfigFileEntry(path="/opt/app/.env", kind="unowned", content="DB_HOST=db.example.com")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[], env_files=[ef]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.non_rpm_software.env_files) == 1
        assert merged.non_rpm_software.env_files[0].fleet.count == 2
        assert merged.non_rpm_software.env_files[0].include is True


class TestNoHostsMode:
    def test_strip_host_lists(self):
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("web-01", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        s2 = _snap("web-02", rpm=RpmSection(packages_added=[
            PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=100, include_hosts=False)
        assert merged.rpm.packages_added[0].fleet.hosts == []
        assert merged.rpm.packages_added[0].fleet.count == 2
