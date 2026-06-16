use crate::types::{
    AggregateBucket, AggregateContext, AggregateTriage, ItemId, Prevalence, Triage, TriageBucket,
    TriageReason, TriageTag,
};
use inspectah_core::types::aggregate::PrevalenceZone;

/// Compute an aggregate-aware triage classification for an item.
///
/// Maps the item's `PrevalenceZone` (from `AggregateContext.zones`) to an
/// `AggregateBucket` and wraps it in a `TriageTag` with `Triage::Aggregate`.
///
/// Items not found in the zone map get `AggregateBucket::Universal` as a
/// conservative default.
pub fn classify_aggregate_bucket(
    ctx: &AggregateContext,
    item_id: &ItemId,
    single_host_bucket: TriageBucket,
    single_host_reason: TriageReason,
    prevalence_count: u32,
    prevalence_total: u32,
) -> TriageTag {
    let zone = ctx.zones.get(item_id).copied();

    let aggregate_bucket = match zone {
        Some(PrevalenceZone::Divergent) if prevalence_count >= prevalence_total => {
            AggregateBucket::Investigate
        }
        Some(PrevalenceZone::Divergent) => AggregateBucket::Divergent,
        Some(PrevalenceZone::NearConsensus) => AggregateBucket::Partial,
        Some(PrevalenceZone::Consensus) => AggregateBucket::Universal,
        None => {
            // No zone info — fall back to single-host bucket mapping
            match single_host_bucket {
                TriageBucket::Investigate => AggregateBucket::Investigate,
                TriageBucket::Site => AggregateBucket::Divergent,
                TriageBucket::Baseline => AggregateBucket::Universal,
            }
        }
    };

    TriageTag {
        triage: Triage::Aggregate(AggregateTriage {
            bucket: aggregate_bucket,
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
    use crate::types::AggregateContext;
    use inspectah_core::types::aggregate::{AggregateSnapshotMeta, PrevalenceZone};
    use std::collections::{BTreeMap, HashMap};

    fn test_ctx(zones: HashMap<ItemId, PrevalenceZone>) -> AggregateContext {
        AggregateContext {
            aggregate_meta: AggregateSnapshotMeta {
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
    fn non_universal_divergent_demoted_to_divergent_bucket() {
        let item = ItemId::Package {
            name: "vim".into(),
            arch: "x86_64".into(),
        };
        let zones = HashMap::from([(item.clone(), PrevalenceZone::Divergent)]);
        let ctx = test_ctx(zones);

        let tag = classify_aggregate_bucket(
            &ctx,
            &item,
            TriageBucket::Investigate,
            TriageReason::PackageNoRepoSource,
            2,
            5,
        );
        match &tag.triage {
            Triage::Aggregate(ft) => {
                assert_eq!(ft.bucket, AggregateBucket::Divergent);
                assert_eq!(ft.prevalence.count, 2);
                assert_eq!(ft.prevalence.total, 5);
            }
            _ => panic!("expected Aggregate triage"),
        }
    }

    #[test]
    fn universal_divergent_stays_investigate() {
        let item = ItemId::Package {
            name: "resolv-conf".into(),
            arch: "x86_64".into(),
        };
        let zones = HashMap::from([(item.clone(), PrevalenceZone::Divergent)]);
        let ctx = test_ctx(zones);

        let tag = classify_aggregate_bucket(
            &ctx,
            &item,
            TriageBucket::Investigate,
            TriageReason::PackageNoRepoSource,
            5,
            5,
        );
        match &tag.triage {
            Triage::Aggregate(ft) => {
                assert_eq!(ft.bucket, AggregateBucket::Investigate);
            }
            _ => panic!("expected Aggregate triage"),
        }
    }

    #[test]
    fn unknown_item_falls_back_to_single_host_bucket() {
        let item = ItemId::Package {
            name: "unknown".into(),
            arch: "x86_64".into(),
        };
        let ctx = test_ctx(HashMap::new());

        let tag = classify_aggregate_bucket(
            &ctx,
            &item,
            TriageBucket::Investigate,
            TriageReason::PackageProvenanceUnavailable,
            5,
            5,
        );
        match &tag.triage {
            Triage::Aggregate(ft) => {
                assert_eq!(ft.bucket, AggregateBucket::Investigate);
            }
            _ => panic!("expected Aggregate triage"),
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

        let tag = classify_aggregate_bucket(
            &ctx,
            &item,
            TriageBucket::Baseline,
            TriageReason::PackageBaselineMatch,
            5,
            5,
        );
        match &tag.triage {
            Triage::Aggregate(ft) => {
                assert_eq!(ft.bucket, AggregateBucket::Universal);
            }
            _ => panic!("expected Aggregate triage"),
        }
    }
}
