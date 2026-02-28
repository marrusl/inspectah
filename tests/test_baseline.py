"""Tests for baseline generation (base image query)."""

from pathlib import Path
from unittest.mock import patch

import yoinkc.baseline as baseline_mod
from yoinkc.baseline import (
    BaselineResolver,
    select_base_image,
    load_baseline_packages_file,
)
from yoinkc.executor import RunResult


FIXTURES = Path(__file__).parent / "fixtures"


def _make_executor(podman_result=None, probe_ok=True):
    """Build a mock executor that handles the nsenter probe and podman commands."""
    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            if probe_ok:
                return RunResult(stdout="", stderr="", returncode=0)
            return RunResult(stdout="", stderr="Operation not permitted", returncode=1)
        if podman_result is not None and "podman" in cmd:
            return podman_result(cmd) if callable(podman_result) else podman_result
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


# ---------------------------------------------------------------------------
# select_base_image / load_baseline_packages_file (pure functions)
# ---------------------------------------------------------------------------

def test_select_base_image_rhel9():
    assert select_base_image("rhel", "9.4") == "registry.redhat.io/rhel9/rhel-bootc:9.4"


def test_select_base_image_centos_stream9():
    assert select_base_image("centos", "9") == "quay.io/centos-bootc/centos-bootc:stream9"


def test_select_base_image_unknown():
    assert select_base_image("fedora", "40") is None


def test_load_baseline_packages_file():
    path = FIXTURES / "base_image_packages.txt"
    names = load_baseline_packages_file(path)
    assert names is not None
    assert "bash" in names
    assert "glibc" in names
    assert len(names) > 10


def test_load_baseline_packages_file_missing(tmp_path):
    assert load_baseline_packages_file(tmp_path / "nope.txt") is None


# ---------------------------------------------------------------------------
# BaselineResolver.get_baseline_packages — file and no-executor paths
# ---------------------------------------------------------------------------

def test_get_baseline_with_file():
    """--baseline-packages FILE loads the file directly, no podman needed."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(None)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
        baseline_packages_file=FIXTURES / "base_image_packages.txt",
    )
    assert no_baseline is False
    assert names is not None
    assert "bash" in names
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


def test_get_baseline_no_executor_no_file():
    """Without executor or file, falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(None)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True


# ---------------------------------------------------------------------------
# BaselineResolver — no global state, each test is independent
# ---------------------------------------------------------------------------

@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_with_podman(_mock_userns):
    """Resolver queries podman when probe succeeds."""
    host_root = FIXTURES / "host_etc"
    pkg_list = (FIXTURES / "base_image_packages.txt").read_text()

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=pkg_list, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(_make_executor(podman_result=podman_handler))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is False
    assert names is not None
    assert "bash" in names
    assert "glibc" in names


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_podman_fails(_mock_userns):
    """When podman fails, resolver falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    podman_err = RunResult(stdout="", stderr="Error: ...", returncode=125)
    resolver = BaselineResolver(_make_executor(podman_result=podman_err))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_nsenter_eperm_falls_back(_mock_userns):
    """nsenter EPERM → probe fails → no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    resolver = BaselineResolver(_make_executor(probe_ok=False))
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "in_user_namespace", return_value=True)
def test_resolver_skipped_in_user_namespace(_mock_userns):
    """User namespace detected → nsenter never attempted, no executor calls."""
    host_root = FIXTURES / "host_etc"
    calls = []

    def tracking_executor(cmd, cwd=None):
        calls.append(cmd)
        return RunResult(stdout="", stderr="", returncode=0)

    resolver = BaselineResolver(tracking_executor)
    names, base_image, no_baseline = resolver.get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True
    assert len(calls) == 0, "No commands should be executed when in user namespace"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_probe_cached(_mock_userns):
    """nsenter probe runs exactly once even when called multiple times."""
    probe_calls = []

    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            probe_calls.append(cmd)
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    resolver = BaselineResolver(executor)
    resolver._probe_nsenter()
    resolver._probe_nsenter()
    resolver._probe_nsenter()
    assert len(probe_calls) == 1, "Probe should be cached after first call"


@patch.object(baseline_mod, "in_user_namespace", return_value=False)
def test_resolver_instances_independent(_mock_userns):
    """Two resolver instances have independent probe caches."""
    r1 = BaselineResolver(_make_executor(probe_ok=True))
    r2 = BaselineResolver(_make_executor(probe_ok=False))
    assert r1._probe_nsenter() is True
    assert r2._probe_nsenter() is False
    # r1's state is unchanged
    assert r1._nsenter_available is True
    assert r2._nsenter_available is False
