# tests/test_fleet_merge.py
"""Tests for fleet merge engine."""

import pytest
from yoinkc.schema import (
    InspectionSnapshot, RpmSection, PackageEntry, RepoFile,
    ServiceSection, ServiceStateChange, NetworkSection, FirewallZone,
    FleetPrevalence, OsRelease, ConfigSection, ConfigFileEntry,
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

    def test_merge_stores_display_names_on_snapshots_and_fleet_metadata(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap(
            "web-01.east.example.com",
            rpm=RpmSection(packages_added=[
                PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
            ]),
        )
        s2 = _snap(
            "web-01.west.example.com",
            rpm=RpmSection(packages_added=[
                PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
            ]),
        )

        merged = merge_snapshots([s1, s2], min_prevalence=100)

        assert s1.meta["display_name"] == "web-01.east"
        assert s2.meta["display_name"] == "web-01.west"
        assert merged.meta["fleet"]["source_hosts"] == ["web-01.east", "web-01.west"]
        assert merged.rpm.packages_added[0].fleet.hosts == ["web-01.east", "web-01.west"]


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


from yoinkc.schema import EnabledModuleStream, VersionLockEntry


class TestMergeModuleStreams:
    def test_same_stream_three_hosts_prevalence(self):
        """Three hosts with the same module:stream → one entry, prevalence 3/3."""
        from yoinkc.fleet.merge import merge_snapshots
        ms = EnabledModuleStream(module_name="postgresql", stream="15")
        snaps = [_snap(f"host-{i}", rpm=RpmSection(module_streams=[ms])) for i in range(3)]
        merged = merge_snapshots(snaps, min_prevalence=100)
        assert len(merged.rpm.module_streams) == 1
        assert merged.rpm.module_streams[0].module_name == "postgresql"
        assert merged.rpm.module_streams[0].stream == "15"
        assert merged.rpm.module_streams[0].fleet.count == 3
        assert merged.rpm.module_streams[0].fleet.total == 3
        assert merged.rpm.module_streams[0].include is True

    def test_different_streams_produce_separate_entries(self):
        """2 hosts on stream 15, 1 host on stream 13 → two entries, each with own prevalence."""
        from yoinkc.fleet.merge import merge_snapshots
        ms15 = EnabledModuleStream(module_name="postgresql", stream="15")
        ms13 = EnabledModuleStream(module_name="postgresql", stream="13")
        snaps = [
            _snap("host-0", rpm=RpmSection(module_streams=[ms15])),
            _snap("host-1", rpm=RpmSection(module_streams=[ms15])),
            _snap("host-2", rpm=RpmSection(module_streams=[ms13])),
        ]
        merged = merge_snapshots(snaps, min_prevalence=50)
        assert len(merged.rpm.module_streams) == 2
        by_stream = {e.stream: e for e in merged.rpm.module_streams}
        assert by_stream["15"].fleet.count == 2
        assert by_stream["13"].fleet.count == 1
        assert by_stream["15"].include is True   # 2/3 ≥ 50%
        assert by_stream["13"].include is False  # 1/3 < 50%

    def test_profiles_unioned_across_hosts(self):
        """Profiles from different hosts sharing the same stream are unioned."""
        from yoinkc.fleet.merge import merge_snapshots
        ms1 = EnabledModuleStream(module_name="nodejs", stream="18", profiles=["development"])
        ms2 = EnabledModuleStream(module_name="nodejs", stream="18", profiles=["minimal", "development"])
        ms3 = EnabledModuleStream(module_name="nodejs", stream="18", profiles=["default"])
        snaps = [
            _snap("host-0", rpm=RpmSection(module_streams=[ms1])),
            _snap("host-1", rpm=RpmSection(module_streams=[ms2])),
            _snap("host-2", rpm=RpmSection(module_streams=[ms3])),
        ]
        merged = merge_snapshots(snaps, min_prevalence=100)
        assert len(merged.rpm.module_streams) == 1
        assert set(merged.rpm.module_streams[0].profiles) == {"development", "minimal", "default"}

    def test_conflicts_recomputed_and_rendered_in_fleet_mode(self):
        """Fleet merge must recompute module stream conflicts for renderer warnings."""
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.renderers.containerfile.packages import section_lines

        conflict = "postgresql: host=15, base_image=13"
        snaps = [
            _snap(
                "host-0",
                rpm=RpmSection(
                    base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                    baseline_module_streams={"postgresql": "13"},
                    module_streams=[
                        EnabledModuleStream(
                            module_name="postgresql",
                            stream="15",
                            baseline_match=False,
                        )
                    ],
                    module_stream_conflicts=[conflict],
                ),
            ),
            _snap(
                "host-1",
                rpm=RpmSection(
                    base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                    baseline_module_streams={"postgresql": "13"},
                    module_streams=[
                        EnabledModuleStream(
                            module_name="postgresql",
                            stream="15",
                            baseline_match=False,
                        )
                    ],
                    module_stream_conflicts=[conflict],
                ),
            ),
        ]

        merged = merge_snapshots(snaps, min_prevalence=100)

        assert merged.rpm.baseline_module_streams == {"postgresql": "13"}
        assert merged.rpm.module_stream_conflicts == [conflict]

        rendered = "\n".join(
            section_lines(
                merged,
                base="registry.redhat.io/rhel9/rhel-bootc:9.6",
                c_ext_pip=[],
                needs_multistage=False,
            )
        )
        assert "# WARNING: postgresql: host=15, base_image=13" in rendered


class TestMergeVersionLocks:
    def test_same_version_three_hosts_prevalence(self):
        """Three hosts locking the same package+version → one entry, prevalence 3/3."""
        from yoinkc.fleet.merge import merge_snapshots
        vl = VersionLockEntry(
            raw_pattern="curl-7.76.1-26.el9.x86_64",
            name="curl", epoch=0, version="7.76.1", release="26.el9", arch="x86_64",
        )
        snaps = [_snap(f"host-{i}", rpm=RpmSection(version_locks=[vl])) for i in range(3)]
        merged = merge_snapshots(snaps, min_prevalence=100)
        assert len(merged.rpm.version_locks) == 1
        assert merged.rpm.version_locks[0].name == "curl"
        assert merged.rpm.version_locks[0].fleet.count == 3
        assert merged.rpm.version_locks[0].fleet.total == 3
        assert merged.rpm.version_locks[0].include is True

    def test_different_versions_produce_separate_entries(self):
        """2 hosts on version A, 1 host on version B → two entries with own prevalence."""
        from yoinkc.fleet.merge import merge_snapshots
        vl_a = VersionLockEntry(
            raw_pattern="curl-7.76.1-26.el9.x86_64",
            name="curl", epoch=0, version="7.76.1", release="26.el9", arch="x86_64",
        )
        vl_b = VersionLockEntry(
            raw_pattern="curl-7.79.1-2.el9.x86_64",
            name="curl", epoch=0, version="7.79.1", release="2.el9", arch="x86_64",
        )
        snaps = [
            _snap("host-0", rpm=RpmSection(version_locks=[vl_a])),
            _snap("host-1", rpm=RpmSection(version_locks=[vl_a])),
            _snap("host-2", rpm=RpmSection(version_locks=[vl_b])),
        ]
        merged = merge_snapshots(snaps, min_prevalence=50)
        assert len(merged.rpm.version_locks) == 2
        by_version = {e.version: e for e in merged.rpm.version_locks}
        assert by_version["7.76.1"].fleet.count == 2
        assert by_version["7.79.1"].fleet.count == 1
        assert by_version["7.76.1"].include is True   # 2/3 ≥ 50%
        assert by_version["7.79.1"].include is False  # 1/3 < 50%

    def test_different_arch_stays_separate(self):
        """curl.x86_64 and curl.i686 are separate entries even with same version."""
        from yoinkc.fleet.merge import merge_snapshots
        vl_x86 = VersionLockEntry(
            raw_pattern="curl-7.76.1-26.el9.x86_64",
            name="curl", epoch=0, version="7.76.1", release="26.el9", arch="x86_64",
        )
        vl_i686 = VersionLockEntry(
            raw_pattern="curl-7.76.1-26.el9.i686",
            name="curl", epoch=0, version="7.76.1", release="26.el9", arch="i686",
        )
        s1 = _snap("host-0", rpm=RpmSection(version_locks=[vl_x86, vl_i686]))
        s2 = _snap("host-1", rpm=RpmSection(version_locks=[vl_x86, vl_i686]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.rpm.version_locks) == 2
        by_arch = {e.arch: e for e in merged.rpm.version_locks}
        assert by_arch["x86_64"].fleet.count == 2
        assert by_arch["i686"].fleet.count == 2
        assert by_arch["x86_64"].name == "curl"
        assert by_arch["i686"].name == "curl"


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


from yoinkc.schema import SelinuxPortLabel, SelinuxSection


class TestMergeSelinux:
    def test_selinux_port_labels_merged(self):
        """Port labels with same protocol/port are deduplicated across hosts."""
        from yoinkc.fleet.merge import merge_snapshots
        pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
        s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.selinux is not None
        assert len(merged.selinux.port_labels) == 1
        assert merged.selinux.port_labels[0].fleet.count == 2
        assert merged.selinux.port_labels[0].fleet.total == 2
        assert merged.selinux.port_labels[0].include is True

    def test_selinux_different_ports_preserved(self):
        """Different protocol/port combinations are all preserved."""
        from yoinkc.fleet.merge import merge_snapshots
        pl1 = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
        pl2 = SelinuxPortLabel(protocol="tcp", port="9090", type="http_port_t")
        s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl1], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl2], mode="enforcing"))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.selinux.port_labels) == 2
        ports = {pl.port for pl in merged.selinux.port_labels}
        assert ports == {"8080", "9090"}

    def test_selinux_port_labels_prevalence(self):
        """Port labels below threshold get include=False."""
        from yoinkc.fleet.merge import merge_snapshots
        pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
        s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(port_labels=[], mode="enforcing"))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.selinux.port_labels) == 1
        assert merged.selinux.port_labels[0].include is False
        assert merged.selinux.port_labels[0].fleet.count == 1

    def test_selinux_boolean_overrides_deduped(self):
        """Boolean overrides are deduplicated by name with fleet prevalence."""
        from yoinkc.fleet.merge import merge_snapshots
        b1 = {"name": "httpd_can_network_connect", "current": "on", "default": "off"}
        s1 = _snap("host1", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert len(merged.selinux.boolean_overrides) == 1
        assert merged.selinux.boolean_overrides[0]["fleet"]["count"] == 2

    def test_selinux_string_lists_unioned(self):
        """String list fields are unioned across hosts."""
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("host1", selinux=SelinuxSection(
            custom_modules=["mymod1"], fcontext_rules=["/opt/app(/.*)? system_u:object_r:httpd_sys_content_t:s0"],
            mode="enforcing",
        ))
        s2 = _snap("host2", selinux=SelinuxSection(
            custom_modules=["mymod2"], fcontext_rules=["/srv/data(/.*)? system_u:object_r:httpd_sys_content_t:s0"],
            mode="enforcing",
        ))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert set(merged.selinux.custom_modules) == {"mymod1", "mymod2"}
        assert len(merged.selinux.fcontext_rules) == 2

    def test_selinux_scalars_first_snapshot(self):
        """Scalar fields (mode, fips_mode) pass through from first snapshot."""
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("host1", selinux=SelinuxSection(mode="enforcing", fips_mode=True))
        s2 = _snap("host2", selinux=SelinuxSection(mode="permissive", fips_mode=False))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.selinux.mode == "enforcing"
        assert merged.selinux.fips_mode is True

    def test_selinux_mode_disagreement_merges(self):
        """Hosts with different SELinux modes still produce a valid merged section."""
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("host1", selinux=SelinuxSection(mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(mode="disabled"))
        s3 = _snap("host3", selinux=SelinuxSection(mode="enforcing"))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=100)
        assert merged.selinux is not None
        assert merged.selinux.mode == "enforcing"


class TestDeduplicateDictsHosts:
    def test_deduplicate_dicts_includes_hosts(self):
        """Dict-based fleet prevalence includes host list."""
        from yoinkc.fleet.merge import merge_snapshots
        b1 = {"name": "httpd_can_network_connect", "current": "on", "default": "off"}
        s1 = _snap("host1", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(boolean_overrides=[b1], mode="enforcing"))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        fleet = merged.selinux.boolean_overrides[0]["fleet"]
        assert "hosts" in fleet
        assert set(fleet["hosts"]) == {"host1", "host2"}


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

    def test_strip_host_lists_selinux(self):
        """--no-hosts strips host lists from selinux port_labels."""
        from yoinkc.fleet.merge import merge_snapshots
        pl = SelinuxPortLabel(protocol="tcp", port="8080", type="http_port_t")
        s1 = _snap("host1", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
        s2 = _snap("host2", selinux=SelinuxSection(port_labels=[pl], mode="enforcing"))
        merged = merge_snapshots([s1, s2], include_hosts=False)
        assert merged.selinux.port_labels[0].fleet.hosts == []
        assert merged.selinux.port_labels[0].fleet.count == 2

    def test_strip_host_lists_non_rpm(self):
        """--no-hosts strips host lists from non_rpm_software items."""
        from yoinkc.fleet.merge import merge_snapshots
        item = NonRpmItem(path="/opt/app/bin/myapp", name="myapp", method="elf")
        s1 = _snap("host1", non_rpm_software=NonRpmSoftwareSection(items=[item]))
        s2 = _snap("host2", non_rpm_software=NonRpmSoftwareSection(items=[item]))
        merged = merge_snapshots([s1, s2], include_hosts=False)
        assert merged.non_rpm_software.items[0].fleet.hosts == []
        assert merged.non_rpm_software.items[0].fleet.count == 2

    def test_strip_host_lists_users_groups(self):
        """--no-hosts strips host lists from users_groups (drive-by fix)."""
        from yoinkc.fleet.merge import merge_snapshots
        s1 = _snap("host1", users_groups=UserGroupSection(
            users=[{"name": "appuser", "uid": 1001}],
        ))
        s2 = _snap("host2", users_groups=UserGroupSection(
            users=[{"name": "appuser", "uid": 1001}],
        ))
        merged = merge_snapshots([s1, s2], include_hosts=False)
        assert merged.users_groups.users[0]["fleet"]["hosts"] == []
        assert merged.users_groups.users[0]["fleet"]["count"] == 2


import pytest


class TestAutoSelectVariants:
    """Auto-selection of most-prevalent variant per path group."""

    @pytest.mark.parametrize("counts,expected_include", [
        # Clear winner: count=3 wins over count=1
        ([3, 1], [True, False]),
        # 2-way tie: both deselected
        ([2, 2], [False, False]),
        # 3-way tie: all deselected
        ([1, 1, 1], [False, False, False]),
        # Single variant: always selected
        ([3], [True]),
        # Mixed [3, 3, 1]: top two tied, all deselected
        ([3, 3, 1], [False, False, False]),
    ])
    def test_auto_select_config_variants(self, counts, expected_include):
        from yoinkc.fleet.merge import merge_snapshots

        total = sum(counts)
        snaps = []
        host_idx = 0
        # Build one snapshot per "host" for each variant count
        for variant_idx, count in enumerate(counts):
            content = f"variant={variant_idx}"
            for _ in range(count):
                f = ConfigFileEntry(
                    path="/etc/test.conf",
                    kind="unowned",
                    content=content,
                )
                snaps.append(_snap(f"host-{host_idx}", config=ConfigSection(files=[f])))
                host_idx += 1

        merged = merge_snapshots(snaps, min_prevalence=0)
        by_content = {f.content: f for f in merged.config.files}

        # Map expected_include indices back to sorted-by-count order
        # variants are in the merged list; sort by fleet.count descending to match spec order
        sorted_variants = sorted(merged.config.files, key=lambda f: f.fleet.count, reverse=True)

        for i, expected in enumerate(expected_include):
            assert sorted_variants[i].include is expected, (
                f"counts={counts}: variant[{i}] (count={sorted_variants[i].fleet.count}) "
                f"expected include={expected}, got {sorted_variants[i].include}"
            )


class TestStorageSuppression:
    def test_storage_suppressed_in_fleet_report(self):
        """Storage section is not merged — remains None in fleet snapshot."""
        from yoinkc.fleet.merge import merge_snapshots
        from yoinkc.schema import StorageSection, FstabEntry
        fstab = FstabEntry(device="/dev/sda1", mount_point="/", fstype="xfs", options="defaults")
        s1 = _snap("host1", storage=StorageSection(fstab_entries=[fstab]))
        s2 = _snap("host2", storage=StorageSection(fstab_entries=[fstab]))
        merged = merge_snapshots([s1, s2], min_prevalence=100)
        assert merged.storage is None


class TestTieFlags:
    """Verify tie/tie_winner flags are set correctly after fleet merge."""

    @pytest.mark.xfail(reason="tie flags not yet set by merge logic")
    def test_tied_variants_get_tie_flags(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-a"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="variant-b"),
        ]))
        merged = merge_snapshots([s1, s2], min_prevalence=0)

        variants = merged.config.files
        assert len(variants) == 2
        assert all(v.tie for v in variants), "All tied variants must have tie=True"
        winners = [v for v in variants if v.tie_winner]
        assert len(winners) == 1, "Exactly one variant should be tie_winner"
        assert winners[0].include is True, "Tie winner must have include=True"
        losers = [v for v in variants if not v.tie_winner]
        assert len(losers) == 1
        assert losers[0].include is False

    def test_clear_winner_no_tie_flags(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="majority"),
        ]))
        s3 = _snap("host-3", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="minority"),
        ]))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=0)

        for v in merged.config.files:
            assert v.tie is False, "Clear winners should not have tie=True"
            assert v.tie_winner is False

    @pytest.mark.xfail(reason="tie flags not yet set by merge logic")
    def test_three_way_tie_one_winner(self):
        from yoinkc.fleet.merge import merge_snapshots

        s1 = _snap("host-1", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="aaa"),
        ]))
        s2 = _snap("host-2", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="bbb"),
        ]))
        s3 = _snap("host-3", config=ConfigSection(files=[
            ConfigFileEntry(path="/etc/test.conf", kind="unowned", content="ccc"),
        ]))
        merged = merge_snapshots([s1, s2, s3], min_prevalence=0)

        variants = merged.config.files
        assert len(variants) == 3
        assert all(v.tie for v in variants)
        winners = [v for v in variants if v.tie_winner]
        assert len(winners) == 1, "3-way tie: exactly one winner"
        assert winners[0].include is True
        losers = [v for v in variants if not v.tie_winner]
        assert len(losers) == 2
        assert all(not v.include for v in losers)
