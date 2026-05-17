use inspectah_core::baseline::INCOMPATIBLE_SERVICES;
use inspectah_core::snapshot::{migrate, InspectionSnapshot};
use crate::types::{AttentionLevel, RefineError, RefinedConfig, RefinedPackage};
use serde_json::Value;

/// Load a raw JSON snapshot for refine, applying presence-aware defaulting.
///
/// Before typed deserialization, walks raw JSON arrays and patches any
/// entry lacking an `include` field by inserting `"include": true`.
/// Entries with an existing `"include": false` are untouched. This
/// resolves the `serde(default)` bool ambiguity where absent and
/// explicit-false collapse to the same `false`.
///
/// Enforces the same schema version gate as `InspectionSnapshot::load()`
/// (MIN_SCHEMA..=SCHEMA_VERSION) by round-tripping through the patched
/// JSON string and calling `load()`.
pub fn load_for_refine(raw_json: &str) -> Result<InspectionSnapshot, RefineError> {
    let mut value: Value = serde_json::from_str(raw_json)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    patch_missing_includes(&mut value);

    // Serialize patched Value back to string and use InspectionSnapshot::load()
    // which enforces MIN_SCHEMA..=SCHEMA_VERSION (12..=14).
    let patched_json = serde_json::to_string(&value)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;
    let mut snap = InspectionSnapshot::load(&patched_json)
        .map_err(|e| RefineError::SnapshotLoad(e.to_string()))?;

    migrate(&mut snap);
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
            if let Value::Object(map) = entry {
                if !map.contains_key("include") {
                    map.insert("include".into(), Value::Bool(true));
                }
            }
        }
    }
}

/// Materialize tier-aware include defaults for packages.
///
/// Baseline subtraction: Tier 1 (Routine/baseline match) packages are
/// included because the package manager handles them. Tier 2
/// (Informational/user-added) packages are included only if they are
/// leaf packages (or if leaf data is unavailable). Tier 3
/// (NeedsReview/unknown provenance) packages are excluded.
pub fn normalize_package_defaults(
    snapshot: &mut InspectionSnapshot,
    packages: &[RefinedPackage],
) {
    let rpm = match snapshot.rpm.as_mut() {
        Some(r) => r,
        None => return,
    };

    let leaf_set: Option<std::collections::HashSet<&str>> = rpm.leaf_packages
        .as_ref()
        .map(|lp| lp.iter().map(|s| s.as_str()).collect());

    for (i, refined) in packages.iter().enumerate() {
        if i >= rpm.packages_added.len() { break; }
        let primary_level = refined.attention.first()
            .map(|t| t.level).unwrap_or(AttentionLevel::Routine);
        match primary_level {
            AttentionLevel::Routine => { rpm.packages_added[i].include = true; }
            AttentionLevel::Informational => {
                let is_leaf = match &leaf_set {
                    Some(set) => set.contains(rpm.packages_added[i].name.as_str()),
                    None => true,
                };
                rpm.packages_added[i].include = is_leaf;
            }
            AttentionLevel::NeedsReview => { rpm.packages_added[i].include = false; }
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

    services.enabled_units.retain(|u| !incompatible_units.contains(&u.as_str()));
}

/// Materialize tier-aware include defaults for config files.
///
/// Baseline subtraction: Tier 1 (Routine) configs — RpmOwnedDefault
/// and BaselineMatch — are NOT copied because the package manager or
/// base image already provides them. Tier 2 (Informational) configs
/// are included unless orphaned. Tier 3 (NeedsReview/user-modified)
/// configs are always included.
pub fn normalize_config_defaults(
    snapshot: &mut InspectionSnapshot,
    configs: &[RefinedConfig],
) {
    let config = match snapshot.config.as_mut() {
        Some(c) => c,
        None => return,
    };
    for (i, refined) in configs.iter().enumerate() {
        if i >= config.files.len() { break; }
        let primary_level = refined.attention.first()
            .map(|t| t.level).unwrap_or(AttentionLevel::Routine);
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
            schema_version: 14,
            services: Some(ServiceSection {
                state_changes,
                enabled_units,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn sc(unit: &str, include: bool) -> ServiceStateChange {
        ServiceStateChange {
            unit: unit.to_string(),
            current_state: "enabled".to_string(),
            default_state: "disabled".to_string(),
            action: "enable".to_string(),
            include,
            ..Default::default()
        }
    }

    #[test]
    fn incompatible_service_flagged_include_false() {
        let mut snap = snap_with_services(
            vec![sc("dnf-makecache.service", true)],
            vec![],
        );
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(!services.state_changes[0].include);
    }

    #[test]
    fn compatible_service_not_flagged() {
        let mut snap = snap_with_services(
            vec![sc("httpd.service", true)],
            vec![],
        );
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
        assert!(services.state_changes[4].include);  // httpd.service
        assert!(services.state_changes[5].include);  // sshd.service

        // enabled_units: only compatible ones remain
        assert_eq!(
            services.enabled_units,
            vec!["httpd.service".to_string(), "sshd.service".to_string()]
        );
    }

    #[test]
    fn no_services_section_is_noop() {
        let mut snap = InspectionSnapshot {
            schema_version: 14,
            ..Default::default()
        };
        // Should not panic.
        normalize_incompatible_services(&mut snap);
        assert!(snap.services.is_none());
    }

    #[test]
    fn already_excluded_service_stays_excluded() {
        let mut snap = snap_with_services(
            vec![sc("dnf-makecache.service", false)],
            vec![],
        );
        normalize_incompatible_services(&mut snap);
        let services = snap.services.as_ref().unwrap();
        assert!(!services.state_changes[0].include);
    }
}
