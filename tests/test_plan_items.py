"""
Tests for the 12 plan items implemented during codebase-vs-design review.

Focuses on internal logic, parsing, and branching that the existing integration
and renderer tests don't exercise.  Renderer output-string tests are omitted
when the existing suite (test_renderers, test_integration) already covers them
indirectly — the cross-cutting end-to-end test at the bottom serves as the
smoke test that all 12 features render without crashing.
"""

import tempfile
from pathlib import Path

import pytest
from jinja2 import Environment

from yoinkc.schema import (
    ComposeFile,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    CronJob,
    FstabEntry,
    FirewallZone,
    GeneratedTimerUnit,
    InspectionSnapshot,
    KernelBootSection,
    KernelModule,
    QuadletUnit,
    SysctlOverride,
    NMConnection,
    NetworkSection,
    NonRpmItem,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    ProxyEntry,
    RepoFile,
    RpmSection,
    ScheduledTaskSection,
    SelinuxSection,
    ServiceSection,
    ServiceStateChange,
    StaticRouteFile,
    StorageSection,
    UserGroupSection,
)
from yoinkc.renderers.containerfile import render as render_containerfile
from yoinkc.renderers.audit_report import render as render_audit
from yoinkc.renderers.html_report import render as render_html_report

from conftest import _env


# ---------------------------------------------------------------------------
# 1. Service baseline from base image presets
# ---------------------------------------------------------------------------

class TestServiceBaselinePresets:

    def test_parse_preset_lines(self):
        from yoinkc.inspectors.service import _parse_preset_lines
        lines = [
            "enable sshd.service",
            "enable chronyd.service",
            "disable kdump.service",
            "disable *",
        ]
        enabled, disabled, has_disable_all, glob_rules = _parse_preset_lines(lines)
        assert "sshd.service" in enabled
        assert "kdump.service" in disabled
        assert has_disable_all is True
        assert ("disable", "*") in glob_rules

    def test_base_image_text_preferred_over_host(self):
        from yoinkc.inspectors.service import _parse_preset_files
        enabled, disabled, _, _glob = _parse_preset_files(
            Path("/nonexistent"),
            base_image_preset_text="enable sshd.service\ndisable *\n",
        )
        assert "sshd.service" in enabled

    def test_run_with_base_image_presets(self):
        """Service enabled on host but not in base presets → action=enable."""
        from yoinkc.inspectors.service import run as run_service
        from yoinkc.executor import RunResult

        def exec_(cmd, cwd=None):
            if "systemctl" in cmd:
                return RunResult(
                    stdout="sshd.service enabled\ncustom.service enabled\n",
                    stderr="", returncode=0,
                )
            return RunResult(stdout="", stderr="", returncode=1)

        result = run_service(
            Path("/nonexistent"), executor=exec_,
            base_image_preset_text="enable sshd.service\ndisable *\n",
        )
        actions = {sc.unit: sc.action for sc in result.state_changes}
        assert actions["sshd.service"] == "unchanged"
        assert actions["custom.service"] == "enable"


# ---------------------------------------------------------------------------
# 2a. Cron-to-OnCalendar conversion
# ---------------------------------------------------------------------------

class TestCronToOnCalendar:

    def _convert(self, expr):
        from yoinkc.inspectors.scheduled_tasks import _cron_to_on_calendar
        return _cron_to_on_calendar(expr)

    def test_simple_min_hour(self):
        cal, ok = self._convert("0 2 * * *")
        assert cal == "*-*-* 02:00:00"
        assert ok is True

    def test_specific_min_hour(self):
        cal, ok = self._convert("30 14 * * *")
        assert cal == "*-*-* 14:30:00"
        assert ok is True

    def test_step_minute(self):
        cal, ok = self._convert("*/15 * * * *")
        assert "*/15" in cal
        assert ok is True

    def test_step_hour(self):
        cal, ok = self._convert("0 */2 * * *")
        assert "00/2" in cal or "*/2" in cal
        assert ok is True

    def test_day_of_month(self):
        cal, ok = self._convert("0 3 1 * *")
        assert "*-*-01" in cal or "*-*-1" in cal
        assert "03:00" in cal
        assert ok is True

    def test_specific_month(self):
        cal, ok = self._convert("0 0 1 6 *")
        assert "-6-" in cal or "-06-" in cal
        assert ok is True

    def test_day_of_week_numeric(self):
        cal, ok = self._convert("0 5 * * 1")
        assert "Mon" in cal
        assert ok is True

    def test_day_of_week_star(self):
        cal, ok = self._convert("0 5 * * *")
        assert "Mon" not in cal
        assert ok is True

    def test_range(self):
        cal, ok = self._convert("0 9 * * 1-5")
        assert "Mon..Fri" in cal
        assert ok is True

    def test_list(self):
        cal, ok = self._convert("0 0 1,15 * *")
        assert "1,15" in cal
        assert ok is True

    def test_at_daily(self):
        cal, ok = self._convert("@daily")
        assert cal == "*-*-* 00:00:00"
        assert ok is True

    def test_at_hourly(self):
        cal, ok = self._convert("@hourly")
        assert cal == "*-*-* *:00:00"
        assert ok is True

    def test_at_weekly(self):
        cal, ok = self._convert("@weekly")
        assert "Mon" in cal
        assert ok is True

    def test_at_monthly(self):
        cal, ok = self._convert("@monthly")
        assert "*-*-01" in cal
        assert ok is True

    def test_at_yearly(self):
        cal, ok = self._convert("@yearly")
        assert "*-01-01" in cal
        assert ok is True

    def test_at_reboot_not_converted(self):
        cal, ok = self._convert("@reboot")
        assert ok is False

    def test_incomplete_expression(self):
        cal, ok = self._convert("*/5")
        assert ok is False

    def test_all_stars(self):
        """Every minute."""
        cal, ok = self._convert("* * * * *")
        assert ok is True
        assert "*:*" in cal or "*-*-* *:*" in cal


# ---------------------------------------------------------------------------
# 2b. Cron command extraction into ExecStart
# ---------------------------------------------------------------------------

