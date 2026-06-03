use std::collections::BTreeMap;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{
    ComposeFile, ComposeService, ContainerSection, QuadletUnit,
};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, VariantSelection};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fleet snapshot with config variants for testing.
///
/// Creates a fleet of `host_count` hosts with a config file at `path` that has
/// two variants: `content_a` (Selected, seen on `count_a` hosts) and
/// `content_b` (Alternative, seen on `count_b` hosts).
fn make_variant_snapshot(
    path: &str,
    content_a: &str,
    count_a: i32,
    content_b: &str,
    count_b: i32,
) -> InspectionSnapshot {
    let host_count = (count_a + count_b) as usize;
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: path.into(),
                content: content_a.into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: count_a,
                    total: host_count as i32,
                    hosts: (0..count_a as usize).map(|i| format!("host-{i}")).collect(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigFileEntry {
                path: path.into(),
                content: content_b.into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: count_b,
                    total: host_count as i32,
                    hosts: (count_a as usize..host_count)
                        .map(|i| format!("host-{i}"))
                        .collect(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
    });
    snap
}

/// Build a fleet snapshot with a single "Only" config variant.
fn make_single_variant_snapshot(
    path: &str,
    content: &str,
    host_count: usize,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: path.into(),
            content: content.into(),
            include: true,
            locked: false,
            variant_selection: VariantSelection::Only,
            fleet: Some(FleetPrevalence {
                count: host_count as i32,
                total: host_count as i32,
                hosts: (0..host_count).map(|i| format!("host-{i}")).collect(),
                ..Default::default()
            }),
            ..Default::default()
        }],
    });
    snap
}

/// Helper to find all config entries for a given path in a projected snapshot.
fn variants_for_path(snap: &InspectionSnapshot, path: &str) -> Vec<ConfigFileEntry> {
    snap.config
        .as_ref()
        .map(|c| c.files.iter().filter(|e| e.path == path).cloned().collect())
        .unwrap_or_default()
}

// ===========================================================================
// SelectVariant tests
// ===========================================================================

#[test]
fn select_variant_swaps_selected_and_alternative() {
    let path = "/etc/nginx/nginx.conf";
    let content_a = "worker_processes 4;";
    let content_b = "worker_processes 8;";

    let snap = make_variant_snapshot(path, content_a, 3, content_b, 2);
    let mut session = RefineSession::new(snap);

    // Before: content_a is Selected, content_b is Alternative
    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_a)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_b)
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );

    // Apply SelectVariant targeting content_b
    let hash_b = ContentHash::from_content(content_b.as_bytes());
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config { path: path.into() },
            target: hash_b,
        })
        .unwrap();

    // After: content_b is Selected, content_a is Alternative
    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_b)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_a)
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn select_variant_already_selected_is_noop_but_harmless() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let hash_a = ContentHash::from_content(b"aaa");
    // Selecting the already-Selected variant should succeed (idempotent)
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config { path: path.into() },
            target: hash_a,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "aaa")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
}

// ===========================================================================
// EditVariant tests
// ===========================================================================

#[test]
fn edit_variant_creates_new_user_variant() {
    let path = "/etc/nginx/nginx.conf";
    let snap = make_variant_snapshot(path, "original-a", 3, "original-b", 2);
    let mut session = RefineSession::new(snap);

    let new_content = "worker_processes auto;";
    let hash_a = ContentHash::from_content(b"original-a");
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: new_content.into(),
            based_on: Some(hash_a),
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);

    // Should now have 3 variants: original-a (Alternative), original-b (Alternative),
    // new_content (Selected — user edit becomes the active variant)
    assert_eq!(variants.len(), 3, "edit should add a new variant");
    let new_variant = variants.iter().find(|v| v.content == new_content).unwrap();
    assert_eq!(new_variant.variant_selection, VariantSelection::Selected);

    // The previously-Selected original-a should now be Alternative
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "original-a")
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn edit_variant_converges_with_existing_content() {
    let path = "/etc/test.conf";
    let content_a = "version=1";
    let content_b = "version=2";
    let snap = make_variant_snapshot(path, content_a, 3, content_b, 2);
    let mut session = RefineSession::new(snap);

    // Edit with content that matches the existing content_b
    // This should converge: select the matching variant instead of creating a duplicate
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: content_b.into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);

    // Should still have exactly 2 variants (no duplicate created)
    assert_eq!(variants.len(), 2, "convergence should not create duplicate");
    // content_b should now be Selected (converged edit selects it)
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_b)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
}

