use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerMount {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", rename = "type")]
    pub mount_type: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub destination: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub mode: String,
    #[serde(default)]
    pub rw: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuadletUnit {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub content: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub image: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec", skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec", skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub generated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeService {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub service: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub image: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComposeFile {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub images: Vec<ComposeService>,
    #[serde(default)]
    pub include: bool,
    #[serde(default)]
    pub tie: bool,
    #[serde(default)]
    pub tie_winner: bool,
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunningContainer {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub id: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub image: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub image_id: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub status: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", skip_serializing_if = "String::is_empty")]
    pub restart_policy: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub mounts: Vec<ContainerMount>,
    #[serde(default)]
    pub networks: serde_json::Value,
    #[serde(default)]
    pub ports: serde_json::Value,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub inspect_data: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakApp {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub app_id: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub origin: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub branch: String,
    #[serde(default)]
    pub include: bool,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", skip_serializing_if = "String::is_empty")]
    pub remote: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string", skip_serializing_if = "String::is_empty")]
    pub remote_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub quadlet_units: Vec<QuadletUnit>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub compose_files: Vec<ComposeFile>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub running_containers: Vec<RunningContainer>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub flatpak_apps: Vec<FlatpakApp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_section_roundtrip() {
        let section = ContainerSection {
            quadlet_units: vec![QuadletUnit {
                name: "myapp.container".into(),
                image: "quay.io/myorg/myapp:latest".into(),
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ContainerSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
