"""User/Group inspector: non-system users and groups, sudoers, SSH key refs. Parses passwd/group under host_root."""

from pathlib import Path
from typing import List, Optional

from ..executor import Executor
from ..schema import UserGroupSection
from .._util import debug as _debug_fn, safe_iterdir as _safe_iterdir


def _debug(msg: str) -> None:
    _debug_fn("users", msg)


def _safe_read_file(p: Path) -> Optional[str]:
    """Read a file, returning its content or None on any error."""
    try:
        if p.exists():
            text = p.read_text()
            _debug(f"read {p} ({len(text)} bytes)")
            return text
        _debug(f"{p} does not exist")
    except (PermissionError, OSError) as exc:
        _debug(f"cannot read {p}: {exc}")
    return None


def run(
    host_root: Path,
    executor: Optional[Executor],
) -> UserGroupSection:
    section = UserGroupSection()
    host_root = Path(host_root)

    passwd_path = host_root / "etc/passwd"
    _debug(f"checking {passwd_path}")
    passwd_text = _safe_read_file(passwd_path)

    non_system_users: set = set()

    if passwd_text:
        for line in passwd_text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                parts = line.split(":")
                if len(parts) >= 7:
                    try:
                        uid = int(parts[2])
                        if 1000 <= uid < 60000:
                            username = parts[0]
                            non_system_users.add(username)
                            _debug(f"found user: {username} uid={uid} home={parts[5]} shell={parts[6]}")
                            section.users.append({
                                "name": username,
                                "uid": uid,
                                "gid": int(parts[3]) if parts[3].isdigit() else None,
                                "shell": parts[6],
                                "home": parts[5],
                            })
                            section.passwd_entries.append(line)
                    except ValueError:
                        pass

    _debug(f"found {len(section.users)} non-system users (uid >= 1000)")

    # /etc/shadow — match by username from passwd
    shadow_path = host_root / "etc/shadow"
    shadow_text = _safe_read_file(shadow_path)
    if shadow_text:
        for line in shadow_text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                username = line.split(":")[0]
                if username in non_system_users:
                    section.shadow_entries.append(line)

    # /etc/group
    group_path = host_root / "etc/group"
    _debug(f"checking {group_path}")
    group_text = _safe_read_file(group_path)
    non_system_groups: set = set()

    if group_text:
        for line in group_text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                parts = line.split(":")
                if len(parts) >= 3:
                    try:
                        gid = int(parts[2])
                        if 1000 <= gid < 60000:
                            non_system_groups.add(parts[0])
                            members = parts[3].split(",") if len(parts) > 3 and parts[3] else []
                            section.groups.append({"name": parts[0], "gid": gid, "members": members})
                            section.group_entries.append(line)
                    except ValueError:
                        pass

    _debug(f"found {len(section.groups)} non-system groups (gid >= 1000)")

    # /etc/gshadow — match by group name
    gshadow_path = host_root / "etc/gshadow"
    gshadow_text = _safe_read_file(gshadow_path)
    if gshadow_text:
        for line in gshadow_text.splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                groupname = line.split(":")[0]
                if groupname in non_system_groups:
                    section.gshadow_entries.append(line)

    # /etc/subuid and /etc/subgid — match by username
    for attr, filename in (("subuid_entries", "etc/subuid"), ("subgid_entries", "etc/subgid")):
        fpath = host_root / filename
        text = _safe_read_file(fpath)
        if text:
            for line in text.splitlines():
                line = line.strip()
                if line and not line.startswith("#"):
                    username = line.split(":")[0]
                    if username in non_system_users:
                        getattr(section, attr).append(line)

    for sudoers_path in ("etc/sudoers", "etc/sudoers.d"):
        sp = host_root / sudoers_path
        if sp.is_file():
            try:
                for line in sp.read_text().splitlines():
                    line = line.strip()
                    if line and not line.startswith("#") and not line.startswith("Defaults"):
                        section.sudoers_rules.append(line)
            except Exception:
                pass
        elif sp.is_dir():
            for f in _safe_iterdir(sp):
                if f.is_file() and not f.name.startswith("."):
                    try:
                        for line in f.read_text().splitlines():
                            line = line.strip()
                            if line and not line.startswith("#") and not line.startswith("Defaults"):
                                section.sudoers_rules.append(line)
                    except Exception:
                        pass

    for user_entry in section.users:
        home = user_entry.get("home", "")
        if home:
            auth_keys = host_root / home.lstrip("/") / ".ssh" / "authorized_keys"
            try:
                if auth_keys.exists():
                    section.ssh_authorized_keys_refs.append({
                        "user": user_entry["name"],
                        "path": f"{home}/.ssh/authorized_keys",
                    })
            except (PermissionError, OSError):
                pass

    return section
