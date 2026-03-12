"""Create tarball from rendered output directory."""

import re
import socket
import tarfile
from datetime import datetime
from pathlib import Path


def sanitize_hostname(hostname: str) -> str:
    """Remove characters unsafe for filenames; fall back to 'unknown'."""
    cleaned = re.sub(r"[^\w.-]", "", hostname)
    return cleaned or "unknown"


def _resolve_hostname() -> str:
    """Hostname with fallback chain: socket -> /etc/hostname -> 'unknown'."""
    try:
        name = socket.gethostname()
        if name:
            return name
    except OSError:
        pass
    try:
        name = Path("/etc/hostname").read_text().strip()
        if name:
            return name
    except OSError:
        pass
    return "unknown"


def get_output_stamp() -> str:
    """Return 'HOSTNAME-YYYYMMDD-HHMMSS' stamp for tarball naming."""
    hostname = sanitize_hostname(_resolve_hostname())
    now = datetime.now().strftime("%Y%m%d-%H%M%S")
    return f"{hostname}-{now}"


def create_tarball(source_dir: Path, tarball_path: Path, prefix: str) -> None:
    """Create a gzipped tarball from source_dir with all entries under prefix/.

    Raises OSError if the tarball cannot be written.
    """
    with tarfile.open(tarball_path, "w:gz") as tf:
        for item in sorted(source_dir.rglob("*")):
            arcname = f"{prefix}/{item.relative_to(source_dir)}"
            tf.add(item, arcname=arcname, recursive=False)
