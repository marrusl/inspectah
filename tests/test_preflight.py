"""Tests for container preflight checks."""

import argparse
import sys
from pathlib import Path
from unittest.mock import MagicMock, patch, mock_open

import pytest

from yoinkc.preflight import (
    in_user_namespace,
    _check_rootful,
    _check_pid_host,
    _check_privileged,
    _check_selinux_label,
    check_container_privileges,
)


def _make_inspect_args(**overrides):
    """Return a minimal argparse namespace suitable for _run_inspect tests."""
    args = argparse.Namespace(
        push_to_github=None,
        from_snapshot=None,
        skip_preflight=False,
        skip_unavailable=False,
        host_root=Path("/host"),
        host_root_explicit=False,
        config_diffs=False,
        deep_binary_scan=False,
        query_podman=False,
        baseline_packages=None,
        target_version=None,
        target_image=None,
        user_strategy=None,
        no_baseline=False,
        refine_mode=False,
        original_snapshot=None,
        inspect_only=False,
        output_file=None,
        output_dir=None,
        no_subscription=False,
        validate=False,
        public=False,
        yes=False,
        github_token=None,
        sensitivity=None,
        no_redaction=False,
    )
    for key, value in overrides.items():
        setattr(args, key, value)
    return args


# ---------------------------------------------------------------------------
# in_user_namespace (uid_map parsing)
# ---------------------------------------------------------------------------

def test_in_user_namespace_rootless():
    """Rootless uid_map (inner 0 -> outer 1000) is detected."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = "         0       1000          1\n"
        assert in_user_namespace() is True


def test_in_user_namespace_rootful():
    """Rootful uid_map (0 -> 0) is not flagged."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = "         0          0 4294967295\n"
        assert in_user_namespace() is False


def test_in_user_namespace_no_procfs():
    """Missing /proc/self/uid_map (e.g. macOS) defaults to False."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.side_effect = OSError("not found")
        assert in_user_namespace() is False


# ---------------------------------------------------------------------------
# _check_rootful
# ---------------------------------------------------------------------------

@patch("yoinkc.preflight.in_user_namespace", return_value=False)
def test_check_rootful_ok(_mock):
    assert _check_rootful() is None


@patch("yoinkc.preflight.in_user_namespace", return_value=True)
def test_check_rootful_fails(_mock):
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = "         0       1000          1\n"
        msg = _check_rootful()
    assert msg is not None
    assert "rootless" in msg
    assert "sudo" in msg


# ---------------------------------------------------------------------------
# _check_pid_host
# ---------------------------------------------------------------------------

def test_check_pid_host_systemd():
    """PID 1 is systemd → --pid=host is set."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_bytes.return_value = b"/usr/lib/systemd/systemd\x00--switched-root\x00"
        assert _check_pid_host() is None


def test_check_pid_host_init():
    """PID 1 is /sbin/init → --pid=host is set."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_bytes.return_value = b"/sbin/init\x00"
        assert _check_pid_host() is None


def test_check_pid_host_container_entrypoint():
    """PID 1 is the container entrypoint → --pid=host is NOT set."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_bytes.return_value = b"/usr/local/bin/yoinkc\x00--help\x00"
        msg = _check_pid_host()
    assert msg is not None
    assert "--pid=host" in msg


