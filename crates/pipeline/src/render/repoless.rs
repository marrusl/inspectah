use inspectah_core::snapshot::InspectionSnapshot;

/// Render Containerfile lines for repo-less RPM packages.
///
/// Cached RPMs: COPY + dnf localinstall (commented out by default —
/// pre-excluded in refine, user must explicitly include).
/// Missing RPMs: MANUAL comment block.
pub fn repoless_rpm_lines(snap: &InspectionSnapshot) -> Vec<String> {
    let rpm_section = match &snap.rpm {
        Some(r) => r,
        None => return Vec::new(),
    };

    let repoless: Vec<_> = rpm_section
        .packages_added
        .iter()
        .filter(|p| !p.repoless_annotation.is_empty())
        .collect();

    if repoless.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push("# === Repo-less RPM packages ===".into());

    for pkg in &repoless {
        let nevra = format!("{}-{}-{}.{}", pkg.name, pkg.version, pkg.release, pkg.arch);
        let rpm_filename = format!("{nevra}.rpm");

        if pkg.repoless_cached {
            if pkg.include {
                // User explicitly included — render active
                lines.push(format!(
                    "# Repo-less package: {} (cached RPM, no repository provenance)",
                    pkg.name
                ));
                lines.push(
                    "# WARNING: This package has no upstream repo and no GPG verification.".into(),
                );
                lines.push(
                    "# It was found in the local dnf cache. Updates must be managed manually."
                        .into(),
                );
                lines.push(format!("COPY repoless-packages/{rpm_filename} /tmp/"));
                lines.push(format!("RUN dnf localinstall -y /tmp/{rpm_filename} \\"));
                lines.push(format!("    && rm /tmp/{rpm_filename}"));
            } else {
                // Pre-excluded — render commented out
                lines.push(format!(
                    "# Repo-less package: {} (cached RPM, no repository provenance)",
                    pkg.name
                ));
                lines.push(
                    "# WARNING: This package has no upstream repo and no GPG verification.".into(),
                );
                lines.push("# Pre-excluded — uncomment after verifying provenance:".into());
                lines.push(format!("# COPY repoless-packages/{rpm_filename} /tmp/"));
                lines.push(format!("# RUN dnf localinstall -y /tmp/{rpm_filename} \\"));
                lines.push(format!("#     && rm /tmp/{rpm_filename}"));
            }
        } else {
            // No cached RPM — manual resolution
            lines.push(format!(
                "# MANUAL: {} (no repo source, RPM not in cache)",
                pkg.name
            ));
            lines.push(
                "# Provide the RPM via the refine UI upload, add a repo, or uncomment:".into(),
            );
            lines.push(format!("# RUN dnf install {}", pkg.name));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::rpm::{PackageEntry, RpmSection};

    fn test_snapshot_with_repoless(packages: Vec<PackageEntry>) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::default();
        snap.rpm = Some(RpmSection {
            packages_added: packages,
            ..Default::default()
        });
        snap
    }

    #[test]
    fn cached_rpm_pre_excluded_renders_commented() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: false,
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("# COPY repoless-packages/"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("# RUN dnf localinstall"))
        );
    }

    #[test]
    fn cached_rpm_user_included_renders_active() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: true,
            repoless_cached: true,
            repoless_annotation: "No repo source — cached RPM bundled".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("COPY repoless-packages/"))
        );
        assert!(lines.iter().any(|l| l.starts_with("RUN dnf localinstall")));
    }

    #[test]
    fn missing_rpm_renders_manual_block() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "custom-tool".into(),
            version: "1.2.3".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: String::new(),
            include: false,
            repoless_cached: false,
            repoless_annotation: "No repo source — manual resolution needed".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("MANUAL: custom-tool")));
        assert!(lines.iter().any(|l| l.contains("dnf install custom-tool")));
    }

    #[test]
    fn packages_with_repo_not_rendered() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "httpd".into(),
            source_repo: "appstream".into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.is_empty());
    }

    #[test]
    fn disabled_repo_package_renders_as_repoless() {
        let snap = test_snapshot_with_repoless(vec![PackageEntry {
            name: "internal-tool".into(),
            version: "2.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            source_repo: "internal-tools".into(), // non-empty but disabled
            include: false,
            repoless_cached: true,
            repoless_annotation:
                "No repo source — repo 'internal-tools' not in enabled repos — cached RPM bundled"
                    .into(),
            ..Default::default()
        }]);
        let lines = repoless_rpm_lines(&snap);
        assert!(lines.iter().any(|l| l.contains("Repo-less package")));
    }
}
