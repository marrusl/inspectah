use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NMConnection {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub method: String,
    #[serde(default, rename = "type")]
    pub conn_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
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
    #[serde(default)]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub fleet: Option<FleetPrevalence>,
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
    #[serde(default)]
    pub include: bool,
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
}
