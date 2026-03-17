"""Containerfile section: kernel configuration and boot arguments."""

from ...schema import InspectionSnapshot
from ._helpers import (
    _is_bootloader_karg, _operator_kargs, _sanitize_shell_value,
    _TUNED_PROFILE_RE,
)


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for kernel config."""
    lines: list[str] = []

    kb = snapshot.kernel_boot
    has_kernel = kb and (
        kb.cmdline or kb.modules_load_d or kb.modprobe_d
        or kb.dracut_conf or kb.sysctl_overrides or kb.non_default_modules
        or kb.tuned_active or kb.tuned_custom_profiles
    )
    if not has_kernel:
        return lines

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
    if kb.tuned_active or kb.tuned_custom_profiles:
        if kb.tuned_active:
            if _TUNED_PROFILE_RE.match(kb.tuned_active):
                if kb.tuned_custom_profiles:
                    lines.append("COPY config/etc/tuned/ /etc/tuned/")
                lines.append(f'RUN echo "{kb.tuned_active}" > /etc/tuned/active_profile')
                lines.append("RUN systemctl enable tuned.service")
            else:
                lines.append(f"# FIXME: tuned profile name contains unsafe characters: {kb.tuned_active!r}")
                if kb.tuned_custom_profiles:
                    lines.append(f"# Custom tuned profiles ({len(kb.tuned_custom_profiles)}): "
                                 "included in COPY config/etc/ above")
        elif kb.tuned_custom_profiles:
            lines.append(f"# Custom tuned profiles ({len(kb.tuned_custom_profiles)}): "
                         "included in COPY config/etc/ above")
    lines.append("")

    return lines
