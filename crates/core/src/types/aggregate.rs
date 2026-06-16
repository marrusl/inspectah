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
pub struct AggregatePrevalence {
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Aggregate host count across all content variants of the same item.
    /// Only set for items with content variants (configs, drop-ins, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_count: Option<i32>,
    /// Aggregate host list across all content variants of the same item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate_hosts: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateMeta {
    #[serde(default)]
    pub source_hosts: Vec<String>,
    #[serde(default)]
    pub total_hosts: i32,
    #[serde(default)]
    pub min_prevalence: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggregateSnapshotMeta {
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

/// Tracks which repo a package was sourced from and how many hosts used it.
/// Used to detect repo-source conflicts in aggregate merge (e.g., nginx from epel
/// on 2 hosts vs appstream on 1 host).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSourceEntry {
    pub repo: String,
    pub host_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_aggregate_prevalence_roundtrip() {
        let fp = AggregatePrevalence {
            count: 3,
            total: 5,
            hosts: vec!["host1".into(), "host2".into(), "host3".into()],
            ..Default::default()
        };
        let json = serde_json::to_string(&fp).unwrap();
        let parsed: AggregatePrevalence = serde_json::from_str(&json).unwrap();
        assert_eq!(fp, parsed);
    }

    #[test]
    fn test_aggregate_prevalence_null_deserialize() {
        let val: Option<AggregatePrevalence> = serde_json::from_str("null").unwrap();
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
    fn test_aggregate_snapshot_meta_roundtrip() {
        let meta = AggregateSnapshotMeta {
            label: "web-servers".into(),
            host_count: 50,
            hostnames: vec!["host-a".into(), "host-b".into()],
            merged_at: "2026-05-20T12:00:00Z".into(),
            baseline_provisional: true,
            section_host_counts: BTreeMap::from([("config".into(), 48usize), ("rpm".into(), 50)]),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: AggregateSnapshotMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, parsed);
    }
}
