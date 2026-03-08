"""
Containerfile renderer: produces Containerfile and config/ tree from snapshot.
"""

import re
from pathlib import Path
from typing import List, Optional

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot


def _summarise_diff(diff_text: str) -> List[str]:
    """Produce human-readable change summaries from a unified diff.

    For simple key=value changes, produces "key: old → new".
    Falls back to raw +/- lines for complex changes.
    """
    additions: dict = {}
    removals: dict = {}
    other: List[str] = []

    for line in diff_text.strip().splitlines():
        if line.startswith("---") or line.startswith("+++") or line.startswith("@@"):
            continue
        stripped = line[1:].strip() if len(line) > 1 else ""
        if not stripped or stripped.startswith("#"):
            continue
        if line.startswith("-"):
            if "=" in stripped or ":" in stripped:
                sep = "=" if "=" in stripped else ":"
                key = stripped.split(sep, 1)[0].strip()
                removals[key] = stripped.split(sep, 1)[1].strip()
            else:
                other.append(f"removed: {stripped}")
        elif line.startswith("+"):
            if "=" in stripped or ":" in stripped:
                sep = "=" if "=" in stripped else ":"
                key = stripped.split(sep, 1)[0].strip()
                additions[key] = stripped.split(sep, 1)[1].strip()
            else:
                other.append(f"added: {stripped}")

    results: List[str] = []
    matched_keys = set()
    for key in additions:
        if key in removals:
            results.append(f"{key}: {removals[key]} → {additions[key]}")
            matched_keys.add(key)
        else:
            results.append(f"{key}: added ({additions[key]})")
    for key in removals:
        if key not in matched_keys:
            results.append(f"{key}: removed")
    results.extend(other)
    return results or ["(diff available — see audit report)"]

# Characters that would change shell semantics if injected into a RUN command.
# The data comes from RPM databases / systemd on an operator-controlled host,
# so this is a safety net against corrupted snapshots, not a security boundary.
_SHELL_UNSAFE_RE = re.compile(r'[\n\r;`|]|\$\(')


def _sanitize_shell_value(value: str, context: str) -> Optional[str]:
    """Return *value* if it is safe to embed in a shell RUN command, else None.

    Rejects values containing newlines, semicolons, backticks, ``$(...)``, or
    pipe characters — the characters that materially change shell semantics.
    When None is returned the caller should emit a FIXME comment instead.
    """
    if _SHELL_UNSAFE_RE.search(value):
        return None
    return value


# ---------------------------------------------------------------------------
# Kernel argument filtering
# ---------------------------------------------------------------------------

# Exact bare-word kernel parameters that are always managed by the bootloader
# or base image and must never appear in a kargs.d image drop-in.
_KARGS_BOOTLOADER_EXACT: frozenset = frozenset({
    "ro", "rw", "rhgb", "quiet", "splash",
})

# Prefixes whose matching kargs are likewise bootloader/installer-owned.
_KARGS_BOOTLOADER_PREFIXES: tuple = (
    "BOOT_IMAGE=",
    "root=",
    "rootflags=",
    "rootfstype=",
    "initrd=",
    "initramfs=",
    "crashkernel=",
    "resume=",
    "rd.lvm.lv=",
    "rd.luks.uuid=",
    "rd.luks.name=",
    "rd.md.uuid=",
    "LANG=",
)


def _is_bootloader_karg(karg: str) -> bool:
    """Return True if *karg* is a standard bootloader/installer parameter.

    These are managed by the bootloader or base image and should not appear
    in a kargs.d TOML drop-in.  Only operator-added kargs belong there.
    """
    if karg in _KARGS_BOOTLOADER_EXACT:
        return True
    for prefix in _KARGS_BOOTLOADER_PREFIXES:
        if karg.startswith(prefix):
            return True
    return False


def _operator_kargs(cmdline: str) -> List[str]:
    """Return the operator-added kernel arguments from a raw cmdline string.

    Filters out both standard bootloader-managed parameters and any kargs
    that contain shell-unsafe characters.
    """
    result: List[str] = []
    for karg in cmdline.split():
        if _is_bootloader_karg(karg):
            continue
        if _sanitize_shell_value(karg, "kargs") is None:
            continue
        result.append(karg)
    return result


def _base_image_from_snapshot(snapshot: InspectionSnapshot) -> str:
    """Return FROM line base image, preferring the one stored in the snapshot."""
    from ..baseline import base_image_for_snapshot
    return base_image_for_snapshot(snapshot)


def _dhcp_connection_paths(snapshot: InspectionSnapshot) -> set:
    """Return relative paths of NM profiles that are NOT static (DHCP/other).

    These belong in the kickstart, not baked into the image.
    """
    paths: set = set()
    if snapshot.network:
        for c in snapshot.network.connections:
            if c.method != "static" and c.path:
                paths.add(c.path)
    return paths


