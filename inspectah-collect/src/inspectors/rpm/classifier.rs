use super::parser::rpmvercmp;
use inspectah_core::types::rpm::{
    PackageEntry, PackageState, VersionChange, VersionChangeDirection,
};
use std::cmp::Ordering;
use std::collections::HashMap;

pub struct ClassificationResult {
    pub packages: Vec<PackageEntry>,
    pub version_changes: Vec<VersionChange>,
}

/// Normalize epoch: treat empty string as "0" so that baseline entries
/// with epoch=None (deserialized as "") match host entries with epoch="0".
fn norm_epoch(e: &str) -> &str {
    if e.is_empty() { "0" } else { e }
}

pub fn classify_packages(
    host: &[PackageEntry],
    baseline: &HashMap<String, PackageEntry>,
) -> ClassificationResult {
    let mut version_changes = Vec::new();

    let packages = host
        .iter()
        .map(|pkg| {
            let key = format!("{}.{}", pkg.name, pkg.arch);
            let state = match baseline.get(&key) {
                None => PackageState::Added,
                Some(base) => {
                    let epoch_cmp =
                        rpmvercmp(norm_epoch(&pkg.epoch), norm_epoch(&base.epoch));
                    let ver_cmp = rpmvercmp(&pkg.version, &base.version);
                    let rel_cmp = rpmvercmp(&pkg.release, &base.release);
                    if epoch_cmp == Ordering::Equal
                        && ver_cmp == Ordering::Equal
                        && rel_cmp == Ordering::Equal
                    {
                        // Same EVR — package matches baseline. Keep as Added.
                        // The attention model assigns PackageBaselineMatch.
                        PackageState::Added
                    } else {
                        let direction = if epoch_cmp != Ordering::Equal {
                            if epoch_cmp == Ordering::Greater {
                                VersionChangeDirection::Upgrade
                            } else {
                                VersionChangeDirection::Downgrade
                            }
                        } else if ver_cmp != Ordering::Equal {
                            if ver_cmp == Ordering::Greater {
                                VersionChangeDirection::Upgrade
                            } else {
                                VersionChangeDirection::Downgrade
                            }
                        } else if rel_cmp == Ordering::Greater {
                            VersionChangeDirection::Upgrade
                        } else {
                            VersionChangeDirection::Downgrade
                        };

                        version_changes.push(VersionChange {
                            name: pkg.name.clone(),
                            arch: pkg.arch.clone(),
                            host_version: format!("{}-{}", pkg.version, pkg.release),
                            base_version: format!("{}-{}", base.version, base.release),
                            host_epoch: pkg.epoch.clone(),
                            base_epoch: base.epoch.clone(),
                            direction,
                        });
                        PackageState::Modified
                    }
                }
            };
            // All host packages get include: true — attention model decides visibility
            PackageEntry {
                state,
                include: true,
                ..pkg.clone()
            }
        })
        .collect();

    ClassificationResult {
        packages,
        version_changes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, release: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            epoch: "0".to_string(),
            version: version.to_string(),
            release: release.to_string(),
            arch: "x86_64".to_string(),
            state: PackageState::Added,
            include: true,
            ..Default::default()
        }
    }

    fn baseline_with(packages: &[(&str, &str, &str)]) -> HashMap<String, PackageEntry> {
        packages
            .iter()
            .map(|(name, version, release)| {
                let pkg = pkg(name, version, release);
                let key = format!("{}.x86_64", name);
                (key, pkg)
            })
            .collect()
    }

    #[test]
    fn test_classify_added_package() {
        let host = vec![pkg("httpd", "2.4.57", "5.el9")];
        let baseline: HashMap<String, PackageEntry> = HashMap::new();
        let result = classify_packages(&host, &baseline);
        assert_eq!(result.packages[0].state, PackageState::Added);
        assert!(result.packages[0].include);
    }

    #[test]
    fn test_classify_same_evr_is_added() {
        let host = vec![pkg("bash", "5.2.26", "3.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result.packages[0].state, PackageState::Added);
        assert!(result.packages[0].include);
    }

    #[test]
    fn test_classify_modified_version() {
        let host = vec![pkg("bash", "5.2.26", "4.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result.packages[0].state, PackageState::Modified);
        assert!(result.packages[0].include);
    }

    #[test]
    fn test_classify_empty_baseline_all_added() {
        let host = vec![pkg("httpd", "2.4.57", "5.el9"), pkg("vim", "9.0", "1.el9")];
        let result = classify_packages(&host, &HashMap::new());
        assert!(result.packages.iter().all(|p| p.state == PackageState::Added));
        assert!(result.packages.iter().all(|p| p.include));
    }

    #[test]
    fn test_classify_duplicate_nevra() {
        let host = vec![
            pkg("bash", "5.2.26", "3.el9"),
            pkg("bash", "5.2.26", "3.el9"),
        ];
        let result = classify_packages(&host, &HashMap::new());
        assert_eq!(result.packages.len(), 2);
    }

    /// Verify that BaselineData.packages converted to the classifier's
    /// HashMap<String, PackageEntry> format produces correct classification.
    #[test]
    fn test_classify_with_baseline_data_converted_packages() {
        use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
        use std::collections::HashMap as StdHashMap;

        // Simulate what build_baseline() does: convert BaselinePackageEntry -> PackageEntry
        let mut baseline_packages = StdHashMap::new();
        baseline_packages.insert(
            "bash".to_string(),
            BaselinePackageEntry {
                name: "bash".to_string(),
                epoch: Some("0".to_string()),
                version: "5.2.26".to_string(),
                release: "3.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );
        baseline_packages.insert(
            "glibc".to_string(),
            BaselinePackageEntry {
                name: "glibc".to_string(),
                epoch: None, // epoch None -> default to ""
                version: "2.34".to_string(),
                release: "100.el9".to_string(),
                arch: "x86_64".to_string(),
            },
        );

        let _baseline_data = BaselineData {
            image_digest: "sha256:abc123".to_string(),
            packages: baseline_packages.clone(),
            extracted_at: "2026-05-17T00:00:00Z".to_string(),
        };

        // Convert BaselinePackageEntry -> classifier's HashMap<String, PackageEntry>
        // (mirrors build_baseline logic)
        let classifier_baseline: HashMap<String, PackageEntry> = baseline_packages
            .values()
            .map(|bp| {
                let key = format!("{}.{}", bp.name, bp.arch);
                let pkg = PackageEntry {
                    name: bp.name.clone(),
                    epoch: bp.epoch.clone().unwrap_or_default(),
                    version: bp.version.clone(),
                    release: bp.release.clone(),
                    arch: bp.arch.clone(),
                    state: PackageState::BaseImageOnly,
                    include: false,
                    ..Default::default()
                };
                (key, pkg)
            })
            .collect();

        // Host has: bash (same version), glibc (upgraded), httpd (new)
        let host = vec![
            PackageEntry {
                name: "bash".to_string(),
                epoch: "0".to_string(),
                version: "5.2.26".to_string(),
                release: "3.el9".to_string(),
                arch: "x86_64".to_string(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "glibc".to_string(),
                epoch: "0".to_string(),
                version: "2.34".to_string(),
                release: "101.el9".to_string(), // upgraded release
                arch: "x86_64".to_string(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
            PackageEntry {
                name: "httpd".to_string(),
                epoch: "0".to_string(),
                version: "2.4.57".to_string(),
                release: "5.el9".to_string(),
                arch: "x86_64".to_string(),
                state: PackageState::Added,
                include: true,
                ..Default::default()
            },
        ];

        let result = classify_packages(&host, &classifier_baseline);

        // bash: same EVR -> Added (baseline match handled by attention model), include: true
        assert_eq!(result.packages[0].state, PackageState::Added);
        assert!(result.packages[0].include);

        // glibc: different release -> Modified, include: true
        assert_eq!(result.packages[1].state, PackageState::Modified);
        assert!(result.packages[1].include);

        // httpd: not in baseline -> Added, include: true
        assert_eq!(result.packages[2].state, PackageState::Added);
        assert!(result.packages[2].include);
    }

    #[test]
    fn test_classify_epoch_change_is_modified() {
        let mut host_pkg = pkg("openssl", "3.0.7", "1.el9");
        host_pkg.epoch = "2".to_string();
        let host = vec![host_pkg];
        let baseline = baseline_with(&[("openssl", "3.0.7", "1.el9")]);
        // baseline has epoch "0" via pkg() helper
        let result = classify_packages(&host, &baseline);
        assert_eq!(
            result.packages[0].state,
            PackageState::Modified,
            "epoch change must be Modified"
        );
    }

    #[test]
    fn test_classify_modified_emits_version_change() {
        let host = vec![pkg("bash", "5.2.26", "4.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result.version_changes.len(), 1);
        assert_eq!(result.version_changes[0].name, "bash");
        assert_eq!(result.version_changes[0].host_version, "5.2.26-4.el9");
        assert_eq!(result.version_changes[0].base_version, "5.2.26-3.el9");
        assert!(matches!(
            result.version_changes[0].direction,
            VersionChangeDirection::Upgrade
        ));
    }

    #[test]
    fn test_classify_modified_downgrade_emits_version_change() {
        let host = vec![pkg("bash", "5.2.26", "3.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "4.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result.version_changes.len(), 1);
        assert!(matches!(
            result.version_changes[0].direction,
            VersionChangeDirection::Downgrade
        ));
    }

    #[test]
    fn test_classify_same_evr_no_version_change() {
        let host = vec![pkg("bash", "5.2.26", "3.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert!(result.version_changes.is_empty());
    }

    #[test]
    fn test_classify_added_no_baseline_no_version_change() {
        let host = vec![pkg("httpd", "2.4.57", "5.el9")];
        let result = classify_packages(&host, &HashMap::new());
        assert!(result.version_changes.is_empty());
    }

    #[test]
    fn test_classify_epoch_change_emits_version_change() {
        let mut host_pkg = pkg("glibc", "2.34", "100.el9");
        host_pkg.epoch = "1".into();
        let mut base_pkg = pkg("glibc", "2.34", "100.el9");
        base_pkg.epoch = "0".into();
        let baseline = HashMap::from([("glibc.x86_64".to_string(), base_pkg)]);
        let result = classify_packages(&[host_pkg], &baseline);
        assert_eq!(result.version_changes.len(), 1);
        assert_eq!(result.version_changes[0].host_epoch, "1");
        assert_eq!(result.version_changes[0].base_epoch, "0");
        assert!(matches!(
            result.version_changes[0].direction,
            VersionChangeDirection::Upgrade
        ));
    }

    #[test]
    fn test_classify_empty_vs_zero_epoch_is_not_drift() {
        let mut host_pkg = pkg("kernel", "5.14.0", "503.el9");
        host_pkg.epoch = "0".into();
        let base_pkg = PackageEntry {
            name: "kernel".into(),
            epoch: String::new(),
            version: "5.14.0".into(),
            release: "503.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::BaseImageOnly,
            include: false,
            ..Default::default()
        };
        let baseline = HashMap::from([("kernel.x86_64".to_string(), base_pkg)]);
        let result = classify_packages(&[host_pkg], &baseline);
        assert_eq!(result.packages[0].state, PackageState::Added);
        assert!(
            result.version_changes.is_empty(),
            "'0' vs '' epoch must not produce a VersionChange after normalization"
        );
    }
}
