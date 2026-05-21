use std::collections::BTreeSet;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
use inspectah_core::types::fleet::{FleetPrevalence, FleetSnapshotMeta, VariantSelection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::types::services::{ServiceSection, SystemdDropIn};
use inspectah_refine::session::{render_refine_export, RefineSession};
use inspectah_refine::types::{ContentHash, ItemId, RefinementOp};

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
        .filter(|f| f.contains("etc_httpd_conf_httpd.conf"))
        .collect();
    assert!(
        !httpd_variants.is_empty(),
        "expected variant file for /etc/httpd/conf/httpd.conf, got: {variant_files:?}"
    );

    let sysctl_variants: Vec<_> = variant_files
        .iter()
        .filter(|f| f.contains("etc_sysctl.conf"))
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
        .find(|f| f.starts_with("fleet/variants/etc_httpd_conf_httpd.conf/"))
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
        .find(|f| f.starts_with("fleet/variants/etc_sysctl.conf/"))
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

// ===========================================================================
// DropIn/Quadlet export tests
// ===========================================================================

/// Build a fleet snapshot with an Alternative drop-in variant.
fn fleet_snapshot_with_dropin_variants() -> InspectionSnapshot {
    let mut snap = single_host_snapshot();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test-fleet".into(),
        host_count: 5,
        hostnames: (0..5).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.services = Some(ServiceSection {
        drop_ins: vec![
            SystemdDropIn {
                unit: "httpd.service".into(),
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                content: "[Service]\nTimeoutStartSec=90".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["host-0".into(), "host-1".into(), "host-2".into()],
                }),
            },
            SystemdDropIn {
                unit: "httpd.service".into(),
                path: "/etc/systemd/system/httpd.service.d/override.conf".into(),
                content: "[Service]\nTimeoutStartSec=120".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: 5,
                    hosts: vec!["host-3".into(), "host-4".into()],
                }),
            },
        ],
        ..Default::default()
    });
    snap
}

