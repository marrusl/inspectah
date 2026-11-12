//! Spec cases for the sysctl explanation, end to end through the scan path.
//!
//! Every host is built with [`SysctlHost`] and run through the real
//! inspectors (`collect()`, `validate`, `redact`); nothing here builds a
//! `SysctlSection` by hand. Each case has one function per surface:
//! `case_NN_<layout>` for the snapshot and export, then `_audit`, `_html`,
//! `_view` and `_groups` for the surfaces.

#[path = "support/sysctl_hosts.rs"]
mod sysctl_hosts;

use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlCarriage, SysctlDir, SysctlExplanation, SysctlFileBody,
    SysctlFileRef, SysctlKey, SysctlLine, SysctlLineRef, SysctlName, SysctlOutcome, SysctlSection,
    SysctlSetting, UnknownInput, VendorReplacement,
};
use inspectah_pipeline::render::audit::render_audit;
use inspectah_pipeline::render::sysctl_report::{
    DRIFT_INTRO, TRUST_LINE, UNAVAILABLE_TAIL, UNKNOWN_INTRO,
};
use inspectah_refine::types::{ItemId, RefinementOp};
use sysctl_hosts::{
    SysctlHost, export_session, reload, render_export, scan, scan_without_config, session,
};

// ── Helpers ──────────────────────────────────────────────────────────

/// The collected sysctl section; panics with the warnings when absent.
fn section(snap: &InspectionSnapshot) -> &SysctlSection {
    snap.kernel_boot
        .as_ref()
        .expect("the kernel/boot section is present")
        .sysctl
        .as_ref()
        .unwrap_or_else(|| panic!("no sysctl section; warnings: {:?}", snap.warnings))
}

/// The resolved rows; panics on `Unknown`.
fn resolved(section: &SysctlSection) -> Vec<SysctlSetting> {
    match section.effective() {
        SysctlExplanation::Resolved(settings) => settings,
        other => panic!("expected a resolved explanation, got {other:?}"),
    }
}

/// The row for `key`.
fn setting<'a>(settings: &'a [SysctlSetting], key: &str) -> &'a SysctlSetting {
    settings
        .iter()
        .find(|s| s.key.as_str() == key)
        .unwrap_or_else(|| panic!("no row for {key} in {settings:?}"))
}

fn line(dir: SysctlDir, name: &str, line_no: u32) -> SysctlLineRef {
    SysctlLineRef {
        dir,
        name: name.into(),
        line_no,
    }
}

fn file_ref(dir: SysctlDir, name: &str) -> SysctlFileRef {
    SysctlFileRef {
        dir,
        name: name.into(),
    }
}

fn path(dir: SysctlDir, name: &str) -> String {
    format!("{}/{name}", dir.path())
}

/// The outcome `Writes value from from`, with `runtime`.
fn writes(value: &str, from: SysctlLineRef, runtime: RuntimeObservation) -> SysctlOutcome {
    SysctlOutcome::Writes {
        value: value.into(),
        from,
        runtime,
    }
}

/// The snapshot as written, so assertions read the wire form.
fn json(snap: &InspectionSnapshot) -> serde_json::Value {
    serde_json::to_value(snap).expect("the snapshot serializes")
}

/// The keys of the serialized `kernel_boot.sysctl.runtime` map.
fn runtime_keys(snap: &InspectionSnapshot) -> Vec<String> {
    json(snap)["kernel_boot"]["sysctl"]["runtime"]
        .as_object()
        .expect("the runtime map is serialized")
        .keys()
        .cloned()
        .collect()
}

/// The config entry at `path`; panics when absent.
fn config_entry<'a>(snap: &'a InspectionSnapshot, path: &str) -> &'a ConfigFileEntry {
    snap.config
        .as_ref()
        .expect("the config section is present")
        .files
        .iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("no config entry at {path}"))
}

/// The config entries at `path`.
fn config_entries_at<'a>(snap: &'a InspectionSnapshot, path: &str) -> Vec<&'a ConfigFileEntry> {
    snap.config
        .as_ref()
        .expect("the config section is present")
        .files
        .iter()
        .filter(|entry| entry.path == path)
        .collect()
}

/// How `carriage()` carries the `/etc` file `name`.
fn carriage_of<'a>(
    section: &SysctlSection,
    snap: &'a InspectionSnapshot,
    name: &str,
) -> SysctlCarriage<'a> {
    section
        .carriage(snap.config.as_ref())
        .into_iter()
        .find(|(file, _)| *file == file_ref(SysctlDir::Etc, name))
        .map(|(_, carriage)| carriage)
        .unwrap_or_else(|| panic!("no carriage row for {name}"))
}

/// Every path in `root`, relative to it, files and directories alike.
fn tree_paths(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the export directory reads") {
            let path = entry.expect("the directory entry reads").path();
            found.push(
                path.strip_prefix(root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned(),
            );
            if path.is_dir() {
                pending.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The export's `path` is an empty regular file, not a link.
fn assert_empty_regular_file(tree: &Path, path: &str) {
    let meta = std::fs::symlink_metadata(tree.join(path))
        .unwrap_or_else(|e| panic!("the export lacks {path}: {e}"));
    assert!(meta.is_file(), "{path} is a regular file: {meta:?}");
    assert_eq!(meta.len(), 0, "{path} is empty");
}

/// The `degraded:` warnings the kernel/boot inspector left.
fn kernel_boot_warnings(snap: &InspectionSnapshot) -> Vec<&str> {
    snap.warnings
        .iter()
        .filter(|w| w.inspector == "KernelBoot")
        .map(|w| w.message.as_str())
        .collect()
}

// ── Case 1 ───────────────────────────────────────────────────────────

/// One file per directory, each setting a key no other file names.
fn each_directory_host(sysctl_a: &str) -> SysctlHost {
    SysctlHost::new()
        .file("/etc/sysctl.d/10-etc.conf", "vm.swappiness = 10\n")
        .file("/run/sysctl.d/20-run.conf", "kernel.sysrq = 1\n")
        .file(
            "/usr/local/lib/sysctl.d/30-local.conf",
            "net.ipv4.ip_forward = 1\n",
        )
        .file(
            "/usr/lib/sysctl.d/40-vendor.conf",
            "kernel.pid_max = 4194304\n",
        )
        .sysctl_a(sysctl_a)
}

#[test]
fn case_01_each_directory_is_read() {
    let snap = reload(&scan(each_directory_host(
        "kernel.pid_max = 4194304\nkernel.sysrq = 1\nnet.ipv4.ip_forward = 1\nvm.swappiness = 10\n",
    )));
    let section = section(&snap);
    let settings = resolved(section);

    let expected = [
        (
            "kernel.pid_max",
            "4194304",
            SysctlDir::UsrLib,
            "40-vendor.conf",
        ),
        ("kernel.sysrq", "1", SysctlDir::Run, "20-run.conf"),
        (
            "net.ipv4.ip_forward",
            "1",
            SysctlDir::UsrLocalLib,
            "30-local.conf",
        ),
        ("vm.swappiness", "10", SysctlDir::Etc, "10-etc.conf"),
    ];
    assert_eq!(settings.len(), expected.len(), "{settings:?}");
    for (key, value, dir, name) in expected {
        let row = setting(&settings, key);
        assert_eq!(
            row.outcome,
            writes(value, line(dir, name, 1), RuntimeObservation::Matches),
            "{key}"
        );
        assert!(row.overwritten.is_empty(), "{key}: {row:?}");
    }
    assert_eq!(
        runtime_keys(&snap),
        [
            "kernel.pid_max",
            "kernel.sysrq",
            "net.ipv4.ip_forward",
            "vm.swappiness"
        ]
    );

    let carriage = section.carriage(snap.config.as_ref());
    let carried: Vec<&SysctlFileRef> = carriage.iter().map(|(file, _)| file).collect();
    assert_eq!(
        carried,
        [&file_ref(SysctlDir::Etc, "10-etc.conf")],
        "only /etc files are carried"
    );
}

#[test]
fn case_01_each_directory_is_read_unlisted_key() {
    let snap = reload(&scan(each_directory_host(
        "kernel.pid_max = 4194304\nnet.ipv4.ip_forward = 1\nvm.swappiness = 10\n",
    )));
    let settings = resolved(section(&snap));

    assert_eq!(
        setting(&settings, "kernel.sysrq").outcome,
        writes(
            "1",
            line(SysctlDir::Run, "20-run.conf", 1),
            RuntimeObservation::NotExposed
        )
    );
    for key in ["kernel.pid_max", "net.ipv4.ip_forward", "vm.swappiness"] {
        assert!(
            matches!(
                setting(&settings, key).outcome,
                SysctlOutcome::Writes {
                    runtime: RuntimeObservation::Matches,
                    ..
                }
            ),
            "{key}"
        );
    }
    assert!(
        !runtime_keys(&snap).contains(&"kernel.sysrq".to_string()),
        "a key absent from sysctl -a is absent from the runtime map"
    );
}

// ── Case 2 ───────────────────────────────────────────────────────────

#[test]
fn case_02_priority_by_name_across_directories() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-default.conf", "vm.swappiness = 1\n")
            .file("/etc/sysctl.d/90-local.conf", "vm.swappiness = 2\n")
            .file("/run/sysctl.d/95-run.conf", "vm.swappiness = 3\n")
            .file(
                "/usr/local/lib/sysctl.d/97-local.conf",
                "vm.swappiness = 4\n",
            )
            .sysctl_a("vm.swappiness = 4\n"),
    ));
    let settings = resolved(section(&snap));
    assert_eq!(
        settings,
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "4",
                line(SysctlDir::UsrLocalLib, "97-local.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: vec![
                line(SysctlDir::UsrLib, "50-default.conf", 1),
                line(SysctlDir::Etc, "90-local.conf", 1),
                line(SysctlDir::Run, "95-run.conf", 1),
            ],
        }]
    );

    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/40-local.conf", "vm.swappiness = 2\n")
            .file("/usr/lib/sysctl.d/50-default.conf", "vm.swappiness = 1\n")
            .sysctl_a("vm.swappiness = 1\n"),
    ));
    let settings = resolved(section(&snap));
    assert_eq!(
        settings,
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "1",
                line(SysctlDir::UsrLib, "50-default.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: vec![line(SysctlDir::Etc, "40-local.conf", 1)],
        }],
        "the vendor file applies later by name, so its value wins"
    );
}

