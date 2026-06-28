use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, QuadletUnit};
use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::users::UserGroupSection;
use inspectah_core::util::env_hash;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, ViewDirective};
use std::collections::{BTreeSet, HashMap};

fn test_snapshot() -> InspectionSnapshot {
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
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
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
    // Baseline data is required so classify_packages treats Added +
    // known-repo packages as Site (user-added) rather than Investigate
    // (provenance unavailable). Without it, normalization sets
    // include=false on all packages.
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });
    snap
}

/// Collect all file entries from a tarball as a sorted set of paths.
/// Directories are excluded — only regular file paths.
/// The tarball prefix directory (derived from the archive filename stem)
/// is stripped so tests can assert against logical paths like
/// "Containerfile" rather than "output/Containerfile".
fn tarball_file_set(tarball_path: &std::path::Path) -> BTreeSet<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    // Derive the prefix the exporter prepends (same logic as render_refine_export).
    let stem = tarball_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let prefix = stem
        .strip_suffix(".tar.gz")
        .or_else(|| stem.strip_suffix(".tgz"))
        .unwrap_or(&stem);
    let prefix_slash = format!("{prefix}/");

    let mut files = BTreeSet::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        if entry.header().entry_type() == tar::EntryType::Regular {
            let raw = entry.path().unwrap().to_string_lossy().to_string();
            let stripped = raw.strip_prefix(&prefix_slash).unwrap_or(&raw).to_string();
            files.insert(stripped);
        }
    }
    files
}

#[test]
fn export_exact_file_set() {
    let mut session = RefineSession::new(test_snapshot());
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
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
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
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
    assert!(
        !httpd.include,
        "httpd must be excluded in exported snapshot"
    );

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
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
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
fn preview_export_containerfile_preserves_non_leaf_manual_follow_up() {
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
                name: "local-tool".into(),
                arch: "x86_64".into(),
                state: PackageState::LocalInstall,
                source_repo: String::new(),
                include: false,
                ..Default::default()
            },
            PackageEntry {
                name: "mystery".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: String::new(),
                include: false,
                ..Default::default()
            },
        ],
        leaf_packages: Some(vec!["httpd.x86_64".into()]),
        auto_packages: Some(vec!["local-tool.x86_64".into(), "mystery.x86_64".into()]),
        ..Default::default()
    });
    // Baseline required so httpd (Added + appstream) classifies as Site
    // and stays included after normalization.
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let session = RefineSession::new(snap);
    let preview = session.view().containerfile_preview.clone();

    // The renderer uses backslash continuation, so the install block
    // spans multiple lines.  Collect every line from the opening
    // "RUN dnf install -y" through the last continuation.
    let mut in_block = false;
    let mut install_block = String::new();
    for line in preview.lines() {
        if line.starts_with("RUN dnf install -y") {
            in_block = true;
        }
        if in_block {
            install_block.push_str(line);
            install_block.push('\n');
            if !line.ends_with('\\') {
                break;
            }
        }
    }
    assert!(
        !install_block.is_empty(),
        "preview must include an install block, got:\n{preview}"
    );

    assert!(
        install_block.contains("httpd") && !install_block.contains("local-tool"),
        "preview must keep install block leaf-only, got: {install_block}"
    );
    assert!(
        preview.contains("# === Manual Follow-up Required ==="),
        "preview must retain manual follow-up section, got:\n{preview}"
    );
    for package in ["local-tool", "mystery"] {
        assert!(
            preview.contains(package),
            "preview must mention {package}, got:\n{preview}"
        );
    }

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
        "preview and exported Containerfile must stay byte-identical"
    );
    assert!(
        exported.contains("# === Manual Follow-up Required ==="),
        "exported Containerfile must retain manual follow-up section, got:\n{exported}"
    );
    for package in ["local-tool", "mystery"] {
        assert!(
            exported.contains(package),
            "exported Containerfile must mention {package}, got:\n{exported}"
        );
    }
}

#[test]
fn reimport_is_clean_and_coherent() {
    // First session: exclude httpd, export
    let mut session1 = RefineSession::new(test_snapshot());
    session1
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Package {
                name: "httpd".into(),
                arch: "x86_64".into(),
            },
            include: false,
        })
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("export1.tar.gz");
    session1
        .export_tarball(&tarball_path, session1.generation())
        .unwrap();

    // Second session: re-import the exported tarball.
    // Normalization runs at construction, so include states are
    // re-evaluated based on tier classification — not preserved
    // verbatim from the export.
    let session2 = inspectah_refine::tarball::from_tarball(&tarball_path).unwrap();

    // The re-imported session should NOT be dirty — normalization
    // establishes the baseline, and there are no ops.
    assert!(
        !session2.is_dirty(),
        "re-imported session must not be dirty"
    );

    // View and projected snapshot must agree on include states
    let view_httpd = session2
        .view()
        .packages
        .iter()
        .find(|p| p.entry.name == "httpd")
        .unwrap();
    let proj_httpd = session2
        .snapshot_projected()
        .rpm
        .as_ref()
        .unwrap()
        .packages_added
        .iter()
        .find(|p| p.name == "httpd")
        .unwrap()
        .include;
    assert_eq!(
        view_httpd.entry.include, proj_httpd,
        "view and projected snapshot must agree"
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
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let files = tarball_file_set(&tarball_path);

    // quadlet/ must NOT appear in the export
    assert!(
        !files.iter().any(|f| f.starts_with("quadlet/")),
        "quadlet/ must not be in refine export, got: {files:?}"
    );
}

