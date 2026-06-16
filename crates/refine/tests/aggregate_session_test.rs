use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerSection, QuadletUnit,
};
use inspectah_core::types::aggregate::{
    AggregatePrevalence, AggregateSnapshotMeta, PrevalenceZone, VariantSelection,
};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};
use std::collections::BTreeMap;

fn make_aggregate_snapshot(host_count: usize) -> InspectionSnapshot {
    InspectionSnapshot {
        aggregate_meta: Some(AggregateSnapshotMeta {
            label: "test".into(),
            host_count,
            hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
            merged_at: "2026-05-20T00:00:00Z".into(),
            baseline_provisional: false,
            section_host_counts: BTreeMap::new(),
        }),
        ..Default::default()
    }
}

#[test]
fn single_host_snapshot_has_no_aggregate_context() {
    let session = RefineSession::new(InspectionSnapshot::default());
    assert!(session.aggregate_context().is_none());
}

#[test]
fn aggregate_of_five_has_aggregate_context() {
    let session = RefineSession::new(make_aggregate_snapshot(5));
    let ctx = session.aggregate_context().unwrap();
    assert_eq!(ctx.total_hosts, 5);
}

#[test]
fn aggregate_of_two_has_aggregate_context_zones_suppressed() {
    let session = RefineSession::new(make_aggregate_snapshot(2));
    let ctx = session.aggregate_context().unwrap();
    assert_eq!(ctx.total_hosts, 2);
    assert!(!ctx.zones_active, "aggregate-of-2 suppresses zones");
}

#[test]
fn aggregate_of_three_has_zones_active() {
    let session = RefineSession::new(make_aggregate_snapshot(3));
    let ctx = session.aggregate_context().unwrap();
    assert!(ctx.zones_active, "aggregate-of-3+ activates zones");
}

// ---------------------------------------------------------------------------
// R1 fixup tests: multi-variant zone, drop-in/quadlet zones, aggregate_attention
// ---------------------------------------------------------------------------

fn aggregate_prevalence(count: i32, total: i32) -> Option<AggregatePrevalence> {
    Some(AggregatePrevalence {
        count,
        total,
        hosts: (0..count).map(|i| format!("host-{i}")).collect(),
        ..Default::default()
    })
}

#[test]
fn multi_variant_path_zone_uses_most_divergent_variant() {
    // Three config variants for /etc/app/main.conf: 3/5, 1/5, 1/5.
    // Each variant is classified individually: NearConsensus, Divergent, Divergent.
    // The path-level zone uses the most-divergent (min): Divergent.
    // This surfaces the decision: the user must choose among variants.
    let mut snap = make_aggregate_snapshot(5);
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                locked: false,
                aggregate: aggregate_prevalence(3, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                locked: false,
                aggregate: aggregate_prevalence(1, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                locked: false,
                aggregate: aggregate_prevalence(1, 5),
                ..Default::default()
            },
        ],
    });

    let session = RefineSession::new(snap);
    let ctx = session.aggregate_context().unwrap();
    let item = ItemId::Config {
        path: "/etc/app/main.conf".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::Divergent),
        "multi-variant path must use most-divergent variant zone (1/5 → Divergent)",
    );
}

#[test]
fn zone_for_partial_path_is_divergent() {
    // A path present on only 2 of 5 hosts (one variant, count=2/total=5) → Divergent.
    // Proves non-trivial classification still works with sum approach.
    let mut snap = make_aggregate_snapshot(5);
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/app/rare.conf".into(),
            include: true,
            locked: false,
            aggregate: aggregate_prevalence(2, 5),
            ..Default::default()
        }],
    });

    let session = RefineSession::new(snap);
    let ctx = session.aggregate_context().unwrap();
    let item = ItemId::Config {
        path: "/etc/app/rare.conf".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::Divergent),
        "path on 2/5 hosts must be Divergent (2*2=4 < 5)",
    );
}

#[test]
fn dropin_zone_classified_on_aggregate_init() {
    let mut snap = make_aggregate_snapshot(5);
    snap.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
            content: "test".into(),
            include: true,
            locked: false,
            aggregate: aggregate_prevalence(4, 5),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let ctx = session.aggregate_context().unwrap();
    let item = ItemId::DropIn {
        path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::NearConsensus),
        "drop-in must appear in zone map",
    );
}

#[test]
fn quadlet_zone_classified_on_aggregate_init() {
    let mut snap = make_aggregate_snapshot(5);
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            path: "/etc/containers/systemd/myapp.container".into(),
            name: "myapp".into(),
            include: true,
            locked: false,
            aggregate: aggregate_prevalence(5, 5),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let ctx = session.aggregate_context().unwrap();
    let item = ItemId::Quadlet {
        path: "/etc/containers/systemd/myapp.container".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::Consensus),
        "quadlet must appear in zone map",
    );
}

// aggregate_attention tests removed — field no longer exists on RefinedPackage.
// Aggregate attention is now handled at the handler/DTO layer, not on the
// domain type.

