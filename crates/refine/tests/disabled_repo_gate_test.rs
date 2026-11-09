//! A package installed from a repo the host has disabled, whose `.repo`
//! file the export carries, is not of unknown origin. Refine's repo-less
//! trust gate leaves it at the classifier's default, so an export made
//! with no changes installs it the way the scan tarball does: from that
//! repo, with `--enablerepo`.

use std::collections::HashMap;

use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::FindingKind;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RepoFile, RpmSection};
use inspectah_core::types::services::{
    PresetDefault, ServiceSection, ServiceStateChange, ServiceUnitState,
};
use inspectah_pipeline::render;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

const REPO_ID: &str = "epel-cisco-openh264";
const REPO_PATH: &str = "/etc/yum.repos.d/fedora-cisco-openh264.repo";
const DISABLED_WITH_SOURCE: &str = "[epel-cisco-openh264]\n\
     name=openh264\n\
     metalink=https://example.invalid/metalink\n\
     enabled=0\n";
const DISABLED_NO_SOURCE: &str = "[epel-cisco-openh264]\nname=openh264\nenabled=0\n";
const ENABLEREPO: &str = "--enablerepo=epel-cisco-openh264";
const INSTALL_LINE: &str = "    openh264 \\";

/// A baseline that does not contain `openh264`, so the classifier puts the
/// package in Site and, with no leaf data, includes it.
fn baseline() -> BaselineData {
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
    BaselineData {
        image_digest: "sha256:test".into(),
        packages,
        extracted_at: "2026-11-08T00:00:00Z".into(),
    }
}

/// A scan-shaped snapshot: `openh264` recorded from `source_repo`, marked
/// repo-less by the scan, included as the collector leaves it.
fn snapshot(source_repo: &str, repo_file: Option<(&str, bool)>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "openh264".into(),
            version: "2.4.1".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: source_repo.into(),
            disposition: FindingKind::included(),
            repoless_annotation: format!(
                "No repo source: repo '{source_repo}' not in enabled repos, manual resolution needed"
            ),
            ..Default::default()
        }],
        repo_files: repo_file
            .map(|(content, included)| {
                vec![RepoFile {
                    path: REPO_PATH.into(),
                    content: content.into(),
                    is_default_repo: false,
                    disposition: FindingKind::from_bool(included),
                    locked: false,
                    aggregate: None,
                }]
            })
            .unwrap_or_default(),
        ..Default::default()
    });
    snap.baseline = Some(baseline());
    snap
}

fn openh264_included(session: &RefineSession) -> bool {
    session
        .snapshot_projected()
        .rpm
        .expect("rpm section")
        .packages_added
        .iter()
        .find(|p| p.name == "openh264")
        .expect("openh264")
        .disposition
        .is_included()
}

/// The gate exempts exactly the packages the export can install from their
/// own repo; everything else stays behind it.
#[test]
fn the_trust_gate_exempts_only_a_shipped_disabled_repo() {
    let cases = [
        (
            "disabled repo shipped with a source",
            snapshot(REPO_ID, Some((DISABLED_WITH_SOURCE, true))),
            true,
        ),
        (
            "@commandline",
            snapshot("@commandline", Some((DISABLED_WITH_SOURCE, true))),
            false,
        ),
        ("repo file absent", snapshot(REPO_ID, None), false),
        (
            "repo file excluded",
            snapshot(REPO_ID, Some((DISABLED_WITH_SOURCE, false))),
            false,
        ),
        (
            "disabled section with no source",
            snapshot(REPO_ID, Some((DISABLED_NO_SOURCE, true))),
            false,
        ),
        (
            "deleted repo: the file names a different repo",
            snapshot("internal-tools", Some((DISABLED_WITH_SOURCE, true))),
            false,
        ),
    ];
    for (name, snap, expected) in cases {
        let session = RefineSession::new(snap);
        assert_eq!(openh264_included(&session), expected, "{name}");
    }
}

/// The scan tarball and a refine export with no changes render this
/// package the same way. Scoped to the package: the other known
/// differences between the two paths are tracked separately.
#[test]
fn scan_tarball_and_unchanged_refine_export_agree_on_the_package() {
    let snap = snapshot(REPO_ID, Some((DISABLED_WITH_SOURCE, true)));
    let about_the_package = |containerfile: &str| -> Vec<String> {
        containerfile
            .lines()
            .filter(|l| l.contains("openh264") || l.contains("disabled on the source host"))
            .map(str::to_string)
            .collect()
    };

    let scan_dir = tempfile::tempdir().expect("tempdir");
    render::render_all(&snap, &RenderContext { target: None }, scan_dir.path()).expect("render");
    let scan = std::fs::read_to_string(scan_dir.path().join("Containerfile")).expect("read");

    let session = RefineSession::new(snap);
    let export_dir = tempfile::tempdir().expect("tempdir");
    let tarball = export_dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    let refine = extract_containerfile(&tarball);

    let scan_lines = about_the_package(&scan);
    assert!(
        scan_lines.iter().any(|l| l.contains(ENABLEREPO)),
        "the scan tarball installs it from its repo:\n{scan}"
    );
    assert_eq!(
        scan_lines,
        about_the_package(&refine),
        "scan:\n{scan}\nrefine:\n{refine}"
    );
}

/// Render a snapshot the way the scan tarball does, and return its
/// Containerfile.
fn scan_containerfile(snap: &InspectionSnapshot) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(snap, &RenderContext { target: None }, dir.path()).expect("render");
    std::fs::read_to_string(dir.path().join("Containerfile")).expect("read")
}

/// Export a session and return the export's Containerfile.
fn export_containerfile(session: &RefineSession) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let tarball = dir.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    extract_containerfile(&tarball)
}

