"""Tests for fleet-related schema additions."""

import json
from inspectah.schema import FleetPrevalence, FleetMeta


class TestFleetPrevalence:
    def test_basic_construction(self):
        fp = FleetPrevalence(count=98, total=100)
        assert fp.count == 98
        assert fp.total == 100
        assert fp.hosts == []

    def test_with_hosts(self):
        fp = FleetPrevalence(count=2, total=100, hosts=["web-01", "web-02"])
        assert fp.hosts == ["web-01", "web-02"]

    def test_serialization_roundtrip(self):
        fp = FleetPrevalence(count=50, total=100, hosts=["a", "b"])
        data = json.loads(fp.model_dump_json())
        fp2 = FleetPrevalence(**data)
        assert fp2.count == fp.count
        assert fp2.hosts == fp.hosts


class TestFleetMeta:
    def test_basic_construction(self):
        fm = FleetMeta(
            source_hosts=["web-01", "web-02"],
            total_hosts=2,
            min_prevalence=90,
        )
        assert fm.total_hosts == 2
        assert fm.min_prevalence == 90

    def test_serialization_roundtrip(self):
        fm = FleetMeta(
            source_hosts=["a", "b", "c"],
            total_hosts=3,
            min_prevalence=100,
        )
        data = json.loads(fm.model_dump_json())
        fm2 = FleetMeta(**data)
        assert fm2.source_hosts == ["a", "b", "c"]


from inspectah.schema import (
    PackageEntry, RepoFile, ConfigFileEntry, ServiceStateChange,
    SystemdDropIn, FirewallZone, GeneratedTimerUnit, QuadletUnit,
    ComposeFile, CronJob,
)


class TestFleetFieldOnModels:
    """Every item model that supports include should accept an optional fleet field."""

    def test_package_entry_fleet_default_none(self):
        p = PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64")
        assert p.fleet is None

    def test_package_entry_with_fleet(self):
        fp = FleetPrevalence(count=98, total=100)
        p = PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64", fleet=fp)
        assert p.fleet.count == 98

    def test_config_file_entry_fleet(self):
        fp = FleetPrevalence(count=50, total=100)
        c = ConfigFileEntry(path="/etc/httpd/conf/httpd.conf", kind="unowned", fleet=fp)
        assert c.fleet.count == 50

    def test_all_models_accept_fleet_none(self):
        """Verify fleet field exists and defaults to None on all target models."""
        models_with_defaults = [
            PackageEntry(name="x", version="1", release="1", arch="x86_64"),
            RepoFile(path="/etc/yum.repos.d/test.repo"),
            ConfigFileEntry(path="/etc/test.conf", kind="unowned"),
            ServiceStateChange(unit="test.service", current_state="enabled",
                             default_state="disabled", action="enable"),
            SystemdDropIn(unit="test.service", path="etc/systemd/system/test.service.d/override.conf"),
            FirewallZone(path="/etc/firewalld/zones/public.xml", name="public"),
            GeneratedTimerUnit(name="test-timer"),
            QuadletUnit(path="/etc/containers/systemd/test.container", name="test"),
            ComposeFile(path="/opt/app/docker-compose.yml"),
            CronJob(path="/etc/cron.d/test", source="cron.d"),
        ]
        for model in models_with_defaults:
            assert model.fleet is None, f"{type(model).__name__}.fleet should default to None"

    def test_fleet_survives_json_roundtrip(self):
        """Fleet data should survive serialization and deserialization."""
        import json
        fp = FleetPrevalence(count=5, total=10, hosts=["h1", "h2"])
        p = PackageEntry(name="vim", version="9", release="1", arch="x86_64", fleet=fp)
        data = json.loads(p.model_dump_json())
        p2 = PackageEntry(**data)
        assert p2.fleet is not None
        assert p2.fleet.count == 5
        assert p2.fleet.hosts == ["h1", "h2"]