// ---------------------------------------------------------------------------
// R4a: Projection-based dirty state — net-zero variant ops should be clean
// ---------------------------------------------------------------------------

#[test]
fn variants_changed_net_zero_is_clean() {
    // Create a aggregate snapshot with two config variants for the same path
    let mut snap = make_aggregate_snapshot(5);
    let content_a = "ServerRoot /etc/httpd\nMaxClients 256";
    let content_b = "ServerRoot /etc/httpd\nMaxClients 128";
    let hash_a = ContentHash::from_content(content_a.as_bytes());
    let hash_b = ContentHash::from_content(content_b.as_bytes());

    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                content: content_a.into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                aggregate: aggregate_prevalence(3, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                content: content_b.into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                aggregate: aggregate_prevalence(2, 5),
                ..Default::default()
            },
        ],
    });

    let mut session = RefineSession::new(snap);

    // Select variant B (Alternative → Selected)
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            target: hash_b.clone(),
        })
        .unwrap();

    // Now variants_changed should be 1 (we changed from original)
    let changes = session.pending_changes();
    assert!(
        changes.variants_changed > 0,
        "after selecting a different variant, variants_changed must be > 0"
    );
    assert!(
        changes.is_dirty,
        "session must be dirty after variant change"
    );

    // Select back to original (A)
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            target: hash_a.clone(),
        })
        .unwrap();

    // Net zero — back to original state
    let changes = session.pending_changes();
    assert_eq!(
        changes.variants_changed, 0,
        "select A→B then B→A should report variants_changed == 0, got {}",
        changes.variants_changed,
    );
    assert!(
        !changes.is_dirty,
        "session must be clean after net-zero variant round-trip",
    );
}

// ---------------------------------------------------------------------------
// R4 round-4: Compose multi-variant dirty-state regression
// ---------------------------------------------------------------------------

#[test]
fn compose_multi_variant_pristine_is_clean() {
    let mut snap = make_aggregate_snapshot(5);
    snap.containers = Some(ContainerSection {
        compose_files: vec![
            ComposeFile {
                path: "/opt/app/docker-compose.yml".into(),
                images: vec![ComposeService {
                    service: "web".into(),
                    image: "nginx:1.25".into(),
                }],
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                aggregate: Some(AggregatePrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["h1".into(), "h2".into(), "h3".into()],
                    ..Default::default()
                }),
            },
            ComposeFile {
                path: "/opt/app/docker-compose.yml".into(),
                images: vec![ComposeService {
                    service: "web".into(),
                    image: "nginx:1.24".into(),
                }],
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                aggregate: Some(AggregatePrevalence {
                    count: 2,
                    total: 5,
                    hosts: vec!["h4".into(), "h5".into()],
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    });

    let session = RefineSession::new(snap);

    // Pristine session with multi-variant compose must report clean
    let changes = session.pending_changes();
    assert_eq!(
        changes.variants_changed, 0,
        "pristine compose session must report variants_changed == 0, got {}",
        changes.variants_changed,
    );
    assert!(
        !changes.is_dirty,
        "pristine compose session must not be dirty",
    );
}

#[test]
fn compose_select_variant_marks_dirty_then_revert_is_clean() {
    let mut snap = make_aggregate_snapshot(5);
    let images_a = vec![ComposeService {
        service: "web".into(),
        image: "nginx:1.25".into(),
    }];
    let images_b = vec![ComposeService {
        service: "web".into(),
        image: "nginx:1.24".into(),
    }];
    let hash_a = ContentHash::from_content(serde_json::to_string(&images_a).unwrap().as_bytes());
    let hash_b = ContentHash::from_content(serde_json::to_string(&images_b).unwrap().as_bytes());

    snap.containers = Some(ContainerSection {
        compose_files: vec![
            ComposeFile {
                path: "/opt/app/docker-compose.yml".into(),
                images: images_a,
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                aggregate: Some(AggregatePrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["h1".into(), "h2".into(), "h3".into()],
                    ..Default::default()
                }),
            },
            ComposeFile {
                path: "/opt/app/docker-compose.yml".into(),
                images: images_b,
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                aggregate: Some(AggregatePrevalence {
                    count: 2,
                    total: 5,
                    hosts: vec!["h4".into(), "h5".into()],
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);

    // Select variant B
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Compose {
                path: "/opt/app/docker-compose.yml".into(),
            },
            target: hash_b.clone(),
        })
        .unwrap();

    let changes = session.pending_changes();
    assert!(
        changes.variants_changed > 0,
        "compose SelectVariant must report dirty",
    );
    assert!(
        changes.is_dirty,
        "compose session must be dirty after variant change"
    );

    // Revert to original selection A
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Compose {
                path: "/opt/app/docker-compose.yml".into(),
            },
            target: hash_a.clone(),
        })
        .unwrap();

    let changes = session.pending_changes();
    assert_eq!(
        changes.variants_changed, 0,
        "compose select A→B then B→A must be net-zero, got {}",
        changes.variants_changed,
    );
    assert!(
        !changes.is_dirty,
        "compose net-zero round-trip must be clean",
    );
}
