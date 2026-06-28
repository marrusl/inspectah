use inspectah_core::baseline::BaselineData;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
use inspectah_core::types::redaction::RedactionState;
use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
use inspectah_refine::session::RefineSession;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// Helper: Create a source tarball with the given directory structure.
/// Each entry is a (path, content) tuple. Paths should NOT have leading slashes.
/// The tarball includes a top-level directory prefix (like "hostname-inspectah/")
/// to match the format produced by the scan command.
fn create_source_tarball(
    tarball_path: &Path,
    entries: Vec<(&str, &str)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(tarball_path)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    // Add a top-level directory prefix to match real tarball format
    let prefix = "test-host-inspectah";

    for (path, content) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        // Prepend the prefix directory
        let full_path = format!("{}/{}", prefix, path);
        archive.append_data(&mut header, &full_path, content.as_bytes())?;
    }

    archive.into_inner()?.finish()?;
    Ok(())
}

/// Helper: Extract all file paths from a tarball (excluding directories).
/// Strips the tarball prefix directory.
fn tarball_file_set(tarball_path: &Path) -> BTreeSet<String> {
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

/// Helper: Extract Containerfile content from export tarball.
fn extract_containerfile(tarball_path: &Path) -> String {
    let file = std::fs::File::open(tarball_path).unwrap();
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);

    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().to_string();
        if path.ends_with("Containerfile") {
            let mut content = String::new();
            std::io::Read::read_to_string(&mut entry, &mut content).unwrap();
            return content;
        }
    }
    panic!("Containerfile not found in export tarball");
}

/// Helper: Extract all COPY source paths from Containerfile that start with the given prefix.
fn extract_copy_paths(containerfile: &str, prefix: &str) -> Vec<String> {
    containerfile
        .lines()
        .filter_map(|line| {
            // Match "COPY <prefix>/path dest" or "COPY <prefix>/path/ dest/"
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("COPY ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let Some(src) = parts.first()
                    && src.starts_with(prefix)
                {
                    return Some(src.to_string());
                }
            }
            None
        })
        .collect()
}

#[test]
fn containerfile_unmanaged_copy_paths_match_export_layout() {
    // Build a snapshot with unmanaged files (include: true).
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![
            UnmanagedFile {
                path: "/opt/splunk/bin/splunkd".into(),
                size: 1024 * 1024,
                file_type: FileType::ElfBinary,
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/opt/splunk/bin/btool".into(),
                size: 512 * 1024,
                file_type: FileType::ElfBinary,
                include: true,
                ..Default::default()
            },
            UnmanagedFile {
                path: "/usr/local/bin/custom-script".into(),
                size: 4096,
                file_type: FileType::Script,
                include: true,
                ..Default::default()
            },
        ],
        total_size: 1024 * 1024 + 512 * 1024 + 4096,
        total_count: 3,
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

    // Create a source tarball with those files under unmanaged/.
    let tempdir = tempfile::tempdir().unwrap();
    let source_tarball = tempdir.path().join("source.tar.gz");
    create_source_tarball(
        &source_tarball,
        vec![
            (
                "unmanaged/opt/splunk/bin/splunkd",
                "fake ELF binary content",
            ),
            ("unmanaged/opt/splunk/bin/btool", "fake tool binary"),
            (
                "unmanaged/usr/local/bin/custom-script",
                "#!/bin/bash\necho hello",
            ),
        ],
    )
    .unwrap();

    // Create session and set the source tarball path.
    let mut session = RefineSession::new(snap);
    session.set_tarball_path(source_tarball.clone());

    // Export with the source tarball.
    let export_tarball = tempdir.path().join("export.tar.gz");
    session
        .export_tarball(&export_tarball, session.generation())
        .unwrap();

    // Extract Containerfile and verify COPY lines.
    let containerfile = extract_containerfile(&export_tarball);
    let copy_paths = extract_copy_paths(&containerfile, "unmanaged/");

    // Verify that COPY lines exist for unmanaged files.
    // The renderer groups by directory, so we expect either individual
    // COPY lines or directory-level COPY lines.
    assert!(
        !copy_paths.is_empty(),
        "Containerfile must contain COPY lines for unmanaged files"
    );

    // Extract all files from the export tarball.
    let export_files = tarball_file_set(&export_tarball);

    // Every COPY source path must exist in the export tarball.
    for copy_path in &copy_paths {
        // Directory COPY lines end with '/' — strip it for file lookups.
        let normalized = copy_path.trim_end_matches('/');

        // For directory COPY, verify at least one file under that prefix exists.
        if copy_path.ends_with('/') {
            assert!(
                export_files.iter().any(|f| f.starts_with(normalized)),
                "COPY {copy_path} exists in Containerfile but no files under {normalized}/ in export"
            );
        } else {
            // For file COPY, verify the exact file exists.
            assert!(
                export_files.contains(normalized),
                "COPY {copy_path} exists in Containerfile but file {normalized} not in export"
            );
        }
    }

    // Verify that specific expected files are present.
    assert!(
        export_files.contains("unmanaged/opt/splunk/bin/splunkd")
            || export_files
                .iter()
                .any(|f| f.starts_with("unmanaged/opt/splunk/bin/")),
        "export must contain unmanaged/opt/splunk/bin/splunkd or directory"
    );
}

