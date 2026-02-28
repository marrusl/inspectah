"""
Containerfile renderer: produces Containerfile and config/ tree from snapshot.
"""

import re
from pathlib import Path
from typing import List, Optional

from jinja2 import Environment

from ..schema import ConfigFileKind, InspectionSnapshot

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


def _base_image_from_snapshot(snapshot: InspectionSnapshot) -> str:
    """Return FROM line base image, preferring the one stored in the snapshot."""
    if snapshot.rpm and snapshot.rpm.base_image:
        return snapshot.rpm.base_image
    if not snapshot.os_release:
        return "registry.redhat.io/rhel9/rhel-bootc:9.6"
    osr = snapshot.os_release
    if osr.id == "rhel" and osr.version_id:
        return f"registry.redhat.io/rhel9/rhel-bootc:{osr.version_id}"
    if "centos" in osr.id.lower():
        return "quay.io/centos-bootc/centos-bootc:stream9"
    return "registry.redhat.io/rhel9/rhel-bootc:9.6"


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
            rel = entry.path.lstrip("/")
            if rel in dhcp_paths:
                continue
            dest = config_dir / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text(entry.content or "")

    if snapshot.rpm and snapshot.rpm.repo_files:
        for repo in snapshot.rpm.repo_files:
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

    # Systemd timer units: cron-generated and existing local timers
    st = snapshot.scheduled_tasks
    if st and (st.generated_timer_units or st.systemd_timers):
        systemd_dir = config_dir / "etc/systemd/system"
        systemd_dir.mkdir(parents=True, exist_ok=True)
        for u in st.generated_timer_units:
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
            if u.name and u.content:
                (quadlet_dir / u.name).write_text(u.content)

    # Non-RPM software files
    if snapshot.non_rpm_software and snapshot.non_rpm_software.items:
        for item in snapshot.non_rpm_software.items:
            path = item.get("path", "")
            # Items with a "files" dict (npm/yarn/gem lockfile dirs)
            files = item.get("files")
            if path and files and isinstance(files, dict):
                rel = path.lstrip("/")
                dest = config_dir / rel
                dest.mkdir(parents=True, exist_ok=True)
                for fname, fcontent in files.items():
                    (dest / fname).write_text(fcontent)
            # Items with simple "content" (requirements.txt, single files)
            elif path and item.get("content", ""):
                rel = path.lstrip("/")
                dest = config_dir / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_text(item["content"])

    # User/group account fragment files for append-based provisioning.
    # Written to config/tmp/ so they are NOT swept up by COPY config/etc/ /etc/.
    # The Containerfile COPYs them to /tmp/ explicitly and RUNs cat >> /etc/...
    ug = snapshot.users_groups
    if ug:
        tmp_dir = config_dir / "tmp"
        tmp_dir.mkdir(parents=True, exist_ok=True)
        for attr, filename in (
            ("passwd_entries", "passwd.append"),
            ("shadow_entries", "shadow.append"),
            ("group_entries", "group.append"),
            ("gshadow_entries", "gshadow.append"),
            ("subuid_entries", "subuid.append"),
            ("subgid_entries", "subgid.append"),
        ):
            entries = getattr(ug, attr, [])
            if entries:
                (tmp_dir / filename).write_text("\n".join(entries) + "\n")

    # Kernel module / sysctl / dracut configs
    if snapshot.kernel_boot:
        for section_list in (
            snapshot.kernel_boot.modules_load_d,
            snapshot.kernel_boot.modprobe_d,
            snapshot.kernel_boot.dracut_conf,
        ):
            for entry in (section_list or []):
                kpath = entry.get("path", "")
                content = entry.get("content", "")
                if kpath:
                    dest = config_dir / kpath
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_text(content)

    if snapshot.kernel_boot and snapshot.kernel_boot.sysctl_overrides:
            sysctl_dir = config_dir / "etc/sysctl.d"
            sysctl_dir.mkdir(parents=True, exist_ok=True)
            sysctl_lines = ["# Non-default sysctl values detected by yoinkc"]
            for s in snapshot.kernel_boot.sysctl_overrides:
                sysctl_lines.append(f"{s['key']} = {s['runtime']}")
            (sysctl_dir / "99-yoinkc.conf").write_text("\n".join(sysctl_lines) + "\n")

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
        config_entries = [f for f in snapshot.config.files if f.path.lstrip("/") not in dhcp_paths]
        modified = [f for f in config_entries if f.kind == ConfigFileKind.RPM_OWNED_MODIFIED]
        unowned = [f for f in config_entries if f.kind == ConfigFileKind.UNOWNED]
        orphaned = [f for f in config_entries if f.kind == ConfigFileKind.ORPHANED]
        if modified:
            lines.append(f"# Modified RPM-owned configs ({len(modified)}):")
            for f in modified:
                rel = f.path.lstrip("/")
                if f.diff_against_rpm and f.diff_against_rpm.strip():
                    pkg_label = f.package or "RPM"
                    diff_lines = [l for l in f.diff_against_rpm.strip().splitlines()
                                  if (l.startswith("+") or l.startswith("-"))
                                  and not l.startswith("---") and not l.startswith("+++")]
                    summary = diff_lines[:3]
                    lines.append(f"#   {rel} (modified from {pkg_label}):")
                    for sl in summary:
                        lines.append(f"#     {sl}")
                    if len(diff_lines) > 3:
                        lines.append(f"#     ... and {len(diff_lines) - 3} more changes")
                else:
                    lines.append(f"#   {rel}")
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
        lines.append(f"# Repo files ({len(snapshot.rpm.repo_files)}):")
        for r in snapshot.rpm.repo_files[:5]:
            lines.append(f"#   {r.path}")

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
        if st.generated_timer_units:
            lines.append(f"# Cron-converted timers ({len(st.generated_timer_units)}): "
                         + ", ".join(u.name for u in st.generated_timer_units[:5]))

    # Kernel
    kb = snapshot.kernel_boot
    if kb:
        if kb.modules_load_d:
            lines.append(f"# modules-load.d: {len(kb.modules_load_d)} file(s)")
        if kb.modprobe_d:
            lines.append(f"# modprobe.d: {len(kb.modprobe_d)} file(s)")
        if kb.dracut_conf:
            lines.append(f"# dracut.conf.d: {len(kb.dracut_conf)} file(s)")
        if kb.sysctl_overrides:
            lines.append(f"# sysctl overrides: etc/sysctl.d/99-yoinkc.conf ({len(kb.sysctl_overrides)} value(s))")

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
            if item.get("method") == "pip dist-info" and item.get("version"):
                if item.get("has_c_extensions"):
                    c_ext_pip.append((item["name"], item["version"]))
                else:
                    pure_pip.append((item["name"], item["version"]))

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
    lines.append("")

    if needs_multistage:
        lines.append("# === Install pre-built pip packages with C extensions ===")
        # Detect the Python minor version from the base image tag or os-release.
        # Fall back to a FIXME placeholder rather than using a glob, which is
        # invalid as a COPY destination in a Containerfile.
        py_ver = ""
        if snapshot.os_release:
            vid = snapshot.os_release.version_id or ""
            major = vid.split(".")[0]
            if major == "9":
                py_ver = "3.9"
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
        raw_names = sorted(set(p.name for p in snapshot.rpm.packages_added))
        safe_names: List[str] = []
        for n in raw_names:
            if _sanitize_shell_value(n, "dnf install") is not None:
                safe_names.append(n)
            else:
                lines.append(f"# FIXME: package name contains unsafe characters, skipped: {n!r}")
        lines.append("# === Package Installation ===")
        if getattr(snapshot.rpm, "no_baseline", False):
            lines.append("# No baseline — including all installed packages")
        else:
            lines.append(f"# Detected: {len(safe_names)} packages added beyond base image")
        if safe_names:
            lines.append("RUN dnf install -y \\")
            for n in safe_names[:-1]:
                lines.append(f"    {n} \\")
            lines.append(f"    {safe_names[-1]} \\")
            lines.append("    && dnf clean all")
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

        if st.generated_timer_units:
            lines.append(f"# Converted from cron: {len(st.generated_timer_units)} timer(s) — included in COPY config/etc/ below")
            for u in st.generated_timer_units:
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
            method = item.get("method", "")
            lang = item.get("lang", "")
            path = item.get("path", item.get("name", ""))

            if lang in ("go", "rust"):
                linking = "statically linked" if item.get("static") else "dynamically linked"
                lines.append(f"# FIXME: {lang.capitalize()} binary at /{path} ({linking})")
                lines.append(f"# Obtain source and rebuild for the target image, or COPY the binary directly")
                lines.append(f"# COPY config/{path} /{path}")
            elif lang == "c/c++":
                if item.get("static"):
                    lines.append(f"# FIXME: static C/C++ binary at /{path} — COPY or rebuild from source")
                    lines.append(f"# COPY config/{path} /{path}")
                else:
                    libs = ", ".join(item.get("shared_libs", [])[:5])
                    lines.append(f"# FIXME: dynamic C/C++ binary at /{path} — needs: {libs}")
                    lines.append(f"# COPY config/{path} /{path}")
            elif method == "python venv":
                pkgs = item.get("packages", [])
                ssp = item.get("system_site_packages", False)
                if ssp:
                    lines.append(f"# FIXME: venv at /{path} uses --system-site-packages — verify RPM deps are in base image")
                if pkgs:
                    lines.append(f"# Python venv at /{path}: {len(pkgs)} package(s)")
                    lines.append(f"RUN python3 -m venv /{path}")
                    pkg_specs = " ".join(f'{p["name"]}=={p["version"]}' for p in pkgs if p.get("version"))
                    if pkg_specs:
                        lines.append(f"RUN /{path}/bin/pip install {pkg_specs}")
                else:
                    lines.append(f"# FIXME: venv at /{path} — no packages detected, verify manually")
            elif method == "git repository":
                remote = item.get("git_remote", "")
                commit = item.get("git_commit", "")
                branch = item.get("git_branch", "")
                lines.append(f"# Git-managed: /{path}")
                if remote:
                    lines.append(f"# FIXME: clone from {remote} (branch: {branch}, commit: {commit[:12]})")
                    lines.append(f"# RUN git clone {remote} /{path} && cd /{path} && git checkout {commit[:12]}")
                else:
                    lines.append(f"# FIXME: git repo at /{path} has no remote — COPY or reconstruct")
            elif method == "pip dist-info" and item.get("version"):
                if not item.get("has_c_extensions"):
                    pip_packages.append((item["name"], item["version"]))
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
            path = item.get("path", item.get("name", ""))
            lines.append(f"# FIXME: unknown provenance — determine upstream source and installation method for /{path}")
            lines.append(f"# COPY config/{path} /{path}")

        lines.append("")

    # 8. Container Workloads (Quadlet)
    if snapshot.containers and (snapshot.containers.quadlet_units or snapshot.containers.compose_files):
        lines.append("# === Container Workloads (Quadlet) ===")
        if snapshot.containers.compose_files:
            lines.append("# FIXME: converted from docker-compose, verify quadlet translation")
        lines.append("COPY quadlet/ /etc/containers/systemd/")
        lines.append("")

    # 9. Users and Groups
    ug = snapshot.users_groups
    if ug and (ug.passwd_entries or ug.users):
        lines.append("# === Users and Groups ===")
        if ug.passwd_entries:
            append_files = ["group", "passwd", "shadow", "gshadow"]
            copy_lines = []
            cat_parts = []
            for name in append_files:
                attr = f"{name}_entries"
                if getattr(ug, attr, []):
                    copy_lines.append(f"COPY config/tmp/{name}.append /tmp/{name}.append")
                    cat_parts.append(f"cat /tmp/{name}.append >> /etc/{name}")
            for sub in ("subuid", "subgid"):
                attr = f"{sub}_entries"
                if getattr(ug, attr, []):
                    copy_lines.append(f"COPY config/tmp/{sub}.append /tmp/{sub}.append")
                    cat_parts.append(f"cat /tmp/{sub}.append >> /etc/{sub}")
            for cl in copy_lines:
                lines.append(cl)
            if cat_parts:
                cat_parts.append("rm -f /tmp/*.append")
                lines.append("RUN " + " && \\\n    ".join(cat_parts))
            # Create home directories
            for u in (ug.users or []):
                home = u.get("home", "")
                uid = u.get("uid", "")
                name = u.get("name", "")
                if home and home != "/" and name and uid:
                    lines.append(f"RUN mkdir -p {home} && chown {uid}:{u.get('gid', uid)} {home}")
        else:
            for g in (ug.groups or [])[:10]:
                gname, gid = g.get("name", ""), g.get("gid", "")
                if gname and gid:
                    lines.append(f"RUN groupadd -g {gid} {gname}")
            for u in (ug.users or [])[:10]:
                uname, uid, gid = u.get("name", ""), u.get("uid", ""), u.get("gid", "")
                shell = u.get("shell", "")
                if uname and uid:
                    gid_opt = f" -g {gid}" if gid else ""
                    shell_opt = f" -s {shell}" if shell and shell != "/sbin/nologin" else ""
                    lines.append(f"RUN useradd -u {uid}{gid_opt}{shell_opt} -m {uname}")
        if ug.sudoers_rules:
            lines.append(f"# FIXME: {len(ug.sudoers_rules)} sudoers rule(s) detected — review and bake into /etc/sudoers.d/")
            for rule in ug.sudoers_rules[:10]:
                lines.append(f"#   {rule}")
            if len(ug.sudoers_rules) > 10:
                lines.append(f"#   ... and {len(ug.sudoers_rules) - 10} more")
            lines.append("# RUN echo '<user> ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/<user>")
        if ug.ssh_authorized_keys_refs:
            lines.append(f"# FIXME: {len(ug.ssh_authorized_keys_refs)} SSH authorized_keys file(s) detected")
            lines.append("# Do NOT bake SSH keys into the image — inject at deploy time via:")
            lines.append("#   - cloud-init (ssh_authorized_keys)")
            lines.append("#   - kickstart (%post with curl from metadata service)")
            lines.append("#   - Ignition (for CoreOS/bootc systems)")
            for ref in ug.ssh_authorized_keys_refs[:5]:
                user = ref.get("user", "?")
                path = ref.get("path", "?")
                lines.append(f"#   Found: {path} (user: {user})")
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
            lines.append("# FIXME: review detected kernel args and add the ones needed for this image")
            # Emit per-karg FIXME lines for any cmdline args that look like key=value.
            # Sanitize each value before embedding in the template comment.
            for karg in kb.cmdline.split():
                if "=" in karg:
                    key, _, val = karg.partition("=")
                    if (_sanitize_shell_value(key, "kargs key") is not None
                            and _sanitize_shell_value(val, "kargs value") is not None):
                        lines.append(f"# RUN rpm-ostree kargs --append={key}={val}")
                    else:
                        lines.append(f"# FIXME: karg contains unsafe characters, skipped: {karg!r}")
                else:
                    if _sanitize_shell_value(karg, "kargs flag") is not None:
                        lines.append(f"# RUN rpm-ostree kargs --append={karg}")
                    else:
                        lines.append(f"# FIXME: karg contains unsafe characters, skipped: {karg!r}")
        if kb.non_default_modules:
            names = ", ".join(m.get("name", "?") for m in kb.non_default_modules[:10])
            lines.append(f"# {len(kb.non_default_modules)} non-default kernel module(s) loaded at runtime: {names}")
            lines.append("# FIXME: if these modules are needed, add them to /etc/modules-load.d/ in the image")
        if kb.modules_load_d:
            lines.append(f"# modules-load.d: {len(kb.modules_load_d)} file(s) — included in COPY config/etc/ above")
        if kb.modprobe_d:
            lines.append(f"# modprobe.d: {len(kb.modprobe_d)} file(s) — included in COPY config/etc/ above")
        if kb.dracut_conf:
            lines.append(f"# dracut.conf.d: {len(kb.dracut_conf)} file(s) — included in COPY config/etc/ above")
        if kb.sysctl_overrides:
            lines.append(f"# sysctl: {len(kb.sysctl_overrides)} non-default value(s) — included in COPY config/etc/ above")
        lines.append("")

    # 11. SELinux Customizations
    has_selinux = snapshot.selinux and (
        snapshot.selinux.custom_modules or snapshot.selinux.boolean_overrides
        or snapshot.selinux.audit_rules or snapshot.selinux.fips_mode
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
        if snapshot.selinux.audit_rules:
            lines.append(f"# {len(snapshot.selinux.audit_rules)} audit rule file(s) — included in COPY config/etc/ above")
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

    if net and net.proxy:
        lines.append("# Proxy settings detected — bake as environment defaults")
        env_lines = []
        for p in net.proxy:
            if "=" in p.line:
                env_lines.append(p.line)
        if env_lines:
            lines.append("RUN mkdir -p /etc/environment.d && cat > /etc/environment.d/proxy.conf << 'PROXYEOF'")
            for el in env_lines:
                lines.append(el)
            lines.append("PROXYEOF")

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
