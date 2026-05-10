use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

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
}
