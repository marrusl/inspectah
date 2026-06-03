use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

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
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub fleet: Option<FleetPrevalence>,
}

/// A file that the security inspector carries forward for materialization
/// in the config tree (audit rules, PAM configs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CarryForwardFile {
    /// Relative path (e.g. "etc/audit/rules.d/custom.rules").
    #[serde(default)]
    pub path: String,
    /// File content to materialize.
    #[serde(default)]
    pub content: String,
}

/// Security & access control section (SELinux, FIPS, PAM, audit rules).
///
/// Display name: "Security & Access Control". The JSON key is `"selinux"`
/// because that is what the collectors emit.
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
    pub audit_rules: Vec<CarryForwardFile>,
    #[serde(default)]
    pub fips_mode: bool,
    #[serde(default)]
    pub pam_configs: Vec<CarryForwardFile>,
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
                serde_json::json!({"name": "httpd_can_network_connect", "state": true}),
            ],
            fcontext_rules: vec!["/opt/app(/.*)?".to_string()],
            audit_rules: vec![CarryForwardFile {
                path: "etc/audit/rules.d/custom.rules".to_string(),
                content: "-w /etc/shadow -p wa".to_string(),
            }],
            fips_mode: false,
            pam_configs: vec![CarryForwardFile {
                path: "etc/pam.d/custom-sshd".to_string(),
                content: "auth required pam_unix.so".to_string(),
            }],
            port_labels: vec![SelinuxPortLabel {
                protocol: "tcp".to_string(),
                port: "8080".to_string(),
                label_type: "http_port_t".to_string(),
                include: true,
                locked: false,
                fleet: None,
            }],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: SelinuxSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
