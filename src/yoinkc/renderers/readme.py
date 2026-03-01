"""README.md renderer: summary, build/deploy commands, FIXME list."""

from pathlib import Path

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot


def _base_image(snapshot: InspectionSnapshot) -> str:
    from ..baseline import base_image_for_snapshot
    return base_image_for_snapshot(snapshot)


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    output_dir = Path(output_dir)
    lines = ["# yoinkc output", ""]

    # --- Summary of findings ---
    os_name = ""
    if snapshot.os_release:
        os_name = snapshot.os_release.pretty_name or snapshot.os_release.name
        lines.append(f"Generated from **{os_name}**.")
        lines.append("")

    hostname = snapshot.meta.get("hostname", "")
    if hostname:
        lines.append(f"**Host:** `{hostname}`")
    ts = snapshot.meta.get("timestamp", "")
    if ts:
        lines.append(f"**Inspected:** {ts}")
    if hostname or ts:
        lines.append("")

    lines.append("## Findings summary")
    lines.append("")

    pkg_added = len(snapshot.rpm.packages_added) if snapshot.rpm else 0
    pkg_removed = len(snapshot.rpm.packages_removed) if snapshot.rpm else 0

    configs_modified = 0
    configs_unowned = 0
    if snapshot.config and snapshot.config.files:
        configs_modified = sum(1 for f in snapshot.config.files if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED)
        configs_unowned = sum(1 for f in snapshot.config.files if f.kind == ConfigFileKind.UNOWNED)

    svc_enabled = len(snapshot.services.enabled_units) if snapshot.services else 0
    svc_disabled = len(snapshot.services.disabled_units) if snapshot.services else 0

    warnings_count = len(snapshot.warnings) if snapshot.warnings else 0
    redactions_count = len(snapshot.redactions) if snapshot.redactions else 0

    fixmes = _extract_fixmes(output_dir)

    lines.append("| Category | Count |")
    lines.append("|---|---|")
    if snapshot.rpm and snapshot.rpm.no_baseline:
        lines.append(f"| Packages (all — no baseline) | {pkg_added} |")
    else:
        lines.append(f"| Packages added (beyond base image) | {pkg_added} |")
    if pkg_removed:
        lines.append(f"| Packages removed | {pkg_removed} |")
    lines.append(f"| Configs modified (RPM-owned) | {configs_modified} |")
    lines.append(f"| Configs unowned | {configs_unowned} |")
    lines.append(f"| Services changed | {svc_enabled + svc_disabled} ({svc_enabled} enabled, {svc_disabled} disabled) |")
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        lines.append(f"| Non-RPM software items | {len(snapshot.non_rpm_software.items)} |")
    if snapshot.containers and (snapshot.containers.quadlet_units or snapshot.containers.compose_files):
        q = len(snapshot.containers.quadlet_units)
        c = len(snapshot.containers.compose_files)
        lines.append(f"| Container workloads | {q} quadlet, {c} compose |")
    if redactions_count:
        lines.append(f"| Secrets redacted | {redactions_count} |")
    lines.append(f"| Warnings | {warnings_count} |")
    lines.append(f"| FIXME items | {len(fixmes)} |")
    lines.append("")

    # --- Build ---
    base = _base_image(snapshot)
    lines.append("## Build")
    lines.append("")
    lines.append("```bash")
    lines.append("podman build -t my-bootc-image:latest .")
    lines.append("```")
    lines.append("")

    # --- Deploy ---
    lines.append("## Deploy")
    lines.append("")
    is_centos = snapshot.os_release and "centos" in snapshot.os_release.id.lower()
    has_kargs = snapshot.kernel_boot and snapshot.kernel_boot.cmdline
    has_selinux = snapshot.selinux and snapshot.selinux.mode

    lines.append("```bash")
    if has_kargs:
        lines.append("# Custom kernel args detected — verify they are baked into the image")
        lines.append("# or pass them via the bootloader configuration at deploy time.")
    lines.append("# Switch an existing system to the new image:")
    lines.append("bootc switch my-bootc-image:latest")
    lines.append("")
    lines.append("# Or install to a new disk:")
    install_flags = []
    if is_centos:
        install_flags.append("--target-no-signature-verification")
    if has_selinux and snapshot.selinux.mode == "enforcing":
        install_flags.append("--enforce-container-sigpolicy")
    flags_str = " ".join(install_flags)
    if flags_str:
        lines.append(f"bootc install to-disk {flags_str} /dev/sdX")
    else:
        lines.append("bootc install to-disk /dev/sdX")
    lines.append("```")
    lines.append("")
    lines.append("Review `kickstart-suggestion.ks` for deployment-time settings (hostname, DHCP, DNS).")
    lines.append("")

    # --- Artifacts ---
    lines.append("## Artifacts")
    lines.append("")
    lines.append("| File | Description |")
    lines.append("|---|---|")
    lines.append("| `Containerfile` | Image definition |")
    lines.append("| `config/` | Files to COPY into the image |")
    lines.append("| `audit-report.md` | Full findings (markdown) |")
    lines.append("| `report.html` | Interactive report (open in browser) |")
    lines.append("| `secrets-review.md` | Redacted items requiring manual handling |")
    lines.append("| `kickstart-suggestion.ks` | Suggested deploy-time settings |")
    lines.append("| `inspection-snapshot.json` | Raw data for re-rendering (`--from-snapshot`) |")
    lines.append("")

    # --- FIXME items ---
    if fixmes:
        lines.append("## FIXME items (resolve before production)")
        lines.append("")
        for i, fixme in enumerate(fixmes, 1):
            lines.append(f"{i}. {fixme}")
        lines.append("")

    # --- Warnings ---
    if snapshot.warnings:
        lines.append("## Warnings")
        lines.append("")
        for w in snapshot.warnings:
            msg = w.get("message") or "—"
            src = w.get("source", "")
            prefix = f"**{src}:** " if src else ""
            lines.append(f"- {prefix}{msg}")
        lines.append("")

    lines.append("See [`audit-report.md`](audit-report.md) or [`report.html`](report.html) for full details.")
    lines.append("")
    (output_dir / "README.md").write_text("\n".join(lines))


def _extract_fixmes(output_dir: Path) -> list:
    """Pull FIXME comments from the generated Containerfile."""
    cf = output_dir / "Containerfile"
    if not cf.exists():
        return []
    fixmes = []
    try:
        for line in cf.read_text().splitlines():
            stripped = line.strip()
            if "FIXME" in stripped and stripped.startswith("#"):
                fixmes.append(stripped.lstrip("# ").strip())
    except Exception:
        pass
    return fixmes
