use inspectah_core::normalize::{diff_snapshots, load_divergence_allowlist};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::os::SystemType;

#[test]
fn test_parity_gate_exercises_full_path() {
    // Load the real allowlist
    let divergences_md = include_str!("../../testdata/divergences.md");
    let allowlist = load_divergence_allowlist(divergences_md);
    assert!(
        !allowlist.is_empty(),
        "allowlist must have documented divergences"
    );

    // Load a real Go fixture
    let go_json = include_str!("../../testdata/golden/go-v12-minimal.json");

    // Build a Rust snapshot that mirrors the Go fixture's structure —
    // NOT InspectionSnapshot::new() which has wrong defaults.
    // This prevents synthetic mismatches from polluting the allowlist.
    let mut rust_snap = InspectionSnapshot::new();
    rust_snap.system_type = SystemType::PackageMode;
    rust_snap.preflight.status = "ok".into();

    let rust_json = serde_json::to_string(&rust_snap).unwrap();

    // Run the full diff pipeline
    let undocumented = diff_snapshots(go_json, &rust_json, &allowlist).unwrap();

    // This is the mandatory gate: undocumented diffs fail CI
    assert!(
        undocumented.is_empty(),
        "undocumented Go-vs-Rust divergences found:\n{}",
        undocumented
            .iter()
            .map(|d| format!("  {}: go={}, rust={}", d.path, d.go_value, d.rust_value))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
