//! Audit report renderer — produces audit-report.md summarizing changes,
//! risks, and recommendations.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::ConfigFileKind;

/// Render the audit report markdown from a snapshot.
pub fn render_audit(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();

    lines.push("# Audit Report".into());
    lines.push(String::new());

    // Incomplete sections warning
    if let Completeness::Partial { ref incomplete_sections, ref reason } = snap.completeness {
        lines.push("## Incomplete Sections".into());
        lines.push(String::new());
        lines.push(
            "The following inspector sections failed or were degraded during inspection:".into(),
        );
        lines.push(String::new());
        for id in incomplete_sections {
            lines.push(format!("- `{:?}`", id).to_lowercase());
        }
        if !reason.is_empty() {
            lines.push(String::new());
            lines.push(format!("**Reason:** {reason}"));
        }
        lines.push(String::new());
        lines.push(
            "Artifacts generated from this snapshot may be missing data from these sections.".into(),
        );
        lines.push(String::new());
    }

    // OS info
    if let Some(os) = &snap.os_release {
        let name = if os.pretty_name.is_empty() {
            &os.name
        } else {
            &os.pretty_name
        };
        lines.push(format!("**Source system:** {name}"));
        lines.push(String::new());
    }

    // Packages
    if let Some(rpm) = &snap.rpm {
        lines.push("## Packages".into());
        lines.push(String::new());

        let included: usize = rpm.packages_added.iter().filter(|p| p.include).count();
        if included > 0 {
            lines.push(format!("### Added Packages ({included})"));
            lines.push(String::new());
            lines.push("| Name | Version | Release | Arch | Repo |".into());
            lines.push("|------|---------|---------|------|------|".into());
            for p in &rpm.packages_added {
                if !p.include {
                    continue;
                }
                lines.push(format!(
                    "| {} | {} | {} | {} | {} |",
                    p.name, p.version, p.release, p.arch, p.source_repo
                ));
            }
            lines.push(String::new());
        }

        // Version changes
        if !rpm.version_changes.is_empty() {
            lines.push(format!(
                "### Version Changes ({})",
                rpm.version_changes.len()
            ));
            lines.push(String::new());
            lines.push("| Package | Host Version | Base Version | Direction |".into());
            lines.push("|---------|--------------|--------------|-----------|".into());
            for vc in &rpm.version_changes {
                let dir = serde_json::to_string(&vc.direction)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                lines.push(format!(
                    "| {} | {} | {} | {} |",
                    vc.name, vc.host_version, vc.base_version, dir
                ));
            }
            lines.push(String::new());
        }

        // Module streams
        let non_baseline: Vec<_> = rpm
            .module_streams
            .iter()
            .filter(|ms| ms.include && !ms.baseline_match)
            .collect();
        if !non_baseline.is_empty() {
            lines.push(format!("### Module Streams ({})", non_baseline.len()));
            lines.push(String::new());
            for ms in &non_baseline {
                lines.push(format!("- {}:{}", ms.module_name, ms.stream));
            }
            lines.push(String::new());
        }
    }

    // Config files
    if let Some(config) = &snap.config {
        if !config.files.is_empty() {
            lines.push("## Configuration Files".into());
            lines.push(String::new());

            let modified: usize = config
                .files
                .iter()
                .filter(|f| f.include && f.kind == ConfigFileKind::RpmOwnedModified)
                .count();
            let unowned: usize = config
                .files
                .iter()
                .filter(|f| f.include && f.kind == ConfigFileKind::Unowned)
                .count();

            if modified > 0 {
                lines.push(format!("### Modified RPM-Owned Files ({modified})"));
                lines.push(String::new());
                for f in &config.files {
                    if !f.include || f.kind != ConfigFileKind::RpmOwnedModified {
                        continue;
                    }
                    lines.push(format!("#### `{}`", f.path));
                    lines.push(String::new());
                    if let Some(ref diff) = f.diff_against_rpm {
                        if !diff.is_empty() {
                            lines.push("```diff".into());
                            lines.push(diff.clone());
                            lines.push("```".into());
                            lines.push(String::new());
                        }
                    }
                }
            }

            if unowned > 0 {
                lines.push(format!("### Unowned Config Files ({unowned})"));
                lines.push(String::new());
                for f in &config.files {
                    if !f.include || f.kind != ConfigFileKind::Unowned {
                        continue;
                    }
                    let category = serde_json::to_string(&f.category)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string();
                    lines.push(format!("- `{}` ({})", f.path, category));
                }
                lines.push(String::new());
            }
        }
    }

    // Services
    if let Some(services) = &snap.services {
        if !services.state_changes.is_empty() {
            lines.push("## Service State Changes".into());
            lines.push(String::new());
            lines.push("| Unit | Current | Default | Action |".into());
            lines.push("|------|---------|---------|--------|".into());
            for sc in &services.state_changes {
                lines.push(format!(
                    "| {} | {} | {} | {} |",
                    sc.unit, sc.current_state, sc.default_state, sc.action
                ));
            }
            lines.push(String::new());
        }
    }

    // Redactions
    if !snap.redactions.is_empty() {
        lines.push("## Redactions".into());
        lines.push(String::new());
        lines.push(format!(
            "{} item(s) redacted. See `secrets-review.md` for details.",
            snap.redactions.len()
        ));
        lines.push(String::new());
    }

    // Warnings
    if !snap.warnings.is_empty() {
        lines.push("## Warnings".into());
        lines.push(String::new());
        for w in &snap.warnings {
            if !w.message.is_empty() {
                lines.push(format!("- {}", w.message));
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_audit_report_renders() {
        let snap = test_snapshot();
        let md = render_audit(&snap);
        assert!(md.contains("# Audit Report"));
    }

    #[test]
    fn test_audit_report_packages() {
        let snap = test_snapshot();
        let md = render_audit(&snap);
        assert!(md.contains("## Packages"));
        assert!(md.contains("httpd"));
    }

    #[test]
    fn test_audit_report_empty_snapshot() {
        let snap = InspectionSnapshot::new();
        let md = render_audit(&snap);
        assert!(md.contains("# Audit Report"));
    }

    #[test]
    fn test_audit_report_partial_completeness() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Partial {
            incomplete_sections: vec![InspectorId::Config, InspectorId::Services],
            reason: "timeout during inspection".into(),
        };
        let md = render_audit(&snap);
        assert!(
            md.contains("## Incomplete Sections"),
            "must contain Incomplete Sections heading"
        );
        assert!(md.contains("config"), "must list config section");
        assert!(md.contains("services"), "must list services section");
        assert!(
            md.contains("timeout during inspection"),
            "must include the reason"
        );
    }

    #[test]
    fn test_audit_report_full_completeness_no_section() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Full;
        let md = render_audit(&snap);
        assert!(
            !md.contains("Incomplete Sections"),
            "full completeness must not produce Incomplete Sections"
        );
    }
}
