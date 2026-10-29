use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION, SnapshotError};
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::selinux::SelinuxSection;

/// Keys that moved off `KernelBootSection` when the config walker took
/// ownership of the files behind them.
const RETIRED_KERNEL_BOOT_KEYS: &[&str] = &[
    "modules_load_d",
    "modprobe_d",
    "dracut_conf",
    "tuned_custom_profiles",
    "non_default_modules",
];

/// Keys that moved off `SelinuxSection` for the same reason.
const RETIRED_SELINUX_KEYS: &[&str] = &["pam_configs", "audit_rules"];

#[test]
fn schema_22_is_rejected() {
    let json = r#"{"schema_version": 22, "meta": {}, "system_type": "package-mode",
        "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
    match InspectionSnapshot::load(json) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 22),
        other => panic!("expected UnsupportedVersion(22), got {other:?}"),
    }
}

#[test]
fn schema_23_round_trips() {
    let snap = InspectionSnapshot {
        schema_version: SCHEMA_VERSION,
        ..Default::default()
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(InspectionSnapshot::load(&json).unwrap().schema_version, 23);
}

/// A schema 23 snapshot does not serialize the lists the config walker
/// now owns. Serde ignores unknown fields, so a 23 payload that still
/// carries one of these keys loads and drops it; that is accepted and is
/// not what this asserts. What it asserts is that nothing we write puts
/// the duplicate back on the wire.
#[test]
fn schema_23_does_not_serialize_the_relocated_section_lists() {
    let snap = InspectionSnapshot {
        schema_version: SCHEMA_VERSION,
        kernel_boot: Some(KernelBootSection::default()),
        selinux: Some(SelinuxSection::default()),
        ..Default::default()
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(InspectionSnapshot::load(&json).unwrap().schema_version, 23);

    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let kernel_boot = value["kernel_boot"]
        .as_object()
        .expect("kernel_boot serializes as an object");
    for key in RETIRED_KERNEL_BOOT_KEYS {
        assert!(
            !kernel_boot.contains_key(*key),
            "kernel_boot must not serialize `{key}`: the config walker owns those files"
        );
    }
    let selinux = value["selinux"]
        .as_object()
        .expect("selinux serializes as an object");
    for key in RETIRED_SELINUX_KEYS {
        assert!(
            !selinux.contains_key(*key),
            "selinux must not serialize `{key}`: the config walker owns those files"
        );
    }
}