def test_check_pid_host_python_entrypoint():
    """PID 1 is python → --pid=host is NOT set."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_bytes.return_value = b"/usr/bin/python3\x00-m\x00yoinkc\x00"
        msg = _check_pid_host()
    assert msg is not None
    assert "--pid=host" in msg


def test_check_pid_host_unreadable():
    """Cannot read /proc/1/cmdline → skip (don't fail)."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_bytes.side_effect = OSError("not found")
        assert _check_pid_host() is None


# ---------------------------------------------------------------------------
# _check_privileged
# ---------------------------------------------------------------------------

_STATUS_PRIVILEGED = """\
Name:\tcat
CapEff:\t000001ffffffffff
"""

_STATUS_UNPRIVILEGED = """\
Name:\tcat
CapEff:\t00000000a80425fb
"""

_STATUS_NO_CAPEFF = """\
Name:\tcat
Pid:\t1234
"""


def test_check_privileged_ok():
    """All capabilities set → privileged."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = _STATUS_PRIVILEGED
        assert _check_privileged() is None


def test_check_privileged_missing_cap_sys_admin():
    """CAP_SYS_ADMIN not set → not privileged."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = _STATUS_UNPRIVILEGED
        msg = _check_privileged()
    assert msg is not None
    assert "CAP_SYS_ADMIN" in msg
    assert "--privileged" in msg


def test_check_privileged_no_capeff():
    """No CapEff line → skip."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = _STATUS_NO_CAPEFF
        assert _check_privileged() is None


def test_check_privileged_unreadable():
    """Cannot read /proc/self/status → skip."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.side_effect = OSError("not found")
        assert _check_privileged() is None


# ---------------------------------------------------------------------------
# _check_selinux_label
# ---------------------------------------------------------------------------

def test_check_selinux_unconfined():
    """unconfined_u context → ok."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = "unconfined_u:unconfined_r:unconfined_t:s0-s0:c0.c1023\n"
        assert _check_selinux_label() is None


def test_check_selinux_container_t():
    """container_t context → labels are enforced."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = "system_u:system_r:container_t:s0:c123,c456\n"
        msg = _check_selinux_label()
    assert msg is not None
    assert "SELinux" in msg
    assert "label=disable" in msg


def test_check_selinux_no_selinux():
    """No SELinux (file absent) → skip."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.side_effect = OSError("not found")
        assert _check_selinux_label() is None


def test_check_selinux_empty():
    """Empty context → ok."""
    with patch("yoinkc.preflight.Path") as MockPath:
        MockPath.return_value.read_text.return_value = ""
        assert _check_selinux_label() is None


# ---------------------------------------------------------------------------
# check_container_privileges (integration)
# ---------------------------------------------------------------------------

@patch("yoinkc.preflight._check_rootful", return_value=None)
@patch("yoinkc.preflight._check_pid_host", return_value=None)
@patch("yoinkc.preflight._check_privileged", return_value=None)
@patch("yoinkc.preflight._check_selinux_label", return_value=None)
def test_all_checks_pass(*_mocks):
    assert check_container_privileges() == []


@patch("yoinkc.preflight._check_rootful", return_value="rootless error")
@patch("yoinkc.preflight._check_pid_host", return_value="pid error")
@patch("yoinkc.preflight._check_privileged", return_value=None)
@patch("yoinkc.preflight._check_selinux_label", return_value=None)
def test_multiple_failures(*_mocks):
    errors = check_container_privileges()
    assert len(errors) == 2
    assert "rootless" in errors[0]
    assert "pid" in errors[1]


@patch("yoinkc.preflight._check_rootful", return_value=None)
@patch("yoinkc.preflight._check_pid_host", return_value=None)
@patch("yoinkc.preflight._check_privileged", return_value="cap error")
@patch("yoinkc.preflight._check_selinux_label", return_value="selinux error")
def test_cap_and_selinux_failures(*_mocks):
    errors = check_container_privileges()
    assert len(errors) == 2
    assert "cap" in errors[0]
    assert "selinux" in errors[1]


# ---------------------------------------------------------------------------
# check_podman (native install podman check)
# ---------------------------------------------------------------------------

def test_podman_missing_on_inspect():
    """When podman is absent, check_podman raises with install instructions for both platforms."""
    from yoinkc.preflight import check_podman
    with patch("shutil.which", return_value=None):
        with pytest.raises(RuntimeError) as exc_info:
            check_podman()
    msg = str(exc_info.value)
    assert "dnf install podman" in msg
    assert "brew install podman" in msg


def test_podman_present():
    """When podman is on PATH, check_podman does not raise."""
    from yoinkc.preflight import check_podman
    with patch("shutil.which", return_value="/usr/bin/podman"):
        check_podman()  # no exception


def test_podman_missing_ignored_for_fleet(tmp_path):
    """Fleet pipeline never invokes check_podman."""
    from yoinkc.__main__ import _run_fleet

    args = argparse.Namespace(
        input_dir=tmp_path,
        min_prevalence=90,
        output_file=None,
        output_dir=None,
        json_only=False,
        no_hosts=False,
    )
    with patch("yoinkc.preflight.check_podman") as mock_check:
        _run_fleet(args)
    mock_check.assert_not_called()


def test_podman_missing_ignored_for_refine():
    """Refine pipeline never invokes check_podman."""
    from yoinkc.__main__ import main
    with patch("yoinkc.preflight.check_podman") as mock_check, \
         patch("yoinkc.refine.run_refine", return_value=0):
        main(["refine", "nonexistent.tar.gz"])
    mock_check.assert_not_called()


# ---------------------------------------------------------------------------
# check_root (native install root privilege check)
# ---------------------------------------------------------------------------

def test_native_inspect_requires_root(capsys):
    """Non-root native inspect exits with 'requires root' error."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args()
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_registry_login"),
        patch("os.geteuid", return_value=1000),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 1
    captured = capsys.readouterr()
    assert "yoinkc inspect requires root" in captured.err
    assert "sudo yoinkc inspect" in captured.err


def test_native_inspect_root_passes():
    """Root native inspect proceeds without root error."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args()
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_registry_login"),
        patch("os.geteuid", return_value=0),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0


def test_root_check_skipped_in_container(monkeypatch):
    """Container entrypoint skips root check regardless of euid."""
    from yoinkc.__main__ import _run_inspect

    monkeypatch.setenv("YOINKC_CONTAINER", "1")
    args = _make_inspect_args()
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_root", create=True) as mock_root,
        patch("os.geteuid", return_value=1000),
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_root.assert_not_called()


def test_root_check_skipped_for_fleet(tmp_path):
    """Fleet pipeline never checks for root."""
    from yoinkc.__main__ import _run_fleet

    args = argparse.Namespace(
        input_dir=tmp_path,
        min_prevalence=90,
        output_file=None,
        output_dir=None,
        json_only=False,
        no_hosts=False,
    )
    with patch("yoinkc.preflight.check_root", create=True) as mock_root:
        _run_fleet(args)
    mock_root.assert_not_called()


def test_root_check_skipped_for_refine():
    """Refine pipeline never checks for root."""
    from yoinkc.__main__ import main
    with (
        patch("yoinkc.preflight.check_root", create=True) as mock_root,
        patch("yoinkc.refine.run_refine", return_value=0),
    ):
        main(["refine", "nonexistent.tar.gz"])
    mock_root.assert_not_called()


def test_root_check_skipped_for_from_snapshot():
    """inspect --from-snapshot does not require root."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(from_snapshot=Path("/tmp/snapshot.json"))
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_root", create=True) as mock_root,
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_root.assert_not_called()


def test_root_check_skipped_with_skip_preflight():
    """inspect --skip-preflight does not require root."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(skip_preflight=True)
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_root", create=True) as mock_root,
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_root.assert_not_called()


# ---------------------------------------------------------------------------
# check_registry_login
# ---------------------------------------------------------------------------

def test_registry_login_ok():
    """Already logged in → no prompts or side-effects."""
    from yoinkc.preflight import check_registry_login
    with patch("subprocess.run", return_value=MagicMock(returncode=0, stdout="user\n")):
        check_registry_login()  # must not raise or exit


def test_registry_login_missing_noninteractive(monkeypatch):
    """Not logged in + non-interactive terminal → SystemExit with error."""
    from yoinkc.preflight import check_registry_login
    monkeypatch.setattr(sys, "stdin", MagicMock(isatty=lambda: False))
    with patch("subprocess.run", return_value=MagicMock(returncode=1, stdout="")), \
         pytest.raises(SystemExit):
        check_registry_login()


def test_registry_login_missing_interactive_succeeds(monkeypatch):
    """Not logged in + interactive TTY → prompts login, re-checks, succeeds."""
    from yoinkc.preflight import check_registry_login
    monkeypatch.setattr(sys, "stdin", MagicMock(isatty=lambda: True))
    side_effects = [
        MagicMock(returncode=1, stdout=""),   # initial get-login: not logged in
        MagicMock(returncode=0),              # interactive podman login
        MagicMock(returncode=0, stdout="me"), # recheck: now logged in
    ]
    with patch("subprocess.run", side_effect=side_effects):
        check_registry_login()  # must not raise or exit


# ---------------------------------------------------------------------------
# check_registry_login is inspect-only (not wired into fleet/refine)
# ---------------------------------------------------------------------------

def test_registry_login_check_inspect_only(tmp_path):
    """check_registry_login is never called by the fleet or refine code paths."""
    from yoinkc.__main__ import _run_fleet

    args = argparse.Namespace(
        input_dir=tmp_path,
        min_prevalence=90,
        output_file=None,
        output_dir=None,
        json_only=False,
        no_hosts=False,
    )
    with patch("yoinkc.preflight.check_registry_login") as mock_login:
        _run_fleet(args)
    mock_login.assert_not_called()


# ---------------------------------------------------------------------------
# native inspect login preflight conditions
# ---------------------------------------------------------------------------

def test_registry_login_skipped_with_baseline_packages():
    """Air-gapped baseline file mode should not force registry.redhat.io auth."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(baseline_packages=Path("/tmp/baseline.txt"))
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_login.assert_not_called()


def test_registry_login_skipped_with_no_baseline():
    """No-baseline mode should not force registry.redhat.io auth."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(no_baseline=True)
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_login.assert_not_called()


def test_registry_login_skipped_with_non_redhat_target_image():
    """Mirrored or local target images should not force registry.redhat.io auth."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(target_image="registry.local/rhel-bootc:9.6")
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_login.assert_not_called()


def test_registry_login_skipped_with_mirrored_redhat_path():
    """Repository paths containing registry.redhat.io should not count as that registry host."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(target_image="registry.local/registry.redhat.io/rhel9/rhel-bootc:9.6")
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_login.assert_not_called()


def test_registry_login_runs_for_redhat_target_image():
    """Explicit registry.redhat.io target images still require auth preflight."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(target_image="registry.redhat.io/rhel9/rhel-bootc:9.6")
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_login.assert_called_once_with()


# ---------------------------------------------------------------------------
# packaged install detection helpers
# ---------------------------------------------------------------------------

def test_is_packaged_install_detects_homebrew_cellar(monkeypatch):
    """A Homebrew Cellar install path counts as a packaged install."""
    import yoinkc.preflight as preflight_mod

    monkeypatch.setattr(preflight_mod, "__file__", "/opt/homebrew/Cellar/yoinkc/0.1.0/libexec/lib/python3.13/site-packages/yoinkc/preflight.py")
    with patch.object(preflight_mod, "_PACKAGED_MARKER", Path("/nonexistent/.packaged")):
        assert preflight_mod.is_packaged_install() is True


# ---------------------------------------------------------------------------
# _run_inspect preflight and packaged-install guards
# ---------------------------------------------------------------------------

def test_packaged_install_defaults_to_local_host_root():
    """Native packaged inspect should treat the local host as the default root."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args()
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login"),
        patch("yoinkc.preflight.check_container_privileges", return_value=[] ) as mock_privs,
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot) as mock_pipeline,
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_privs.assert_not_called()
    assert mock_pipeline.call_args.kwargs["host_root"] == Path("/")


def test_packaged_install_preserves_explicit_host_root():
    """An explicit host root should be preserved for packaged installs."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(host_root=Path("/mnt/host"), host_root_explicit=True)
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login"),
        patch("yoinkc.preflight.check_container_privileges", return_value=[] ) as mock_privs,
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot) as mock_pipeline,
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_privs.assert_not_called()
    assert mock_pipeline.call_args.kwargs["host_root"] == Path("/mnt/host")


