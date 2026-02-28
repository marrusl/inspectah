"""Container inspector: quadlet units, compose files, optional podman query.

Parses Image= from quadlet .container files, image: from compose YAML,
and optionally runs podman inspect for live container details.
"""

import json
import os
import re
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..executor import Executor
from ..schema import ContainerSection
from . import filtered_rglob

_DEBUG = bool(os.environ.get("YOINKC_DEBUG", ""))


def _debug(msg: str) -> None:
    if _DEBUG:
        print(f"[yoinkc] container: {msg}", file=sys.stderr)


def _safe_glob(d: Path, pattern: str) -> List[Path]:
    try:
        return list(d.glob(pattern))
    except (PermissionError, OSError):
        return []


def _safe_rglob(d: Path, pattern: str) -> List[Path]:
    try:
        return list(d.rglob(pattern))
    except (PermissionError, OSError):
        return []


def _safe_read(p: Path) -> str:
    try:
        return p.read_text()
    except (PermissionError, OSError) as exc:
        _debug(f"cannot read {p}: {exc}")
        return ""


def _extract_quadlet_image(content: str) -> str:
    """Extract the Image= value from a quadlet .container file."""
    for line in content.splitlines():
        stripped = line.strip()
        if stripped.lower().startswith("image") and "=" in stripped:
            key, _, val = stripped.partition("=")
            if key.strip().lower() == "image":
                return val.strip()
    return ""


def _extract_compose_images(content: str) -> List[Dict[str, str]]:
    """Extract image: fields from a compose YAML without requiring PyYAML.

    Returns a list of {service, image} dicts. Uses simple line-by-line
    parsing to avoid adding a dependency.  Detects service-level indent
    dynamically so 2-space, 4-space, and tab-indented files all work.
    """
    results: List[Dict[str, str]] = []
    lines = content.splitlines()
    current_service = ""
    in_services = False
    service_indent: Optional[int] = None  # indent of the first service key seen

    for line in lines:
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue

        indent = len(line) - len(line.lstrip())

        if stripped == "services:" or stripped.startswith("services:"):
            in_services = True
            service_indent = None
            continue

        # Top-level key that isn't services — exit services block
        if indent == 0 and ":" in stripped and not stripped.startswith("-"):
            key = stripped.split(":")[0].strip()
            if key != "services":
                in_services = False
                service_indent = None
                current_service = ""
                continue

        if not in_services:
            continue

        # Detect service-block indent from the first indented key
        if service_indent is None and indent > 0:
            service_indent = indent

        if service_indent is not None:
            if indent == service_indent and stripped.endswith(":") and not stripped.startswith("-"):
                current_service = stripped.rstrip(":")
                continue

        if current_service:
            m = re.match(r"image:\s*(.+)", stripped)
            if m:
                image_ref = m.group(1).strip().strip("'\"")
                results.append({"service": current_service, "image": image_ref})

    return results


def _parse_podman_inspect(data: List[dict]) -> List[dict]:
    """Normalize podman inspect JSON into our schema format."""
    results: List[dict] = []
    for c in data:
        mounts = []
        for m in (c.get("Mounts") or []):
            mounts.append({
                "type": m.get("Type", ""),
                "source": m.get("Source", ""),
                "destination": m.get("Destination", ""),
                "mode": m.get("Mode", ""),
                "rw": m.get("RW", True),
            })

        net_settings = c.get("NetworkSettings") or {}
        networks = {}
        for net_name, net_info in (net_settings.get("Networks") or {}).items():
            networks[net_name] = {
                "ip": net_info.get("IPAddress", ""),
                "gateway": net_info.get("Gateway", ""),
                "mac": net_info.get("MacAddress", ""),
            }
        ports = net_settings.get("Ports") or {}

        env_list = (c.get("Config") or {}).get("Env") or []

        state = c.get("State") or {}

        results.append({
            "id": c.get("Id", ""),
            "name": c.get("Name", ""),
            "image": c.get("Image", ""),
            "image_id": c.get("ImageID", ""),
            "status": state.get("Status", ""),
            "mounts": mounts,
            "networks": networks,
            "ports": ports,
            "env": env_list,
        })
    return results


def run(
    host_root: Path,
    executor: Optional[Executor],
    query_podman: bool = False,
) -> ContainerSection:
    section = ContainerSection()
    host_root = Path(host_root)

    # --- Quadlet units ---
    quadlet_dirs = [
        "etc/containers/systemd",
        "usr/share/containers/systemd",
        "etc/systemd/system",
    ]

    # User-level quadlets: ~/.config/containers/systemd/ for each real user
    passwd = host_root / "etc/passwd"
    try:
        if passwd.exists():
            for line in passwd.read_text().splitlines():
                parts = line.split(":")
                if len(parts) >= 7:
                    try:
                        uid = int(parts[2])
                    except ValueError:
                        continue
                    if 1000 <= uid < 60000:
                        home = parts[5].lstrip("/")
                        quadlet_dirs.append(
                            f"{home}/.config/containers/systemd"
                        )
    except (PermissionError, OSError):
        pass

    for subdir in quadlet_dirs:
        d = host_root / subdir
        try:
            exists = d.exists()
        except (PermissionError, OSError):
            exists = False
        if not exists:
            continue
        for f in _safe_glob(d, "*.container"):
            content = _safe_read(f)
            _debug(f"quadlet: {f} ({len(content)} bytes)")
            image_ref = _extract_quadlet_image(content)
            _debug(f"quadlet: {f.name} Image={image_ref!r}")
            if not image_ref and content:
                _debug(f"quadlet: no Image= found, first 5 lines: {content.splitlines()[:5]}")
            section.quadlet_units.append({
                "path": str(f.relative_to(host_root)),
                "name": f.name,
                "content": content,
                "image": image_ref,
            })

    # --- Compose files ---
    for search_dir in ("opt", "srv", "etc"):
        d = host_root / search_dir
        if not d.exists():
            continue
        for pattern in ("docker-compose*.yml", "docker-compose*.yaml",
                        "compose*.yml", "compose*.yaml"):
            for f in filtered_rglob(d, pattern):
                if not f.is_file():
                    continue
                content = _safe_read(f)
                images = _extract_compose_images(content)
                section.compose_files.append({
                    "path": str(f.relative_to(host_root)),
                    "images": images,
                })

    # --- Podman query ---
    if query_podman and executor:
        # podman ps for the container list
        r = executor(["podman", "ps", "-a", "--format", "json"])
        if r.returncode == 0 and r.stdout.strip():
            try:
                ps_data = json.loads(r.stdout)
            except json.JSONDecodeError:
                ps_data = []

            if isinstance(ps_data, list) and ps_data:
                container_ids = [c.get("ID", "") for c in ps_data if c.get("ID")]
                if container_ids:
                    ir = executor(["podman", "inspect"] + container_ids)
                    if ir.returncode == 0 and ir.stdout.strip():
                        try:
                            inspect_data = json.loads(ir.stdout)
                            if isinstance(inspect_data, list):
                                section.running_containers = _parse_podman_inspect(inspect_data)
                        except json.JSONDecodeError:
                            pass

                # Fallback: if inspect failed, use ps data
                if not section.running_containers:
                    for c in ps_data:
                        if isinstance(c, dict):
                            section.running_containers.append({
                                "id": c.get("ID", ""),
                                "name": c.get("Names", [""])[0] if isinstance(c.get("Names"), list) else str(c.get("Names", "")),
                                "image": c.get("Image", ""),
                                "status": c.get("Status", ""),
                            })

    return section
