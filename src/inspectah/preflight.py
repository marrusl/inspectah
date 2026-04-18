"""
Preflight checks for container environment.

Detects whether inspectah is running with the required container flags:
  - rootful (sudo)        — uid 0 maps to host uid 0
  - --pid=host            — PID 1 is the host init, not container entrypoint
  - --privileged          — full capability set (CAP_SYS_ADMIN for nsenter)
  - --security-opt label=disable — SELinux not confining the container

These checks only apply when running inside a container (host_root != "/").
"""

import os
from pathlib import Path
from typing import List, Optional

from ._util import debug as _debug_fn


def _debug(msg: str) -> None:
    _debug_fn("preflight", msg)


_REGISTRY = "registry.redhat.io"
_PACKAGED_MARKER = Path("/usr/share/inspectah/.packaged")


def is_container() -> bool:
    """Return True when running inside the published inspectah container image."""
    return os.environ.get("INSPECTAH_CONTAINER") == "1"


def is_packaged_install() -> bool:
    """Return True for RPM/Homebrew installs that ship a packaged marker file."""
    if _PACKAGED_MARKER.is_file():
        return True

    # Homebrew installs live under a Cellar prefix rather than /usr. Walk upward
    # from the installed module path and look for the shared marker alongside it.
    for parent in Path(__file__).resolve().parents:
        if (parent / "share" / "inspectah" / ".packaged").is_file():
            return True

    # The Homebrew formula lives outside this repo, so also recognize the
    # conventional Cellar install path even before the formula grows a marker.
    parts = Path(__file__).resolve().parts
    for idx, part in enumerate(parts[:-1]):
        if part == "Cellar" and idx + 1 < len(parts) and parts[idx + 1] == "inspectah":
            return True

    return False


def requires_registry_login(
    *,
    target_image: str | None,
    baseline_packages_file: Path | None,
    no_baseline: bool,
) -> bool:
    """Return True when inspect will need auth for registry.redhat.io.

    Air-gapped modes don't query the remote base image at all, and explicit
    non-Red Hat images may come from a mirrored or local registry.
    """
    if no_baseline or baseline_packages_file is not None:
        return False

    if target_image is not None:
        first_component = target_image.split("/", 1)[0]
        if "." in first_component or ":" in first_component or first_component == "localhost":
            return first_component == _REGISTRY
        return False

    # Automatic target-image resolution currently maps to Red Hat-hosted images.
    return True


# ---------------------------------------------------------------------------
# Individual checks
# ---------------------------------------------------------------------------

def in_user_namespace() -> bool:
    """Return True if running inside a non-root user namespace.

    Rootless podman creates a user namespace where inner uid 0 maps to an
    unprivileged host uid.  ``nsenter -t 1`` requires real ``CAP_SYS_ADMIN``
    in the *target* namespace, which is impossible from a user namespace.
    """
    try:
        text = Path("/proc/self/uid_map").read_text()
        for line in text.strip().splitlines():
            parts = line.split()
            if len(parts) >= 3 and parts[0] == "0" and parts[1] != "0":
                return True
    except (OSError, ValueError):
        pass
    return False


def _check_rootful() -> Optional[str]:
    """Check that the container is rootful (not in a user namespace)."""
    if not in_user_namespace():
        _debug("rootful: ok")
        return None
    try:
        text = Path("/proc/self/uid_map").read_text()
        host_uid = text.split()[1]
    except (OSError, IndexError):
        host_uid = "?"
    _debug(f"rootful: FAIL (uid 0 maps to host uid {host_uid})")
    return (
        f"Container is running rootless (uid 0 maps to host uid {host_uid}). "
        "inspectah requires a rootful container. Run with: sudo podman run …"
    )


def _check_pid_host() -> Optional[str]:
    """Check that --pid=host is set (PID 1 is the host init, not the container entrypoint)."""
    try:
        cmdline = Path("/proc/1/cmdline").read_bytes()
        argv0 = cmdline.split(b"\x00")[0].decode("utf-8", errors="replace")
    except (OSError, IndexError):
        _debug("pid=host: cannot read /proc/1/cmdline, skipping")
        return None

    basename = os.path.basename(argv0)
    host_inits = {"systemd", "init", "launchd"}
    if basename in host_inits or argv0 in ("/sbin/init", "/usr/lib/systemd/systemd"):
        _debug(f"pid=host: ok (PID 1 is {argv0})")
        return None

    _debug(f"pid=host: FAIL (PID 1 is {argv0!r})")
    return (
        f"PID namespace is not shared (PID 1 is {basename!r}, expected host init). "
        "Add --pid=host to the podman run command."
    )


_CAP_SYS_ADMIN = 21


