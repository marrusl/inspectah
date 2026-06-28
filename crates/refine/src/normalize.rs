use crate::types::{RefineError, RefinedConfig, RefinedPackage, TriageBucket, TriageReason};
use inspectah_core::baseline::INCOMPATIBLE_SERVICES;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_pipeline::render::language_packages::is_language_env;
use serde_json::Value;

fn is_anaconda_classified(reason: &TriageReason) -> bool {
    matches!(
        reason,
        TriageReason::PackagePlatformPlumbing
            | TriageReason::PackageInstallerDefault
            | TriageReason::PackageInstallerPromotedService
            | TriageReason::PackageInstallerPromotedConfig
            | TriageReason::PackageInstallerAmbiguous
            | TriageReason::PackageInstallerEvidenceUnavailable
    )
}

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
    normalize_merge_hostile_configs(&mut snap);

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
/// right (with its own dependency subtree). In aggregate/merged snapshots a
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

        // Anaconda classifier already made tier-specific include/locked
        // decisions — respect them instead of applying generic bucket defaults.
        if is_anaconda_classified(&refined.triage.primary_reason) {
            rpm.packages_added[i].include = refined.entry.include;
            rpm.packages_added[i].locked = refined.entry.locked;
        } else {
            let bucket = refined.triage.bucket();
            match bucket {
                TriageBucket::Baseline => {
                    rpm.packages_added[i].include = true;
                }
                TriageBucket::Site => {
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
                TriageBucket::Investigate => {
                    rpm.packages_added[i].include = false;
                }
            }
        }

        // Aggregate prevalence gate: items not present on 100% of hosts
        // default to excluded. Universal items keep their tier-based default.
        // Applies after all classification paths, including anaconda.
        if let Some(ref agg) = rpm.packages_added[i].aggregate
            && agg.count < agg.total
        {
            rpm.packages_added[i].include = false;
        }
    }
}

/// Paths that fight bootc's /etc 3-way merge or carry host-specific state
/// that is never image-portable.
const MERGE_HOSTILE_PATHS: &[&str] = &["/etc/fstab", "/etc/crypttab"];

/// Lock merge-hostile configs and all fstab entries.
///
/// Fstab entries are host-specific mount state — never image-portable.
/// Config files matching `MERGE_HOSTILE_PATHS` fight bootc's /etc 3-way
/// merge and must not be included in a Containerfile.
pub fn normalize_merge_hostile_configs(snapshot: &mut InspectionSnapshot) {
    // Lock ALL fstab entries — host state, not image-portable
    if let Some(ref mut storage) = snapshot.storage {
        for entry in &mut storage.fstab_entries {
            entry.include = false;
            entry.locked = true;
            entry.attention_reason = Some("host state — not image-portable".into());
        }
    }
    // Lock /etc/crypttab (and any future merge-hostile paths) in config files
    if let Some(ref mut config) = snapshot.config {
        for file in &mut config.files {
            if MERGE_HOSTILE_PATHS.contains(&file.path.as_str()) {
                file.include = false;
                file.locked = true;
                file.attention_reason =
                    Some("merge-hostile — fights bootc /etc 3-way merge".into());
            }
        }
    }
}

/// Exclude systemd services incompatible with immutable /usr.
///
/// Walks `services.state_changes` and sets `include = false`, `locked = true`
/// on units listed in `INCOMPATIBLE_SERVICES`. Also locks any drop-ins
/// belonging to those units and removes the units from `services.enabled_units`.
pub fn normalize_incompatible_services(snapshot: &mut InspectionSnapshot) {
    let services = match snapshot.services.as_mut() {
        Some(s) => s,
        None => return,
    };

    let incompatible_units: Vec<&str> = INCOMPATIBLE_SERVICES.iter().map(|e| e.unit).collect();

    // Lock incompatible services
    for sc in &mut services.state_changes {
        if incompatible_units.contains(&sc.unit.as_str()) {
            sc.include = false;
            sc.locked = true;
            sc.attention_reason = Some("service-image-mode-incompatible".to_string());
        }
    }

    // Lock drop-ins owned by incompatible services
    for di in &mut services.drop_ins {
        if incompatible_units.contains(&di.unit.as_str()) {
            di.include = false;
            di.locked = true;
            di.attention_reason = Some("parent service image-mode incompatible".to_string());
        }
    }

    services
        .enabled_units
        .retain(|u| !incompatible_units.contains(&u.as_str()));
}

/// Exclude inspectah's own COPR repo file from the RPM repo files list.
///
/// The inspectah tool's own COPR repo definition should never be carried
/// over to the target image. This is the RPM-level counterpart to the
/// config-level filtering in `normalize_config_defaults`.
pub fn normalize_inspectah_repo_files(snapshot: &mut InspectionSnapshot) {
    let rpm = match snapshot.rpm.as_mut() {
        Some(r) => r,
        None => return,
    };
    for rf in &mut rpm.repo_files {
        if is_inspectah_repo_file(&rf.path) {
            rf.include = false;
        }
    }
}

