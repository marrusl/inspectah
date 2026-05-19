use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManualFollowUpReason {
    LocalInstall,
    NoRepo,
    MissingSourceRepo,
}

fn package_display_name(pkg: &PackageEntry) -> String {
    if pkg.arch.is_empty() {
        pkg.name.clone()
    } else {
        format!("{}.{}", pkg.name, pkg.arch)
    }
}

fn package_state_label(pkg: &PackageEntry) -> String {
    serde_json::to_string(&pkg.state)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn manual_follow_up_reason(pkg: &PackageEntry) -> Option<ManualFollowUpReason> {
    match pkg.state {
        PackageState::LocalInstall => Some(ManualFollowUpReason::LocalInstall),
        PackageState::NoRepo => Some(ManualFollowUpReason::NoRepo),
        _ if pkg.source_repo.is_empty() => Some(ManualFollowUpReason::MissingSourceRepo),
        _ => None,
    }
}

/// Generate the TODO comment line for a package that cannot be installed via
/// `dnf install`. Returns `None` for packages that are directly installable.
///
/// Output matches the original containerfile renderer exactly — changing
/// wording here changes the Containerfile output.
pub(crate) fn manual_follow_up_line(pkg: &PackageEntry) -> Option<String> {
    match manual_follow_up_reason(pkg) {
        Some(ManualFollowUpReason::LocalInstall) => Some(format!(
            "# TODO: '{}' was installed locally (state: {}) \u{2014} provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        Some(ManualFollowUpReason::NoRepo) => Some(format!(
            "# TODO: '{}' has no repository source (state: {}) \u{2014} provide a .rpm or custom repo.",
            package_display_name(pkg),
            package_state_label(pkg)
        )),
        Some(ManualFollowUpReason::MissingSourceRepo) => Some(format!(
            "# TODO: '{}' has no recorded repository source \u{2014} verify how to reinstall it in the image.",
            package_display_name(pkg)
        )),
        None => None,
    }
}

/// Returns `true` when a package can be installed via `dnf install` — i.e.,
/// it has a known repository source and is not a local-install or no-repo
/// package.
pub fn is_package_installable(pkg: &PackageEntry) -> bool {
    manual_follow_up_reason(pkg).is_none()
}

/// Compute the set of package *names* (not `name.arch`) that the target
/// image is expected to contain. This merges baseline package names with
/// user-included added packages.
pub fn effective_target_packages(rpm: &RpmSection) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    if let Some(baseline) = &rpm.baseline_package_names {
        names.extend(baseline.iter().cloned());
    }
    names.extend(
        rpm.packages_added
            .iter()
            .filter(|pkg| pkg.include)
            .map(|pkg| pkg.name.clone()),
    );
    names
}
