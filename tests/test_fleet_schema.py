"""Tests for fleet-related schema additions."""

import json
from yoinkc.schema import FleetPrevalence, FleetMeta


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