class TestCronCommandExtraction:

    def test_command_in_exec_start(self):
        from yoinkc.inspectors.scheduled_tasks import _make_timer_service
        _, service = _make_timer_service(
            "cron-backup", "0 2 * * *", "etc/cron.d/backup",
            command="/usr/local/bin/backup.sh --full",
        )
        assert "ExecStart=/usr/local/bin/backup.sh --full" in service
        assert "FIXME" not in service

    def test_fallback_when_no_command(self):
        from yoinkc.inspectors.scheduled_tasks import _make_timer_service
        _, service = _make_timer_service("x", "0 0 * * *", "etc/cron.d/x")
        assert "ExecStart=/bin/true" in service
        assert "FIXME" in service

    def test_system_crontab_skips_user_field(self, tmp_path):
        from yoinkc.inspectors.scheduled_tasks import _scan_cron_file
        cron_d = tmp_path / "etc/cron.d"
        cron_d.mkdir(parents=True)
        (cron_d / "logrotate").write_text(
            "0 4 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n"
        )
        section = ScheduledTaskSection()
        _scan_cron_file(section, tmp_path, cron_d / "logrotate", "cron.d")
        assert section.generated_timer_units[0].command == "/usr/sbin/logrotate /etc/logrotate.conf"

    def test_user_crontab_no_user_field(self, tmp_path):
        spool = tmp_path / "var/spool/cron"
        spool.mkdir(parents=True)
        (spool / "mark").write_text("30 1 * * * /home/mark/cleanup.sh\n")
        from yoinkc.inspectors.scheduled_tasks import _scan_cron_file
        section = ScheduledTaskSection()
        _scan_cron_file(section, tmp_path, spool / "mark", "spool/cron (mark)")
        assert section.generated_timer_units[0].command == "/home/mark/cleanup.sh"


# ---------------------------------------------------------------------------
# 2c. RPM-owned cron file filtering
# ---------------------------------------------------------------------------

class TestRpmOwnedCronFiltering:

    def test_rpm_owned_cron_d_file_not_converted(self, tmp_path):
        """RPM-owned cron.d files are recorded with rpm_owned=True but no timer is generated."""
        from yoinkc.inspectors.scheduled_tasks import _scan_cron_file
        cron_d = tmp_path / "etc/cron.d"
        cron_d.mkdir(parents=True)
        (cron_d / "logrotate").write_text("0 4 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n")

        section = ScheduledTaskSection()
        rpm_owned = {"/etc/cron.d/logrotate"}
        _scan_cron_file(section, tmp_path, cron_d / "logrotate", "cron.d", rpm_owned_paths=rpm_owned)

        assert len(section.cron_jobs) == 1
        assert section.cron_jobs[0].rpm_owned is True
        assert len(section.generated_timer_units) == 0

    def test_unowned_cron_d_file_generates_timer(self, tmp_path):
        """Non-RPM-owned cron.d files are recorded with rpm_owned=False and a timer is generated."""
        from yoinkc.inspectors.scheduled_tasks import _scan_cron_file
        cron_d = tmp_path / "etc/cron.d"
        cron_d.mkdir(parents=True)
        (cron_d / "my-backup").write_text("0 2 * * * root /usr/local/bin/backup.sh\n")

        section = ScheduledTaskSection()
        rpm_owned = {"/etc/cron.d/logrotate"}  # does not include my-backup
        _scan_cron_file(section, tmp_path, cron_d / "my-backup", "cron.d", rpm_owned_paths=rpm_owned)

        assert len(section.cron_jobs) == 1
        assert section.cron_jobs[0].rpm_owned is False
        assert len(section.generated_timer_units) == 1

    def test_user_crontab_always_generates_timer(self, tmp_path):
        """User spool crontabs always generate timers — the rpm_owned_paths check is not applied."""
        from yoinkc.inspectors.scheduled_tasks import _scan_cron_file
        spool = tmp_path / "var/spool/cron"
        spool.mkdir(parents=True)
        (spool / "alice").write_text("30 1 * * * /home/alice/backup.sh\n")

        section = ScheduledTaskSection()
        # No rpm_owned_paths passed (mimics the run() spool scanning path)
        _scan_cron_file(section, tmp_path, spool / "alice", "spool/cron (alice)")

        assert len(section.generated_timer_units) == 1
        assert section.cron_jobs[0].rpm_owned is False

    def test_run_mixes_owned_and_unowned_cron_d(self, tmp_path):
        """run() with rpm_owned_paths: owned file in cron_jobs but no timer; unowned gets a timer."""
        from yoinkc.inspectors.scheduled_tasks import run as run_scheduled_tasks
        cron_d = tmp_path / "etc/cron.d"
        cron_d.mkdir(parents=True)
        (cron_d / "logrotate").write_text("0 4 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n")
        (cron_d / "my-backup").write_text("0 2 * * * root /usr/local/bin/backup.sh\n")

        rpm_owned = {"/etc/cron.d/logrotate"}
        section = run_scheduled_tasks(tmp_path, executor=None, rpm_owned_paths=rpm_owned)

        assert len(section.cron_jobs) == 2

        logrotate_job = next(j for j in section.cron_jobs if j.path.endswith("logrotate"))
        backup_job = next(j for j in section.cron_jobs if j.path.endswith("my-backup"))

        assert logrotate_job.rpm_owned is True
        assert backup_job.rpm_owned is False

        assert len(section.generated_timer_units) == 1
        assert section.generated_timer_units[0].source_path.endswith("my-backup")

    def test_run_cron_period_rpm_owned_skips_timer(self, tmp_path):
        """RPM-owned cron.daily scripts are recorded but no timer unit is generated."""
        from yoinkc.inspectors.scheduled_tasks import run as run_scheduled_tasks
        cron_daily = tmp_path / "etc/cron.daily"
        cron_daily.mkdir(parents=True)
        (cron_daily / "man-db.cron").write_text("#!/bin/sh\nmandb --quiet\n")
        (cron_daily / "my-report").write_text("#!/bin/sh\n/usr/local/bin/report.sh\n")

        rpm_owned = {"/etc/cron.daily/man-db.cron"}
        section = run_scheduled_tasks(tmp_path, executor=None, rpm_owned_paths=rpm_owned)

        assert len(section.cron_jobs) == 2

        man_db_job = next(j for j in section.cron_jobs if "man-db" in j.path)
        report_job = next(j for j in section.cron_jobs if "my-report" in j.path)

        assert man_db_job.rpm_owned is True
        assert report_job.rpm_owned is False

        assert len(section.generated_timer_units) == 1
        assert "my-report" in section.generated_timer_units[0].source_path

    def test_run_no_rpm_owned_set_converts_all(self, tmp_path):
        """When rpm_owned_paths is None (no executor), all cron files generate timers."""
        from yoinkc.inspectors.scheduled_tasks import run as run_scheduled_tasks
        cron_d = tmp_path / "etc/cron.d"
        cron_d.mkdir(parents=True)
        (cron_d / "logrotate").write_text("0 4 * * * root /usr/sbin/logrotate /etc/logrotate.conf\n")

        section = run_scheduled_tasks(tmp_path, executor=None, rpm_owned_paths=None)

        assert len(section.cron_jobs) == 1
        assert section.cron_jobs[0].rpm_owned is False
        assert len(section.generated_timer_units) == 1


