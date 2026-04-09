"""Non-RPM software inspector tests for ostree/bootc source systems."""
from pathlib import Path
from yoinkc.executor import RunResult
from yoinkc.schema import SystemType


def _non_rpm_executor(cmd, *, cwd=None):
    return RunResult(stdout="", stderr="", returncode=1)


def test_immutable_usr_local_skipped_on_ostree(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    usr_local = tmp_path / "usr" / "local" / "bin"
    usr_local.mkdir(parents=True)
    (usr_local / "custom-app").write_text("#!/bin/bash\n")
    opt = tmp_path / "opt" / "myapp"
    opt.mkdir(parents=True)
    (opt / "app.py").write_text("print('hello')\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any(p.startswith("usr/local") for p in paths), \
        f"/usr/local found: {[p for p in paths if 'usr/local' in p]}"
    assert any("opt/myapp" in p for p in paths)


def test_immutable_usr_lib_python_skipped_on_ostree(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    pydir = tmp_path / "usr" / "lib" / "python3.12" / "site-packages" / "mylib"
    pydir.mkdir(parents=True)
    (pydir / "__init__.py").write_text("# immutable\n")
    pydir64 = tmp_path / "usr" / "lib64" / "python3.12" / "site-packages" / "mylib64"
    pydir64.mkdir(parents=True)
    (pydir64 / "__init__.py").write_text("# immutable\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any("usr/lib/python3" in p for p in paths), \
        f"/usr/lib/python3 found: {[p for p in paths if 'python3' in p]}"
    assert not any("usr/lib64/python3" in p for p in paths)


def test_ostree_var_internal_paths_skipped(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    for internal in ["var/lib/ostree", "var/lib/rpm-ostree", "var/lib/flatpak"]:
        p = tmp_path / internal / "data"
        p.mkdir(parents=True)
        (p / "file.db").write_text("data")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.RPM_OSTREE)
    paths = [item.path for item in section.items]
    assert not any("lib/ostree" in p for p in paths)
    assert not any("lib/rpm-ostree" in p for p in paths)
    assert not any("lib/flatpak" in p for p in paths)


def test_package_mode_usr_local_still_scanned(tmp_path):
    from yoinkc.inspectors.non_rpm_software import run as run_non_rpm
    usr_local = tmp_path / "usr" / "local" / "bin"
    usr_local.mkdir(parents=True)
    (usr_local / "custom-tool").write_text("#!/bin/bash\n")
    section = run_non_rpm(tmp_path, _non_rpm_executor, system_type=SystemType.PACKAGE_MODE)
    paths = [item.path for item in section.items]
    assert any("usr/local" in p for p in paths)
