use inspectah_core::snapshot::{InspectionSnapshot, SCHEMA_VERSION, SnapshotError};
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::repo::RepoId;
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, RpmSection};
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::units::{
    UnitName, UnitNameError, UnitSection, UnitState, UnitStateChangeError,
};
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

/// A schema-27 snapshot, the last with the `services` section. The other
/// production loaders' refusal tests read the same fixture file with
/// `include_str!`.
const SCHEMA_27_SNAPSHOT: &str =
    include_str!("../../../testdata/fixtures/snapshot/schema-27-services-section.json");

/// The schema-27 payload moved to the current version with its `services`
/// section renamed to `units` and its three unit name lists replaced by
/// the one inventory that holds the same unit, empty `mount_findings`,
/// `manual_links` and `undetermined_installs` lists added, and the state
/// change's name and owner
/// moved into a carried unit with a vendor fragment. Nothing else changes.
fn schema_27_payload_as_current() -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_str(SCHEMA_27_SNAPSHOT).unwrap();
    let object = value.as_object_mut().unwrap();
    let mut section = object.remove("services").expect("fixture has `services`");
    let lists = section.as_object_mut().unwrap();
    for list in ["enabled_units", "disabled_units", "preset_matched_units"] {
        lists.remove(list).expect("schema 27 has the list");
    }
    lists.insert(
        "inventory".into(),
        serde_json::json!([{"unit": "httpd.service", "state": "enabled", "preset": "disable"}]),
    );
    lists.insert("mount_findings".into(), serde_json::json!([]));
    lists.insert("manual_links".into(), serde_json::json!([]));
    lists.insert("undetermined_installs".into(), serde_json::json!([]));
    let change = &mut lists.get_mut("state_changes").unwrap()[0];
    let owner = change
        .as_object_mut()
        .unwrap()
        .remove("owning_package")
        .expect("schema 27 state change has an owner");
    change["unit"] = serde_json::json!({
        "name": "httpd.service", "fragment": "vendor", "file": "httpd.service",
        "owning_package": owner
    });
    object.insert("units".into(), section);
    object.insert("schema_version".into(), SCHEMA_VERSION.into());
    value
}

/// Schema 27 is the last version whose unit section was `services` and
/// held services only. Its payload parses: serde drops the unknown key.
/// It is refused because every socket and timer state the host had is
/// absent from it with nothing to say so, and only a re-scan collects
/// them. The same section under `units` at the current version loads, so
/// the version gate is what refuses it.
#[test]
fn schema_27_snapshot_is_refused() {
    let current = InspectionSnapshot::load(&schema_27_payload_as_current().to_string())
        .expect("the same section under `units` loads at the current version");
    let units = current.units.expect("the `units` section is present");
    assert_eq!(units.state_changes.len(), 1);
    assert_eq!(
        units.state_changes[0].unit().name().as_str(),
        "httpd.service"
    );

    match InspectionSnapshot::load(SCHEMA_27_SNAPSHOT) {
        Err(SnapshotError::UnsupportedVersion(v)) => assert_eq!(v, 27),
        other => panic!("expected UnsupportedVersion(27), got {other:?}"),
    }
}

/// A unit name in a state change is parsed on load: a name with a byte
/// outside the systemd unit name set fails the load, and the error names
/// the rule. The same payload with a valid name loads.
#[test]
fn state_change_unit_name_is_parsed_on_load() {
    let mut payload = schema_27_payload_as_current();
    InspectionSnapshot::load(&payload.to_string()).expect("a valid unit name loads");

    payload["units"]["state_changes"][0]["unit"]["name"] = "httpd daemon.service".into();
    let err = InspectionSnapshot::load(&payload.to_string())
        .expect_err("a unit name with a space must fail the load")
        .to_string();
    let rule = UnitNameError::InvalidCharacter { ch: ' ' }.to_string();
    assert!(err.contains(&rule), "error must carry `{rule}`: {err}");
}

