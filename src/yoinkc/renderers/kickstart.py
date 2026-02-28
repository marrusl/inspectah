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
        dhcp_conns = [c for c in snapshot.network.connections if c.method == "dhcp"]
        static_conns = [c for c in snapshot.network.connections if c.method == "static"]
        if dhcp_conns:
            lines.append("# --- DHCP connections (deploy-time config) ---")
            for c in dhcp_conns:
                lines.append(f"network --bootproto=dhcp --device={c.name}")
            lines.append("")
        if static_conns:
            lines.append("# --- Static connections (baked into image — shown here for reference) ---")
            for c in static_conns:
                lines.append(f"# network --bootproto=static --device={c.name}  # already in image")
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
                lines.append(f"# {p.line}")
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
            lines.append(f"# FIXME: review {r.path} and add equivalent route to NM connection or kickstart")
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
            if "=" in p.line:
                lines.append(p.line)
        lines.append("PROXYEOF")
        lines.append("%end")
        lines.append("")

    lines.append("# --- Examples ---")
    lines.append("# network --bootproto=dhcp --device=eth0")
    lines.append("# network --hostname=myhost.example.com")
    lines.append("# network --bootproto=static --ip=192.168.1.10 --netmask=255.255.255.0 --gateway=192.168.1.1")
    lines.append("")

    if snapshot.storage:
        nfs_mounts = [e for e in snapshot.storage.fstab_entries if "nfs" in e.fstype.lower()]
        cifs_mounts = [e for e in snapshot.storage.fstab_entries if "cifs" in e.fstype.lower()]
        if nfs_mounts or cifs_mounts:
            lines.append("# --- Remote filesystem mounts detected ---")
            for m in nfs_mounts:
                lines.append(f"# NFS: {m.device} → {m.mount_point}")
                lines.append(f"#   FIXME: provide NFS credentials at deploy time")
            for m in cifs_mounts:
                lines.append(f"# CIFS: {m.device} → {m.mount_point}")
                lines.append(f"#   FIXME: provide CIFS credentials (username/password) at deploy time")
            lines.append("")
            lines.append("# Mount remote filesystems in %post or via systemd .mount units:")
            lines.append("%post")
            for m in nfs_mounts:
                lines.append(f"# mkdir -p {m.mount_point or '/mnt/nfs'}")
                lines.append(f"# echo '{m.device} {m.mount_point} nfs defaults 0 0' >> /etc/fstab")
            for m in cifs_mounts:
                lines.append(f"# mkdir -p {m.mount_point or '/mnt/cifs'}")
                lines.append(f"# echo '{m.device} {m.mount_point} cifs credentials=/etc/samba/creds 0 0' >> /etc/fstab")
            lines.append("%end")
            lines.append("")

    (output_dir / "kickstart-suggestion.ks").write_text("\n".join(lines))