/// Materialize confidence-based include defaults for language environments.
///
/// High-confidence items (lockfile-backed, RPM-filtered) default to included.
/// Medium/low-confidence items default to excluded — users must explicitly
/// include them. Implements the spec's provenance trust gate.
pub fn normalize_language_env_defaults(snapshot: &mut InspectionSnapshot) {
    let nrs = match snapshot.non_rpm_software.as_mut() {
        Some(n) => n,
        None => return,
    };
    for item in &mut nrs.items {
        if !is_language_env(item) {
            continue;
        }
        match item.confidence.as_str() {
            "high" => {
                // Leave include: true (default from serde)
            }
            "medium" | "low" => {
                item.include = false;
            }
            _ => {
                // Unknown/empty confidence — treat as low
                item.include = false;
            }
        }
    }
}

/// Materialize tier-aware include defaults for config files.
///
/// Baseline subtraction: Tier 1 (Routine) configs — RpmOwnedDefault
/// and BaselineMatch — are NOT copied because the package manager or
/// base image already provides them. Tier 2 (Informational) configs
/// are included unless orphaned. Tier 3 (NeedsReview/user-modified)
/// configs are always included.
/// Path segment that identifies inspectah's own COPR repo file.
/// Matches paths like `/etc/yum.repos.d/_copr:copr.fedorainfracloud.org:...:inspectah.repo`.
const INSPECTAH_COPR_MARKER: &str = "inspectah";

/// Returns true if a config file path is inspectah's own COPR repo definition.
/// These should never be carried over to the target image.
fn is_inspectah_repo_file(path: &str) -> bool {
    // Only repo files in yum.repos.d
    if !path.starts_with("/etc/yum.repos.d/") {
        return false;
    }
    let filename = path.rsplit('/').next().unwrap_or("");
    // COPR repo files start with `_copr:` or `_copr_`
    let is_copr = filename.starts_with("_copr");
    is_copr && filename.contains(INSPECTAH_COPR_MARKER)
}

