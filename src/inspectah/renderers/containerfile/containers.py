"""Containerfile section: container workloads (quadlets, compose)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for container workloads."""
    lines: list[str] = []

    included_quadlets = [u for u in (snapshot.containers.quadlet_units or []) if u.include] if snapshot.containers else []
    included_compose = [c for c in (snapshot.containers.compose_files or []) if c.include] if snapshot.containers else []
    if snapshot.containers and (included_quadlets or included_compose):
        lines.append("# === Container Workloads ===")
        if included_quadlets:
            lines.append("COPY quadlet/ /etc/containers/systemd/")
        if included_compose:
            for cf in included_compose:
                lines.append(f"# Compose file included: {cf.path}")
            lines.append("# Compose file(s) included as-is. For native systemd integration,")
            lines.append("# consider converting to Quadlet units — see https://github.com/containers/podlet")
            lines.append("# or https://docs.podman.io/en/latest/markdown/podman-systemd.unit.5.html")
        lines.append("")

    return lines
