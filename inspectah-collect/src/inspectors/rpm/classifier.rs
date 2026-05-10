use inspectah_core::types::rpm::{PackageEntry, PackageState};
use super::parser::rpmvercmp;
use std::collections::HashMap;

pub fn classify_packages(
    host: &[PackageEntry],
    baseline: &HashMap<String, PackageEntry>,
) -> Vec<PackageEntry> {
    host.iter().map(|pkg| {
        let key = format!("{}.{}", pkg.name, pkg.arch);
        let state = match baseline.get(&key) {
            None => PackageState::Added,
            Some(base) => {
                let ver_cmp = rpmvercmp(&pkg.version, &base.version);
                let rel_cmp = rpmvercmp(&pkg.release, &base.release);
                if ver_cmp == std::cmp::Ordering::Equal && rel_cmp == std::cmp::Ordering::Equal {
                    PackageState::BaseImageOnly
                } else {
                    PackageState::Modified
                }
            }
        };
        let include = state != PackageState::BaseImageOnly;
        PackageEntry {
            state,
            include,
            ..pkg.clone()
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, release: &str) -> PackageEntry {
        PackageEntry {
            name: name.to_string(),
            version: version.to_string(),
            release: release.to_string(),
            arch: "x86_64".to_string(),
            state: PackageState::Added,
            include: true,
            ..Default::default()
        }
    }

    fn baseline_with(packages: &[(&str, &str, &str)]) -> HashMap<String, PackageEntry> {
        packages.iter().map(|(name, version, release)| {
            let pkg = pkg(name, version, release);
            let key = format!("{}.x86_64", name);
            (key, pkg)
        }).collect()
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
    fn test_classify_base_image_only() {
        let host = vec![pkg("bash", "5.2.26", "3.el9")];
        let baseline = baseline_with(&[("bash", "5.2.26", "3.el9")]);
        let result = classify_packages(&host, &baseline);
        assert_eq!(result[0].state, PackageState::BaseImageOnly);
        assert!(!result[0].include);
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
}
