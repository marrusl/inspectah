"""Scheduled Task inspector: cron, systemd timers, at jobs.

Scans all cron locations, existing systemd .timer units (both vendor and
local), at spool files, and generates timer units from cron entries.
"""

import re
from pathlib import Path
from typing import Dict, List, Optional

from ..executor import Executor
from ..schema import ScheduledTaskSection


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

def _cron_to_on_calendar(cron_expr: str) -> str:
    """Convert simple cron (min hour * * *) to systemd OnCalendar."""
    parts = cron_expr.strip().split()
    if len(parts) >= 2 and parts[0].isdigit() and parts[1].isdigit():
        m, h = int(parts[0]), int(parts[1])
        return f"*-*-* {h:02d}:{m:02d}:00"
    return "*-*-* 02:00:00"


def _make_timer_service(name: str, cron_expr: str, path: str, command: str = "") -> tuple:
    on_calendar = _cron_to_on_calendar(cron_expr)
    timer_content = (
        f"[Unit]\nDescription=Generated from cron: {path}\n"
        f"# Original cron: {cron_expr}\n\n"
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
    section.cron_jobs.append({"path": rel, "source": source})
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
                    section.generated_timer_units.append({
                        "name": safe_name,
                        "timer_content": timer_content,
                        "service_content": service_content,
                        "cron_expr": cron_expr,
                        "source_path": rel,
                        "command": command,
                    })
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

        results.append({
            "name": name,
            "on_calendar": on_calendar,
            "exec_start": exec_start,
            "description": description,
            "source": source_label,
            "path": str(f.relative_to(host_root)),
            "timer_content": timer_text,
            "service_content": service_text,
        })
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

    return {
        "file": rel,
        "command": command,
        "user": user,
        "working_dir": working_dir,
    }


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
                    section.cron_jobs.append({"path": rel, "source": f"cron.{period}"})

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
