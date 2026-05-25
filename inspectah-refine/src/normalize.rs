use crate::types::{AttentionLevel, RefineError, RefinedConfig, RefinedPackage};
use inspectah_core::baseline::INCOMPATIBLE_SERVICES;
use inspectah_core::snapshot::InspectionSnapshot;
use serde_json::Value;

fn canonical_package_id(name: &str, arch: &str) -> String {
    format!("{name}.{arch}")
}

/// Load a raw JSON snapshot for refine, applying presence-aware defaulting.
///
/// Before typed deserialization, walks raw JSON arrays and patches any
/// entry lacking an `include` field by inserting `"include": true`.
/// Entries with an existing `"include": false` are untouched. This
/// resolves the `serde(default)` bool ambiguity where absent and
/// explicit-false collapse to the same `false`.
///
/// Enforces the same schema version gate as `InspectionSnapshot::load()`
/// (current SCHEMA_VERSION only) by round-tripping through the patched
/// JSON string and calling `load()`.
pub fn load_for_refine(raw_json: &str) -> Result<InspectionSnapshot, RefineError> {
    let mut value: Value =
        serde_json::from_str(raw_json).map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    patch_missing_includes(&mut value);

    // Serialize patched Value back to string and use InspectionSnapshot::load()
    // which enforces schema_version == SCHEMA_VERSION.
    let patched_json =
        serde_json::to_string(&value).map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;
    let mut snap = InspectionSnapshot::load(&patched_json)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    normalize_incompatible_services(&mut snap);

    Ok(snap)
}

fn patch_missing_includes(value: &mut Value) {
    if let Some(rpm) = value.get_mut("rpm") {
        patch_array_includes(rpm, "packages_added");
        patch_array_includes(rpm, "base_image_only");
    }
    if let Some(config) = value.get_mut("config") {
        patch_array_includes(config, "files");
    }
}

fn patch_array_includes(parent: &mut Value, array_key: &str) {
    if let Some(Value::Array(entries)) = parent.get_mut(array_key) {
        for entry in entries {
            if let Value::Object(map) = entry
                && !map.contains_key("include")
            {
                map.insert("include".into(), Value::Bool(true));
            }
        }
    }
}

/// Collect canonical package identities from a leaf_dep_tree JSON value,
/// excluding packages that are themselves top-level keys in the tree.
///
/// The tree is a JSON object
/// `{ "leaf.name.arch": ["dep1.name.arch", "dep2.name.arch", ...], ... }`.
/// A top-level key means the package was identified as a leaf in its own
/// right (with its own dependency subtree). In fleet/merged snapshots a
/// package can be both a leaf on one host and a dependency on another.
/// Only identities that appear exclusively as dependencies (not as
/// top-level keys) should be subtracted from the leaf set.
fn collect_dep_tree_names(tree: &serde_json::Value) -> std::collections::HashSet<&str> {
    let mut deps = std::collections::HashSet::new();
    if let serde_json::Value::Object(map) = tree {
        for (_leaf, dep_list) in map {
            if let serde_json::Value::Array(arr) = dep_list {
                for item in arr {
                    if let serde_json::Value::String(name) = item {
                        deps.insert(name.as_str());
                    }
                }
            }
        }
        // Retain only deps that are NOT top-level keys. A top-level key means
        // the package was independently identified as a leaf — it should stay
        // in the leaf set even if another leaf lists it as a dependency.
        deps.retain(|name| !map.contains_key(*name));
    }
    deps
}

