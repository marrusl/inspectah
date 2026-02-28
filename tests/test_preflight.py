"""Tests for container preflight checks."""

from unittest.mock import patch, mock_open

import pytest

from yoinkc.preflight import (
    in_user_namespace,
    _check_rootful,
    _check_pid_host,
    _check_privileged,
    _check_selinux_label,
    check_container_privileges,
)


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
