"""Scheduled Task inspector: cron, systemd timers, at jobs.

Scans all cron locations, existing systemd .timer units (both vendor and
local), at spool files, and generates timer units from cron entries.
"""

import re
from pathlib import Path
from typing import Dict, List, Optional

from ..executor import Executor
from ..schema import (
    ScheduledTaskSection, CronJob, SystemdTimer, AtJob, GeneratedTimerUnit,
)


def _safe_iterdir(d: Path) -> List[Path]:
    try:
        return sorted(d.iterdir())
    except (PermissionError, OSError):
        return []


def _safe_read(p: Path) -> str:
    try:
        return p.read_text()
    except (PermissionError, OSError):
        return ""


# ---------------------------------------------------------------------------
# Cron helpers
# ---------------------------------------------------------------------------

def _cron_field_to_calendar(field: str, kind: str) -> str:
    """Convert a single cron field to its systemd OnCalendar equivalent.

    *kind* is one of ``minute``, ``hour``, ``dom``, ``month``, ``dow``.
    Returns the calendar fragment or the field unchanged if it already maps
    cleanly (e.g. ``*`` stays ``*``).
    """
    if field == "*":
        return "*"

    # Step values: */5 → *:00/5 (for minute), */2 → 00/2 (for hour), etc.
    if field.startswith("*/"):
        step = field[2:]
        if step.isdigit():
            if kind == "minute":
                return f"*/{step}"
            if kind == "hour":
                return f"00/{ step}"
            # dom, month, dow: systemd doesn't support steps directly
            return field
        return field

    # Ranges: 1-5 → 1..5
    if "-" in field and "/" not in field:
        parts = field.split("-")
        if len(parts) == 2 and all(p.isdigit() for p in parts):
            return f"{parts[0]}..{parts[1]}"

    # Lists: 1,3,5 → 1,3,5 (same syntax)
    if "," in field:
        return field

    # Numeric day of week: 0=Sun, 1=Mon, ... 7=Sun — convert to names
    dow_names = {"0": "Sun", "1": "Mon", "2": "Tue", "3": "Wed",
                 "4": "Thu", "5": "Fri", "6": "Sat", "7": "Sun"}
    if kind == "dow" and field in dow_names:
        return dow_names[field]

    # Plain digit
    if field.isdigit():
        if kind in ("minute", "hour"):
            return f"{int(field):02d}"
        return field

    return field


def _cron_to_on_calendar(cron_expr: str) -> tuple:
    """Convert a 5-field cron expression to a systemd OnCalendar value.

    Returns ``(on_calendar, converted)`` where *converted* is True if the
    expression was fully handled, False if a fallback was used.

    Handles: literal values, ``*``, step (``*/N``), ranges (``M-N``),
    lists (``M,N``), and named shortcuts (``@daily``, ``@reboot``, etc.).
    """
    expr = cron_expr.strip()

    # Named shortcuts
    _SHORTCUTS = {
        "@yearly":  ("*-01-01 00:00:00", True),
        "@annually": ("*-01-01 00:00:00", True),
        "@monthly": ("*-*-01 00:00:00", True),
        "@weekly":  ("Mon *-*-* 00:00:00", True),
        "@daily":   ("*-*-* 00:00:00", True),
        "@midnight": ("*-*-* 00:00:00", True),
        "@hourly":  ("*-*-* *:00:00", True),
    }
    if expr.lower() in _SHORTCUTS:
        return _SHORTCUTS[expr.lower()]

    # @reboot has no calendar equivalent
    if expr.lower() == "@reboot":
        return ("@reboot", False)

    parts = expr.split()
    if len(parts) < 5:
        return ("*-*-* 02:00:00", False)

    minute, hour, dom, month, dow = parts[:5]

    cal_min = _cron_field_to_calendar(minute, "minute")
    cal_hour = _cron_field_to_calendar(hour, "hour")
    cal_dom = _cron_field_to_calendar(dom, "dom")
    cal_month = _cron_field_to_calendar(month, "month")
    cal_dow = _cron_field_to_calendar(dow, "dow")

    # Build OnCalendar: [DOW] YYYY-MM-DD HH:MM:SS
    date_part = f"*-{cal_month}-{cal_dom}"
    time_part = f"{cal_hour}:{cal_min}:00"

    if cal_dow != "*":
        return (f"{cal_dow} {date_part} {time_part}", True)
    return (f"{date_part} {time_part}", True)