// ── Case 3 ───────────────────────────────────────────────────────────

#[test]
fn case_03_same_name_shadow_each_adjacent_pair() {
    let pairs = [
        (SysctlDir::Etc, SysctlDir::Run),
        (SysctlDir::Run, SysctlDir::UsrLocalLib),
        (SysctlDir::UsrLocalLib, SysctlDir::UsrLib),
        (SysctlDir::Etc, SysctlDir::UsrLib),
        (SysctlDir::Run, SysctlDir::UsrLib),
    ];
    for (higher, lower) in pairs {
        let snap = reload(&scan(
            SysctlHost::new()
                .file(&path(higher, "50-x.conf"), "vm.swappiness = 10\n")
                .file(
                    &path(lower, "50-x.conf"),
                    "vm.swappiness = 20\nvm.dirty_ratio = 30\n",
                )
                .sysctl_a("vm.dirty_ratio = 20\nvm.swappiness = 10\n"),
        ));
        let section = section(&snap);
        assert_eq!(
            resolved(section),
            [SysctlSetting {
                key: SysctlKey::normalize("vm.swappiness"),
                outcome: writes(
                    "10",
                    line(higher, "50-x.conf", 1),
                    RuntimeObservation::Matches
                ),
                overwritten: Vec::new(),
            }],
            "{higher:?} over {lower:?}: the lower file is not read, so vm.dirty_ratio is absent"
        );

        let expected_replacements = if (higher, lower) == (SysctlDir::Etc, SysctlDir::UsrLib) {
            vec![VendorReplacement::Shadows {
                name: "50-x.conf".into(),
            }]
        } else {
            Vec::new()
        };
        assert_eq!(
            section.vendor_replacements(),
            expected_replacements,
            "{higher:?} over {lower:?}"
        );
    }
}

// ── Legacy capture ───────────────────────────────────────────────────

#[test]
fn legacy_sysctl_overrides_not_captured() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/90-swap.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let written = json(&snap);
    assert_eq!(
        written["kernel_boot"]["sysctl_overrides"],
        serde_json::json!([]),
        "the legacy per-key capture no longer runs"
    );
    assert_eq!(runtime_keys(&snap), ["vm.swappiness"]);
}

// ── Case 4 ───────────────────────────────────────────────────────────

#[test]
fn case_04_unowned_mask() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/usr/lib/sysctl.d/50-coredump.conf",
                "kernel.core_pattern = |/usr/lib/systemd/systemd-coredump %P\n",
            )
            .link("/etc/sysctl.d/50-coredump.conf", "/dev/null")
            .sysctl_a("kernel.core_pattern = core\n"),
    ));
    let section = section(&snap);
    assert_eq!(resolved(section), [], "no key from the masked vendor file");
    assert_eq!(
        section.vendor_replacements(),
        [VendorReplacement::Masks {
            name: "50-coredump.conf".into()
        }]
    );

    let tree = render_export(&snap);
    assert_empty_regular_file(tree.path(), "config/etc/sysctl.d/50-coredump.conf");
}

// ── Case 5 ───────────────────────────────────────────────────────────

const RPM_MASK: &str = "/etc/sysctl.d/99-sysctl.conf";
const SYSCTL_CONF: &str = "/etc/sysctl.conf";

#[test]
fn case_05_rpm_owned_mask() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_CONF, "vm.swappiness = 10\n")
            .link(RPM_MASK, "/dev/null")
            .rpm_owned(SYSCTL_CONF, "procps-ng")
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("....L....", RPM_MASK)
            .sysctl_a("vm.swappiness = 60\n"),
    ));

    let entries = config_entries_at(&snap, RPM_MASK);
    assert_eq!(entries.len(), 1, "one entry at the link path: {entries:?}");
    let entry = entries[0];
    assert_eq!(entry.kind, ConfigFileKind::RpmOwnedModified);
    assert!(entry.disposition.is_included(), "{:?}", entry.disposition);
    assert_eq!(entry.content, "");
    assert_eq!(entry.rpm_va_flags.as_deref(), Some("....L...."));

    let section = section(&snap);
    let mask = section
        .files
        .iter()
        .find(|f| f.dir == SysctlDir::Etc && f.name == "99-sysctl.conf")
        .expect("the mask is collected");
    assert_eq!(mask.body, SysctlFileBody::Mask);
    assert_eq!(
        resolved(section),
        [],
        "the mask hides /etc/sysctl.conf, so its key is not effective"
    );
    assert_eq!(
        section.vendor_replacements(),
        [],
        "no /usr/lib/sysctl.d/99-sysctl.conf exists, so the mask replaces no vendor file"
    );

    let tree = render_export(&snap);
    assert_empty_regular_file(tree.path(), "config/etc/sysctl.d/99-sysctl.conf");
}

#[test]
fn case_05_rpm_owned_link_to_another_file() {
    let site = "/etc/sysctl-site.conf";
    let snap = reload(&scan(
        SysctlHost::new()
            .file(site, "vm.swappiness = 10\n")
            .link(RPM_MASK, site)
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("....L....", RPM_MASK)
            .sysctl_a("vm.swappiness = 10\n"),
    ));
    assert_eq!(
        config_entries_at(&snap, RPM_MASK),
        Vec::<&ConfigFileEntry>::new(),
        "a link re-pointed to a real file has no config entry"
    );
    assert_eq!(
        carriage_of(section(&snap), &snap, "99-sysctl.conf"),
        SysctlCarriage::NoConfigEntry
    );
}

// ── Case 6 ───────────────────────────────────────────────────────────

