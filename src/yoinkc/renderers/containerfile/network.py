"""Containerfile section: firewall configuration and network/kickstart notes."""

from ...schema import InspectionSnapshot


def section_lines(
    snapshot: InspectionSnapshot,
    *,
    firewall_only: bool,
) -> list[str]:
    """Return firewall or network lines depending on firewall_only flag."""
    if firewall_only:
        return _firewall_lines(snapshot)
    return _network_lines(snapshot)


def _firewall_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Firewall comment block."""
    lines: list[str] = []
    net = snapshot.network
    fw_zones = [z for z in net.firewall_zones if z.include] if net else []
    fw_direct = [r for r in net.firewall_direct_rules if r.include] if net else []
    if fw_zones or fw_direct:
        lines.append("# === Firewall Configuration (bake into image) ===")
        if fw_zones:
            total_rich = sum(len(z.rich_rules) for z in fw_zones)
            lines.append(f"# Detected: {len(fw_zones)} zone(s)"
                         + (f", {total_rich} rich rule(s)" if total_rich else "")
                         + " — included in COPY config/etc/ below")
        if fw_direct:
            lines.append(f"# Detected: {len(fw_direct)} direct rule(s) — included in COPY config/etc/ below")
        lines.append("# See audit-report.md for firewall-offline-cmd equivalents per zone.")
        lines.append("")
    return lines


def _network_lines(snapshot: InspectionSnapshot) -> list[str]:
    """Network / kickstart note."""
    lines: list[str] = []
    net = snapshot.network
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

    _DNF_PROXY_SOURCES = ("etc/dnf/dnf.conf", "etc/yum.conf")
    if net and net.proxy:
        env_entries = [p for p in net.proxy if p.source not in _DNF_PROXY_SOURCES]
        dnf_entries = [p for p in net.proxy if p.source in _DNF_PROXY_SOURCES]
        if env_entries:
            lines.append("# Proxy settings detected — bake as environment defaults")
            env_lines = [p.line for p in env_entries if "=" in p.line]
            if env_lines:
                lines.append("RUN mkdir -p /etc/environment.d && cat > /etc/environment.d/proxy.conf << 'PROXYEOF'")
                for el in env_lines:
                    lines.append(el)
                lines.append("PROXYEOF")
        if dnf_entries:
            lines.append("# DNF proxy configured — preserved in etc/dnf/dnf.conf (included in COPY config/etc/)")
            for p in dnf_entries:
                lines.append(f"#   {p.line}")

    if net and net.static_routes:
        lines.append(f"# {len(net.static_routes)} static route file(s) detected")
        lines.append("# FIXME: add static routes via NM connection or nmstatectl config")
        for r in net.static_routes[:10]:
            lines.append(f"# Route file: {r.path} — review and translate to NM connection (+ipv4.routes)")
    lines.append("")
    return lines
