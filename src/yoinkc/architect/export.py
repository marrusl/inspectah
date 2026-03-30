"""Export layer topology as a Containerfile tree tarball."""

from __future__ import annotations

import io
import tarfile
from yoinkc.architect.analyzer import LayerTopology


def export_topology(topo: LayerTopology, base_image: str) -> bytes:
    """Generate a .tar.gz containing Containerfile + tree/ per layer, plus build.sh."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for layer in topo.layers:
            containerfile = render_containerfile(layer.name, layer.parent, layer.packages, base_image)
            _add_string_to_tar(tar, f"{layer.name}/Containerfile", containerfile)

        build_sh = _render_build_sh(topo, base_image)
        _add_string_to_tar(tar, "build.sh", build_sh)

    return buf.getvalue()


def render_containerfile(
    layer_name: str,
    parent: str | None,
    packages: list[str],
    base_image: str,
) -> str:
    """Render a Containerfile for a single layer."""
    lines = []

    if parent is None:
        lines.append(f"FROM {base_image}")
    else:
        lines.append(f"FROM localhost/{parent}:latest")

    lines.append("")

    if packages:
        pkg_list = " \\\n    ".join(sorted(packages))
        lines.append(f"RUN dnf install -y \\\n    {pkg_list} \\\n    && dnf clean all")
        lines.append("")

    return "\n".join(lines)


def _render_build_sh(topo: LayerTopology, base_image: str) -> str:
    """Render build.sh with ordered build commands."""
    lines = [
        "#!/bin/bash",
        "# Build base first, then derived images",
        "set -euo pipefail",
        "",
    ]

    # Base first
    base = topo.get_layer("base")
    if base is not None:
        lines.append(f"podman build -t localhost/base:latest base/")

    # Then derived in order
    for layer in topo.layers:
        if layer.parent is not None:
            lines.append(f"podman build -t localhost/{layer.name}:latest {layer.name}/")

    lines.append("")
    return "\n".join(lines)


def _add_string_to_tar(tar: tarfile.TarFile, name: str, content: str) -> None:
    """Add a string as a file to a tarball."""
    data = content.encode("utf-8")
    info = tarfile.TarInfo(name=name)
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))
