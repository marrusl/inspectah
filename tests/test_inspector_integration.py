"""Integration inspector tests: users/groups, run_all, baseline, hostname, snapshot roundtrip,
leaf/auto classification, inspector failure degradation."""

import tempfile
from pathlib import Path

import pytest

from inspectah.executor import RunResult
from inspectah.inspectors import run_all
from inspectah.schema import InspectionSnapshot

FIXTURES = Path(__file__).parent / "fixtures"


def test_user_classification():
    from inspectah.inspectors.users_groups import _classify_user, _STRATEGY_MAP

    assert _classify_user({"shell": "/sbin/nologin", "home": "/var/lib/redis", "uid": 1001}) == "service"
    assert _STRATEGY_MAP["service"] == "sysusers"

    assert _classify_user({"shell": "/bin/false", "home": "/home/nobody", "uid": 1002}) == "service"

    assert _classify_user({"shell": "/sbin/nologin", "home": "/var/lib/myapp", "uid": 1003}) == "service"

    assert _classify_user({"shell": "/bin/bash", "home": "/home/alice", "uid": 1000}) == "human"
    assert _STRATEGY_MAP["human"] == "kickstart"

    assert _classify_user({"shell": "/bin/zsh", "home": "/home/bob", "uid": 1001}) == "human"

    assert _classify_user({"shell": "/bin/bash", "home": "/var/lib/myapp", "uid": 1004}) == "ambiguous"
    assert _STRATEGY_MAP["ambiguous"] == "useradd"

    assert _classify_user({"shell": "/usr/local/bin/custom-shell", "home": "/home/custom", "uid": 1005}) == "ambiguous"


def test_user_classification_in_fixture(host_root, fixture_executor):
    from inspectah.inspectors.users_groups import run as run_users_groups
    section = run_users_groups(host_root, fixture_executor)
    jdoe = next(u for u in section.users if u["name"] == "jdoe")
    assert jdoe["classification"] == "human"
    assert jdoe["strategy"] == "kickstart"

    jdoe_group = next(g for g in section.groups if g["name"] == "jdoe")
    assert jdoe_group["strategy"] == "kickstart"


def test_group_strategy_no_user(tmp_path):
    """Groups with no associated user default to sysusers."""
    from inspectah.inspectors.users_groups import run as run_users_groups

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "passwd").write_text("root:x:0:0:root:/root:/bin/bash\n")
    (etc / "group").write_text("mygroup:x:2000:\n")

    section = run_users_groups(tmp_path, executor=None)

    mygroup = next((g for g in section.groups if g["name"] == "mygroup"), None)
    assert mygroup is not None, "mygroup must be collected by the inspector"
    assert mygroup["strategy"] == "sysusers", (
        f"Group with no primary user must default to sysusers, got {mygroup['strategy']!r}"
    )


def test_group_strategy_first_user_wins_on_shared_gid(tmp_path):
    """When two users share a primary GID, the group inherits the first user's strategy."""
    from inspectah.inspectors.users_groups import run as run_users_groups

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "passwd").write_text(
        "alice:x:1000:2000:Alice:/home/alice:/bin/bash\n"
        "bob:x:1001:2000:Bob:/var/lib/bob:/sbin/nologin\n"
    )
    (etc / "group").write_text("appgroup:x:2000:\n")

    section = run_users_groups(tmp_path, executor=None)

    alice = next(u for u in section.users if u["name"] == "alice")
    assert alice["strategy"] == "kickstart"

    appgroup = next(g for g in section.groups if g["name"] == "appgroup")
    assert appgroup["strategy"] == "kickstart", (
        f"appgroup should follow alice (first user with gid 2000), got {appgroup['strategy']!r}"
    )


