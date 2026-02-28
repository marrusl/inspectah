"""
Baseline generation by querying the target bootc base image.

Detects the host OS from /etc/os-release, maps to the corresponding bootc
base image, and runs ``podman run --rm <image> rpm -qa --queryformat '%{NAME}\n'``
to get the concrete package list.  The diff against host packages produces
exactly the ``dnf install`` list the Containerfile needs.

When running inside a container (the normal case), podman is not available
directly. The tool uses ``nsenter -t 1 -m -u -i -n`` to execute podman in
the host's namespaces.  This requires ``--pid=host`` on the outer container.
"""

import os
import sys
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple

from .preflight import in_user_namespace

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] baseline: {msg}", file=sys.stderr)


# ---------------------------------------------------------------------------
# OS → base image mapping  (pure functions — no state)
# ---------------------------------------------------------------------------

_IMAGE_MAP: Dict[str, str] = {
    "rhel-9": "registry.redhat.io/rhel9/rhel-bootc",
    "centos-9": "quay.io/centos-bootc/centos-bootc",
}


def select_base_image(os_id: str, version_id: str) -> Optional[str]:
    """Map host OS identity to the bootc base image reference.

    Returns a fully-qualified image:tag string, or None if unmapped.
    """
    os_id = os_id.lower()
    major = version_id.split(".")[0] if version_id else ""

    if os_id == "rhel" and major == "9":
        return f"registry.redhat.io/rhel9/rhel-bootc:{version_id}"
    if "centos" in os_id and major == "9":
        return "quay.io/centos-bootc/centos-bootc:stream9"

    _debug(f"no base image mapping for os_id={os_id} version_id={version_id}")
    return None


def load_baseline_packages_file(path: Path) -> Optional[Set[str]]:
    """Read a newline-separated package name list from *path*."""
    path = Path(path)
    if not path.exists():
        _debug(f"baseline packages file not found: {path}")
        return None
    try:
        text = path.read_text()
    except (PermissionError, OSError) as exc:
        _debug(f"cannot read baseline packages file: {exc}")
        return None
    names = {line.strip() for line in text.splitlines() if line.strip()}
    _debug(f"loaded {len(names)} baseline package names from {path}")
    return names


# ---------------------------------------------------------------------------
# BaselineResolver — holds nsenter probe cache as instance state
# ---------------------------------------------------------------------------

