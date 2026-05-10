use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxPortLabel {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub port: String,
    #[serde(default, rename = "type")]
    pub label_type: String,
    #[serde(default)]
    pub include: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SelinuxSection {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub custom_modules: Vec<String>,
    #[serde(default)]
    pub boolean_overrides: Vec<serde_json::Value>,
    #[serde(default)]
    pub fcontext_rules: Vec<String>,
    #[serde(default)]
    pub audit_rules: Vec<String>,
    #[serde(default)]
    pub fips_mode: bool,
    #[serde(default)]
    pub pam_configs: Vec<String>,
    #[serde(default)]
    pub port_labels: Vec<SelinuxPortLabel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selinux_section_roundtrip() {
        let section = SelinuxSection {
            mode: "enforcing".to_string(),
            custom_modules: vec!["myapp".to_string()],
            boolean_overrides: vec![
                serde_json::json!({"name": "httpd_can_network_connect", "state": true})
            ],
            fcontext_rules: vec!["/opt/app(/.*)?".to_string()],
            audit_rules: vec![],
            fips_mode: false,
            pam_configs: vec![],
            port_labels: vec![SelinuxPortLabel {
                protocol: "tcp".to_string(),
                port: "8080".to_string(),
                label_type: "http_port_t".to_string(),
                include: true,
                fleet: None,
            }],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: SelinuxSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
