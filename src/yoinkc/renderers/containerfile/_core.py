"""Containerfile renderer orchestrator."""

from pathlib import Path

from jinja2 import Environment

from ...schema import InspectionSnapshot
from ._helpers import _base_image_from_snapshot, _dhcp_connection_paths
from ._config_tree import write_config_tree
from . import (
    config,
    containers,
    kernel_boot,
    network,
    non_rpm_software,
    packages,
    scheduled_tasks,
    selinux,
    services,
    users_groups,
)


def _classify_pip(snapshot: InspectionSnapshot) -> tuple[list, list]:
    """Classify pip packages into C-extension and pure lists."""
    c_ext_pip: list = []
    pure_pip: list = []
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        for item in snapshot.non_rpm_software.items:
            if not item.include:
                continue
            if item.method == "pip dist-info" and item.version:
                if item.has_c_extensions:
                    c_ext_pip.append((item.name, item.version))
                else:
                    pure_pip.append((item.name, item.version))
    return c_ext_pip, pure_pip


def _tmpfiles_lines() -> list[str]:
    """Epilogue: tmpfiles.d comment block."""
    return [
        "# === tmpfiles.d for /var structure ===",
        "# Directories created on every boot; /var is not updated by bootc after bootstrap.",
        "# tmpfiles.d/yoinkc-var.conf included in COPY config/etc/ above",
        "",
    ]


def _validate_lines() -> list[str]:
    """Epilogue: bootc validation."""
    return [
        "# === Validate bootc compatibility ===",
        "RUN bootc container lint",
    ]


def _render_containerfile_content(
    snapshot: InspectionSnapshot, output_dir: Path
) -> str:
    """Build Containerfile content from snapshot."""
    base = _base_image_from_snapshot(snapshot)
    c_ext_pip, pure_pip = _classify_pip(snapshot)
    needs_multistage = bool(c_ext_pip)
    dhcp_paths = _dhcp_connection_paths(snapshot)

    lines: list[str] = []

    # Layer order matches design doc for cache efficiency
    lines += packages.section_lines(
        snapshot, base=base, c_ext_pip=c_ext_pip,
        needs_multistage=needs_multistage,
    )
    lines += services.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=True)
    lines += scheduled_tasks.section_lines(snapshot)
    lines += config.section_lines(
        snapshot, output_dir=output_dir, dhcp_paths=dhcp_paths,
    )
    lines += non_rpm_software.section_lines(
        snapshot, pure_pip=pure_pip, needs_multistage=needs_multistage,
    )
    lines += containers.section_lines(snapshot)
    lines += users_groups.section_lines(snapshot)
    lines += kernel_boot.section_lines(snapshot)
    lines += selinux.section_lines(snapshot)
    lines += network.section_lines(snapshot, firewall_only=False)

    # Epilogue
    lines += _tmpfiles_lines()
    lines += _validate_lines()

    return "\n".join(lines)


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    """Write Containerfile and config/ tree to output_dir."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    write_config_tree(snapshot, output_dir)
    content = _render_containerfile_content(snapshot, output_dir)
    (output_dir / "Containerfile").write_text(content)
