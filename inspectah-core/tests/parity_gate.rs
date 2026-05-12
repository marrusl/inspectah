//! Provisional serde roundtrip tests for golden-file compatibility.
//!
//! These tests prove that Rust types can deserialize golden JSON and
//! re-serialize it without loss. They do NOT exercise actual Rust inspector
//! code — the real inspector-vs-golden parity gate lives in
//! `inspectah-collect/tests/parity_test.rs`, which runs inspectors on
//! fixture data and diffs the output against these same golden files.
//!
//! When real Go-captured goldens replace the current provisional ones,
//! serde roundtrip failures here will surface any type-level incompatibility.

use inspectah_core::normalize::{diff_snapshots, load_divergence_allowlist};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::os::SystemType;
use inspectah_core::types::services::ServiceSection;
use inspectah_core::types::storage::StorageSection;
use std::collections::BTreeSet;

/// Shared divergence allowlist, loaded once per test from the canonical source.
fn allowlist() -> BTreeSet<String> {
    let md = include_str!("../../testdata/divergences.md");
    load_divergence_allowlist(md)
}

// ── Snapshot self-roundtrip ──────────────────────────────────────────

/// Provisional: proves Rust snapshot serde roundtrip fidelity.
/// Does NOT compare against Go output.
#[test]
fn test_provisional_snapshot_serde_roundtrip() {
    let divergences_md = include_str!("../../testdata/divergences.md");
    let allowlist = load_divergence_allowlist(divergences_md);

    let mut snap = InspectionSnapshot::new();
    snap.system_type = SystemType::PackageMode;
    snap.preflight.status = "ok".into();

    let json = serde_json::to_string(&snap).unwrap();
    let parsed: InspectionSnapshot = serde_json::from_str(&json).unwrap();
    let json2 = serde_json::to_string(&parsed).unwrap();

    let undocumented = diff_snapshots(&json, &json2, &allowlist).unwrap();

    assert!(
        undocumented.is_empty(),
        "Rust snapshot does not round-trip faithfully:\n{}",
        undocumented
            .iter()
            .map(|d| format!("  {}: a={}, b={}", d.path, d.go_value, d.rust_value))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── Services section serde roundtrip ─────────────────────────────────

/// Provisional: proves golden JSON deserializes into ServiceSection and
/// re-serializes without field loss. Real Go-vs-Rust parity gate requires
/// inspector execution (see inspectah-collect/tests/parity_test.rs).
#[test]
fn test_provisional_services_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-services-section.json");

    let section: ServiceSection =
        serde_json::from_str(golden).expect("golden must deserialize into ServiceSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Services section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Provisional: validates golden file structural completeness — all key
/// fields are populated so the roundtrip test exercises the full type.
#[test]
fn test_provisional_services_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-services-section.json");
    let section: ServiceSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.state_changes.is_empty(),
        "golden must contain state_changes"
    );
    assert!(
        !section.enabled_units.is_empty(),
        "golden must contain enabled_units"
    );
    assert!(
        !section.disabled_units.is_empty(),
        "golden must contain disabled_units"
    );
    assert!(!section.drop_ins.is_empty(), "golden must contain drop_ins");

    let sc = &section.state_changes[0];
    assert!(!sc.unit.is_empty(), "unit must be populated");
    assert!(
        !sc.current_state.is_empty(),
        "current_state must be populated"
    );
    assert!(
        !sc.default_state.is_empty(),
        "default_state must be populated"
    );
    assert!(!sc.action.is_empty(), "action must be populated");
}

// ── Storage section serde roundtrip ──────────────────────────────────

/// Provisional: proves golden JSON deserializes into StorageSection and
/// re-serializes without field loss. Real parity gate is in
/// inspectah-collect/tests/parity_test.rs.
#[test]
fn test_provisional_storage_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-storage-section.json");

    let section: StorageSection =
        serde_json::from_str(golden).expect("golden must deserialize into StorageSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Storage section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Provisional: validates storage golden file structural completeness.
#[test]
fn test_provisional_storage_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-storage-section.json");
    let section: StorageSection = serde_json::from_str(golden).unwrap();

    assert!(
        !section.fstab_entries.is_empty(),
        "golden must contain fstab_entries"
    );
    assert!(
        !section.mount_points.is_empty(),
        "golden must contain mount_points"
    );
    assert!(!section.lvm_info.is_empty(), "golden must contain lvm_info");
    assert!(
        !section.credential_refs.is_empty(),
        "golden must contain credential_refs"
    );

    let entry = &section.fstab_entries[0];
    assert!(!entry.device.is_empty(), "device must be populated");
    assert!(
        !entry.mount_point.is_empty(),
        "mount_point must be populated"
    );
    assert!(!entry.fstype.is_empty(), "fstype must be populated");
}

// ── Kernel boot section serde roundtrip ──────────────────────────────

/// Provisional: proves golden JSON deserializes into KernelBootSection and
/// re-serializes without field loss. Real parity gate is in
/// inspectah-collect/tests/parity_test.rs.
#[test]
fn test_provisional_kernelboot_serde_roundtrip() {
    let golden = include_str!("../../testdata/golden/go-v13-kernelboot-section.json");

    let section: KernelBootSection =
        serde_json::from_str(golden).expect("golden must deserialize into KernelBootSection");

    let rust_json = serde_json::to_string_pretty(&section).unwrap();
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Kernelboot section has undocumented divergences:\n{}",
        format_diffs(&undocumented)
    );
}