#[test]
fn edit_variant_based_on_nonexistent_hash_rejected() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let bogus_hash = ContentHash::new("f".repeat(64)).unwrap();
    // based_on pointing to a non-existent hash should be rejected
    let result = session.apply(RefinementOp::EditVariant {
        item_id: ItemId::Config { path: path.into() },
        content: "new-content".into(),
        based_on: Some(bogus_hash),
    });
    assert!(
        result.is_err(),
        "EditVariant with bogus based_on hash must be rejected"
    );
}

#[test]
fn edit_variant_on_only_item_transitions_to_selected_alternative() {
    let path = "/etc/single.conf";
    let snap = make_single_variant_snapshot(path, "only-content", 5);
    let mut session = RefineSession::new(snap);

    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-edit".into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2, "edit on Only creates a second variant");

    // User edit becomes Selected
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "user-edit")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    // Original becomes Alternative
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "only-content")
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

// ===========================================================================
// DiscardVariant tests
// ===========================================================================

#[test]
fn discard_user_created_variant() {
    let path = "/etc/nginx/nginx.conf";
    let snap = make_variant_snapshot(path, "host-a", 3, "host-b", 2);
    let mut session = RefineSession::new(snap);

    // Create a user variant
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-edit".into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    assert_eq!(variants_for_path(&proj, path).len(), 3);

    // Discard the user variant
    let user_hash = ContentHash::from_content(b"user-edit");
    session
        .apply(RefinementOp::DiscardVariant {
            item_id: ItemId::Config { path: path.into() },
            variant: user_hash,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2, "discard should remove the user variant");
    assert!(!variants.iter().any(|v| v.content == "user-edit"));
}

#[test]
fn discard_selected_falls_back_to_original_selection() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "high-prev", 4, "low-prev", 1);
    let mut session = RefineSession::new(snap);

    // Create user variant (becomes Selected)
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-content".into(),
            based_on: None,
        })
        .unwrap();

    // Verify user-content is Selected
    let proj = session.snapshot_projected();
    assert_eq!(
        variants_for_path(&proj, path)
            .iter()
            .find(|v| v.content == "user-content")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );

    // Discard the Selected user variant
    let user_hash = ContentHash::from_content(b"user-content");
    session
        .apply(RefinementOp::DiscardVariant {
            item_id: ItemId::Config { path: path.into() },
            variant: user_hash,
        })
        .unwrap();

    // Fallback: most-prevalent host-sourced variant becomes Selected
    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "high-prev")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
        "fallback should select the most-prevalent host-sourced variant",
    );
}

#[test]
fn discard_host_sourced_variant_fails() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "host-a", 3, "host-b", 2);
    let mut session = RefineSession::new(snap);

    let hash_a = ContentHash::from_content(b"host-a");
    let result = session.apply(RefinementOp::DiscardVariant {
        item_id: ItemId::Config { path: path.into() },
        variant: hash_a,
    });
    assert!(
        result.is_err(),
        "discarding a host-sourced variant should fail"
    );
}

#[test]
fn discard_leaving_one_variant_becomes_only() {
    let path = "/etc/single.conf";
    let snap = make_single_variant_snapshot(path, "original", 5);
    let mut session = RefineSession::new(snap);

    // Add a user variant (now 2 variants)
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-edit".into(),
            based_on: None,
        })
        .unwrap();

    // Discard the user variant (back to 1)
    let user_hash = ContentHash::from_content(b"user-edit");
    session
        .apply(RefinementOp::DiscardVariant {
            item_id: ItemId::Config { path: path.into() },
            variant: user_hash,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 1);
    assert_eq!(
        variants[0].variant_selection,
        VariantSelection::Only,
        "single remaining variant should be Only",
    );
}

