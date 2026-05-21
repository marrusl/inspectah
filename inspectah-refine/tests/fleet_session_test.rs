use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, PrevalenceZone, VariantSelection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};
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

// ---------------------------------------------------------------------------
// R1 fixup tests: multi-variant zone, drop-in/quadlet zones, fleet_attention
// ---------------------------------------------------------------------------

fn fleet_prevalence(count: i32, total: i32) -> Option<FleetPrevalence> {
    Some(FleetPrevalence {
        count,
        total,
        hosts: (0..count).map(|i| format!("host-{i}")).collect(),
    })
}

#[test]
fn multi_variant_path_zone_uses_sum_prevalence() {
    // Three config variants for /etc/app/main.conf: 3/5, 1/5, 1/5.
    // Zone should sum prevalence: 3+1+1=5 of 5 hosts = Consensus.
    // The merger partitions hosts across variants — each host appears in
    // exactly one variant, so summing gives item-level prevalence.
    let mut snap = make_fleet_snapshot(5);
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                fleet: fleet_prevalence(3, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                fleet: fleet_prevalence(1, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                include: true,
                fleet: fleet_prevalence(1, 5),
                ..Default::default()
            },
        ],
    });

    let session = RefineSession::new(snap);
    let ctx = session.fleet_context().unwrap();
    let item = ItemId::Config {
        path: "/etc/app/main.conf".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::Consensus),
        "zone must use sum prevalence (3+1+1=5/5), not max",
    );
}

#[test]
fn zone_for_partial_path_is_divergent() {
    // A path present on only 2 of 5 hosts (one variant, count=2/total=5) → Divergent.
    // Proves non-trivial classification still works with sum approach.
    let mut snap = make_fleet_snapshot(5);
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/app/rare.conf".into(),
            include: true,
            fleet: fleet_prevalence(2, 5),
            ..Default::default()
        }],
    });

    let session = RefineSession::new(snap);
    let ctx = session.fleet_context().unwrap();
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
fn dropin_zone_classified_on_fleet_init() {
    let mut snap = make_fleet_snapshot(5);
    snap.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "httpd.service".into(),
            path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
            content: "test".into(),
            include: true,
            fleet: fleet_prevalence(4, 5),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let ctx = session.fleet_context().unwrap();
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
fn quadlet_zone_classified_on_fleet_init() {
    let mut snap = make_fleet_snapshot(5);
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            path: "/etc/containers/systemd/myapp.container".into(),
            name: "myapp".into(),
            include: true,
            fleet: fleet_prevalence(5, 5),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let ctx = session.fleet_context().unwrap();
    let item = ItemId::Quadlet {
        path: "/etc/containers/systemd/myapp.container".into(),
    };
    assert_eq!(
        ctx.zones.get(&item),
        Some(&PrevalenceZone::Consensus),
        "quadlet must appear in zone map",
    );
}

#[test]
fn fleet_session_populates_fleet_attention_on_refined_package() {
    let mut snap = make_fleet_snapshot(5);
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "rhel-9-appstream".into(),
            include: true,
            fleet: fleet_prevalence(3, 5),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let view = session.view();
    assert!(!view.packages.is_empty(), "must have packages");
    let pkg = &view.packages[0];
    assert!(
        pkg.fleet_attention.is_some(),
        "fleet session must populate fleet_attention on packages",
    );
    let fa = pkg.fleet_attention.unwrap();
    assert_eq!(fa.prevalence, 3);
}

#[test]
fn single_host_session_has_no_fleet_attention() {
    let mut snap = InspectionSnapshot::default();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "rhel-9-appstream".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let view = session.view();
    assert!(!view.packages.is_empty(), "must have packages");
    for pkg in &view.packages {
        assert!(
            pkg.fleet_attention.is_none(),
            "single-host session must NOT populate fleet_attention",
        );
    }
}

// ---------------------------------------------------------------------------
// R4a: Projection-based dirty state — net-zero variant ops should be clean
// ---------------------------------------------------------------------------

#[test]
fn variants_changed_net_zero_is_clean() {
    // Create a fleet snapshot with two config variants for the same path
    let mut snap = make_fleet_snapshot(5);
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
                variant_selection: VariantSelection::Selected,
                fleet: fleet_prevalence(3, 5),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                content: content_b.into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: fleet_prevalence(2, 5),
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
    assert!(changes.is_dirty, "session must be dirty after variant change");

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