def _write_config_tree(snapshot: InspectionSnapshot, output_dir: Path) -> None:
    """Write all config files from snapshot to output_dir/config/ preserving paths."""
    config_dir = output_dir / "config"
    dhcp_paths = _dhcp_connection_paths(snapshot)

    if snapshot.config and snapshot.config.files:
        for entry in snapshot.config.files:
            if not entry.include:
                continue
            rel = entry.path.lstrip("/")
            if rel in dhcp_paths:
                continue
            dest = config_dir / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(entry.content or "")

    if snapshot.rpm and snapshot.rpm.repo_files:
        for repo in snapshot.rpm.repo_files:
            if not repo.include:
                continue
            dest = config_dir / repo.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(repo.content or "")

    # Firewalld zones and direct rules
    if snapshot.network:
        for z in snapshot.network.firewall_zones:
            if z.path:
                dest = config_dir / z.path
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_text(z.content)
        if snapshot.network.firewall_direct_rules:
            import xml.etree.ElementTree as ET
            direct_el = ET.Element("direct")
            for r in snapshot.network.firewall_direct_rules:
                rule_el = ET.SubElement(direct_el, "rule")
                rule_el.set("priority", r.priority)
                rule_el.set("table", r.table)
                rule_el.set("ipv", r.ipv)
                rule_el.set("chain", r.chain)
                rule_el.text = r.args
            dest = config_dir / "etc/firewalld/direct.xml"
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text('<?xml version="1.0" encoding="utf-8"?>\n'
                            + ET.tostring(direct_el, encoding="unicode") + "\n")

        # Static NM connection profiles (baked into image)
        for c in snapshot.network.connections:
            if c.method == "static" and c.path:
                dest = config_dir / c.path
                dest.parent.mkdir(parents=True, exist_ok=True)
                if not dest.exists():
                    dest.write_text("")

    # Systemd drop-in overrides
    if snapshot.services and snapshot.services.drop_ins:
        for di in snapshot.services.drop_ins:
            if not di.include:
                continue
            dest = config_dir / di.path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(di.content or "")

    # Systemd timer units: cron-generated and existing local timers
    st = snapshot.scheduled_tasks
    if st and (st.generated_timer_units or st.systemd_timers):
        systemd_dir = config_dir / "etc/systemd/system"
        systemd_dir.mkdir(parents=True, exist_ok=True)
        for u in st.generated_timer_units:
            if not u.include:
                continue
            (systemd_dir / f"{u.name}.timer").write_text(u.timer_content)
            (systemd_dir / f"{u.name}.service").write_text(u.service_content)
        for t in st.systemd_timers:
            if t.source == "local":
                if t.name and t.timer_content:
                    (systemd_dir / f"{t.name}.timer").write_text(t.timer_content)
                if t.name and t.service_content:
                    (systemd_dir / f"{t.name}.service").write_text(t.service_content)

    # Quadlet units
    if snapshot.containers and snapshot.containers.quadlet_units:
        quadlet_dir = output_dir / "quadlet"
        quadlet_dir.mkdir(parents=True, exist_ok=True)
        for u in snapshot.containers.quadlet_units:
            if not u.include:
                continue
            if u.name and u.content:
                (quadlet_dir / u.name).write_text(u.content)

    # Dotenv / secret files found under /opt
    if snapshot.non_rpm_software and snapshot.non_rpm_software.env_files:
        for entry in snapshot.non_rpm_software.env_files:
            if not entry.include:
                continue
            rel = entry.path.lstrip("/")
            dest = config_dir / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(entry.content or "")

    # Non-RPM software files
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        for item in snapshot.non_rpm_software.items:
            if not item.include:
                continue
            # Items with a "files" dict (npm/yarn/gem lockfile dirs)
            if item.path and item.files and isinstance(item.files, dict):
                rel = item.path.lstrip("/")
                dest = config_dir / rel
                dest.mkdir(parents=True, exist_ok=True)
                for fname, fcontent in item.files.items():
                    (dest / fname).write_text(fcontent)
            # Items with simple "content" (requirements.txt, single files)
            elif item.path and item.content:
                rel = item.path.lstrip("/")
                dest = config_dir / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_text(item.content)

    # User/group provisioning files — strategy-aware
    ug = snapshot.users_groups
    if ug:
        # Sysusers: write /usr/lib/sysusers.d/yoinkc-users.conf
        sysusers_users = [u for u in (ug.users or []) if u.get("strategy") == "sysusers" and u.get("include", True)]
        sysusers_groups = [g for g in (ug.groups or []) if g.get("strategy") == "sysusers" and g.get("include", True)]
        if sysusers_users or sysusers_groups:
            sysusers_dir = config_dir / "usr/lib/sysusers.d"
            sysusers_dir.mkdir(parents=True, exist_ok=True)
            sysusers_lines = ["# Generated by yoinkc — service accounts created at boot by systemd-sysusers"]
            for g in sysusers_groups:
                sysusers_lines.append(f"g {g.get('name', '')} {g.get('gid', '-')}")
            for u in sysusers_users:
                uid = u.get("uid", "-")
                gecos = u.get("name", "")
                home = u.get("home", "-")
                shell = u.get("shell", "-")
                gid_ref = f"{u.get('gid', '-')}" if u.get("gid") else "-"
                sysusers_lines.append(f"u {u.get('name', '')} {uid}:{gid_ref} \"{gecos}\" {home} {shell}")
            (sysusers_dir / "yoinkc-users.conf").write_text("\n".join(sysusers_lines) + "\n")

            # Tmpfiles.d for sysusers home directories
            tmpfiles_dir = config_dir / "etc/tmpfiles.d"
            tmpfiles_dir.mkdir(parents=True, exist_ok=True)
            home_lines = ["# Home directories for sysusers-managed accounts"]
            for u in sysusers_users:
                home = u.get("home", "")
                name = u.get("name", "")
                if home and home != "/" and name:
                    home_lines.append(f"d {home} 0755 {name} {name} -")
            if len(home_lines) > 1:
                tmpfiles_path = tmpfiles_dir / "yoinkc-sysusers-homes.conf"
                tmpfiles_path.write_text("\n".join(home_lines) + "\n")

        # Exact-copy: write append fragments (only for exact-copy users)
        exact_users = {u.get("name") for u in (ug.users or []) if u.get("strategy") == "exact-copy" and u.get("include", True)}
        exact_groups = {g.get("name") for g in (ug.groups or []) if g.get("strategy") == "exact-copy" and g.get("include", True)}
        if exact_users or exact_groups:
            tmp_dir = config_dir / "tmp"
            tmp_dir.mkdir(parents=True, exist_ok=True)
            for attr, filename, match_set in (
                ("passwd_entries", "passwd.append", exact_users),
                ("shadow_entries", "shadow.append", exact_users),
                ("group_entries", "group.append", exact_groups),
                ("gshadow_entries", "gshadow.append", exact_groups),
                ("subuid_entries", "subuid.append", exact_users),
                ("subgid_entries", "subgid.append", exact_users),
            ):
                entries = getattr(ug, attr, [])
                filtered = [e for e in entries if e.split(":")[0] in match_set] if match_set else []
                if filtered:
                    (tmp_dir / filename).write_text("\n".join(filtered) + "\n")

        # Blueprint TOML — only when at least one user has blueprint strategy
        blueprint_users = [u for u in (ug.users or []) if u.get("strategy") == "blueprint" and u.get("include", True)]
        blueprint_groups = [g for g in (ug.groups or []) if g.get("strategy") == "blueprint" and g.get("include", True)]
        if blueprint_users or blueprint_groups:
            toml_lines = ["# Generated by yoinkc — bootc-image-builder customization", ""]
            for g in blueprint_groups:
                toml_lines.append("[[customizations.group]]")
                toml_lines.append(f"name = \"{g.get('name', '')}\"")
                toml_lines.append(f"gid = {g.get('gid', 0)}")
                toml_lines.append("")
            for u in blueprint_users:
                toml_lines.append("[[customizations.user]]")
                toml_lines.append(f"name = \"{u.get('name', '')}\"")
                if u.get("uid"):
                    toml_lines.append(f"uid = {u['uid']}")
                if u.get("gid"):
                    toml_lines.append(f"gid = {u['gid']}")
                if u.get("home"):
                    toml_lines.append(f"home = \"{u['home']}\"")
                if u.get("shell"):
                    toml_lines.append(f"shell = \"{u['shell']}\"")
                toml_lines.append("")
            (output_dir / "yoinkc-users.toml").write_text("\n".join(toml_lines))

    # Kernel module / sysctl / dracut configs
    if snapshot.kernel_boot:
        for section_list in (
            snapshot.kernel_boot.modules_load_d,
            snapshot.kernel_boot.modprobe_d,
            snapshot.kernel_boot.dracut_conf,
        ):
            for entry in (section_list or []):
                kpath = entry.path
                content = entry.content
                if kpath:
                    dest = config_dir / kpath
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_text(content)

    if snapshot.kernel_boot and snapshot.kernel_boot.sysctl_overrides:
            included_sysctls = [s for s in snapshot.kernel_boot.sysctl_overrides if s.include]
            if included_sysctls:
                sysctl_dir = config_dir / "etc/sysctl.d"
                sysctl_dir.mkdir(parents=True, exist_ok=True)
                sysctl_lines = ["# Non-default sysctl values detected by yoinkc"]
                for s in included_sysctls:
                    sysctl_lines.append(f"{s.key} = {s.runtime}")
                (sysctl_dir / "99-yoinkc.conf").write_text("\n".join(sysctl_lines) + "\n")

    # Kernel arguments: write bootc-native kargs.d TOML drop-in
    if snapshot.kernel_boot and snapshot.kernel_boot.cmdline:
        safe_kargs = _operator_kargs(snapshot.kernel_boot.cmdline)
        if safe_kargs:
            kargs_dir = config_dir / "usr/lib/bootc/kargs.d"
            kargs_dir.mkdir(parents=True, exist_ok=True)
            kargs_quoted = ", ".join(f'"{k}"' for k in safe_kargs)
            (kargs_dir / "yoinkc-migrated.toml").write_text(
                f"# Generated by yoinkc — review and remove args that belong to\n"
                f"# the bootloader/base image rather than this specific image.\n"
                f"kargs = [{kargs_quoted}]\n"
            )

    # tmpfiles.d for /var (and home) directory structure
    tmpfiles_dir = config_dir / "etc/tmpfiles.d"
    tmpfiles_dir.mkdir(parents=True, exist_ok=True)
    tmpfiles_lines = [
        "# Generated by yoinkc: directories created on every boot.",
        "# /var is seeded at initial bootstrap only; bootc does not update it.",
        "# Add d lines for application dirs under /var or /home as needed.",
    ]
    if snapshot.users_groups and snapshot.users_groups.users:
        for u in snapshot.users_groups.users[:20]:
            if not u.get("include", True):
                continue
            name = u.get("name", "")
            if name and name != "root":
                tmpfiles_lines.append(f"d /home/{name} 0755 {name} - -")
    if len(tmpfiles_lines) <= 3:
        tmpfiles_lines.append("d /var/lib/app 0755 root root -")
    (tmpfiles_dir / "yoinkc-var.conf").write_text("\n".join(tmpfiles_lines) + "\n")


