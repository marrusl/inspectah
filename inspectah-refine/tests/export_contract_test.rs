use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{PackageTarget, RefinementOp};
use std::collections::BTreeSet;

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }],
    });
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    });
    snap
}

/// Collect all file entries from a tarball as a sorted set of paths.
/// Directories are excluded — only regular file paths.
fn tarball_file_set(tarball_path: &std::path::Path) -> BTreeSet<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    let mut files = BTreeSet::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        if entry.header().entry_type() == tar::EntryType::Regular {
            let path = entry.path().unwrap().to_string_lossy().to_string();
            files.insert(path);
        }
    }
    files
}

#[test]
fn export_exact_file_set() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let actual = tarball_file_set(&tarball_path);

    // Build the EXACT expected set for this fixture.
    // The test snapshot has one included config file at
    // /etc/httpd/conf/httpd.conf, so config/ tree is populated.
    let expected: BTreeSet<String> = [
        "inspection-snapshot.json",
        "Containerfile",
        "audit-report.md",
        "schema/snapshot.schema.json",
        // config tree materialized from the included config file:
        "config/etc/httpd/conf/httpd.conf",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // Full equality — not subset, not superset.
    // Any missing file, any extra file, any wrong path = failure.
    let missing: BTreeSet<_> = expected.difference(&actual).collect();
    let extra: BTreeSet<_> = actual.difference(&expected).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "export contract violated!\n  missing: {missing:?}\n  extra: {extra:?}\n  expected: {expected:?}\n  actual: {actual:?}"
    );
}

#[test]
fn export_snapshot_reflects_refinements() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    // Extract and verify
    let extract_dir = tempdir.path().join("extract");
    std::fs::create_dir(&extract_dir).unwrap();
    let file = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&extract_dir).unwrap();

    let snap_path = walkdir::WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "inspection-snapshot.json")
        .expect("snapshot file must exist");

    let snap_json = std::fs::read_to_string(snap_path.path()).unwrap();
    let snap: InspectionSnapshot = serde_json::from_str(&snap_json).unwrap();

    let httpd = snap
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap();
    assert!(!httpd.include, "httpd must be excluded in exported snapshot");

    let vim = snap
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "vim")
        .unwrap();
    assert!(vim.include, "vim must remain included");
}

#[test]
fn preview_export_containerfile_fidelity() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    // Capture the preview Containerfile
    let preview = session.view().containerfile_preview.clone();

    // Export and extract the Containerfile
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let extract_dir = tempdir.path().join("extract");
    std::fs::create_dir(&extract_dir).unwrap();
    let file = std::fs::File::open(&tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(&extract_dir).unwrap();

    let cf_path = walkdir::WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "Containerfile")
        .expect("Containerfile must exist in export");

    let exported = std::fs::read_to_string(cf_path.path()).unwrap();

    assert_eq!(
        preview, exported,
        "preview and exported Containerfile must be byte-identical"
    );
}

#[test]
fn reimport_preserves_excludes() {
    // First session: exclude httpd, export
    let mut session1 = RefineSession::new(test_snapshot());
    session1
        .apply(RefinementOp::ExcludePackage(PackageTarget {
            name: "httpd".into(),
            arch: "x86_64".into(),
        }))
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("export1.tar.gz");
    session1
        .export_tarball(&tarball_path, session1.generation())
        .unwrap();

    // Second session: re-import the exported tarball
    let session2 = inspectah_refine::tarball::from_tarball(&tarball_path).unwrap();

    // httpd must still be excluded in the re-imported session
    let httpd = session2
        .view()
        .packages
        .iter()
        .find(|p| p.entry.name == "httpd")
        .unwrap();
    assert!(
        !httpd.entry.include,
        "httpd must remain excluded after re-import"
    );

    // The re-imported session should NOT be dirty — the exclude is
    // part of the normalized original, not a new mutation
    assert!(
        !session2.is_dirty(),
        "re-imported session must not be dirty"
    );
}

#[test]
fn export_excludes_extra_config_tree_artifacts() {
    let mut snap = test_snapshot();
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            name: "myapp.container".into(),
            content: "[Container]\nImage=registry.example.com/myapp:latest\n".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session.export_tarball(&tarball_path, session.generation()).unwrap();

    let files = tarball_file_set(&tarball_path);

    // quadlet/ must NOT appear in the export
    assert!(
        !files.iter().any(|f| f.starts_with("quadlet/")),
        "quadlet/ must not be in refine export, got: {files:?}"
    );
}
