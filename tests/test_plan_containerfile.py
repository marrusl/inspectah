"""Plan item tests: multi-stage containerfile, exclusions, config diff, sanitizer, cross-major, HTML diffs, storage."""

import tempfile
from pathlib import Path

from inspectah.schema import (
    ComposeFile,
    ConfigFileEntry,
    ConfigFileKind,
    ConfigSection,
    ContainerSection,
    GeneratedTimerUnit,
    InspectionSnapshot,
    NonRpmItem,
    NonRpmSoftwareSection,
    OsRelease,
    PackageEntry,
    QuadletUnit,
    RepoFile,
    RpmSection,
    ScheduledTaskSection,
    ServiceSection,
    ServiceStateChange,
)
from inspectah.renderers.containerfile import render as render_containerfile
from inspectah.renderers.audit_report import render as render_audit
from inspectah.renderers.html_report import render as render_html_report

from conftest import _env


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
        assert "2 additional" in cf

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
        from inspectah.schema import UserGroupSection
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


class TestConfigDiffFallback:

    def test_download_rpm_from_repo_success(self):
        from inspectah.inspectors.config import _download_rpm_from_repo
        from inspectah.executor import RunResult

        def exec_(cmd, cwd=None):
            cmd_str = " ".join(cmd)
            if "dnf" in cmd_str and "download" in cmd_str:
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
        from inspectah.inspectors.config import _extract_file_from_rpm
        from inspectah.executor import RunResult

        captured = []
        def exec_(cmd, cwd=None):
            captured.append(" ".join(cmd))
            return RunResult(stdout="content", stderr="", returncode=0)

        _extract_file_from_rpm(exec_, Path("/a.rpm"), "etc/httpd/conf/httpd.conf")
        assert "./etc/httpd/conf/httpd.conf" in captured[0]


class TestSanitizeShellValue:

    def _sanitize(self, value, context="test"):
        from inspectah.renderers.containerfile._helpers import _sanitize_shell_value
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
        from inspectah.schema import (
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
            from inspectah.renderers.containerfile import render
            from jinja2 import Environment
            render(snapshot, Environment(), Path(tmp))
            cf = (Path(tmp) / "Containerfile").read_text()
        assert "httpd" in cf
        run_lines = [l for l in cf.splitlines() if l.startswith("RUN ")]
        assert not any("bad;pkg" in l for l in run_lines), "Unsafe package name injected into RUN"
        assert "FIXME" in cf
        assert "unsafe characters" in cf

    def test_unsafe_unit_name_produces_fixme(self):
        """Units with unsafe names are skipped with a FIXME, not injected."""
        import tempfile
        from inspectah.schema import InspectionSnapshot, OsRelease, ServiceSection
        snapshot = InspectionSnapshot(
            meta={},
            os_release=OsRelease(name="RHEL", version_id="9.6", id="rhel"),
            services=ServiceSection(
                enabled_units=["httpd.service", "evil;cmd.service"],
                disabled_units=[],
            ),
        )
        with tempfile.TemporaryDirectory() as tmp:
            from inspectah.renderers.containerfile import render
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


def test_html_diff_preview_removed():
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
        assert f'class="{cls}"' not in html


def test_storage_recommendation_mapping():
    from inspectah.renderers.audit_report import _storage_recommendation as rec
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
