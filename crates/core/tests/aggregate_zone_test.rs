use inspectah_core::aggregate::classify_zone;
use inspectah_core::types::aggregate::{AggregatePrevalence, PrevalenceZone};

#[test]
fn consensus_when_all_hosts() {
    let fp = AggregatePrevalence {
        count: 5,
        total: 5,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Consensus);
}

#[test]
fn near_consensus_at_exactly_half() {
    let fp = AggregatePrevalence {
        count: 5,
        total: 10,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::NearConsensus);
}

#[test]
fn near_consensus_above_half_odd() {
    // 3/5 = 60%, count*2=6 >= total=5 → NearConsensus
    let fp = AggregatePrevalence {
        count: 3,
        total: 5,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::NearConsensus);
}

#[test]
fn divergent_below_half() {
    // 2/5 = 40%, count*2=4 < total=5 → Divergent
    let fp = AggregatePrevalence {
        count: 2,
        total: 5,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Divergent);
}

#[test]
fn divergent_single_host_of_twenty() {
    let fp = AggregatePrevalence {
        count: 1,
        total: 20,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Divergent);
}

#[test]
fn consensus_when_count_equals_total_min_case() {
    let fp = AggregatePrevalence {
        count: 1,
        total: 1,
        hosts: vec![],
        ..Default::default()
    };
    assert_eq!(classify_zone(&fp), PrevalenceZone::Consensus);
}

#[test]
fn ord_divergent_less_than_near_consensus_less_than_consensus() {
    assert!(PrevalenceZone::Divergent < PrevalenceZone::NearConsensus);
    assert!(PrevalenceZone::NearConsensus < PrevalenceZone::Consensus);
}

#[test]
fn zone_serde_roundtrip() {
    for zone in [
        PrevalenceZone::Divergent,
        PrevalenceZone::NearConsensus,
        PrevalenceZone::Consensus,
    ] {
        let json = serde_json::to_string(&zone).unwrap();
        let parsed: PrevalenceZone = serde_json::from_str(&json).unwrap();
        assert_eq!(zone, parsed);
    }
}

#[test]
fn zone_is_hashable() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(PrevalenceZone::Consensus);
    set.insert(PrevalenceZone::Consensus);
    assert_eq!(set.len(), 1);
}
