//! Renderer module — produces all output artifacts from an InspectionSnapshot.
//!
//! Go writes 8 artifacts unconditionally. Phase 1 must produce all 8:
//!
//! 1. Containerfile — image build definition
//! 2. report.html — minimal PatternFly HTML report
//! 3. audit-report.md — findings and recommendations
//! 4. secrets-review.md — redaction details
//! 5. README.md — summary with build commands
//! 6. kickstart-suggestion.ks — deploy-time settings
//! 7. inspection-snapshot.json — the snapshot itself (written by caller)
//! 8. config/ tree — config files to COPY into the image

pub mod audit;
pub mod configtree;
pub mod containerfile;
pub mod kickstart;
pub mod readme;
pub mod report;
pub mod safety;
pub mod secrets;
pub mod tarball;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::{RenderContext, RenderError};
use std::path::Path;

/// Write all 8 output artifacts to the given directory.
///
/// The snapshot JSON itself is written here as well (artifact 7).
/// Config tree writing (artifact 8) is deferred to Task 24.
pub fn render_all(
    snap: &InspectionSnapshot,
    context: &RenderContext,
    output_dir: &Path,
) -> Result<(), RenderError> {
    std::fs::create_dir_all(output_dir)?;

    // 8. config/ tree — materialize FIRST so the Containerfile can use
    //    the actual directory list for its COPY lines (single source of truth).
    let materialized_roots = configtree::write_config_tree(snap, output_dir)?;

    // 1. Containerfile — COPY lines derived from materialized config tree roots
    let containerfile = containerfile::render_containerfile(snap, Some(&materialized_roots));
    std::fs::write(output_dir.join("Containerfile"), containerfile)?;

    // 2. report.html
    let html = report::render_report(snap, context);
    std::fs::write(output_dir.join("report.html"), html)?;

    // 3. audit-report.md
    let audit = audit::render_audit(snap);
    std::fs::write(output_dir.join("audit-report.md"), audit)?;

    // 4. secrets-review.md
    let secrets = secrets::render_secrets_review(snap);
    std::fs::write(output_dir.join("secrets-review.md"), secrets)?;

    // 5. README.md
    let readme = readme::render_readme(snap);
    std::fs::write(output_dir.join("README.md"), readme)?;

    // 6. kickstart-suggestion.ks
    let ks = kickstart::render_kickstart(snap);
    std::fs::write(output_dir.join("kickstart-suggestion.ks"), ks)?;

    // 7. inspection-snapshot.json
    let json = serde_json::to_string_pretty(snap)
        .map_err(|e| RenderError::Failed(format!("serialize snapshot: {e}")))?;
    std::fs::write(output_dir.join("inspection-snapshot.json"), json)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};
    use tempfile::TempDir;

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                release: "5.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_render_all_produces_all_artifacts() {
        let snap = test_snapshot();
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();

        render_all(&snap, &context, dir.path()).unwrap();

        // Verify all 8 artifacts exist
        assert!(dir.path().join("Containerfile").exists(), "Containerfile missing");
        assert!(dir.path().join("report.html").exists(), "report.html missing");
        assert!(dir.path().join("audit-report.md").exists(), "audit-report.md missing");
        assert!(dir.path().join("secrets-review.md").exists(), "secrets-review.md missing");
        assert!(dir.path().join("README.md").exists(), "README.md missing");
        assert!(dir.path().join("kickstart-suggestion.ks").exists(), "kickstart-suggestion.ks missing");
        assert!(dir.path().join("inspection-snapshot.json").exists(), "inspection-snapshot.json missing");
        assert!(dir.path().join("config").exists(), "config/ directory missing");
    }

    #[test]
    fn test_render_all_artifacts_non_empty() {
        let snap = test_snapshot();
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();

        render_all(&snap, &context, dir.path()).unwrap();

        let files = &[
            "Containerfile",
            "report.html",
            "audit-report.md",
            "secrets-review.md",
            "README.md",
            "kickstart-suggestion.ks",
            "inspection-snapshot.json",
        ];

        for name in files {
            let content = std::fs::read_to_string(dir.path().join(name)).unwrap();
            assert!(!content.is_empty(), "{} must be non-empty", name);
        }
    }

    /// Verify that every directory under config/ has a matching COPY line
    /// in the Containerfile, and vice versa: no COPY line references a
    /// directory that doesn't exist in config/. This is the core desync
    /// invariant — the Containerfile and config tree describe the same system.
    #[test]
    fn test_containerfile_copy_lines_match_config_tree() {
        let mut snap = test_snapshot();
        snap.config = Some(ConfigSection {
            files: vec![
                ConfigFileEntry {
                    path: "/etc/httpd/conf/httpd.conf".into(),
                    content: "ServerRoot /etc/httpd".into(),
                    include: true,
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "/usr/lib/sysctl.d/99-custom.conf".into(),
                    content: "net.ipv4.ip_forward = 1".into(),
                    include: true,
                    ..Default::default()
                },
            ],
        });

        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        // Collect actual top-level dirs under config/
        let config_dir = dir.path().join("config");
        let mut actual_dirs: Vec<String> = std::fs::read_dir(&config_dir)
            .unwrap()
            .filter_map(|e| {
                let e = e.ok()?;
                if e.path().is_dir() {
                    Some(e.file_name().to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        actual_dirs.sort();

        // Parse COPY lines from Containerfile for config/ entries
        let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).unwrap();
        let mut copy_roots: Vec<String> = Vec::new();
        for line in containerfile.lines() {
            // Match "COPY config/XYZ/ /XYZ/" pattern
            if let Some(rest) = line.strip_prefix("COPY config/") {
                if let Some(root) = rest.split('/').next() {
                    if !root.is_empty() && !copy_roots.contains(&root.to_string()) {
                        copy_roots.push(root.to_string());
                    }
                }
            }
        }
        copy_roots.sort();

        // Every config dir must have a COPY line
        for dir_name in &actual_dirs {
            assert!(
                copy_roots.contains(dir_name),
                "config/{dir_name}/ exists but has no COPY line in Containerfile"
            );
        }

        // Every COPY line must reference a directory that exists
        for root in &copy_roots {
            assert!(
                actual_dirs.contains(root),
                "Containerfile has COPY config/{root}/ but no such directory exists"
            );
        }

        // Sanity: both should contain etc and usr
        assert!(actual_dirs.contains(&"etc".to_string()), "etc must be materialized");
        assert!(actual_dirs.contains(&"usr".to_string()), "usr must be materialized");
    }

    /// Verify that a snapshot with no config files produces no COPY
    /// lines for config/ directories — both the tree and Containerfile
    /// should agree on "nothing to copy".
    #[test]
    fn test_empty_config_no_copy_lines() {
        let snap = test_snapshot(); // has RPM but no config
        let context = RenderContext { target: None };
        let dir = TempDir::new().unwrap();
        render_all(&snap, &context, dir.path()).unwrap();

        let containerfile = std::fs::read_to_string(dir.path().join("Containerfile")).unwrap();
        let config_copy_lines: Vec<&str> = containerfile
            .lines()
            .filter(|l| l.starts_with("COPY config/") && !l.contains("yum.repos.d") && !l.contains("pki/rpm-gpg"))
            .collect();
        assert!(
            config_copy_lines.is_empty(),
            "no config files -> no COPY config/ lines, got: {:?}",
            config_copy_lines
        );
    }
}
