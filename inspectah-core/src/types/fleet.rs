use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariantSelection {
    #[default]
    Only,
    Selected,
    Alternative,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetPrevalence {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetMeta {
    #[serde(default)]
    pub source_hosts: Vec<String>,
    #[serde(default)]
    pub total_hosts: i32,
    #[serde(default)]
    pub min_prevalence: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetSnapshotMeta {
    pub label: String,
    pub host_count: usize,
    pub hostnames: Vec<String>,
    pub merged_at: String,
    #[serde(default)]
    pub baseline_provisional: bool,
    #[serde(default)]
    pub section_host_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PrevalenceZone {
    Divergent,
    NearConsensus,
    Consensus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_fleet_prevalence_roundtrip() {
        let fp = FleetPrevalence {
            count: 3,
            total: 5,
            hosts: vec!["host1".into(), "host2".into(), "host3".into()],
        };
        let json = serde_json::to_string(&fp).unwrap();
        let parsed: FleetPrevalence = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, parsed);
    }

    #[test]
    fn test_fleet_prevalence_null_deserialize() {
        let val: Option<FleetPrevalence> = serde_json::from_str("null").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn test_variant_selection_default() {
        let vs = VariantSelection::default();
        assert_eq!(vs, VariantSelection::Only);
    }

    #[test]
    fn test_variant_selection_serde_roundtrip() {
        for variant in [
            VariantSelection::Only,
            VariantSelection::Selected,
            VariantSelection::Alternative,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: VariantSelection = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn test_fleet_snapshot_meta_roundtrip() {
        let meta = FleetSnapshotMeta {
            label: "web-servers".into(),
            host_count: 50,
            hostnames: vec!["host-a".into(), "host-b".into()],
            merged_at: "2026-05-20T12:00:00Z".into(),
            baseline_provisional: true,
            section_host_counts: BTreeMap::from([("config".into(), 48usize), ("rpm".into(), 50)]),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: FleetSnapshotMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, parsed);
    }
}
