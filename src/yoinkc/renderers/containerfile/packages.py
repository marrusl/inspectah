"""Containerfile section: packages (build stage, FROM, repos, GPG keys, dnf install)."""

import re
from pathlib import Path
from typing import List

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    base: str,
    c_ext_pip: list,
    needs_multistage: bool,
) -> list[str]:
    """Return Containerfile lines for build stage, FROM, repos, and packages."""
    lines: list[str] = []

    if needs_multistage:
        lines.append("# === Build stage: compile pip packages with C extensions ===")
        lines.append(f"FROM {base} AS builder")
        lines.append("RUN dnf install -y gcc python3-devel make && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
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

    # Repository Configuration
    included_gpg = [k for k in snapshot.rpm.gpg_keys if k.include] if snapshot.rpm else []
    gpg_copy_dirs = sorted(set(str(Path(k.path).parent) for k in included_gpg)) if included_gpg else []

    if snapshot.rpm and snapshot.rpm.repo_files:
        included_repos = [r for r in snapshot.rpm.repo_files if r.include]
        if included_repos:
            lines.append("# === Repository Configuration ===")
            for d in gpg_copy_dirs:
                lines.append(f"COPY config/{d}/ /{d}/")
            has_yum_repos = any(r.path.startswith("etc/yum.repos.d/") for r in included_repos)
            has_dnf_conf   = any(r.path.startswith("etc/dnf/")         for r in included_repos)
            if has_yum_repos:
                lines.append("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/")
            if has_dnf_conf:
                lines.append("COPY config/etc/dnf/ /etc/dnf/")
            key_note = f" + {len(included_gpg)} GPG key(s)" if included_gpg else ""
            lines.append(f"# {len(included_repos)} repo file(s){key_note} — also included in consolidated COPY config/etc/ below")
            lines.append("")
    elif gpg_copy_dirs:
        lines.append("# === GPG Keys ===")
        for d in gpg_copy_dirs:
            lines.append(f"COPY config/{d}/ /{d}/")
        lines.append("")

    # Package Installation
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
            lines.append("    && dnf clean all && rm -rf /var/cache/dnf /var/lib/dnf/history* /var/log/dnf* /var/log/hawkey.log /var/log/rhsm")
        if auto_count:
            lines.append(f"# {auto_count} additional package(s) will be pulled in as dependencies")
            lines.append("# See audit-report.md for full package list")
        lines.append("")

    return lines