def _make_timer_service(name: str, cron_expr: str, path: str, command: str = "") -> tuple:
    on_calendar, converted = _cron_to_on_calendar(cron_expr)

    fixme_lines = ""
    if not converted:
        if on_calendar == "@reboot":
            fixme_lines = (
                "# FIXME: @reboot has no OnCalendar equivalent.\n"
                "# Use a oneshot service with WantedBy=multi-user.target instead.\n"
            )
            on_calendar = "*-*-* 02:00:00"
        else:
            fixme_lines = (
                f"# FIXME: cron expression '{cron_expr}' could not be fully converted.\n"
                "# Review and correct the OnCalendar value below.\n"
            )

    timer_content = (
        f"[Unit]\nDescription=Generated from cron: {path}\n"
        f"# Original cron: {cron_expr}\n"
        f"{fixme_lines}\n"
        f"[Timer]\nOnCalendar={on_calendar}\nPersistent=true\n\n"
        "[Install]\nWantedBy=timers.target\n"
    )
    if command:
        exec_line = f"ExecStart={command}"
    else:
        exec_line = "ExecStart=/bin/true\n# FIXME: could not extract command from cron entry"
    service_content = (
        f"[Unit]\nDescription=Timer from cron {path}\n\n"
        f"[Service]\nType=oneshot\n{exec_line}\n"
    )
    return timer_content, service_content


def _scan_cron_file(section: ScheduledTaskSection, host_root: Path, f: Path, source: str) -> None:
    rel = str(f.relative_to(host_root))
    section.cron_jobs.append(CronJob(path=rel, source=source))
    try:
        text = f.read_text()
        for line in text.splitlines():
            line = line.strip()
            if line and not line.startswith("#") and re.match(r"^[\d*]", line):
                parts = line.split()
                if len(parts) >= 6:
                    cron_expr = " ".join(parts[:5])
                    # System crontabs (cron.d) have a user field at position 5
                    if source in ("cron.d", "crontab"):
                        command = " ".join(parts[6:]) if len(parts) > 6 else ""
                    else:
                        command = " ".join(parts[5:])
                    safe_name = "cron-" + f.name.replace(".", "-")
                    timer_content, service_content = _make_timer_service(
                        safe_name, cron_expr, rel, command=command,
                    )
                    section.generated_timer_units.append(GeneratedTimerUnit(
                        name=safe_name,
                        timer_content=timer_content,
                        service_content=service_content,
                        cron_expr=cron_expr,
                        source_path=rel,
                        command=command,
                    ))
    except Exception:
        pass


# ---------------------------------------------------------------------------
# Systemd timer scanner
# ---------------------------------------------------------------------------

def _parse_unit_field(text: str, field: str) -> str:
    """Extract the first value of *field*= from a unit file."""
    for line in text.splitlines():
        line = line.strip()
        if line.startswith(f"{field}="):
            return line.split("=", 1)[1].strip()
    return ""


def _scan_systemd_timers(
    host_root: Path, base_dir: str, source_label: str,
) -> List[dict]:
    """Scan a systemd unit directory for .timer files and their .service pairs."""
    results: List[dict] = []
    d = host_root / base_dir
    if not d.exists():
        return results

    for f in _safe_iterdir(d):
        if not f.is_file() or not f.name.endswith(".timer"):
            continue
        timer_text = _safe_read(f)
        if not timer_text:
            continue

        name = f.stem
        on_calendar = _parse_unit_field(timer_text, "OnCalendar")
        description = _parse_unit_field(timer_text, "Description")

        service_file = f.with_suffix(".service")
        service_text = _safe_read(service_file) if service_file.exists() else ""
        exec_start = _parse_unit_field(service_text, "ExecStart")

        results.append(SystemdTimer(
            name=name,
            on_calendar=on_calendar,
            exec_start=exec_start,
            description=description,
            source=source_label,
            path=str(f.relative_to(host_root)),
            timer_content=timer_text,
            service_content=service_text,
        ))
    return results