/// Build a fleet snapshot with an Alternative quadlet variant.
fn fleet_snapshot_with_quadlet_variants() -> InspectionSnapshot {
    let mut snap = single_host_snapshot();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "test-fleet".into(),
        host_count: 5,
        hostnames: (0..5).map(|i| format!("host-{i}")).collect(),
        merged_at: "2026-05-21T00:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: Default::default(),
    });
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![
            QuadletUnit {
                path: "/etc/containers/systemd/app.container".into(),
                name: "app.container".into(),
                content: "[Container]\nImage=quay.io/app:v1".into(),
                image: "quay.io/app:v1".into(),
                include: true,
                variant_selection: VariantSelection::Selected,
                fleet: Some(FleetPrevalence {
                    count: 3,
                    total: 5,
                    hosts: vec!["host-0".into(), "host-1".into(), "host-2".into()],
                }),
                ..Default::default()
            },
            QuadletUnit {
                path: "/etc/containers/systemd/app.container".into(),
                name: "app.container".into(),
                content: "[Container]\nImage=quay.io/app:v2".into(),
                image: "quay.io/app:v2".into(),
                include: true,
                variant_selection: VariantSelection::Alternative,
                fleet: Some(FleetPrevalence {
                    count: 2,
                    total: 5,
                    hosts: vec!["host-3".into(), "host-4".into()],
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap
}

#[test]
fn export_includes_dropin_alternative_variants() {
    let snap = fleet_snapshot_with_dropin_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    let variant_files: Vec<_> = files
        .iter()
        .filter(|f| f.starts_with("fleet/variants/"))
        .collect();

    // Should have a variant file for the drop-in Alternative
    let dropin_variants: Vec<_> = variant_files
        .iter()
        .filter(|f| f.contains("etc_systemd_system_httpd.service.d_override.conf"))
        .collect();
    assert!(
        !dropin_variants.is_empty(),
        "expected variant file for drop-in Alternative, got: {variant_files:?}"
    );

    // Verify content
    let content = tarball_read_file(&tarball_path, dropin_variants[0]).unwrap();
    assert_eq!(
        content, "[Service]\nTimeoutStartSec=120",
        "drop-in variant file must contain the Alternative content"
    );
}

#[test]
fn export_includes_quadlet_alternative_variants() {
    let snap = fleet_snapshot_with_quadlet_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    let variant_files: Vec<_> = files
        .iter()
        .filter(|f| f.starts_with("fleet/variants/"))
        .collect();

    // Should have a variant file for the quadlet Alternative
    let quadlet_variants: Vec<_> = variant_files
        .iter()
        .filter(|f| f.contains("etc_containers_systemd_app.container"))
        .collect();
    assert!(
        !quadlet_variants.is_empty(),
        "expected variant file for quadlet Alternative, got: {variant_files:?}"
    );

    // Verify content
    let content = tarball_read_file(&tarball_path, quadlet_variants[0]).unwrap();
    assert_eq!(
        content, "[Container]\nImage=quay.io/app:v2",
        "quadlet variant file must contain the Alternative content"
    );
}

// ===========================================================================
// R4b: Export path layout fix — no leading underscores
// ===========================================================================

#[test]
fn export_variant_paths_no_leading_underscore() {
    let snap = fleet_snapshot_with_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    let variant_files: Vec<_> = files
        .iter()
        .filter(|f| f.starts_with("fleet/variants/"))
        .collect();

    assert!(
        !variant_files.is_empty(),
        "must have variant files to test"
    );

    for file in &variant_files {
        // Extract the escaped-path directory name (between fleet/variants/ and the filename)
        let after_prefix = file.strip_prefix("fleet/variants/").unwrap();
        assert!(
            !after_prefix.starts_with('_'),
            "variant path must NOT start with underscore (leading / not escaped): {file}"
        );
    }
}

#[test]
fn export_dropin_variant_paths_no_leading_underscore() {
    let snap = fleet_snapshot_with_dropin_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    for file in files.iter().filter(|f| f.starts_with("fleet/variants/")) {
        let after_prefix = file.strip_prefix("fleet/variants/").unwrap();
        assert!(
            !after_prefix.starts_with('_'),
            "drop-in variant path must NOT start with underscore: {file}"
        );
    }
}

#[test]
fn export_quadlet_variant_paths_no_leading_underscore() {
    let snap = fleet_snapshot_with_quadlet_variants();
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");

    render_refine_export(&snap, &tarball_path).unwrap();

    let files = tarball_file_set(&tarball_path);
    for file in files.iter().filter(|f| f.starts_with("fleet/variants/")) {
        let after_prefix = file.strip_prefix("fleet/variants/").unwrap();
        assert!(
            !after_prefix.starts_with('_'),
            "quadlet variant path must NOT start with underscore: {file}"
        );
    }
}

// ===========================================================================
// R4d: Export-reimport round-trip test
// ===========================================================================

#[test]
fn export_reimport_preserves_variant_state() {
    let snap = fleet_snapshot_with_variants();

    // Apply a SelectVariant op — switch /etc/httpd/conf/httpd.conf to Alternative
    let alt_content = "ServerRoot /etc/httpd\nMaxClients 128";
    let alt_hash = ContentHash::from_content(alt_content.as_bytes());

    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SelectVariant {
            item_id: ItemId::Config {
                path: "/etc/httpd/conf/httpd.conf".into(),
            },
            target: alt_hash,
        })
        .unwrap();

    // Export
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    // Read the exported tarball and parse inspection-snapshot.json
    let snap_json = tarball_read_file(&tarball_path, "inspection-snapshot.json")
        .expect("exported tarball must contain inspection-snapshot.json");
    let reimported: InspectionSnapshot =
        serde_json::from_str(&snap_json).expect("inspection-snapshot.json must parse");

    // Verify the reimported snapshot's config files have correct variant selections
    let config = reimported
        .config
        .as_ref()
        .expect("reimported snapshot must have config section");

    let httpd_entries: Vec<_> = config
        .files
        .iter()
        .filter(|e| e.path == "/etc/httpd/conf/httpd.conf")
        .collect();

    // After SelectVariant, the previously-Alternative entry should now be Selected
    let selected_entries: Vec<_> = httpd_entries
        .iter()
        .filter(|e| e.variant_selection == VariantSelection::Selected)
        .collect();
    assert_eq!(
        selected_entries.len(),
        1,
        "reimported snapshot must have exactly one Selected entry for httpd.conf, got {}",
        selected_entries.len()
    );
    assert_eq!(
        selected_entries[0].content, alt_content,
        "the Selected entry must contain the Alternative content we selected"
    );
}
