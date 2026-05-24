use crate::types::{AttentionLevel, AttentionScore, FleetAttention, FleetContext, ItemId};

/// Compute a fleet-aware attention score for an item.
///
/// Composes the item's `PrevalenceZone` (from `FleetContext.zones`) with the
/// existing single-host `AttentionLevel` and the raw prevalence count to
/// produce a `FleetAttention` score.
///
/// Items not found in the zone map get `zone: None` (unclassified),
/// which sorts after all classified zones in the FleetAttention Ord.
pub fn score_fleet_attention(
    ctx: &FleetContext,
    item_id: &ItemId,
    attention: AttentionLevel,
    prevalence: u32,
) -> AttentionScore {
    let zone = ctx.zones.get(item_id).copied();

    AttentionScore::Fleet(FleetAttention {
        zone,
        attention,
        prevalence,
    })
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
            name_arch: "vim.x86_64".into(),
        };
        let zones = HashMap::from([(item.clone(), PrevalenceZone::Divergent)]);
        let ctx = test_ctx(zones);

        let score = score_fleet_attention(&ctx, &item, AttentionLevel::NeedsReview, 2);
        match score {
            AttentionScore::Fleet(fa) => {
                assert_eq!(fa.zone, Some(PrevalenceZone::Divergent));
                assert_eq!(fa.attention, AttentionLevel::NeedsReview);
                assert_eq!(fa.prevalence, 2);
            }
            _ => panic!("expected Fleet score"),
        }
    }

    #[test]
    fn unknown_item_is_unclassified() {
        let item = ItemId::Package {
            name_arch: "unknown.x86_64".into(),
        };
        let ctx = test_ctx(HashMap::new());

        let score = score_fleet_attention(&ctx, &item, AttentionLevel::Informational, 5);
        match score {
            AttentionScore::Fleet(fa) => {
                assert_eq!(fa.zone, None, "missing zone should be None (unclassified)");
            }
            _ => panic!("expected Fleet score"),
        }
    }
}