# ---------------------------------------------------------------------------
# 3. Multi-stage Containerfile for pip C extensions
# ---------------------------------------------------------------------------

class TestMultiStageContainerfile:

    def _pip_snapshot(self, c_ext=True):
        items = [
            NonRpmItem(name="cryptography", version="41.0.0", method="pip dist-info",
                       has_c_extensions=c_ext, confidence="high",
                       path="usr/lib/python3.9/site-packages/cryptography-41.0.0.dist-info"),
            NonRpmItem(name="requests", version="2.32.5", method="pip dist-info",
                       confidence="high",
                       path="usr/lib/python3.9/site-packages/requests-2.32.5.dist-info"),
        ]
        return InspectionSnapshot(
            meta={}, os_release=OsRelease(name="CentOS Stream", version_id="9", id="centos"),
            rpm=RpmSection(base_image="quay.io/centos-bootc/centos-bootc:stream9"),
            non_rpm_software=NonRpmSoftwareSection(items=items),
        )

    def test_builder_stage_when_c_extensions(self):
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(self._pip_snapshot(c_ext=True), _env(), Path(tmp))
            content = (Path(tmp) / "Containerfile").read_text()
        assert "AS builder" in content
        assert "COPY --from=builder" in content
        assert "pip install cryptography==41.0.0" in content

    def test_no_builder_stage_without_c_extensions(self):
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(self._pip_snapshot(c_ext=False), _env(), Path(tmp))
            content = (Path(tmp) / "Containerfile").read_text()
        assert "AS builder" not in content
        assert "COPY --from=builder" not in content


# ---------------------------------------------------------------------------
# 4. Config diff download-from-repos fallback
# ---------------------------------------------------------------------------

class TestConfigDiffFallback:

    def test_download_rpm_from_repo_success(self):
        from yoinkc.inspectors.config import _download_rpm_from_repo
        from yoinkc.executor import RunResult

        def exec_(cmd, cwd=None):
            cmd_str = " ".join(cmd)
            if "dnf" in cmd_str and "download" in cmd_str:
                # Write a fake RPM into whatever --destdir was passed
                for i, part in enumerate(cmd):
                    if part == "--destdir" and i + 1 < len(cmd):
                        dest = Path(cmd[i + 1])
                        dest.mkdir(parents=True, exist_ok=True)
                        (dest / "httpd-2.4.51-7.el9.x86_64.rpm").write_text("fake")
                        break
                return RunResult(stdout="", stderr="", returncode=0)
            if "rpm2cpio" in cmd_str:
                return RunResult(stdout="ServerRoot /etc/httpd", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=1)

        result = _download_rpm_from_repo(exec_, Path("/host"), "httpd", "etc/httpd/conf/httpd.conf")
        assert result == "ServerRoot /etc/httpd"

    def test_extract_uses_dot_slash_prefix(self):
        from yoinkc.inspectors.config import _extract_file_from_rpm
        from yoinkc.executor import RunResult

        captured = []
        def exec_(cmd, cwd=None):
            captured.append(" ".join(cmd))
            return RunResult(stdout="content", stderr="", returncode=0)

        _extract_file_from_rpm(exec_, Path("/a.rpm"), "etc/httpd/conf/httpd.conf")
        assert "./etc/httpd/conf/httpd.conf" in captured[0]


# ---------------------------------------------------------------------------
# 5. HTML syntax-highlighted diffs
# ---------------------------------------------------------------------------

def test_html_diff_spans():
    snapshot = InspectionSnapshot(
        meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
        config=ConfigSection(files=[ConfigFileEntry(
            path="/etc/test.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED,
            content="x", diff_against_rpm="--- rpm\n+++ current\n@@ -1 +1 @@\n-old\n+new\n",
            rpm_va_flags="S.5....T.",
        )]),
    )
    with tempfile.TemporaryDirectory() as tmp:
        render_html_report(snapshot, _env(), Path(tmp))
        html = (Path(tmp) / "report.html").read_text()
    for cls in ("diff-view", "diff-hdr", "diff-hunk", "diff-add", "diff-del"):
        assert f'class="{cls}"' in html


# ---------------------------------------------------------------------------
# 6. Storage migration recommendation mapping
# ---------------------------------------------------------------------------

def test_storage_recommendation_mapping():
    from yoinkc.renderers.audit_report import _storage_recommendation as rec
    assert "image-embedded" in rec("/", "xfs", "/dev/sda1")
    assert "network mount" in rec("/data", "nfs", "server:/share")
    assert "swap" in rec("none", "swap", "/dev/sda3")
    assert "tmpfs" in rec("/tmp", "tmpfs", "tmpfs")
    assert "database" in rec("/var/lib/mysql", "xfs", "/dev/sdb1")
    assert "container" in rec("/var/lib/containers", "xfs", "/dev/sdb2")
    assert "log" in rec("/var/log", "xfs", "/dev/sdc1")
    assert "user home" in rec("/home", "xfs", "/dev/sdd1")
    assert "served content" in rec("/srv", "xfs", "/dev/sde1")
    assert "removable" in rec("/mnt/usb", "vfat", "/dev/sdf1")