/// A mount finding is an administrator's storage unit: for each storage
/// kind, a local unit loads in `mount_findings`, and the same unit as a
/// vendor file with an owning package fails the load, with the error
/// naming `classify`.
#[test]
fn mount_finding_unit_is_classified_on_load() {
    for name in [
        "tmp.mount",
        "proc-sys-fs-binfmt_misc.automount",
        "dev-zram0.swap",
    ] {
        let mut payload = schema_27_payload_as_current();
        payload["units"]["mount_findings"] = serde_json::json!([{
            "unit": {"name": name, "fragment": "local", "file": name},
            "state_change": ["enabled", null],
            "mount_point": null,
            "nofail": false,
            "aggregate": null
        }]);
        let snap = InspectionSnapshot::load(&payload.to_string())
            .unwrap_or_else(|err| panic!("a local {name} loads: {err}"));
        let findings = &snap.units.expect("units present").mount_findings;
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].unit.name().as_unit_name().as_str(), name);

        payload["units"]["mount_findings"][0]["unit"] = serde_json::json!({
            "name": name, "fragment": "vendor", "file": name,
            "owning_package": "systemd"
        });
        let err = InspectionSnapshot::load(&payload.to_string())
            .expect_err("a package-owned storage unit is not a mount finding")
            .to_string();
        assert!(
            err.contains(&format!("classify places {name} as carried")),
            "{name}: error must name classify: {err}"
        );
    }
}

/// The schema-28 payload with its one state change replaced by `unit`
/// in `state`, as JSON text.
fn payload_with_state_change(unit: serde_json::Value, state: &str) -> String {
    let mut payload = schema_27_payload_as_current();
    let change = &mut payload["units"]["state_changes"][0];
    change["unit"] = unit;
    change["current_state"] = state.into();
    payload.to_string()
}

/// The wire form of a unit subject.
fn subject(name: &str, fragment: &str, owner: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::json!({"name": name, "fragment": fragment});
    if fragment != "absent" {
        value["file"] = name.into();
    }
    if matches!(fragment, "vendor" | "shadowed") {
        value["owning_package"] = owner.into();
    }
    value
}

/// T2, `state_changes`: for each storage kind, only a vendor file with a
/// known owner can be a state change. Every other fragment fails the load
/// naming `classify`, in the `masked` state so that no state rule refuses
/// it first.
#[test]
fn state_change_storage_unit_is_classified_on_load() {
    for name in [
        "tmp.mount",
        "proc-sys-fs-binfmt_misc.automount",
        "dev-zram0.swap",
    ] {
        let owned = payload_with_state_change(subject(name, "vendor", Some("systemd")), "masked");
        let snap = InspectionSnapshot::load(&owned)
            .unwrap_or_else(|err| panic!("{name} vendor with an owner loads: {err}"));
        let units = snap.units.expect("units present");
        assert_eq!(units.state_changes[0].unit().name().as_str(), name);
        assert_eq!(
            units.state_changes[0].unit().owning_package(),
            Some("systemd")
        );

        for (fragment, owner) in [
            ("local", None),
            ("shadowed", Some("systemd")),
            ("absent", None),
            ("vendor", None),
        ] {
            let json = payload_with_state_change(subject(name, fragment, owner), "masked");
            let err = InspectionSnapshot::load(&json)
                .expect_err("an administrator's storage unit is not a state change")
                .to_string();
            assert!(
                err.contains(&format!("classify places {name}")),
                "{name} {fragment}: error must name classify: {err}"
            );
        }
    }
}

/// § 6.7: `vendor_drop_ins` holds file names only, each one path
/// component ending in `.conf`. A value with `/`, or without `.conf`,
/// fails the load naming the field; a list of plain names loads, and so do
/// `install_drop_ins` paths, which are read as destinations at render.
#[test]
fn state_change_vendor_drop_ins_are_file_names_on_load() {
    let with_lists = |vendor: serde_json::Value| {
        let mut payload: serde_json::Value = serde_json::from_str(&payload_with_state_change(
            subject("app.service", "vendor", Some("apppkg")),
            "enabled",
        ))
        .expect("the payload is JSON");
        let change = &mut payload["units"]["state_changes"][0];
        change["vendor_drop_ins"] = vendor;
        change["install_drop_ins"] =
            serde_json::json!(["/etc/systemd/system/app.service.d/10-a.conf"]);
        payload.to_string()
    };

    let snap = InspectionSnapshot::load(&with_lists(serde_json::json!(["10-a.conf"])))
        .expect("plain file names load");
    let change = &snap.units.expect("units present").state_changes[0];
    assert_eq!(change.vendor_drop_ins, ["10-a.conf"]);
    assert_eq!(
        change.install_drop_ins,
        ["/etc/systemd/system/app.service.d/10-a.conf"]
    );

    for refused in [
        "app.service.d/10-a.conf",
        "10-a.txt",
        ".conf",
        "../10-a.conf",
    ] {
        let err = InspectionSnapshot::load(&with_lists(serde_json::json!([refused])))
            .expect_err("not a drop-in file name")
            .to_string();
        assert!(
            err.contains("vendor_drop_ins") && err.contains(refused),
            "{refused}: {err}"
        );
    }
}

