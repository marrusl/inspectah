use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{
    ConfigCategory, ConfigFileEntry, ConfigFileKind, ConfigSection,
};
use inspectah_core::types::containers::{ComposeFile, ContainerSection, QuadletUnit};
use inspectah_core::types::nonrpm::{
    FileType, NonRpmItem, NonRpmSoftwareSection, UnmanagedFile, UnmanagedFileSection,
};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::storage::{StorageSection, VarDirBacking, VarDirectory};
use inspectah_core::types::users::UserGroupSection;
use inspectah_core::util::{
    METHOD_GEM_LOCKFILE, METHOD_NPM_LOCKFILE, METHOD_PYTHON_VENV, env_hash,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp, ViewDirective};
use std::collections::{BTreeSet, HashMap};

/// Put a snapshot through the real redaction pass, the way `inspectah
/// scan` does before a tarball is ever written.
///
/// `test_snapshot()` stamps `FullyRedacted` by hand. A snapshot carrying
/// that label without having been through `redact()` is not a state
/// production can reach: the scan path redacts before it writes, and the
/// import path re-redacts anything not already labelled. Asserting that
/// the *export* scrubs such a snapshot tested a projection that lived on
/// the refine path alone, which is how the scan path came to ship these
/// fields raw. The export now serializes what `redact()` produced, so
/// the fixture has to produce it.
fn redacted(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    inspectah_pipeline::redaction::engine::redact(&mut snap, &Default::default());
    snap
}

fn test_snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "httpd".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "vim".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            disposition: FindingKind::included(),
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

/// Read a single file's content from a tarball, returning None if not found.
/// The tarball prefix directory is stripped (same as `tarball_file_set`).
fn tarball_read_file(tarball_path: &std::path::Path, target: &str) -> Option<String> {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    let stem = tarball_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let prefix = stem
        .strip_suffix(".tar.gz")
        .or_else(|| stem.strip_suffix(".tgz"))
        .unwrap_or(&stem);
    let prefix_slash = format!("{prefix}/");

    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.header().entry_type() == tar::EntryType::Regular {
            let raw = entry.path().unwrap().to_string_lossy().to_string();
            let stripped = raw.strip_prefix(&prefix_slash).unwrap_or(&raw).to_string();
            if stripped == target {
                let mut buf = String::new();
                std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
                return Some(buf);
            }
        }
    }
    None
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

/// The source operand of every **active** `COPY` in a Containerfile.
///
/// Commented lines are skipped deliberately: the renderer emits
/// `# COPY <path> /usr/local/bin/...` for a Go or shell binary it will
/// not carry, `# COPY config/selinux/ ...` in the SELinux block, and a
/// commented `COPY repoless-packages/...` for a demoted package. None of
/// those is an instruction to the builder.
fn active_copy_sources(containerfile: &str) -> Vec<String> {
    containerfile
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("COPY "))
        .filter_map(|line| line.split_whitespace().nth(1).map(str::to_string))
        .collect()
}

/// A `/var` directory whose only creator is a `config.files` entry the
/// operator can exclude, which is the § 3.4 route that makes
/// `render_tmpfiles_conf` produce a line.
const APPONE_CONF: &str = "/etc/tmpfiles.d/appone.conf";
const VAR_TMPFILES_MEMBER: &str = "config/usr/lib/tmpfiles.d/inspectah-var.conf";