# ---------------------------------------------------------------------------
# 9. User append files written to config tree
# ---------------------------------------------------------------------------

class TestUserStrategies:

    def test_sysusers_writes_conf(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "appuser", "uid": 1001, "gid": 1001,
                        "home": "/opt/myapp", "shell": "/sbin/nologin",
                        "classification": "service", "strategy": "sysusers"}],
                groups=[{"name": "appuser", "gid": 1001, "members": [], "strategy": "sysusers"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            sysusers_path = Path(tmp) / "config/usr/lib/sysusers.d/yoinkc-users.conf"
            assert sysusers_path.exists()
            content = sysusers_path.read_text()
            assert "u appuser 1001" in content
            assert "g appuser 1001" in content
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "systemd-sysusers" in cf
            assert "COPY config/usr/lib/sysusers.d" in cf

    def test_useradd_renders_commands(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "deploy", "uid": 1003, "gid": 1003,
                        "home": "/var/lib/deploy", "shell": "/bin/bash",
                        "classification": "ambiguous", "strategy": "useradd"}],
                groups=[{"name": "deploy", "gid": 1003, "members": [], "strategy": "useradd"}],
                shadow_entries=["deploy:$6$saltsalt$hashhashhash:19700:0:99999:7:::"],
                sudoers_rules=["deploy ALL=(ALL) NOPASSWD: /usr/bin/systemctl"],
                ssh_authorized_keys_refs=[{"user": "deploy", "path": "/var/lib/deploy/.ssh/authorized_keys"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "RUN groupadd -g 1003 deploy" in cf
            assert "RUN useradd -m -u 1003" in cf
            assert "chpasswd -e" in cf
            assert "FIXME: SSH keys for 'deploy'" in cf
            assert "sudoers" in cf.lower()

    def test_useradd_no_ssh_keys(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "deploy", "uid": 1003, "gid": 1003,
                        "home": "/var/lib/deploy", "shell": "/bin/bash",
                        "classification": "ambiguous", "strategy": "useradd"}],
                ssh_authorized_keys_refs=[{"user": "deploy", "path": "/var/lib/deploy/.ssh/authorized_keys"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "authorized_keys" not in cf or "FIXME" in cf

    def test_kickstart_defers_user(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "mark", "uid": 1000, "gid": 1000,
                        "home": "/home/mark", "shell": "/bin/bash",
                        "classification": "human", "strategy": "kickstart"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "FIXME: human user 'mark' deferred" in cf
            assert "kickstart" in cf.lower()

    def test_kickstart_adds_user_directive(self):
        from yoinkc.renderers.kickstart import render as render_kickstart
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "mark", "uid": 1000, "gid": 1000,
                        "home": "/home/mark", "shell": "/bin/bash",
                        "classification": "human", "strategy": "kickstart"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_kickstart(snapshot, _env(), Path(tmp))
            ks = (Path(tmp) / "kickstart-suggestion.ks").read_text()
            assert "user --name=mark" in ks
            assert "--uid=1000" in ks

    def test_blueprint_generates_toml(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "admin", "uid": 1000, "gid": 1000,
                        "home": "/home/admin", "shell": "/bin/bash",
                        "classification": "human", "strategy": "blueprint"}],
                groups=[{"name": "admin", "gid": 1000, "members": [], "strategy": "blueprint"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            toml_path = Path(tmp) / "yoinkc-users.toml"
            assert toml_path.exists()
            content = toml_path.read_text()
            assert "[[customizations.user]]" in content
            assert 'name = "admin"' in content
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "blueprint" in cf.lower()

    def test_no_blueprint_toml_without_blueprint_users(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "appuser", "uid": 1001, "gid": 1001,
                        "home": "/opt/myapp", "shell": "/sbin/nologin",
                        "classification": "service", "strategy": "sysusers"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            assert not (Path(tmp) / "yoinkc-users.toml").exists()

    def test_mixed_strategies(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[
                    {"name": "redis", "uid": 1001, "gid": 1001,
                     "home": "/var/lib/redis", "shell": "/sbin/nologin",
                     "classification": "service", "strategy": "sysusers"},
                    {"name": "appuser", "uid": 1002, "gid": 1002,
                     "home": "/var/lib/myapp", "shell": "/bin/bash",
                     "classification": "ambiguous", "strategy": "useradd"},
                    {"name": "mark", "uid": 1000, "gid": 1000,
                     "home": "/home/mark", "shell": "/bin/bash",
                     "classification": "human", "strategy": "kickstart"},
                ],
                groups=[
                    {"name": "redis", "gid": 1001, "members": [], "strategy": "sysusers"},
                    {"name": "appuser", "gid": 1002, "members": [], "strategy": "useradd"},
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
            assert "systemd-sysusers" in cf
            assert "RUN useradd" in cf
            assert "FIXME: human user 'mark' deferred" in cf

    def test_user_strategy_override_all_sysusers(self):
        from yoinkc.inspectors.users_groups import run as run_ug
        import tempfile as _tf
        host_root = Path(__file__).parent / "fixtures" / "host_etc"
        section = run_ug(host_root, None, user_strategy_override="sysusers")
        for u in section.users:
            assert u["strategy"] == "sysusers", f"{u['name']} should be sysusers"
        for g in section.groups:
            assert g["strategy"] == "sysusers", f"{g['name']} should be sysusers"

    def test_user_strategy_override_blueprint_generates_toml(self):
        snapshot = InspectionSnapshot(
            meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "mark", "uid": 1000, "gid": 1000,
                        "home": "/home/mark", "shell": "/bin/bash",
                        "classification": "human", "strategy": "blueprint"}],
                groups=[{"name": "mark", "gid": 1000, "members": [], "strategy": "blueprint"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            assert (Path(tmp) / "yoinkc-users.toml").exists()
            toml = (Path(tmp) / "yoinkc-users.toml").read_text()
            assert "[[customizations.user]]" in toml
            assert 'name = "mark"' in toml

    def test_audit_report_strategy_table(self):
        from yoinkc.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6"),
            users_groups=UserGroupSection(
                users=[
                    {"name": "appuser", "uid": 1001, "gid": 1001,
                     "home": "/opt/myapp", "shell": "/sbin/nologin",
                     "classification": "service", "strategy": "sysusers"},
                    {"name": "mark", "uid": 1000, "gid": 1000,
                     "home": "/home/mark", "shell": "/bin/bash",
                     "classification": "human", "strategy": "kickstart"},
                ],
                sudoers_rules=["mark ALL=(ALL) NOPASSWD: ALL"],
                ssh_authorized_keys_refs=[{"user": "mark", "path": "/home/mark/.ssh/authorized_keys"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
            assert "User Migration Strategy" in report
            assert "| appuser" in report
            assert "sysusers" in report
            assert "kickstart" in report
            assert "has sudo" in report

    def test_readme_user_strategies_section(self):
        from yoinkc.renderers.readme import render as render_readme
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(base_image="registry.redhat.io/rhel9/rhel-bootc:9.6"),
            users_groups=UserGroupSection(
                users=[{"name": "mark", "uid": 1000, "gid": 1000,
                        "home": "/home/mark", "shell": "/bin/bash",
                        "classification": "human", "strategy": "kickstart"}],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            # Need a Containerfile for _extract_fixmes
            (Path(tmp) / "Containerfile").write_text("FROM base\n")
            render_readme(snapshot, _env(), Path(tmp))
            readme = (Path(tmp) / "README.md").read_text()
            assert "User Creation Strategies" in readme
            assert "sysusers" in readme
            assert "bootc" in readme.lower()

    def test_cli_user_strategy_invalid(self):
        from yoinkc.cli import parse_args
        import sys
        try:
            parse_args(["--user-strategy", "invalid"])
            assert False, "Should have raised SystemExit"
        except SystemExit:
            pass


# ---------------------------------------------------------------------------
# 11. Deep binary scan expanded patterns
# ---------------------------------------------------------------------------

class TestDeepVersionPatterns:

    def _match(self, data: bytes, expected: bytes):
        from yoinkc.inspectors.non_rpm_software import DEEP_VERSION_PATTERNS
        for pat in DEEP_VERSION_PATTERNS:
            m = pat.search(data)
            if m and m.group(1) == expected:
                return
        pytest.fail(f"No pattern matched {data!r} → {expected!r}")

    def test_go(self):
        self._match(b"go1.21.5 linux/amd64", b"1.21.5")

    def test_rust(self):
        self._match(b"rustc 1.75.0 (82e1608df 2023-12-21)", b"1.75.0")

    def test_openssl(self):
        self._match(b"OpenSSL 3.0.12 24 Oct 2023", b"3.0.12")

    def test_deep_is_superset_of_base(self):
        from yoinkc.inspectors.non_rpm_software import VERSION_PATTERNS, DEEP_VERSION_PATTERNS
        for pat in VERSION_PATTERNS:
            assert pat in DEEP_VERSION_PATTERNS


# ---------------------------------------------------------------------------
# 12. CLI: removed flags stay dead
# ---------------------------------------------------------------------------

def test_profile_flag_rejected():
    from yoinkc.cli import parse_args
    with pytest.raises(SystemExit):
        parse_args(["--profile", "server"])


def test_comps_file_flag_rejected():
    from yoinkc.cli import parse_args
    with pytest.raises(SystemExit):
        parse_args(["--comps-file", "/tmp/comps.xml"])


# ---------------------------------------------------------------------------
# Cross-cutting: all 12 features in one render pass (smoke test)
# ---------------------------------------------------------------------------

def test_all_features_render_together():
    """Exercises every new code path in a single rich snapshot."""
    snapshot = InspectionSnapshot(
        meta={"hostname": "test-host"},
        os_release=OsRelease(name="CentOS Stream", version_id="9", id="centos",
                             pretty_name="CentOS Stream 9"),
        rpm=RpmSection(
            packages_added=[PackageEntry(name="httpd", version="2.4", release="1.el9", arch="x86_64")],
            base_image="quay.io/centos-bootc/centos-bootc:stream9",
            baseline_package_names=["bash", "coreutils"],
        ),
        config=ConfigSection(files=[ConfigFileEntry(
            path="/etc/httpd/conf/httpd.conf", kind=ConfigFileKind.RPM_OWNED_MODIFIED,
            content="ServerRoot /etc/httpd\n", rpm_va_flags="S.5....T.", package="httpd",
            diff_against_rpm="--- rpm\n+++ current\n@@ -1 +1 @@\n-Listen 80\n+Listen 8080\n",
        )]),
        services=ServiceSection(
            state_changes=[ServiceStateChange(
                unit="httpd.service", current_state="enabled",
                default_state="disabled", action="enable",
            )],
            enabled_units=["httpd.service"],
        ),
        network=NetworkSection(
            connections=[NMConnection(name="eth0", method="static", path="etc/NM/eth0.nmconnection")],
            firewall_zones=[FirewallZone(name="public", path="etc/firewalld/zones/public.xml",
                                        content="<zone/>", services=["ssh"], ports=[], rich_rules=[])],
            static_routes=[StaticRouteFile(path="etc/sysconfig/network-scripts/route-eth0", name="route-eth0")],
            hosts_additions=["10.0.0.5 api-server"],
            proxy=[ProxyEntry(source="etc/environment", line="http_proxy=http://proxy:3128")],
            resolv_provenance="networkmanager",
        ),
        storage=StorageSection(fstab_entries=[
            FstabEntry(device="/dev/sda1", mount_point="/", fstype="xfs"),
            FstabEntry(device="nas:/data", mount_point="/data", fstype="nfs"),
        ]),
        scheduled_tasks=ScheduledTaskSection(
            cron_jobs=[CronJob(path="etc/cron.d/backup", source="cron.d")],
            generated_timer_units=[GeneratedTimerUnit(
                name="cron-backup",
                timer_content="[Timer]\nOnCalendar=*-*-* 02:00:00\n",
                service_content="[Service]\nExecStart=/usr/local/bin/backup.sh\n",
                cron_expr="0 2 * * *", source_path="etc/cron.d/backup",
                command="/usr/local/bin/backup.sh",
            )],
        ),
        non_rpm_software=NonRpmSoftwareSection(items=[
            NonRpmItem(name="cryptography", version="41.0.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.9/site-packages/cryptography-41.0.0.dist-info"),
            NonRpmItem(name="requests", version="2.32.5", method="pip dist-info",
                       confidence="high",
                       path="usr/lib/python3.9/site-packages/requests-2.32.5.dist-info"),
        ]),
        users_groups=UserGroupSection(
            users=[{"name": "mark", "uid": "1000", "gid": "1000",
                    "home": "/home/mark", "shell": "/bin/bash"}],
            passwd_entries=["mark:x:1000:1000::/home/mark:/bin/bash"],
            shadow_entries=["mark:!!:19700:0:99999:7:::"],
            group_entries=["mark:x:1000:"],
            gshadow_entries=["mark:!::"],
            sudoers_rules=["mark ALL=(ALL) NOPASSWD: ALL"],
            ssh_authorized_keys_refs=[{"user": "mark", "path": "/home/mark/.ssh/authorized_keys"}],
        ),
        kernel_boot=KernelBootSection(
            sysctl_overrides=[SysctlOverride(key="net.ipv4.ip_forward", runtime="1", default="0", source="operator")],
            non_default_modules=[KernelModule(name="br_netfilter", size="32768", used_by="")],
        ),
        selinux=SelinuxSection(
            mode="enforcing", custom_modules=["mypolicy"],
            boolean_overrides=[{"name": "httpd_can_network_connect", "current": "on",
                                "default": "off", "non_default": True}],
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        from yoinkc.renderers import run_all
        run_all(snapshot, Path(tmp))
        content = (Path(tmp) / "Containerfile").read_text()

    assert "AS builder" in content
    assert "# === Base Image ===" in content
    assert "# === Service Enablement ===" in content
    assert "# === Firewall Configuration (bake into image) ===" in content
    assert "# === Scheduled Tasks ===" in content
    assert "# === Non-RPM Software ===" in content
    assert "# === Users and Groups ===" in content
    assert "# === Kernel Configuration ===" in content
    assert "# === SELinux Customizations ===" in content
    assert "# === Network / Kickstart ===" in content
    assert "# === tmpfiles.d for /var structure ===" in content


# ---------------------------------------------------------------------------
# Shell value sanitizer
# ---------------------------------------------------------------------------

class TestSanitizeShellValue:

    def _sanitize(self, value, context="test"):
        from yoinkc.renderers.containerfile._helpers import _sanitize_shell_value
        return _sanitize_shell_value(value, context)

    def test_safe_package_name(self):
        assert self._sanitize("httpd") == "httpd"

    def test_safe_package_with_hyphen_and_dot(self):
        assert self._sanitize("python3-pip") == "python3-pip"
        assert self._sanitize("libssl3.0") == "libssl3.0"

    def test_safe_unit_name(self):
        assert self._sanitize("httpd.service") == "httpd.service"

    def test_safe_boolean_name(self):
        assert self._sanitize("httpd_can_network_connect") == "httpd_can_network_connect"

    def test_rejects_newline(self):
        assert self._sanitize("foo\nbar") is None

    def test_rejects_carriage_return(self):
        assert self._sanitize("foo\rbar") is None

    def test_rejects_semicolon(self):
        assert self._sanitize("foo;rm -rf /") is None

    def test_rejects_backtick(self):
        assert self._sanitize("foo`id`") is None

    def test_rejects_dollar_paren(self):
        assert self._sanitize("foo$(id)") is None

    def test_rejects_pipe(self):
        assert self._sanitize("foo|bar") is None

    def test_dollar_without_paren_is_safe(self):
        """$VAR without () is a variable reference — no shell execution risk here."""
        assert self._sanitize("foo$BAR") == "foo$BAR"

    def test_unsafe_package_name_produces_fixme(self):
        """Packages with unsafe names should produce a FIXME line, not a dnf install line."""
        import tempfile
        from yoinkc.schema import (
            InspectionSnapshot, OsRelease, RpmSection, PackageEntry, PackageState,
        )
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="bad;pkg", epoch="0", version="1.0", release="1", arch="x86_64"),
                ],
                no_baseline=True,
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            from yoinkc.renderers.containerfile import render
            from jinja2 import Environment
            render(snapshot, Environment(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        # The unsafe name must not appear in any RUN line
        run_lines = [l for l in cf.splitlines() if l.startswith("RUN ")]
        assert not any("bad;pkg" in l for l in run_lines), "Unsafe package name injected into RUN"
        assert "FIXME" in cf
        assert "unsafe characters" in cf

    def test_unsafe_unit_name_produces_fixme(self):
        """Units with unsafe names are skipped with a FIXME, not injected."""
        import tempfile
        from yoinkc.schema import InspectionSnapshot, OsRelease, ServiceSection
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            services=ServiceSection(
                enabled_units=["httpd.service", "evil;cmd.service"],
                disabled_units=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            from yoinkc.renderers.containerfile import render
            from jinja2 import Environment
            render(snapshot, Environment(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd.service" in cf
        assert "evil;cmd.service" not in cf.replace("FIXME", "")
        assert "unsafe characters" in cf


class TestCrossMajorWarning:

    def test_cross_major_warning_in_containerfile(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
            rpm=RpmSection(base_image="registry.redhat.io/rhel10/rhel-bootc:10.0"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "CROSS-MAJOR-VERSION MIGRATION" in cf
        assert "heavier manual review" in cf

    def test_no_warning_same_major(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.4", id="rhel"),
            rpm=RpmSection(base_image="registry.redhat.io/rhel9/rhel-bootc:9.6"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "CROSS-MAJOR-VERSION" not in cf

    def test_no_warning_centos_stream_tag(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="CentOS Stream", version_id="10", id="centos"),
            rpm=RpmSection(base_image="quay.io/centos-bootc/centos-bootc:stream10"),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "CROSS-MAJOR-VERSION" not in cf


class TestPythonVersionMap:

    def test_rhel10_uses_python312(self):
        items = [
            NonRpmItem(name="cryptography", version="41.0.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.12/site-packages/cryptography-41.0.0.dist-info"),
        ]
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="10.0", id="rhel"),
            rpm=RpmSection(base_image="registry.redhat.io/rhel10/rhel-bootc:10.0"),
            non_rpm_software=NonRpmSoftwareSection(items=items),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "python3.12" in cf
        assert "python3.X" not in cf

    def test_fedora_uses_python312(self):
        items = [
            NonRpmItem(name="numpy", version="1.26.0", method="pip dist-info",
                       has_c_extensions=True, confidence="high",
                       path="usr/lib/python3.12/site-packages/numpy-1.26.0.dist-info"),
        ]
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="Fedora", version_id="41", id="fedora"),
            rpm=RpmSection(base_image="quay.io/fedora/fedora-bootc:41"),
            non_rpm_software=NonRpmSoftwareSection(items=items),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "python3.12" in cf


class TestLeafAutoSlimming:

    def test_only_leaf_packages_in_dnf_install(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="httpd-core", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="httpd-filesystem", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                    PackageEntry(name="apr-util", epoch="0", version="1.6", release="1", arch="x86_64"),
                    PackageEntry(name="nginx", epoch="0", version="1.24", release="1", arch="x86_64"),
                ],
                leaf_packages=["httpd", "nginx"],
                auto_packages=["apr", "apr-util", "httpd-core", "httpd-filesystem"],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        install_block = cf.split("dnf install")[1].split("dnf clean")[0]
        assert "httpd" in install_block
        assert "nginx" in install_block
        assert "apr" not in install_block
        assert "httpd-core" not in install_block
        assert "4 additional package" in cf

    def test_fallback_when_no_leaf_data(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                ],
                leaf_packages=None,
                auto_packages=None,
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "apr" in cf
        assert "additional package" not in cf

    def test_audit_report_shows_both_groups(self):
        from yoinkc.renderers.audit_report import render as render_audit
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", epoch="0", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="apr", epoch="0", version="1.7", release="1", arch="x86_64"),
                ],
                leaf_packages=["httpd"],
                auto_packages=["apr"],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "Explicitly installed" in report
        assert "Dependencies" in report
        assert "httpd" in report
        assert "apr" in report


class TestIncludeFieldDefaults:
    """Every toggleable model defaults include=True."""

    def test_package_entry_defaults_true(self):
        p = PackageEntry(name="x", version="1", release="1", arch="x86_64")
        assert p.include is True

    def test_config_file_entry_defaults_true(self):
        c = ConfigFileEntry(path="/etc/foo", kind=ConfigFileKind.UNOWNED)
        assert c.include is True

    def test_service_state_change_defaults_true(self):
        s = ServiceStateChange(unit="x.service", current_state="enabled", default_state="disabled", action="enable")
        assert s.include is True

    def test_cron_job_defaults_true(self):
        assert CronJob(path="etc/cron.d/x", source="cron.d").include is True

    def test_generated_timer_unit_defaults_true(self):
        assert GeneratedTimerUnit(name="x").include is True

    def test_non_rpm_item_defaults_true(self):
        assert NonRpmItem(path="/opt/x").include is True

    def test_sysctl_override_defaults_true(self):
        assert SysctlOverride(key="net.ipv4.ip_forward").include is True

    def test_kernel_module_defaults_true(self):
        assert KernelModule(name="vfat").include is True

    def test_quadlet_unit_defaults_true(self):
        assert QuadletUnit(path="/etc/containers/systemd/x.container", name="x.container").include is True

    def test_compose_file_defaults_true(self):
        assert ComposeFile(path="/opt/app/docker-compose.yml").include is True

    def test_repofile_include_defaults_true(self):
        assert RepoFile(path="etc/yum.repos.d/epel.repo", content="").include is True


class TestContainerfileExclusion:
    """Excluded items are omitted from Containerfile output."""

    def _base_snapshot(self):
        return InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64", include=False),
                ],
                leaf_packages=["httpd", "nginx"],
                auto_packages=[],
            ),
        )

    def test_excluded_package_omitted_from_dnf_install(self):
        snapshot = self._base_snapshot()
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "nginx" not in cf

    def test_excluded_leaf_removes_auto_deps(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64", include=False),
                    PackageEntry(name="apr", version="1.7", release="1", arch="x86_64"),
                    PackageEntry(name="apr-util", version="1.6", release="1", arch="x86_64"),
                    PackageEntry(name="nginx-core", version="1.24", release="1", arch="x86_64"),
                ],
                leaf_packages=["httpd", "nginx"],
                auto_packages=["apr", "apr-util", "nginx-core"],
                leaf_dep_tree={
                    "httpd": ["apr", "apr-util"],
                    "nginx": ["nginx-core"],
                },
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "nginx" not in cf
        # nginx-core should not be counted since nginx (its only puller) is excluded
        assert "2 additional" in cf  # apr + apr-util from httpd

    def test_excluded_config_file_not_written(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            config=ConfigSection(files=[
                ConfigFileEntry(path="/etc/foo.conf", kind=ConfigFileKind.UNOWNED, content="hello"),
                ConfigFileEntry(path="/etc/bar.conf", kind=ConfigFileKind.UNOWNED, content="world", include=False),
            ]),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            assert (Path(tmp) / "config" / "etc" / "foo.conf").exists()
            assert not (Path(tmp) / "config" / "etc" / "bar.conf").exists()

    def test_excluded_timer_not_enabled(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            scheduled_tasks=ScheduledTaskSection(
                generated_timer_units=[
                    GeneratedTimerUnit(name="cron-foo", timer_content="[Timer]", service_content="[Service]"),
                    GeneratedTimerUnit(name="cron-bar", timer_content="[Timer]", service_content="[Service]", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "cron-foo" in cf
        assert "cron-bar" not in cf

    def test_excluded_quadlet_not_written(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            containers=ContainerSection(
                quadlet_units=[
                    QuadletUnit(path="/etc/containers/systemd/a.container", name="a.container", content="[Container]"),
                    QuadletUnit(path="/etc/containers/systemd/b.container", name="b.container", content="[Container]", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            assert (Path(tmp) / "quadlet" / "a.container").exists()
            assert not (Path(tmp) / "quadlet" / "b.container").exists()

    def test_excluded_repo_not_written(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                repo_files=[
                    RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://epel"),
                    RepoFile(path="etc/yum.repos.d/custom.repo", content="[custom]\nbaseurl=http://custom", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            assert (Path(tmp) / "config" / "etc" / "yum.repos.d" / "epel.repo").exists()
            assert not (Path(tmp) / "config" / "etc" / "yum.repos.d" / "custom.repo").exists()

    def test_excluded_repo_comment_in_containerfile(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                repo_files=[
                    RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\n"),
                    RepoFile(path="etc/yum.repos.d/custom.repo", content="[custom]\n", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "# Excluded repo: etc/yum.repos.d/custom.repo" in cf


class TestAuditReportExcluded:
    """Excluded items still appear in the audit report with [EXCLUDED] prefix."""

    def test_excluded_package_shows_excluded(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64"),
                    PackageEntry(name="nginx", version="1.24", release="1", arch="x86_64", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "[EXCLUDED] nginx" in report
        assert "httpd" in report
        assert "[EXCLUDED] httpd" not in report

    def test_excluded_service_shows_excluded(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            services=ServiceSection(
                state_changes=[
                    ServiceStateChange(unit="foo.service", current_state="enabled",
                                       default_state="disabled", action="enable"),
                    ServiceStateChange(unit="bar.service", current_state="enabled",
                                       default_state="disabled", action="enable", include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "[EXCLUDED] bar.service" in report
        assert "foo.service" in report
        assert "[EXCLUDED] foo.service" not in report

    def test_excluded_user_shows_excluded(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            users_groups=UserGroupSection(
                users=[
                    {"name": "alice", "uid": 1000, "shell": "/bin/bash", "home": "/home/alice", "include": True},
                    {"name": "bob", "uid": 1001, "shell": "/bin/bash", "home": "/home/bob", "include": False},
                ],
                groups=[
                    {"name": "alice", "gid": 1000, "include": True},
                    {"name": "bob", "gid": 1001, "include": False},
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_audit(snapshot, _env(), Path(tmp))
            report = (Path(tmp) / "audit-report.md").read_text()
        assert "[EXCLUDED] User: **bob**" in report
        assert "[EXCLUDED] Group: **bob**" in report
        assert "[EXCLUDED] User: **alice**" not in report
        assert "[EXCLUDED] Group: **alice**" not in report


class TestUserGroupIncludeKey:
    """User and group dicts respect the include key in renderers."""

    def test_excluded_user_omitted_from_containerfile(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            users_groups=UserGroupSection(
                users=[
                    {"name": "alice", "uid": 1000, "gid": 1000, "shell": "/bin/bash",
                     "home": "/home/alice", "include": True, "classification": "human",
                     "strategy": "useradd"},
                    {"name": "bob", "uid": 1001, "gid": 1001, "shell": "/bin/bash",
                     "home": "/home/bob", "include": False, "classification": "human",
                     "strategy": "useradd"},
                ],
                groups=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "useradd" in cf
        assert "alice" in cf
        assert "bob" not in cf

    def test_user_include_defaults_true(self):
        """Dicts without explicit include key are treated as included."""
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            users_groups=UserGroupSection(
                users=[
                    {"name": "carol", "uid": 1002, "gid": 1002, "shell": "/bin/bash",
                     "home": "/home/carol", "classification": "human",
                     "strategy": "useradd"},
                ],
                groups=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "carol" in cf


class TestSourceRepo:
    """PackageEntry.source_repo field."""

    def test_source_repo_field_populated(self):
        p = PackageEntry(name="htop", version="3.2", release="1", arch="x86_64", source_repo="epel")
        assert p.source_repo == "epel"
        d = p.model_dump()
        assert d["source_repo"] == "epel"
        p2 = PackageEntry.model_validate(d)
        assert p2.source_repo == "epel"

    def test_source_repo_defaults_empty(self):
        p = PackageEntry(name="x", version="1", release="1", arch="x86_64")
        assert p.source_repo == ""


class TestRepoFileClassification:
    """is_default_repo classification logic."""

    def test_default_repo_redhat(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/redhat.repo", content="[rhel-baseos]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_epel(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_appstream_section(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/centos.repo", content="[appstream]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True

    def test_non_default_repo_copr(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/copr-myrepo.repo", content="[copr:user:project]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is False

    def test_default_repo_fedora_section(self):
        from yoinkc.inspectors.rpm import _classify_default_repo
        rf = RepoFile(path="etc/yum.repos.d/fedora.repo", content="[fedora]\nbaseurl=http://x\n")
        assert _classify_default_repo(rf) is True


class TestRepoCascadeContainerfile:
    """When repo include=False and its packages also have include=False, both are excluded."""

    def test_excluded_repo_and_its_packages(self):
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            rpm=RpmSection(
                base_image="registry.redhat.io/rhel9/rhel-bootc:9.6",
                packages_added=[
                    PackageEntry(name="httpd", version="2.4", release="1", arch="x86_64", source_repo="baseos"),
                    PackageEntry(name="htop", version="3.2", release="1", arch="x86_64", source_repo="epel", include=False),
                ],
                leaf_packages=["httpd", "htop"],
                auto_packages=[],
                repo_files=[
                    RepoFile(path="etc/yum.repos.d/epel.repo", content="[epel]\nbaseurl=http://x\n",
                             is_default_repo=False, include=False),
                ],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            render_containerfile(snapshot, _env(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        assert "htop" not in cf
        assert "# Excluded repo: etc/yum.repos.d/epel.repo" in cf
        assert not (Path(tmp) / "config" / "etc" / "yum.repos.d" / "epel.repo").exists()
