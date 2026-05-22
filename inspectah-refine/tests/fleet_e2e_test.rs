//! End-to-end lifecycle test for the fleet refine engine.
//!
//! Exercises the full fleet refine pipeline in a single test:
//! zone classification, fleet detection, attention scoring, variant ops,
//! diff engine, variant summary, undo, and export.

use std::collections::BTreeMap;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, PrevalenceZone, VariantSelection};
use inspectah_refine::fleet::attention::score_fleet_attention;
use inspectah_refine::fleet::diff::compute_diff;
use inspectah_refine::fleet::variant_summary;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{
    AttentionLevel, AttentionScore, ContentHash, ItemId, RefinementOp,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fleet snapshot with 5 hosts, two config paths:
/// - `/etc/app/main.conf`: 3 variants (3 hosts, 1 host, 1 host)
/// - `/etc/app/db.conf`: 2 variants (4 hosts, 1 host)
/// - `/etc/app/logging.conf`: 1 variant (all 5 hosts — no divergence)
fn make_e2e_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "e2e-fleet".into(),
        host_count: 5,
        hostnames: (0..5).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::new(),
    });
    snap.config = Some(ConfigSection {
        files: vec![
            // /etc/app/main.conf — variant A (3 hosts, Selected)
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                content: "setting=alpha".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["host-0".into(), "host-1".into(), "host-2".into()],
                }),
                ..Default::default()
            },
            // /etc/app/main.conf — variant B (1 host, Alternative)
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                content: "setting=beta".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 5,
                    hosts: vec!["host-3".into()],
                }),
                ..Default::default()
            },
            // /etc/app/main.conf — variant C (1 host, Alternative)
            ConfigFileEntry {
                path: "/etc/app/main.conf".into(),
                content: "setting=gamma".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 5,
                    hosts: vec!["host-4".into()],
                }),
                ..Default::default()
            },
            // /etc/app/db.conf — variant A (4 hosts, Selected)
            ConfigFileEntry {
                path: "/etc/app/db.conf".into(),
                content: "host=db-primary".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 4,
                    total: 5,
                    hosts: vec![
                        "host-0".into(),
                        "host-1".into(),
                        "host-2".into(),
                        "host-3".into(),
                    ],
                }),
                ..Default::default()
            },
            // /etc/app/db.conf — variant B (1 host, Alternative)
            ConfigFileEntry {
                path: "/etc/app/db.conf".into(),
                content: "host=db-replica".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 1,
                    total: 5,
                    hosts: vec!["host-4".into()],
                }),
                ..Default::default()
            },
            // /etc/app/logging.conf — single variant (all 5 hosts, Only)
            ConfigFileEntry {
                path: "/etc/app/logging.conf".into(),
                content: "level=info".into(),
                include: true,
                variant_selection: VariantSelection::Only,
                fleet: Some(FleetPrevalence {
                    count: 5,
                    total: 5,
                    hosts: (0..5).map(|i| format!("host-{i}")).collect(),
                }),
                ..Default::default()
            },
        ],
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
// variant_summary unit tests
// ===========================================================================

#[test]
fn summary_none_for_single_host() {
    // Single-host snapshot — no fleet_meta, no FleetContext.
    let snap = InspectionSnapshot::default();
    let session = RefineSession::new(snap);
    assert!(
        variant_summary(&session.snapshot_projected(), session.fleet_context()).is_none(),
        "variant_summary must return None for single-host sessions",
    );
}

#[test]
fn summary_counts_paths_with_variants() {
    let snap = make_e2e_snapshot();
    let session = RefineSession::new(snap);
    let summary = variant_summary(&session.snapshot_projected(), session.fleet_context())
        .expect("fleet session should produce a summary");

    // Two paths have variants: main.conf (3 variants) and db.conf (2 variants).
    // logging.conf has only 1 variant (Only) so it's excluded.
    assert_eq!(
        summary.paths_with_variants, 2,
        "should count only paths with 2+ variants",
    );
    assert!(summary.variant_distribution.contains_key("/etc/app/main.conf"));
    assert!(summary.variant_distribution.contains_key("/etc/app/db.conf"));
    assert!(!summary.variant_distribution.contains_key("/etc/app/logging.conf"));
}

#[test]
fn summary_reports_variant_count_per_path() {
    let snap = make_e2e_snapshot();
    let session = RefineSession::new(snap);
    let summary = variant_summary(&session.snapshot_projected(), session.fleet_context()).unwrap();

    assert_eq!(
        summary.variant_distribution["/etc/app/main.conf"].variant_count,
        3,
    );
    assert_eq!(
        summary.variant_distribution["/etc/app/db.conf"].variant_count,
        2,
    );
}