/// Materialize tier-aware include defaults for packages.
///
/// Baseline subtraction: Tier 1 (Routine/baseline match) packages are
/// included because the package manager handles them. Tier 2
/// (Informational/user-added) packages are included only if they are
/// leaf packages (or if leaf data is unavailable). Tier 3
/// (NeedsReview/unknown provenance) packages are excluded.
pub fn normalize_package_defaults(snapshot: &mut InspectionSnapshot, packages: &[RefinedPackage]) {
    let rpm = match snapshot.rpm.as_mut() {
        Some(r) => r,
        None => return,
    };

    let leaf_set: Option<std::collections::HashSet<&str>> = rpm.leaf_packages.as_ref().map(|lp| {
        let mut set: std::collections::HashSet<&str> = lp.iter().map(|s| s.as_str()).collect();
        // Exclude transitive dependencies of leaf packages.
        // leaf_dep_tree maps each leaf identity to an array of its dep
        // identities. Any package that appears as a dep of another leaf is
        // not a true leaf — it was pulled in automatically.
        let all_deps = collect_dep_tree_names(&rpm.leaf_dep_tree);
        set.retain(|name| !all_deps.contains(*name));
        set
    });

    for (i, refined) in packages.iter().enumerate() {
        if i >= rpm.packages_added.len() {
            break;
        }
        let primary_level = refined
            .attention
            .first()
            .map(|t| t.level)
            .unwrap_or(AttentionLevel::Routine);
        match primary_level {
            AttentionLevel::Routine => {
                rpm.packages_added[i].include = true;
            }
            AttentionLevel::Informational => {
                let package_id = canonical_package_id(
                    rpm.packages_added[i].name.as_str(),
                    rpm.packages_added[i].arch.as_str(),
                );
                let is_leaf = match &leaf_set {
                    Some(set) => set.contains(package_id.as_str()),
                    None => true,
                };
                rpm.packages_added[i].include = is_leaf;
            }
            AttentionLevel::NeedsReview => {
                rpm.packages_added[i].include = false;
            }
        }
    }
}

/// Exclude systemd services incompatible with immutable /usr.
///
/// Walks `services.state_changes` and sets `include = false` on units
/// listed in `INCOMPATIBLE_SERVICES`. Also removes those units from
/// `services.enabled_units`.
pub fn normalize_incompatible_services(snapshot: &mut InspectionSnapshot) {
    let services = match snapshot.services.as_mut() {
        Some(s) => s,
        None => return,
    };

    let incompatible_units: Vec<&str> = INCOMPATIBLE_SERVICES.iter().map(|e| e.unit).collect();

    for sc in &mut services.state_changes {
        if incompatible_units.contains(&sc.unit.as_str()) {
            sc.include = false;
            sc.attention_reason = Some("service-image-mode-incompatible".to_string());
        }
    }

    services
        .enabled_units
        .retain(|u| !incompatible_units.contains(&u.as_str()));
}

