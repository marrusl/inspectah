use super::aggregate::{AggregatePrevalence, VariantSelection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerMount {
    #[serde(default, rename = "type")]
    pub mount_type: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub rw: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuadletUnit {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub variant_selection: VariantSelection,
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub generated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeService {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub image: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComposeFile {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub images: Vec<ComposeService>,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default)]
    pub variant_selection: VariantSelection,
    pub aggregate: Option<AggregatePrevalence>,
    /// Raw compose YAML content, retained for verbatim export.
    /// Subject to redaction rules when snapshot is in redacted state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningContainer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub image_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub restart_policy: String,
    #[serde(default)]
    pub mounts: Vec<ContainerMount>,
    #[serde(default)]
    pub networks: serde_json::Value,
    #[serde(default)]
    pub ports: serde_json::Value,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub inspect_data: bool,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
}

impl Default for RunningContainer {
    fn default() -> Self {
        Self {
            include: true,
            id: Default::default(),
            name: Default::default(),
            image: Default::default(),
            image_id: Default::default(),
            status: Default::default(),
            restart_policy: Default::default(),
            mounts: Default::default(),
            networks: Default::default(),
            ports: Default::default(),
            env: Default::default(),
            inspect_data: Default::default(),
            locked: Default::default(),
            acknowledged: Default::default(),
            aggregate: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakApp {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default = "crate::default_true")]
    pub include: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregatePrevalence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContainerSection {
    #[serde(default)]
    pub quadlet_units: Vec<QuadletUnit>,
    #[serde(default)]
    pub compose_files: Vec<ComposeFile>,
    #[serde(default)]
    pub running_containers: Vec<RunningContainer>,
    #[serde(default)]
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
            compose_files: vec![ComposeFile {
                path: "opt/myapp/docker-compose.yml".to_string(),
                images: vec![],
                include: true,
                raw_content: Some(
                    "version: '3'\nservices:\n  web:\n    image: nginx\n".to_string(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: ContainerSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }

    #[test]
    fn compose_raw_content_roundtrip() {
        let cf = ComposeFile {
            path: "opt/app/docker-compose.yml".to_string(),
            raw_content: Some("services:\n  db:\n    image: postgres:16\n".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&cf).unwrap();
        assert!(json.contains("raw_content"));
        let parsed: ComposeFile = serde_json::from_str(&json).unwrap();
        assert_eq!(cf.raw_content, parsed.raw_content);
    }

    #[test]
    fn compose_raw_content_none_omitted() {
        let cf = ComposeFile {
            path: "opt/app/compose.yml".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&cf).unwrap();
        assert!(!json.contains("raw_content"));
    }

    #[test]
    fn quadlet_without_include_deserializes_as_true() {
        let json = r#"{"path":"/etc/containers/systemd/myapp.container","name":"myapp.container","content":"","image":"quay.io/myorg/myapp:latest"}"#;
        let q: QuadletUnit = serde_json::from_str(json).unwrap();
        assert!(
            q.include,
            "missing include field should deserialize as true"
        );
    }
}