#[test]
fn summary_reports_host_split_sorted_descending() {
    let snap = make_e2e_snapshot();
    let session = RefineSession::new(snap);
    let summary = variant_summary(&session.snapshot_projected(), session.fleet_context()).unwrap();

    // main.conf: 3 hosts, 1 host, 1 host → [3, 1, 1]
    assert_eq!(
        summary.variant_distribution["/etc/app/main.conf"].host_split,
        vec![3, 1, 1],
    );
    // db.conf: 4 hosts, 1 host → [4, 1]
    assert_eq!(
        summary.variant_distribution["/etc/app/db.conf"].host_split,
        vec![4, 1],
    );
}

// ===========================================================================
// Full lifecycle E2E test
// ===========================================================================

#[test]
fn fleet_refine_full_lifecycle() {
    // -----------------------------------------------------------------------
    // 1. Build fleet snapshot: 5 hosts, configs with variants, mixed prevalence
    // -----------------------------------------------------------------------
    let snap = make_e2e_snapshot();

    // -----------------------------------------------------------------------
    // 2. Init session — verify fleet mode, zones computed
    // -----------------------------------------------------------------------
    let mut session = RefineSession::new(snap);
    let fleet_ctx = session
        .fleet_context()
        .expect("session must detect fleet mode");
    assert_eq!(fleet_ctx.total_hosts, 5);
    assert!(
        fleet_ctx.zones_active,
        "zones must be active for fleet of 5",
    );

    // -----------------------------------------------------------------------
    // 3. Check zone classification
    // -----------------------------------------------------------------------
    let main_id = ItemId::Config {
        path: "/etc/app/main.conf".into(),
    };
    // Zone uses most-divergent variant: 3/5→NearConsensus, 1/5→Divergent, 1/5→Divergent.
    // The path-level zone is the min (most-divergent): Divergent.
    assert_eq!(
        fleet_ctx.zones.get(&main_id),
        Some(&PrevalenceZone::Divergent),
        "main.conf zone should use most-divergent variant (1/5 → Divergent)",
    );

    // logging.conf — single variant, 5/5 → Consensus
    let log_id = ItemId::Config {
        path: "/etc/app/logging.conf".into(),
    };
    assert_eq!(
        fleet_ctx.zones.get(&log_id),
        Some(&PrevalenceZone::Consensus),
        "logging.conf (5/5) must be Consensus",
    );

    // -----------------------------------------------------------------------
    // 4. Attention scoring — verify fleet-aware scoring works
    // -----------------------------------------------------------------------
    let score = score_fleet_attention(fleet_ctx, &main_id, AttentionLevel::Informational, 3);
    match &score {
        AttentionScore::Fleet(fa) => {
            assert_eq!(fa.attention, AttentionLevel::Informational);
        }
        AttentionScore::SingleHost(_) => {
            panic!("fleet session must produce Fleet attention score");
        }
    }

    // -----------------------------------------------------------------------
    // 5. SelectVariant — pick variant B for main.conf
    // -----------------------------------------------------------------------
    let hash_beta = ContentHash::from_content(b"setting=beta");
    session
        .apply(RefinementOp::SelectVariant {
            item_id: main_id.clone(),
            target: hash_beta,
        })
        .expect("SelectVariant should succeed");

    let proj = session.snapshot_projected();
    let main_variants = variants_for_path(&proj, "/etc/app/main.conf");
    assert_eq!(main_variants.len(), 3, "variant count must not change");
    assert_eq!(
        main_variants
            .iter()
            .find(|v| v.content == "setting=beta")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
        "beta must be Selected after SelectVariant",
    );
    assert_eq!(
        main_variants
            .iter()
            .find(|v| v.content == "setting=alpha")
            .unwrap()
            .variant_selection,
        VariantSelection::Alternative,
        "alpha must be Alternative after SelectVariant",
    );

    // -----------------------------------------------------------------------
    // 6. EditVariant — create a user-modified version
    // -----------------------------------------------------------------------
    let edited_content = "setting=custom-merged";
    session
        .apply(RefinementOp::EditVariant {
            item_id: main_id.clone(),
            content: edited_content.into(),
            based_on: Some(ContentHash::from_content(b"setting=beta")),
        })
        .expect("EditVariant should succeed");

    let proj = session.snapshot_projected();
    let main_variants = variants_for_path(&proj, "/etc/app/main.conf");
    assert_eq!(
        main_variants.len(),
        4,
        "edit should add a fourth variant",
    );
    assert_eq!(
        main_variants
            .iter()
            .find(|v| v.content == edited_content)
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
        "user edit must become Selected",
    );

    // -----------------------------------------------------------------------
    // 7. Compute diff between original alpha and edited content
    // -----------------------------------------------------------------------
    let diff_result = compute_diff("setting=alpha", edited_content, 3)
        .expect("diff should succeed");
    assert!(
        diff_result.stats.insertions > 0 || diff_result.stats.deletions > 0,
        "diff must show changes between alpha and edited content",
    );

    // -----------------------------------------------------------------------
    // 8. Variant summary — verify it reflects the edit
    // -----------------------------------------------------------------------
    let summary = variant_summary(&session.snapshot_projected(), session.fleet_context())
        .expect("fleet session must produce a summary");
    // main.conf now has 4 variants (3 original + 1 user edit)
    assert_eq!(
        summary.variant_distribution["/etc/app/main.conf"].variant_count,
        4,
    );
    // db.conf still has 2
    assert_eq!(
        summary.variant_distribution["/etc/app/db.conf"].variant_count,
        2,
    );
    assert_eq!(summary.paths_with_variants, 2);

    // -----------------------------------------------------------------------
    // 9. Undo edit — edited variant removed
    // -----------------------------------------------------------------------
    session.undo().expect("undo should succeed");
    let proj = session.snapshot_projected();
    let main_variants = variants_for_path(&proj, "/etc/app/main.conf");
    assert_eq!(
        main_variants.len(),
        3,
        "undo should remove the user-edited variant",
    );
    assert!(
        !main_variants.iter().any(|v| v.content == edited_content),
        "edited content must not appear after undo",
    );
    // After undo of the edit, the SelectVariant is still active:
    // beta should still be Selected (from step 5)
    assert_eq!(
        main_variants
            .iter()
            .find(|v| v.content == "setting=beta")
            .unwrap()
            .variant_selection,
        VariantSelection::Selected,
        "beta must remain Selected after undoing only the edit",
    );

    // -----------------------------------------------------------------------
    // 10. Export — verify tarball has fleet/variants/ directory
    // -----------------------------------------------------------------------
    let tmpdir = tempfile::tempdir().expect("tempdir for export");
    let tarball_path = tmpdir.path().join("export.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .expect("export should succeed");

    // Read tarball entries and check for fleet/variants/ content
    let f = std::fs::File::open(&tarball_path).expect("open tarball");
    let gz = flate2::read::GzDecoder::new(f);
    let mut archive = tar::Archive::new(gz);
    let entry_paths: Vec<String> = archive
        .entries()
        .expect("read entries")
        .filter_map(|e| e.ok())
        .map(|e| e.path().unwrap().to_string_lossy().to_string())
        .collect();

    // Fleet snapshot with alternatives should produce fleet/variants/ entries
    assert!(
        entry_paths.iter().any(|p| p.starts_with("fleet/variants/")),
        "export must contain fleet/variants/ directory for alternative variants; entries: {entry_paths:?}",
    );

    // Core export artifacts must also be present
    assert!(
        entry_paths.iter().any(|p| p == "inspection-snapshot.json"),
        "export must contain inspection-snapshot.json",
    );
    assert!(
        entry_paths.iter().any(|p| p == "Containerfile"),
        "export must contain Containerfile",
    );

    // -----------------------------------------------------------------------
    // 11. Verify snapshot still has the selected variant from step 5
    // -----------------------------------------------------------------------
    let proj = session.snapshot_projected();
    let main_variants = variants_for_path(&proj, "/etc/app/main.conf");
    // beta remains Selected even after undo+export cycle
    let beta = main_variants
        .iter()
        .find(|v| v.content == "setting=beta")
        .expect("beta variant must still exist");
    assert_eq!(beta.variant_selection, VariantSelection::Selected);
}

// ===========================================================================
// R4c: variant_summary distribution must be sorted (BTreeMap)
// ===========================================================================

#[test]
fn variant_summary_distribution_is_sorted() {
    let snap = make_e2e_snapshot();
    let session = RefineSession::new(snap);
    let summary = variant_summary(&session.snapshot_projected(), session.fleet_context())
        .expect("fleet session must produce a summary");

    // BTreeMap keys are always sorted — verify this contract
    let keys: Vec<_> = summary.variant_distribution.keys().cloned().collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(
        keys, sorted_keys,
        "variant_distribution keys must be in sorted order"
    );

    // Also verify host_split uses usize (no negative values possible)
    for (path, info) in &summary.variant_distribution {
        for &count in &info.host_split {
            // This is a compile-time check really — if host_split is Vec<usize>,
            // negative values are impossible. But we verify the values are sensible.
            assert!(
                count <= 5,
                "host count for {path} must be <= total hosts (5), got {count}"
            );
        }
    }
}