// ===========================================================================
// Undo tests
// ===========================================================================

#[test]
fn undo_select_variant_restores_previous_selection() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let hash_b = ContentHash::from_content(b"bbb");
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config { path: path.into() },
            target: hash_b,
        })
        .unwrap();

    // After apply: bbb is Selected
    let proj = session.snapshot_projected();
    assert_eq!(
        variants_for_path(&proj, path)
            .iter()
            .find(|v| v.content == "bbb")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );

    // Undo
    session.undo().unwrap();

    // After undo: aaa is Selected again (original state)
    let proj = session.snapshot_projected();
    assert_eq!(
        variants_for_path(&proj, path)
            .iter()
            .find(|v| v.content == "aaa")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants_for_path(&proj, path)
            .iter()
            .find(|v| v.content == "bbb")
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn undo_edit_variant_removes_user_content() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-edit".into(),
            based_on: None,
        })
        .unwrap();

    // After apply: 3 variants
    let proj = session.snapshot_projected();
    assert_eq!(variants_for_path(&proj, path).len(), 3);

    // Undo
    session.undo().unwrap();

    // After undo: back to 2 original variants
    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert!(!variants.iter().any(|v| v.content == "user-edit"));
    // Original selection should be restored
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == "aaa")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
}

#[test]
fn undo_discard_variant_restores_discarded_variant() {
    let path = "/etc/test.conf";
    let snap = make_variant_snapshot(path, "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    // Create then discard a user variant
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Config { path: path.into() },
            content: "user-edit".into(),
            based_on: None,
        })
        .unwrap();

    let user_hash = ContentHash::from_content(b"user-edit");
    session
        .apply(RefinementOp::DiscardVariant {
            item_id: ItemId::Config { path: path.into() },
            variant: user_hash,
        })
        .unwrap();

    // After discard: 2 variants
    let proj = session.snapshot_projected();
    assert_eq!(variants_for_path(&proj, path).len(), 2);

    // Undo the discard
    session.undo().unwrap();

    // After undo: 3 variants again (user-edit restored)
    let proj = session.snapshot_projected();
    let variants = variants_for_path(&proj, path);
    assert_eq!(variants.len(), 3);
    assert!(variants.iter().any(|v| v.content == "user-edit"));
}

// ===========================================================================
// Validation tests
// ===========================================================================