def _config_copy_roots(config_dir: Path):
    """Return sorted list of non-empty top-level subdirectory names under config_dir.

    'tmp' is excluded — those files get individual COPY lines to /tmp/.
    This is called after _write_config_tree so we copy exactly what was written.
    """
    roots = []
    try:
        for d in sorted(config_dir.iterdir()):
            if not d.is_dir() or d.name == "tmp":
                continue
            # Only include if the directory is non-empty
            if any(True for _ in d.rglob("*") if _.is_file()):
                roots.append(d.name)
    except (PermissionError, OSError):
        pass
    return roots


def _config_inventory_comment(snapshot: InspectionSnapshot, dhcp_paths: set) -> list:
    """Build a block comment listing everything that will be in the consolidated COPY."""
    lines = []

    # Config files (modified, unowned, orphaned)
    if snapshot.config and snapshot.config.files:
        config_entries = [f for f in snapshot.config.files if f.include and f.path.lstrip("/") not in dhcp_paths]
        modified = [f for f in config_entries if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        unowned = [f for f in config_entries if f.kind == ConfigFileKind.UNOWNED]
        orphaned = [f for f in config_entries if f.kind == ConfigFileKind.ORPHANED]
        if modified:
            lines.append(f"# Modified RPM-owned configs ({len(modified)}):")
            for f in modified:
                rel = f.path.lstrip("/")
                if f.diff_against_rpm and f.diff_against_rpm.strip():
                    pkg_label = f.package or "RPM"
                    lines.append(f"#   {rel} (modified from {pkg_label} default):")
                    changes = _summarise_diff(f.diff_against_rpm)
                    for ch in changes[:5]:
                        lines.append(f"#     - {ch}")
                    remaining = len(changes) - 5
                    if remaining > 0:
                        lines.append(f"#     ... and {remaining} more change(s)")
                    lines.append(f"#     See audit-report.md or report.html for full diff")
                else:
                    flags = f" (rpm -Va: {f.rpm_va_flags})" if f.rpm_va_flags else ""
                    lines.append(f"#   {rel}{flags}")
        if unowned:
            lines.append(f"# Unowned configs ({len(unowned)}):")
            for f in unowned[:10]:
                lines.append(f"#   {f.path.lstrip('/')}")
            if len(unowned) > 10:
                lines.append(f"#   ... and {len(unowned) - 10} more")
        if orphaned:
            lines.append(f"# Orphaned configs from removed packages ({len(orphaned)}):")
            for f in orphaned[:5]:
                lines.append(f"#   {f.path.lstrip('/')}")

    # Repo files
    if snapshot.rpm and snapshot.rpm.repo_files:
        included_repos = [r for r in snapshot.rpm.repo_files if r.include]
        excluded_repos = [r for r in snapshot.rpm.repo_files if not r.include]
        if included_repos:
            lines.append(f"# Repo files ({len(included_repos)}):")
            for r in included_repos[:5]:
                lines.append(f"#   {r.path}")
        for r in excluded_repos:
            lines.append(f"# Excluded repo: {r.path}")

    # Firewall
    net = snapshot.network
    if net and net.firewall_zones:
        lines.append(f"# Firewall zones ({len(net.firewall_zones)}): "
                     + ", ".join(z.name for z in net.firewall_zones[:5]))
    if net and net.firewall_direct_rules:
        lines.append(f"# Firewall direct rules: etc/firewalld/direct.xml")

    # Static NM connections
    if net:
        static_conns = [c for c in net.connections if c.method == "static"]
        if static_conns:
            lines.append(f"# Static NM connections ({len(static_conns)}): "
                         + ", ".join(c.name for c in static_conns[:5]))

    # Timers
    st = snapshot.scheduled_tasks
    if st:
        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        if local_timers:
            lines.append(f"# Local systemd timers ({len(local_timers)}): "
                         + ", ".join(t.name for t in local_timers[:5]))
        incl_gen_timers = [u for u in st.generated_timer_units if u.include]
        if incl_gen_timers:
            lines.append(f"# Cron-converted timers ({len(incl_gen_timers)}): "
                         + ", ".join(u.name for u in incl_gen_timers[:5]))

    # Kernel
    kb = snapshot.kernel_boot
    if kb:
        if kb.cmdline:
            safe_kargs = _operator_kargs(kb.cmdline)
            if safe_kargs:
                lines.append(f"# kargs.d: usr/lib/bootc/kargs.d/yoinkc-migrated.toml ({len(safe_kargs)} arg(s))")
        if kb.modules_load_d:
            lines.append(f"# modules-load.d: {len(kb.modules_load_d)} file(s)")
        if kb.modprobe_d:
            lines.append(f"# modprobe.d: {len(kb.modprobe_d)} file(s)")
        if kb.dracut_conf:
            lines.append(f"# dracut.conf.d: {len(kb.dracut_conf)} file(s)")
        if kb.sysctl_overrides:
            lines.append(f"# sysctl overrides: etc/sysctl.d/99-yoinkc.conf ({len(kb.sysctl_overrides)} value(s))")

    # Systemd drop-in overrides
    if snapshot.services and snapshot.services.drop_ins:
        included = [d for d in snapshot.services.drop_ins if d.include]
        if included:
            units = sorted(set(d.unit for d in included))
            lines.append(f"# Systemd drop-in overrides ({len(included)}): "
                         + ", ".join(units[:5])
                         + (" ..." if len(units) > 5 else ""))

    # SELinux audit rules
    if snapshot.selinux and snapshot.selinux.audit_rules:
        lines.append(f"# Audit rules: {len(snapshot.selinux.audit_rules)} file(s)")

    # tmpfiles.d (always written)
    lines.append("# tmpfiles.d: etc/tmpfiles.d/yoinkc-var.conf")

    return lines


def _render_containerfile_content(snapshot: InspectionSnapshot, output_dir: Path) -> str:
    """Build Containerfile content from snapshot.

    Layer order matches the design doc for cache efficiency:
      repos → packages → services → firewall → scheduled tasks → configs →
      non-RPM software → quadlets → users → kernel → SELinux →
      network note → tmpfiles.d
    """
    lines = []
    base = _base_image_from_snapshot(snapshot)

    # Detect pip packages with C extensions for multi-stage build
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

    needs_multistage = bool(c_ext_pip)

    if needs_multistage:
        lines.append("# === Build stage: compile pip packages with C extensions ===")
        lines.append(f"FROM {base} AS builder")
        lines.append("RUN dnf install -y gcc python3-devel make && dnf clean all")
        lines.append("RUN python3 -m venv /tmp/pip-build")
        c_ext_pip.sort()
        specs = " ".join(f"{n}=={v}" for n, v in c_ext_pip)
        lines.append(f"RUN /tmp/pip-build/bin/pip install {specs}")
        lines.append("")

    lines.append("# === Base Image ===")
    os_desc = "unknown"
    if snapshot.os_release:
        os_desc = snapshot.os_release.pretty_name or snapshot.os_release.name or os_desc
    lines.append(f"# Detected: {os_desc}")
    lines.append(f"FROM {base}")

    # Cross-major-version migration warning
    if snapshot.os_release and snapshot.os_release.version_id and snapshot.rpm and snapshot.rpm.base_image:
        source_major = snapshot.os_release.version_id.split(".")[0]
        target_tag = snapshot.rpm.base_image.rsplit(":", 1)[-1] if ":" in snapshot.rpm.base_image else ""
        # Strip "stream" prefix from CentOS tags (e.g. "stream10" -> "10")
        target_major = re.sub(r"^stream", "", target_tag).split(".")[0] if target_tag else ""
        if source_major and target_major and source_major != target_major:
            lines.append("")
            lines.append("# !! CROSS-MAJOR-VERSION MIGRATION !!")
            lines.append(f"# Source: {os_desc} ({snapshot.os_release.version_id})")
            lines.append(f"# Target: {snapshot.rpm.base_image}")
            lines.append("# Package names, service names, and config formats may have changed.")
            lines.append("# This Containerfile requires heavier manual review than a same-version migration.")

    lines.append("")

    _PYTHON_VERSION_MAP = {"9": "3.9", "10": "3.12"}

    if needs_multistage:
        lines.append("# === Install pre-built pip packages with C extensions ===")
        py_ver = ""
        if snapshot.os_release:
            vid = snapshot.os_release.version_id or ""
            major = vid.split(".")[0]
            os_id = snapshot.os_release.id.lower()
            py_ver = _PYTHON_VERSION_MAP.get(major, "")
            if not py_ver and os_id == "fedora":
                py_ver = "3.12"
        if py_ver:
            lines.append(f"COPY --from=builder /tmp/pip-build/lib/python{py_ver}/site-packages/ "
                         f"/usr/lib/python{py_ver}/site-packages/")
        else:
            lines.append("# FIXME: replace python3.X with the actual Python version in the base image")
            lines.append("COPY --from=builder /tmp/pip-build/lib/python3.X/site-packages/ "
                         "/usr/lib/python3.X/site-packages/")
        lines.append("")

    # 1. Repository Configuration
    if snapshot.rpm and snapshot.rpm.repo_files:
        lines.append("# === Repository Configuration ===")
        lines.append(f"# Detected: {len(snapshot.rpm.repo_files)} repo file(s) — included in COPY config/etc/ below")
        lines.append("")

    # 2. Package Installation
    if snapshot.rpm and snapshot.rpm.packages_added:
        included_pkgs = [p for p in snapshot.rpm.packages_added if p.include]
        raw_names = sorted(set(p.name for p in included_pkgs))
        safe_names: List[str] = []
        for n in raw_names:
            if _sanitize_shell_value(n, "dnf install") is not None:
                safe_names.append(n)
            else:
                lines.append(f"# FIXME: package name contains unsafe characters, skipped: {n!r}")

        # Use leaf/auto split if available
        leaf_set = set(snapshot.rpm.leaf_packages) if snapshot.rpm.leaf_packages is not None else None
        dep_tree = snapshot.rpm.leaf_dep_tree or {}
        if leaf_set is not None and not getattr(snapshot.rpm, "no_baseline", False):
            included_name_set = set(raw_names)
            included_leaf_names = leaf_set & included_name_set
            install_names = [n for n in safe_names if n in included_leaf_names]
            if dep_tree:
                remaining_auto = set()
                for lf in included_leaf_names:
                    remaining_auto.update(dep_tree.get(lf, []))
                auto_count = len(remaining_auto)
            else:
                all_auto = set(snapshot.rpm.auto_packages) if snapshot.rpm.auto_packages else set()
                auto_count = len(all_auto & included_name_set)
        else:
            install_names = safe_names
            auto_count = 0

        lines.append("# === Package Installation ===")
        if getattr(snapshot.rpm, "no_baseline", False):
            lines.append("# No baseline — including all installed packages")
        elif auto_count:
            lines.append(f"# Detected: {len(install_names)} explicitly installed packages "
                         f"(+{auto_count} dependencies pulled in automatically)")
        else:
            lines.append(f"# Detected: {len(install_names)} packages added beyond base image")
        if install_names:
            lines.append("RUN dnf install -y \\")
            for n in install_names[:-1]:
                lines.append(f"    {n} \\")
            lines.append(f"    {install_names[-1]} \\")
            lines.append("    && dnf clean all")
        if auto_count:
            lines.append(f"# {auto_count} additional package(s) will be pulled in as dependencies")
            lines.append("# See audit-report.md for full package list")
        lines.append("")

    # 3. Service Enablement
    if snapshot.services:
        enabled = snapshot.services.enabled_units
        disabled = snapshot.services.disabled_units
        if enabled or disabled:
            lines.append("# === Service Enablement ===")
            safe_enabled = [u for u in enabled if _sanitize_shell_value(u, "systemctl enable") is not None]
            safe_disabled = [u for u in disabled if _sanitize_shell_value(u, "systemctl disable") is not None]
            skipped = (len(enabled) - len(safe_enabled)) + (len(disabled) - len(safe_disabled))
            if skipped:
                lines.append(f"# FIXME: {skipped} unit name(s) contained unsafe characters and were skipped")
            lines.append(f"# Detected: {len(safe_enabled)} non-default enabled, {len(safe_disabled)} disabled")
            if safe_enabled:
                lines.append("RUN systemctl enable " + " ".join(safe_enabled))
            if safe_disabled:
                lines.append("RUN systemctl disable " + " ".join(safe_disabled))
            lines.append("")

    # 3b. Systemd drop-in overrides
    if snapshot.services and snapshot.services.drop_ins:
        included_dropins = [d for d in snapshot.services.drop_ins if d.include]
        if included_dropins:
            lines.append("# === Systemd Drop-in Overrides ===")
            lines.append(f"# Detected: {len(included_dropins)} drop-in override(s) — included in COPY config/etc/ below")
            seen_dirs: set = set()
            for di in included_dropins:
                dropin_dir = str(Path(di.path).parent)
                if dropin_dir not in seen_dirs:
                    seen_dirs.add(dropin_dir)
                    lines.append(f"#   {di.unit}: {dropin_dir}/")
            lines.append("")

    # 4. Firewall Configuration (bake into image)
    net = snapshot.network
    has_fw = net and (net.firewall_zones or net.firewall_direct_rules)
    if has_fw:
        lines.append("# === Firewall Configuration (bake into image) ===")
        if net.firewall_zones:
            total_rich = sum(len(z.rich_rules) for z in net.firewall_zones)
            lines.append(f"# Detected: {len(net.firewall_zones)} zone(s)"
                         + (f", {total_rich} rich rule(s)" if total_rich else "")
                         + " — included in COPY config/etc/ below")
        if net.firewall_direct_rules:
            lines.append(f"# Detected: {len(net.firewall_direct_rules)} direct rule(s) — included in COPY config/etc/ below")
        lines.append("")
        lines.append("# firewall-cmd equivalents (alternative to the consolidated COPY below):")
        for z in net.firewall_zones:
            for svc in z.services:
                lines.append(f"# RUN firewall-offline-cmd --zone={z.name} --add-service={svc}")
            for port in z.ports:
                lines.append(f"# RUN firewall-offline-cmd --zone={z.name} --add-port={port}")
            for rr in z.rich_rules:
                if rr:
                    lines.append(f"# RUN firewall-offline-cmd --zone={z.name} --add-rich-rule='{rr}'")
        for dr in net.firewall_direct_rules:
            lines.append(f"# RUN firewall-offline-cmd --direct --add-rule {dr.ipv} {dr.table} {dr.chain} 0 {dr.args}")
        lines.append("")

    # 5. Scheduled Tasks
    st = snapshot.scheduled_tasks
    if st and (st.generated_timer_units or st.systemd_timers or st.cron_jobs or st.at_jobs):
        lines.append("# === Scheduled Tasks ===")

        local_timers = [t for t in st.systemd_timers if t.source == "local"]
        vendor_timers = [t for t in st.systemd_timers if t.source == "vendor"]

        if local_timers:
            lines.append(f"# Existing local timers ({len(local_timers)}): timer files included in COPY config/etc/ below")
            for t in local_timers:
                lines.append(f"RUN systemctl enable {t.name}.timer")

        if vendor_timers:
            lines.append(f"# Vendor timers ({len(vendor_timers)}): already in base image, no action needed")
            for t in vendor_timers:
                lines.append(f"#   - {t.name} ({t.on_calendar})")

        included_timers = [u for u in st.generated_timer_units if u.include]
        if included_timers:
            lines.append(f"# Converted from cron: {len(included_timers)} timer(s) — included in COPY config/etc/ below")
            for u in included_timers:
                if u.name:
                    lines.append(f"RUN systemctl enable {u.name}.timer")

        if st.at_jobs:
            lines.append(f"# FIXME: {len(st.at_jobs)} at job(s) found — convert to systemd timers or cron")
            for a in st.at_jobs:
                lines.append(f"#   at job: {a.command}")

        lines.append("")

    # 6. Configuration Files — consolidated COPY
    # All captured config files, repo files, firewall, timers, NM connections,
    # kernel module configs, sysctl overrides, audit rules, and tmpfiles.d are
    # written under config/ and copied to the image root in a single layer.
    dhcp_paths = _dhcp_connection_paths(snapshot)
    lines.append("# === Configuration Files ===")
    inventory_lines = _config_inventory_comment(snapshot, dhcp_paths)
    lines.extend(inventory_lines)
    if any(f.diff_against_rpm for f in (snapshot.config.files if snapshot.config else [])):
        lines.append("# Config diffs (--config-diffs): see audit-report.md and report.html for per-file diffs.")
    lines.append("")

    # Emit one COPY per non-empty top-level dir under config/ (excluding tmp/).
    config_dir = output_dir / "config"
    roots = _config_copy_roots(config_dir)
    for root in roots:
        lines.append(f"COPY config/{root}/ /{root}/")
    if not roots:
        lines.append("# (no config files captured)")
    lines.append("")

    # 7. Non-RPM Software
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        lines.append("# === Non-RPM Software ===")

        pip_packages: list = []
        remaining: list = []

        for item in snapshot.non_rpm_software.items:
            if not item.include:
                continue
            method = item.method
            lang = item.lang
            path = item.path or item.name

            if lang in ("go", "rust"):
                linking = "statically linked" if item.static else "dynamically linked"
                lines.append(f"# FIXME: {lang.capitalize()} binary at /{path} ({linking})")
                lines.append(f"# Obtain source and rebuild for the target image, or COPY the binary directly")
                lines.append(f"# COPY config/{path} /{path}")
            elif lang == "c/c++":
                if item.static:
                    lines.append(f"# FIXME: static C/C++ binary at /{path} — COPY or rebuild from source")
                    lines.append(f"# COPY config/{path} /{path}")
                else:
                    libs = ", ".join(item.shared_libs[:5])
                    lines.append(f"# FIXME: dynamic C/C++ binary at /{path} — needs: {libs}")
                    lines.append(f"# COPY config/{path} /{path}")
            elif method == "python venv":
                pkgs = item.packages
                if item.system_site_packages:
                    lines.append(f"# FIXME: venv at /{path} uses --system-site-packages — verify RPM deps are in base image")
                if pkgs:
                    lines.append(f"# Python venv at /{path}: {len(pkgs)} package(s)")
                    lines.append(f"RUN python3 -m venv /{path}")
                    pkg_specs = " ".join(f"{p.name}=={p.version}" for p in pkgs if p.version)
                    if pkg_specs:
                        lines.append(f"RUN /{path}/bin/pip install {pkg_specs}")
                else:
                    lines.append(f"# FIXME: venv at /{path} — no packages detected, verify manually")
            elif method == "git repository":
                lines.append(f"# Git-managed: /{path}")
                if item.git_remote:
                    lines.append(f"# FIXME: clone from {item.git_remote} (branch: {item.git_branch}, commit: {item.git_commit[:12]})")
                    lines.append(f"# RUN git clone {item.git_remote} /{path} && cd /{path} && git checkout {item.git_commit[:12]}")
                else:
                    lines.append(f"# FIXME: git repo at /{path} has no remote — COPY or reconstruct")
            elif method == "pip dist-info" and item.version:
                if not item.has_c_extensions:
                    pip_packages.append((item.name, item.version))
            elif method == "pip requirements.txt":
                lines.append(f"# FIXME: verify pip packages in /{path} install correctly from PyPI")
                lines.append(f"COPY config/{path} /{path}")
                lines.append(f"RUN pip install -r /{path}")
            elif method == "npm package-lock.json":
                lines.append(f"# FIXME: verify npm packages in /{path} install correctly")
                lines.append(f"COPY config/{path}/ /{path}/")
                lines.append(f"RUN cd /{path} && npm ci")
            elif method == "yarn.lock":
                lines.append(f"# FIXME: verify yarn packages in /{path} install correctly")
                lines.append(f"COPY config/{path}/ /{path}/")
                lines.append(f"RUN cd /{path} && yarn install --frozen-lockfile")
            elif method == "gem Gemfile.lock":
                lines.append(f"# FIXME: verify Ruby gems in /{path} install correctly")
                lines.append(f"COPY config/{path}/ /{path}/")
                lines.append(f"RUN cd /{path} && bundle install")
            else:
                remaining.append(item)

        if pip_packages:
            pip_packages.sort()
            lines.append(f"# Detected: {len(pip_packages)} pip package(s) via dist-info")
            lines.append("# FIXME: verify these pip packages install correctly from PyPI")
            lines.append("RUN pip install \\")
            for name, ver in pip_packages[:-1]:
                lines.append(f"    {name}=={ver} \\")
            name, ver = pip_packages[-1]
            lines.append(f"    {name}=={ver}")

        for item in remaining[:20]:
            path = item.path or item.name
            lines.append(f"# FIXME: unknown provenance — determine upstream source and installation method for /{path}")
            lines.append(f"# COPY config/{path} /{path}")

        lines.append("")

    # 8. Container Workloads (Quadlet)
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

    # 9. Users and Groups — strategy-aware rendering
    ug = snapshot.users_groups
    _included_users = [u for u in (ug.users or []) if u.get("include", True)] if ug else []
    if ug and _included_users:
        lines.append("# === Users and Groups ===")

        # Group users/groups by strategy
        sysusers_users = [u for u in _included_users if u.get("strategy") == "sysusers"]
        useradd_users = [u for u in _included_users if u.get("strategy") == "useradd"]
        exact_users = [u for u in _included_users if u.get("strategy") == "exact-copy"]
        blueprint_users = [u for u in _included_users if u.get("strategy") == "blueprint"]
        kickstart_users = [u for u in _included_users if u.get("strategy") == "kickstart"]

        # --- sysusers strategy ---
        if sysusers_users:
            names = ", ".join(u.get("name", "") for u in sysusers_users)
            lines.append(f"# Service accounts ({len(sysusers_users)}): {names}")
            lines.append("# Created at boot by systemd-sysusers — declarative, no RUN needed")
            lines.append("COPY config/usr/lib/sysusers.d/yoinkc-users.conf /usr/lib/sysusers.d/yoinkc-users.conf")
            lines.append("# Home directories ensured by tmpfiles.d")

        # --- useradd strategy ---
        if useradd_users:
            useradd_groups = [g for g in (ug.groups or []) if g.get("strategy") == "useradd" and g.get("include", True)]
            lines.append(f"# Explicitly created users ({len(useradd_users)}):")
            for g in useradd_groups:
                gname, gid = g.get("name", ""), g.get("gid", "")
                if gname and gid:
                    lines.append(f"RUN groupadd -g {gid} {gname}")
            for u in useradd_users:
                uname = u.get("name", "")
                uid = u.get("uid", "")
                gid = u.get("gid", "")
                shell = u.get("shell", "")
                if uname and uid:
                    gid_opt = f" -g {gid}" if gid else ""
                    shell_opt = f" -s {shell}" if shell else ""
                    lines.append(f"RUN useradd -m -u {uid}{gid_opt}{shell_opt} {uname}")
            # Password hashes from shadow
            for u in useradd_users:
                uname = u.get("name", "")
                # Find matching shadow entry
                for se in (ug.shadow_entries or []):
                    parts = se.split(":")
                    if parts[0] == uname and len(parts) > 1 and parts[1] and parts[1] not in ("!", "!!", "*"):
                        lines.append(f"RUN echo '{uname}:{parts[1]}' | chpasswd -e")
                        lines.append(f"# Password hash from source — rotate after migration")
                        break
            # Sudoers for useradd users
            useradd_names = {u.get("name") for u in useradd_users}
            for rule in (ug.sudoers_rules or []):
                for uname in useradd_names:
                    if uname in rule:
                        lines.append(f"RUN echo '{rule}' > /etc/sudoers.d/{uname}")
                        lines.append(f"# FIXME: review sudoers rule for {uname}")
                        break
            # SSH key FIXMEs for useradd users
            for ref in (ug.ssh_authorized_keys_refs or []):
                if ref.get("user") in useradd_names:
                    lines.append(f"# FIXME: SSH keys for '{ref.get('user')}' — deploy via kickstart, cloud-init, or identity provider")

        # --- exact-copy strategy ---
        if exact_users:
            exact_names = {u.get("name") for u in exact_users}
            lines.append(f"# Exact-copy users ({len(exact_users)}): byte-level replica of source entries")
            lines.append("# NOTE: raw append to /etc/passwd may conflict with bootc /etc merge on updates")
            cat_parts = []
            tmp_dir = output_dir / "config" / "tmp"
            for name in ("group", "passwd", "shadow", "gshadow", "subuid", "subgid"):
                if (tmp_dir / f"{name}.append").exists():
                    cat_parts.append(f"cat /tmp/{name}.append >> /etc/{name}")
            if cat_parts:
                lines.append("COPY config/tmp/ /tmp/")
                cat_parts.append("rm -f /tmp/*.append")
                lines.append("RUN " + " && \\\n    ".join(cat_parts))

        # --- blueprint strategy ---
        if blueprint_users:
            names = ", ".join(u.get("name", "") for u in blueprint_users)
            lines.append(f"# Users managed via blueprint ({len(blueprint_users)}): {names}")
            lines.append("# See yoinkc-users.toml for bootc-image-builder customization")

        # --- kickstart strategy ---
        if kickstart_users:
            for u in kickstart_users:
                lines.append(f"# FIXME: human user '{u.get('name', '')}' deferred to kickstart/provisioning")
                lines.append("# See kickstart-suggestion.ks or configure via identity provider")

        # Sudoers rules not tied to a specific useradd user
        useradd_names = {u.get("name") for u in useradd_users}
        remaining_rules = [r for r in (ug.sudoers_rules or [])
                          if not any(n in r for n in useradd_names)]
        if remaining_rules:
            lines.append(f"# FIXME: {len(remaining_rules)} sudoers rule(s) — review and bake into /etc/sudoers.d/")
            for rule in remaining_rules[:10]:
                lines.append(f"#   {rule}")

        # SSH key refs not tied to useradd users
        remaining_ssh = [ref for ref in (ug.ssh_authorized_keys_refs or [])
                        if ref.get("user") not in useradd_names]
        if remaining_ssh:
            lines.append(f"# FIXME: {len(remaining_ssh)} SSH authorized_keys file(s) detected")
            lines.append("# Do NOT bake SSH keys into the image — inject at deploy time via:")
            lines.append("#   - cloud-init (ssh_authorized_keys)")
            lines.append("#   - kickstart (%post with curl from metadata service)")
            lines.append("#   - Ignition (for CoreOS/bootc systems)")
            for ref in remaining_ssh[:5]:
                lines.append(f"#   Found: {ref.get('path', '?')} (user: {ref.get('user', '?')})")

        lines.append("")

    # 10. Kernel Configuration
    kb = snapshot.kernel_boot
    has_kernel = kb and (
        kb.cmdline or kb.modules_load_d or kb.modprobe_d
        or kb.dracut_conf or kb.sysctl_overrides or kb.non_default_modules
    )
    if has_kernel:
        lines.append("# === Kernel Configuration ===")
        if kb.cmdline:
            # Emit FIXME for any karg that was skipped due to unsafe characters
            # (bootloader-standard kargs are silently excluded — not a FIXME).
            for karg in kb.cmdline.split():
                if not _is_bootloader_karg(karg) and _sanitize_shell_value(karg, "kargs") is None:
                    lines.append(f"# FIXME: karg contains unsafe characters, skipped: {karg!r}")
            safe_kargs = _operator_kargs(kb.cmdline)
            if safe_kargs:
                lines.append("# === Kernel Arguments (bootc-native kargs.d) ===")
                lines.append("# These are applied at install and honored across image upgrades. See bootc documentation:")
                lines.append("# https://containers.github.io/bootc/building/kernel-arguments.html")
                lines.append("RUN mkdir -p /usr/lib/bootc/kargs.d")
                lines.append("COPY config/usr/lib/bootc/kargs.d/yoinkc-migrated.toml /usr/lib/bootc/kargs.d/")
        included_mods = [m for m in kb.non_default_modules if m.include] if kb.non_default_modules else []
        if included_mods:
            names = ", ".join(m.name for m in included_mods[:10])
            lines.append(f"# {len(included_mods)} non-default kernel module(s) loaded at runtime: {names}")
            lines.append("# FIXME: if these modules are needed, add them to /etc/modules-load.d/ in the image")
        if kb.modules_load_d:
            lines.append(f"# modules-load.d: {len(kb.modules_load_d)} file(s) — included in COPY config/etc/ above")
        if kb.modprobe_d:
            lines.append(f"# modprobe.d: {len(kb.modprobe_d)} file(s) — included in COPY config/etc/ above")
        if kb.dracut_conf:
            lines.append(f"# dracut.conf.d: {len(kb.dracut_conf)} file(s) — included in COPY config/etc/ above")
        included_sysctl = [s for s in kb.sysctl_overrides if s.include] if kb.sysctl_overrides else []
        if included_sysctl:
            lines.append(f"# sysctl: {len(included_sysctl)} non-default value(s) — included in COPY config/etc/ above")
        lines.append("")

    # 11. SELinux Customizations
    has_selinux = snapshot.selinux and (
        snapshot.selinux.custom_modules or snapshot.selinux.boolean_overrides
        or snapshot.selinux.fcontext_rules or snapshot.selinux.audit_rules
        or snapshot.selinux.fips_mode or snapshot.selinux.port_labels
    )
    if has_selinux:
        lines.append("# === SELinux Customizations ===")
        if snapshot.selinux.custom_modules:
            lines.append(f"# FIXME: {len(snapshot.selinux.custom_modules)} custom policy module(s) detected — "
                         "export .pp files to config/selinux/ and uncomment the COPY + semodule lines below")
            lines.append("# COPY config/selinux/ /tmp/selinux/")
            lines.append("# RUN semodule -i /tmp/selinux/*.pp && rm -rf /tmp/selinux/")
        non_default = [b for b in snapshot.selinux.boolean_overrides if b.get("non_default")]
        if non_default:
            lines.append(f"# FIXME: {len(non_default)} non-default boolean(s) detected — verify each is still needed")
            for b in non_default[:20]:
                bname = b.get("name", "unknown_bool")
                bval = b.get("current", "on")
                if (_sanitize_shell_value(bname, "setsebool name") is not None
                        and _sanitize_shell_value(bval, "setsebool value") is not None):
                    lines.append(f"RUN setsebool -P {bname} {bval}")
                else:
                    lines.append(f"# FIXME: boolean name/value contains unsafe characters, skipped: {bname!r}={bval!r}")
        if snapshot.selinux.fcontext_rules:
            lines.append(f"# FIXME: {len(snapshot.selinux.fcontext_rules)} custom fcontext rule(s) detected — apply in image")
            for fc in snapshot.selinux.fcontext_rules[:10]:
                if _sanitize_shell_value(fc, "semanage fcontext") is not None:
                    lines.append(f"# RUN semanage fcontext -a {fc}")
                else:
                    lines.append(f"# FIXME: fcontext rule contains unsafe characters: {fc!r}")
            lines.append("# RUN restorecon -Rv /  # apply fcontext changes after all COPYs")
        if snapshot.selinux.audit_rules:
            lines.append(f"# {len(snapshot.selinux.audit_rules)} audit rule file(s) — included in COPY config/etc/ above")
        if snapshot.selinux.port_labels:
            lines.append(f"# {len(snapshot.selinux.port_labels)} custom SELinux port label(s) detected")
            for pl in snapshot.selinux.port_labels:
                proto = _sanitize_shell_value(pl.protocol, "semanage port protocol")
                port = _sanitize_shell_value(pl.port, "semanage port number")
                ptype = _sanitize_shell_value(pl.type, "semanage port type")
                if proto is not None and port is not None and ptype is not None:
                    lines.append(f"RUN semanage port -a -t {ptype} -p {proto} {port}")
                else:
                    lines.append(f"# FIXME: port label contains unsafe characters, skipped: {pl.type!r} {pl.protocol!r} {pl.port!r}")
        if snapshot.selinux.fips_mode:
            lines.append("# FIXME: host has FIPS mode enabled — enable FIPS in the bootc image via fips-mode-setup")
        lines.append("")

    # 12. Network / Kickstart
    lines.append("# === Network / Kickstart ===")
    if net and net.connections:
        static_conns = [c for c in net.connections if c.method == "static"]
        dhcp_conns = [c for c in net.connections if c.method == "dhcp"]
        if static_conns:
            names = ", ".join(c.name for c in static_conns)
            lines.append(f"# Static connections (baked into image): {names} — included in COPY config/etc/ above")
        if dhcp_conns:
            names = ", ".join(c.name for c in dhcp_conns)
            lines.append(f"# DHCP connections (kickstart at deploy time): {names}")
            lines.append("# FIXME: configure these interfaces via kickstart — see kickstart-suggestion.ks")
    else:
        lines.append("# NOTE: Interface-specific config (DHCP, DNS) should be applied via kickstart at deploy time.")
        lines.append("# FIXME: review kickstart-suggestion.ks for deployment-time config")
    if net and net.resolv_provenance:
        prov = net.resolv_provenance
        if prov == "networkmanager":
            lines.append("# resolv.conf: NM-managed — DNS assigned at deploy time via DHCP/kickstart")
        elif prov == "systemd-resolved":
            lines.append("# resolv.conf: systemd-resolved — DNS assigned at deploy time")
        else:
            lines.append("# resolv.conf: hand-edited — review whether to bake into image or manage at deploy")

    if net and net.hosts_additions:
        lines.append(f"# {len(net.hosts_additions)} custom /etc/hosts entries detected")
        lines.append("RUN cat >> /etc/hosts << 'HOSTSEOF'")
        for h in net.hosts_additions:
            lines.append(h)
        lines.append("HOSTSEOF")

    _DNF_PROXY_SOURCES = ("etc/dnf/dnf.conf", "etc/yum.conf")
    if net and net.proxy:
        env_entries = [p for p in net.proxy if p.source not in _DNF_PROXY_SOURCES]
        dnf_entries = [p for p in net.proxy if p.source in _DNF_PROXY_SOURCES]
        if env_entries:
            lines.append("# Proxy settings detected — bake as environment defaults")
            env_lines = [p.line for p in env_entries if "=" in p.line]
            if env_lines:
                lines.append("RUN mkdir -p /etc/environment.d && cat > /etc/environment.d/proxy.conf << 'PROXYEOF'")
                for el in env_lines:
                    lines.append(el)
                lines.append("PROXYEOF")
        if dnf_entries:
            lines.append("# DNF proxy configured — preserved in etc/dnf/dnf.conf (included in COPY config/etc/)")
            for p in dnf_entries:
                lines.append(f"#   {p.line}")

    if net and net.static_routes:
        lines.append(f"# {len(net.static_routes)} static route file(s) detected")
        lines.append("# FIXME: add static routes via NM connection or nmstatectl config")
        for r in net.static_routes[:10]:
            lines.append(f"# Route file: {r.path} — review and translate to NM connection (+ipv4.routes)")
    lines.append("")

    # 13. tmpfiles.d for /var structure — included in COPY config/etc/ above
    lines.append("# === tmpfiles.d for /var structure ===")
    lines.append("# Directories created on every boot; /var is not updated by bootc after bootstrap.")
    lines.append("# tmpfiles.d/yoinkc-var.conf included in COPY config/etc/ above")
    lines.append("")

    return "\n".join(lines)


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    """Write Containerfile and config/ tree to output_dir."""
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    _write_config_tree(snapshot, output_dir)
    content = _render_containerfile_content(snapshot, output_dir)
    (output_dir / "Containerfile").write_text(content)
