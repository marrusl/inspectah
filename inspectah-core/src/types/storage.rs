use super::fleet::FleetPrevalence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FstabEntry {
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    #[serde(default)]
    pub mount_point: String,
    #[serde(default)]
    pub credential_path: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountPoint {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub fstype: String,
    #[serde(default)]
    pub options: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvmVolume {
    #[serde(default)]
    pub lv_name: String,
    #[serde(default)]
    pub vg_name: String,
    #[serde(default)]
    pub lv_size: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDirectory {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size_estimate: String,
    #[serde(default)]
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageSection {
    #[serde(default)]
    pub fstab_entries: Vec<FstabEntry>,
    #[serde(default)]
    pub mount_points: Vec<MountPoint>,
    #[serde(default)]
    pub lvm_info: Vec<LvmVolume>,
    #[serde(default)]
    pub var_directories: Vec<VarDirectory>,
    #[serde(default)]
    pub credential_refs: Vec<CredentialRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_section_roundtrip() {
        let section = StorageSection {
            fstab_entries: vec![FstabEntry {
                device: "/dev/sda1".into(),
                mount_point: "/boot".into(),
                fstype: "xfs".into(),
                options: "defaults".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&section).unwrap();
        let parsed: StorageSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, parsed);
    }
}