#[test]
fn case_06_sysctl_conf_through_the_symlink() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_CONF, "vm.swappiness = 10\n")
            .link(RPM_MASK, "../sysctl.conf")
            .rpm_owned(SYSCTL_CONF, "procps-ng")
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("S.5....T.", SYSCTL_CONF)
            .sysctl_a("vm.swappiness = 10\n"),
    ));
    let section = section(&snap);

    let link = section
        .files
        .iter()
        .find(|f| f.dir == SysctlDir::Etc && f.name == "99-sysctl.conf")
        .expect("the link is collected");
    assert!(
        matches!(
            &link.body,
            SysctlFileBody::Read { link_target: Some(target), .. } if target == SYSCTL_CONF
        ),
        "{:?}",
        link.body
    );
    assert_eq!(
        resolved(section),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "10",
                line(SysctlDir::Etc, "99-sysctl.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: Vec::new(),
        }]
    );

    let carrier = config_entry(&snap, SYSCTL_CONF);
    assert_eq!(
        carriage_of(section, &snap, "99-sysctl.conf"),
        SysctlCarriage::Config(carrier)
    );
    assert!(
        carrier.disposition.is_included(),
        "{:?}",
        carrier.disposition
    );

    let exported = "config/etc/sysctl.conf";
    let tree = render_export(&snap);
    assert_eq!(
        std::fs::read_to_string(tree.path().join(exported))
            .unwrap_or_else(|e| panic!("the export lacks {exported}: {e}")),
        "vm.swappiness = 10\n"
    );

    let mut refined = session(&snap);
    refined
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: SYSCTL_CONF.into(),
            },
            include: false,
        })
        .expect("the exclusion applies");
    let refined_tree = export_session(&refined);
    assert!(
        !refined_tree.path().join(exported).exists(),
        "an excluded /etc/sysctl.conf is not exported"
    );
}

// ── Case 11 ──────────────────────────────────────────────────────────

#[test]
fn case_11_drifted_key_export() {
    /// The drop-in the removed per-key route synthesized.
    const LEGACY_DROP_IN: &str = "99-inspectah-migrated.conf";

    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/90-swap.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let tree = render_export(&snap);
    let paths = tree_paths(tree.path());

    assert!(
        !tree.path().join("sysctl").exists(),
        "no sysctl/ directory: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with(LEGACY_DROP_IN)),
        "no {LEGACY_DROP_IN} anywhere: {paths:?}"
    );
    let containerfile =
        std::fs::read_to_string(tree.path().join("Containerfile")).expect("Containerfile");
    assert!(
        !containerfile.contains(LEGACY_DROP_IN) && !containerfile.contains("sysctl/"),
        "the Containerfile names neither, got:\n{containerfile}"
    );
    assert_eq!(
        std::fs::read_to_string(tree.path().join("config/etc/sysctl.d/90-swap.conf"))
            .expect("the file is carried by the config tree"),
        "vm.swappiness = 10\n"
    );
}

// ── Case 12 ──────────────────────────────────────────────────────────

#[test]
fn case_12_one_drifted_key_does_not_drop_the_file() {
    const TUNING: &str = "/etc/sysctl.d/90-tuning.conf";
    const CONTENT: &str = "# site tuning\n\
        vm.swappiness = 10\n\
        vm.dirty_ratio = 20\n\
        vm.dirty_background_ratio = 5\n\
        kernel.sysrq = 0\n\
        net.ipv4.ip_forward = 1\n\
        net.core.somaxconn = 4096\n\
        fs.file-max = 2097152\n";
    let snap = reload(&scan(SysctlHost::new().file(TUNING, CONTENT).sysctl_a(
        "fs.file-max = 2097152\n\
             kernel.sysrq = 0\n\
             net.core.somaxconn = 4096\n\
             net.ipv4.ip_forward = 1\n\
             vm.dirty_background_ratio = 5\n\
             vm.dirty_ratio = 20\n\
             vm.swappiness = 60\n",
    )));

    let entry = config_entry(&snap, TUNING);
    assert!(entry.disposition.is_included(), "{:?}", entry.disposition);
    assert_eq!(entry.shadowed_by, None);
    assert_eq!(
        json(&snap)["config"]["files"]
            .as_array()
            .expect("config files")
            .iter()
            .find(|e| e["path"] == TUNING)
            .expect("serialized entry")["disposition"]["kind"],
        "actionable"
    );

    let section = section(&snap);
    assert_eq!(
        carriage_of(section, &snap, "90-tuning.conf"),
        SysctlCarriage::Config(entry)
    );
    let settings = resolved(section);
    assert_eq!(settings.len(), 7, "{settings:?}");
    let drifted: Vec<&str> = settings
        .iter()
        .filter(|s| {
            matches!(
                s.outcome,
                SysctlOutcome::Writes {
                    runtime: RuntimeObservation::Differs { .. },
                    ..
                }
            )
        })
        .map(|s| s.key.as_str())
        .collect();
    assert_eq!(drifted, ["vm.swappiness"], "exactly one Differs row");

    let tree = render_export(&snap);
    assert_eq!(
        std::fs::read(tree.path().join("config/etc/sysctl.d/90-tuning.conf"))
            .expect("the file is exported"),
        CONTENT.as_bytes(),
        "byte-identical to the host file"
    );
}

// ── Case 7 ───────────────────────────────────────────────────────────

/// The `/proc/sys` spelling with a dot inside an interface component.
const VLAN_KEY: &str = "net.ipv4.conf.enp3s0/200.forwarding";

#[test]
fn case_07_key_spellings() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/50-a.conf", "net/ipv4/ip_forward = 1\n")
            .sysctl_a("net.ipv4.ip_forward = 1\n"),
    ));
    assert_eq!(
        resolved(section(&snap)),
        [SysctlSetting {
            key: SysctlKey::normalize("net.ipv4.ip_forward"),
            outcome: writes(
                "1",
                line(SysctlDir::Etc, "50-a.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: Vec::new(),
        }]
    );

    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/10-a.conf",
                "net/ipv4/conf/enp3s0.200/forwarding = 1\n",
            )
            .file(
                "/etc/sysctl.d/20-b.conf",
                "net.ipv4.conf.enp3s0/200.forwarding = 0\n",
            )
            .sysctl_a("net.ipv4.conf.enp3s0/200.forwarding = 0\n"),
    ));
    let settings = resolved(section(&snap));
    assert_eq!(
        settings.len(),
        1,
        "both spellings are one key: {settings:?}"
    );
    assert_eq!(settings[0].key.as_str(), VLAN_KEY);
    assert_eq!(
        settings[0].outcome,
        writes(
            "0",
            line(SysctlDir::Etc, "20-b.conf", 1),
            RuntimeObservation::Matches
        )
    );
    assert_eq!(
        settings[0].overwritten,
        [line(SysctlDir::Etc, "10-a.conf", 1)]
    );
    assert_eq!(runtime_keys(&snap), [VLAN_KEY]);
}

// ── Case 8, 9, 13, 14 ────────────────────────────────────────────────

#[test]
fn case_08_repeated_assignment_in_one_file() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-x.conf",
                "# tuning\nvm.swappiness = 1\n\n\nvm.swappiness = 2\n",
            )
            .sysctl_a("vm.swappiness = 2\n"),
    ));
    assert_eq!(
        resolved(section(&snap)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "2",
                line(SysctlDir::Etc, "50-x.conf", 5),
                RuntimeObservation::Matches
            ),
            overwritten: vec![line(SysctlDir::Etc, "50-x.conf", 2)],
        }]
    );
}

#[test]
fn case_09_dash_prefixed_assignment() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/10-a.conf",
                "net.ipv6.conf.all.disable_ipv6 = 0\n",
            )
            .file(
                "/etc/sysctl.d/50-b.conf",
                "-net.ipv6.conf.all.disable_ipv6 = 1\n",
            )
            .sysctl_a("net.ipv6.conf.all.disable_ipv6 = 0\n"),
    ));
    let section = section(&snap);

    let dashed = section
        .files
        .iter()
        .find(|f| f.name == "50-b.conf")
        .expect("the file is collected")
        .lines();
    assert_eq!(
        dashed,
        [SysctlLine::Assign {
            line_no: 1,
            name: SysctlName::Key(SysctlKey::normalize("net.ipv6.conf.all.disable_ipv6")),
            value: "1".into(),
            ignore_failure: true,
        }]
    );
    assert_eq!(
        resolved(section),
        [SysctlSetting {
            key: SysctlKey::normalize("net.ipv6.conf.all.disable_ipv6"),
            outcome: writes(
                "1",
                line(SysctlDir::Etc, "50-b.conf", 1),
                RuntimeObservation::Differs {
                    runtime: "0".into()
                }
            ),
            overwritten: vec![line(SysctlDir::Etc, "10-a.conf", 1)],
        }]
    );
}

#[test]
fn case_13_whitespace_is_not_drift() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-ports.conf",
                "net.ipv4.ip_local_port_range = 32768 60999\n",
            )
            .sysctl_a("net.ipv4.ip_local_port_range = 32768\t60999\n"),
    ));
    let settings = resolved(section(&snap));
    assert!(
        matches!(
            setting(&settings, "net.ipv4.ip_local_port_range").outcome,
            SysctlOutcome::Writes {
                runtime: RuntimeObservation::Matches,
                ..
            }
        ),
        "{settings:?}"
    );
}

