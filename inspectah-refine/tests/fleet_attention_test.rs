use inspectah_core::types::fleet::PrevalenceZone;
use inspectah_refine::types::{AttentionLevel, AttentionScore, FleetAttention};

#[test]
fn divergent_sorts_before_consensus_regardless_of_attention() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::Informational,
        prevalence: 10,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Consensus,
        attention: AttentionLevel::NeedsReview,
        prevalence: 1,
    };
    assert!(a < b);
}

#[test]
fn within_zone_needs_review_before_informational() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 5,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::Informational,
        prevalence: 5,
    };
    assert!(a < b);
}

#[test]
fn within_zone_and_attention_lower_prevalence_first() {
    let a = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 2,
    };
    let b = FleetAttention {
        zone: PrevalenceZone::Divergent,
        attention: AttentionLevel::NeedsReview,
        prevalence: 8,
    };
    assert!(a < b);
}

#[test]
fn sort_vec_of_fleet_attention_produces_correct_order() {
    let items = vec![
        FleetAttention { zone: PrevalenceZone::Consensus, attention: AttentionLevel::Informational, prevalence: 20 },
        FleetAttention { zone: PrevalenceZone::Divergent, attention: AttentionLevel::NeedsReview, prevalence: 1 },
        FleetAttention { zone: PrevalenceZone::NearConsensus, attention: AttentionLevel::NeedsReview, prevalence: 15 },
        FleetAttention { zone: PrevalenceZone::Divergent, attention: AttentionLevel::Informational, prevalence: 3 },
    ];
    let mut sorted = items.clone();
    sorted.sort();
    assert_eq!(sorted[0].prevalence, 1);  // Divergent, NeedsReview, lowest prevalence
    assert_eq!(sorted[1].prevalence, 3);  // Divergent, Informational
    assert_eq!(sorted[2].prevalence, 15); // NearConsensus
    assert_eq!(sorted[3].prevalence, 20); // Consensus
}
