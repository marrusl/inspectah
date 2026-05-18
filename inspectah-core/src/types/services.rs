use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceStateChange {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub default_state: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub include: bool,
    pub owning_package: Option<String>,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdDropIn {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceSection {
    #[serde(default)]
    pub state_changes: Vec<ServiceStateChange>,
    #[serde(default)]
    pub enabled_units: Vec<String>,
    #[serde(default)]
    pub disabled_units: Vec<String>,
    #[serde(default)]
    pub drop_ins: Vec<SystemdDropIn>,
    #[serde(default)]
    pub preset_matched_units: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_section_roundtrip() {
        let section = ServiceSection {
            state_changes: vec![ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: "enabled".into(),
                default_state: "disabled".into(),
                action: "enable".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ServiceSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn test_preset_matched_units_roundtrip() {
        let section = ServiceSection {
            state_changes: Vec::new(),
            enabled_units: vec!["sshd.service".into()],
            disabled_units: Vec::new(),
            drop_ins: Vec::new(),
            preset_matched_units: vec!["chronyd.service".into(), "firewalld.service".into()],
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ServiceSection = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.preset_matched_units,
            vec!["chronyd.service", "firewalld.service"]
        );
    }

    #[test]
    fn test_preset_matched_units_missing_deserializes_empty() {
        let json = r#"{"state_changes":[],"enabled_units":[],"disabled_units":[],"drop_ins":[]}"#;
        let parsed: ServiceSection = serde_json::from_str(json).unwrap();
        assert!(parsed.preset_matched_units.is_empty());
    }
}
