use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION, SnapshotError};
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, RpmSection};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::services::ServiceSection;
use std::collections::BTreeSet;

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

/// Schema 23 is the last version collected before the cross-inspector
/// pass that marks a `services.drop_ins`-owned config entry as
/// inventory. Loading one would mean re-rendering from the two-toggle
/// state the pass exists to remove, with no field that distinguishes it,
/// so `MIN_SCHEMA` rose alongside `SCHEMA_VERSION`.
#[test]
fn schema_23_is_rejected() {
    let json = r#"{"schema_version": 23, "meta": {}, "system_type": "package-mode",
        "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
    match InspectionSnapshot::load(json) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 23),
        other => panic!("expected UnsupportedVersion(23), got {other:?}"),
    }
}

/// Schema 24 is the last version collected before `detect_var_dir_backing`
/// recorded which file declared a `/var` directory. It names a unit by
/// filename in a `unit` field schema 25 does not have, and the path it
/// stands for cannot be recovered from the rest of the snapshot, so a 24
/// payload is both unreadable and unupgradable.
#[test]
fn schema_24_is_rejected() {
    let json = r#"{"schema_version": 24, "meta": {}, "system_type": "package-mode",
        "preflight": {"status": "ok"}, "warnings": [], "redactions": []}"#;
    match InspectionSnapshot::load(json) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 24),
        other => panic!("expected UnsupportedVersion(24), got {other:?}"),
    }
}

/// Schema 25 is the last version collected before the config inspector
/// told a file it could not read from a file that is empty. In a 25
/// payload a failed read is an included entry with empty content, as in
/// this one, and nothing in it says the read failed; loading it would let
/// an export copy the empty file over the real one. It deserializes, so
/// only the version gate refuses it.
#[test]
fn schema_25_is_rejected() {
    let json = r#"{"schema_version": 25, "meta": {}, "system_type": "package-mode",
        "preflight": {"status": "ok"}, "warnings": [], "redactions": [],
        "config": {"files": [{"path": "/etc/httpd/conf.d/ssl.conf",
            "kind": "rpm_owned_modified", "category": "other", "content": "",
            "disposition": {"kind": "actionable", "include": true}}]}}"#;
    match InspectionSnapshot::load(json) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 25),
        other => panic!("expected UnsupportedVersion(25), got {other:?}"),
    }
}

/// A schema-26 snapshot, the last with the per-key sysctl override list.
/// Read by the schema 26 refusal test in every production loader.
const SCHEMA_26_SNAPSHOT: &str =
    include_str!("../../../testdata/fixtures/snapshot/schema-26-sysctl-overrides.json");

/// Schema 26 is the last version collected before the sysctl section
/// followed systemd-sysctl's precedence. Its sysctl overrides were read
/// per key with no notion of a file shadowing another, so a source it
/// names may be a file the host never applied, and nothing in the payload
/// says which. It deserializes, so only the version gate refuses it.
#[test]
fn schema_26_snapshot_is_refused() {
    match InspectionSnapshot::load(SCHEMA_26_SNAPSHOT) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 26),
        other => panic!("expected UnsupportedVersion(26), got {other:?}"),
    }
}

/// A snapshot JSON whose sysctl section holds `runtime` and
/// `redacted_runtime` as given, at the current schema version.
fn snapshot_with_sysctl_runtime(runtime: &str, redacted_runtime: &str) -> String {
    format!(
        r#"{{"schema_version": {SCHEMA_VERSION}, "meta": {{}}, "system_type": "package-mode",
        "preflight": {{"status": "ok"}}, "warnings": [], "redactions": [],
        "kernel_boot": {{"sysctl": {{"files": [], "runtime": {runtime},
            "redacted_runtime": {redacted_runtime}}}}}}}"#
    )
}