#[test]
fn select_variant_unknown_path_fails() {
    let snap = make_variant_snapshot("/etc/real.conf", "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let hash = ContentHash::from_content(b"aaa");
    let result = session.apply(RefinementOp::SelectVariant {
        item_id: ItemId::Config {
            path: "/etc/nonexistent.conf".into(),
        },
        target: hash,
    });
    assert!(result.is_err(), "select on unknown path should fail");
}

#[test]
fn select_variant_unknown_hash_fails() {
    let snap = make_variant_snapshot("/etc/test.conf", "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let bogus = ContentHash::new("0".repeat(64)).unwrap();
    let result = session.apply(RefinementOp::SelectVariant {
        item_id: ItemId::Config {
            path: "/etc/test.conf".into(),
        },
        target: bogus,
    });
    assert!(result.is_err(), "select with unknown hash should fail");
}

#[test]
fn discard_variant_unknown_hash_fails() {
    let snap = make_variant_snapshot("/etc/test.conf", "aaa", 3, "bbb", 2);
    let mut session = RefineSession::new(snap);

    let bogus = ContentHash::new("0".repeat(64)).unwrap();
    let result = session.apply(RefinementOp::DiscardVariant {
        item_id: ItemId::Config {
            path: "/etc/test.conf".into(),
        },
        variant: bogus,
    });
    assert!(result.is_err(), "discard with unknown hash should fail");
}

// ===========================================================================
// DropIn/Quadlet/Compose helper functions
// ===========================================================================

/// Build a fleet snapshot with two DropIn variants for testing.
fn make_dropin_variant_snapshot(
    path: &str,
    content_a: &str,
    count_a: i32,
    content_b: &str,
    count_b: i32,
) -> InspectionSnapshot {
    let host_count = (count_a + count_b) as usize;
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.services = Some(ServiceSection {
        drop_ins: vec![
            SystemdDropIn {
                unit: "test.service".into(),
                path: path.into(),
                content: content_a.into(),
                include: true,
                locked: false,
                attention_reason: None,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: count_a,
                    total: host_count as i32,
                    hosts: (0..count_a as usize).map(|i| format!("host-{i}")).collect(),
                    ..Default::default()
                }),
            },
            SystemdDropIn {
                unit: "test.service".into(),
                path: path.into(),
                content: content_b.into(),
                include: true,
                locked: false,
                attention_reason: None,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: count_b,
                    total: host_count as i32,
                    hosts: (count_a as usize..host_count)
                        .map(|i| format!("host-{i}"))
                        .collect(),
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    });
    snap
}

/// Build a fleet snapshot with two Quadlet variants for testing.
fn make_quadlet_variant_snapshot(
    path: &str,
    content_a: &str,
    count_a: i32,
    content_b: &str,
    count_b: i32,
) -> InspectionSnapshot {
    let host_count = (count_a + count_b) as usize;
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![
            QuadletUnit {
                path: path.into(),
                name: "test.container".into(),
                content: content_a.into(),
                image: "quay.io/test:latest".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: count_a,
                    total: host_count as i32,
                    hosts: (0..count_a as usize).map(|i| format!("host-{i}")).collect(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            QuadletUnit {
                path: path.into(),
                name: "test.container".into(),
                content: content_b.into(),
                image: "quay.io/test:latest".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: count_b,
                    total: host_count as i32,
                    hosts: (count_a as usize..host_count)
                        .map(|i| format!("host-{i}"))
                        .collect(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

/// Build a fleet snapshot with two Compose variants (different images lists).
fn make_compose_variant_snapshot(path: &str) -> InspectionSnapshot {
    let host_count = 5;
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.containers = Some(ContainerSection {
        compose_files: vec![
            ComposeFile {
                path: path.into(),
                images: vec![ComposeService {
                    service: "web".into(),
                    image: "nginx:1.24".into(),
                }],
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: host_count as i32,
                    hosts: vec!["host-0".into(), "host-1".into(), "host-2".into()],
                    ..Default::default()
                }),
            },
            ComposeFile {
                path: path.into(),
                images: vec![ComposeService {
                    service: "web".into(),
                    image: "nginx:1.25".into(),
                }],
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: host_count as i32,
                    hosts: vec!["host-3".into(), "host-4".into()],
                    ..Default::default()
                }),
            },
        ],
        ..Default::default()
    });
    snap
}

/// Helper to find all DropIn entries for a given path in a projected snapshot.
fn dropins_for_path(snap: &InspectionSnapshot, path: &str) -> Vec<SystemdDropIn> {
    snap.services
        .as_ref()
        .map(|s| {
            s.drop_ins
                .iter()
                .filter(|e| e.path == path)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Helper to find all Quadlet entries for a given path in a projected snapshot.
fn quadlets_for_path(snap: &InspectionSnapshot, path: &str) -> Vec<QuadletUnit> {
    snap.containers
        .as_ref()
        .map(|c| {
            c.quadlet_units
                .iter()
                .filter(|e| e.path == path)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Helper to find all Compose entries for a given path in a projected snapshot.
fn compose_for_path(snap: &InspectionSnapshot, path: &str) -> Vec<ComposeFile> {
    snap.containers
        .as_ref()
        .map(|c| {
            c.compose_files
                .iter()
                .filter(|e| e.path == path)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

// ===========================================================================
// DropIn variant tests
// ===========================================================================

#[test]
fn select_variant_dropin_swaps_selection() {
    let path = "/etc/systemd/system/test.service.d/override.conf";
    let content_a = "[Service]\nTimeoutStartSec=90";
    let content_b = "[Service]\nTimeoutStartSec=120";

    let snap = make_dropin_variant_snapshot(path, content_a, 3, content_b, 2);
    let mut session = RefineSession::new(snap);

    // Before: content_a is Selected
    let proj = session.snapshot_projected();
    let variants = dropins_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_a)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );

    // Select content_b
    let hash_b = ContentHash::from_content(content_b.as_bytes());
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::DropIn { path: path.into() },
            target: hash_b,
        })
        .unwrap();

    // After: content_b is Selected
    let proj = session.snapshot_projected();
    let variants = dropins_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_b)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_a)
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn edit_variant_dropin_creates_user_variant() {
    let path = "/etc/systemd/system/test.service.d/override.conf";
    let snap = make_dropin_variant_snapshot(path, "original-a", 3, "original-b", 2);
    let mut session = RefineSession::new(snap);

    let new_content = "[Service]\nTimeoutStartSec=300";
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::DropIn { path: path.into() },
            content: new_content.into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = dropins_for_path(&proj, path);
    assert_eq!(variants.len(), 3, "edit should add a new drop-in variant");
    let new_variant = variants.iter().find(|v| v.content == new_content).unwrap();
    assert_eq!(new_variant.variant_selection, VariantSelection::Selected);
}

#[test]
fn discard_variant_dropin_removes_user_variant() {
    let path = "/etc/systemd/system/test.service.d/override.conf";
    let snap = make_dropin_variant_snapshot(path, "host-a", 3, "host-b", 2);
    let mut session = RefineSession::new(snap);

    // Create a user variant
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::DropIn { path: path.into() },
            content: "user-dropin".into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    assert_eq!(dropins_for_path(&proj, path).len(), 3);

    // Discard the user variant
    let user_hash = ContentHash::from_content(b"user-dropin");
    session
        .apply(RefinementOp::DiscardVariant {
            item_id: ItemId::DropIn { path: path.into() },
            variant: user_hash,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = dropins_for_path(&proj, path);
    assert_eq!(
        variants.len(),
        2,
        "discard should remove the user drop-in variant"
    );
    assert!(!variants.iter().any(|v| v.content == "user-dropin"));
}

// ===========================================================================
// Quadlet variant tests
// ===========================================================================

#[test]
fn select_variant_quadlet_works() {
    let path = "/etc/containers/systemd/app.container";
    let content_a = "[Container]\nImage=quay.io/app:v1";
    let content_b = "[Container]\nImage=quay.io/app:v2";

    let snap = make_quadlet_variant_snapshot(path, content_a, 3, content_b, 2);
    let mut session = RefineSession::new(snap);

    // Select content_b
    let hash_b = ContentHash::from_content(content_b.as_bytes());
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Quadlet { path: path.into() },
            target: hash_b,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = quadlets_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_b)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants
            .iter()
            .find(|v| v.content == content_a)
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn edit_variant_quadlet_works() {
    let path = "/etc/containers/systemd/app.container";
    let snap = make_quadlet_variant_snapshot(path, "original-a", 3, "original-b", 2);
    let mut session = RefineSession::new(snap);

    let new_content = "[Container]\nImage=quay.io/app:v3";
    session
        .apply(RefinementOp::EditVariant {
            item_id: ItemId::Quadlet { path: path.into() },
            content: new_content.into(),
            based_on: None,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = quadlets_for_path(&proj, path);
    assert_eq!(variants.len(), 3, "edit should add a new quadlet variant");
    let new_variant = variants.iter().find(|v| v.content == new_content).unwrap();
    assert_eq!(new_variant.variant_selection, VariantSelection::Selected);
}

// ===========================================================================
// Compose variant tests
// ===========================================================================

#[test]
fn select_variant_compose_works() {
    let path = "/opt/app/docker-compose.yml";
    let snap = make_compose_variant_snapshot(path);
    let mut session = RefineSession::new(snap);

    // Compute hash of the Alternative variant (nginx:1.25)
    let alt_images = vec![ComposeService {
        service: "web".into(),
        image: "nginx:1.25".into(),
    }];
    let hash_b = ContentHash::from_content(serde_json::to_string(&alt_images).unwrap().as_bytes());

    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Compose { path: path.into() },
            target: hash_b,
        })
        .unwrap();

    let proj = session.snapshot_projected();
    let variants = compose_for_path(&proj, path);
    assert_eq!(variants.len(), 2);
    // The nginx:1.25 variant should now be Selected
    assert_eq!(
        variants
            .iter()
            .find(|v| v.images[0].image == "nginx:1.25")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
    );
    assert_eq!(
        variants
            .iter()
            .find(|v| v.images[0].image == "nginx:1.24")
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
    );
}

#[test]
fn edit_variant_compose_rejected() {
    let path = "/opt/app/docker-compose.yml";
    let snap = make_compose_variant_snapshot(path);
    let mut session = RefineSession::new(snap);

    let result = session.apply(RefinementOp::EditVariant {
        item_id: ItemId::Compose { path: path.into() },
        content: "should-not-work".into(),
        based_on: None,
    });
    assert!(
        result.is_err(),
        "EditVariant must be rejected for Compose items"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Compose"),
        "error should mention Compose: {err_msg}"
    );
}

#[test]
fn edit_variant_based_on_hash_from_different_item_rejected() {
    // Create snapshot with two config paths, each having variants.
    // Hash from path A used as based_on for EditVariant on path B should fail.
    let host_count = 5;
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test".into(),
        host_count,
        hostnames: (0..host_count).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![
            // Path A
            ConfigFileEntry {
                path: "/etc/path-a.conf".into(),
                content: "path-a-content".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: host_count as i32,
                    hosts: vec!["host-0".into(), "host-1".into(), "host-2".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/path-a.conf".into(),
                content: "path-a-variant-2".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: host_count as i32,
                    hosts: vec!["host-3".into(), "host-4".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            // Path B
            ConfigFileEntry {
                path: "/etc/path-b.conf".into(),
                content: "path-b-content".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 4,
                    total: host_count as i32,
                    hosts: vec![
                        "host-0".into(),
                        "host-1".into(),
                        "host-2".into(),
                        "host-3".into(),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/path-b.conf".into(),
                content: "path-b-variant-2".into(),
                include: true,
                locked: false,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: host_count as i32,
                    hosts: vec!["host-4".into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
    });

    let mut session = RefineSession::new(snap);

    // Get hash of path-a content
    let hash_from_path_a = ContentHash::from_content(b"path-a-content");

    // Try to use path-a's hash as based_on for an edit on path-b — should fail
    let result = session.apply(RefinementOp::EditVariant {
        item_id: ItemId::Config {
            path: "/etc/path-b.conf".into(),
        },
        content: "new-content-for-b".into(),
        based_on: Some(hash_from_path_a),
    });
    assert!(
        result.is_err(),
        "EditVariant with based_on hash from a different item's path must be rejected"
    );
}

#[test]
fn discard_variant_compose_rejected() {
    let path = "/opt/app/docker-compose.yml";
    let snap = make_compose_variant_snapshot(path);
    let mut session = RefineSession::new(snap);

    let bogus = ContentHash::new("0".repeat(64)).unwrap();
    let result = session.apply(RefinementOp::DiscardVariant {
        item_id: ItemId::Compose { path: path.into() },
        variant: bogus,
    });
    assert!(
        result.is_err(),
        "DiscardVariant must be rejected for Compose items"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Compose"),
        "error should mention Compose: {err_msg}"
    );
}
