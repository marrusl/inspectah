"""Plan item tests: service baseline presets, cron-to-OnCalendar, cron command extraction, RPM-owned cron filtering."""

from pathlib import Path

from yoinkc.schema import ScheduledTaskSection


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
        rpm_owned = {"/etc/cron.d/logrotate"}
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
