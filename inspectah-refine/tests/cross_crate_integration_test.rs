//! Cross-crate integration tests — round-trip contract proofs.
//!
//! These tests exercise the contracts between inspectah-core, inspectah-pipeline,
//! and inspectah-refine to prove that baseline data flows correctly across crate
//! boundaries.

use std::collections::HashMap;

use inspectah_core::baseline::{
    BaselineData, BaselinePackageEntry, ResolutionStrategy, TargetImageIdentity,
};
use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{ServiceSection, ServiceStateChange};
use inspectah_pipeline::render::containerfile::{base_image_from_snapshot, render_containerfile};
use inspectah_refine::normalize::load_for_refine;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn snapshot_with_full_baseline() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "registry.redhat.io/rhel9/rhel-bootc:9.6".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    });

    let mut packages = HashMap::new();
    packages.insert(
        "bash".to_string(),
        BaselinePackageEntry {
            name: "bash".to_string(),
            epoch: Some("0".to_string()),
            version: "5.2.26".to_string(),
            release: "4.el9".to_string(),
            arch: "x86_64".to_string(),
        },
    );
    packages.insert(
        "systemd".to_string(),
        BaselinePackageEntry {
            name: "systemd".to_string(),
            epoch: None,
            version: "256.7".to_string(),
            release: "1.el9".to_string(),
            arch: "x86_64".to_string(),
        },
    );

    snap.baseline = Some(BaselineData {
        image_digest: "sha256:deadbeef1234".to_string(),
        packages,
        extracted_at: "2026-05-17T00:00:00Z".to_string(),
    });
    snap.no_baseline = false;

    snap.rpm = Some(RpmSection {
        baseline_package_names: Some(vec!["bash".into(), "systemd".into()]),
        packages_added: vec![
            PackageEntry {
                name: "bash".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "baseos".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    snap
}

// ---------------------------------------------------------------------------
// Step 1: Snapshot round-trip with NEVRA
// ---------------------------------------------------------------------------

#[test]
fn snapshot_roundtrip_with_nevra_baseline() {
    let snap = snapshot_with_full_baseline();

    // Serialize -> deserialize
    let json = serde_json::to_string_pretty(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

    // target_image intact
    assert!(parsed.target_image.is_some());
    let ti = parsed.target_image.as_ref().unwrap();
    assert_eq!(ti.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
    assert_eq!(ti.strategy, ResolutionStrategy::OsRelease);

    // baseline packages HashMap intact
    assert!(parsed.baseline.is_some());
    let bl = parsed.baseline.as_ref().unwrap();
    assert_eq!(bl.packages.len(), 2);

    let bash = &bl.packages["bash"];
    assert_eq!(bash.name, "bash");
    assert_eq!(bash.epoch, Some("0".to_string()));
    assert_eq!(bash.version, "5.2.26");
    assert_eq!(bash.release, "4.el9");
    assert_eq!(bash.arch, "x86_64");

    let systemd = &bl.packages["systemd"];
    assert_eq!(systemd.name, "systemd");
    assert_eq!(systemd.epoch, None);
    assert_eq!(systemd.version, "256.7");

    // no_baseline flag
    assert!(!parsed.no_baseline);

    // schema_version
    assert_eq!(parsed.schema_version, SCHEMA_VERSION);
}

// ---------------------------------------------------------------------------
// Step 2: Degraded FROM persistence — target_image present
// ---------------------------------------------------------------------------

#[test]
fn degraded_target_image_present_cross_crate() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: "quay.io/fedora/fedora-bootc:41".to_string(),
        strategy: ResolutionStrategy::OsRelease,
    });
    snap.baseline = None;
    snap.no_baseline = true;

    // Round-trip
    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

    // Cross-crate call: pipeline's base_image_from_snapshot
    let base = base_image_from_snapshot(&parsed);
    assert!(
        base.is_some(),
        "degraded with target_image must return Some"
    );
    assert_eq!(base.unwrap(), "quay.io/fedora/fedora-bootc:41");
}

// ---------------------------------------------------------------------------
// Step 3: Degraded FROM persistence — target_image null
// ---------------------------------------------------------------------------

#[test]
fn degraded_target_image_null_cross_crate() {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = None;
    snap.baseline = None;
    snap.no_baseline = true;

    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();

    // Cross-crate call: pipeline's base_image_from_snapshot
    let base = base_image_from_snapshot(&parsed);
    assert!(
        base.is_none(),
        "degraded without target_image must return None"
    );
}

// ---------------------------------------------------------------------------
// Step 4: Old schema versions rejected
// ---------------------------------------------------------------------------

#[test]
fn old_schema_version_rejected() {
    let json = r#"{
        "schema_version": 16,
        "meta": {},
        "system_type": "package-mode",
        "preflight": {"status": "ok"},
        "warnings": [],
        "redactions": []
    }"#;

    let result = InspectionSnapshot::load(json);
    assert!(result.is_err(), "old schema versions must be rejected");
}

// ---------------------------------------------------------------------------
// Step 5: Service surface agreement — normalize + render
// ---------------------------------------------------------------------------

#[test]
fn service_surface_agreement() {
    // Build a snapshot JSON with dnf-makecache.service in services
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![
            ServiceStateChange {
                unit: "dnf-makecache.service".into(),
                current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            },
            ServiceStateChange {
                unit: "httpd.service".into(),
                current_state: inspectah_core::types::services::ServiceUnitState::Enabled,
                default_state: Some(inspectah_core::types::services::PresetDefault::Disable),
                include: true,
                owning_package: None,
                fleet: None,
                attention_reason: None,
            },
        ],
        enabled_units: vec!["dnf-makecache.service".into(), "httpd.service".into()],
        ..Default::default()
    });

    let raw_json = serde_json::to_string(&snap).unwrap();

    // Cross-crate: load_for_refine calls normalize_incompatible_services
    let normalized = load_for_refine(&raw_json).unwrap();
    let services = normalized.services.as_ref().unwrap();

    // dnf-makecache.service must have include: false in state_changes
    let dnf_sc = services
        .state_changes
        .iter()
        .find(|sc| sc.unit == "dnf-makecache.service")
        .expect("dnf-makecache.service must be in state_changes");
    assert!(
        !dnf_sc.include,
        "dnf-makecache.service must have include=false after normalization"
    );

    // httpd.service stays included
    let httpd_sc = services
        .state_changes
        .iter()
        .find(|sc| sc.unit == "httpd.service")
        .expect("httpd.service must be in state_changes");
    assert!(httpd_sc.include, "httpd.service must remain include=true");

    // dnf-makecache.service must NOT be in enabled_units
    assert!(
        !services
            .enabled_units
            .contains(&"dnf-makecache.service".to_string()),
        "dnf-makecache.service must be removed from enabled_units"
    );

    // httpd.service should remain in enabled_units
    assert!(
        services
            .enabled_units
            .contains(&"httpd.service".to_string()),
        "httpd.service must remain in enabled_units"
    );

    // Cross-crate: render_containerfile on the normalized snapshot
    let containerfile = render_containerfile(&normalized, None);

    // systemctl enable must NOT mention dnf-makecache.service
    assert!(
        !containerfile.contains("dnf-makecache.service"),
        "Containerfile must not contain dnf-makecache.service after normalization.\nContainerfile:\n{containerfile}"
    );
    // But httpd.service should be there
    assert!(
        containerfile.contains("httpd.service"),
        "Containerfile must contain httpd.service"
    );
}

