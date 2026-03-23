"""Regression tests for the run-yoinkc.sh wrapper."""

import os
import stat
import subprocess
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "run-yoinkc.sh"


def _write_executable(path: Path, content: str) -> None:
    path.write_text(content)
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def test_wrapper_does_not_abort_when_hostname_lookup_fails(tmp_path):
    """Missing hostnamectl and hostname -f must not abort the wrapper under set -e."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    args_file = tmp_path / "podman-args.txt"

    _write_executable(
        bin_dir / "podman",
        "#!/bin/sh\nprintf '%s\n' \"$@\" > \"$PODMAN_ARGS_FILE\"\nexit 0\n",
    )
    _write_executable(bin_dir / "hostnamectl", "#!/bin/sh\nexit 1\n")
    _write_executable(bin_dir / "hostname", "#!/bin/sh\nexit 1\n")

    env = os.environ.copy()
    env["PATH"] = f"{bin_dir}:{env['PATH']}"
    env["PODMAN_ARGS_FILE"] = str(args_file)
    env["YOINKC_IMAGE"] = "ghcr.io/marrusl/yoinkc:test"
    env.pop("YOINKC_HOSTNAME", None)

    result = subprocess.run(
        [str(SCRIPT)],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
        timeout=10,
    )

    assert result.returncode == 0, result.stderr
    args = args_file.read_text().splitlines()
    assert any(
        args[i] == "-e" and args[i + 1] == "YOINKC_HOSTNAME="
        for i in range(len(args) - 1)
    ), args