def test_preflight_skipped_in_container(monkeypatch):
    """Container entrypoint skips native podman/login checks."""
    from yoinkc.__main__ import _run_inspect

    monkeypatch.setenv("YOINKC_CONTAINER", "1")
    args = _make_inspect_args()
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.check_podman") as mock_podman,
        patch("yoinkc.preflight.check_registry_login") as mock_login,
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
    ):
        result = _run_inspect(args)

    assert result == 0
    mock_podman.assert_not_called()
    mock_login.assert_not_called()


def test_push_to_github_unsupported_in_packaged_install(capsys):
    """Missing deps in packaged install emits the packaged-install error."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(push_to_github="owner/repo")
    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch.dict(sys.modules, {"github": None, "git": None}),
    ):
        result = _run_inspect(args)

    assert result == 1
    captured = capsys.readouterr()
    assert "--push-to-github is not supported in packaged installs" in captured.err
    assert "commit and push the output directory manually" in captured.err


def test_push_to_github_missing_extras_in_dev_install(capsys):
    """Missing deps in non-packaged install tells the user to install extras."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(push_to_github="owner/repo")
    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=False),
        patch.dict(sys.modules, {"github": None, "git": None}),
    ):
        result = _run_inspect(args)

    assert result == 1
    captured = capsys.readouterr()
    assert "--push-to-github requires the github extras" in captured.err
    assert "pip install 'yoinkc[github]'" in captured.err


def test_push_to_github_works_with_deps():
    """Importable deps bypass the early guard regardless of install type."""
    from yoinkc.__main__ import _run_inspect

    args = _make_inspect_args(push_to_github="owner/repo", output_dir=Path("/tmp/out"))
    snapshot = MagicMock(redactions=[])

    with (
        patch("yoinkc.preflight.is_packaged_install", return_value=True),
        patch.dict(sys.modules, {"github": MagicMock(), "git": MagicMock()}),
        patch("yoinkc.preflight.check_podman"),
        patch("yoinkc.preflight.check_root"),
        patch("yoinkc.preflight.check_registry_login"),
        patch("yoinkc.preflight.check_container_privileges", return_value=[]),
        patch("yoinkc.__main__.run_pipeline", return_value=snapshot),
        patch("yoinkc.git_github.init_git_repo", return_value=True),
        patch("yoinkc.git_github.add_and_commit", return_value=True),
        patch("yoinkc.git_github.output_stats", return_value=(1, 1, 0)),
        patch("yoinkc.git_github.push_to_github", return_value=None),
    ):
        result = _run_inspect(args)

    assert result == 0