def test_users_groups_inspector_with_fixtures(host_root, fixture_executor):
    from inspectah.inspectors.users_groups import run as run_users_groups
    section = run_users_groups(host_root, fixture_executor)
    assert section is not None
    assert any(u.get("name") == "jdoe" and u.get("uid") == 1000 for u in section.users)
    assert any(g.get("name") == "jdoe" and g.get("gid") == 1000 for g in section.groups)

    assert len(section.passwd_entries) >= 1
    assert any("jdoe" in e for e in section.passwd_entries)
    assert len(section.shadow_entries) >= 1
    assert any("jdoe" in e for e in section.shadow_entries)
    assert len(section.group_entries) >= 1
    assert any("jdoe" in e for e in section.group_entries)
    assert len(section.gshadow_entries) >= 1
    assert any("jdoe" in e for e in section.gshadow_entries)
    assert len(section.subuid_entries) >= 1
    assert any("jdoe" in e for e in section.subuid_entries)
    assert len(section.subgid_entries) >= 1
    assert any("jdoe" in e for e in section.subgid_entries)

    assert not any("root" in e for e in section.passwd_entries)
    assert not any("nobody" in e for e in section.passwd_entries)


def test_run_all_with_fixtures(host_root, fixture_executor):
    """Full run with base image query → baseline applied, all inspectors run."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    assert isinstance(snapshot, InspectionSnapshot)
    assert snapshot.os_release is not None
    assert snapshot.os_release.name == "Red Hat Enterprise Linux"
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is False
    assert snapshot.rpm.baseline_package_names is not None
    assert len(snapshot.rpm.packages_added) > 0
    added_names = [p.name for p in snapshot.rpm.packages_added]
    assert "httpd" in added_names
    assert "bash" not in added_names
    assert snapshot.services is not None
    assert snapshot.config is not None
    assert snapshot.network is not None
    assert snapshot.storage is not None
    assert snapshot.scheduled_tasks is not None
    assert snapshot.containers is not None
    assert snapshot.non_rpm_software is not None
    assert snapshot.kernel_boot is not None
    assert snapshot.selinux is not None
    assert snapshot.users_groups is not None


# ---------------------------------------------------------------------------
# Local helpers (not shared)
# ---------------------------------------------------------------------------

def _no_baseline_executor(cmd, cwd=None):
    """Executor where podman always fails but rpm/systemctl work."""
    if "podman" in cmd:
        return RunResult(stdout="", stderr="not available", returncode=127)
    if "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-Va" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-ql" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
    if "systemctl" in cmd:
        return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
    return RunResult(stdout="", stderr="", returncode=1)


def _failing_executor(cmd, cwd=None):
    """Executor that returns non-zero for everything except nsenter probe."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    return RunResult(stdout="", stderr="command not available", returncode=1)


# ---------------------------------------------------------------------------
# Baseline / no-baseline tests
# ---------------------------------------------------------------------------

def test_run_all_no_baseline_fails_fast(host_root, capsys):
    """Without --no-baseline, missing baseline causes sys.exit(1)."""
    with pytest.raises(SystemExit) as exc_info:
        run_all(host_root, executor=_no_baseline_executor)
    assert exc_info.value.code == 1
    err = capsys.readouterr().err
    assert "Could not query the base image package list" in err
    assert "--no-baseline" in err
    assert "--baseline-packages" in err


def test_no_baseline_error_includes_registry_login_for_rhel(host_root, capsys):
    """Fail-fast error includes registry login step for RHEL images."""
    with pytest.raises(SystemExit):
        run_all(host_root, executor=_no_baseline_executor)
    err = capsys.readouterr().err
    assert "sudo podman login registry.redhat.io" in err


def test_no_baseline_error_omits_registry_login_for_centos(tmp_path, capsys):
    """Fail-fast error omits registry login step for CentOS (public registry)."""
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('NAME="CentOS Stream"\nVERSION_ID="9"\nID=centos\n')
    with pytest.raises(SystemExit):
        run_all(tmp_path, executor=_no_baseline_executor)
    err = capsys.readouterr().err
    assert "Could not query the base image package list" in err
    assert "registry.redhat.io" not in err