/// With leaf data, the disabled-repo transaction takes the plain list's
/// selection. The leaf `gst-openh264` installs from its repo; its
/// dependency `openh264`, from the same repo, is left to dnf, which
/// resolves it inside that transaction. The scan tarball includes the
/// dependency and filters it at render; refine excludes it by default.
/// Both must render the same lines, including the unit each package owns,
/// and when refine excludes the leaf too, nothing installs and neither
/// unit is enabled.
#[test]
fn a_non_leaf_disabled_repo_package_renders_the_same_on_both_paths() {
    const LEAF: &str = "gst-openh264";
    const UNIT: &str = "openh264-helper.service";
    const DEP_UNIT: &str = "openh264-dep.service";
    let mut snap = snapshot(REPO_ID, Some((DISABLED_WITH_SOURCE, true)));
    {
        let rpm = snap.rpm.as_mut().expect("rpm section");
        let mut leaf = rpm.packages_added[0].clone();
        leaf.name = LEAF.into();
        rpm.packages_added.push(leaf);
        rpm.leaf_packages = Some(vec![format!("{LEAF}.x86_64")]);
        rpm.leaf_dep_tree = serde_json::json!({ "gst-openh264.x86_64": ["openh264.x86_64"] });
    }
    let unit = |name: &str, owner: &str| ServiceStateChange {
        unit: name.into(),
        current_state: ServiceUnitState::Enabled,
        default_state: Some(PresetDefault::Disable),
        disposition: FindingKind::included(),
        locked: false,
        owning_package: Some(owner.into()),
        aggregate: None,
        attention_reason: None,
        shadow_type: None,
        shadow_rationale: None,
    };
    snap.services = Some(ServiceSection {
        // The dependency owns a unit too. dnf installs it with the leaf,
        // so its unit is enabled although refine excludes it by default.
        state_changes: vec![unit(UNIT, LEAF), unit(DEP_UNIT, "openh264")],
        enabled_units: vec![],
        disabled_units: vec![],
        drop_ins: vec![],
        preset_matched_units: vec![],
    });
    let about_the_packages = |containerfile: &str| -> Vec<String> {
        containerfile
            .lines()
            .filter(|l| {
                l.contains("openh264")
                    || l.contains("disabled on the source host")
                    || l.contains(UNIT)
            })
            .map(str::to_string)
            .collect()
    };

    let scan = scan_containerfile(&snap);
    assert!(
        scan.lines().any(|l| l == "    gst-openh264 \\"),
        "the leaf installs:\n{scan}"
    );
    assert!(
        !scan.lines().any(|l| l == INSTALL_LINE),
        "the dependency is left to dnf, as the plain list leaves one:\n{scan}"
    );
    assert!(
        !scan.contains("# MANUAL: openh264"),
        "and it is not offered for upload:\n{scan}"
    );

    let mut session = RefineSession::new(snap);
    let refine = export_containerfile(&session);
    assert_eq!(
        about_the_packages(&scan),
        about_the_packages(&refine),
        "scan:\n{scan}\nrefine:\n{refine}"
    );
    for (u, whose) in [(UNIT, "the leaf's"), (DEP_UNIT, "the dependency's")] {
        for (path, containerfile) in [("scan", &scan), ("refine", &refine)] {
            assert!(
                containerfile
                    .lines()
                    .any(|l| l.contains(u) && !l.starts_with('#')),
                "{path}: {whose} unit is enabled:\n{containerfile}"
            );
        }
    }

    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: LEAF.into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .expect("the leaf can be excluded");
    let excluded = export_containerfile(&session);
    assert!(
        !excluded.contains(ENABLEREPO),
        "nothing installs from the repo:\n{excluded}"
    );
    for u in [UNIT, DEP_UNIT] {
        assert!(
            !excluded
                .lines()
                .any(|l| l.contains(u) && !l.starts_with('#')),
            "no unit of an uninstalled package is enabled ({u}):\n{excluded}"
        );
    }
}

/// Refine refuses to toggle a disabled repo, so through the session the
/// file cannot be excluded after the gate let the package through. If it
/// is excluded anyway (a resumed or hand-edited snapshot), the package,
/// still included, falls back to the repo-less block: no flag and no
/// plain install, so the build does not fail on it.
#[test]
fn a_repo_file_excluded_after_the_gate_falls_back_to_repoless() {
    let mut session = RefineSession::new(snapshot(REPO_ID, Some((DISABLED_WITH_SOURCE, true))));
    assert!(openh264_included(&session), "the gate let it through");
    assert!(
        session
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Repo {
                    path: REPO_ID.into(),
                },
                include: false,
            })
            .is_err(),
        "refine refuses to toggle a disabled repo today; if that changes, \
         this test should drive the exclusion through the op instead"
    );

    let mut projected = session.snapshot_projected();
    projected
        .rpm
        .as_mut()
        .expect("rpm section")
        .repo_files
        .iter_mut()
        .for_each(|f| f.disposition = FindingKind::excluded());
    let containerfile = render_containerfile(&projected, None, None);

    assert!(
        !containerfile.contains("--enablerepo"),
        "no flag for a repo the export no longer carries:\n{containerfile}"
    );
    assert!(
        !containerfile.lines().any(|l| l == INSTALL_LINE),
        "no install by name, which would fail with `Unable to find a match`:\n{containerfile}"
    );
    assert!(
        containerfile.contains("# MANUAL: openh264"),
        "the repo-less block carries it:\n{containerfile}"
    );
}

/// The Containerfile inside an export tarball.
fn extract_containerfile(tarball: &std::path::Path) -> String {
    let file = std::fs::File::open(tarball).expect("open tarball");
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