# ---------------------------------------------------------------------------
# At job parser
# ---------------------------------------------------------------------------

def _parse_at_job(host_root: Path, f: Path) -> Dict[str, str]:
    """Parse an at spool file to extract the command, user, and working dir."""
    rel = str(f.relative_to(host_root))
    text = _safe_read(f)
    if not text:
        return {"file": rel, "command": "", "user": "", "working_dir": ""}

    lines = text.splitlines()
    user = ""
    working_dir = ""
    command_lines: List[str] = []

    in_preamble = True
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("# atrun uid="):
            m = re.match(r"# atrun uid=(\d+)", stripped)
            if m:
                user = f"uid={m.group(1)}"
        if stripped.startswith("# mail "):
            parts = stripped.split()
            if len(parts) >= 3:
                user = parts[2]
        if stripped.startswith("cd ") and in_preamble:
            working_dir = stripped.split()[1].rstrip("|") if len(stripped.split()) > 1 else ""
            # "cd /root || {" → extract /root
            working_dir = working_dir.split("||")[0].strip()
            continue
        if in_preamble and (stripped.startswith("#!/") or stripped.startswith("#")
                           or stripped.startswith("umask") or stripped == ""
                           or stripped.startswith("cd ")
                           or "export" in stripped
                           or stripped.startswith("SHELL=")
                           or stripped.startswith("echo") and "inaccessible" in stripped
                           or stripped.startswith("exit")
                           or stripped == "}"):
            continue
        in_preamble = False
        if stripped:
            command_lines.append(stripped)

    command = "; ".join(command_lines) if command_lines else ""

    return AtJob(file=rel, command=command, user=user, working_dir=working_dir)


# ---------------------------------------------------------------------------
# Main entry
# ---------------------------------------------------------------------------

def run(
    host_root: Path,
    executor: Optional[Executor],
) -> ScheduledTaskSection:
    section = ScheduledTaskSection()
    host_root = Path(host_root)

    # --- Cron ---
    cron_d = host_root / "etc/cron.d"
    if cron_d.exists():
        for f in _safe_iterdir(cron_d):
            if f.is_file() and not f.name.startswith("."):
                _scan_cron_file(section, host_root, f, "cron.d")

    crontab = host_root / "etc/crontab"
    try:
        if crontab.exists():
            _scan_cron_file(section, host_root, crontab, "crontab")
    except (PermissionError, OSError):
        pass

    for period in ("hourly", "daily", "weekly", "monthly"):
        d = host_root / f"etc/cron.{period}"
        if d.exists():
            for f in _safe_iterdir(d):
                if f.is_file() and not f.name.startswith("."):
                    rel = str(f.relative_to(host_root))
                    section.cron_jobs.append(CronJob(path=rel, source=f"cron.{period}"))

    spool = host_root / "var/spool/cron"
    if spool.exists():
        for f in _safe_iterdir(spool):
            if f.is_file() and not f.name.startswith("."):
                _scan_cron_file(section, host_root, f, f"spool/cron ({f.name})")

    # --- Existing systemd timers ---
    for base_dir, label in [
        ("etc/systemd/system", "local"),
        ("usr/lib/systemd/system", "vendor"),
    ]:
        section.systemd_timers.extend(
            _scan_systemd_timers(host_root, base_dir, label)
        )

    # --- At jobs ---
    at_spool = host_root / "var/spool/at"
    if at_spool.exists():
        for f in _safe_iterdir(at_spool):
            if f.is_file() and not f.name.startswith("."):
                section.at_jobs.append(_parse_at_job(host_root, f))

    return section