#[test]
fn case_14_glob_reported_not_expanded() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-rp.conf",
                "net.ipv4.conf.*.rp_filter = 2\nvm.swappiness = 10\n",
            )
            .sysctl_a("net.ipv4.conf.all.rp_filter = 2\nvm.swappiness = 10\n"),
    ));
    let section = section(&snap);

    let settings = resolved(section);
    let keys: Vec<&str> = settings.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["vm.swappiness"], "a glob adds no row");
    assert_eq!(
        section.files[0].lines()[0],
        SysctlLine::Assign {
            line_no: 1,
            name: SysctlName::Glob("net.ipv4.conf.*.rp_filter".into()),
            value: "2".into(),
            ignore_failure: false,
        }
    );
    assert_eq!(
        runtime_keys(&snap),
        ["vm.swappiness"],
        "a glob is not expanded against sysctl -a"
    );
}

// ── Case 10 ──────────────────────────────────────────────────────────

#[test]
fn case_10_key_exclusion_both_orders() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/10-set.conf", "vm.swappiness = 10\n")
            .file("/etc/sysctl.d/20-unset.conf", "-vm.swappiness\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    assert_eq!(
        resolved(section(&snap)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: SysctlOutcome::Cancelled {
                by: line(SysctlDir::Etc, "20-unset.conf", 1),
                runtime: Some("60".into()),
            },
            overwritten: vec![line(SysctlDir::Etc, "10-set.conf", 1)],
        }]
    );

    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/10-unset.conf", "-vm.swappiness\n")
            .file("/etc/sysctl.d/20-set.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    assert_eq!(
        resolved(section(&snap)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "10",
                line(SysctlDir::Etc, "20-set.conf", 1),
                RuntimeObservation::Differs {
                    runtime: "60".into()
                }
            ),
            overwritten: vec![line(SysctlDir::Etc, "10-unset.conf", 1)],
        }]
    );
}

#[test]
fn case_10_key_exclusion_unlisted_key() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/10-set.conf", "vm.swappiness = 10\n")
            .file("/etc/sysctl.d/20-unset.conf", "-vm.swappiness\n")
            .sysctl_a("kernel.sysrq = 16\n"),
    ));
    let settings = resolved(section(&snap));
    assert_eq!(
        setting(&settings, "vm.swappiness").outcome,
        SysctlOutcome::Cancelled {
            by: line(SysctlDir::Etc, "20-unset.conf", 1),
            runtime: None,
        }
    );
    assert!(runtime_keys(&snap).is_empty(), "only named keys are kept");
}

// ── Case 15 ──────────────────────────────────────────────────────────

