use super::FindingKind;
use super::aggregate::AggregatePrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NMConnection {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default, rename = "type")]
    pub conn_type: String,
    // Field-level `serde(default)` uses `FindingKind::default()` (actionable,
    // included), not the `Default for NMConnection` impl below. Network
    // findings are inventory, so the default must be named explicitly.
    #[serde(default = "crate::types::finding::default_finding_inventory")]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

impl Default for NMConnection {
    fn default() -> Self {
        Self {
            disposition: FindingKind::inventory(),
            path: Default::default(),
            name: Default::default(),
            method: Default::default(),
            conn_type: Default::default(),
            locked: Default::default(),
            acknowledged: Default::default(),
            aggregate: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallZone {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub rich_rules: Vec<String>,
    #[serde(default = "crate::types::finding::default_finding_inventory")]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub aggregate: Option<AggregatePrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallDirectRule {
    #[serde(default)]
    pub ipv: String,
    #[serde(default)]
    pub table: String,
    #[serde(default)]
    pub chain: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub args: String,
    #[serde(default = "crate::types::finding::default_finding_inventory")]
    pub disposition: FindingKind,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticRouteFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyEntry {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkSection {
    #[serde(default)]
    pub connections: Vec<NMConnection>,
    #[serde(default)]
    pub firewall_zones: Vec<FirewallZone>,
    #[serde(default)]
    pub firewall_direct_rules: Vec<FirewallDirectRule>,
    #[serde(default)]
    pub static_routes: Vec<StaticRouteFile>,
    #[serde(default)]
    pub ip_routes: Vec<String>,
    #[serde(default)]
    pub ip_rules: Vec<String>,
    #[serde(default)]
    pub resolv_provenance: String,
    #[serde(default)]
    pub hosts_additions: Vec<String>,
    #[serde(default)]
    pub proxy: Vec<ProxyEntry>,
}

/// Contextual note shown when the source host uses ifcfg network scripts
/// and the target is RHEL 9+.
pub const IFCFG_DEPRECATION_NOTE: &str = "Source host uses ifcfg network scripts. \
    RHEL 9+ targets use NetworkManager keyfiles by default. \
    ifcfg support is deprecated in RHEL 9 and removed in RHEL 10. \
    Plan network configuration separately for the target environment.";

impl NetworkSection {
    /// True when any connection was collected from the legacy
    /// `/etc/sysconfig/network-scripts/` directory (ifcfg format).
    pub fn has_ifcfg_connections(&self) -> bool {
        self.connections
            .iter()
            .any(|c| c.path.contains("network-scripts"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_section_roundtrip() {
        let section = NetworkSection {
            connections: vec![NMConnection {
                path: "/etc/NetworkManager/system-connections/eth0.nmconnection".into(),
                name: "eth0".into(),
                method: "auto".into(),
                conn_type: "802-3-ethernet".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: NetworkSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn absent_disposition_deserializes_as_inventory() {
        // A snapshot written before the disposition fields existed — or hand
        // edited — omits the key entirely. Every network finding is inventory.
        let json = r#"{
            "connections": [
                {"path": "/etc/NetworkManager/system-connections/eth0.nmconnection",
                 "name": "eth0"}
            ],
            "firewall_zones": [
                {"path": "/etc/firewalld/zones/public.xml", "name": "public"}
            ],
            "firewall_direct_rules": [
                {"ipv": "ipv4", "table": "filter", "chain": "INPUT"}
            ]
        }"#;
        let section: NetworkSection = serde_json::from_str(json).unwrap();

        let cases: [(&str, &FindingKind); 3] = [
            ("NMConnection", &section.connections[0].disposition),
            ("FirewallZone", &section.firewall_zones[0].disposition),
            (
                "FirewallDirectRule",
                &section.firewall_direct_rules[0].disposition,
            ),
        ];
        for (name, disposition) in cases {
            assert_eq!(
                *disposition,
                FindingKind::inventory(),
                "{name}: absent disposition must deserialize as inventory"
            );
            assert!(
                !disposition.is_included(),
                "{name}: absent disposition must not render into the Containerfile"
            );
        }
    }
}
