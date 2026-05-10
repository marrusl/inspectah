use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NMConnection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub method: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", rename = "type")]
    pub conn_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallZone {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub content: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub services: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ports: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub rich_rules: Vec<String>,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirewallDirectRule {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub ipv: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub table: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub chain: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub priority: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub args: String,
    #[serde(default)]
    pub include: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticRouteFile {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub connections: Vec<NMConnection>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub firewall_zones: Vec<FirewallZone>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub firewall_direct_rules: Vec<FirewallDirectRule>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub static_routes: Vec<StaticRouteFile>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ip_routes: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub ip_rules: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub resolv_provenance: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub hosts_additions: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
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
