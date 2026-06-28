//! Containerfile preview/export parity test for language packages.
//!
//! Verifies that COPY paths in the rendered Containerfile match the actual
//! layout of the export tarball. The renderer and exporter both use `env_hash()`
//! from inspectah_core::util to generate consistent directory names.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{LanguagePackage, NonRpmItem, NonRpmSoftwareSection};
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_core::util::{env_hash, METHOD_NPM_LOCKFILE, METHOD_PYTHON_VENV};
use inspectah_pipeline::render::language_packages::language_package_lines;
use inspectah_refine::session::RefineSession;
use std::collections::{BTreeSet, HashMap};

/// Extract all COPY source paths from rendered Containerfile lines.
/// Returns paths like "language-packages/pip/<hash>/requirements.txt".
fn extract_copy_paths(containerfile_lines: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    for line in containerfile_lines {
        let trimmed = line.trim();
        if trimmed.starts_with("COPY ") {
            // Format: COPY source dest
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 && parts[0] == "COPY" {
                let source = parts[1];
                // Only include paths starting with language-packages/
                if source.starts_with("language-packages/") {
                    paths.push(source.to_string());
                }
            }
        }
    }
    paths
}

/// Collect all file entries from a tarball as a sorted set of paths.
/// Strips the tarball prefix directory.
fn tarball_file_set(tarball_path: &std::path::Path) -> BTreeSet<String> {
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
fn containerfile_copy_paths_match_export_layout() {
    // Build a snapshot with pip and npm items that will emit COPY instructions.
    let mut snap = InspectionSnapshot::new();

    // Add python3 and nodejs to RPM list so renderers don't skip the items.
    snap.rpm = Some(RpmSection {
        packages_added: vec![
            PackageEntry {
                name: "python3".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "nodejs".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                source_repo: "appstream".into(),
                include: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    // Build pip venv item with requirements.txt
    let pip_path = "/opt/myapp/venv";
    let mut pip_manifests = HashMap::new();
    pip_manifests.insert("requirements.txt".to_string(), "flask==2.3.3\n".to_string());

    let pip_item = NonRpmItem {
        path: pip_path.into(),
        name: "venv".into(),
        method: METHOD_PYTHON_VENV.into(), // Must match collector's actual output
        confidence: "high".into(),
        include: true,
        manifest_files: pip_manifests,
        packages: vec![LanguagePackage {
            name: "flask".into(),
            version: "2.3.3".into(),
        }],
        ..Default::default()
    };

    // Build npm project item with package.json and package-lock.json
    let npm_path = "/opt/webapp";
    let mut npm_manifests = HashMap::new();
    npm_manifests.insert(
        "package.json".to_string(),
        r#"{"name":"webapp","version":"1.0.0"}"#.to_string(),
    );
    npm_manifests.insert(
        "package-lock.json".to_string(),
        r#"{"lockfileVersion":3}"#.to_string(),
    );

    let npm_item = NonRpmItem {
        path: npm_path.into(),
        name: "webapp".into(),
        method: METHOD_NPM_LOCKFILE.into(),
        confidence: "high".into(),
        include: true,
        manifest_files: npm_manifests,
        ..Default::default()
    };

    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![pip_item, npm_item],
        ..Default::default()
    });

    // Render Containerfile lines (preview)
    let containerfile_lines = language_package_lines(&snap);

    // Extract COPY paths from the rendered lines
    let copy_paths = extract_copy_paths(&containerfile_lines);

    assert!(
        !copy_paths.is_empty(),
        "test fixture must produce COPY instructions"
    );

    // Export the snapshot to a tarball
    let session = RefineSession::new(snap);
    let tempdir = tempfile::tempdir().unwrap();
    let tarball_path = tempdir.path().join("parity-test.tar.gz");
    session
        .export_tarball(&tarball_path, session.generation())
        .unwrap();

    // Extract the tarball file list
    let tarball_files = tarball_file_set(&tarball_path);

    // Verify: every COPY source path exists in the tarball
    let mut missing = Vec::new();
    for copy_path in &copy_paths {
        if !tarball_files.contains(copy_path) {
            missing.push(copy_path.clone());
        }
    }

    // Debug: show what's actually in the tarball
    let lang_pkg_entries: Vec<_> = tarball_files
        .iter()
        .filter(|p| p.starts_with("language-packages/"))
        .collect();

    // Verify path consistency using env_hash
    let pip_hash = env_hash(pip_path);
    let npm_hash = env_hash(npm_path);

    let pip_requirements_path = format!("language-packages/pip/{pip_hash}/requirements.txt");
    let npm_package_path = format!("language-packages/npm/{npm_hash}/package.json");
    let npm_lock_path = format!("language-packages/npm/{npm_hash}/package-lock.json");

    assert!(
        tarball_files.contains(&pip_requirements_path),
        "tarball must contain pip requirements.txt at {pip_requirements_path}\n\
         Found language-packages/ entries: {lang_pkg_entries:?}"
    );
    assert!(
        tarball_files.contains(&npm_package_path),
        "tarball must contain npm package.json at {npm_package_path}\n\
         Found language-packages/ entries: {lang_pkg_entries:?}"
    );
    assert!(
        tarball_files.contains(&npm_lock_path),
        "tarball must contain npm package-lock.json at {npm_lock_path}\n\
         Found language-packages/ entries: {lang_pkg_entries:?}"
    );

    assert!(
        missing.is_empty(),
        "Containerfile COPY paths missing from export tarball:\n  {missing:?}\n\n\
         Containerfile paths: {copy_paths:?}\n\n\
         Tarball language-packages/ entries: {:?}",
        tarball_files
            .iter()
            .filter(|p| p.starts_with("language-packages/"))
            .collect::<Vec<_>>()
    );
}
