"""End-to-end integration tests for ostree/bootc source scanning."""

import json
from pathlib import Path
from unittest.mock import patch

import pytest

from yoinkc.executor import RunResult
from yoinkc.inspectors import run_all
from yoinkc.renderers import run_all as run_all_renderers
from yoinkc.schema import SystemType
import yoinkc.preflight as preflight_mod


def _setup_silverblue_root(tmp_path):
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text(
        'NAME="Fedora Linux"\nVERSION_ID=41\nID=fedora\n'
        'VARIANT_ID=silverblue\nPRETTY_NAME="Fedora Linux 41 (Silverblue)"\n'
    )
    (etc / "hostname").write_text("silverblue-test\n")
    usr_etc = tmp_path / "usr" / "etc"
    (usr_etc / "ssh").mkdir(parents=True)
    (usr_etc / "ssh" / "sshd_config").write_text("Port 22\nPermitRootLogin yes\n")
    (etc / "ssh").mkdir()
    (etc / "ssh" / "sshd_config").write_text("Port 2222\nPermitRootLogin no\n")
    (etc / "resolv.conf").write_text("nameserver 8.8.8.8\n")
    (etc / "machine-id").write_text("abc123\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("BOOT_IMAGE=/vmlinuz root=/dev/sda2 ro rhgb quiet")
    return tmp_path


def test_full_pipeline_silverblue(tmp_path, ostree_fixture_executor):
    host_root = _setup_silverblue_root(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=ostree_fixture_executor, no_baseline_opt_in=True)

    assert snapshot.system_type == SystemType.RPM_OSTREE
    assert snapshot.os_release.variant_id == "silverblue"

    # RPM: no rpm -Va, layered packages captured
    assert snapshot.rpm is not None
    assert snapshot.rpm.rpm_va == []
    added_names = [p.name for p in snapshot.rpm.packages_added]
    assert "httpd" in added_names
    assert "vim-enhanced" in added_names

    # Config: sshd_config detected, volatile files skipped
    if snapshot.config:
        config_paths = [f.path for f in snapshot.config.files]
        assert "etc/ssh/sshd_config" in config_paths
        assert "etc/resolv.conf" not in config_paths
        assert "etc/machine-id" not in config_paths

    # Flatpak: apps detected
    if snapshot.containers:
        app_ids = [a.app_id for a in snapshot.containers.flatpak_apps]
        assert "org.mozilla.firefox" in app_ids


def test_full_pipeline_renders_containerfile(tmp_path, ostree_fixture_executor):
    """Pipeline + renderer produces valid Containerfile + flatpaks.list."""
    host_root = _setup_silverblue_root(tmp_path)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(host_root, executor=ostree_fixture_executor, no_baseline_opt_in=True)
    output_dir = tmp_path / "output"
    output_dir.mkdir()
    run_all_renderers(snapshot, output_dir)
    containerfile = output_dir / "Containerfile"
    assert containerfile.exists()
    content = containerfile.read_text()
    assert "FROM " in content
    assert "dnf install" in content or "dnf remove" in content


def test_refusal_path_integration(tmp_path):
    """Unknown ostree system without --target-image -> exit with spec error."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('NAME="CustomOS"\nVERSION_ID=1.0\nID=custom\nVARIANT_ID=custom\n')
    (etc / "hostname").write_text("test\n")
    def executor(cmd, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="", stderr="", returncode=1)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        with pytest.raises(SystemExit) as exc_info:
            run_all(tmp_path, executor=executor)
    assert exc_info.value.code == 1


def test_pure_bootc_pipeline_warns(tmp_path):
    """Pure bootc system without rpm-ostree emits low-confidence warning."""
    (tmp_path / "ostree").mkdir()
    etc = tmp_path / "etc"
    etc.mkdir()
    (etc / "os-release").write_text('NAME="Fedora"\nVERSION_ID=41\nID=fedora\n')
    (etc / "hostname").write_text("bootc-test\n")
    proc = tmp_path / "proc"
    proc.mkdir()
    (proc / "cmdline").write_text("root=/dev/sda2")
    def executor(cmd, cwd=None):
        if cmd == ["bootc", "status"]:
            return RunResult(stdout="ok", stderr="", returncode=0)
        if cmd == ["bootc", "status", "--json"]:
            return RunResult(
                stdout=json.dumps({"status": {"booted": {"image": {"image": {"image": "quay.io/fedora/fedora-bootc:41"}}}}}),
                stderr="", returncode=0,
            )
        if cmd == ["rpm-ostree", "status", "--json"]:
            return RunResult(stdout="", stderr="not found", returncode=127)
        if cmd == ["rpm-ostree", "status"]:
            return RunResult(stdout="", stderr="not found", returncode=127)
        if "rpm" in cmd and "-qa" in cmd:
            return RunResult(stdout="0:bash-5.2.15-2.fc41.x86_64\n", stderr="", returncode=0)
        if "nsenter" in " ".join(cmd):
            return RunResult(stdout="", stderr="", returncode=0)
        return RunResult(stdout="", stderr="", returncode=1)
    with patch.object(preflight_mod, "in_user_namespace", return_value=False):
        snapshot = run_all(tmp_path, executor=executor, no_baseline_opt_in=True)
    assert snapshot.system_type == SystemType.BOOTC
    warning_msgs = [w.get("message", "") for w in snapshot.warnings]
    assert any("approximate" in m.lower() or "rpm-ostree" in m.lower() for m in warning_msgs), \
        f"Expected low-confidence warning, got: {warning_msgs}"