/// Provisional: validates kernelboot golden file structural completeness.
#[test]
fn test_provisional_kernelboot_field_coverage() {
    let golden = include_str!("../../testdata/golden/go-v13-kernelboot-section.json");
    let section: KernelBootSection = serde_json::from_str(golden).unwrap();

    assert!(!section.cmdline.is_empty(), "golden must contain cmdline");
    assert!(
        !section.sysctl_overrides.is_empty(),
        "golden must contain sysctl_overrides"
    );
    assert!(
        !section.loaded_modules.is_empty(),
        "golden must contain loaded_modules"
    );
    assert!(
        !section.dracut_conf.is_empty(),
        "golden must contain dracut_conf"
    );
    assert!(section.locale.is_some(), "golden must contain locale");
    assert!(section.timezone.is_some(), "golden must contain timezone");
    assert!(
        !section.tuned_active.is_empty(),
        "golden must contain tuned_active"
    );

    let so = &section.sysctl_overrides[0];
    assert!(!so.key.is_empty(), "sysctl key must be populated");
    assert!(!so.runtime.is_empty(), "sysctl runtime must be populated");
    assert!(!so.source.is_empty(), "sysctl source must be populated");
}

// ── Cross-section golden consistency ─────────────────────────────────

#[test]
fn test_all_section_goldens_are_valid_json() {
    // Ensure every golden file in testdata/golden/ is valid JSON.
    // This catches accidental corruption from manual edits.
    let goldens: &[(&str, &str)] = &[
        (
            "rpm",
            include_str!("../../testdata/golden/go-v13-rpm-section.json"),
        ),
        (
            "services",
            include_str!("../../testdata/golden/go-v13-services-section.json"),
        ),
        (
            "storage",
            include_str!("../../testdata/golden/go-v13-storage-section.json"),
        ),
        (
            "kernelboot",
            include_str!("../../testdata/golden/go-v13-kernelboot-section.json"),
        ),
    ];

    for (name, json) in goldens {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(json);
        assert!(
            parsed.is_ok(),
            "golden file for {name} is not valid JSON: {}",
            parsed.unwrap_err()
        );
    }
}

/// Format differences for assertion messages.
fn format_diffs(diffs: &[inspectah_core::normalize::Difference]) -> String {
    diffs
        .iter()
        .map(|d| format!("  {}: go={}, rust={}", d.path, d.go_value, d.rust_value))
        .collect::<Vec<_>>()
        .join("\n")
}
