"""Containerfile section: SELinux customizations."""

from ...schema import InspectionSnapshot
from ._helpers import _sanitize_shell_value


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for SELinux customizations."""
    lines: list[str] = []

    has_selinux = snapshot.selinux and (
        snapshot.selinux.custom_modules or snapshot.selinux.boolean_overrides
        or snapshot.selinux.fcontext_rules or snapshot.selinux.audit_rules
        or snapshot.selinux.fips_mode or snapshot.selinux.port_labels
    )
    if not has_selinux:
        return lines

    lines.append("# === SELinux Customizations ===")
    if snapshot.selinux.custom_modules:
        lines.append(f"# FIXME: {len(snapshot.selinux.custom_modules)} custom policy module(s) detected — "
                     "export .pp files to config/selinux/ and uncomment the COPY + semodule lines below")
        lines.append("# COPY config/selinux/ /tmp/selinux/")
        lines.append("# RUN semodule -i /tmp/selinux/*.pp && rm -rf /tmp/selinux/")
    # boolean_overrides only contains non-default entries (filtered at collection time)
    if snapshot.selinux.boolean_overrides:
        lines.append(f"# FIXME: {len(snapshot.selinux.boolean_overrides)} non-default boolean(s) detected — verify each is still needed")
        for b in snapshot.selinux.boolean_overrides[:20]:
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

    return lines