// ---------------------------------------------------------------------------
// Step 6: Preview/export parity — FROM line matches target_image
// ---------------------------------------------------------------------------

#[test]
fn preview_export_from_line_parity() {
    let snap = snapshot_with_full_baseline();

    // Cross-crate: RefineSession wraps core snapshot, pipeline renders
    let session = RefineSession::new(snap);

    let projected = session.snapshot_projected();
    let containerfile = render_containerfile(&projected, None);

    // FROM line must match target_image.image_ref
    assert!(
        containerfile.contains("FROM registry.redhat.io/rhel9/rhel-bootc:9.6"),
        "Containerfile FROM line must match target_image.image_ref.\nContainerfile:\n{containerfile}"
    );

    // The preview in the cached view should also contain the FROM line
    let view = session.view();
    assert!(
        view.containerfile_preview
            .contains("FROM registry.redhat.io/rhel9/rhel-bootc:9.6"),
        "Preview Containerfile FROM line must match target_image.image_ref"
    );
}

// ---------------------------------------------------------------------------
// Step 7: BaselineSummary count stability
// ---------------------------------------------------------------------------

#[test]
fn baseline_summary_count_stability() {
    let snap = snapshot_with_full_baseline();

    let mut session = RefineSession::new(snap);

    // Get initial summary
    let summary1 = session.baseline_summary();
    assert!(
        summary1.is_some(),
        "baseline_summary must return Some when baseline is present"
    );
    let s1 = summary1.unwrap();

    // Apply an exclude operation on httpd
    let op = RefinementOp::SetInclude {
        item_id: ItemId::Package {
            name: "httpd".into(),
            arch: "x86_64".into(),
        },
        include: false,
    };
    session.apply(op).unwrap();

    // Get summary after exclude
    let summary2 = session.baseline_summary();
    assert!(summary2.is_some());
    let s2 = summary2.unwrap();

    // Counts must be identical — they reflect classification, not triage state
    assert_eq!(
        s1.baseline_count, s2.baseline_count,
        "baseline_count must be stable across include/exclude ops"
    );
    assert_eq!(
        s1.user_added_count, s2.user_added_count,
        "user_added_count must be stable across include/exclude ops"
    );
    assert_eq!(
        s1.review_count, s2.review_count,
        "review_count must be stable across include/exclude ops"
    );

    // Image ref must match
    assert_eq!(s1.image_ref, "registry.redhat.io/rhel9/rhel-bootc:9.6");
    assert_eq!(s1.image_digest, "sha256:deadbeef1234");
}
