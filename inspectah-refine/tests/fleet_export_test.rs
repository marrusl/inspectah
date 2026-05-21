use std::collections::BTreeSet;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::fleet::{FleetSnapshotMeta, VariantSelection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::session::{render_refine_export, RefineSession};

/// Build a single-host snapshot (no fleet_meta) with one config file.
fn single_host_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::default();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "httpd".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: "appstream".into(),
            include: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            content: "ServerRoot /etc/httpd".into(),
            include: true,
            variant_selection: VariantSelection::Only,
            ..Default::default()
        }],
    });
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    });
    snap
}

/// Build a fleet snapshot with Selected + Alternative config variants.
fn fleet_snapshot_with_variants() -> InspectionSnapshot {
    let mut snap = single_host_snapshot();

    // Set fleet_meta so it's recognized as a fleet snapshot
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "web-servers".into(),
        host_count: 5,
        hostnames: vec![
            "web-01".into(),
            "web-02".into(),
            "web-03".into(),
            "web-04".into(),
            "web-05".into(),
        ],
        merged_at: "2026-05-20T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });

    // Replace config with Selected + Alternative variants
    snap.config = Some(ConfigSection {
        files: vec![
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                content: "ServerRoot /etc/httpd\nMaxClients 256".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/httpd/conf/httpd.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                content: "ServerRoot /etc/httpd\nMaxClients 128".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                ..Default::default()
            },
            ConfigFileEntry {
                path: "/etc/sysctl.conf".into(),
                kind: ConfigFileKind::RpmOwnedModified,
                content: "vm.swappiness = 10".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                ..Default::default()
            },
        ],
    });

    snap
}

/// Collect all file entries from a tarball as a sorted set of paths.
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

/// Read a specific file's content from a tarball.
fn tarball_read_file(tarball_path: &std::path::Path, target: &str) -> Option<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().to_string();
        if path == target {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).unwrap();
            return Some(content);
        }
    }
    None
}

#[test]
fn single_host_export_has_no_fleet_dir() {
    let snap = single_host_snapshot();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(
        !files.iter().any(|f| f.starts_with("fleet/")),
        "single-host export must NOT contain fleet/, got: {files:?}"
    );
}

#[test]
fn fleet_export_creates_variant_files() {
    let snap = fleet_snapshot_with_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);

    // fleet/variants/ must exist with files for each Alternative entry
    let variant_files: Vec<_> = files.iter().filter(|f| f.starts_with("fleet/variants/")).collect();
    assert!(
        variant_files.len() >= 2,
        "expected at least 2 variant files (one per Alternative config), got {}: {variant_files:?}",
        variant_files.len()
    );

    // Check escaped path directories exist for both config paths
    let httpd_variants: Vec<_> = variant_files
        .iter()
        .filter(|f| f.contains("_etc_httpd_conf_httpd.conf"))
        .collect();
    assert!(
        !httpd_variants.is_empty(),
        "expected variant file for /etc/httpd/conf/httpd.conf, got: {variant_files:?}"
    );

    let sysctl_variants: Vec<_> = variant_files
        .iter()
        .filter(|f| f.contains("_etc_sysctl.conf"))
        .collect();
    assert!(
        !sysctl_variants.is_empty(),
        "expected variant file for /etc/sysctl.conf, got: {variant_files:?}"
    );
}

#[test]
fn fleet_variant_content_is_materialized() {
    let snap = fleet_snapshot_with_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);

    // Find the httpd variant file and read its content
    let httpd_variant = files
        .iter()
        .find(|f| f.starts_with("fleet/variants/_etc_httpd_conf_httpd.conf/"))
        .expect("httpd variant file must exist");

    let content = tarball_read_file(&tarball_path, httpd_variant)
        .expect("must be able to read variant file content");
    assert_eq!(
        content, "ServerRoot /etc/httpd\nMaxClients 128",
        "variant file must contain the Alternative content"
    );

    // Verify the sysctl variant content
    let sysctl_variant = files
        .iter()
        .find(|f| f.starts_with("fleet/variants/_etc_sysctl.conf/"))
        .expect("sysctl variant file must exist");

    let content = tarball_read_file(&tarball_path, sysctl_variant)
        .expect("must be able to read variant file content");
    assert_eq!(
        content, "vm.swappiness = 10",
        "sysctl variant must contain the Alternative content"
    );
}

#[test]
fn fleet_variant_file_uses_hash_prefix() {
    let snap = fleet_snapshot_with_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);

    // Every variant file must end with .content and have a 12-char hex prefix
    for file in files.iter().filter(|f| f.starts_with("fleet/variants/")) {
        let filename = file.rsplit('/').next().unwrap();
        assert!(
            filename.ends_with(".content"),
            "variant file must end with .content, got: {filename}"
        );
        let prefix = &filename[..filename.len() - ".content".len()];
        assert_eq!(
            prefix.len(),
            12,
            "hash prefix must be 12 chars, got {} chars: {prefix}",
            prefix.len()
        );
        assert!(
            prefix.chars().all(|c| c.is_ascii_hexdigit()),
            "hash prefix must be hex, got: {prefix}"
        );
    }
}

#[test]
fn fleet_export_selected_not_in_variants() {
    let snap = fleet_snapshot_with_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    let variant_files: Vec<_> = files.iter().filter(|f| f.starts_with("fleet/variants/")).collect();

    // Selected httpd variant content is "MaxClients 256" — should NOT be in variants/
    for vf in &variant_files {
        if let Some(content) = tarball_read_file(&tarball_path, vf) {
            assert!(
                !content.contains("MaxClients 256"),
                "Selected variant content must NOT appear in fleet/variants/"
            );
        }
    }
}

#[test]
fn fleet_export_via_session_export_tarball() {
    // Verify the session-level export_tarball method also produces fleet/variants/
    let snap = fleet_snapshot_with_variants();
    let session = RefineSession::new(snap);

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(
        files.iter().any(|f| f.starts_with("fleet/variants/")),
        "session export_tarball must produce fleet/variants/ for fleet snapshots"
    );
}
