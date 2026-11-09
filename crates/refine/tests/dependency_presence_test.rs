//! A package something included depends on is in the image, whatever its
//! own disposition: dnf installs it with the package that needs it. So
//! the unit it owns is enabled on both export paths, when refine leaves it
//! excluded by its non-leaf default and when the operator excludes it
//! outright. Only when nothing included pulls it in is the unit omitted.

use std::collections::HashMap;

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

const UNIT: &str = "cockpit.socket";
const ENABLED: &str = "    cockpit.socket";
const OMITTED: &str = "# Omitted: cockpit.socket (package 'cockpit-ws' was excluded)";

fn package(name: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        version: "1".into(),
        release: "1.el9".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        source_repo: "appstream".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// The leaf `cockpit` and its dependency `cockpit-ws`, which owns
/// `cockpit.socket`. The baseline ships neither, so the classifier puts
/// both in Site and refine excludes the non-leaf one by default.
fn snapshot() -> InspectionSnapshot {
    let mut packages = HashMap::new();
    packages.insert(
        "bash.x86_64".to_string(),
        BaselinePackageEntry {
            name: "bash".into(),
            epoch: Some("0".into()),
            version: "5.2".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
        },
    );
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![package("cockpit"), package("cockpit-ws")],
        leaf_packages: Some(vec!["cockpit.x86_64".into()]),
        auto_packages: Some(vec!["cockpit-ws.x86_64".into()]),
        leaf_dep_tree: serde_json::json!({ "cockpit.x86_64": ["cockpit-ws.x86_64"] }),
        ..Default::default()
    });
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages,
        extracted_at: "2026-11-08T00:00:00Z".into(),
    });
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: UNIT.into(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            disposition: FindingKind::included(),
            locked: false,
            owning_package: Some("cockpit-ws".into()),
            aggregate: None,
            attention_reason: None,
            shadow_type: None,
            shadow_rationale: None,
        }],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    snap
}

fn scan_containerfile(snap: &InspectionSnapshot) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(snap, &RenderContext { target: None }, dir.path()).expect("render");
    std::fs::read_to_string(dir.path().join("Containerfile")).expect("read")
}

fn export_containerfile(session: &RefineSession) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let tarball = dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    let file = std::fs::File::open(&tarball).expect("open tarball");
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let path = entry.path().expect("path").to_string_lossy().to_string();
        if path.ends_with("/Containerfile") || path == "Containerfile" {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).expect("read");
            return content;
        }
    }
    panic!("no Containerfile in the export tarball");
}

fn set_include(session: &mut RefineSession, name: &str, include: bool) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: name.into(),
                arch: "x86_64".into(),
            },
            include,
        })
        .unwrap_or_else(|e| panic!("{name} can be toggled: {e:?}"));
}

fn cockpit_ws_included(session: &RefineSession) -> bool {
    session
        .snapshot_projected()
        .rpm
        .expect("rpm section")
        .packages_added
        .iter()
        .find(|p| p.name == "cockpit-ws")
        .expect("cockpit-ws")
        .disposition
        .is_included()
}

#[test]
fn a_dependency_of_an_included_leaf_keeps_its_unit_on_both_paths() {
    let scan = scan_containerfile(&snapshot());
    assert!(scan.lines().any(|l| l == ENABLED), "scan tarball:\n{scan}");

    let session = RefineSession::new(snapshot());
    assert!(
        !cockpit_ws_included(&session),
        "refine starts the non-leaf dependency excluded, so this case is the default one"
    );
    let refine = export_containerfile(&session);
    assert!(
        refine.lines().any(|l| l == ENABLED) && !refine.contains(OMITTED),
        "unchanged refine export:\n{refine}"
    );
}

#[test]
fn an_explicitly_excluded_dependency_of_an_included_leaf_keeps_its_unit() {
    let mut session = RefineSession::new(snapshot());
    set_include(&mut session, "cockpit-ws", true);
    set_include(&mut session, "cockpit-ws", false);
    let refine = export_containerfile(&session);
    assert!(
        refine.lines().any(|l| l == ENABLED) && !refine.contains(OMITTED),
        "dnf installs cockpit-ws with cockpit, so its unit stays:\n{refine}"
    );
}

#[test]
fn a_dependency_whose_parent_is_excluded_too_loses_its_unit() {
    let mut session = RefineSession::new(snapshot());
    set_include(&mut session, "cockpit", false);
    let refine = export_containerfile(&session);
    assert!(
        refine.contains(OMITTED) && !refine.lines().any(|l| l == ENABLED),
        "nothing included pulls cockpit-ws in:\n{refine}"
    );
}