#[test]
fn export_includes_user_artifacts() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(UserGroupSection {
        users: vec![serde_json::json!({
            "name": "webadmin",
            "uid": 1001,
            "gid": 1001,
            "include": true,
            "containerfile_strategy": "useradd",
            "password_choice": "preserve",
            "password_hash": "$6$rounds=5000$salt$hash",
            "home": "/home/webadmin",
            "shell": "/bin/bash",
            "ssh_keys": ["ssh-rsa AAAAB3test webadmin@host"],
            "source": "custom"
        })],
        groups: vec![serde_json::json!({
            "name": "webadmin",
            "gid": 1001,
            "source": "custom",
            "include": true
        })],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);

    // Apply a user strategy op to exercise the pipeline
    session
        .apply(RefinementOp::UserStrategy {
            username: "webadmin".into(),
            strategy: inspectah_core::types::users::UserContainerfileStrategy::Useradd,
        })
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let actual = tarball_file_set(&tarball_path);

    // User artifacts must be present in the export
    assert!(
        actual.contains("inspectah-users.ks"),
        "export must contain inspectah-users.ks, got: {actual:?}"
    );
    assert!(
        actual.contains("inspectah-users.toml"),
        "export must contain inspectah-users.toml, got: {actual:?}"
    );
    assert!(
        actual.iter().any(|f| f.starts_with("users/")),
        "export must contain users/ SSH key directory, got: {actual:?}"
    );
}

// ── Task 16a: Preview/export parity with groups and UngroupGroup ──

#[test]
fn preview_and_export_produce_same_containerfile_with_groups() {
    // Build a snapshot with groups and packages, then apply an UngroupGroup
    // directive. Both preview (session.view().containerfile_preview) and
    // export (tarball Containerfile) must produce identical output.
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
                name: "podman".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "buildah".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
        ],
        installed_groups: Some(vec![InstalledGroup {
            name: "Container Management".into(),
            members: vec!["podman".into(), "buildah".into()],
            optional_installed: vec![],
        }]),
        baseline_package_names: Some(vec![]),
        ..Default::default()
    });
    snap.redaction_state = Some(RedactionState::FullyRedacted {
        redacted_by: "inspectah 0.8.0".into(),
        config_hash: "abc123".into(),
    });
    snap.baseline = Some(BaselineData {
        image_digest: "sha256:test".into(),
        packages: HashMap::new(),
        extracted_at: "2026-01-01T00:00:00Z".into(),
    });

    let mut session = RefineSession::new(snap);

    // Apply UngroupGroup directive — changes rendering from group install
    // to individual package installs with ungrouped provenance comment.
    session
        .apply_directive(ViewDirective::UngroupGroup {
            group_name: "Container Management".into(),
        })
        .unwrap();

    // Verify render context reflects the UngroupGroup directive.
    assert!(
        session
            .render_context()
            .is_ungrouped("Container Management"),
        "render_context must show Ungrouped after UngroupGroup directive"
    );

    // Capture the preview Containerfile — both preview and export must use
    // the SAME render path. Note: recompute_view() builds the render_context
    // AFTER computing the preview, so the preview may not include group-aware
    // provenance comments. This is a known ordering issue tracked separately.
    // The parity test below verifies that whatever the preview shows, export
    // produces the exact same output.
    let preview = session.view().containerfile_preview.clone();

    // Sanity: packages must render (include=true survived normalization)
    assert!(preview.contains("podman"), "preview must contain podman");
    assert!(preview.contains("buildah"), "preview must contain buildah");

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
        "preview and exported Containerfile must be byte-identical (with groups + UngroupGroup)"
    );
}

#[test]
fn export_includes_language_packages_root() {
    let mut snap = test_snapshot();
    let mut manifests = HashMap::new();
    manifests.insert(
        "package.json".to_string(),
        r#"{"name":"myapp"}"#.to_string(),
    );
    manifests.insert(
        "package-lock.json".to_string(),
        r#"{"lockfileVersion":3}"#.to_string(),
    );
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/myapp".into(),
            name: "myapp".into(),
            method: "npm lockfile".into(),
            confidence: "high".into(),
            include: true,
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("lang-pkg-test.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let actual = tarball_file_set(&tarball_path);
    let hash = env_hash("/opt/myapp");

    let pkg_json = format!("language-packages/npm/{hash}/package.json");
    let lock_json = format!("language-packages/npm/{hash}/package-lock.json");

    assert!(
        actual.contains(&pkg_json),
        "tarball must contain {pkg_json}, got: {actual:?}"
    );
    assert!(
        actual.contains(&lock_json),
        "tarball must contain {lock_json}, got: {actual:?}"
    );
}

#[test]
fn export_excludes_language_packages_when_none_included() {
    let mut snap = test_snapshot();
    let mut manifests = HashMap::new();
    manifests.insert("requirements.txt".to_string(), "flask==3.0".to_string());
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/venv".into(),
            name: "venv".into(),
            method: "pip".into(),
            confidence: "medium".into(),
            include: false,
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("no-lang-pkg-test.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    let actual = tarball_file_set(&tarball_path);
    let has_lang_pkg = actual.iter().any(|p| p.starts_with("language-packages/"));
    assert!(
        !has_lang_pkg,
        "tarball must not contain language-packages/ when no items are included, got: {actual:?}"
    );
}