#[test]
fn case_15_mask_only_and_glob_only_snapshot() {
    let mask_only = reload(&scan(
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .link("/etc/sysctl.d/50-x.conf", "/dev/null")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let section_a = section(&mask_only);
    assert_eq!(resolved(section_a), [], "the mask leaves no setting");
    assert_eq!(
        section_a.vendor_replacements(),
        [VendorReplacement::Masks {
            name: "50-x.conf".into()
        }]
    );
    let etc = section_a
        .files
        .iter()
        .find(|f| f.dir == SysctlDir::Etc)
        .expect("the mask is collected");
    assert_eq!(etc.body, SysctlFileBody::Mask);

    let glob_only = reload(&scan(SysctlHost::new().file(
        "/etc/sysctl.d/50-rp.conf",
        "net.ipv4.conf.*.rp_filter = 2\n",
    )));
    let section_b = section(&glob_only);
    assert_eq!(resolved(section_b), [], "a glob leaves no setting");
    assert!(section_b.runtime.is_empty(), "no key, so no sysctl -a");
}

// ── Case 16 ──────────────────────────────────────────────────────────

/// The regular `/etc` file every failure host keeps.
const KEPT: &str = "/etc/sysctl.d/10-keep.conf";

fn failure_host() -> SysctlHost {
    SysctlHost::new().file(KEPT, "vm.swappiness = 10\n")
}

#[test]
fn case_16_collection_failures_snapshot() {
    let hosts = [
        (
            "sysctl -a fails",
            failure_host().sysctl_a_fails(1),
            "sysctl: sysctl -a failed with exit code 1",
        ),
        (
            "/run/sysctl.d cannot be listed",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .dir_error("/run/sysctl.d", std::io::ErrorKind::PermissionDenied),
            "sysctl: cannot list /run/sysctl.d:",
        ),
        (
            "a dangling link",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .dangling_link("/etc/sysctl.d/90-x.conf", "/etc/sysctl.d/gone.conf"),
            "sysctl: cannot read /etc/sysctl.d/90-x.conf:",
        ),
    ];
    for (layout, host, cause) in hosts {
        let snap = reload(&scan(host));
        assert_eq!(
            snap.kernel_boot
                .as_ref()
                .expect("the degraded section is present")
                .sysctl,
            None,
            "{layout}: no partial section is stored"
        );
        let warnings = kernel_boot_warnings(&snap);
        assert!(
            warnings.iter().any(|message| message.contains(cause)),
            "{layout}: a kernel/boot warning names `{cause}`, got {warnings:?}"
        );
        let kept = snap
            .config
            .as_ref()
            .expect("the config section is present")
            .files
            .iter()
            .find(|entry| entry.path == KEPT)
            .unwrap_or_else(|| panic!("{layout}: {KEPT} is still a config entry"));
        assert!(
            kept.disposition.is_included(),
            "{layout}: config carriage is unaffected, got {:?}",
            kept.disposition
        );
    }
}

// ── Case 17 ──────────────────────────────────────────────────────────

const SITE_LINK: &str = "/etc/sysctl.d/90-site.conf";
const SITE_TARGET: &str = "/usr/share/site/tuning.conf";

fn overlapping_host(outside_content: &str) -> SysctlHost {
    SysctlHost::new()
        .file("/usr/lib/sysctl.d/50-default.conf", "vm.swappiness = 10\n")
        .link(SITE_LINK, SITE_TARGET)
        .file(SITE_TARGET, outside_content)
        .sysctl_a("vm.swappiness = 60\n")
}

#[test]
fn case_17_outside_links_snapshot() {
    // Canary: the target is recorded, its content is not read.
    let canary = reload(&scan(
        SysctlHost::new()
            .link("/etc/sysctl.d/90-app.conf", "/var/lib/app/credentials")
            .file("/var/lib/app/credentials", "password: canary-outside\n"),
    ));
    let app = section(&canary)
        .files
        .iter()
        .find(|f| f.name == "90-app.conf")
        .expect("the link is collected");
    assert_eq!(
        app.body,
        SysctlFileBody::OutsideLink {
            target: "/var/lib/app/credentials".into()
        }
    );
    let written = serde_json::to_string(&canary).expect("serializes");
    assert!(
        !written.contains("canary-outside"),
        "an outside link's content never reaches the snapshot"
    );

    // Overlapping assignment and overlapping exclusion: the link survives,
    // so nothing is stated.
    for (layout, outside) in [
        ("overlapping assignment", "vm.swappiness = 60\n"),
        ("overlapping exclusion", "-vm.swappiness\n"),
    ] {
        let snap = reload(&scan(overlapping_host(outside)));
        assert_eq!(
            section(&snap).effective(),
            SysctlExplanation::Unknown(vec![UnknownInput::OutsideLink {
                file: file_ref(SysctlDir::Etc, "90-site.conf"),
                target: SITE_TARGET.into(),
            }]),
            "{layout}"
        );
    }

    // Shadowed link: the /etc file of the same name wins, so the link is
    // not read by systemd-sysctl either.
    let shadowed = reload(&scan(
        SysctlHost::new()
            .link("/run/sysctl.d/90-site.conf", SITE_TARGET)
            .file(SITE_TARGET, "vm.swappiness = 10\n")
            .file(SITE_LINK, "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    assert_eq!(
        resolved(section(&shadowed)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "60",
                line(SysctlDir::Etc, "90-site.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: Vec::new(),
        }]
    );
}

// ── Case 18 ──────────────────────────────────────────────────────────

const APP_CONF: &str = "/etc/sysctl.d/80-app.conf";
const CORE_CONF: &str = "/etc/sysctl.d/85-core.conf";
/// The path a runtime value's findings carry.
const PROC_SYS: &str = "/proc/sys";
/// A one-line PEM in JSON, each line break written as the two
/// characters `\n`.
const ONE_LINE_PEM: &str = concat!(
    r#"{"private_key":"-----BEGIN PRIVATE KEY-----\nU0VDUkVUX0NBTkFSWQ==\n"#,
    r#"-----END PRIVATE KEY-----"}"#,
    "\n"
);

/// The host of the *secret in a file* layout, with `sysctl -a` set by
/// the caller.
fn secret_in_a_file_host() -> SysctlHost {
    SysctlHost::new()
        .file(
            APP_CONF,
            "vm.swappiness = 10\ndb_password = canary-inside\n",
        )
        .file("/usr/lib/sysctl.d/10-vendor.conf", "vm.dirty_ratio = 20\n")
}

/// The findings the redaction pass recorded at `path`.
fn findings_at(snap: &InspectionSnapshot, path: &str) -> usize {
    snap.redactions.iter().filter(|f| f.path == path).count()
}

/// The reloaded snapshot of `host`, after asserting that `canary` is in
/// neither the serialized snapshot nor any file of the export.
fn scan_without_canary(layout: &str, host: SysctlHost, canary: &str) -> InspectionSnapshot {
    let snap = reload(&scan(host));
    let written = serde_json::to_string(&snap).expect("the snapshot serializes");
    assert!(
        !written.contains(canary),
        "{layout}: `{canary}` survives in the snapshot"
    );
    let tree = render_export(&snap);
    for relative in tree_paths(tree.path()) {
        let file = tree.path().join(&relative);
        if let Ok(bytes) = std::fs::read(&file) {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(canary),
                "{layout}: `{canary}` survives in the export's {relative}"
            );
        }
    }
    snap
}

/// The content of the sysctl file at `path`; panics unless it was read.
fn read_content<'a>(section: &'a SysctlSection, path: &str) -> &'a str {
    match &section
        .files
        .iter()
        .find(|f| f.path() == path)
        .unwrap_or_else(|| panic!("no sysctl file at {path}"))
        .body
    {
        SysctlFileBody::Read { content, .. } => content,
        other => panic!("{path} was not read: {other:?}"),
    }
}

#[test]
fn case_18_redaction_makes_the_explanation_unknown_snapshot() {
    // Secret in a file.
    let secret_in_file = scan_without_canary(
        "secret in a file",
        secret_in_a_file_host().sysctl_a("vm.dirty_ratio = 30\nvm.swappiness = 60\n"),
        "canary-inside",
    );
    assert_eq!(
        section(&secret_in_file).effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::RedactedFile {
            file: file_ref(SysctlDir::Etc, "80-app.conf"),
        }])
    );
    // The config pass records one finding at the path and the sysctl
    // pass records its own.
    assert_eq!(findings_at(&secret_in_file, APP_CONF), 2);

    // Its twin has no sysctl section to redact; config carriage is the
    // same byte for byte.
    let twin = reload(&scan(secret_in_a_file_host().sysctl_a_fails(1)));
    assert!(
        twin.kernel_boot
            .as_ref()
            .expect("kernel/boot")
            .sysctl
            .is_none(),
        "the twin collects no sysctl section"
    );
    assert_eq!(findings_at(&twin, APP_CONF), 1);
    let entry = config_entry(&secret_in_file, APP_CONF);
    assert!(entry.disposition.is_included(), "{:?}", entry.disposition);
    assert_eq!(
        serde_json::to_string(entry).expect("serializes"),
        serde_json::to_string(config_entry(&twin, APP_CONF)).expect("serializes"),
        "the config entry is the same with and without the sysctl pass"
    );
    let exported = "config/etc/sysctl.d/80-app.conf";
    let read_exported = |snap: &InspectionSnapshot| {
        std::fs::read(render_export(snap).path().join(exported))
            .unwrap_or_else(|e| panic!("the export lacks {exported}: {e}"))
    };
    assert_eq!(read_exported(&secret_in_file), read_exported(&twin));

    // Secret only at runtime: the file's literal equals the token the
    // runtime value would get, and is never compared with it.
    let runtime_only = scan_without_canary(
        "secret only at runtime",
        SysctlHost::new()
            .file(CORE_CONF, "kernel.core_pattern = REDACTED_PASSWORD_1\n")
            .sysctl_a("kernel.core_pattern = password=canary-runtime\n"),
        "canary-runtime",
    );
    let core_pattern = SysctlKey::normalize("kernel.core_pattern");
    let runtime_section = section(&runtime_only);
    assert_eq!(
        runtime_section.effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::RedactedRuntime {
            key: core_pattern.clone(),
        }])
    );
    assert_eq!(
        runtime_section.redacted_runtime,
        std::slice::from_ref(&core_pattern)
    );
    assert!(!runtime_section.runtime.contains_key(&core_pattern));
    assert!(runtime_keys(&runtime_only).is_empty());
    assert_eq!(findings_at(&runtime_only, PROC_SYS), 1);

    // Secret only in the file, token-like runtime: the mirror.
    let file_only = scan_without_canary(
        "secret only in the file",
        SysctlHost::new()
            .file(CORE_CONF, "kernel.core_pattern = password=canary-file\n")
            .sysctl_a("kernel.core_pattern = REDACTED_PASSWORD_1\n"),
        "canary-file",
    );
    assert_eq!(
        section(&file_only).effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::RedactedFile {
            file: file_ref(SysctlDir::Etc, "85-core.conf"),
        }])
    );
    assert!(section(&file_only).redacted_runtime.is_empty());
    assert_eq!(findings_at(&file_only, CORE_CONF), 2);
    assert_eq!(findings_at(&file_only, PROC_SYS), 0);

    // One-line PEM through an allowed link.
    let pem_link = "/etc/sysctl.d/90-app.conf";
    let pem = scan_without_canary(
        "one-line PEM through an allowed link",
        SysctlHost::new()
            .link(pem_link, "/etc/app/credentials.json")
            .file("/etc/app/credentials.json", ONE_LINE_PEM),
        "U0VDUkVUX0NBTkFSWQ",
    );
    assert_eq!(
        section(&pem).effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::RedactedFile {
            file: file_ref(SysctlDir::Etc, "90-app.conf"),
        }])
    );
    // The config walker reads the link through as well, so its entry
    // records one finding at the link's path and the sysctl pass another.
    assert!(
        config_entry(&pem, pem_link)
            .content
            .contains("REDACTED_PRIVATEKEY_"),
        "{:?}",
        config_entry(&pem, pem_link).content
    );
    assert_eq!(findings_at(&pem, pem_link), 2);

    // Equal values under different keys: the canary is the pairing.
    let equal = scan_without_canary(
        "equal values under different keys",
        SysctlHost::new()
            .file(APP_CONF, "db_password = 60\nvm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
        "db_password = 60",
    );
    let equal_section = section(&equal);
    assert_eq!(
        equal_section.effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::RedactedFile {
            file: file_ref(SysctlDir::Etc, "80-app.conf"),
        }])
    );
    for (copy, content) in [
        ("sysctl copy", read_content(equal_section, APP_CONF)),
        (
            "config entry",
            config_entry(&equal, APP_CONF).content.as_str(),
        ),
    ] {
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "{copy}: {content:?}");
        let token = lines[0];
        assert!(
            token.starts_with("REDACTED_") && !token.contains(char::is_whitespace),
            "{copy}: the password line is one token: {token:?}"
        );
        assert_eq!(lines[1], "vm.swappiness = 60", "{copy}");
        assert_eq!(
            content.matches(token).count(),
            1,
            "{copy}: the token occurs only on the password line"
        );
    }
    assert_eq!(findings_at(&equal, APP_CONF), 2);

    // Shadowed redacted file: it does not survive, so nothing is unknown.
    let vendor = "/usr/lib/sysctl.d/50-x.conf";
    let shadowed = scan_without_canary(
        "shadowed redacted file",
        SysctlHost::new()
            .file(vendor, "db_password = canary-shadowed\n")
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
        "canary-shadowed",
    );
    assert_eq!(
        resolved(section(&shadowed)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: writes(
                "60",
                line(SysctlDir::Etc, "50-x.conf", 1),
                RuntimeObservation::Matches
            ),
            overwritten: Vec::new(),
        }]
    );
    assert_eq!(findings_at(&shadowed, vendor), 1);
}

