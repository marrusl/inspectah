use serde_json::Value;
use std::collections::BTreeSet;

/// Volatile meta subfields that differ between Go and Rust output.
/// Only THESE specific keys are stripped — contract-bearing meta keys
/// (hostname, host_root) survive normalization.
const VOLATILE_META_KEYS: &[&str] = &[
    "timestamp",
    "inspectah_version",
    "inspectah_commit",
    "inspectah_date",
];

/// Normalize a snapshot for comparison. Only strips explicitly volatile
/// subfields, NOT the entire meta object.
pub fn normalize(value: &mut Value) {
    if let Value::Object(map) = value {
        // Strip only volatile meta subfields, not the whole meta
        if let Some(Value::Object(meta)) = map.get_mut("meta") {
            for key in VOLATILE_META_KEYS {
                meta.remove(*key);
            }
        }
        // Strip Rust-only fields not present in Go output
        map.remove("redaction_state");
        map.remove("completeness");

        for (_, v) in map.iter_mut() {
            normalize(v);
        }
    }
    if let Value::Array(arr) = value {
        for v in arr.iter_mut() {
            normalize(v);
        }
    }
}


#[derive(Debug, PartialEq)]
pub struct Difference {
    pub path: String,
    pub go_value: String,
    pub rust_value: String,
}

/// Load the divergences allowlist from testdata/divergences.md.
/// Parses paths from `## ` headers followed by `- Path: ` lines.
pub fn load_divergence_allowlist(md: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for line in md.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("- Path: `").and_then(|s| s.strip_suffix('`')) {
            paths.insert(path.to_string());
        }
    }
    paths
}

/// Compare two snapshots after normalization. Returns only
/// UNDOCUMENTED differences (not in the allowlist).
pub fn diff_snapshots(
    go_json: &str,
    rust_json: &str,
    allowlist: &BTreeSet<String>,
) -> Result<Vec<Difference>, serde_json::Error> {
    let mut go: Value = serde_json::from_str(go_json)?;
    let mut rust: Value = serde_json::from_str(rust_json)?;
    normalize(&mut go);
    normalize(&mut rust);

    let mut all_diffs = Vec::new();
    diff_values("$", &go, &rust, &mut all_diffs);

    // Filter out documented divergences
    let undocumented: Vec<Difference> = all_diffs
        .into_iter()
        .filter(|d| !allowlist.contains(&d.path))
        .collect();
    Ok(undocumented)
}

fn diff_values(path: &str, go: &Value, rust: &Value, diffs: &mut Vec<Difference>) {
    match (go, rust) {
        (Value::Object(g), Value::Object(r)) => {
            let keys: BTreeSet<_> = g.keys().chain(r.keys()).collect();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (g.get(key), r.get(key)) {
                    (Some(gv), Some(rv)) => diff_values(&child_path, gv, rv, diffs),
                    (Some(gv), None) => diffs.push(Difference {
                        path: child_path,
                        go_value: gv.to_string(),
                        rust_value: "<missing>".into(),
                    }),
                    (None, Some(rv)) => diffs.push(Difference {
                        path: child_path,
                        go_value: "<missing>".into(),
                        rust_value: rv.to_string(),
                    }),
                    _ => {}
                }
            }
        }
        (Value::Array(g), Value::Array(r)) => {
            for i in 0..g.len().max(r.len()) {
                let child_path = format!("{path}[{i}]");
                match (g.get(i), r.get(i)) {
                    (Some(gv), Some(rv)) => diff_values(&child_path, gv, rv, diffs),
                    (Some(gv), None) => diffs.push(Difference {
                        path: child_path,
                        go_value: gv.to_string(),
                        rust_value: "<missing>".into(),
                    }),
                    (None, Some(rv)) => diffs.push(Difference {
                        path: child_path,
                        go_value: "<missing>".into(),
                        rust_value: rv.to_string(),
                    }),
                    _ => {}
                }
            }
        }
        _ if go != rust => {
            diffs.push(Difference {
                path: path.to_string(),
                go_value: go.to_string(),
                rust_value: rust.to_string(),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_preserves_contract_meta() {
        let mut val: Value = serde_json::from_str(
            r#"{"meta":{"hostname":"web01","timestamp":"2024-01-01","inspectah_version":"0.7.0"}}"#,
        )
        .unwrap();
        normalize(&mut val);
        let meta = val["meta"].as_object().unwrap();
        assert!(
            meta.contains_key("hostname"),
            "contract-bearing meta key must survive"
        );
        assert!(
            !meta.contains_key("timestamp"),
            "volatile key must be stripped"
        );
        assert!(
            !meta.contains_key("inspectah_version"),
            "volatile key must be stripped"
        );
    }

    #[test]
    fn test_divergence_allowlist_parsing() {
        let md = "## schema_version\n- Path: `$.schema_version`\n- Reason: version bump\n";
        let allowlist = load_divergence_allowlist(md);
        assert!(allowlist.contains("$.schema_version"));
    }

    #[test]
    fn test_allowed_divergences_filtered() {
        let go = r#"{"schema_version":13,"system_type":"package-mode"}"#;
        let rust = r#"{"schema_version":14,"system_type":"package-mode"}"#;
        let mut allowlist = BTreeSet::new();
        allowlist.insert("$.schema_version".to_string());
        let diffs = diff_snapshots(go, rust, &allowlist).unwrap();
        assert!(diffs.is_empty(), "allowed divergence should be filtered");
    }

    #[test]
    fn test_undocumented_divergence_surfaces() {
        let go = r#"{"system_type":"package-mode","rpm":{"packages_added":[]}}"#;
        let rust = r#"{"system_type":"bootc","rpm":{"packages_added":[]}}"#;
        let allowlist = BTreeSet::new();
        let diffs = diff_snapshots(go, rust, &allowlist).unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path, "$.system_type");
    }

    #[test]
    fn test_go_v12_golden_loads() {
        let json = include_str!("../../testdata/golden/go-v12-minimal.json");
        let snap = crate::snapshot::InspectionSnapshot::load(json).unwrap();
        assert!(snap.schema_version >= 12);
    }
}
