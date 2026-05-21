use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::fleet::FleetSnapshotMeta;
use inspectah_refine::session::RefineSession;
use std::collections::BTreeMap;

fn make_fleet_snapshot(host_count: usize) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-20T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap
}

#[test]
fn single_host_snapshot_has_no_fleet_context() {
    let session = RefineSession::new(InspectionSnapshot::default());
    assert!(session.fleet_context().is_none());
}

#[test]
fn fleet_of_five_has_fleet_context() {
    let session = RefineSession::new(make_fleet_snapshot(5));
    let ctx = session.fleet_context().unwrap();
    assert_eq!(ctx.total_hosts, 5);
}

#[test]
fn fleet_of_two_has_fleet_context_zones_suppressed() {
    let session = RefineSession::new(make_fleet_snapshot(2));
    let ctx = session.fleet_context().unwrap();
    assert_eq!(ctx.total_hosts, 2);
    assert!(!ctx.zones_active, "fleet-of-2 suppresses zones");
}

#[test]
fn fleet_of_three_has_zones_active() {
    let session = RefineSession::new(make_fleet_snapshot(3));
    let ctx = session.fleet_context().unwrap();
    assert!(ctx.zones_active, "fleet-of-3+ activates zones");
}
