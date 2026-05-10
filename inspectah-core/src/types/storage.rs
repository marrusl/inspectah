use serde::{Deserialize, Serialize};
use super::fleet::FleetPrevalence;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FstabEntry {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub device: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub mount_point: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub fstype: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub options: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<bool>,
    #[serde(default, skip_serializing_if = "crate::is_false")]
    pub acknowledged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fleet: Option<FleetPrevalence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub mount_point: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub credential_path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountPoint {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub target: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub source: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub fstype: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub options: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LvmVolume {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub lv_name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub vg_name: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub lv_size: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDirectory {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub path: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub size_estimate: String,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_string")]
    pub recommendation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageSection {
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub fstab_entries: Vec<FstabEntry>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub mount_points: Vec<MountPoint>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub lvm_info: Vec<LvmVolume>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
    pub var_directories: Vec<VarDirectory>,
    #[serde(default, deserialize_with = "crate::deserialize_null_as_empty_vec")]
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