/// Every active `COPY` in the exported Containerfile names something the
/// archive carries.
///
/// Stated over the rendered output rather than over a list of members,
/// so it holds for the next `COPY` the renderer learns to emit as well
/// as for the ones it emits today. The defect it was written for:
/// `render_tmpfiles_conf` returning `Some` makes the Containerfile stage
/// `inspectah-var.conf`, and the refine export did not write it, so the
/// archive a user downloads failed at that line under `podman build`
/// with nothing from inspectah saying why.
///
/// The op below is the reachable route, not the shortest one. A
/// `VarDirBacking::Unbacked` verdict would also produce the line, but it
/// comes from the scan and no decision reaches it; excluding a
/// tmpfiles.d fragment under Configuration Files is something the UI
/// offers, and it turns a building export into a non-building one.
#[test]
fn every_copy_source_is_in_the_archive() {
    let mut snap = test_snapshot();
    snap.config.as_mut().unwrap().files.push(ConfigFileEntry {
        path: APPONE_CONF.into(),
        kind: ConfigFileKind::Unowned,
        category: ConfigCategory::Tmpfiles,
        content: "d /var/lib/appone 0750 root root -\n".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    });
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: "/var/lib/appone".into(),
            backing: Some(VarDirBacking::Tmpfiles {
                conf: APPONE_CONF.into(),
                line: "d /var/lib/appone 0750 root root -".into(),
            }),
            mode: Some("0750".into()),
            owner_name: Some("root".into()),
            group_name: Some("root".into()),
            ..Default::default()
        }],
        ..Default::default()
    });

    let mut session = RefineSession::new(snap);
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: APPONE_CONF.into(),
            },
            include: false,
        })
        .unwrap();

    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let files = tarball_file_set(&tarball_path);
    let containerfile = tarball_read_file(&tarball_path, "Containerfile")
        .expect("the export writes a Containerfile");
    let sources = active_copy_sources(&containerfile);

    // Without these two the invariant below holds over nothing: a
    // Containerfile with no COPY line satisfies it, and so does one that
    // stopped staging the member this test was written for.
    assert!(
        sources.len() >= 2,
        "the fixture must produce several COPY lines: {sources:?}\n{containerfile}"
    );
    assert!(
        sources.iter().any(|s| s == VAR_TMPFILES_MEMBER),
        "the exclusion must still reach the /var provisioning COPY: \
         {sources:?}\n{containerfile}"
    );

    let missing: Vec<&String> = sources
        .iter()
        .filter(|src| match src.strip_suffix('/') {
            // A directory operand is satisfied by any member beneath it.
            Some(dir) => !files.iter().any(|f| f.starts_with(&format!("{dir}/"))),
            None => !files.contains(*src),
        })
        .collect();

    assert!(
        missing.is_empty(),
        "the Containerfile COPYs {missing:?}, which the archive does not \
         contain, so the build fails at that line.\n  archive: {files:?}"
    );
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
        !httpd.disposition.is_included(),
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
    assert!(vim.disposition.is_included(), "vim must remain included");
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "local-tool".into(),
                arch: "x86_64".into(),
                state: PackageState::LocalInstall,
                source_repo: String::new(),
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
            PackageEntry {
                name: "mystery".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: String::new(),
                disposition: FindingKind::excluded(),
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
        .export_plan(session1.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
        .disposition
        .is_included();
    assert_eq!(
        view_httpd.entry.disposition.is_included(),
        proj_httpd,
        "view and projected snapshot must agree"
    );
}

/// The refine export carries `quadlet/`, because the Containerfile it is
/// rendered alongside emits `COPY quadlet/ /etc/containers/systemd/`.
///
/// This test previously asserted the opposite, matching a top-level
/// allowlist that deleted the directory. Measured on a real RHEL 9.8 host:
/// every web download from a quadlet-running host failed `podman build` at
/// that COPY line.
#[test]
fn export_carries_the_quadlet_root_its_containerfile_copies() {
    let mut snap = test_snapshot();
    snap.containers = Some(ContainerSection {
        quadlet_units: vec![QuadletUnit {
            name: "myapp.container".into(),
            content: "[Container]\nImage=registry.example.com/myapp:latest\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let files = tarball_file_set(&tarball_path);

    assert!(
        files.contains("quadlet/myapp.container"),
        "quadlet/ must be in the refine export, got: {files:?}"
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "podman".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            },
            PackageEntry {
                name: "buildah".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                disposition: FindingKind::included(),
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
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
            method: METHOD_NPM_LOCKFILE.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("lang-pkg-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
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
            method: METHOD_PYTHON_VENV.into(),
            confidence: "medium".into(),
            disposition: FindingKind::excluded(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("no-lang-pkg-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let actual = tarball_file_set(&tarball_path);
    let has_lang_pkg = actual.iter().any(|p| p.starts_with("language-packages/"));
    assert!(
        !has_lang_pkg,
        "tarball must not contain language-packages/ when no items are included, got: {actual:?}"
    );
}

#[test]
fn export_redacts_manifest_files_when_snapshot_redacted() {
    let mut snap = test_snapshot();
    let mut manifests = HashMap::new();
    manifests.insert(
        "requirements.txt".to_string(),
        "--index-url https://token:s3cret@private.pypi.org/simple/\nflask==2.3.3\n".to_string(),
    );
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/venv".into(),
            name: "venv".into(),
            method: METHOD_PYTHON_VENV.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    // test_snapshot() already sets FullyRedacted — confirm it's active.
    assert!(
        snap.redaction_state.is_some(),
        "fixture must have redaction_state set"
    );

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-manifest-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let hash = env_hash("/opt/venv");
    let req_path = format!("language-packages/pip/{hash}/requirements.txt");
    let content =
        tarball_read_file(&tarball_path, &req_path).expect("requirements.txt must exist in export");

    // Auth token must be scrubbed.
    assert!(
        !content.contains("s3cret"),
        "auth token must be scrubbed from requirements.txt, got:\n{content}"
    );
    assert!(
        content.contains("REDACTED"),
        "scrubbed URL must contain REDACTED placeholder, got:\n{content}"
    );
    // Non-secret lines must be preserved.
    assert!(
        content.contains("flask==2.3.3"),
        "package lines must be preserved, got:\n{content}"
    );
}

#[test]
fn export_redacts_manifest_files_in_snapshot_json() {
    let mut snap = test_snapshot();
    let mut manifests = HashMap::new();
    manifests.insert(
        "requirements.txt".to_string(),
        "--index-url https://token:s3cret@private.pypi.org/simple/\nflask==2.3.3\n".to_string(),
    );
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/venv".into(),
            name: "venv".into(),
            method: METHOD_PYTHON_VENV.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    // test_snapshot() sets FullyRedacted.
    let session = RefineSession::new(redacted(snap));
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-snapshot-json-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let snap_json = tarball_read_file(&tarball_path, "inspection-snapshot.json")
        .expect("inspection-snapshot.json must exist");
    let exported: InspectionSnapshot = serde_json::from_str(&snap_json).unwrap();

    let item = &exported.non_rpm_software.as_ref().unwrap().items[0];
    let req_content = item
        .manifest_files
        .get("requirements.txt")
        .expect("manifest_files must still have requirements.txt key");

    assert!(
        !req_content.contains("s3cret"),
        "snapshot JSON manifest_files must have auth tokens scrubbed, got:\n{req_content}"
    );
    assert!(
        req_content.contains("REDACTED"),
        "snapshot JSON manifest_files must contain REDACTED placeholder, got:\n{req_content}"
    );
    assert!(
        req_content.contains("flask==2.3.3"),
        "snapshot JSON manifest_files must preserve package lines, got:\n{req_content}"
    );
}

#[test]
fn export_preserves_manifest_files_when_unredacted() {
    let mut snap = test_snapshot();
    // Clear redaction state to simulate an unredacted snapshot.
    snap.redaction_state = None;

    let raw_content =
        "--index-url https://token:s3cret@private.pypi.org/simple/\nflask==2.3.3\n".to_string();
    let mut manifests = HashMap::new();
    manifests.insert("requirements.txt".to_string(), raw_content.clone());
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/venv".into(),
            name: "venv".into(),
            method: METHOD_PYTHON_VENV.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("no-redact-manifest-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let hash = env_hash("/opt/venv");
    let req_path = format!("language-packages/pip/{hash}/requirements.txt");
    let content =
        tarball_read_file(&tarball_path, &req_path).expect("requirements.txt must exist in export");

    // Content must be verbatim — no scrubbing.
    assert_eq!(
        content, raw_content,
        "unredacted export must preserve manifest content verbatim"
    );
}

#[test]
fn export_redacts_package_json_registry_auth() {
    let mut snap = test_snapshot();
    let raw = r#"{
  "name": "myapp",
  "dependencies": {},
  "publishConfig": {
    "registry": "https://deploy:tok3n@npm.example.com/repo/"
  }
}"#
    .to_string();
    let mut manifests = HashMap::new();
    manifests.insert("package.json".to_string(), raw);
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/myapp".into(),
            name: "myapp".into(),
            method: METHOD_NPM_LOCKFILE.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-npm-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let hash = env_hash("/opt/myapp");
    let pkg_path = format!("language-packages/npm/{hash}/package.json");
    let content =
        tarball_read_file(&tarball_path, &pkg_path).expect("package.json must exist in export");

    assert!(
        !content.contains("tok3n"),
        "auth token must be scrubbed from package.json, got:\n{content}"
    );
    assert!(
        content.contains("REDACTED@npm.example.com"),
        "scrubbed URL must contain REDACTED@host, got:\n{content}"
    );
}

#[test]
fn export_redacts_gemfile_source_auth() {
    let mut snap = test_snapshot();
    let raw =
        "source \"https://user:p4ss@gems.example.com\"\n\ngem 'rails', '~> 7.0'\n".to_string();
    let mut manifests = HashMap::new();
    manifests.insert("Gemfile".to_string(), raw);
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/railsapp".into(),
            name: "railsapp".into(),
            method: METHOD_GEM_LOCKFILE.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-gem-test.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let hash = env_hash("/opt/railsapp");
    let gemfile_path = format!("language-packages/gem/{hash}/Gemfile");
    let content =
        tarball_read_file(&tarball_path, &gemfile_path).expect("Gemfile must exist in export");

    assert!(
        !content.contains("p4ss"),
        "auth token must be scrubbed from Gemfile, got:\n{content}"
    );
    assert!(
        content.contains("REDACTED@gems.example.com"),
        "scrubbed URL must contain REDACTED@host, got:\n{content}"
    );
    assert!(
        content.contains("gem 'rails'"),
        "non-source lines must be preserved, got:\n{content}"
    );
}

#[test]
fn export_redacts_package_lock_json_resolved_auth() {
    let mut snap = test_snapshot();
    let raw = r#"{
  "name": "myapp",
  "lockfileVersion": 3,
  "packages": {
    "node_modules/@scope/pkg": {
      "version": "1.2.3",
      "resolved": "https://deploy:s3cret@npm.corp.example.com/@scope/pkg/-/pkg-1.2.3.tgz",
      "integrity": "sha512-abc123"
    }
  }
}"#
    .to_string();
    let mut manifests = HashMap::new();
    manifests.insert("package-lock.json".to_string(), raw);
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/webapp".into(),
            name: "webapp".into(),
            method: METHOD_NPM_LOCKFILE.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(redacted(snap));
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-lockfile-npm.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    // Check sidecar file
    let hash = env_hash("/opt/webapp");
    let lockfile_path = format!("language-packages/npm/{hash}/package-lock.json");
    let content = tarball_read_file(&tarball_path, &lockfile_path)
        .expect("package-lock.json must exist in export");

    assert!(
        !content.contains("s3cret"),
        "auth token must be scrubbed from package-lock.json sidecar, got:\n{content}"
    );
    assert!(
        content.contains("REDACTED@npm.corp.example.com"),
        "scrubbed URL must contain REDACTED@host, got:\n{content}"
    );
    assert!(
        content.contains("integrity"),
        "non-resolved fields must be preserved, got:\n{content}"
    );
    // The scrubbed lockfile has to survive `npm ci`, so validity is the
    // assertion that matters — the substring checks above all passed while
    // the exported file was unparseable.
    serde_json::from_str::<serde_json::Value>(&content)
        .unwrap_or_else(|e| panic!("exported package-lock.json must parse: {e}\n{content}"));

    // Check inspection-snapshot.json
    let snapshot_json =
        tarball_read_file(&tarball_path, "inspection-snapshot.json").expect("snapshot must exist");
    assert!(
        !snapshot_json.contains("s3cret"),
        "auth token must be scrubbed from inspection-snapshot.json, got lockfile leak"
    );
}

#[test]
fn export_redacts_gemfile_lock_auth() {
    let mut snap = test_snapshot();
    let raw = "GEM\n  remote: https://deploy:g3m_tok@gems.corp.example.com/\n  specs:\n    rails (7.0.8)\n\nPLATFORMS\n  ruby\n\nDEPENDENCIES\n  rails (~> 7.0)\n".to_string();
    let mut manifests = HashMap::new();
    manifests.insert("Gemfile.lock".to_string(), raw);
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            path: "/opt/railsapp".into(),
            name: "railsapp".into(),
            method: METHOD_GEM_LOCKFILE.into(),
            confidence: "high".into(),
            disposition: FindingKind::included(),
            manifest_files: manifests,
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(redacted(snap));
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("redact-lockfile-gem.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    // Check sidecar file
    let hash = env_hash("/opt/railsapp");
    let lockfile_path = format!("language-packages/gem/{hash}/Gemfile.lock");
    let content = tarball_read_file(&tarball_path, &lockfile_path)
        .expect("Gemfile.lock must exist in export");

    assert!(
        !content.contains("g3m_tok"),
        "auth token must be scrubbed from Gemfile.lock sidecar, got:\n{content}"
    );
    assert!(
        content.contains("REDACTED@gems.corp.example.com"),
        "scrubbed URL must contain REDACTED@host, got:\n{content}"
    );
    assert!(
        content.contains("rails (7.0.8)"),
        "spec lines must be preserved, got:\n{content}"
    );

    // Check inspection-snapshot.json
    let snapshot_json =
        tarball_read_file(&tarball_path, "inspection-snapshot.json").expect("snapshot must exist");
    assert!(
        !snapshot_json.contains("g3m_tok"),
        "auth token must be scrubbed from inspection-snapshot.json, got lockfile leak"
    );
}

#[test]
fn export_allowlist_includes_unmanaged_root() {
    // Build a snapshot with unmanaged files
    let mut snap = test_snapshot();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![UnmanagedFile {
            path: "/opt/myapp/server".into(),
            size: 1024,
            file_type: FileType::ElfBinary,
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        usr_entries: Vec::new(),
        total_size: 1024,
        total_count: 1,
    });

    // Create a source tarball with files under unmanaged/
    let source_tempdir = tempfile::tempdir().unwrap();
    let source_dir = source_tempdir.path();
    let unmanaged_dir = source_dir.join("unmanaged/opt/myapp");
    std::fs::create_dir_all(&unmanaged_dir).unwrap();
    std::fs::write(unmanaged_dir.join("server"), b"binary content").unwrap();

    let source_tarball_path = source_tempdir.path().join("source.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(source_dir, &source_tarball_path, "source")
        .unwrap();

    // Run render_refine_export with source tarball
    let out_tempdir = tempfile::tempdir().unwrap();
    let out_path = out_tempdir.path().join("export.tar.gz");
    inspectah_refine::session::render_refine_export(
        &snap,
        &out_path,
        None,
        None,
        Some(&source_tarball_path),
        None,
    )
    .unwrap();

    // Assert: unmanaged/ directory present in output
    let files = tarball_file_set(&out_path);
    assert!(
        files.iter().any(|f| f.starts_with("unmanaged/")),
        "export must contain unmanaged/ directory, got: {files:?}"
    );
    assert!(
        files.contains("unmanaged/opt/myapp/server"),
        "export must contain extracted unmanaged file, got: {files:?}"
    );
}

#[test]
fn export_prunes_excluded_unmanaged_files() {
    // Build a snapshot with two unmanaged files, one include:false
    let mut snap = test_snapshot();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![
            UnmanagedFile {
                path: "/opt/app/included".into(),
                size: 512,
                file_type: FileType::DataFile,
                disposition: FindingKind::included(),
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/app/excluded".into(),
                size: 256,
                file_type: FileType::DataFile,
                disposition: FindingKind::excluded(),
                ..Default::default()
            },
        ],
        usr_entries: Vec::new(),
        total_size: 768,
        total_count: 2,
    });

    // Create source tarball with both files
    let source_tempdir = tempfile::tempdir().unwrap();
    let source_dir = source_tempdir.path();
    let unmanaged_dir = source_dir.join("unmanaged/opt/app");
    std::fs::create_dir_all(&unmanaged_dir).unwrap();
    std::fs::write(unmanaged_dir.join("included"), b"included content").unwrap();
    std::fs::write(unmanaged_dir.join("excluded"), b"excluded content").unwrap();

    let source_tarball_path = source_tempdir.path().join("source.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(source_dir, &source_tarball_path, "source")
        .unwrap();

    // Run export
    let out_tempdir = tempfile::tempdir().unwrap();
    let out_path = out_tempdir.path().join("export.tar.gz");
    inspectah_refine::session::render_refine_export(
        &snap,
        &out_path,
        None,
        None,
        Some(&source_tarball_path),
        None,
    )
    .unwrap();

    // Assert: included file present, excluded file absent
    let files = tarball_file_set(&out_path);
    assert!(
        files.contains("unmanaged/opt/app/included"),
        "export must contain included file, got: {files:?}"
    );
    assert!(
        !files.contains("unmanaged/opt/app/excluded"),
        "export must NOT contain excluded file, got: {files:?}"
    );
}

#[test]
fn export_allowlist_includes_repoless_packages_root() {
    // Build a snapshot with repo-less RPM data (include: true)
    let mut snap = test_snapshot();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            disposition: FindingKind::included(),
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    // Create source tarball with RPM under repoless-packages/
    let source_tempdir = tempfile::tempdir().unwrap();
    let source_dir = source_tempdir.path();
    let repoless_dir = source_dir.join("repoless-packages");
    std::fs::create_dir_all(&repoless_dir).unwrap();
    std::fs::write(
        repoless_dir.join("custom-tool-1.2.3-1.el9.x86_64.rpm"),
        b"rpm data",
    )
    .unwrap();

    let source_tarball_path = source_tempdir.path().join("source.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(source_dir, &source_tarball_path, "source")
        .unwrap();

    // Run export
    let out_tempdir = tempfile::tempdir().unwrap();
    let out_path = out_tempdir.path().join("export.tar.gz");
    inspectah_refine::session::render_refine_export(
        &snap,
        &out_path,
        None,
        None,
        Some(&source_tarball_path),
        None,
    )
    .unwrap();

    // Assert: repoless-packages/ present in output
    let files = tarball_file_set(&out_path);
    assert!(
        files.iter().any(|f| f.starts_with("repoless-packages/")),
        "export must contain repoless-packages/ directory, got: {files:?}"
    );
    assert!(
        files.contains("repoless-packages/custom-tool-1.2.3-1.el9.x86_64.rpm"),
        "export must contain cached RPM, got: {files:?}"
    );
}

#[test]
fn export_includes_uploaded_rpms() {
    // Build a snapshot with repo-less RPM that will be uploaded
    let mut snap = test_snapshot();
    // `set_uploaded` sets `uploaded`, `repoless_cached` and `cache_path`
    // together, and the export proves the last of those rather than
    // scanning the directory: the filename matching the snapshot's NEVRA
    // is no longer what gets it exported.
    let upload_tempdir = tempfile::tempdir().unwrap();
    let upload_dir = upload_tempdir.path().canonicalize().unwrap();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "uploaded-tool".into(),
            version: "2.0.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            disposition: FindingKind::included(),
            repoless_cached: true,
            uploaded: true,
            cache_path: Some(
                upload_dir
                    .join("uploaded-tool-2.0.0-1.el9.x86_64.rpm")
                    .to_string_lossy()
                    .into_owned(),
            ),
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let upload_dir = upload_dir.as_path();
    std::fs::write(
        upload_dir.join("uploaded-tool-2.0.0-1.el9.x86_64.rpm"),
        b"uploaded rpm data",
    )
    .unwrap();

    // Run export with upload_dir (no source tarball)
    let out_tempdir = tempfile::tempdir().unwrap();
    let out_path = out_tempdir.path().join("export.tar.gz");
    inspectah_refine::session::render_refine_export(
        &snap,
        &out_path,
        None,
        None,
        None,
        Some(upload_dir),
    )
    .unwrap();

    // Assert: uploaded RPM appears in repoless-packages/ output
    let files = tarball_file_set(&out_path);
    assert!(
        files.contains("repoless-packages/uploaded-tool-2.0.0-1.el9.x86_64.rpm"),
        "export must contain uploaded RPM, got: {files:?}"
    );
}

#[test]
fn export_merges_cached_and_uploaded_rpms() {
    // Build a snapshot with two repo-less RPMs: one cached, one uploaded
    let mut snap = test_snapshot();
    let upload_tempdir = tempfile::tempdir().unwrap();
    let upload_dir = upload_tempdir.path().canonicalize().unwrap();
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "cached-tool".into(),
                version: "1.0.0".into(),
                release: "1.el9".into(),
                arch: "x86_64".into(),
                source_repo: String::new(),
                disposition: FindingKind::included(),
                repoless_cached: true,
                repoless_annotation: "No repo source — cached RPM bundled".into(),
                ..Default::default()
            },
            PackageEntry {
                name: "uploaded-tool".into(),
                version: "2.0.0".into(),
                release: "1.el9".into(),
                arch: "x86_64".into(),
                source_repo: String::new(),
                disposition: FindingKind::included(),
                repoless_cached: true, // set by set_uploaded after upload
                uploaded: true,
                cache_path: Some(
                    upload_dir
                        .join("uploaded-tool-2.0.0-1.el9.x86_64.rpm")
                        .to_string_lossy()
                        .into_owned(),
                ),
                repoless_annotation: "No repo source — cached RPM bundled".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    // Source tarball has cached-tool.rpm under repoless-packages/
    let source_tempdir = tempfile::tempdir().unwrap();
    let source_dir = source_tempdir.path();
    let repoless_dir = source_dir.join("repoless-packages");
    std::fs::create_dir_all(&repoless_dir).unwrap();
    std::fs::write(
        repoless_dir.join("cached-tool-1.0.0-1.el9.x86_64.rpm"),
        b"cached rpm data",
    )
    .unwrap();

    let source_tarball_path = source_tempdir.path().join("source.tar.gz");
    inspectah_pipeline::render::tarball::create_tarball(source_dir, &source_tarball_path, "source")
        .unwrap();

    // Upload dir has uploaded-tool.rpm
    let upload_dir = upload_dir.as_path();
    std::fs::write(
        upload_dir.join("uploaded-tool-2.0.0-1.el9.x86_64.rpm"),
        b"uploaded rpm data",
    )
    .unwrap();

    // Run export with both source_tarball and upload_dir
    let out_tempdir = tempfile::tempdir().unwrap();
    let out_path = out_tempdir.path().join("export.tar.gz");
    inspectah_refine::session::render_refine_export(
        &snap,
        &out_path,
        None,
        None,
        Some(&source_tarball_path),
        Some(upload_dir),
    )
    .unwrap();

    // Assert: both RPMs present in repoless-packages/ output
    let files = tarball_file_set(&out_path);
    assert!(
        files.contains("repoless-packages/cached-tool-1.0.0-1.el9.x86_64.rpm"),
        "export must contain cached RPM, got: {files:?}"
    );
    assert!(
        files.contains("repoless-packages/uploaded-tool-2.0.0-1.el9.x86_64.rpm"),
        "export must contain uploaded RPM, got: {files:?}"
    );
}

#[test]
fn export_includes_compose_files() {
    let mut snap = test_snapshot();
    snap.containers = Some(ContainerSection {
        compose_files: vec![ComposeFile {
            path: "opt/myapp/docker-compose.yml".to_string(),
            raw_content: Some("services:\n  web:\n    image: nginx\n".to_string()),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    let files = tarball_file_set(&tarball_path);
    assert!(
        files
            .iter()
            .any(|f| f.contains("compose/opt/myapp/docker-compose.yml")),
        "compose file not found in tarball: {files:?}"
    );
}

#[test]
fn export_redacts_compose_secrets() {
    let mut snap = test_snapshot();
    let yaml = "services:\n  db:\n    environment:\n      DB_PASSWORD=hunter2\n      PORT=5432\n";
    snap.containers = Some(ContainerSection {
        compose_files: vec![ComposeFile {
            path: "opt/db/docker-compose.yml".to_string(),
            raw_content: Some(yaml.to_string()),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });

    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("output.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball_path)
        .unwrap();

    // The test_snapshot() sets redaction_state to FullyRedacted,
    // so compose secrets should be scrubbed.
    let content = tarball_read_file(&tarball_path, "compose/opt/db/docker-compose.yml")
        .expect("compose file must exist in tarball");
    assert!(
        content.contains("<REDACTED>"),
        "secrets not redacted in compose export: {content}"
    );
    assert!(
        !content.contains("hunter2"),
        "secret value leaked in compose export: {content}"
    );
    assert!(
        content.contains("PORT=5432"),
        "non-secret was incorrectly redacted: {content}"
    );
}
