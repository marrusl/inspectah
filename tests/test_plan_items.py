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
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    CronJob,
    FstabEntry,
    FirewallZone,
    GeneratedTimerUnit,
    InspectionSnapshot,
    KernelBootSection,
    NMConnection,
    NetworkSection,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    ProxyEntry,
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
from yoinkc.renderers.html_report import render as render_html_report


def _env():
    return Environment(autoescape=True)


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
        enabled, disabled, has_disable_all = _parse_preset_lines(lines)
        assert "sshd.service" in enabled
        assert "kdump.service" in disabled
        assert has_disable_all is True

    def test_base_image_text_preferred_over_host(self):
        from yoinkc.inspectors.service import _parse_preset_files
        enabled, disabled, _ = _parse_preset_files(
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
        assert "1..5" in cal
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
# 3. Multi-stage Containerfile for pip C extensions
# ---------------------------------------------------------------------------

class TestMultiStageContainerfile:

    def _pip_snapshot(self, c_ext=True):
        items = [
            {"name": "cryptography", "version": "41.0.0", "method": "pip dist-info",
             "has_c_extensions": c_ext, "confidence": "high",
             "path": "usr/lib/python3.9/site-packages/cryptography-41.0.0.dist-info"},
            {"name": "requests", "version": "2.31.0", "method": "pip dist-info",
             "confidence": "high",
             "path": "usr/lib/python3.9/site-packages/requests-2.31.0.dist-info"},
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
            if "dnf" in " ".join(cmd) and "download" in " ".join(cmd):
                dest = Path("/tmp/yoinkc-rpm-download")
                dest.mkdir(parents=True, exist_ok=True)
                (dest / "httpd-2.4.51-7.el9.x86_64.rpm").write_text("fake")
                return RunResult(stdout="", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=1)

        result = _download_rpm_from_repo(exec_, Path("/host"), "httpd")
        assert result is not None and result.name.startswith("httpd-")
        import shutil; shutil.rmtree("/tmp/yoinkc-rpm-download", ignore_errors=True)

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

def test_append_files_written():
    snapshot = InspectionSnapshot(
        meta={}, os_release=OsRelease(name="RHEL", version_id="9.6"),
        users_groups=UserGroupSection(
            users=[{"name": "mark", "uid": "1000", "gid": "1000",
                    "home": "/home/mark", "shell": "/bin/bash"}],
            passwd_entries=["mark:x:1000:1000::/home/mark:/bin/bash"],
            shadow_entries=["mark:!!:19700:0:99999:7:::"],
            group_entries=["mark:x:1000:"],
            gshadow_entries=["mark:!::"],
            subuid_entries=["mark:100000:65536"],
            subgid_entries=["mark:100000:65536"],
        ),
    )
    with tempfile.TemporaryDirectory() as tmp:
        render_containerfile(snapshot, _env(), Path(tmp))
        # .append files are written to config/tmp/ so they are NOT swept up by
        # COPY config/etc/ /etc/ — they need to go to /tmp/ via a separate COPY.
        tmp_dir = Path(tmp) / "config" / "tmp"
        for f in ("passwd.append", "shadow.append", "group.append",
                  "gshadow.append", "subuid.append", "subgid.append"):
            assert (tmp_dir / f).exists(), f"Missing {f}"
        assert "mark:x:1000:1000" in (tmp_dir / "passwd.append").read_text()
        # Verify the Containerfile uses config/tmp/ as the COPY source
        cf = (Path(tmp) / "Containerfile").read_text()
        assert "COPY config/tmp/passwd.append /tmp/passwd.append" in cf
        assert "COPY config/etc/passwd.append" not in cf


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
            {"name": "cryptography", "version": "41.0.0", "method": "pip dist-info",
             "has_c_extensions": True, "confidence": "high",
             "path": "usr/lib/python3.9/site-packages/cryptography-41.0.0.dist-info"},
            {"name": "requests", "version": "2.31.0", "method": "pip dist-info",
             "confidence": "high",
             "path": "usr/lib/python3.9/site-packages/requests-2.31.0.dist-info"},
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
            sysctl_overrides=[{"key": "net.ipv4.ip_forward", "runtime": "1", "default": "0", "source": "operator"}],
            non_default_modules=[{"name": "br_netfilter", "size": "32768", "used_by": []}],
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
        from yoinkc.renderers.containerfile import _sanitize_shell_value
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