def test_run_all_no_baseline_warning(host_root):
    """With --no-baseline, missing baseline produces a warning and continues."""
    snapshot = run_all(host_root, executor=_no_baseline_executor, no_baseline_opt_in=True)
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is True
    rpm_warnings = [w for w in snapshot.warnings if w.get("source") == "rpm"]
    assert any("--no-baseline" in w.get("message", "") for w in rpm_warnings)


# ---------------------------------------------------------------------------
# Cross-major-version warnings
# ---------------------------------------------------------------------------

def test_cross_major_warning_in_snapshot(host_root, fixture_executor):
    """Cross-major-version warning appears in snapshot.warnings."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        target_version="10.0",
    )
    cross_warnings = [
        w for w in snapshot.warnings
        if "Cross-major-version" in w.get("message", "")
    ]
    assert len(cross_warnings) == 1, (
        f"Expected exactly one cross-major warning, got {len(cross_warnings)}"
    )
    assert cross_warnings[0]["severity"] == "error"


def test_no_cross_major_warning_same_version(host_root, fixture_executor):
    """No cross-major warning when source and target are same major."""
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        target_version="9.8",
    )
    cross_warnings = [
        w for w in snapshot.warnings
        if "Cross-major-version" in w.get("message", "")
    ]
    assert len(cross_warnings) == 0


# ---------------------------------------------------------------------------
# Hostname resolution
# ---------------------------------------------------------------------------

@pytest.mark.parametrize("etc_hostname_content", ["", "\n"])
def test_hostname_env_var_takes_priority_over_etc_hostname(tmp_path, fixture_executor, monkeypatch, etc_hostname_content):
    """INSPECTAH_HOSTNAME env var takes priority over /etc/hostname."""
    monkeypatch.setenv("INSPECTAH_HOSTNAME", "myhost")

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text(etc_hostname_content)
    (etc / "os-release").write_text('NAME="Red Hat Enterprise Linux"\nVERSION_ID="9.4"\nID=rhel\n')

    snapshot = run_all(tmp_path, executor=fixture_executor, no_baseline_opt_in=True)
    assert snapshot.meta.get("hostname") == "myhost"


def test_hostname_env_var_overrides_etc_hostname(tmp_path, fixture_executor, monkeypatch):
    """INSPECTAH_HOSTNAME takes precedence even when /etc/hostname is non-empty."""
    monkeypatch.setenv("INSPECTAH_HOSTNAME", "from-env")

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text("from-etc-hostname\n")
    (etc / "os-release").write_text('NAME="CentOS Stream"\nVERSION_ID="9"\nID=centos\n')

    snapshot = run_all(tmp_path, executor=fixture_executor, no_baseline_opt_in=True)
    assert snapshot.meta.get("hostname") == "from-env"


def test_hostname_falls_back_to_etc_hostname_when_env_unset(tmp_path, fixture_executor, monkeypatch):
    """When INSPECTAH_HOSTNAME is absent, /etc/hostname is used."""
    monkeypatch.delenv("INSPECTAH_HOSTNAME", raising=False)

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text("from-etc-hostname\n")
    (etc / "os-release").write_text('NAME="CentOS Stream"\nVERSION_ID="9"\nID=centos\n')

    snapshot = run_all(tmp_path, executor=fixture_executor, no_baseline_opt_in=True)
    assert snapshot.meta.get("hostname") == "from-etc-hostname"


def test_hostname_from_etc_hostname_strips_whitespace(tmp_path, fixture_executor, monkeypatch):
    """The first /etc/hostname line is stripped before storing it in snapshot metadata."""
    monkeypatch.delenv("INSPECTAH_HOSTNAME", raising=False)

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text("  from-etc-hostname  \nextra-line\n")
    (etc / "os-release").write_text('NAME="CentOS Stream"\nVERSION_ID="9"\nID=centos\n')

    snapshot = run_all(tmp_path, executor=fixture_executor, no_baseline_opt_in=True)
    assert snapshot.meta.get("hostname") == "from-etc-hostname"


def test_hostname_falls_back_to_hostnamectl_when_env_and_etc_hostname_are_empty(
    tmp_path, fixture_executor, monkeypatch
):
    """When env and /etc/hostname are empty, hostnamectl hostname is used."""
    monkeypatch.delenv("INSPECTAH_HOSTNAME", raising=False)

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text("\n")
    (etc / "os-release").write_text('NAME="Red Hat Enterprise Linux"\nVERSION_ID="9.4"\nID=rhel\n')

    calls = []

    def executor(cmd, cwd=None):
        if cmd == ["hostnamectl", "hostname"]:
            calls.append(cmd)
            return RunResult(stdout="host.example.com\n", stderr="", returncode=0)
        return fixture_executor(cmd, cwd=cwd)

    snapshot = run_all(tmp_path, executor=executor, no_baseline_opt_in=True)
    assert snapshot.meta.get("hostname") == "host.example.com"
    assert calls == [["hostnamectl", "hostname"]]


def test_hostname_is_omitted_when_env_file_and_hostnamectl_are_unavailable(
    tmp_path, fixture_executor, monkeypatch
):
    """Missing env, empty /etc/hostname, and unavailable hostnamectl must not crash."""
    monkeypatch.delenv("INSPECTAH_HOSTNAME", raising=False)

    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "hostname").write_text("")
    (etc / "os-release").write_text('NAME="Red Hat Enterprise Linux"\nVERSION_ID="9.4"\nID=rhel\n')

    def executor(cmd, cwd=None):
        if cmd == ["hostnamectl", "hostname"]:
            return RunResult(stdout="", stderr="Command not found", returncode=127)
        return fixture_executor(cmd, cwd=cwd)

    snapshot = run_all(tmp_path, executor=executor, no_baseline_opt_in=True)
    assert "hostname" not in snapshot.meta


# ---------------------------------------------------------------------------
# Snapshot roundtrip
# ---------------------------------------------------------------------------

def test_snapshot_roundtrip_with_baseline(host_root, fixture_executor):
    """Resolved baseline is in inspection-snapshot.json; --from-snapshot re-renders without network."""
    from inspectah.pipeline import load_snapshot, save_snapshot
    from inspectah.renderers import run_all as run_all_renderers
    snapshot = run_all(
        host_root,
        executor=fixture_executor,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
    )
    assert snapshot.rpm is not None
    assert snapshot.rpm.no_baseline is False
    assert snapshot.rpm.baseline_package_names is not None
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp)
        path = out / "inspection-snapshot.json"
        save_snapshot(snapshot, path)
        loaded = load_snapshot(path)
        assert loaded.rpm is not None
        assert loaded.rpm.baseline_package_names is not None
        assert loaded.rpm.no_baseline is False
        run_all_renderers(loaded, out)
        assert (out / "Containerfile").exists()
        assert (out / "audit-report.md").exists()
        assert (out / "report.html").exists()


# ---------------------------------------------------------------------------
# Leaf/auto classification
# ---------------------------------------------------------------------------

def test_classify_leaf_auto_falls_back_when_dnf_repoquery_fails(host_root):
    """When dnf repoquery is unavailable, _classify_leaf_auto falls back to rpm -qR."""
    from inspectah.inspectors.rpm import _classify_leaf_auto
    from inspectah.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="mod_ssl", epoch="1", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def dnf_fail_executor(cmd, cwd=None):
        cmd_str = " ".join(cmd)
        if "dnf" in cmd and "repoquery" in cmd:
            return RunResult(stdout="", stderr="dnf: command not found", returncode=127)
        if "rpm" in cmd and "-qR" in cmd:
            if "httpd" in cmd_str:
                return RunResult(stdout="mod_ssl\nhttpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        if "rpm" in cmd and "--whatprovides" in cmd:
            return RunResult(stdout="mod_ssl-1:2.4.62-1.el9.x86_64\n", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(dnf_fail_executor, host_root, packages)

    assert "httpd" in leaf
    assert "mod_ssl" in auto
    assert dep_tree["httpd"] == ["mod_ssl"]


def test_classify_leaf_auto_uses_userinstalled(host_root):
    """When dnf repoquery --userinstalled succeeds, it determines the leaf set."""
    from inspectah.inspectors.rpm import _classify_leaf_auto
    from inspectah.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="git", epoch="0", version="2.43.5", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="perl-interpreter", epoch="4", version="5.32.1", release="480.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="httpd\ngit\n", stderr="", returncode=0)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            dep_map = {
                "httpd": "httpd-core\n",
                "git": "perl-interpreter\n",
                "httpd-core": "",
                "perl-interpreter": "",
            }
            return RunResult(stdout=dep_map.get(pkg, ""), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["git", "httpd"]
    assert auto == ["httpd-core", "perl-interpreter"]
    assert dep_tree["httpd"] == ["httpd-core"]
    assert dep_tree["git"] == ["perl-interpreter"]


def test_classify_leaf_auto_userinstalled_fallback_on_failure(host_root):
    """When --userinstalled fails but dnf dep queries work, graph-based classification is used."""
    from inspectah.inspectors.rpm import _classify_leaf_auto
    from inspectah.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="", stderr="error", returncode=1)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            if pkg == "httpd":
                return RunResult(stdout="httpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["httpd"]
    assert auto == ["httpd-core"]


def test_classify_leaf_auto_empty_userinstalled_falls_back(host_root):
    """When --userinstalled succeeds but has no overlap with added packages, fall back to graph."""
    from inspectah.inspectors.rpm import _classify_leaf_auto
    from inspectah.schema import PackageEntry, PackageState

    packages = [
        PackageEntry(name="httpd", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
        PackageEntry(name="httpd-core", epoch="0", version="2.4.62", release="1.el9", arch="x86_64", state=PackageState.ADDED),
    ]

    def executor(cmd, cwd=None):
        if "--userinstalled" in cmd:
            return RunResult(stdout="vim\nemacs\n", stderr="", returncode=0)
        if "dnf" in cmd and "repoquery" in cmd and "--requires" in cmd:
            pkg = cmd[-1]
            if pkg == "httpd":
                return RunResult(stdout="httpd-core\n", stderr="", returncode=0)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    leaf, auto, dep_tree = _classify_leaf_auto(executor, host_root, packages)

    assert leaf == ["httpd"]
    assert auto == ["httpd-core"]


# ---------------------------------------------------------------------------
# Graceful degradation
# ---------------------------------------------------------------------------

class TestInspectorFailures:
    """Each inspector must return a valid (possibly empty) section when commands fail."""

    def test_service_falls_back_to_fs_scan(self, host_root):
        from inspectah.inspectors.service import run as run_service
        section = run_service(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.state_changes, list)

    def test_kernel_boot_empty_on_lsmod_failure(self, host_root):
        from inspectah.inspectors.kernel_boot import run as run_kernel_boot
        section = run_kernel_boot(host_root, _failing_executor)
        assert section is not None
        assert section.loaded_modules == []

    def test_scheduled_tasks_skips_at_jobs(self, host_root):
        from inspectah.inspectors.scheduled_tasks import run as run_scheduled_tasks
        section = run_scheduled_tasks(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.at_jobs, list)
        assert isinstance(section.cron_jobs, list)

    def test_rpm_empty_on_failure(self, host_root):
        from inspectah.inspectors.rpm import run as run_rpm
        section = run_rpm(host_root, _failing_executor)
        assert section is not None
        assert section.packages_added == []
        assert section.rpm_va == []

    def test_selinux_graceful_on_failure(self, host_root):
        from inspectah.inspectors.selinux import run as run_selinux
        section = run_selinux(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.boolean_overrides, list)

    def test_network_empty_on_failure(self, host_root):
        from inspectah.inspectors.network import run as run_network
        section = run_network(host_root, _failing_executor)
        assert section is not None
        assert isinstance(section.connections, list)
