use inspectah_core::traits::executor::Executor;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::types::completeness::{InspectorId, SectionData, SourceSystemKind};
use inspectah_core::types::network::{
    FirewallDirectRule, FirewallZone, NMConnection, NetworkSection, ProxyEntry, StaticRouteFile,
};
use inspectah_core::types::redaction::RedactionHint;
use inspectah_core::types::warnings::Warning;
use std::path::Path;

/// Inspects network configuration: NM connections, firewall zones, routing,
/// hosts additions, proxy settings, and DNS provenance detection.
pub struct NetworkInspector;

impl NetworkInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NetworkInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Inspector for NetworkInspector {
    fn id(&self) -> InspectorId {
        InspectorId::Network
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(&self, ctx: &InspectionContext<'_>) -> Result<InspectorOutput, InspectorError> {
        let exec = ctx.executor;
        let mut warnings: Vec<Warning> = Vec::new();
        let mut hints: Vec<RedactionHint> = Vec::new();
        let mut degraded_reasons: Vec<String> = Vec::new();

        let mut section = NetworkSection {
            connections: Vec::new(),
            firewall_zones: Vec::new(),
            firewall_direct_rules: Vec::new(),
            static_routes: Vec::new(),
            ip_routes: Vec::new(),
            ip_rules: Vec::new(),
            hosts_additions: Vec::new(),
            proxy: Vec::new(),
            ..Default::default()
        };

        collect_nm_connections(exec, &mut section, &mut degraded_reasons);
        collect_firewall_zones(exec, &mut section, &mut warnings, &mut degraded_reasons);
        collect_firewall_direct_rules(exec, &mut section);
        section.resolv_provenance = detect_resolv_provenance(exec);
        collect_hosts_additions(exec, &mut section);
        collect_static_routes(exec, &mut section);
        collect_ip_routes(exec, &mut section, &mut warnings);
        collect_proxy(exec, &mut section, &mut hints);
        collect_dnf_proxy(exec, &mut section, &mut hints);

        let output = InspectorOutput {
            section: SectionData::Network(section),
            warnings,
            redaction_hints: hints,
        };

        if degraded_reasons.is_empty() {
            Ok(output)
        } else {
            Err(InspectorError::Degraded {
                partial: Box::new(output),
                reason: degraded_reasons.join("; "),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// NM connection profiles
// ---------------------------------------------------------------------------

/// Extracts method (dhcp/static) and type (ethernet/wifi/etc.) from a NM
/// keyfile connection profile (INI-style format).
fn classify_connection(text: &str) -> (String, String) {
    let mut method = "unknown".to_string();
    let mut conn_type = String::new();
    let mut current_section = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].to_lowercase();
            // Detect wifi from section name.
            if (current_section == "wifi" || current_section == "802-11-wireless")
                && conn_type.is_empty()
            {
                conn_type = "wifi".to_string();
            }
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let val = val.trim();

            if current_section == "ipv4" && key == "method" {
                method = match val {
                    "manual" => "static".to_string(),
                    "auto" => "dhcp".to_string(),
                    other => other.to_string(),
                };
            }
            if current_section == "connection" && key == "type" {
                conn_type = val.to_string();
            }
        }
    }

    (method, conn_type)
}

/// Scans NM system-connections and legacy network-scripts directories for
/// connection profile files.
fn collect_nm_connections(
    exec: &dyn Executor,
    section: &mut NetworkSection,
    degraded_reasons: &mut Vec<String>,
) {
    let dirs = [
        "/etc/NetworkManager/system-connections",
        "/etc/sysconfig/network-scripts",
    ];

    for subdir in &dirs {
        let dir_path = Path::new(subdir);

        let entries = match exec.read_dir(dir_path) {
            Ok(e) => e,
            Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Directory doesn't exist -- NM or network-scripts not installed.
                continue;
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                degraded_reasons.push(format!(
                    "Permission denied reading {subdir} \
                     -- NM connection profiles may be incomplete"
                ));
                continue;
            }
            Err(_) => continue,
        };

        let mut names: Vec<String> = entries;
        names.sort();

        for name in &names {
            if name.starts_with('.') {
                continue;
            }
            let path = format!("{subdir}/{name}");
            let text = match exec.read_file(Path::new(&path)) {
                Ok(t) => t,
                Err(_) => {
                    degraded_reasons.push(format!(
                        "Failed to read NM connection file {path} -- skipped"
                    ));
                    continue;
                }
            };

            let (method, conn_type) = classify_connection(&text);
            let rel_path = path.strip_prefix('/').unwrap_or(&path).to_string();
            let stem = name
                .rsplit_once('.')
                .map(|(s, _)| s)
                .unwrap_or(name)
                .to_string();

            section.connections.push(NMConnection {
                path: rel_path,
                name: stem,
                method,
                conn_type,
                ..Default::default()
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Firewall zones
// ---------------------------------------------------------------------------

/// Result of parsing a firewalld zone XML file.
struct ZoneParseResult {
    services: Vec<String>,
    ports: Vec<String>,
    rich_rules: Vec<String>,
    /// True if the parser encountered structural issues (unclosed tags,
    /// missing attributes) but still extracted partial data.
    had_parse_errors: bool,
}

/// Parses a firewalld zone XML and extracts services, ports, and rich rules
/// using simple string scanning (no XML crate).
fn parse_zone_xml(text: &str) -> Option<ZoneParseResult> {
    // Verify this looks like a zone document.
    if !text.contains("<zone") {
        return None;
    }

    // Check for unsupported XML features that our hand-parser cannot handle.
    if text.contains("xmlns:") || text.contains("<![CDATA[") {
        return None;
    }

    let mut services = Vec::new();
    let mut ports = Vec::new();
    let mut had_parse_errors = false;

    // Structural check: a well-formed zone document has a closing tag.
    if !text.contains("</zone>") {
        had_parse_errors = true;
    }

    // Extract services: <service name="..."/>
    let mut remaining = text;
    while let Some(start) = remaining.find("<service ") {
        let tag_end = match remaining[start..].find("/>") {
            Some(e) => start + e + 2,
            None => match remaining[start..].find('>') {
                Some(e) => start + e + 1,
                None => {
                    had_parse_errors = true;
                    break;
                }
            },
        };
        let tag = &remaining[start..tag_end];
        match extract_attr(tag, "name") {
            Some(name) if !name.is_empty() => services.push(name),
            Some(_) => {} // empty name, skip
            None => {
                // <service tag without a parseable name= attribute
                had_parse_errors = true;
            }
        }
        remaining = &remaining[tag_end..];
    }

    // Extract ports: <port port="..." protocol="..."/>
    remaining = text;
    while let Some(start) = remaining.find("<port ") {
        let tag_end = match remaining[start..].find("/>") {
            Some(e) => start + e + 2,
            None => match remaining[start..].find('>') {
                Some(e) => start + e + 1,
                None => {
                    had_parse_errors = true;
                    break;
                }
            },
        };
        let tag = &remaining[start..tag_end];
        if let Some(port) = extract_attr(tag, "port") {
            if !port.is_empty() {
                let proto = extract_attr(tag, "protocol").unwrap_or_default();
                if proto.is_empty() {
                    ports.push(port);
                } else {
                    ports.push(format!("{port}/{proto}"));
                }
            }
        }
        remaining = &remaining[tag_end..];
    }

    let rich_rules = extract_rich_rules(text);

    Some(ZoneParseResult {
        services,
        ports,
        rich_rules,
        had_parse_errors,
    })
}

/// Extracts a named XML attribute value from a tag string.
fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let pattern = format!("{name}=\"");
    let start = tag.find(&pattern)?;
    let val_start = start + pattern.len();
    let val_end = tag[val_start..].find('"')?;
    Some(tag[val_start..val_start + val_end].to_string())
}

/// Extracts `<rule>...</rule>` elements from raw XML text via string scanning.
fn extract_rich_rules(text: &str) -> Vec<String> {
    let mut rules = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("<rule") {
        let end = match remaining[start..].find("</rule>") {
            Some(e) => start + e + "</rule>".len(),
            None => break,
        };
        let rule = remaining[start..end].trim().to_string();
        rules.push(rule);
        remaining = &remaining[end..];
    }
    rules
}

/// Reads firewalld zone XML files from /etc/firewalld/zones/.
fn collect_firewall_zones(
    exec: &dyn Executor,
    section: &mut NetworkSection,
    warnings: &mut Vec<Warning>,
    degraded_reasons: &mut Vec<String>,
) {
    let zones_dir = Path::new("/etc/firewalld/zones");

    let entries = match exec.read_dir(zones_dir) {
        Ok(e) => e,
        Err(ref err) if err.kind() == std::io::ErrorKind::NotFound => {
            // firewalld not installed -- silent skip.
            return;
        }
        Err(ref err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            degraded_reasons.push(
                "Permission denied reading /etc/firewalld/zones \
                 -- firewall configuration may be incomplete"
                    .to_string(),
            );
            return;
        }
        Err(_) => {
            warnings.push(Warning {
                inspector: "network".into(),
                message: "Firewall zone directory unreadable \
                          -- firewall configuration may be incomplete."
                    .into(),
                ..Default::default()
            });
            return;
        }
    };

    let mut names = entries;
    names.sort();

    for name in &names {
        if !name.ends_with(".xml") {
            continue;
        }
        let path = format!("/etc/firewalld/zones/{name}");
        let content = match exec.read_file(Path::new(&path)) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = path.strip_prefix('/').unwrap_or(&path).to_string();
        let stem = name.strip_suffix(".xml").unwrap_or(name).to_string();

        match parse_zone_xml(&content) {
            Some(result) => {
                if result.had_parse_errors {
                    degraded_reasons.push(format!(
                        "Firewall zone {name} has malformed XML \
                         -- extracted data may be incomplete"
                    ));
                }
                section.firewall_zones.push(FirewallZone {
                    path: rel_path,
                    name: stem,
                    content: content.clone(),
                    services: result.services,
                    ports: result.ports,
                    rich_rules: result.rich_rules,
                    ..Default::default()
                });
            }
            None => {
                // Completely unparseable or unsupported XML -- degrade but continue.
                degraded_reasons.push(format!("Failed to parse firewall zone {name} -- skipped"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Firewall direct rules
// ---------------------------------------------------------------------------

/// Parses a firewalld direct.xml passthrough rule from a `<passthrough>` tag.
fn parse_passthrough_tag(tag_text: &str, content: &str) -> Option<FirewallDirectRule> {
    let ipv = extract_attr(tag_text, "ipv").unwrap_or_default();
    // Passthrough rules have implicit table/chain from the args.
    // Parse the args to extract chain name from -A flag.
    let args = content.trim().to_string();
    let parts: Vec<&str> = args.split_whitespace().collect();

    let chain = if let Some(pos) = parts.iter().position(|&p| p == "-A") {
        parts.get(pos + 1).unwrap_or(&"").to_string()
    } else {
        String::new()
    };

    Some(FirewallDirectRule {
        ipv,
        table: "filter".to_string(),
        chain,
        priority: "0".to_string(),
        args,
        ..Default::default()
    })
}

/// Reads firewalld direct.xml and extracts passthrough rules.
fn collect_firewall_direct_rules(exec: &dyn Executor, section: &mut NetworkSection) {
    let direct_path = Path::new("/etc/firewalld/direct.xml");
    let content = match exec.read_file(direct_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Parse <passthrough> elements via string scanning.
    let mut remaining = content.as_str();
    while let Some(start) = remaining.find("<passthrough") {
        let tag_close = match remaining[start..].find('>') {
            Some(e) => start + e,
            None => break,
        };
        let tag_text = &remaining[start..=tag_close];
        let content_start = tag_close + 1;
        let end = match remaining[content_start..].find("</passthrough>") {
            Some(e) => content_start + e,
            None => break,
        };
        let inner = &remaining[content_start..end];
        if let Some(rule) = parse_passthrough_tag(tag_text, inner) {
            section.firewall_direct_rules.push(rule);
        }
        remaining = &remaining[end + "</passthrough>".len()..];
    }
}

// ---------------------------------------------------------------------------
// resolv.conf provenance detection
// ---------------------------------------------------------------------------

/// Determines who manages /etc/resolv.conf by checking file content for
/// known generator signatures.
fn detect_resolv_provenance(exec: &dyn Executor) -> String {
    let resolv_path = Path::new("/etc/resolv.conf");
    let content = match exec.read_file(resolv_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    // Check for systemd-resolved signature first.
    for line in content.lines() {
        let lower = line.to_lowercase();
        if lower.contains("systemd-resolve") || lower.contains("resolved") {
            return "systemd-resolved".to_string();
        }
    }
    // Check for NetworkManager signature.
    for line in content.lines() {
        if line.contains("Generated by NetworkManager") {
            return "networkmanager".to_string();
        }
    }
    "hand-edited".to_string()
}

// ---------------------------------------------------------------------------
// /etc/hosts additions
// ---------------------------------------------------------------------------

/// Filters /etc/hosts to only non-localhost entries.
fn collect_hosts_additions(exec: &dyn Executor, section: &mut NetworkSection) {
    let hosts_path = Path::new("/etc/hosts");
    let content = match exec.read_file(hosts_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.contains("localhost") {
            continue;
        }
        // Filter loopback addresses.
        if trimmed.starts_with("127.") || trimmed.starts_with("::1") {
            continue;
        }
        section.hosts_additions.push(trimmed.to_string());
    }
}

// ---------------------------------------------------------------------------
// Static route files
// ---------------------------------------------------------------------------

/// Scans for legacy route-* files and iproute2 configs.
fn collect_static_routes(exec: &dyn Executor, section: &mut NetworkSection) {
    // Legacy network-scripts route files.
    let ns_dir = Path::new("/etc/sysconfig/network-scripts");
    if let Ok(entries) = exec.read_dir(ns_dir) {
        let mut names = entries;
        names.sort();
        for name in &names {
            if name.starts_with("route-") {
                let path = format!("/etc/sysconfig/network-scripts/{name}");
                let rel_path = path.strip_prefix('/').unwrap_or(&path).to_string();
                section.static_routes.push(StaticRouteFile {
                    path: rel_path,
                    name: name.clone(),
                });
            }
        }
    }

    // iproute2 config directory.
    let ip_dir = Path::new("/etc/iproute2");
    if let Ok(entries) = exec.read_dir(ip_dir) {
        let mut names = entries;
        names.sort();
        for name in &names {
            let path = format!("/etc/iproute2/{name}");
            let rel_path = path.strip_prefix('/').unwrap_or(&path).to_string();
            section.static_routes.push(StaticRouteFile {
                path: rel_path,
                name: name.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// ip route / ip rule via executor
// ---------------------------------------------------------------------------

/// Splits `ip route` output into non-empty trimmed lines.
fn parse_ip_routes(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Default routing tables filtered from ip rule output.
const DEFAULT_RULE_TABLES: &[&str] = &["local", "main", "default"];

/// Parses `ip rule` output, filtering out default table rules.
fn parse_ip_rules(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if let Some(idx) = parts.iter().position(|&p| p == "lookup") {
                if let Some(table) = parts.get(idx + 1) {
                    return !DEFAULT_RULE_TABLES.contains(table);
                }
            }
            true
        })
        .map(|l| l.to_string())
        .collect()
}

/// Runs `ip route` and `ip rule` commands, populating the section.
fn collect_ip_routes(
    exec: &dyn Executor,
    section: &mut NetworkSection,
    warnings: &mut Vec<Warning>,
) {
    // ip route
    let result = exec.run("ip", &["route"]);
    if result.exit_code == 0 && !result.stdout.trim().is_empty() {
        section.ip_routes = parse_ip_routes(&result.stdout);
    } else if result.exit_code != 0 {
        warnings.push(Warning {
            inspector: "network".into(),
            message: "ip route failed -- static route information unavailable.".into(),
            ..Default::default()
        });
    }

    // ip rule
    let result = exec.run("ip", &["rule"]);
    if result.exit_code == 0 && !result.stdout.trim().is_empty() {
        section.ip_rules = parse_ip_rules(&result.stdout);
    } else if result.exit_code != 0 {
        warnings.push(Warning {
            inspector: "network".into(),
            message: "ip rule failed -- policy routing rule information unavailable.".into(),
            ..Default::default()
        });
    }
}

// ---------------------------------------------------------------------------
// Proxy settings
// ---------------------------------------------------------------------------

/// Proxy-related environment variable keywords.
const PROXY_KEYWORDS: &[&str] = &["http_proxy", "https_proxy", "no_proxy", "ftp_proxy"];

/// Checks if a line contains any proxy-related variable.
fn is_proxy_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    PROXY_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Checks if a proxy URL contains embedded credentials (user:pass@host pattern).
fn has_embedded_credentials(line: &str) -> bool {
    // Look for ://user:pass@host pattern.
    if let Some(proto_end) = line.find("://") {
        let after_proto = &line[proto_end + 3..];
        // Check for user:pass@host -- must have both : and @ with : before @.
        if let Some(at_pos) = after_proto.find('@') {
            let user_pass = &after_proto[..at_pos];
            return user_pass.contains(':');
        }
    }
    false
}

/// Scans environment files and profile.d for proxy configuration.
fn collect_proxy(
    exec: &dyn Executor,
    section: &mut NetworkSection,
    hints: &mut Vec<RedactionHint>,
) {
    for proxy_path in &["/etc/environment", "/etc/profile.d"] {
        let path = Path::new(proxy_path);
        // Try as file first.
        if let Ok(content) = exec.read_file(path) {
            let rel_path = proxy_path
                .strip_prefix('/')
                .unwrap_or(proxy_path)
                .to_string();
            for line in content.lines() {
                if is_proxy_line(line) {
                    let trimmed = line.trim().to_string();
                    if has_embedded_credentials(&trimmed) {
                        hints.push(RedactionHint {
                            path: rel_path.clone(),
                            reason: "proxy URL contains embedded credentials".to_string(),
                            confidence: None,
                        });
                    }
                    section.proxy.push(ProxyEntry {
                        source: rel_path.clone(),
                        line: trimmed,
                    });
                }
            }
            continue;
        }
        // Try as directory.
        let entries = match exec.read_dir(path) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut names = entries;
        names.sort();
        for name in &names {
            let file_path = format!("{proxy_path}/{name}");
            let file_content = match exec.read_file(Path::new(&file_path)) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let rel_path = file_path
                .strip_prefix('/')
                .unwrap_or(&file_path)
                .to_string();
            for line in file_content.lines() {
                if is_proxy_line(line) {
                    let trimmed = line.trim().to_string();
                    if has_embedded_credentials(&trimmed) {
                        hints.push(RedactionHint {
                            path: rel_path.clone(),
                            reason: "proxy URL contains embedded credentials".to_string(),
                            confidence: None,
                        });
                    }
                    section.proxy.push(ProxyEntry {
                        source: rel_path.clone(),
                        line: trimmed,
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DNF/Yum proxy config
// ---------------------------------------------------------------------------

/// Proxy-related keys in dnf.conf/yum.conf.
const DNF_PROXY_KEYS: &[&str] = &[
    "proxy",
    "proxy_username",
    "proxy_password",
    "proxy_auth_method",
];

/// Scans dnf.conf and yum.conf for proxy settings.
fn collect_dnf_proxy(
    exec: &dyn Executor,
    section: &mut NetworkSection,
    hints: &mut Vec<RedactionHint>,
) {
    for conf_path in &["/etc/dnf/dnf.conf", "/etc/yum.conf"] {
        let content = match exec.read_file(Path::new(conf_path)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel_path = conf_path.strip_prefix('/').unwrap_or(conf_path).to_string();
        for line in content.lines() {
            let stripped = line.trim();
            if stripped.starts_with('#') || !stripped.contains('=') {
                continue;
            }
            if let Some((key, _)) = stripped.split_once('=') {
                let key_lower = key.trim().to_lowercase();
                if DNF_PROXY_KEYS.contains(&key_lower.as_str()) {
                    if key_lower == "proxy_password" {
                        hints.push(RedactionHint {
                            path: rel_path.clone(),
                            reason: "DNF proxy password in plaintext".to_string(),
                            confidence: None,
                        });
                    }
                    section.proxy.push(ProxyEntry {
                        source: rel_path.clone(),
                        line: stripped.to_string(),
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::mock::MockExecutor;
    use inspectah_core::traits::executor::ExecResult;

    fn fixture(name: &str) -> String {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let workspace_root = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap_or(std::path::Path::new(manifest_dir));
        let path = workspace_root.join("testdata/fixtures/network").join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
    }

    // -----------------------------------------------------------------------
    // NM connection classification
    // -----------------------------------------------------------------------

    #[test]
    fn nm_connection_dhcp() {
        let text = fixture("eth0.nmconnection");
        let (method, conn_type) = classify_connection(&text);
        assert_eq!(method, "dhcp", "method=auto should map to dhcp");
        assert_eq!(conn_type, "ethernet");
    }

    #[test]
    fn nm_connection_static() {
        let text =
            "[connection]\ntype=bond\n\n[ipv4]\nmethod=manual\naddress1=10.0.0.5/24,10.0.0.1\n";
        let (method, conn_type) = classify_connection(text);
        assert_eq!(method, "static", "method=manual should map to static");
        assert_eq!(conn_type, "bond");
    }

    #[test]
    fn nm_connection_wifi() {
        let text = "[connection]\ntype=wifi\n\n[wifi]\nssid=MyNetwork\n\n[ipv4]\nmethod=auto\n";
        let (method, conn_type) = classify_connection(text);
        assert_eq!(method, "dhcp");
        assert_eq!(conn_type, "wifi");
    }

    #[test]
    fn nm_malformed_ini_skip_file() {
        // NM dir exists, but file read fails -> Degraded.
        // We can't use with_dir_error for a file read; instead, don't
        // register the file so read_file returns NotFound.
        let exec = MockExecutor::new().with_dir(
            "/etc/NetworkManager/system-connections",
            vec!["bad.nmconnection"],
        );
        // File not registered -> read_file returns NotFound.

        let mut section = NetworkSection::default();
        let mut degraded = Vec::new();
        collect_nm_connections(&exec, &mut section, &mut degraded);
        assert!(section.connections.is_empty(), "bad file should be skipped");
        assert!(
            !degraded.is_empty(),
            "should have degraded reason for unreadable file"
        );
    }

    // -----------------------------------------------------------------------
    // Firewall zones
    // -----------------------------------------------------------------------

    #[test]
    fn firewall_zone_services_and_ports() {
        let xml = fixture("public-zone.xml");
        let result = parse_zone_xml(&xml).expect("valid XML");
        assert_eq!(result.services, vec!["ssh", "dhcpv6-client"]);
        assert_eq!(result.ports, vec!["443/tcp"]);
        assert!(result.rich_rules.is_empty());
        assert!(
            !result.had_parse_errors,
            "valid XML should not have parse errors"
        );
    }

    #[test]
    fn firewall_zone_rich_rules() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<zone>
  <service name="ssh"/>
  <rule family="ipv4">
    <source address="10.0.0.0/8"/>
    <accept/>
  </rule>
</zone>"#;
        let result = parse_zone_xml(xml).expect("valid XML");
        assert_eq!(result.rich_rules.len(), 1);
        assert!(result.rich_rules[0].starts_with("<rule"));
        assert!(result.rich_rules[0].ends_with("</rule>"));
        assert!(result.rich_rules[0].contains("10.0.0.0/8"));
        assert!(!result.had_parse_errors);
    }

    #[test]
    fn firewall_zone_malformed_xml_degraded() {
        // Malformed XML: missing closing quote on <service name="ssh"
        // The simple string scanner is tolerant of structural XML issues —
        // it extracts what it can and returns Some(...) with had_parse_errors=true.
        // The key contract: no panic, best-effort extraction, AND honest
        // Degraded reporting so the caller knows data may be incomplete.
        let xml = fixture("malformed-zone.xml");
        let parse_result = parse_zone_xml(&xml);

        // The scanner returns Some because it finds <zone> and scans for
        // <service>/<port> tags by string matching. The broken <service>
        // tag triggers had_parse_errors because the name attribute is
        // unparseable.
        let result = parse_result.expect("simple scanner is tolerant of malformed XML");
        assert!(
            result.had_parse_errors,
            "malformed XML must set had_parse_errors"
        );

        // Verify via full collector flow that the zone is added AND
        // a degraded reason is produced.
        let exec = MockExecutor::new()
            .with_dir("/etc/firewalld/zones", vec!["malformed.xml"])
            .with_file("/etc/firewalld/zones/malformed.xml", &xml);

        let mut section = NetworkSection::default();
        let mut warnings = Vec::new();
        let mut degraded = Vec::new();
        collect_firewall_zones(&exec, &mut section, &mut warnings, &mut degraded);

        // The zone is added to the section (parser returned Some), but the
        // extracted services/ports may be incomplete or incorrect.
        assert_eq!(
            section.firewall_zones.len(),
            1,
            "malformed zone is added best-effort (parser returned Some)"
        );
        // Degraded reason signals that extracted data may be incomplete.
        assert!(
            !degraded.is_empty(),
            "malformed XML must produce a Degraded reason"
        );
        assert!(
            degraded[0].contains("malformed XML"),
            "degraded reason should mention malformed XML, got: {}",
            degraded[0]
        );
    }

    #[test]
    fn firewall_zone_valid_but_unsupported_xml_degraded() {
        let xml = fixture("unsupported-zone.xml");
        let result = parse_zone_xml(&xml);
        // Contains xmlns: and CDATA which our scanner doesn't support.
        assert!(
            result.is_none(),
            "unsupported XML features should return None"
        );
    }

    // -----------------------------------------------------------------------
    // Firewall direct rules
    // -----------------------------------------------------------------------

    #[test]
    fn firewall_direct_rules() {
        let xml = fixture("direct.xml");
        let exec = MockExecutor::new().with_file("/etc/firewalld/direct.xml", &xml);

        let mut section = NetworkSection::default();
        collect_firewall_direct_rules(&exec, &mut section);

        assert_eq!(section.firewall_direct_rules.len(), 1);
        assert_eq!(section.firewall_direct_rules[0].ipv, "ipv4");
        assert_eq!(section.firewall_direct_rules[0].table, "filter");
        assert_eq!(section.firewall_direct_rules[0].chain, "INPUT");
        assert_eq!(section.firewall_direct_rules[0].priority, "0");
        assert!(section.firewall_direct_rules[0]
            .args
            .contains("--dport 9090"));
    }

    // -----------------------------------------------------------------------
    // IP routes and rules
    // -----------------------------------------------------------------------

    #[test]
    fn ip_route_parsing() {
        let text = fixture("ip-route.txt");
        let routes = parse_ip_routes(&text);
        assert_eq!(routes.len(), 2);
        assert!(routes[0].contains("default via"));
        assert!(routes[1].contains("192.168.1.0/24"));
    }

    #[test]
    fn ip_rule_filtering() {
        let text = fixture("ip-rule.txt");
        let rules = parse_ip_rules(&text);
        // Should filter out local, main, default -- keeping only the custom table.
        assert_eq!(rules.len(), 1);
        assert!(rules[0].contains("lookup 100"));
    }

    #[test]
    fn ip_route_command_failure() {
        let exec = MockExecutor::new()
            .with_command(
                "ip route",
                ExecResult {
                    exit_code: 1,
                    stderr: "command not found".to_string(),
                    ..Default::default()
                },
            )
            .with_command(
                "ip rule",
                ExecResult {
                    exit_code: 1,
                    stderr: "command not found".to_string(),
                    ..Default::default()
                },
            );

        let mut section = NetworkSection::default();
        let mut warnings = Vec::new();
        collect_ip_routes(&exec, &mut section, &mut warnings);

        assert!(section.ip_routes.is_empty());
        assert_eq!(
            warnings.len(),
            2,
            "should warn for both ip route and ip rule failures"
        );
    }

    // -----------------------------------------------------------------------
    // Hosts additions
    // -----------------------------------------------------------------------

    #[test]
    fn hosts_additions_filters_localhost() {
        let hosts = fixture("hosts");
        let exec = MockExecutor::new().with_file("/etc/hosts", &hosts);

        let mut section = NetworkSection::default();
        collect_hosts_additions(&exec, &mut section);

        // Should exclude localhost lines and include the custom entry.
        assert_eq!(section.hosts_additions.len(), 1);
        assert!(section.hosts_additions[0].contains("db.internal"));
    }

    // -----------------------------------------------------------------------
    // resolv.conf provenance
    // -----------------------------------------------------------------------

    #[test]
    fn resolv_provenance_networkmanager() {
        let content = fixture("resolv-nm.conf");
        let exec = MockExecutor::new().with_file("/etc/resolv.conf", &content);
        let result = detect_resolv_provenance(&exec);
        assert_eq!(result, "networkmanager");
    }

    #[test]
    fn resolv_provenance_systemd() {
        let content =
            "# This is /run/systemd/resolve/stub-resolv.conf managed by systemd-resolved(8).\nnameserver 127.0.0.53\n";
        let exec = MockExecutor::new().with_file("/etc/resolv.conf", content);
        let result = detect_resolv_provenance(&exec);
        assert_eq!(result, "systemd-resolved");
    }

    // -----------------------------------------------------------------------
    // Proxy scanning
    // -----------------------------------------------------------------------

    #[test]
    fn proxy_from_environment() {
        let content = fixture("proxy-environment");
        let exec = MockExecutor::new().with_file("/etc/environment", &content);

        let mut section = NetworkSection::default();
        let mut hints = Vec::new();
        collect_proxy(&exec, &mut section, &mut hints);

        assert_eq!(section.proxy.len(), 3);
        assert_eq!(section.proxy[0].source, "etc/environment");
        assert!(section.proxy[0].line.contains("http_proxy"));
    }

    #[test]
    fn proxy_redaction_hint_for_credentials() {
        let content = "https_proxy=http://user:secret@proxy:8080\n";
        let exec = MockExecutor::new().with_file("/etc/environment", &content);

        let mut section = NetworkSection::default();
        let mut hints = Vec::new();
        collect_proxy(&exec, &mut section, &mut hints);

        assert_eq!(section.proxy.len(), 1);
        assert_eq!(hints.len(), 1, "should emit RedactionHint for credentials");
        assert!(hints[0].reason.contains("credentials"));
    }

    #[test]
    fn dnf_proxy() {
        let content = fixture("dnf-proxy.conf");
        let exec = MockExecutor::new().with_file("/etc/dnf/dnf.conf", &content);

        let mut section = NetworkSection::default();
        let mut hints = Vec::new();
        collect_dnf_proxy(&exec, &mut section, &mut hints);

        assert_eq!(section.proxy.len(), 3); // proxy, proxy_username, proxy_password
        assert_eq!(section.proxy[0].source, "etc/dnf/dnf.conf");
        assert!(section.proxy[0].line.contains("proxy="));
        assert_eq!(
            hints.len(),
            1,
            "should emit RedactionHint for proxy_password"
        );
        assert!(hints[0].reason.contains("proxy password"));
    }

    // -----------------------------------------------------------------------
    // is_proxy_line
    // -----------------------------------------------------------------------

    #[test]
    fn is_proxy_line_true_cases() {
        assert!(is_proxy_line("http_proxy=http://proxy:8080"));
        assert!(is_proxy_line("HTTPS_PROXY=http://proxy:8080"));
        assert!(is_proxy_line("no_proxy=localhost"));
        assert!(is_proxy_line("export FTP_PROXY=http://proxy:21"));
    }

    #[test]
    fn is_proxy_line_false_cases() {
        assert!(!is_proxy_line("PATH=/usr/bin:/usr/sbin"));
        assert!(!is_proxy_line("# this is a comment"));
        assert!(!is_proxy_line("EDITOR=vim"));
        assert!(!is_proxy_line(""));
    }
}