def _check_privileged() -> Optional[str]:
    """Check that --privileged is set (full capability set including CAP_SYS_ADMIN)."""
    try:
        text = Path("/proc/self/status").read_text()
    except OSError:
        _debug("privileged: cannot read /proc/self/status, skipping")
        return None

    cap_eff = None
    for line in text.splitlines():
        if line.startswith("CapEff:"):
            cap_eff = line.split(":", 1)[1].strip()
            break

    if cap_eff is None:
        _debug("privileged: CapEff not found in /proc/self/status, skipping")
        return None

    try:
        cap_bits = int(cap_eff, 16)
    except ValueError:
        _debug(f"privileged: cannot parse CapEff={cap_eff!r}, skipping")
        return None

    if cap_bits & (1 << _CAP_SYS_ADMIN):
        _debug(f"privileged: ok (CapEff={cap_eff})")
        return None

    _debug(f"privileged: FAIL (CapEff={cap_eff}, CAP_SYS_ADMIN not set)")
    return (
        "Container is missing CAP_SYS_ADMIN (needed for nsenter). "
        "Add --privileged to the podman run command."
    )


def _check_selinux_label() -> Optional[str]:
    """Check that --security-opt label=disable is set (not confined by SELinux)."""
    attr_path = Path("/proc/self/attr/current")
    try:
        context = attr_path.read_text().strip().rstrip("\x00")
    except OSError:
        _debug("selinux label: /proc/self/attr/current not readable, skipping (no SELinux)")
        return None

    if not context or "unconfined" in context:
        _debug(f"selinux label: ok ({context!r})")
        return None

    if "container_t" in context:
        _debug(f"selinux label: FAIL ({context!r})")
        return (
            f"Container is confined by SELinux ({context}). "
            "Add --security-opt label=disable to the podman run command."
        )

    _debug(f"selinux label: ok (context={context!r} does not contain container_t)")
    return None


# ---------------------------------------------------------------------------
# Public entry point: container privilege checks
# ---------------------------------------------------------------------------

def check_container_privileges() -> List[str]:
    """Run all preflight checks. Returns a list of error strings (empty = all good)."""
    errors: List[str] = []
    for check in (_check_rootful, _check_pid_host, _check_privileged, _check_selinux_label):
        msg = check()
        if msg:
            errors.append(msg)
    return errors


# ---------------------------------------------------------------------------
# Native install preflight: podman availability and registry login
# ---------------------------------------------------------------------------


def check_root() -> None:
    """Verify running as root for native inspect.

    Native (RPM/Homebrew) installs need real root to nsenter into host
    namespaces and read protected paths.  Raises RuntimeError with sudo
    instructions if euid is not 0.
    """
    if os.geteuid() != 0:
        raise RuntimeError(
            "inspectah inspect requires root. Run with: sudo inspectah inspect"
        )
    _debug("root: ok")


def check_podman() -> None:
    """Verify podman is on PATH.

    Called by the inspect subcommand for native (RPM/Homebrew) installs.
    Raises RuntimeError with platform-appropriate install instructions if missing.
    """
    import shutil
    if shutil.which("podman") is None:
        raise RuntimeError(
            "inspectah inspect requires podman, which was not found on PATH.\n"
            "Install it with:\n"
            "  dnf install podman   # Fedora / RHEL / CentOS\n"
            "  brew install podman  # macOS"
        )
    _debug("podman: ok")


def check_registry_login() -> None:
    """Verify authentication with registry.redhat.io.

    If already logged in, returns immediately. Otherwise:
    - Interactive terminal: prompts ``podman login`` and re-checks.
    - Non-interactive: prints instructions and exits with code 1.
    """
    import subprocess
    import sys

    result = subprocess.run(
        ["podman", "login", "--get-login", _REGISTRY],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        _debug(f"registry login: ok ({result.stdout.strip()!r})")
        return

    _debug(f"registry login: not authenticated with {_REGISTRY}")

    if sys.stdin.isatty():
        print(
            f"\nNot logged in to {_REGISTRY}. Running: podman login {_REGISTRY}",
            file=sys.stderr,
        )
        subprocess.run(["podman", "login", _REGISTRY])
        recheck = subprocess.run(
            ["podman", "login", "--get-login", _REGISTRY],
            capture_output=True,
        )
        if recheck.returncode != 0:
            print(
                f"Error: still not authenticated with {_REGISTRY}.\n"
                f"Run: podman login {_REGISTRY}",
                file=sys.stderr,
            )
            sys.exit(1)
    else:
        print(
            f"Error: not authenticated with {_REGISTRY}.\n"
            f"Run: podman login {_REGISTRY}",
            file=sys.stderr,
        )
        sys.exit(1)
