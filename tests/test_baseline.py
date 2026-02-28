"""Tests for baseline generation (base image query)."""

from pathlib import Path
from unittest.mock import patch

import pytest

import yoinkc.baseline as baseline_mod
from yoinkc.baseline import (
    select_base_image,
    load_baseline_packages_file,
    get_baseline_packages,
    _nsenter_probe,
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


def test_get_baseline_with_file():
    """--baseline-packages FILE loads the file directly, no podman needed."""
    host_root = FIXTURES / "host_etc"
    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
        baseline_packages_file=FIXTURES / "base_image_packages.txt",
    )
    assert no_baseline is False
    assert names is not None
    assert "bash" in names
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "_in_user_namespace", return_value=False)
def test_get_baseline_with_podman(_mock_userns):
    """When executor is provided, podman is called to query the base image."""
    host_root = FIXTURES / "host_etc"
    pkg_list = (FIXTURES / "base_image_packages.txt").read_text()

    def podman_handler(cmd):
        if "rpm" in cmd:
            return RunResult(stdout=pkg_list, stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)

    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
        executor=_make_executor(podman_result=podman_handler),
    )
    assert no_baseline is False
    assert names is not None
    assert "bash" in names
    assert "glibc" in names


def test_get_baseline_no_podman_no_file():
    """Without executor or file, falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
    )
    assert no_baseline is True


@patch.object(baseline_mod, "_in_user_namespace", return_value=False)
def test_get_baseline_podman_fails(_mock_userns):
    """When podman fails, falls back to no-baseline mode."""
    host_root = FIXTURES / "host_etc"

    podman_err = RunResult(stdout="", stderr="Error: ...", returncode=125)
    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
        executor=_make_executor(podman_result=podman_err),
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "_in_user_namespace", return_value=False)
def test_nsenter_probe_eperm_falls_back(_mock_userns):
    """nsenter EPERM (rootless container) → probe fails, no-baseline mode."""
    host_root = FIXTURES / "host_etc"
    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
        executor=_make_executor(probe_ok=False),
    )
    assert no_baseline is True
    assert base_image == "quay.io/centos-bootc/centos-bootc:stream9"


@patch.object(baseline_mod, "_in_user_namespace", return_value=True)
def test_nsenter_skipped_in_user_namespace(_mock_userns):
    """When inside a user namespace, nsenter is skipped entirely (no probe)."""
    host_root = FIXTURES / "host_etc"
    calls = []

    def tracking_executor(cmd, cwd=None):
        calls.append(cmd)
        return RunResult(stdout="", stderr="", returncode=0)

    names, base_image, no_baseline = get_baseline_packages(
        host_root, "centos", "9",
        executor=tracking_executor,
    )
    assert no_baseline is True
    assert len(calls) == 0, "No commands should be executed when in user namespace"
