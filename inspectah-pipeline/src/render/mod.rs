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
//! 8. config/ tree — config files to COPY (Task 24)

pub mod audit;
pub mod containerfile;
pub mod kickstart;
pub mod readme;
pub mod report;
pub mod safety;
pub mod secrets;

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

    // 1. Containerfile
    let containerfile = containerfile::render_containerfile(snap);
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

    // 8. config/ tree — deferred to Task 24
    // config tree writing requires filesystem traversal from the snapshot's
    // config section. For now, create the empty directory.
    let config_dir = output_dir.join("config");
    std::fs::create_dir_all(&config_dir)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
