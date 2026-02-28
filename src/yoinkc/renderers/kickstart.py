"""kickstart-suggestion.ks renderer: deploy-time settings suggestion."""

from pathlib import Path

from jinja2 import Environment

from ..schema import InspectionSnapshot


def render(
    snapshot: InspectionSnapshot,
    env: Environment,
    output_dir: Path,
) -> None:
    output_dir = Path(output_dir)
    lines = [
        "# Kickstart suggestion — review and adapt for your environment",
        "# These settings belong at deploy time, not baked into the image.",
        "",
    ]

    if snapshot.network:
        dhcp_conns = [c for c in (snapshot.network.connections or []) if c.get("method") == "dhcp"]
        static_conns = [c for c in (snapshot.network.connections or []) if c.get("method") == "static"]
        if dhcp_conns:
            lines.append("# --- DHCP connections (deploy-time config) ---")
            for c in dhcp_conns:
                name = c.get("name", "eth0")
                lines.append(f"network --bootproto=dhcp --device={name}")
            lines.append("")
        if static_conns:
            lines.append("# --- Static connections (baked into image — shown here for reference) ---")
            for c in static_conns:
                name = c.get("name", "eth0")
                lines.append(f"# network --bootproto=static --device={name}  # already in image")
            lines.append("")

        if snapshot.network.hosts_additions:
            lines.append("# --- /etc/hosts additions detected ---")
            for h in snapshot.network.hosts_additions:
                lines.append(f"# {h}")
            lines.append("")

        if snapshot.network.resolv_provenance:
            lines.append("# --- DNS configuration ---")
            lines.append("# network --nameserver=<DNS_IP>")
            lines.append("")

        if snapshot.network.proxy:
            lines.append("# --- Proxy settings detected ---")
            for p in snapshot.network.proxy:
                lines.append(f"# {p.get('line') or ''}")
            lines.append("")

    hostname = ""
    if snapshot.meta:
        hostname = snapshot.meta.get("hostname") or ""
    if hostname:
        lines.append(f"# network --hostname={hostname}")
        lines.append("")

    # Static routes and policy routing
    if snapshot.network and snapshot.network.static_routes:
        lines.append("# --- Static route files detected ---")
        lines.append("# These files were present on the source host. Review each and translate")
        lines.append("# to NM connection properties (+ipv4.routes) or kickstart route directives.")
        for r in snapshot.network.static_routes:
            path = r.get("path", "")
            lines.append(f"# FIXME: review {path} and add equivalent route to NM connection or kickstart")
        lines.append("")

    if snapshot.network and snapshot.network.ip_rules:
        policy_rules = [r for r in snapshot.network.ip_rules if r.strip()]
        if policy_rules:
            lines.append("# --- Policy routing rules detected ---")
            for r in policy_rules[:10]:
                lines.append(f"# ip rule: {r}")
            lines.append("")

    # Environment variables (proxy, etc.)
    if snapshot.network and snapshot.network.proxy:
        lines.append("# --- Proxy environment variables ---")
        lines.append("# Deploy-time: set via systemd drop-in or environment file.")
        lines.append("%post")
        lines.append("cat > /etc/environment.d/proxy.conf << 'PROXYEOF'")
        for p in snapshot.network.proxy:
            var_line = p.get("line", "")
            if "=" in var_line:
                lines.append(var_line)
        lines.append("PROXYEOF")
        lines.append("%end")
        lines.append("")

    lines.append("# --- Examples ---")
    lines.append("# network --bootproto=dhcp --device=eth0")
    lines.append("# network --hostname=myhost.example.com")
    lines.append("# network --bootproto=static --ip=192.168.1.10 --netmask=255.255.255.0 --gateway=192.168.1.1")
    lines.append("")

    if snapshot.storage:
        nfs_mounts = [e for e in (snapshot.storage.fstab_entries or []) if "nfs" in (e.get("fstype") or "").lower()]
        cifs_mounts = [e for e in (snapshot.storage.fstab_entries or []) if "cifs" in (e.get("fstype") or "").lower()]
        if nfs_mounts or cifs_mounts:
            lines.append("# --- Remote filesystem mounts detected ---")
            for m in nfs_mounts:
                dev = m.get("device") or ""
                mp = m.get("mount_point") or ""
                opts = m.get("options") or ""
                lines.append(f"# NFS: {dev} → {mp}")
                if "sec=" in opts:
                    lines.append(f"#   mount options include Kerberos auth: {opts}")
                else:
                    lines.append(f"#   FIXME: provide NFS credentials at deploy time")
            for m in cifs_mounts:
                dev = m.get("device") or ""
                mp = m.get("mount_point") or ""
                lines.append(f"# CIFS: {dev} → {mp}")
                lines.append(f"#   FIXME: provide CIFS credentials (username/password) at deploy time")
            lines.append("")
            lines.append("# Mount remote filesystems in %post or via systemd .mount units:")
            lines.append("%post")
            for m in nfs_mounts:
                lines.append(f"# mkdir -p {m.get('mount_point', '/mnt/nfs')}")
                lines.append(f"# echo '{m.get('device', '')} {m.get('mount_point', '')} nfs defaults 0 0' >> /etc/fstab")
            for m in cifs_mounts:
                lines.append(f"# mkdir -p {m.get('mount_point', '/mnt/cifs')}")
                lines.append(f"# echo '{m.get('device', '')} {m.get('mount_point', '')} cifs credentials=/etc/samba/creds 0 0' >> /etc/fstab")
            lines.append("%end")
            lines.append("")

    (output_dir / "kickstart-suggestion.ks").write_text("\n".join(lines))