/// `SysctlKey` is deserialized only in canonical (dotted, `sysctl -a`)
/// form: another spelling of a key, or one key in both runtime fields,
/// fails the load and the error names the key.
#[test]
fn sysctl_key_on_the_wire() {
    let canonical = "net.ipv4.conf.enp3s0/200.forwarding";
    let json = snapshot_with_sysctl_runtime(&format!(r#"{{"{canonical}": "1"}}"#), "[]");
    let snap = InspectionSnapshot::load(&json).expect("canonical key loads");
    let out: serde_json::Value = serde_json::to_value(&snap).unwrap();
    let runtime = out["kernel_boot"]["sysctl"]["runtime"]
        .as_object()
        .expect("runtime serializes as an object");
    assert_eq!(runtime.keys().collect::<Vec<_>>(), vec![canonical]);
    assert_eq!(runtime[canonical], "1");

    for spelling in [
        "net/ipv4/ip_forward",
        "net/ipv4/conf/enp3s0.200/forwarding",
        ".vm.swappiness",
        "vm.swappiness.",
        "vm..swappiness",
    ] {
        let json = snapshot_with_sysctl_runtime(&format!(r#"{{"{spelling}": "1"}}"#), "[]");
        let err = InspectionSnapshot::load(&json)
            .expect_err("a non-canonical key must fail the load")
            .to_string();
        assert!(
            err.contains(spelling),
            "error must name `{spelling}`: {err}"
        );
    }

    let json = snapshot_with_sysctl_runtime(r#"{"vm.swappiness": "10"}"#, r#"["vm.swappiness"]"#);
    let err = InspectionSnapshot::load(&json)
        .expect_err("a key in both runtime fields must fail the load")
        .to_string();
    assert!(
        err.contains("vm.swappiness"),
        "error must name the key: {err}"
    );
}

#[test]
fn schema_27_round_trips() {
    let snap = InspectionSnapshot {
        schema_version: SCHEMA_VERSION,
        ..Default::default()
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(
        InspectionSnapshot::load(&json).unwrap().schema_version,
        SCHEMA_VERSION
    );
}

/// A schema 27 snapshot does not serialize the lists the config walker
/// now owns. Serde ignores unknown fields, so a 23 payload that still
/// carries one of these keys loads and drops it; that is accepted and is
/// not what this asserts. What it asserts is that nothing we write puts
/// the duplicate back on the wire.
#[test]
fn schema_27_does_not_serialize_the_relocated_section_lists() {
    let snap = InspectionSnapshot {
        schema_version: SCHEMA_VERSION,
        kernel_boot: Some(KernelBootSection::default()),
        selinux: Some(SelinuxSection::default()),
        ..Default::default()
    };
    let json = serde_json::to_string(&snap).unwrap();
    assert_eq!(
        InspectionSnapshot::load(&json).unwrap().schema_version,
        SCHEMA_VERSION
    );

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

/// The serialized keys of a fully populated `PackageEntry`.
const PACKAGE_ENTRY_KEYS: &[&str] = &[
    "acknowledged",
    "aggregate",
    "arch",
    "cache_path",
    "disposition",
    "epoch",
    "locked",
    "name",
    "release",
    "repo_attribution",
    "repo_enabling_from",
    "repoless_annotation",
    "repoless_cached",
    "source_repo",
    "state",
    "uploaded",
    "uploaded_version",
    "version",
];

/// The serialized keys of a fully populated `RpmSection`.
const RPM_SECTION_KEYS: &[&str] = &[
    "auto_packages",
    "base_image",
    "base_image_only",
    "baseline_module_streams",
    "baseline_package_names",
    "baseline_suppressed",
    "dnf_history_removed",
    "duplicate_packages",
    "file_ownership",
    "gpg_keys",
    "installed_groups",
    "leaf_authority_hosts",
    "leaf_dep_tree",
    "leaf_packages",
    "leaf_total_hosts",
    "module_stream_conflicts",
    "module_streams",
    "multiarch_packages",
    "ostree_overrides",
    "ostree_removals",
    "packages_added",
    "repo_files",
    "rpm_va",
    "version_changes",
    "version_locks",
    "versionlock_command_output",
];

/// The serialized keys of a `ServiceSection`.
const SERVICE_SECTION_KEYS: &[&str] = &[
    "disabled_units",
    "drop_ins",
    "enabled_units",
    "preset_matched_units",
    "state_changes",
];

fn serialized_keys<T: serde::Serialize>(value: &T) -> BTreeSet<String> {
    serde_json::to_value(value)
        .expect("snapshot types serialize")
        .as_object()
        .expect("snapshot struct serializes as an object")
        .keys()
        .cloned()
        .collect()
}

fn expected_keys(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| (*k).to_string()).collect()
}

/// The snapshot format guard: schema 27 and the key sets of the package
/// and service structs are pinned. Serde ignores unknown fields and
/// defaults missing ones, so a field added, renamed or removed with
/// `#[serde(default)]` passes every round-trip test; this one fails
/// instead. Every `skip_serializing_if` field is set to a non-default
/// value so it appears.
#[test]
fn schema_27_format_guard_pins_package_and_service_keys() {
    assert_eq!(SCHEMA_VERSION, 27);

    let package = PackageEntry {
        locked: true,
        acknowledged: true,
        repo_enabling_from: vec![RepoId::new("appstream")],
        repoless_annotation: "annotated".into(),
        repoless_cached: true,
        uploaded: true,
        cache_path: Some("/var/cache/dnf/x.rpm".into()),
        uploaded_version: Some("1.0-1".into()),
        ..Default::default()
    };
    assert_eq!(serialized_keys(&package), expected_keys(PACKAGE_ENTRY_KEYS));

    let rpm = RpmSection {
        packages_added: vec![package],
        baseline_suppressed: Some(vec!["bash".into()]),
        leaf_authority_hosts: Some(1),
        leaf_total_hosts: Some(1),
        installed_groups: Some(vec![InstalledGroup::default()]),
        ..Default::default()
    };
    assert_eq!(serialized_keys(&rpm), expected_keys(RPM_SECTION_KEYS));

    assert_eq!(
        serialized_keys(&ServiceSection::default()),
        expected_keys(SERVICE_SECTION_KEYS)
    );
}