// ── Case 20 ──────────────────────────────────────────────────────────

#[test]
fn case_20_key_on_the_wire() {
    let snap = scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-a.conf",
                "net/ipv4/conf/enp3s0.200/forwarding = 1\n",
            )
            .sysctl_a("net.ipv4.conf.enp3s0/200.forwarding = 1\n"),
    );
    let mut written = json(&snap);
    assert!(
        written["kernel_boot"]["sysctl"]["runtime"]
            .get(VLAN_KEY)
            .is_some(),
        "the runtime map holds the canonical key: {}",
        written["kernel_boot"]["sysctl"]
    );

    let text = serde_json::to_string(&written).expect("serializes");
    let loaded = InspectionSnapshot::load(&text).expect("the canonical key loads");
    assert_eq!(
        runtime_keys(&loaded),
        [VLAN_KEY],
        "and serializes back exactly"
    );

    for renamed in ["net/ipv4/ip_forward", "net/ipv4/conf/enp3s0.200/forwarding"] {
        let mut copy = written.clone();
        let runtime = copy["kernel_boot"]["sysctl"]["runtime"]
            .as_object_mut()
            .expect("the runtime map");
        let value = runtime.remove(VLAN_KEY).expect("the key is present");
        runtime.insert(renamed.into(), value);
        let text = serde_json::to_string(&copy).expect("serializes");
        let error = InspectionSnapshot::load(&text)
            .expect_err("a non-canonical key does not load")
            .to_string();
        assert!(
            error.contains(renamed),
            "the error names `{renamed}`: {error}"
        );
    }

    written["kernel_boot"]["sysctl"]["redacted_runtime"] = serde_json::json!([VLAN_KEY]);
    let text = serde_json::to_string(&written).expect("serializes");
    let error = InspectionSnapshot::load(&text)
        .expect_err("a key in both runtime fields does not load")
        .to_string();
    assert!(error.contains(VLAN_KEY), "the error names the key: {error}");
}

// ── Config absent ────────────────────────────────────────────────────

#[test]
fn config_section_absent_from_a_scan() {
    let snap = reload(&scan_without_config(
        SysctlHost::new()
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 10\n"),
    ));
    assert!(snap.config.is_none(), "no config inspector ran");
    assert_eq!(
        section(&snap).carriage(snap.config.as_ref()),
        [(
            file_ref(SysctlDir::Etc, "50-x.conf"),
            SysctlCarriage::NoConfigEntry
        )]
    );
}

// ── Harness ──────────────────────────────────────────────────────────

/// The scan export and the refine session's export both carry an `/etc`
/// sysctl file at the same place, so later cases can read either tree.
#[test]
fn harness_reaches_every_surface() {
    let snap = scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 10\n"),
    );
    let exported = "config/etc/sysctl.d/50-x.conf";

    let scan_tree = render_export(&snap);
    let refine_tree = export_session(&session(&snap));
    for (surface, tree) in [("scan", &scan_tree), ("refine", &refine_tree)] {
        assert_eq!(
            std::fs::read_to_string(tree.path().join(exported))
                .unwrap_or_else(|e| panic!("{surface} export lacks {exported}: {e}")),
            "vm.swappiness = 10\n",
            "{surface}"
        );
    }
}

// ── Audit ────────────────────────────────────────────────────────────

/// The audit report of `snap`, as the scan writes it.
fn audit(snap: &InspectionSnapshot) -> String {
    render_audit(snap)
}

/// The `#### Sysctl` block of an audit: its heading up to the next
/// heading of the same or a higher level.
fn sysctl_block(md: &str) -> Vec<&str> {
    let mut lines = md.lines().skip_while(|l| *l != "#### Sysctl");
    let heading = lines.next().expect("the audit has a `#### Sysctl` block");
    std::iter::once(heading)
        .chain(lines.take_while(|l| !l.starts_with("### ") && !l.starts_with("#### ")))
        .collect()
}

/// Split a GFM table row into its cells on unescaped pipes.
fn cells(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for c in row.trim().trim_start_matches('|').chars() {
        if c == '|' && !escaped {
            cells.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(c);
        }
        escaped = c == '\\';
    }
    cells
}

/// The body rows of the sysctl table, as cells.
fn table_rows(block: &[&str]) -> Vec<Vec<String>> {
    block
        .iter()
        .filter(|l| l.starts_with('|'))
        .skip(2)
        .map(|l| cells(l))
        .collect()
}

/// The sysctl table's row for `key`.
fn table_row(block: &[&str], key: &str) -> Vec<String> {
    let cell = format!("`{key}`");
    table_rows(block)
        .into_iter()
        .find(|row| row[0] == cell)
        .unwrap_or_else(|| panic!("no table row for {key} in {block:#?}"))
}

/// Drift lines: a line starting with a code span and naming a running
/// value.
fn drift_lines<'a>(block: &[&'a str]) -> Vec<&'a str> {
    block
        .iter()
        .copied()
        .filter(|l| l.starts_with('`') && l.contains(" runs at "))
        .collect()
}

/// The lines of the "As written" item.
fn as_written_lines<'a>(block: &[&'a str]) -> Vec<&'a str> {
    block
        .iter()
        .copied()
        .skip_while(|l| *l != "**As written**")
        .skip(1)
        .take_while(|l| !l.starts_with("**"))
        .filter(|l| !l.is_empty())
        .collect()
}

/// The unknown-explanation lines: those naming an input inspectah does
/// not know as the host had it.
fn unknown_lines<'a>(block: &[&'a str]) -> Vec<&'a str> {
    block
        .iter()
        .copied()
        .filter(|l| {
            l.contains("and inspectah did not read it.")
                || l.contains("held text that redaction replaced")
                || l.starts_with("The running value of ")
        })
        .collect()
}

/// No settings table and no drift line.
fn assert_no_table_or_drift(layout: &str, block: &[&str]) {
    assert!(
        !block.iter().any(|l| l.starts_with('|')),
        "{layout}: no settings table: {block:#?}"
    );
    assert_eq!(drift_lines(block), Vec::<&str>::new(), "{layout}");
}

#[test]
fn case_01_each_directory_is_read_audit() {
    let snap = reload(&scan(each_directory_host(
        "kernel.pid_max = 4194304\nkernel.sysrq = 1\nnet.ipv4.ip_forward = 1\nvm.swappiness = 10\n",
    )));
    let md = audit(&snap);
    let block = sysctl_block(&md);
    assert_eq!(block[2], TRUST_LINE, "{block:#?}");
    assert!(
        block.contains(&"| Key | File value | Runtime value | Source |"),
        "{block:#?}"
    );
    assert_eq!(
        table_rows(&block),
        [
            [
                "`kernel.pid_max`",
                "`4194304`",
                "same",
                "`/usr/lib/sysctl.d/40-vendor.conf:1`"
            ],
            [
                "`kernel.sysrq`",
                "`1`",
                "same",
                "`/run/sysctl.d/20-run.conf:1`"
            ],
            [
                "`net.ipv4.ip_forward`",
                "`1`",
                "same",
                "`/usr/local/lib/sysctl.d/30-local.conf:1`"
            ],
            [
                "`vm.swappiness`",
                "`10`",
                "same",
                "`/etc/sysctl.d/10-etc.conf:1`"
            ],
        ]
    );

    let not_carried: Vec<&str> = block
        .iter()
        .copied()
        .skip_while(|l| *l != "**Not carried**")
        .filter(|l| l.starts_with('`'))
        .collect();
    assert_eq!(
        not_carried,
        [
            "`/run/sysctl.d/20-run.conf`: generated at runtime.",
            "`/usr/local/lib/sysctl.d/30-local.conf`: carried only as unmanaged files under \
             `--include-unmanaged`.",
        ]
    );

    let unlisted = reload(&scan(each_directory_host(
        "kernel.pid_max = 4194304\nnet.ipv4.ip_forward = 1\nvm.swappiness = 10\n",
    )));
    let md = audit(&unlisted);
    assert_eq!(
        table_row(&sysctl_block(&md), "kernel.sysrq")[2],
        "not exposed"
    );
}

