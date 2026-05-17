use super::parser::rpmvercmp;
use inspectah_core::types::rpm::{PackageEntry, PackageState};
use std::collections::HashMap;

pub fn classify_packages(
    host: &[PackageEntry],
    baseline: &HashMap<String, PackageEntry>,
) -> Vec<PackageEntry> {
    host.iter()
        .map(|pkg| {
            let key = format!("{}.{}", pkg.name, pkg.arch);
            let state = match baseline.get(&key) {
                None => PackageState::Added,
                Some(base) => {
                    let epoch_cmp = rpmvercmp(&pkg.epoch, &base.epoch);
                    let ver_cmp = rpmvercmp(&pkg.version, &base.version);
                    let rel_cmp = rpmvercmp(&pkg.release, &base.release);
                    if epoch_cmp == std::cmp::Ordering::Equal
                        && ver_cmp == std::cmp::Ordering::Equal
                        && rel_cmp == std::cmp::Ordering::Equal
                    {
                        // Same EVR — package matches baseline. Keep as Added.
                        // The attention model assigns PackageBaselineMatch.
                        PackageState::Added
                    } else {
                        PackageState::Modified
                    }
                }
            };
            // All host packages get include: true — attention model decides visibility
            let include = true;
            PackageEntry {
                state,
                include,
                ..pkg.clone()
            }
        })
        .collect()
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
        assert_eq!(result[0].state, PackageState::Added);
        assert!(result[0].include);
    }

    #[test]
    fn test_classify_same_evr_is_added() {
        let host = vec![pkg("bash", "5.2.26", "3.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result[0].state, PackageState::Added);
        assert!(result[0].include);
    }

    #[test]
    fn test_classify_modified_version() {
        let host = vec![pkg("bash", "5.2.26", "4.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result[0].state, PackageState::Modified);
        assert!(result[0].include);
    }

    #[test]
    fn test_classify_empty_baseline_all_added() {
        let host = vec![pkg("httpd", "2.4.57", "5.el9"), pkg("vim", "9.0", "1.el9")];
        let result = classify_packages(&host, &HashMap::new());
        assert!(result.iter().all(|p| p.state == PackageState::Added));
        assert!(result.iter().all(|p| p.include));
    }

    #[test]
    fn test_classify_duplicate_nevra() {
        let host = vec![
            pkg("bash", "5.2.26", "3.el9"),
            pkg("bash", "5.2.26", "3.el9"),
        ];
        let result = classify_packages(&host, &HashMap::new());
        assert_eq!(result.len(), 2);
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
        assert_eq!(result[0].state, PackageState::Added);
        assert!(result[0].include);

        // glibc: different release -> Modified, include: true
        assert_eq!(result[1].state, PackageState::Modified);
        assert!(result[1].include);

        // httpd: not in baseline -> Added, include: true
        assert_eq!(result[2].state, PackageState::Added);
        assert!(result[2].include);
    }

    /// Verify that None epoch in BaselinePackageEntry defaults to empty
    /// string, matching the classifier's epoch comparison.
    #[test]
    fn test_classify_baseline_none_epoch_defaults_to_empty() {
        // Baseline package with epoch: None
        let mut baseline = HashMap::new();
        let key = "kernel.x86_64".to_string();
        baseline.insert(
            key,
            PackageEntry {
                name: "kernel".to_string(),
                epoch: String::new(), // None.unwrap_or_default() = ""
                version: "5.14.0".to_string(),
                release: "503.el9".to_string(),
                arch: "x86_64".to_string(),
                state: PackageState::BaseImageOnly,
                include: false,
                ..Default::default()
            },
        );

        // Host has kernel with epoch "0" (from rpm -qa which always emits epoch)
        let host = vec![PackageEntry {
            name: "kernel".to_string(),
            epoch: "0".to_string(),
            version: "5.14.0".to_string(),
            release: "503.el9".to_string(),
            arch: "x86_64".to_string(),
            state: PackageState::Added,
            include: true,
            ..Default::default()
        }];

        let result = classify_packages(&host, &baseline);
        // epoch "0" vs "" -> rpmvercmp should treat as Modified (they differ)
        assert_eq!(result[0].state, PackageState::Modified);
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
            result[0].state,
            PackageState::Modified,
            "epoch change must be Modified"
        );
    }
}
