use super::fleet::{FleetPrevalence, VariantSelection};
use serde::{Deserialize, Serialize};

/// Durable systemd unit states that represent administrator intent.
/// Only these three states produce migration-worthy `ServiceStateChange` entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceUnitState {
    Enabled,
    Disabled,
    Masked,
}

/// Preset default from systemd `.preset` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetDefault {
    Enable,
    Disable,
}

/// Derived action for the Containerfile renderer.
/// Not serialized — computed from `current_state` via `implied_action()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Enable,
    Disable,
    Mask,
}

impl std::fmt::Display for ServiceUnitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ServiceUnitState::Enabled => "enabled",
            ServiceUnitState::Disabled => "disabled",
            ServiceUnitState::Masked => "masked",
        };
        write!(f, "{}", s)
    }
}

impl std::fmt::Display for PresetDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PresetDefault::Enable => "enable",
            PresetDefault::Disable => "disable",
        };
        write!(f, "{}", s)
    }
}

impl std::fmt::Display for ServiceAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ServiceAction::Enable => "enable",
            ServiceAction::Disable => "disable",
            ServiceAction::Mask => "mask",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceStateChange {
    pub unit: String,
    pub current_state: ServiceUnitState,
    #[serde(deserialize_with = "require_explicit_null")]
    pub default_state: Option<PresetDefault>,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    pub owning_package: Option<String>,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

fn require_explicit_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl ServiceStateChange {
    /// Derive the Containerfile action from the current unit state.
    pub fn implied_action(&self) -> ServiceAction {
        match self.current_state {
            ServiceUnitState::Enabled => ServiceAction::Enable,
            ServiceUnitState::Disabled => ServiceAction::Disable,
            ServiceUnitState::Masked => ServiceAction::Mask,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemdDropIn {
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub variant_selection: VariantSelection,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceSection {
    pub state_changes: Vec<ServiceStateChange>,
    pub enabled_units: Vec<String>,
    pub disabled_units: Vec<String>,
    pub drop_ins: Vec<SystemdDropIn>,
    pub preset_matched_units: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_state_change_roundtrip_uses_typed_enums() {
        let section = ServiceSection {
            state_changes: vec![
                ServiceStateChange {
                    unit: "firewalld.service".into(),
                    current_state: ServiceUnitState::Enabled,
                    default_state: Some(PresetDefault::Disable),
                    include: true,
                    locked: false,
                    owning_package: Some("firewalld".into()),
                    fleet: None,
                    attention_reason: None,
                },
                ServiceStateChange {
                    unit: "cups.service".into(),
                    current_state: ServiceUnitState::Masked,
                    default_state: None,
                    include: true,
                    locked: false,
                    owning_package: Some("cups".into()),
                    fleet: None,
                    attention_reason: None,
                },
            ],
            enabled_units: vec!["firewalld.service".into()],
            disabled_units: vec![],
            drop_ins: vec![],
            preset_matched_units: vec![],
        };

        let json = serde_json::to_string(&section).unwrap();
        let parsed: ServiceSection = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, section);
    }

    #[test]
    fn test_implied_action_derives_from_current_state() {
        let enabled = ServiceStateChange {
            unit: "firewalld.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            locked: false,
            owning_package: Some("firewalld".into()),
            fleet: None,
            attention_reason: None,
        };
        let disabled = ServiceStateChange {
            unit: "sshd.service".into(),
            current_state: ServiceUnitState::Disabled,
            default_state: Some(PresetDefault::Enable),
            include: true,
            locked: false,
            owning_package: Some("openssh-server".into()),
            fleet: None,
            attention_reason: None,
        };
        let masked = ServiceStateChange {
            unit: "cups.service".into(),
            current_state: ServiceUnitState::Masked,
            default_state: None,
            include: true,
            locked: false,
            owning_package: Some("cups".into()),
            fleet: None,
            attention_reason: None,
        };

        assert_eq!(enabled.implied_action(), ServiceAction::Enable);
        assert_eq!(disabled.implied_action(), ServiceAction::Disable);
        assert_eq!(masked.implied_action(), ServiceAction::Mask);
    }

    #[test]
    fn test_option_preset_default_serde_roundtrip() {
        let with_preset = ServiceStateChange {
            unit: "firewalld.service".into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include: true,
            locked: false,
            owning_package: Some("firewalld".into()),
            fleet: None,
            attention_reason: None,
        };
        let json = serde_json::to_string(&with_preset).unwrap();
        let parsed: ServiceStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, Some(PresetDefault::Disable));

        let without_preset = ServiceStateChange {
            unit: "cups.service".into(),
            current_state: ServiceUnitState::Masked,
            default_state: None,
            include: true,
            locked: false,
            owning_package: Some("cups".into()),
            fleet: None,
            attention_reason: None,
        };
        let json = serde_json::to_string(&without_preset).unwrap();
        let parsed: ServiceStateChange = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.default_state, None);

        let null_json = r#"{
            "unit":"cups.service",
            "current_state":"masked",
            "default_state":null,
            "include":true,
            "owning_package":"cups",
            "fleet":null
        }"#;
        let parsed: ServiceStateChange = serde_json::from_str(null_json).unwrap();
        assert_eq!(parsed.default_state, None);
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
    fn test_missing_field_fails_deserialization() {
        // Missing preset_matched_units must fail, not silently backfill.
        let json = r#"{"state_changes":[],"enabled_units":[],"disabled_units":[],"drop_ins":[]}"#;
        let err = serde_json::from_str::<ServiceSection>(json).unwrap_err();
        assert!(
            err.to_string().contains("preset_matched_units"),
            "expected missing-field error for preset_matched_units, got: {err}"
        );
    }

    #[test]
    fn test_missing_default_state_does_not_deserialize() {
        let json = r#"{
            "unit":"firewalld.service",
            "current_state":"enabled",
            "include":true,
            "owning_package":"firewalld",
            "fleet":null
        }"#;

        let err = serde_json::from_str::<ServiceStateChange>(json).unwrap_err();
        assert!(
            err.to_string().contains("default_state"),
            "expected missing-field error, got: {err}"
        );
    }

    // -- Serde backward-compat contract tests ---------------------------------
    // Verify that JSON without `include` deserializes with include=true.

    #[test]
    fn service_without_include_deserializes_as_true() {
        let json = r#"{"unit":"test.service","current_state":"enabled","default_state":null}"#;
        let sc: ServiceStateChange = serde_json::from_str(json).unwrap();
        assert!(
            sc.include,
            "missing include field should deserialize as true"
        );
    }

    #[test]
    fn dropin_without_include_deserializes_as_true() {
        let json = r#"{"unit":"test.service","path":"/etc/systemd/system/test.service.d/override.conf","content":"[Service]\nRestart=always"}"#;
        let di: SystemdDropIn = serde_json::from_str(json).unwrap();
        assert!(
            di.include,
            "missing include field should deserialize as true"
        );
    }
}
