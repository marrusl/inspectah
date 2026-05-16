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
