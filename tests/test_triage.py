"""Tests for triage count computation respecting the `.include` flag."""

import tempfile
from pathlib import Path

import pytest

from inspectah.renderers._triage import compute_triage, compute_triage_detail
from inspectah.schema import (
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    FirewallZone,
    GeneratedTimerUnit,
    InspectionSnapshot,
    NetworkSection,
    PackageEntry,
    PackageState,
    QuadletUnit,
    RpmSection,
    ScheduledTaskSection,
    ServiceSection,
    ServiceStateChange,
    SystemdTimer,
    UserGroupSection,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _snapshot(**kwargs) -> InspectionSnapshot:
    return InspectionSnapshot(meta={}, **kwargs)


def _pkg(name: str, include: bool = True) -> PackageEntry:
    return PackageEntry(name=name, version="1.0", release="1", arch="x86_64", include=include)


def _service_change(unit: str, action: str = "enable", include: bool = True) -> ServiceStateChange:
    return ServiceStateChange(
        unit=unit,
        current_state="enabled",
        default_state="disabled",
        action=action,
        include=include,
    )


def _config_file(path: str, include: bool = True) -> ConfigFileEntry:
    return ConfigFileEntry(path=path, kind=ConfigFileKind.UNOWNED, include=include)


def _firewall_zone(name: str, include: bool = True) -> FirewallZone:
    return FirewallZone(path=f"/etc/firewalld/zones/{name}.xml", name=name, include=include)


def _timer_unit(name: str, include: bool = True) -> GeneratedTimerUnit:
    return GeneratedTimerUnit(name=name, include=include)


def _quadlet(name: str, include: bool = True) -> QuadletUnit:
    return QuadletUnit(path=f"/etc/containers/systemd/{name}.container", name=name, include=include)


def _run_both(snapshot: InspectionSnapshot) -> tuple[dict, list]:
    """Run compute_triage and compute_triage_detail in a temporary directory."""
    with tempfile.TemporaryDirectory() as d:
        output_dir = Path(d)
        triage = compute_triage(snapshot, output_dir)
        detail = compute_triage_detail(snapshot, output_dir)
    return triage, detail


def _detail_count(detail: list, label: str) -> int:
    for item in detail:
        if item["label"] == label:
            return item["count"]
    return 0


# ---------------------------------------------------------------------------
# Packages
# ---------------------------------------------------------------------------

class TestPackageCounts:
    def test_all_included(self):
        snap = _snapshot(rpm=RpmSection(packages_added=[_pkg("a"), _pkg("b"), _pkg("c")]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 3
        assert _detail_count(detail, "Packages added") == 3

    def test_all_excluded(self):
        snap = _snapshot(rpm=RpmSection(packages_added=[_pkg("a", False), _pkg("b", False)]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 0
        assert _detail_count(detail, "Packages added") == 0

    def test_mixed(self):
        snap = _snapshot(rpm=RpmSection(
            packages_added=[_pkg("a"), _pkg("b", False), _pkg("c"), _pkg("d", False)],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Packages added") == 2

    def test_base_image_only_include_filter(self):
        snap = _snapshot(rpm=RpmSection(
            base_image_only=[
                PackageEntry(name="x", version="1", release="1", arch="x86_64",
                             state=PackageState.BASE_IMAGE_ONLY, include=True),
                PackageEntry(name="y", version="1", release="1", arch="x86_64",
                             state=PackageState.BASE_IMAGE_ONLY, include=False),
            ]
        ))
        _, detail = _run_both(snap)
        assert _detail_count(detail, "New from base image") == 1


# ---------------------------------------------------------------------------
# Services
# ---------------------------------------------------------------------------

class TestServiceCounts:
    def test_all_included(self):
        snap = _snapshot(services=ServiceSection(
            state_changes=[
                _service_change("sshd.service", "enable"),
                _service_change("httpd.service", "disable"),
            ]
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Services enabled/disabled") == 2

    def test_excluded_not_counted(self):
        snap = _snapshot(services=ServiceSection(
            state_changes=[
                _service_change("sshd.service", "enable", include=True),
                _service_change("httpd.service", "enable", include=False),
            ]
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1
        assert _detail_count(detail, "Services enabled/disabled") == 1

    def test_unchanged_action_not_counted(self):
        """Actions other than enable/disable should not be counted."""
        snap = _snapshot(services=ServiceSection(
            state_changes=[
                _service_change("sshd.service", "enable"),
                _service_change("noop.service", "unchanged"),
            ]
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1

    def test_all_excluded(self):
        snap = _snapshot(services=ServiceSection(
            state_changes=[
                _service_change("sshd.service", "enable", include=False),
                _service_change("httpd.service", "disable", include=False),
            ]
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 0
        assert _detail_count(detail, "Services enabled/disabled") == 0


# ---------------------------------------------------------------------------
# Config files
# ---------------------------------------------------------------------------

class TestConfigFileCounts:
    def test_all_included(self):
        snap = _snapshot(config=ConfigSection(files=[
            _config_file("/etc/hosts"),
            _config_file("/etc/myapp.conf"),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Config files") == 2

    def test_excluded_not_counted(self):
        snap = _snapshot(config=ConfigSection(files=[
            _config_file("/etc/hosts", include=True),
            _config_file("/etc/myapp.conf", include=False),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1
        assert _detail_count(detail, "Config files") == 1

    def test_quadlet_path_excluded_regardless_of_include(self):
        """Files under the quadlet prefix are never counted by triage."""
        snap = _snapshot(config=ConfigSection(files=[
            _config_file("/etc/containers/systemd/myapp.container", include=True),
            _config_file("/etc/hosts", include=True),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1

    def test_quadlet_path_with_include_false(self):
        snap = _snapshot(config=ConfigSection(files=[
            _config_file("/etc/containers/systemd/myapp.container", include=False),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 0


# ---------------------------------------------------------------------------
# Firewall zones
# ---------------------------------------------------------------------------

class TestFirewallZoneCounts:
    def test_all_included(self):
        snap = _snapshot(network=NetworkSection(firewall_zones=[
            _firewall_zone("public"), _firewall_zone("internal"),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Firewall zones") == 2

    def test_mixed(self):
        snap = _snapshot(network=NetworkSection(firewall_zones=[
            _firewall_zone("public", include=True),
            _firewall_zone("internal", include=False),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1
        assert _detail_count(detail, "Firewall zones") == 1

    def test_all_excluded(self):
        snap = _snapshot(network=NetworkSection(firewall_zones=[
            _firewall_zone("public", include=False),
        ]))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 0


# ---------------------------------------------------------------------------
# Generated timer units
# ---------------------------------------------------------------------------

class TestGeneratedTimerCounts:
    def test_all_included(self):
        snap = _snapshot(scheduled_tasks=ScheduledTaskSection(
            generated_timer_units=[_timer_unit("backup"), _timer_unit("cleanup")],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Cron-to-timer conversions") == 2

    def test_mixed(self):
        snap = _snapshot(scheduled_tasks=ScheduledTaskSection(
            generated_timer_units=[
                _timer_unit("backup", include=True),
                _timer_unit("cleanup", include=False),
            ],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1
        assert _detail_count(detail, "Cron-to-timer conversions") == 1


# ---------------------------------------------------------------------------
# Quadlet units
# ---------------------------------------------------------------------------

class TestQuadletCounts:
    def test_all_included(self):
        snap = _snapshot(containers=ContainerSection(
            quadlet_units=[_quadlet("db"), _quadlet("cache")],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 2
        assert _detail_count(detail, "Quadlet units") == 2

    def test_mixed(self):
        snap = _snapshot(containers=ContainerSection(
            quadlet_units=[_quadlet("db", include=True), _quadlet("cache", include=False)],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 1
        assert _detail_count(detail, "Quadlet units") == 1


# ---------------------------------------------------------------------------
# Types without include field (users/groups, systemd timers)
# ---------------------------------------------------------------------------

class TestNoIncludeField:
    def test_users_and_groups_always_counted(self):
        snap = _snapshot(users_groups=UserGroupSection(
            users=[{"name": "alice"}, {"name": "bob"}],
            groups=[{"name": "wheel"}],
        ))
        triage, detail = _run_both(snap)
        assert triage["automatic"] == 3
        assert _detail_count(detail, "Users/groups") == 3

    def test_systemd_timers_local_always_counted(self):
        snap = _snapshot(scheduled_tasks=ScheduledTaskSection(
            systemd_timers=[
                SystemdTimer(name="backup.timer", source="local"),
                SystemdTimer(name="vendor.timer", source="vendor"),
            ]
        ))
        triage, detail = _run_both(snap)
        # Only local timers count; no include field — cannot be excluded
        assert triage["automatic"] == 1


# ---------------------------------------------------------------------------
# Consistency: compute_triage and compute_triage_detail agree
# ---------------------------------------------------------------------------

class TestTriageConsistency:
    """Verify compute_triage() and compute_triage_detail() produce the same totals."""

    def test_automatic_totals_agree(self):
        snap = _snapshot(
            rpm=RpmSection(
                packages_added=[_pkg("a"), _pkg("b", False)],
                base_image_only=[
                    PackageEntry(name="x", version="1", release="1", arch="x86_64",
                                 state=PackageState.BASE_IMAGE_ONLY, include=True),
                    PackageEntry(name="y", version="1", release="1", arch="x86_64",
                                 state=PackageState.BASE_IMAGE_ONLY, include=False),
                ],
            ),
            services=ServiceSection(state_changes=[
                _service_change("sshd.service", "enable"),
                _service_change("httpd.service", "disable", include=False),
            ]),
            config=ConfigSection(files=[_config_file("/etc/hosts"), _config_file("/etc/foo", False)]),
            network=NetworkSection(firewall_zones=[_firewall_zone("public")]),
            scheduled_tasks=ScheduledTaskSection(
                generated_timer_units=[_timer_unit("backup"), _timer_unit("cleanup", False)],
                systemd_timers=[SystemdTimer(name="local.timer", source="local")],
            ),
            users_groups=UserGroupSection(users=[{"name": "alice"}]),
            containers=ContainerSection(quadlet_units=[_quadlet("db"), _quadlet("cache", False)]),
        )
        triage, detail = _run_both(snap)
        detail_automatic = sum(item["count"] for item in detail if item["status"] == "automatic")
        assert triage["automatic"] == detail_automatic