#[test]
fn containerfile_repoless_copy_paths_match_export_layout() {
    // Build a snapshot with a repoless RPM (include: true, cached: true).
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: String::new(),
            include: true,
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }],
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

    // Create a source tarball with the RPM under repoless-packages/.
    let tempdir = tempfile::tempdir().unwrap();
    let source_tarball = tempdir.path().join("source.tar.gz");
    let rpm_filename = "custom-tool-1.2.3-1.el9.x86_64.rpm";
    create_source_tarball(
        &source_tarball,
        vec![(
            &format!("repoless-packages/{rpm_filename}"),
            "fake RPM binary content",
        )],
    )
    .unwrap();

    // Create session and set the source tarball path.
    let mut session = RefineSession::new(snap);
    session.set_tarball_path(source_tarball.clone());

    // Repoless packages are pre-excluded by the classifier (Task 9).
    // Explicitly include it to test the active COPY rendering.
    session
        .apply(inspectah_refine::types::RefinementOp::SetInclude {
            item_id: inspectah_refine::types::ItemId::Package {
                name: "custom-tool".into(),
                arch: "x86_64".into(),
            },
            include: true,
        })
        .unwrap();

    // Export with the source tarball.
    let export_tarball = tempdir.path().join("export.tar.gz");
    session
        .export_tarball(&export_tarball, session.generation())
        .unwrap();

    // Extract Containerfile and verify COPY lines.
    let containerfile = extract_containerfile(&export_tarball);

    let copy_paths = extract_copy_paths(&containerfile, "repoless-packages/");

    // Verify that COPY lines exist for repoless packages.
    // After explicit inclusion, repoless packages should render active COPY lines.
    assert!(
        !copy_paths.is_empty(),
        "Containerfile must contain active COPY lines for explicitly-included repoless packages"
    );

    // Extract all files from the export tarball.
    let export_files = tarball_file_set(&export_tarball);

    // Every COPY source path must exist in the export tarball.
    for copy_path in &copy_paths {
        let normalized = copy_path.trim_end_matches('/');
        assert!(
            export_files.contains(normalized),
            "COPY {copy_path} exists in Containerfile but file {normalized} not in export"
        );
    }

    // Verify that the specific RPM file is present.
    assert!(
        export_files.contains(&format!("repoless-packages/{rpm_filename}")),
        "export must contain repoless-packages/{rpm_filename}"
    );
}

#[test]
fn excluded_items_absent_from_both_containerfile_and_export() {
    // Build a snapshot with an unmanaged file (include: false) and
    // a repoless RPM (include: false).
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![UnmanagedFile {
            path: "/opt/excluded/binary".into(),
            size: 1024,
            file_type: FileType::ElfBinary,
            include: false, // Excluded
            ..Default::default()
        }],
        total_size: 1024,
        total_count: 1,
    });
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "excluded-tool".into(),
            version: "2.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            source_repo: String::new(),
            include: false, // Excluded
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }],
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

    // Create a source tarball with these items (even though they're excluded).
    let tempdir = tempfile::tempdir().unwrap();
    let source_tarball = tempdir.path().join("source.tar.gz");
    create_source_tarball(
        &source_tarball,
        vec![
            ("unmanaged/opt/excluded/binary", "fake binary content"),
            (
                "repoless-packages/excluded-tool-2.0-1.el9.x86_64.rpm",
                "fake RPM content",
            ),
        ],
    )
    .unwrap();

    // Create session and set the source tarball path.
    let mut session = RefineSession::new(snap);
    session.set_tarball_path(source_tarball.clone());

    // Export with the source tarball.
    let export_tarball = tempdir.path().join("export.tar.gz");
    session
        .export_tarball(&export_tarball, session.generation())
        .unwrap();

    // Extract Containerfile.
    let containerfile = extract_containerfile(&export_tarball);

    // Assert: no active COPY lines reference these excluded items.
    // Pre-excluded repoless packages may appear as commented-out COPY lines,
    // but unmanaged files should not appear at all when include=false.
    assert!(
        !containerfile.contains("COPY unmanaged/opt/excluded/binary"),
        "excluded unmanaged file must not have active COPY line in Containerfile"
    );

    // Extract all files from the export tarball.
    let export_files = tarball_file_set(&export_tarball);

    // Assert: excluded items are not present in the export directory.
    assert!(
        !export_files.contains("unmanaged/opt/excluded/binary"),
        "excluded unmanaged file must not be in export tarball"
    );
    assert!(
        !export_files.contains("repoless-packages/excluded-tool-2.0-1.el9.x86_64.rpm"),
        "excluded repoless RPM must not be in export tarball"
    );

    // Verify that unmanaged/ and repoless-packages/ directories are either
    // absent or empty (no files under those prefixes).
    let has_unmanaged = export_files.iter().any(|f| f.starts_with("unmanaged/"));
    let has_repoless = export_files
        .iter()
        .any(|f| f.starts_with("repoless-packages/"));

    assert!(
        !has_unmanaged,
        "export must not contain any unmanaged/ files when all are excluded"
    );
    assert!(
        !has_repoless,
        "export must not contain any repoless-packages/ files when all are excluded"
    );
}
