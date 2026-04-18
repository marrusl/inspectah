"""Plan item tests: user creation strategies, user/group include key."""

import tempfile
from pathlib import Path

from inspectah.schema import (
    InspectionSnapshot,
    OsRelease,
    PackageEntry,
    RpmSection,
    UserGroupSection,
)
from inspectah.renderers.containerfile import render as render_containerfile

from conftest import _env


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
            sysusers_path = Path(tmp) / "config/usr/lib/sysusers.d/inspectah-users.conf"
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
        from inspectah.renderers.kickstart import render as render_kickstart
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
            toml_path = Path(tmp) / "inspectah-users.toml"
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
            assert not (Path(tmp) / "inspectah-users.toml").exists()

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
        from inspectah.inspectors.users_groups import run as run_ug
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
            assert (Path(tmp) / "inspectah-users.toml").exists()
            toml = (Path(tmp) / "inspectah-users.toml").read_text()
            assert "[[customizations.user]]" in toml
            assert 'name = "mark"' in toml

    def test_audit_report_strategy_table(self):
        from inspectah.renderers.audit_report import render as render_audit
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
        from inspectah.renderers.readme import render as render_readme
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
            (Path(tmp) / "Containerfile").write_text("FROM base\n")
            render_readme(snapshot, _env(), Path(tmp))
            readme = (Path(tmp) / "README.md").read_text()
            assert "User Creation Strategies" in readme
            assert "sysusers" in readme
            assert "bootc" in readme.lower()

    def test_cli_user_strategy_invalid(self):
        from inspectah.cli import parse_args
        try:
            parse_args(["--user-strategy", "invalid"])
            assert False, "Should have raised SystemExit"
        except SystemExit:
            pass


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
