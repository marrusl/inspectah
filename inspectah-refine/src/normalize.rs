use inspectah_core::snapshot::{migrate, InspectionSnapshot};
use crate::types::RefineError;
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
