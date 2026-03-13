"""Create tarball from rendered output directory."""

import re
import socket
import tarfile
from datetime import datetime
from pathlib import Path
from typing import Optional


def sanitize_hostname(hostname: str) -> str:
    """Remove characters unsafe for filenames; fall back to 'unknown'."""
    cleaned = re.sub(r"[^\w.-]", "", hostname)
    return cleaned or "unknown"


def _resolve_hostname(host_root: Optional[Path] = None) -> str:
    """Hostname with fallback chain: {host_root}/etc/hostname -> socket -> 'unknown'.

    When running inside a container, socket.gethostname() returns the container
    ID rather than the inspected host's name. Reading from {host_root}/etc/hostname
    (where the host filesystem is bind-mounted) gives the correct hostname.
    """
    if host_root is not None:
        try:
            name = (host_root / "etc" / "hostname").read_text().splitlines()[0].strip()
            if name:
                return name
        except (OSError, IndexError):
            pass
    try:
        name = socket.gethostname()
        if name:
            return name
    except OSError:
        pass
    return "unknown"


def get_output_stamp(
    host_root: Optional[Path] = None,
    hostname: Optional[str] = None,
) -> str:
    """Return 'HOSTNAME-YYYYMMDD-HHMMSS' stamp for tarball naming.

    If hostname is provided it is used directly (after sanitizing), bypassing
    filesystem and socket resolution. This lets callers supply the hostname
    already captured in the inspection snapshot, which is more reliable than
    re-reading /etc/hostname (empty on RHEL hosts that use hostnamectl).
    """
    if hostname:
        resolved = sanitize_hostname(hostname)
    else:
        resolved = sanitize_hostname(_resolve_hostname(host_root))
    now = datetime.now().strftime("%Y%m%d-%H%M%S")
    return f"{resolved}-{now}"


def create_tarball(source_dir: Path, tarball_path: Path, prefix: str) -> None:
    """Create a gzipped tarball from source_dir with all entries under prefix/.

    Raises OSError if the tarball cannot be written.
    """
    with tarfile.open(tarball_path, "w:gz") as tf:
        for item in sorted(source_dir.rglob("*")):
            arcname = f"{prefix}/{item.relative_to(source_dir)}"
            tf.add(item, arcname=arcname, recursive=False)
