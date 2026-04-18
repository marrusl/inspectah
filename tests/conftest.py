"""Shared test fixtures and helpers used across split test modules."""

import json
from pathlib import Path
from typing import Optional
from unittest.mock import patch

import pytest
from jinja2 import Environment

import inspectah.preflight as preflight_mod
from inspectah.executor import Executor, RunResult
from inspectah.inspectors import run_all as run_all_inspectors
from inspectah.redact import redact_snapshot
from inspectah.renderers import run_all as run_all_renderers


FIXTURES = Path(__file__).parent / "fixtures"


# ---------------------------------------------------------------------------
# Autouse fixture: mock user namespace check for all tests
# ---------------------------------------------------------------------------

@pytest.fixture(autouse=True)
def _mock_user_namespace():
    """Pretend we are NOT in a user namespace so nsenter probe runs."""
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        yield


# ---------------------------------------------------------------------------
# Renderer helpers (from test_renderer_outputs.py)
# ---------------------------------------------------------------------------

def _make_executor(pkg_list: Optional[str] = None):
    """Return a fixture executor.  If pkg_list is None, the baseline podman call fails."""
    def executor(cmd, cwd=None):
        if cmd[-1] == "true" and "nsenter" in cmd:
            return RunResult(stdout="", stderr="", returncode=0)
        c = " ".join(cmd)
        if "podman" in c and "login" in c and "--get-login" in c:
            return RunResult(stdout="testuser\n", stderr="", returncode=0)
        if "podman" in c and "image" in c and "exists" in c:
            return RunResult(stdout="", stderr="", returncode=0)
        if "podman" in c and "rpm" in c:
            if pkg_list is not None:
                return RunResult(stdout=pkg_list, stderr="", returncode=0)
            return RunResult(stdout="", stderr="Error: podman unavailable", returncode=1)
        if "rpm" in c and "-qa" in c:
            return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
        if "rpm" in c and "-Va" in c:
            return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
        if "dnf" in c and "list" in c:
            return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
        if "dnf" in c and "info" in c and "4" in c:
            return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
        if "rpm" in c and "-ql" in c:
            return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
        if "systemctl" in c:
            return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
        if "semodule" in c and "-l" in c:
            return RunResult(stdout=(FIXTURES / "semodule_l_output.txt").read_text(), stderr="", returncode=0)
        if "semanage" in c and "boolean" in c:
            return RunResult(stdout=(FIXTURES / "semanage_boolean_l_output.txt").read_text(), stderr="", returncode=0)
        if "lsmod" in c:
            return RunResult(stdout=(FIXTURES / "lsmod_output.txt").read_text(), stderr="", returncode=0)
        if "ip" in c and "route" in c:
            return RunResult(stdout=(FIXTURES / "ip_route_output.txt").read_text(), stderr="", returncode=0)
        if "ip" in c and "rule" in c:
            return RunResult(stdout=(FIXTURES / "ip_rule_output.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    return executor


def _build_snapshot(with_baseline: bool):
    pkg_list = (FIXTURES / "base_image_packages_nevra.txt").read_text() if with_baseline else None
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all_inspectors(
            FIXTURES / "host_etc",
            executor=_make_executor(pkg_list),
            no_baseline_opt_in=not with_baseline,
        )
    return redact_snapshot(snapshot)


@pytest.fixture(scope="module")
def outputs_with_baseline(tmp_path_factory):
    """Full renderer outputs built with baseline resolved."""
    tmp = tmp_path_factory.mktemp("with_baseline")
    snapshot = _build_snapshot(with_baseline=True)
    run_all_renderers(snapshot, tmp)
    return {"snapshot": snapshot, "dir": tmp}


@pytest.fixture(scope="module")
def outputs_no_baseline(tmp_path_factory):
    """Full renderer outputs built without baseline (no_baseline=True)."""
    tmp = tmp_path_factory.mktemp("no_baseline")
    snapshot = _build_snapshot(with_baseline=False)
    run_all_renderers(snapshot, tmp)
    return {"snapshot": snapshot, "dir": tmp}


# ---------------------------------------------------------------------------
# Inspector helpers (from test_inspectors.py)
# ---------------------------------------------------------------------------

def _fixture_executor(cmd, cwd=None):
    """Executor that returns fixture file content for known commands."""
    if cmd[-1] == "true" and "nsenter" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    cmd_str = " ".join(cmd)
    if "podman" in cmd and "login" in cmd and "--get-login" in cmd:
        return RunResult(stdout="testuser\n", stderr="", returncode=0)
    if "podman" in cmd and "image" in cmd and "exists" in cmd:
        return RunResult(stdout="", stderr="", returncode=0)
    if "podman" in cmd and "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "base_image_packages_nevra.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qa" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qa_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-Va" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_va_output.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "repoquery" in cmd and "--userinstalled" in cmd:
        return RunResult(stdout="httpd\nrsync\n", stderr="", returncode=0)
    if "dnf" in cmd and "repoquery" in cmd and "--installed" in cmd and "--requires" not in cmd:
        repo_output = "\n".join([
            "httpd baseos",
            "nginx appstream",
            "htop epel",
            "bat epel",
        ])
        return RunResult(stdout=repo_output, stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "list" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_list.txt").read_text(), stderr="", returncode=0)
    if "dnf" in cmd and "history" in cmd and "info" in cmd and "4" in cmd:
        return RunResult(stdout=(FIXTURES / "dnf_history_info_4.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-ql" in cmd:
        return RunResult(stdout=(FIXTURES / "rpm_qla_output.txt").read_text(), stderr="", returncode=0)
    if "rpm" in cmd and "-qf" in cmd:
        assert "--root" not in cmd, (
            f"rpm -qf must use --dbpath, not --root (chroot fails in containers); got: {cmd}"
        )
        path_args = []
        skip_next = False
        for a in cmd:
            if skip_next:
                skip_next = False
                continue
            if a in ("--dbpath", "--root", "--queryformat"):
                skip_next = True
                continue
            if a.startswith("/"):
                path_args.append(a)
        _KNOWN_OWNERS = {"httpd.service": "httpd"}
        names = [_KNOWN_OWNERS.get(Path(p).name, "") for p in path_args]
        if any(names):
            return RunResult(stdout="\n".join(names) + "\n", stderr="", returncode=0)
        return RunResult(
            stdout="",
            stderr=f"file {path_args[-1] if path_args else '?'} is not owned by any package",
            returncode=1,
        )
    if "systemctl" in cmd and "list-unit-files" in cmd:
        return RunResult(stdout=(FIXTURES / "systemctl_list_unit_files.txt").read_text(), stderr="", returncode=0)
    if "semodule" in cmd and "-l" in cmd:
        return RunResult(stdout=(FIXTURES / "semodule_l_output.txt").read_text(), stderr="", returncode=0)
    if "semanage" in cmd and "boolean" in cmd:
        return RunResult(stdout=(FIXTURES / "semanage_boolean_l_output.txt").read_text(), stderr="", returncode=0)
    if "semanage" in cmd and "port" in cmd:
        return RunResult(stdout=(FIXTURES / "semanage_port_l_C_output.txt").read_text(), stderr="", returncode=0)
    if "lsmod" in cmd:
        return RunResult(stdout=(FIXTURES / "lsmod_output.txt").read_text(), stderr="", returncode=0)
    if "ip" in cmd and "route" in cmd:
        return RunResult(stdout=(FIXTURES / "ip_route_output.txt").read_text(), stderr="", returncode=0)
    if "ip" in cmd and "rule" in cmd:
        return RunResult(stdout=(FIXTURES / "ip_rule_output.txt").read_text(), stderr="", returncode=0)
    if "podman" in cmd and "ps" in cmd:
        return RunResult(stdout=(FIXTURES / "podman_ps_output.json").read_text(), stderr="", returncode=0)
    if "podman" in cmd and "inspect" in cmd:
        return RunResult(stdout=(FIXTURES / "podman_inspect_output.json").read_text(), stderr="", returncode=0)
    if "readelf" in cmd and "-S" in cmd:
        if "go-server" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_go_sections.txt").read_text(), stderr="", returncode=0)
        if "rust-worker" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_rust_sections.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="not an ELF", returncode=1)
    if "readelf" in cmd and "-d" in cmd:
        if "go-server" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_go_dynamic.txt").read_text(), stderr="", returncode=0)
        if "rust-worker" in cmd_str:
            return RunResult(stdout=(FIXTURES / "readelf_rust_dynamic.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="not an ELF", returncode=1)
    if "file" in cmd and "-b" in cmd:
        if "go-server" in cmd_str or "rust-worker" in cmd_str:
            return RunResult(stdout="ELF 64-bit LSB executable", stderr="", returncode=0)
        return RunResult(stdout="ASCII text", stderr="", returncode=0)
    if "pip" in cmd and "list" in cmd and "--path" in cmd:
        if "webapp" in cmd_str:
            return RunResult(stdout=(FIXTURES / "pip_list_webapp.txt").read_text(), stderr="", returncode=0)
        if "analytics" in cmd_str:
            return RunResult(stdout=(FIXTURES / "pip_list_analytics.txt").read_text(), stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    return RunResult(stdout="", stderr="unknown command", returncode=1)


@pytest.fixture
def fixture_executor() -> Executor:
    return _fixture_executor


@pytest.fixture
def host_root() -> Path:
    return FIXTURES / "host_etc"


# ---------------------------------------------------------------------------
# Plan items helper (from test_plan_items.py)
# ---------------------------------------------------------------------------

def _env():
    return Environment(autoescape=True)


# ---------------------------------------------------------------------------
# ostree / Silverblue fixture executor
# ---------------------------------------------------------------------------

_RPMOSTREE_STATUS = json.dumps({
    "deployments": [{
        "booted": True,
        "requested-packages": ["httpd", "vim-enhanced"],
        "requested-local-packages": [],
        "packages": [],
        "base-removals": [{"name": "nano", "nevra": "nano-7.2-3.fc41.x86_64"}],
        "base-local-replacements": [],
    }]
})

_FLATPAK_LIST = "org.mozilla.firefox\tflathub\tstable\n"


def _ostree_fixture_executor(cmd, cwd=None):
    """Executor that simulates an rpm-ostree/Silverblue system."""
    cmd_str = " ".join(cmd) if isinstance(cmd, list) else cmd
    if cmd == ["bootc", "status"]:
        return RunResult(stdout="", stderr="not found", returncode=1)
    if cmd == ["rpm-ostree", "status", "--json"]:
        return RunResult(stdout=_RPMOSTREE_STATUS, stderr="", returncode=0)
    if cmd == ["rpm-ostree", "status"]:
        return RunResult(stdout="State: idle", stderr="", returncode=0)
    if "which" in cmd_str and "flatpak" in cmd_str:
        return RunResult(stdout="/usr/bin/flatpak", stderr="", returncode=0)
    if "flatpak" in cmd_str and "list" in cmd_str:
        return RunResult(stdout=_FLATPAK_LIST, stderr="", returncode=0)
    return _fixture_executor(cmd, cwd=cwd)


@pytest.fixture
def ostree_fixture_executor() -> Executor:
    return _ostree_fixture_executor
