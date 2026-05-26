use crate::types::{
    FleetBucket, FleetContext, FleetTriage, ItemId, Prevalence, Triage, TriageBucket, TriageReason,
    TriageTag,
};
use inspectah_core::types::fleet::PrevalenceZone;

/// Compute a fleet-aware triage classification for an item.
///
/// Maps the item's `PrevalenceZone` (from `FleetContext.zones`) to a
/// `FleetBucket` and wraps it in a `TriageTag` with `Triage::Fleet`.
///
/// Items not found in the zone map get `FleetBucket::Universal` as a
/// conservative default.
pub fn classify_fleet_bucket(
    ctx: &FleetContext,
    item_id: &ItemId,
    single_host_bucket: TriageBucket,
    single_host_reason: TriageReason,
    prevalence_count: u32,
    prevalence_total: u32,
) -> TriageTag {
    let zone = ctx.zones.get(item_id).copied();

    let fleet_bucket = match zone {
        Some(PrevalenceZone::Divergent) => FleetBucket::Investigate,
        Some(PrevalenceZone::NearConsensus) => FleetBucket::Partial,
        Some(PrevalenceZone::Consensus) => FleetBucket::Universal,
        None => {
            // No zone info — fall back to single-host bucket mapping
            match single_host_bucket {
                TriageBucket::Investigate => FleetBucket::Investigate,
                TriageBucket::Site => FleetBucket::Divergent,
                TriageBucket::Baseline => FleetBucket::Universal,
            }
        }
    };

    TriageTag {
        triage: Triage::Fleet(FleetTriage {
            bucket: fleet_bucket,
            prevalence: Prevalence {
                count: prevalence_count,
                total: prevalence_total,
            },
        }),
        primary_reason: single_host_reason,
        annotations: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FleetContext;
    use inspectah_core::types::fleet::{FleetSnapshotMeta, PrevalenceZone};
    use std::collections::{BTreeMap, HashMap};

    fn test_ctx(zones: HashMap<ItemId, PrevalenceZone>) -> FleetContext {
        FleetContext {
            fleet_meta: FleetSnapshotMeta {
                label: "test".into(),
                host_count: 5,
                hostnames: vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
                merged_at: "2026-05-21T00:00:00Z".into(),
                baseline_provisional: false,
                section_host_counts: BTreeMap::new(),
            },
            zones,
            total_hosts: 5,
            zones_active: true,
            repo_conflicts: HashMap::new(),
        }
    }

    #[test]
    fn known_item_uses_zone_from_context() {
        let item = ItemId::Package {
            name: "vim".into(),
            arch: "x86_64".into(),
        };
        let zones = HashMap::from([(item.clone(), PrevalenceZone::Divergent)]);
        let ctx = test_ctx(zones);

        let tag = classify_fleet_bucket(
            &ctx,
            &item,
            TriageBucket::Investigate,
            TriageReason::PackageNoRepoSource,
            2,
            5,
        );
        match &tag.triage {
            Triage::Fleet(ft) => {
                assert_eq!(ft.bucket, FleetBucket::Investigate);
                assert_eq!(ft.prevalence.count, 2);
                assert_eq!(ft.prevalence.total, 5);
            }
            _ => panic!("expected Fleet triage"),
        }
    }

    #[test]
    fn unknown_item_falls_back_to_single_host_bucket() {
        let item = ItemId::Package {
            name: "unknown".into(),
            arch: "x86_64".into(),
        };
        let ctx = test_ctx(HashMap::new());

        let tag = classify_fleet_bucket(
            &ctx,
            &item,
            TriageBucket::Investigate,
            TriageReason::PackageProvenanceUnavailable,
            5,
            5,
        );
        match &tag.triage {
            Triage::Fleet(ft) => {
                assert_eq!(ft.bucket, FleetBucket::Investigate);
            }
            _ => panic!("expected Fleet triage"),
        }
    }

    #[test]
    fn consensus_zone_maps_to_universal() {
        let item = ItemId::Package {
            name: "glibc".into(),
            arch: "x86_64".into(),
        };
        let zones = HashMap::from([(item.clone(), PrevalenceZone::Consensus)]);
        let ctx = test_ctx(zones);

        let tag = classify_fleet_bucket(
            &ctx,
            &item,
            TriageBucket::Baseline,
            TriageReason::PackageBaselineMatch,
            5,
            5,
        );
        match &tag.triage {
            Triage::Fleet(ft) => {
                assert_eq!(ft.bucket, FleetBucket::Universal);
            }
            _ => panic!("expected Fleet triage"),
        }
    }
}