/// Materialize tier-aware include defaults for config files.
///
/// Baseline subtraction: Tier 1 (Routine) configs — RpmOwnedDefault
/// and BaselineMatch — are NOT copied because the package manager or
/// base image already provides them. Tier 2 (Informational) configs
/// are included unless orphaned. Tier 3 (NeedsReview/user-modified)
/// configs are always included.
pub fn normalize_config_defaults(snapshot: &mut InspectionSnapshot, configs: &[RefinedConfig]) {
    let config = match snapshot.config.as_mut() {
        Some(c) => c,
        None => return,
    };
    for (i, refined) in configs.iter().enumerate() {
        if i >= config.files.len() {
            break;
        }
        let primary_level = refined
            .attention
            .first()
            .map(|t| t.level)
            .unwrap_or(AttentionLevel::Routine);
        match primary_level {
            AttentionLevel::Routine => {
                // Tier 1: NOT copied — package manager handles these
                config.files[i].include = false;
            }
            AttentionLevel::Informational => {
                config.files[i].include = !matches!(
                    config.files[i].kind,
                    inspectah_core::types::config::ConfigFileKind::Orphaned
                );
            }
            AttentionLevel::NeedsReview => {
                // Tier 3: user-customized, include
                config.files[i].include = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::services::{ServiceSection, ServiceStateChange};

    /// Helper: build a snapshot with a ServiceSection.
    fn snap_with_services(
        state_changes: Vec<ServiceStateChange>,
        enabled_units: Vec<String>,
    ) -> InspectionSnapshot {
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            services: Some(ServiceSection {
                state_changes,
                enabled_units,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn sc(unit: &str, include: bool) -> ServiceStateChange {
        use inspectah_core::types::services::{PresetDefault, ServiceUnitState};
        ServiceStateChange {
            unit: unit.to_string(),
            current_state: ServiceUnitState::Enabled,
            default_state: Some(PresetDefault::Disable),
            include,
            owning_package: None,
            fleet: None,
            attention_reason: None,
        }
    }

    #[test]
    fn incompatible_service_flagged_include_false() {
        let mut snap = snap_with_services(vec![sc("dnf-makecache.service", true)], vec![]);
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(!services.state_changes[0].include);
    }

    #[test]
    fn compatible_service_not_flagged() {
        let mut snap = snap_with_services(vec![sc("httpd.service", true)], vec![]);
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(services.state_changes[0].include);
    }

    #[test]
    fn incompatible_service_removed_from_enabled_units() {
        let mut snap = snap_with_services(
            vec![],
            vec![
                "dnf-makecache.service".into(),
                "httpd.service".into(),
                "packagekit.service".into(),
            ],
        );
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert_eq!(services.enabled_units, vec!["httpd.service".to_string()]);
    }

    #[test]
    fn all_incompatible_services_flagged() {
        let mut snap = snap_with_services(
            vec![
                sc("dnf-makecache.service", true),
                sc("dnf-makecache.timer", true),
                sc("packagekit.service", true),
                sc("packagekit-offline-update.service", true),
                sc("httpd.service", true),
                sc("sshd.service", true),
            ],
            vec![
                "dnf-makecache.service".into(),
                "dnf-makecache.timer".into(),
                "packagekit.service".into(),
                "packagekit-offline-update.service".into(),
                "httpd.service".into(),
                "sshd.service".into(),
            ],
        );
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();

        // state_changes: incompatible ones excluded, compatible ones untouched
        assert!(!services.state_changes[0].include); // dnf-makecache.service
        assert!(!services.state_changes[1].include); // dnf-makecache.timer
        assert!(!services.state_changes[2].include); // packagekit.service
        assert!(!services.state_changes[3].include); // packagekit-offline-update.service
        assert!(services.state_changes[4].include); // httpd.service
        assert!(services.state_changes[5].include); // sshd.service

        // enabled_units: only compatible ones remain
        assert_eq!(
            services.enabled_units,
            vec!["httpd.service".to_string(), "sshd.service".to_string()]
        );
    }

    #[test]
    fn no_services_section_is_noop() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            ..Default::default()
        };
        // Should not panic.
        normalize_incompatible_services(&mut snap);
        assert!(snap.services.is_none());
    }

    #[test]
    fn already_excluded_service_stays_excluded() {
        let mut snap = snap_with_services(vec![sc("dnf-makecache.service", false)], vec![]);
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(!services.state_changes[0].include);
    }

    #[test]
    fn collect_dep_tree_empty() {
        let tree = serde_json::json!({});
        let deps = super::collect_dep_tree_names(&tree);
        assert!(deps.is_empty());
    }

    #[test]
    fn collect_dep_tree_with_deps() {
        let tree = serde_json::json!({
            "git": ["git-core", "perl-Git", "perl-libs"],
            "vim": ["vim-common", "gpm-libs"]
        });
        let deps = super::collect_dep_tree_names(&tree);
        assert_eq!(deps.len(), 5);
        assert!(deps.contains("git-core"));
        assert!(deps.contains("perl-Git"));
        assert!(deps.contains("perl-libs"));
        assert!(deps.contains("vim-common"));
        assert!(deps.contains("gpm-libs"));
    }

    #[test]
    fn collect_dep_tree_skips_non_strings() {
        let tree = serde_json::json!({
            "vim": ["vim-common", 42, null]
        });
        let deps = super::collect_dep_tree_names(&tree);
        assert_eq!(deps.len(), 1);
        assert!(deps.contains("vim-common"));
    }

    #[test]
    fn collect_dep_tree_null_value() {
        let tree = serde_json::Value::Null;
        let deps = super::collect_dep_tree_names(&tree);
        assert!(deps.is_empty());
    }

    #[test]
    fn leaf_set_excludes_transitive_deps() {
        use crate::types::{AttentionLevel, AttentionReason, AttentionTag, RefinedPackage};
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "perl-Git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "git-core".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                ],
                leaf_packages: Some(vec![
                    "git.x86_64".into(),
                    "perl-Git.x86_64".into(),
                    "git-core.x86_64".into(),
                ]),
                leaf_dep_tree: serde_json::json!({
                    "git.x86_64": ["perl-Git.x86_64", "git-core.x86_64"]
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        // All packages are Tier 2 (Informational) — no baseline
        let packages: Vec<RefinedPackage> = snap
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .map(|entry| RefinedPackage {
                entry: entry.clone(),
                attention: vec![AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::PackageProvenanceUnavailable,
                    detail: None,
                }],
                fleet_attention: None,
            })
            .collect();

        normalize_package_defaults(&mut snap, &packages);
        let rpm = snap.rpm.as_ref().unwrap();

        // git is a true leaf — should be included
        assert!(
            rpm.packages_added[0].include,
            "git should be included (true leaf)"
        );
        // perl-Git is a dep of git — should be excluded
        assert!(
            !rpm.packages_added[1].include,
            "perl-Git should be excluded (dep of git)"
        );
        // git-core is a dep of git — should be excluded
        assert!(
            !rpm.packages_added[2].include,
            "git-core should be excluded (dep of git)"
        );
    }

    #[test]
    fn fleet_leaf_also_dep_of_another_leaf_stays_included() {
        // Fleet/merged scenario: perl-Git is a top-level leaf (user installed
        // it directly on one host) AND appears as a dep of git. Because it has
        // its own entry in leaf_dep_tree, it should NOT be subtracted.
        use crate::types::{AttentionLevel, AttentionReason, AttentionTag, RefinedPackage};
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            rpm: Some(RpmSection {
                packages_added: vec![
                    PackageEntry {
                        name: "git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "perl-Git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "git-core".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        ..Default::default()
                    },
                ],
                leaf_packages: Some(vec![
                    "git.x86_64".into(),
                    "perl-Git.x86_64".into(),
                    "git-core.x86_64".into(),
                ]),
                leaf_dep_tree: serde_json::json!({
                    "git.x86_64": ["perl-Git.x86_64", "git-core.x86_64"],
                    "perl-Git.x86_64": ["perl-libs.x86_64"]
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let packages: Vec<RefinedPackage> = snap
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .map(|entry| RefinedPackage {
                entry: entry.clone(),
                attention: vec![AttentionTag {
                    level: AttentionLevel::Informational,
                    reason: AttentionReason::PackageProvenanceUnavailable,
                    detail: None,
                }],
                fleet_attention: None,
            })
            .collect();

        normalize_package_defaults(&mut snap, &packages);
        let rpm = snap.rpm.as_ref().unwrap();

        // git is a top-level leaf — included
        assert!(
            rpm.packages_added[0].include,
            "git should be included (top-level leaf)"
        );
        // perl-Git is BOTH a dep of git AND a top-level leaf — stays included
        assert!(
            rpm.packages_added[1].include,
            "perl-Git should be included (top-level leaf, even though also a dep of git)"
        );
        // git-core is only a dep, not a top-level key — excluded
        assert!(
            !rpm.packages_added[2].include,
            "git-core should be excluded (dep only, not a top-level leaf)"
        );
    }

    #[test]
    fn collect_dep_tree_excludes_top_level_keys() {
        // A dep that is also a top-level key should NOT appear in the
        // returned set — it's a confirmed leaf.
        let tree = serde_json::json!({
            "git.x86_64": ["perl-Git.x86_64", "git-core.x86_64"],
            "perl-Git.x86_64": ["perl-libs.x86_64"]
        });
        let deps = super::collect_dep_tree_names(&tree);
        // perl-libs.x86_64 and git-core.x86_64 are pure deps.
        assert!(deps.contains("perl-libs.x86_64"));
        assert!(deps.contains("git-core.x86_64"));
        // perl-Git is a top-level key — should NOT be in the dep set
        assert!(
            !deps.contains("perl-Git.x86_64"),
            "perl-Git.x86_64 is a top-level key and should not be in the subtraction set"
        );
        assert_eq!(deps.len(), 2);
    }
}
