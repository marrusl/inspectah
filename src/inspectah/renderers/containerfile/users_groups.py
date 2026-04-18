"""Containerfile section: users and groups (strategy-aware rendering)."""

from ...schema import InspectionSnapshot


def section_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Return Containerfile lines for users and groups."""
    lines: list[str] = []

    ug = snapshot.users_groups
    _included_users = [u for u in (ug.users or []) if u.get("include", True)] if ug else []
    if not (ug and _included_users):
        return lines

    lines.append("# === Users and Groups ===")

    # Group users/groups by strategy
    sysusers_users = [u for u in _included_users if u.get("strategy") == "sysusers"]
    useradd_users = [u for u in _included_users if u.get("strategy") == "useradd"]
    blueprint_users = [u for u in _included_users if u.get("strategy") == "blueprint"]
    kickstart_users = [u for u in _included_users if u.get("strategy") == "kickstart"]

    # --- sysusers strategy ---
    if sysusers_users:
        names = ", ".join(u.get("name", "") for u in sysusers_users)
        lines.append(f"# Service accounts ({len(sysusers_users)}): {names}")
        lines.append("# Created at boot by systemd-sysusers — declarative, no RUN needed")
        lines.append("COPY config/usr/lib/sysusers.d/inspectah-users.conf /usr/lib/sysusers.d/inspectah-users.conf")
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

    # --- blueprint strategy ---
    if blueprint_users:
        names = ", ".join(u.get("name", "") for u in blueprint_users)
        lines.append(f"# Users managed via blueprint ({len(blueprint_users)}): {names}")
        lines.append("# See inspectah-users.toml for bootc-image-builder customization")

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

    return lines