#[test]
fn case_04_unowned_mask_audit() {
    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(
                "/usr/lib/sysctl.d/50-coredump.conf",
                "kernel.core_pattern = |/usr/lib/systemd/systemd-coredump %P\n",
            )
            .link("/etc/sysctl.d/50-coredump.conf", "/dev/null")
            .sysctl_a("kernel.core_pattern = core\n"),
    )));
    let block = sysctl_block(&md);
    assert!(
        block.contains(
            &"`/etc/sysctl.d/50-coredump.conf` disables `/usr/lib/sysctl.d/50-coredump.conf` \
              entirely, on the target as well, so keys the target's version of that vendor file \
              sets will not apply from it. Compare it with the target image's copy."
        ),
        "{block:#?}"
    );
    assert!(!block.iter().any(|l| l.starts_with('|')), "{block:#?}");
}

#[test]
fn case_06_sysctl_conf_through_the_symlink_audit() {
    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_CONF, "vm.swappiness = 10\n")
            .link(RPM_MASK, "../sysctl.conf")
            .rpm_owned(SYSCTL_CONF, "procps-ng")
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("S.5....T.", SYSCTL_CONF)
            .sysctl_a("vm.swappiness = 10\n"),
    )));
    let block = sysctl_block(&md);
    assert_eq!(
        table_row(&block, "vm.swappiness")[3],
        "`/etc/sysctl.d/99-sysctl.conf:1` (link to `/etc/sysctl.conf`)"
    );
    assert!(
        !block.contains(&"**Not carried**"),
        "the link is carried by the /etc/sysctl.conf entry: {block:#?}"
    );
}

#[test]
fn case_10_key_exclusion_both_orders_audit() {
    let cancelled = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/10-set.conf", "vm.swappiness = 10\n")
            .file("/etc/sysctl.d/20-unset.conf", "-vm.swappiness\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let md = audit(&cancelled);
    let block = sysctl_block(&md);
    assert_eq!(
        table_row(&block, "vm.swappiness"),
        [
            "`vm.swappiness`",
            "not written",
            "`60`",
            "`/etc/sysctl.d/20-unset.conf:1`"
        ]
    );
    assert_eq!(drift_lines(&block), Vec::<&str>::new());

    let reversed = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/10-unset.conf", "-vm.swappiness\n")
            .file("/etc/sysctl.d/20-set.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let md = audit(&reversed);
    assert_eq!(
        drift_lines(&sysctl_block(&md)),
        ["`vm.swappiness` runs at `60`; the files request `10` (`/etc/sysctl.d/20-set.conf:1`)."]
    );
}

/// The case 11 host, with an optional active tuned profile.
fn drifted_key_host() -> SysctlHost {
    SysctlHost::new()
        .file("/etc/sysctl.d/90-swap.conf", "vm.swappiness = 10\n")
        .sysctl_a("vm.swappiness = 60\n")
}

#[test]
fn case_11_drifted_key_audit() {
    /// The only phrase that predicts the value after a reboot, and only
    /// to say it cannot be read from the scan.
    const AFTER_A_REBOOT: &str = "after a reboot";

    let md = audit(&reload(&scan(drifted_key_host())));
    let block = sysctl_block(&md);
    assert_eq!(
        drift_lines(&block),
        ["`vm.swappiness` runs at `60`; the files request `10` (`/etc/sysctl.d/90-swap.conf:1`)."]
    );
    assert_eq!(md.matches(DRIFT_INTRO).count(), 1, "{block:#?}");
    assert_eq!(
        md.matches(AFTER_A_REBOOT).count(),
        DRIFT_INTRO.matches(AFTER_A_REBOOT).count(),
        "no sentence but the shared paragraph's names the value after a reboot"
    );
    assert!(
        !md.contains("is active on this host"),
        "no tuned sentence without a profile: {block:#?}"
    );

    let md = audit(&reload(&scan(drifted_key_host().tuned("site-tuning"))));
    let block = sysctl_block(&md);
    assert!(
        block.contains(
            &format!("{DRIFT_INTRO} tuned profile `site-tuning` is active on this host.").as_str()
        ),
        "{block:#?}"
    );
}

#[test]
fn case_12_one_drifted_key_does_not_drop_the_file_audit() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/90-tuning.conf",
                "# site tuning\n\
                 vm.swappiness = 10\n\
                 vm.dirty_ratio = 20\n\
                 vm.dirty_background_ratio = 5\n\
                 kernel.sysrq = 0\n\
                 net.ipv4.ip_forward = 1\n\
                 net.core.somaxconn = 4096\n\
                 fs.file-max = 2097152\n",
            )
            .sysctl_a(
                "fs.file-max = 2097152\n\
                 kernel.sysrq = 0\n\
                 net.core.somaxconn = 4096\n\
                 net.ipv4.ip_forward = 1\n\
                 vm.dirty_background_ratio = 5\n\
                 vm.dirty_ratio = 20\n\
                 vm.swappiness = 60\n",
            ),
    ));
    let md = audit(&snap);
    let block = sysctl_block(&md);
    assert_eq!(table_rows(&block).len(), 7, "{block:#?}");
    assert_eq!(
        drift_lines(&block),
        [
            "`vm.swappiness` runs at `60`; the files request `10` (`/etc/sysctl.d/90-tuning.conf:2`)."
        ]
    );
    assert_eq!(md.matches(DRIFT_INTRO).count(), 1);
}

#[test]
fn case_13_whitespace_is_not_drift_audit() {
    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-ports.conf",
                "net.ipv4.ip_local_port_range = 32768 60999\n",
            )
            .sysctl_a("net.ipv4.ip_local_port_range = 32768\t60999\n"),
    )));
    let block = sysctl_block(&md);
    assert_eq!(table_row(&block, "net.ipv4.ip_local_port_range")[2], "same");
    assert_eq!(drift_lines(&block), Vec::<&str>::new());
    assert!(!md.contains(DRIFT_INTRO));
}

#[test]
fn case_14_glob_reported_not_expanded_audit() {
    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-rp.conf",
                "net.ipv4.conf.*.rp_filter = 2\nvm.swappiness = 10\n",
            )
            .sysctl_a("net.ipv4.conf.all.rp_filter = 2\nvm.swappiness = 10\n"),
    )));
    let block = sysctl_block(&md);
    let written = as_written_lines(&block);
    assert!(
        written.iter().any(|l| {
            l.contains("`/etc/sysctl.d/50-rp.conf:1`")
                && l.contains("`net.ipv4.conf.*.rp_filter`")
                && l.contains("`2`")
        }),
        "{written:#?}"
    );
    assert!(
        !table_rows(&block)
            .iter()
            .any(|row| row[0].starts_with("`net.ipv4.conf.")),
        "{block:#?}"
    );
}

#[test]
fn case_16_collection_failures_audit() {
    let hosts = [
        (
            "sysctl -a fails",
            failure_host().sysctl_a_fails(1),
            "sysctl: sysctl -a failed with exit code 1",
        ),
        (
            "/run/sysctl.d cannot be listed",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .dir_error("/run/sysctl.d", std::io::ErrorKind::PermissionDenied),
            "sysctl: cannot list /run/sysctl.d:",
        ),
        (
            "a dangling link",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .dangling_link("/etc/sysctl.d/90-x.conf", "/etc/sysctl.d/gone.conf"),
            "sysctl: cannot read /etc/sysctl.d/90-x.conf:",
        ),
    ];
    for (layout, host, cause) in hosts {
        let md = audit(&reload(&scan(host)));
        let block = sysctl_block(&md);
        let unavailable: Vec<&str> = block
            .iter()
            .copied()
            .filter(|l| l.starts_with("Sysctl explanation unavailable: `"))
            .collect();
        assert_eq!(unavailable.len(), 1, "{layout}: {block:#?}");
        assert!(
            unavailable[0].contains(cause) && unavailable[0].ends_with(UNAVAILABLE_TAIL),
            "{layout}: {unavailable:?}"
        );
        assert_no_table_or_drift(layout, &block);
        assert!(!md.contains("not exposed"), "{layout}");
    }
}