/// § 10.2: "holds an `[Install]` section" is one lexical test. True for
/// the header with surrounding whitespace, with a trailing CR, before a
/// backslash-continued assignment, after a comment ending in a backslash
/// (a comment continues nothing), and with a lone CR or a NUL as the line
/// end, as systemd's `read_line` splits lines. False for a `[Service]`-only
/// drop-in, a commented-out header, and a header text that is the
/// continuation of another line.
#[test]
fn holds_install_section_is_one_lexical_test() {
    use inspectah_core::types::units::holds_install_section;
    for (text, expected) in [
        ("[Install]\nWantedBy=a.target\n", true),
        ("  [Install]  \nWantedBy=a.target\n", true),
        ("[Install]\r\nWantedBy=a.target\r\n", true),
        ("[Install]\r", true),
        ("[Install]\nWantedBy=a.target \\\n  b.target\n", true),
        ("[Install]\nWantedBy=\n", true),
        ("# note \\\n[Install]\n", true),
        ("[Service]\nNice=5\n", false),
        ("# [Install]\n", false),
        ("[Service]\nExecStart=/bin/echo \\\n[Install]\n", false),
        ("[Service]\rNice=5\r[Install]\rWantedBy=a.target\r", true),
        ("[Service]\0[Install]\0WantedBy=a.target\0", true),
        ("[Service]\rNice=5\r", false),
    ] {
        assert_eq!(holds_install_section(text), expected, "{text:?}");
    }
}

/// T2, `state_changes`: a template and a unit with no file load masked
/// and fail in any other state, naming the rule.
#[test]
fn state_change_template_and_absent_unit_are_masked_only() {
    let unit_name = |n: &str| UnitName::try_from(n.to_owned()).unwrap();
    let cases = [
        (
            subject("getty@.service", "vendor", Some("systemd")),
            UnitStateChangeError::TemplateNotMasked {
                unit: unit_name("getty@.service"),
                state: UnitState::Enabled,
            },
        ),
        (
            subject("gone.service", "absent", None),
            UnitStateChangeError::AbsentNotMasked {
                unit: unit_name("gone.service"),
                state: UnitState::Enabled,
            },
        ),
    ];
    for (unit, refusal) in cases {
        let name = unit["name"].as_str().unwrap().to_owned();
        InspectionSnapshot::load(&payload_with_state_change(unit.clone(), "masked"))
            .unwrap_or_else(|err| panic!("{name} masked loads: {err}"));
        let err = InspectionSnapshot::load(&payload_with_state_change(unit, "enabled"))
            .expect_err("only a mask is allowed")
            .to_string();
        assert!(
            err.contains(&refusal.to_string()),
            "{name}: error must carry `{refusal}`: {err}"
        );
    }
}

/// T2, `state_changes`: a fragment file that is neither the unit nor its
/// template fails the load. The instance's template file loads.
#[test]
fn state_change_file_must_be_the_unit_or_its_template() {
    let unit = |file: &str| {
        serde_json::json!({
            "name": "getty@tty1.service", "fragment": "vendor", "file": file,
            "owning_package": "systemd"
        })
    };
    InspectionSnapshot::load(&payload_with_state_change(
        unit("getty@.service"),
        "enabled",
    ))
    .expect("an instance loads from its template's file");
    let err = InspectionSnapshot::load(&payload_with_state_change(
        unit("serial-getty@.service"),
        "enabled",
    ))
    .expect_err("another unit's file fails")
    .to_string();
    assert!(
        err.contains(
            "unit file serial-getty@.service is neither getty@tty1.service nor its template"
        ),
        "{err}"
    );
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

/// The serialized keys of a `UnitSection`.
const UNIT_SECTION_KEYS: &[&str] = &[
    "drop_ins",
    "inventory",
    "manual_links",
    "mount_findings",
    "state_changes",
    "undetermined_installs",
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

/// The snapshot format guard: schema 28 and the key sets of the package
/// and unit structs are pinned. Serde ignores unknown fields and
/// defaults missing ones, so a field added, renamed or removed with
/// `#[serde(default)]` passes every round-trip test; this one fails
/// instead. Every `skip_serializing_if` field is set to a non-default
/// value so it appears.
#[test]
fn schema_28_format_guard_pins_package_and_unit_keys() {
    assert_eq!(SCHEMA_VERSION, 28);

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
        serialized_keys(&UnitSection::default()),
        expected_keys(UNIT_SECTION_KEYS)
    );
}