class BaselineResolver:
    """Encapsulates baseline querying for one inspection run.

    The nsenter probe result is cached as instance state (``_nsenter_available``)
    so it is local to a single run and never leaks between tests or concurrent
    invocations.

    Parameters
    ----------
    executor:
        The executor callable used to run subprocesses.  May be None, in which
        case all podman queries are skipped.
    """

    def __init__(self, executor) -> None:
        self._executor = executor
        self._nsenter_available: Optional[bool] = None

    # ------------------------------------------------------------------
    # nsenter probe
    # ------------------------------------------------------------------

    def _probe_nsenter(self) -> bool:
        """Return True if nsenter into PID 1 namespaces is available.

        Cached after the first call — runs at most once per resolver instance.
        """
        if self._nsenter_available is not None:
            return self._nsenter_available

        if in_user_namespace():
            _debug("running inside a user namespace (rootless container) — "
                   "nsenter into host namespaces will not work. "
                   "Run the container with 'sudo podman run …' or provide "
                   "--baseline-packages FILE.")
            self._nsenter_available = False
            return False

        probe = ["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--", "true"]
        _debug(f"nsenter probe: {' '.join(probe)}")
        result = self._executor(probe)
        if result.returncode != 0:
            stderr = result.stderr.strip()
            if "Operation not permitted" in stderr:
                _debug(f"nsenter probe failed (EPERM): {stderr}. "
                       "This typically means the container is rootless. "
                       "Run with 'sudo podman run …' or provide "
                       "--baseline-packages FILE.")
            elif "No such process" in stderr:
                _debug(f"nsenter probe failed: {stderr}. "
                       "Is --pid=host set on the container?")
            else:
                _debug(f"nsenter probe failed (rc={result.returncode}): {stderr}")
            self._nsenter_available = False
            return False

        _debug("nsenter probe succeeded")
        self._nsenter_available = True
        return True

    def _run_on_host(self, cmd: List[str]):
        """Run *cmd* via nsenter into PID 1's namespaces.

        Returns the RunResult, or None if nsenter is not available.
        """
        if not self._probe_nsenter():
            return None
        nsenter_cmd = ["nsenter", "-t", "1", "-m", "-u", "-i", "-n", "--"] + cmd
        _debug(f"nsenter cmd: {' '.join(nsenter_cmd)}")
        result = self._executor(nsenter_cmd)
        if result.returncode == 127:
            _debug("nsenter failed (rc=127) — is --pid=host set on the container?")
        return result

    # ------------------------------------------------------------------
    # Podman queries
    # ------------------------------------------------------------------

    def query_packages(self, base_image: str) -> Optional[Set[str]]:
        """Run ``podman run --rm <base_image> rpm -qa`` via nsenter.

        Returns the set of package names in the base image, or None on failure.
        """
        cmd = [
            "podman", "run", "--rm", "--cgroups=disabled", base_image,
            "rpm", "-qa", "--queryformat", r"%{NAME}\n",
        ]
        _debug(f"querying base image: {' '.join(cmd)}")
        result = self._run_on_host(cmd)
        if result is None:
            return None
        if result.returncode != 0:
            _debug(f"podman run failed (rc={result.returncode}): "
                   f"{result.stderr.strip()[:800]}")
            return None
        names: Set[str] = set()
        for line in result.stdout.splitlines():
            name = line.strip()
            if name:
                names.add(name)
        _debug(f"base image has {len(names)} packages")
        return names

    def query_presets(self, base_image: str) -> Optional[str]:
        """Dump all systemd preset content from the base image via nsenter.

        Returns the concatenated preset text, or None on failure.
        """
        cmd = [
            "podman", "run", "--rm", "--cgroups=disabled", base_image,
            "bash", "-c",
            "cat /usr/lib/systemd/system-preset/*.preset 2>/dev/null || true",
        ]
        _debug(f"querying base image presets: {' '.join(cmd)}")
        result = self._run_on_host(cmd)
        if result is None:
            return None
        if result.returncode != 0:
            _debug(f"preset query failed (rc={result.returncode}): "
                   f"{result.stderr.strip()[:200]}")
            return None
        text = result.stdout.strip()
        if not text:
            _debug("base image returned no preset data")
            return None
        _debug(f"base image presets: {len(text.splitlines())} lines")
        return result.stdout

    # ------------------------------------------------------------------
    # Top-level entry point
    # ------------------------------------------------------------------

    def get_baseline_packages(
        self,
        host_root: Path,
        os_id: str,
        version_id: str,
        baseline_packages_file: Optional[Path] = None,
    ) -> Tuple[Optional[Set[str]], Optional[str], bool]:
        """Resolve the baseline package set.

        Returns ``(package_names, base_image_ref, no_baseline)``.

        Strategy (in priority order):
        1. ``--baseline-packages FILE`` — load from file (air-gapped).
        2. Query the target bootc base image via podman.
        3. Fall back to no-baseline mode.
        """
        # 1. Explicit file override
        if baseline_packages_file:
            names = load_baseline_packages_file(baseline_packages_file)
            if names:
                base_image = select_base_image(os_id, version_id)
                return (names, base_image, False)
            _debug("baseline packages file provided but empty/unreadable")

        # 2. Query the base image
        base_image = select_base_image(os_id, version_id)
        if base_image and self._executor is not None:
            names = self.query_packages(base_image)
            if names:
                return (names, base_image, False)

        # 3. No baseline
        _debug("no baseline available")
        return (None, base_image, True)