#[test]
fn case_17_outside_links_audit() {
    for (layout, outside) in [
        ("overlapping assignment", "vm.swappiness = 60\n"),
        ("overlapping exclusion", "-vm.swappiness\n"),
    ] {
        let md = audit(&reload(&scan(overlapping_host(outside))));
        let block = sysctl_block(&md);
        assert!(block.contains(&UNKNOWN_INTRO), "{layout}: {block:#?}");
        assert_eq!(
            unknown_lines(&block),
            [
                "`/etc/sysctl.d/90-site.conf` links to `/usr/share/site/tuning.conf`, \
                 outside `/etc` and the sysctl directories, and inspectah did not read it."
            ],
            "{layout}"
        );
        assert_no_table_or_drift(layout, &block);
        assert!(
            !md.lines()
                .any(|l| l.contains("vm.swappiness") && (l.contains("10") || l.contains("60"))),
            "{layout}: no value is stated for vm.swappiness"
        );
    }

    let canary = audit(&reload(&scan(
        SysctlHost::new()
            .link("/etc/sysctl.d/90-app.conf", "/var/lib/app/credentials")
            .file("/var/lib/app/credentials", "password: canary-outside\n"),
    )));
    assert!(!canary.contains("canary-outside"));

    let md = audit(&reload(&scan(
        SysctlHost::new()
            .link("/run/sysctl.d/90-site.conf", SITE_TARGET)
            .file(SITE_TARGET, "vm.swappiness = 10\n")
            .file(SITE_LINK, "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    let block = sysctl_block(&md);
    assert!(!md.contains(UNKNOWN_INTRO), "shadowed link: {block:#?}");
    assert_eq!(
        table_rows(&block),
        [[
            "`vm.swappiness`",
            "`60`",
            "same",
            "`/etc/sysctl.d/90-site.conf:1`"
        ]]
    );
    assert_eq!(drift_lines(&block), Vec::<&str>::new());
}

#[test]
fn case_18_redaction_makes_the_explanation_unknown_audit() {
    let redacted_file = |path: &str| {
        format!("`{path}` held text that redaction replaced, so what it sets is not known.")
    };
    let unknown_layouts = [
        (
            "secret in a file",
            secret_in_a_file_host().sysctl_a("vm.dirty_ratio = 30\nvm.swappiness = 60\n"),
            "canary-inside",
            redacted_file(APP_CONF),
        ),
        (
            "secret only at runtime",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = REDACTED_PASSWORD_1\n")
                .sysctl_a("kernel.core_pattern = password=canary-runtime\n"),
            "canary-runtime",
            "The running value of `kernel.core_pattern` held text that redaction removed."
                .to_string(),
        ),
        (
            "secret only in the file",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = password=canary-file\n")
                .sysctl_a("kernel.core_pattern = REDACTED_PASSWORD_1\n"),
            "canary-file",
            redacted_file(CORE_CONF),
        ),
        (
            "one-line PEM through an allowed link",
            SysctlHost::new()
                .link("/etc/sysctl.d/90-app.conf", "/etc/app/credentials.json")
                .file("/etc/app/credentials.json", ONE_LINE_PEM),
            "U0VDUkVUX0NBTkFSWQ",
            redacted_file("/etc/sysctl.d/90-app.conf"),
        ),
        (
            "equal values under different keys",
            SysctlHost::new()
                .file(APP_CONF, "db_password = 60\nvm.swappiness = 60\n")
                .sysctl_a("vm.swappiness = 60\n"),
            "db_password = 60",
            redacted_file(APP_CONF),
        ),
    ];
    for (layout, host, canary, input_line) in unknown_layouts {
        let snap = reload(&scan(host));
        let md = audit(&snap);
        let block = sysctl_block(&md);
        assert!(block.contains(&UNKNOWN_INTRO), "{layout}: {block:#?}");
        assert_eq!(unknown_lines(&block), [input_line.as_str()], "{layout}");
        assert_no_table_or_drift(layout, &block);
        assert!(!md.contains(canary), "{layout}: `{canary}` is in the audit");

        match layout {
            "one-line PEM through an allowed link" => assert!(
                !as_written_lines(&block)
                    .iter()
                    .any(|l| l.contains("90-app.conf")),
                "{layout}: a redacted file contributes no as-written line"
            ),
            "equal values under different keys" => {
                assert!(
                    !md.lines()
                        .any(|l| l.contains("vm.swappiness") && l.contains("60")),
                    "{layout}: no value is stated for vm.swappiness"
                );
                let content = &config_entry(&snap, APP_CONF).content;
                let token = content.lines().next().expect("the password line");
                assert!(token.starts_with("REDACTED_"), "{content:?}");
                assert!(!md.contains(token), "{layout}: `{token}` is in the audit");
            }
            _ => {}
        }
    }

    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(
                "/usr/lib/sysctl.d/50-x.conf",
                "db_password = canary-shadowed\n",
            )
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    let block = sysctl_block(&md);
    assert!(
        !md.contains(UNKNOWN_INTRO),
        "shadowed redacted file: {block:#?}"
    );
    assert_eq!(
        table_rows(&block),
        [[
            "`vm.swappiness`",
            "`60`",
            "same",
            "`/etc/sysctl.d/50-x.conf:1`"
        ]]
    );
    assert_eq!(drift_lines(&block), Vec::<&str>::new());
    assert!(!md.contains("canary-shadowed"));
}

// ── Case 19 ──────────────────────────────────────────────────────────

/// A value that holds a table-cell separator.
const CORE_PATTERN: &str = "|/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h";
/// A file name holding a line break.
const NEWLINE_NAME: &str = "70-new\nline.conf";

#[test]
fn case_19_literal_rendering_audit() {
    use comrak::nodes::NodeValue;
    use comrak::{Arena, Options, parse_document};

    let md = audit(&reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/50-core.conf",
                &format!("kernel.core_pattern = {CORE_PATTERN}\n"),
            )
            .file(
                "/etc/sysctl.d/60-odd.conf",
                "<img src=x>\n[link](http://example.com)\n```\n",
            )
            .file(
                &format!("/etc/sysctl.d/{NEWLINE_NAME}"),
                "vm.swappiness = 10\n",
            )
            .sysctl_a(&format!(
                "kernel.core_pattern = {CORE_PATTERN}\nvm.swappiness = 60\n"
            )),
    )));

    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.table = true;
    let root = parse_document(&arena, &md, &options);

    // The block's nodes: the `Sysctl` heading's following siblings up to
    // the next heading of level 4 or higher.
    let heading_text = |node: comrak::nodes::Node<'_>| -> String {
        node.descendants()
            .filter_map(|n| match &n.data.borrow().value {
                NodeValue::Text(text) => Some(text.to_string()),
                _ => None,
            })
            .collect()
    };
    let start = root
        .children()
        .find(|n| {
            matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level == 4)
                && heading_text(n) == "Sysctl"
        })
        .expect("a `Sysctl` heading");
    let block: Vec<_> = start
        .following_siblings()
        .skip(1)
        .take_while(|n| !matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level <= 4))
        .collect();

    for node in block.iter().flat_map(|n| n.descendants()) {
        let value = &node.data.borrow().value;
        assert!(
            !matches!(
                value,
                NodeValue::HtmlBlock(_)
                    | NodeValue::HtmlInline(_)
                    | NodeValue::Link(_)
                    | NodeValue::Image(_)
            ),
            "the sysctl block holds no HTML, link or image node: {value:?}"
        );
    }

    // A cell is a code span, whose literal is the value, or fixed text.
    let cell_text = |cell: comrak::nodes::Node<'_>| -> String {
        cell.children()
            .map(|n| match &n.data.borrow().value {
                NodeValue::Code(code) => code.literal.clone(),
                NodeValue::Text(text) => text.to_string(),
                other => panic!("unexpected node in a cell: {other:?}"),
            })
            .collect()
    };
    let table = block
        .iter()
        .find(|n| matches!(n.data.borrow().value, NodeValue::Table(_)))
        .expect("the sysctl table");
    let rows: Vec<Vec<String>> = table
        .children()
        .skip(1)
        .map(|row| row.children().map(cell_text).collect())
        .collect();
    assert_eq!(
        rows,
        [
            [
                "kernel.core_pattern",
                CORE_PATTERN,
                "same",
                "/etc/sysctl.d/50-core.conf:1"
            ],
            [
                "vm.swappiness",
                "10",
                "60",
                "/etc/sysctl.d/70-new\\u{a}line.conf:1"
            ],
        ]
    );

    // Every unparsed line is one code span, holding the text exactly.
    let code_literals: Vec<String> = block
        .iter()
        .flat_map(|n| n.descendants())
        .filter_map(|n| match &n.data.borrow().value {
            NodeValue::Code(code) => Some(code.literal.clone()),
            _ => None,
        })
        .collect();
    for text in ["<img src=x>", "[link](http://example.com)", "```"] {
        assert!(
            code_literals.iter().any(|l| l == text),
            "`{text}` is one code span: {code_literals:#?}"
        );
    }
}
