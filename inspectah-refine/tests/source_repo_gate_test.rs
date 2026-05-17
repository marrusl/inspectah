use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::types::rpm::{RpmSection, PackageEntry, PackageState};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::AttentionLevel;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_source_repo_populated_in_golden_data() {
    // Path is relative to workspace root when running cargo test
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up from inspectah-refine to workspace root
    path.push("testdata/golden/go-v13-rpm-section.json");
    let content = fs::read_to_string(&path)
        .expect("golden test data must exist");
    let rpm: RpmSection = serde_json::from_str(&content)
        .expect("golden data must deserialize");
    let packages_with_repo: Vec<_> = rpm.packages_added.iter()
        .filter(|p| !p.source_repo.is_empty()).collect();
    assert!(!packages_with_repo.is_empty(),
        "golden data must have packages with non-empty source_repo");
    let known_repos: Vec<_> = packages_with_repo.iter()
        .map(|p| p.source_repo.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    assert!(known_repos.len() >= 2,
        "golden data should have multiple distinct repos, got: {:?}", known_repos);
}

#[test]
fn test_source_repo_proof_rust_collector_path() {
    // Build a snapshot that simulates what the Rust collector now produces
    // (packages with populated source_repo values)
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(), arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(), // Rust collector now populates this
                include: true, ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(), arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "epel".into(),
                include: true, ..Default::default()
            },
            PackageEntry {
                name: "local-pkg".into(), arch: "x86_64".into(),
                state: PackageState::LocalInstall,
                source_repo: "".into(), // Correctly empty for local installs
                include: true, ..Default::default()
            },
        ],
        ..Default::default()
    });
    // Empty baseline — presence puts us in verified mode
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    });

    // Verify the refine session correctly classifies based on source_repo
    let session = RefineSession::new(snap);
    let view = session.view();

    // Packages with source_repo and baseline present should be Routine (Tier 1)
    // — user-added from recognized repos are auto-included.
    let httpd = view.packages.iter().find(|p| p.entry.name == "httpd").unwrap();
    assert_eq!(httpd.attention[0].level, AttentionLevel::Routine);
    assert!(!httpd.entry.source_repo.is_empty());

    // Local install with empty source_repo should be NeedsReview (Tier 3)
    let local = view.packages.iter().find(|p| p.entry.name == "local-pkg").unwrap();
    assert_eq!(local.attention[0].level, AttentionLevel::NeedsReview);
}