pub fn normalize_config_defaults(snapshot: &mut InspectionSnapshot, configs: &[RefinedConfig]) {
    let config = match snapshot.config.as_mut() {
        Some(c) => c,
        None => return,
    };
    for (i, refined) in configs.iter().enumerate() {
        if i >= config.files.len() {
            break;
        }

        // Exclude inspectah's own COPR repo definition — the migration
        // tool should never carry its own repo into the target image.
        if is_inspectah_repo_file(&config.files[i].path) {
            config.files[i].include = false;
            config.files[i].locked = true;
            continue;
        }

        let bucket = refined.triage.bucket();
        match bucket {
            TriageBucket::Baseline => {
                // Baseline: NOT copied — package manager handles these
                config.files[i].include = false;
            }
            TriageBucket::Site => {
                config.files[i].include = !matches!(
                    config.files[i].kind,
                    inspectah_core::types::config::ConfigFileKind::Orphaned
                );
            }
            TriageBucket::Investigate => {
                // Investigate: user-customized, include
                config.files[i].include = true;
            }
        }

        // Aggregate prevalence gate: configs not present on 100% of hosts
        // default to excluded. Universal configs keep their tier-based default.
        if let Some(ref agg) = config.files[i].aggregate
            && agg.count < agg.total
        {
            config.files[i].include = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::services::{ServiceSection, ServiceStateChange, SystemdDropIn};

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
            locked: false,
            owning_package: None,
            aggregate: None,
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
    fn incompatible_service_is_locked() {
        let mut snap = snap_with_services(vec![sc("dnf-makecache.service", true)], vec![]);
        normalize_incompatible_services(&mut snap);
        let svc = &snap.services.as_ref().unwrap().state_changes[0];
        assert!(!svc.include);
        assert!(svc.locked);
        assert_eq!(
            svc.attention_reason.as_deref(),
            Some("service-image-mode-incompatible")
        );
    }

    #[test]
    fn incompatible_service_dropin_is_locked() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            services: Some(ServiceSection {
                state_changes: vec![sc("dnf-makecache.service", true)],
                drop_ins: vec![SystemdDropIn {
                    unit: "dnf-makecache.service".into(),
                    path: "/etc/systemd/system/dnf-makecache.service.d/override.conf".into(),
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        // Service itself locked
        assert!(!services.state_changes[0].include);
        assert!(services.state_changes[0].locked);
        // Drop-in also locked
        assert!(!services.drop_ins[0].include);
        assert!(services.drop_ins[0].locked);
        assert_eq!(
            services.drop_ins[0].attention_reason.as_deref(),
            Some("parent service image-mode incompatible")
        );
    }

    #[test]
    fn unrelated_dropin_not_locked() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            services: Some(ServiceSection {
                state_changes: vec![sc("httpd.service", true)],
                drop_ins: vec![SystemdDropIn {
                    unit: "httpd.service".into(),
                    path: "/etc/systemd/system/httpd.service.d/limits.conf".into(),
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(services.drop_ins[0].include);
        assert!(!services.drop_ins[0].locked);
        assert!(services.drop_ins[0].attention_reason.is_none());
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
        use crate::types::{RefinedPackage, Triage, TriageBucket, TriageReason, TriageTag};
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

        fn site_package(entry: &PackageEntry) -> RefinedPackage {
            RefinedPackage {
                entry: entry.clone(),
                triage: TriageTag {
                    triage: Triage::SingleHost(TriageBucket::Site),
                    primary_reason: TriageReason::PackageProvenanceUnavailable,
                    annotations: Vec::new(),
                },
            }
        }

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
                        locked: false,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "perl-Git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "git-core".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        locked: false,
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

        // All packages are Site (user-added) — no baseline
        let packages: Vec<RefinedPackage> = snap
            .rpm
            .as_ref()
            .unwrap()
            .packages_added
            .iter()
            .map(site_package)
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
    fn aggregate_leaf_also_dep_of_another_leaf_stays_included() {
        use crate::types::{RefinedPackage, Triage, TriageBucket, TriageReason, TriageTag};
        use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

        fn site_package(entry: &PackageEntry) -> RefinedPackage {
            RefinedPackage {
                entry: entry.clone(),
                triage: TriageTag {
                    triage: Triage::SingleHost(TriageBucket::Site),
                    primary_reason: TriageReason::PackageProvenanceUnavailable,
                    annotations: Vec::new(),
                },
            }
        }

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
                        locked: false,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "perl-Git".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        locked: false,
                        ..Default::default()
                    },
                    PackageEntry {
                        name: "git-core".into(),
                        arch: "x86_64".into(),
                        state: PackageState::Added,
                        source_repo: "appstream".into(),
                        include: true,
                        locked: false,
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
            .map(site_package)
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

    // --- merge-hostile config tests ---

    fn snap_with_fstab(
        entries: Vec<inspectah_core::types::storage::FstabEntry>,
    ) -> InspectionSnapshot {
        use inspectah_core::types::storage::StorageSection;
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            storage: Some(StorageSection {
                fstab_entries: entries,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn snap_with_configs(
        files: Vec<inspectah_core::types::config::ConfigFileEntry>,
    ) -> InspectionSnapshot {
        use inspectah_core::types::config::ConfigSection;
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            config: Some(ConfigSection { files }),
            ..Default::default()
        }
    }

    #[test]
    fn merge_hostile_fstab_locked() {
        use inspectah_core::types::storage::FstabEntry;
        let mut snap = snap_with_fstab(vec![FstabEntry {
            device: "/dev/sda1".into(),
            mount_point: "/boot".into(),
            fstype: "xfs".into(),
            options: "defaults".into(),
            include: true,
            ..Default::default()
        }]);
        normalize_merge_hostile_configs(&mut snap);
        let entry = &snap.storage.as_ref().unwrap().fstab_entries[0];
        assert!(!entry.include);
        assert!(entry.locked);
        assert!(entry.attention_reason.is_some());
    }

    #[test]
    fn merge_hostile_crypttab_locked() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
        let mut snap = snap_with_configs(vec![ConfigFileEntry {
            path: "/etc/crypttab".into(),
            kind: ConfigFileKind::Unowned,
            include: true,
            ..Default::default()
        }]);
        normalize_merge_hostile_configs(&mut snap);
        let file = &snap.config.as_ref().unwrap().files[0];
        assert!(!file.include);
        assert!(file.locked);
        assert!(file.attention_reason.is_some());
    }

    #[test]
    fn non_hostile_config_untouched() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
        let mut snap = snap_with_configs(vec![ConfigFileEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            kind: ConfigFileKind::RpmOwnedModified,
            include: true,
            ..Default::default()
        }]);
        normalize_merge_hostile_configs(&mut snap);
        let file = &snap.config.as_ref().unwrap().files[0];
        assert!(file.include);
        assert!(!file.locked);
        assert!(file.attention_reason.is_none());
    }

    #[test]
    fn merge_hostile_no_storage_no_config_is_noop() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            ..Default::default()
        };
        normalize_merge_hostile_configs(&mut snap);
        assert!(snap.storage.is_none());
        assert!(snap.config.is_none());
    }

    #[test]
    fn merge_hostile_fstab_locked_also_sets_crypttab() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind, ConfigSection};
        use inspectah_core::types::storage::{FstabEntry, StorageSection};
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            storage: Some(StorageSection {
                fstab_entries: vec![FstabEntry {
                    device: "/dev/sda1".into(),
                    mount_point: "/boot".into(),
                    include: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            config: Some(ConfigSection {
                files: vec![
                    ConfigFileEntry {
                        path: "/etc/crypttab".into(),
                        kind: ConfigFileKind::Unowned,
                        include: true,
                        ..Default::default()
                    },
                    ConfigFileEntry {
                        path: "/etc/httpd/conf/httpd.conf".into(),
                        kind: ConfigFileKind::RpmOwnedModified,
                        include: true,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };
        normalize_merge_hostile_configs(&mut snap);

        // fstab locked
        let fstab = &snap.storage.as_ref().unwrap().fstab_entries[0];
        assert!(!fstab.include);
        assert!(fstab.locked);

        // crypttab locked
        let crypttab = &snap.config.as_ref().unwrap().files[0];
        assert!(!crypttab.include);
        assert!(crypttab.locked);

        // httpd.conf untouched
        let httpd = &snap.config.as_ref().unwrap().files[1];
        assert!(httpd.include);
        assert!(!httpd.locked);
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

    // --- language environment confidence-based defaulting tests ---

    fn snap_with_nonrpm(
        items: Vec<inspectah_core::types::nonrpm::NonRpmItem>,
    ) -> InspectionSnapshot {
        use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
        InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            non_rpm_software: Some(NonRpmSoftwareSection {
                items,
                env_files: vec![],
            }),
            ..Default::default()
        }
    }

    fn nonrpm_item(
        method: &str,
        confidence: &str,
        include: bool,
    ) -> inspectah_core::types::nonrpm::NonRpmItem {
        inspectah_core::types::nonrpm::NonRpmItem {
            method: method.to_string(),
            confidence: confidence.to_string(),
            include,
            ..Default::default()
        }
    }

    #[test]
    fn high_confidence_language_env_defaults_to_included() {
        let mut snap = snap_with_nonrpm(vec![
            nonrpm_item("npm lockfile", "high", true),
            nonrpm_item("python venv", "high", true),
            nonrpm_item("gem lockfile", "high", true),
        ]);
        normalize_language_env_defaults(&mut snap);
        let nrs = snap.non_rpm_software.as_ref().unwrap();
        assert!(
            nrs.items[0].include,
            "npm high-confidence should stay included"
        );
        assert!(
            nrs.items[1].include,
            "venv high-confidence should stay included"
        );
        assert!(
            nrs.items[2].include,
            "gem high-confidence should stay included"
        );
    }

    #[test]
    fn medium_confidence_language_env_defaults_to_excluded() {
        let mut snap = snap_with_nonrpm(vec![
            nonrpm_item("npm lockfile", "medium", true),
            nonrpm_item("pip dist-info", "medium", true),
        ]);
        normalize_language_env_defaults(&mut snap);
        let nrs = snap.non_rpm_software.as_ref().unwrap();
        assert!(
            !nrs.items[0].include,
            "npm medium-confidence should default to excluded"
        );
        assert!(
            !nrs.items[1].include,
            "pip medium-confidence should default to excluded"
        );
    }

    #[test]
    fn low_confidence_language_env_defaults_to_excluded() {
        let mut snap = snap_with_nonrpm(vec![nonrpm_item("python venv", "low", true)]);
        normalize_language_env_defaults(&mut snap);
        let nrs = snap.non_rpm_software.as_ref().unwrap();
        assert!(
            !nrs.items[0].include,
            "low-confidence should default to excluded"
        );
    }

    #[test]
    fn non_language_env_items_untouched() {
        let mut snap = snap_with_nonrpm(vec![
            nonrpm_item("binary", "medium", true),
            nonrpm_item("git repo", "low", true),
        ]);
        normalize_language_env_defaults(&mut snap);
        let nrs = snap.non_rpm_software.as_ref().unwrap();
        // Non-language items should not be modified by this normalize function
        assert!(nrs.items[0].include, "binary item should stay included");
        assert!(nrs.items[1].include, "git repo item should stay included");
    }

    #[test]
    fn no_nonrpm_section_is_noop() {
        let mut snap = InspectionSnapshot {
            schema_version: inspectah_core::snapshot::SCHEMA_VERSION,
            ..Default::default()
        };
        normalize_language_env_defaults(&mut snap);
        assert!(snap.non_rpm_software.is_none());
    }

    #[test]
    fn empty_confidence_treated_as_low() {
        let mut snap = snap_with_nonrpm(vec![nonrpm_item("npm lockfile", "", true)]);
        normalize_language_env_defaults(&mut snap);
        let nrs = snap.non_rpm_software.as_ref().unwrap();
        assert!(
            !nrs.items[0].include,
            "empty confidence should default to excluded"
        );
    }
}
