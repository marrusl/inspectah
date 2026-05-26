use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::TriageBucket;

#[test]
fn test_source_repo_proof_rust_collector_path() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "epel-release".into(),
                arch: "noarch".into(),
                state: PackageState::Added,
                source_repo: "epel".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "local-pkg".into(),
                arch: "x86_64".into(),
                state: PackageState::LocalInstall,
                source_repo: "".into(),
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: std::collections::HashMap::new(),
        extracted_at: "2026-05-17T00:00:00Z".into(),
    });

    let session = RefineSession::new(snap);
    let view = session.view();

    let httpd = view.packages.iter().find(|p| p.entry.name == "httpd").unwrap();
    assert_eq!(httpd.triage.bucket(), TriageBucket::Site);
    assert!(!httpd.entry.source_repo.is_empty());

    let local = view.packages.iter().find(|p| p.entry.name == "local-pkg").unwrap();
    assert_eq!(local.triage.bucket(), TriageBucket::Investigate);
}
