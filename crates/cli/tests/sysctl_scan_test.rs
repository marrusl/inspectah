//! Spec cases for the sysctl explanation, end to end through the scan path.
//!
//! Every host is built with [`SysctlHost`] and run through the real
//! inspectors (`collect()`, `validate`, `redact`); nothing here builds a
//! `SysctlSection` by hand. Each case has one function per surface:
//! `case_NN_<layout>` for the snapshot and export, then `_audit`, `_html`,
//! `_view` and `_groups` for the surfaces.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::sysctl_hosts::{
    KERNEL_BOOT_FAILURE, SysctlHost, export_session, reload, render_export, scan,
    scan_with_failed_kernel_boot, scan_without_config, scan_without_kernel_boot, session,
};
use axum::response::IntoResponse;
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlCarriage, SysctlDir, SysctlExplanation, SysctlFileBody,
    SysctlFileRef, SysctlKey, SysctlLine, SysctlLineRef, SysctlName, SysctlOutcome, SysctlSection,
    SysctlSetting, UnknownInput, VendorReplacement,
};
use inspectah_pipeline::render::audit::render_audit;
use inspectah_pipeline::render::configtree::PathError;
use inspectah_pipeline::render::report::render_report;
use inspectah_pipeline::render::secrets::render_secrets_review;
use inspectah_pipeline::render::sysctl_report::AGGREGATE_LINE;
use inspectah_pipeline::render::sysctl_report::{
    DRIFT_INTRO, TRUST_LINE, UNAVAILABLE_TAIL, UNKNOWN_INTRO,
};
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};
use inspectah_web::adapter::build_web_view;
use inspectah_web::aggregate_handlers::aggregate_view;
use inspectah_web::handlers::{AppState, get_groups};

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
fn legacy_sysctl_overrides_absent_from_a_scan() {
    // The case 11 host: a file sets a key the kernel reports differently.
    let snap = reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/90-swap.conf", "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let written = json(&snap);
    let kernel_boot = written["kernel_boot"]
        .as_object()
        .expect("kernel_boot is serialized");
    assert!(
        !kernel_boot.contains_key("sysctl_overrides"),
        "the legacy per-key field is gone from the snapshot, got keys: {:?}",
        kernel_boot.keys().collect::<Vec<_>>()
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

/// systemd simplifies a key's path before comparing it, so an exclusion
/// spelled with a leading slash cancels the dotted assignment before it.
#[test]
fn case_10_key_exclusion_other_spelling() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                "/etc/sysctl.d/10-set.conf",
                "vm.swappiness = 10\n-/vm/swappiness\n",
            )
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    assert_eq!(
        resolved(section(&snap)),
        [SysctlSetting {
            key: SysctlKey::normalize("vm.swappiness"),
            outcome: SysctlOutcome::Cancelled {
                by: line(SysctlDir::Etc, "10-set.conf", 2),
                runtime: Some("60".into()),
            },
            overwritten: vec![line(SysctlDir::Etc, "10-set.conf", 1)],
        }],
        "the two spellings are one key, and the last entry writes nothing"
    );
    assert_eq!(runtime_keys(&snap), ["vm.swappiness"]);
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
        (
            "/run/sysctl.d fails partway through its listing",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .file("/run/sysctl.d/50-early.conf", "vm.swappiness = 10\n")
                .dir_iteration_error("/run/sysctl.d", std::io::ErrorKind::Other),
            "sysctl: cannot list /run/sysctl.d:",
        ),
        (
            "a link check that fails other than as not-a-link",
            failure_host()
                .sysctl_a("vm.swappiness = 10\n")
                .file("/etc/sysctl.d/90-x.conf", "vm.dirty_ratio = 20\n")
                .link_error(
                    "/etc/sysctl.d/90-x.conf",
                    std::io::ErrorKind::PermissionDenied,
                ),
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

/// An opaque line no redaction pattern matches, in a file reached only
/// through a linked sysctl directory.
const ANCESTOR_CANARY: &str = "host-canary-94c6";
const ANCESTOR_FILE: &str = "/etc/sysctl.d/90-app.conf";
const ANCESTOR_TARGET: &str = "/var/lib/app/90-app.conf";

/// `/etc/sysctl.d` is itself a link out of the configuration trees; the
/// entry under it is a regular file there.
fn ancestor_link_host() -> SysctlHost {
    SysctlHost::new()
        .link("/etc/sysctl.d", "/var/lib/app")
        .file(
            ANCESTOR_TARGET,
            &format!("vm.swappiness = 10\n{ANCESTOR_CANARY}\n"),
        )
        .file("/usr/lib/sysctl.d/50-default.conf", "vm.swappiness = 60\n")
        .sysctl_a("vm.swappiness = 10\n")
}

#[test]
fn case_17_outside_ancestor_link_snapshot() {
    let snap = scan_without_canary("ancestor link", ancestor_link_host(), ANCESTOR_CANARY);
    let app = section(&snap)
        .files
        .iter()
        .find(|f| f.path() == ANCESTOR_FILE)
        .expect("the entry is collected");
    assert_eq!(
        app.body,
        SysctlFileBody::OutsideLink {
            target: ANCESTOR_TARGET.into()
        },
        "classified by its resolved path, not read"
    );
    assert_eq!(
        section(&snap).effective(),
        SysctlExplanation::Unknown(vec![UnknownInput::OutsideLink {
            file: file_ref(SysctlDir::Etc, "90-app.conf"),
            target: ANCESTOR_TARGET.into(),
        }])
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
        if file.is_dir() {
            continue;
        }
        let bytes = std::fs::read(&file)
            .unwrap_or_else(|e| panic!("{layout}: the export's {relative} reads: {e}"));
        assert!(
            !String::from_utf8_lossy(&bytes).contains(canary),
            "{layout}: `{canary}` survives in the export's {relative}"
        );
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
    // The config pass records the finding at the path; the sysctl pass
    // scrubs its own copy without recording the same match again.
    assert_eq!(findings_at(&secret_in_file, APP_CONF), 1);

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
    assert_eq!(findings_at(&file_only, CORE_CONF), 1);
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
    // records the finding at the link's path, and the sysctl pass does
    // not record it again. The key is a quoted JSON value under a
    // credential key, and that match is the longer one, so the value
    // takes a password token. It takes the quotes and the closing brace
    // too, which costs the JSON its syntax, not a key byte.
    assert_eq!(
        config_entry(&pem, pem_link).content,
        "{\"private_key\":REDACTED_PASSWORD_1\n",
        "{:?}",
        config_entry(&pem, pem_link).content
    );
    assert_eq!(findings_at(&pem, pem_link), 1);

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
        // Redaction keeps the key and replaces only the value.
        let token = lines[0]
            .strip_prefix("db_password = ")
            .unwrap_or_else(|| panic!("{copy}: the key is kept: {:?}", lines[0]));
        assert!(
            token.starts_with("REDACTED_") && !token.contains(char::is_whitespace),
            "{copy}: the value is one token: {token:?}"
        );
        assert_eq!(lines[1], "vm.swappiness = 60", "{copy}");
        assert_eq!(
            content.matches(token).count(),
            1,
            "{copy}: the token occurs only on the password line"
        );
    }
    assert_eq!(findings_at(&equal, APP_CONF), 1);

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

/// The `secrets-review.md` rows under the heading of `path`.
fn review_rows(snap: &InspectionSnapshot, path: &str) -> Vec<String> {
    let heading = format!("## `{path}`");
    render_secrets_review(snap)
        .lines()
        .skip_while(|l| *l != heading)
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Line "))
        .map(str::to_string)
        .collect()
}

#[test]
fn case_18_a_sysctl_file_with_a_config_entry_is_one_finding() {
    // Observed on real RHEL 9.8 and 10.2 hosts: an `/etc/sysctl.d` file
    // with a password line was listed twice in secrets-review.md, once by
    // the config pass and once by the sysctl pass, for the same match.
    let snap = scan_without_canary(
        "secret in an /etc file with a config entry",
        SysctlHost::new()
            .file(APP_CONF, "db_password = canary-realhost\n")
            .sysctl_a("vm.swappiness = 60\n"),
        "canary-realhost",
    );
    assert!(config_entry(&snap, APP_CONF).content.contains("REDACTED_"));
    assert!(read_content(section(&snap), APP_CONF).contains("REDACTED_"));
    let rows = review_rows(&snap, APP_CONF);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(findings_at(&snap, APP_CONF), 1, "{:?}", snap.redactions);
}

#[test]
fn case_18_a_sysctl_only_file_keeps_its_finding() {
    let vendor = "/usr/lib/sysctl.d/60-vendor.conf";
    let snap = scan_without_canary(
        "secret in a /usr/lib file with no config entry",
        SysctlHost::new()
            .file(vendor, "db_password = canary-vendor\n")
            .sysctl_a("vm.swappiness = 60\n"),
        "canary-vendor",
    );
    assert!(
        config_entries_at(&snap, vendor).is_empty(),
        "a vendor file has no config entry"
    );
    assert_eq!(findings_at(&snap, vendor), 1, "{:?}", snap.redactions);
    assert_eq!(review_rows(&snap, vendor).len(), 1);
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
/// heading. Every host value in the block is in a code span, so no line
/// of it starts with `#`.
fn sysctl_block(md: &str) -> Vec<&str> {
    let mut lines = md.lines().skip_while(|l| *l != "#### Sysctl");
    let heading = lines.next().expect("the audit has a `#### Sysctl` block");
    std::iter::once(heading)
        .chain(lines.take_while(|l| !l.starts_with('#')))
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
fn case_17_outside_ancestor_link_audit() {
    let md = audit(&reload(&scan(ancestor_link_host())));
    assert!(
        !md.contains(ANCESTOR_CANARY),
        "the linked directory's file never reaches the audit"
    );
    assert!(
        sysctl_block(&md)
            .iter()
            .any(|line| line.contains(ANCESTOR_TARGET))
    );
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
                let token = content
                    .lines()
                    .next()
                    .and_then(|l| l.strip_prefix("db_password = "))
                    .expect("the password line keeps its key");
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

/// The top-level nodes of a parsed audit's `#### Sysctl` block: the
/// heading's following siblings up to the next heading of level 4 or
/// higher.
fn sysctl_block_nodes<'a>(root: comrak::nodes::Node<'a>) -> Vec<comrak::nodes::Node<'a>> {
    use comrak::nodes::NodeValue;

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
    start
        .following_siblings()
        .skip(1)
        .take_while(|n| !matches!(&n.data.borrow().value, NodeValue::Heading(h) if h.level <= 4))
        .collect()
}

/// The sysctl table's body rows as a GFM parser reads them: each cell is
/// a code span, given as its literal, or fixed text.
fn parsed_table_rows(block: &[comrak::nodes::Node<'_>]) -> Vec<Vec<String>> {
    use comrak::nodes::NodeValue;

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
    table
        .children()
        .skip(1)
        .map(|row| row.children().map(cell_text).collect())
        .collect()
}

/// GFM parsing options with the table extension on.
fn gfm_tables() -> comrak::Options<'static> {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options
}

#[test]
fn case_19_literal_rendering_audit() {
    use comrak::nodes::NodeValue;
    use comrak::{Arena, parse_document};

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
    let root = parse_document(&arena, &md, &gfm_tables());
    let block = sysctl_block_nodes(root);

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

    assert_eq!(
        parsed_table_rows(&block),
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

/// GFM removes the backslash of an unpaired `\|` in a table cell and
/// keeps every backslash pair, so no run of backslashes before `|` reads
/// back as a backslash then a pipe. The cell writes the backslash next to
/// the pipe as `\u{5c}`; every other character is unchanged.
#[test]
fn case_19_literal_rendering_backslash_before_pipe_audit() {
    use comrak::{Arena, parse_document};

    for (backslashes, value, shown) in [
        (1, r"a\|b", r"a\u{5c}|b"),
        (2, r"a\\|b", r"a\\u{5c}|b"),
        (3, r"a\\\|b", r"a\\\u{5c}|b"),
    ] {
        let md = audit(&reload(&scan(
            SysctlHost::new()
                .file(
                    "/etc/sysctl.d/50-core.conf",
                    &format!("kernel.core_pattern = {value}\n"),
                )
                .sysctl_a(&format!("kernel.core_pattern = {value}\n")),
        )));
        let arena = Arena::new();
        let root = parse_document(&arena, &md, &gfm_tables());
        assert_eq!(
            parsed_table_rows(&sysctl_block_nodes(root)),
            [[
                "kernel.core_pattern",
                shown,
                "same",
                "/etc/sysctl.d/50-core.conf:1"
            ]],
            "{backslashes} backslash(es) before a pipe"
        );
    }
}

// ── Case 19, a path the export refuses ───────────────────────────────

/// The case 19 newline file, included, with a same-named vendor file.
fn refused_path_host() -> SysctlHost {
    SysctlHost::new()
        .file(&refused_path(), "vm.swappiness = 10\n")
        .file(
            &format!("{}/{NEWLINE_NAME}", SysctlDir::UsrLib.path()),
            "vm.swappiness = 30\n",
        )
        .sysctl_a("vm.swappiness = 10\n")
}

/// The `/etc` path of the case 19 newline file.
fn refused_path() -> String {
    path(SysctlDir::Etc, NEWLINE_NAME)
}

/// Why the exporter refuses the newline file's path.
fn refusal() -> String {
    PathError::LineBreakOrControl.to_string()
}

/// The wording a vendor line uses only when the replacement reaches the
/// target.
const REACHES_THE_TARGET: &str = "on the target as well";

#[test]
fn case_19_refused_path_export() {
    let snap = reload(&scan(refused_path_host()));
    assert!(
        config_entry(&snap, &refused_path())
            .disposition
            .is_included(),
        "the config entry is included"
    );
    let scan_tree = render_export(&snap);
    let refine_tree = export_session(&session(&snap));
    for (surface, tree) in [("scan", &scan_tree), ("refine", &refine_tree)] {
        let paths = tree_paths(tree.path());
        assert!(
            !paths.iter().any(|p| p.contains("70-new")),
            "{surface}: the export refuses the path, so lacks the file: {paths:?}"
        );
    }
}

#[test]
fn case_19_refused_path_audit() {
    let md = audit(&reload(&scan(refused_path_host())));
    let block = sysctl_block(&md);
    assert!(
        !block.iter().any(|line| line.contains(REACHES_THE_TARGET)),
        "no line claims the replacement reaches the target: {block:#?}"
    );
    let refusal = refusal();
    assert!(
        block.iter().any(|line| line.contains(&refusal)),
        "the not-carried line names the refusal: {block:#?}"
    );
}

#[test]
fn case_19_refused_path_html() {
    let text = html_text(html_sysctl_block(&html(&reload(
        &scan(refused_path_host()),
    ))));
    assert!(!text.contains(REACHES_THE_TARGET), "{text}");
    assert!(text.contains(&refusal()), "{text}");
}

#[test]
fn case_19_refused_path_view() {
    let sysctl = sysctl_view(&session(&reload(&scan(refused_path_host()))));
    let refused = serde_json::json!({
        "kind": "refused",
        "config_path": refused_path(),
        "reason": refusal(),
    });
    assert_eq!(
        sysctl["not_carried"],
        serde_json::json!([{"path": refused_path(), "reason": refused}])
    );
    assert_eq!(sysctl["vendor_replacements"][0]["carriage"], refused);
}

// ── HTML report ──────────────────────────────────────────────────────

/// The HTML report of `snap`, as the scan writes it.
fn html(snap: &InspectionSnapshot) -> String {
    render_report(snap, &RenderContext { target: None })
}

/// The Kernel & Boot count the HTML report shows: its summary card and
/// its section badge, which must agree.
fn kernel_boot_count(html: &str) -> usize {
    let card = html
        .split_once(r##"<dt><a href="#kernel-boot">Kernel &amp; Boot</a></dt>"##)
        .and_then(|(_, rest)| rest.split_once("</dd>"))
        .and_then(|(dd, _)| dd.rsplit_once('>'))
        .map(|(_, count)| count.trim())
        .expect("a Kernel & Boot summary card");
    let badge = html
        .split_once(r#"<details id="kernel-boot">"#)
        .and_then(|(_, rest)| rest.split_once(r#"<span class="badge">("#))
        .and_then(|(_, rest)| rest.split_once(")</span>"))
        .map(|(count, _)| count)
        .expect("a Kernel & Boot section badge");
    assert_eq!(card, badge, "the summary card and the section badge agree");
    card.parse().expect("a numeric count")
}

/// The sysctl block of the HTML report: its heading up to the end of the
/// Kernel & Boot section.
fn html_sysctl_block(html: &str) -> &str {
    html.split_once(r#"<details id="kernel-boot">"#)
        .and_then(|(_, rest)| rest.split_once("<h4>Sysctl</h4>"))
        .and_then(|(_, rest)| rest.split_once("</details>"))
        .map(|(block, _)| block)
        .expect("the Kernel & Boot section has a sysctl block")
}

/// The text of an HTML fragment: inline tags (`code`, `strong`) removed,
/// every other tag a space, runs of whitespace made one space, and the
/// entities MiniJinja's autoescape writes decoded.
fn html_text(fragment: &str) -> String {
    const INLINE_TAGS: [&str; 4] = ["<code>", "</code>", "<strong>", "</strong>"];
    let mut text = String::with_capacity(fragment.len());
    let mut tag: Option<String> = None;
    for c in fragment.chars() {
        match (&mut tag, c) {
            (None, '<') => tag = Some(String::from('<')),
            (Some(open), '>') => {
                open.push('>');
                if !INLINE_TAGS.contains(&open.as_str()) {
                    text.push(' ');
                }
                tag = None;
            }
            (Some(open), _) => open.push(c),
            (None, _) => text.push(c),
        }
    }
    let decoded = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#x2f;", "/")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn case_12_one_drifted_key_does_not_drop_the_file_html() {
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
    let report = html(&snap);
    assert_eq!(kernel_boot_count(&report), 7);
    let text = html_text(html_sysctl_block(&report));
    assert!(text.contains(TRUST_LINE), "{text}");
    assert_eq!(text.matches(" runs at ").count(), 1, "{text}");
    assert!(
        text.contains(
            "vm.swappiness runs at 60; the files request 10 (/etc/sysctl.d/90-tuning.conf:2)."
        ),
        "{text}"
    );
}

#[test]
fn case_17_outside_links_html() {
    for (layout, outside) in [
        ("overlapping assignment", "vm.swappiness = 60\n"),
        ("overlapping exclusion", "-vm.swappiness\n"),
    ] {
        let report = html(&reload(&scan(overlapping_host(outside))));
        assert_eq!(kernel_boot_count(&report), 0, "{layout}");
        let block = html_sysctl_block(&report);
        let text = html_text(block);
        assert!(text.contains(UNKNOWN_INTRO), "{layout}: {text}");
        assert!(
            text.contains(
                "/etc/sysctl.d/90-site.conf links to /usr/share/site/tuning.conf, \
                 outside /etc and the sysctl directories, and inspectah did not read it."
            ),
            "{layout}: {text}"
        );
        assert!(!block.contains("<table"), "{layout}: no settings table");
        assert!(!text.contains(" runs at "), "{layout}: no drift line");
    }

    let canary = html(&reload(&scan(
        SysctlHost::new()
            .link("/etc/sysctl.d/90-app.conf", "/var/lib/app/credentials")
            .file("/var/lib/app/credentials", "password: canary-outside\n"),
    )));
    assert!(!canary.contains("canary-outside"));

    let shadowed = html(&reload(&scan(
        SysctlHost::new()
            .link("/run/sysctl.d/90-site.conf", SITE_TARGET)
            .file(SITE_TARGET, "vm.swappiness = 10\n")
            .file(SITE_LINK, "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    assert_eq!(kernel_boot_count(&shadowed), 1, "shadowed link");
    assert!(!html_text(html_sysctl_block(&shadowed)).contains(UNKNOWN_INTRO));
}

#[test]
fn case_17_outside_ancestor_link_html() {
    let page = html(&reload(&scan(ancestor_link_host())));
    assert!(
        !page.contains(ANCESTOR_CANARY),
        "the linked directory's file never reaches the HTML report"
    );
    assert!(html_text(html_sysctl_block(&page)).contains(ANCESTOR_TARGET));
}

#[test]
fn case_18_redaction_makes_the_explanation_unknown_html() {
    let unknown_layouts = [
        (
            "secret in a file",
            secret_in_a_file_host().sysctl_a("vm.dirty_ratio = 30\nvm.swappiness = 60\n"),
            "canary-inside",
        ),
        (
            "secret only at runtime",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = REDACTED_PASSWORD_1\n")
                .sysctl_a("kernel.core_pattern = password=canary-runtime\n"),
            "canary-runtime",
        ),
        (
            "secret only in the file",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = password=canary-file\n")
                .sysctl_a("kernel.core_pattern = REDACTED_PASSWORD_1\n"),
            "canary-file",
        ),
        (
            "one-line PEM through an allowed link",
            SysctlHost::new()
                .link("/etc/sysctl.d/90-app.conf", "/etc/app/credentials.json")
                .file("/etc/app/credentials.json", ONE_LINE_PEM),
            "U0VDUkVUX0NBTkFSWQ",
        ),
        (
            "equal values under different keys",
            SysctlHost::new()
                .file(APP_CONF, "db_password = 60\nvm.swappiness = 60\n")
                .sysctl_a("vm.swappiness = 60\n"),
            "db_password = 60",
        ),
    ];
    for (layout, host, canary) in unknown_layouts {
        let snap = reload(&scan(host));
        let report = html(&snap);
        assert_eq!(kernel_boot_count(&report), 0, "{layout}");
        let block = html_sysctl_block(&report);
        assert!(
            html_text(block).contains(UNKNOWN_INTRO),
            "{layout}: {block}"
        );
        assert!(!block.contains("<table"), "{layout}: no settings table");
        assert!(
            !html_text(&report).contains(canary),
            "{layout}: `{canary}` is in the HTML report"
        );

        if layout == "equal values under different keys" {
            let content = &config_entry(&snap, APP_CONF).content;
            let token = content
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("db_password = "))
                .expect("the password line keeps its key");
            assert!(token.starts_with("REDACTED_"), "{content:?}");
            assert!(
                !html_text(&report).contains(token),
                "{layout}: `{token}` is in the HTML report"
            );
        }
    }

    let shadowed = html(&reload(&scan(
        SysctlHost::new()
            .file(
                "/usr/lib/sysctl.d/50-x.conf",
                "db_password = canary-shadowed\n",
            )
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    assert_eq!(kernel_boot_count(&shadowed), 1, "shadowed redacted file");
    assert!(!html_text(&shadowed).contains("canary-shadowed"));
}

#[test]
fn case_19_literal_rendering_html() {
    let report = html(&reload(&scan(
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
    let block = html_sysctl_block(&report);
    assert!(block.contains("&lt;img src=x&gt;"), "{block}");
    assert!(!block.contains("<img"), "no `<img` element: {block}");
    assert!(!block.contains("<a "), "no link element: {block}");
    let text = html_text(block);
    for value in [
        "[link](http://example.com)",
        "```",
        CORE_PATTERN,
        "/etc/sysctl.d/50-core.conf:1",
        // The audit's escape for a line break, not a raw newline the
        // browser shows as a space.
        "/etc/sysctl.d/70-new\\u{a}line.conf:1",
    ] {
        assert!(text.contains(value), "`{value}` is in the text: {text}");
    }
    assert!(
        !block.contains("70-new\nline.conf"),
        "no raw newline in a value: {block}"
    );
}

/// An empty written value prints `(empty)`, as the audit prints it.
#[test]
fn case_19_literal_rendering_empty_value_html() {
    let report = html(&reload(&scan(
        SysctlHost::new()
            .file("/etc/sysctl.d/50-empty.conf", "kernel.foo =\n")
            .sysctl_a("kernel.foo = \n"),
    )));
    let block = html_sysctl_block(&report);
    assert!(!block.contains("<code></code>"), "{block}");
    let text = html_text(block);
    assert!(
        text.contains("kernel.foo (empty) same /etc/sysctl.d/50-empty.conf:1"),
        "{text}"
    );
}

// ── Refine view ──────────────────────────────────────────────────────

/// The session's whole view response, as `/api/view` serializes it.
fn view(session: &RefineSession) -> serde_json::Value {
    serde_json::to_value(build_web_view(session)).expect("the view serializes")
}

/// The view's `sysctl` object.
fn sysctl_view(session: &RefineSession) -> serde_json::Value {
    serde_json::to_value(build_web_view(session).sysctl).expect("the sysctl view serializes")
}

/// The view's file row at `path`.
fn view_file<'a>(sysctl: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    sysctl["files"]
        .as_array()
        .expect("files")
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no file row at {path}: {sysctl}"))
}

/// A line ref as the view serializes it.
fn line_json(path: &str, line: u32) -> serde_json::Value {
    serde_json::json!({"path": path, "line": line, "link_target": null})
}

/// Every object key anywhere in `value`.
fn json_keys(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .flat_map(|(key, v)| std::iter::once(key.clone()).chain(json_keys(v)))
            .collect(),
        serde_json::Value::Array(items) => items.iter().flat_map(json_keys).collect(),
        _ => Vec::new(),
    }
}

/// Neither a settings row nor a drift row is anywhere in `sysctl`.
fn assert_no_settings_or_drift(layout: &str, sysctl: &serde_json::Value) {
    let keys = json_keys(sysctl);
    assert!(
        !keys.iter().any(|k| k == "settings" || k == "drift"),
        "{layout}: no settings or drift in {sysctl}"
    );
}

/// The one-row `vm.swappiness` explanation both shadow layouts resolve to.
fn swappiness_writes_60_from(path: &str) -> serde_json::Value {
    serde_json::json!({
        "state": "resolved",
        "settings": [{
            "key": "vm.swappiness",
            "outcome": {
                "kind": "writes",
                "value": "60",
                "from": line_json(path, 1),
                "runtime": {"kind": "matches"},
            },
            "overwritten": [],
        }],
        "drift": [],
    })
}

#[test]
fn case_05_rpm_owned_mask_view() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_CONF, "vm.swappiness = 10\n")
            .link(RPM_MASK, "/dev/null")
            .rpm_owned(SYSCTL_CONF, "procps-ng")
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("....L....", RPM_MASK)
            .sysctl_a("vm.swappiness = 60\n"),
    ));
    let sysctl = sysctl_view(&session(&snap));

    let mask = view_file(&sysctl, RPM_MASK);
    assert_eq!(mask["body"], serde_json::json!({"kind": "mask"}));
    assert_eq!(mask["config_path"], RPM_MASK);
    assert!(
        !sysctl["vendor_replacements"]
            .to_string()
            .contains("99-sysctl.conf"),
        "the mask replaces no vendor file: {}",
        sysctl["vendor_replacements"]
    );
}

#[test]
fn case_06_sysctl_conf_through_the_symlink_view() {
    let snap = reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_CONF, "vm.swappiness = 10\n")
            .link(RPM_MASK, "../sysctl.conf")
            .rpm_owned(SYSCTL_CONF, "procps-ng")
            .rpm_owned(RPM_MASK, "procps-ng")
            .rpm_va("S.5....T.", SYSCTL_CONF)
            .sysctl_a("vm.swappiness = 10\n"),
    ));
    let mut refined = session(&snap);

    let before = sysctl_view(&refined);
    assert_eq!(before["vendor_replacements"], serde_json::json!([]));
    let link = view_file(&before, RPM_MASK);
    assert_eq!(link["config_path"], SYSCTL_CONF);
    assert_eq!(
        link["body"],
        serde_json::json!({"kind": "read", "link_target": SYSCTL_CONF})
    );
    assert_eq!(before["not_carried"], serde_json::json!([]));

    refined
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: SYSCTL_CONF.into(),
            },
            include: false,
        })
        .expect("the exclusion applies");
    let after = sysctl_view(&refined);
    assert_eq!(
        after["not_carried"],
        serde_json::json!([{
            "path": RPM_MASK,
            "reason": {"kind": "excluded", "config_path": SYSCTL_CONF},
        }])
    );
}

#[test]
fn case_12_one_drifted_key_does_not_drop_the_file_view() {
    const TUNING: &str = "/etc/sysctl.d/90-tuning.conf";
    let snap = reload(&scan(
        SysctlHost::new()
            .file(
                TUNING,
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
    let mut refined = session(&snap);

    let sysctl = sysctl_view(&refined);
    let explanation = &sysctl["explanation"];
    assert_eq!(explanation["state"], "resolved");
    assert_eq!(
        explanation["settings"].as_array().expect("settings").len(),
        7,
        "{explanation}"
    );
    assert_eq!(
        explanation["drift"],
        serde_json::json!([{
            "key": "vm.swappiness",
            "file_value": "10",
            "runtime": "60",
            "from": line_json(TUNING, 2),
        }])
    );

    refined
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: TUNING.into(),
            },
            include: false,
        })
        .expect("a sysctl config row is an ordinary toggleable row");
}

#[test]
fn case_15_mask_only_and_glob_only_view() {
    let mask_only = sysctl_view(&session(&reload(&scan(
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .link("/etc/sysctl.d/50-x.conf", "/dev/null")
            .sysctl_a("vm.swappiness = 60\n"),
    ))));
    let glob_only = sysctl_view(&session(&reload(&scan(SysctlHost::new().file(
        "/etc/sysctl.d/50-rp.conf",
        "net.ipv4.conf.*.rp_filter = 2\n",
    )))));

    for (layout, sysctl) in [("mask only", &mask_only), ("glob only", &glob_only)] {
        assert_eq!(sysctl["state"], "host", "{layout}: {sysctl}");
        assert_eq!(
            sysctl["explanation"],
            serde_json::json!({"state": "resolved", "settings": [], "drift": []}),
            "{layout}"
        );
    }
    assert_eq!(
        mask_only["vendor_replacements"],
        serde_json::json!([{
            "action": "masks",
            "path": "/etc/sysctl.d/50-x.conf",
            "vendor_path": "/usr/lib/sysctl.d/50-x.conf",
            "carriage": {"kind": "carried", "config_path": "/etc/sysctl.d/50-x.conf"},
        }])
    );
    assert_eq!(
        glob_only["as_written"],
        serde_json::json!([{
            "kind": "glob_assign",
            "at": line_json("/etc/sysctl.d/50-rp.conf", 1),
            "pattern": "net.ipv4.conf.*.rp_filter",
            "value": "2",
        }])
    );
}

// ── Groups and the aggregate view ───────────────────────────────────

/// The JSON body a handler returns.
async fn handler_json(response: impl IntoResponse) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_response().into_body(), usize::MAX)
        .await
        .expect("the handler body reads");
    serde_json::from_slice(&body).expect("the handler returns JSON")
}

/// The section ids `/api/groups` lists under System Configuration.
async fn system_config_section_ids() -> Vec<String> {
    let groups = handler_json(get_groups().await).await;
    let system_config = groups
        .as_array()
        .expect("groups array")
        .iter()
        .find(|g| g["slug"] == "system-config")
        .unwrap_or_else(|| panic!("no System Configuration group in {groups}"));
    system_config["sections"]
        .as_array()
        .expect("sections array")
        .iter()
        .map(|s| s["id"].as_str().expect("section id").to_string())
        .collect()
}

#[tokio::test]
async fn case_15_mask_only_and_glob_only_groups() {
    let mask_only = session(&reload(&scan(
        SysctlHost::new()
            .file("/usr/lib/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
            .link("/etc/sysctl.d/50-x.conf", "/dev/null")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    let glob_only = session(&reload(&scan(SysctlHost::new().file(
        "/etc/sysctl.d/50-rp.conf",
        "net.ipv4.conf.*.rp_filter = 2\n",
    ))));

    // `/api/groups` reads the registry, not the session, so it answers the
    // same for both hosts; each session is opened to confirm the layout
    // loads as a `host` view, the state the System Tuning panel renders.
    for (layout, held) in [("mask only", mask_only), ("glob only", glob_only)] {
        assert_eq!(sysctl_view(&held)["state"], "host", "{layout}");
        let ids = system_config_section_ids().await;
        assert!(
            ids.iter().any(|id| id == "system_tuning"),
            "{layout}: System Configuration lists system_tuning: {ids:?}"
        );
    }
}

#[test]
fn case_16_collection_failures_view() {
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
        let sysctl = sysctl_view(&session(&reload(&scan(host))));
        let mut keys: Vec<&String> = sysctl.as_object().expect("an object").keys().collect();
        keys.sort();
        assert_eq!(keys, ["reason", "state"], "{layout}: {sysctl}");
        assert_eq!(sysctl["state"], "unavailable", "{layout}");
        let reason = sysctl["reason"].as_str().expect("a reason");
        assert!(reason.contains(cause), "{layout}: `{cause}` in {reason:?}");
    }
}

#[test]
fn case_17_outside_links_view() {
    for (layout, outside) in [
        ("overlapping assignment", "vm.swappiness = 60\n"),
        ("overlapping exclusion", "-vm.swappiness\n"),
    ] {
        let sysctl = sysctl_view(&session(&reload(&scan(overlapping_host(outside)))));
        assert_eq!(
            sysctl["explanation"],
            serde_json::json!({
                "state": "unknown",
                "inputs": [{"kind": "outside_link", "path": SITE_LINK, "target": SITE_TARGET}],
            }),
            "{layout}"
        );
        assert_no_settings_or_drift(layout, &sysctl);
    }

    let canary = view(&session(&reload(&scan(
        SysctlHost::new()
            .link("/etc/sysctl.d/90-app.conf", "/var/lib/app/credentials")
            .file("/var/lib/app/credentials", "password: canary-outside\n"),
    ))));
    assert!(!canary.to_string().contains("canary-outside"));

    let shadowed = sysctl_view(&session(&reload(&scan(
        SysctlHost::new()
            .link("/run/sysctl.d/90-site.conf", SITE_TARGET)
            .file(SITE_TARGET, "vm.swappiness = 10\n")
            .file(SITE_LINK, "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    ))));
    assert_eq!(
        shadowed["explanation"],
        swappiness_writes_60_from(SITE_LINK)
    );
}

#[test]
fn case_17_outside_ancestor_link_view() {
    let session = session(&reload(&scan(ancestor_link_host())));
    assert!(
        !view(&session).to_string().contains(ANCESTOR_CANARY),
        "the linked directory's file never reaches the view"
    );
    assert_eq!(
        sysctl_view(&session)["explanation"],
        serde_json::json!({
            "state": "unknown",
            "inputs": [{"kind": "outside_link", "path": ANCESTOR_FILE, "target": ANCESTOR_TARGET}],
        })
    );
}

#[test]
fn case_18_redaction_makes_the_explanation_unknown_view() {
    let redacted_file = |path: &str| serde_json::json!({"kind": "redacted_file", "path": path});
    let unknown_layouts = [
        (
            "secret in a file",
            secret_in_a_file_host().sysctl_a("vm.dirty_ratio = 30\nvm.swappiness = 60\n"),
            "canary-inside",
            vec![redacted_file(APP_CONF)],
        ),
        (
            "secret only at runtime",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = REDACTED_PASSWORD_1\n")
                .sysctl_a("kernel.core_pattern = password=canary-runtime\n"),
            "canary-runtime",
            vec![serde_json::json!({"kind": "redacted_runtime", "key": "kernel.core_pattern"})],
        ),
        (
            "secret only in the file",
            SysctlHost::new()
                .file(CORE_CONF, "kernel.core_pattern = password=canary-file\n")
                .sysctl_a("kernel.core_pattern = REDACTED_PASSWORD_1\n"),
            "canary-file",
            vec![redacted_file(CORE_CONF)],
        ),
        (
            "one-line PEM through an allowed link",
            SysctlHost::new()
                .link("/etc/sysctl.d/90-app.conf", "/etc/app/credentials.json")
                .file("/etc/app/credentials.json", ONE_LINE_PEM),
            "U0VDUkVUX0NBTkFSWQ",
            vec![redacted_file("/etc/sysctl.d/90-app.conf")],
        ),
        (
            "equal values under different keys",
            SysctlHost::new()
                .file(APP_CONF, "db_password = 60\nvm.swappiness = 60\n")
                .sysctl_a("vm.swappiness = 60\n"),
            "db_password = 60",
            vec![redacted_file(APP_CONF)],
        ),
    ];
    for (layout, host, canary, inputs) in unknown_layouts {
        let snap = reload(&scan(host));
        let refined = session(&snap);
        let sysctl = sysctl_view(&refined);
        assert_eq!(
            sysctl["explanation"],
            serde_json::json!({"state": "unknown", "inputs": inputs}),
            "{layout}"
        );
        assert_no_settings_or_drift(layout, &sysctl);
        assert!(
            !view(&refined).to_string().contains(canary),
            "{layout}: `{canary}` is in the view"
        );

        if layout == "equal values under different keys" {
            let content = &config_entry(&snap, APP_CONF).content;
            let token = content
                .lines()
                .next()
                .and_then(|l| l.strip_prefix("db_password = "))
                .expect("the password line keeps its key");
            assert!(token.starts_with("REDACTED_"), "{content:?}");
            assert!(
                !sysctl.to_string().contains(token),
                "{layout}: `{token}` is in the sysctl view"
            );
        }
    }

    let shadowed = session(&reload(&scan(
        SysctlHost::new()
            .file(
                "/usr/lib/sysctl.d/50-x.conf",
                "db_password = canary-shadowed\n",
            )
            .file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 60\n")
            .sysctl_a("vm.swappiness = 60\n"),
    )));
    assert_eq!(
        sysctl_view(&shadowed)["explanation"],
        swappiness_writes_60_from("/etc/sysctl.d/50-x.conf")
    );
    assert!(!view(&shadowed).to_string().contains("canary-shadowed"));
}

#[test]
fn no_sysctl_item_id_view() {
    let whole = view(&session(&reload(&scan(drifted_key_host()))));
    let object = whole.as_object().expect("an object");
    assert!(!object.contains_key("sysctls"), "no sysctl decision rows");
    assert_eq!(whole["sysctl"]["state"], "host");

    let op = |item_id: serde_json::Value| serde_json::json!({"op": "SetInclude", "target": {"item_id": item_id, "include": false}});
    serde_json::from_value::<RefinementOp>(op(serde_json::json!({
        "kind": "Config",
        "key": {"path": "/etc/sysctl.d/90-swap.conf"},
    })))
    .expect("the positive control: a config op deserializes");
    let refused = serde_json::from_value::<RefinementOp>(op(serde_json::json!({
        "kind": "Sysctl",
        "key": {"key": "vm.swappiness"},
    })));
    assert!(
        refused.is_err(),
        "a sysctl op has no target, got {refused:?}"
    );
}

// ── Case 21 ──────────────────────────────────────────────────────────

/// The `/etc` sysctl file the two fleet hosts disagree on.
const FLEET_FILE: &str = "/etc/sysctl.d/90-x.conf";

/// A fleet host named `hostname` whose [`FLEET_FILE`] holds `content`.
fn fleet_host(hostname: &str, content: &str) -> InspectionSnapshot {
    reload(&scan(
        SysctlHost::new()
            .file("/etc/hostname", &format!("{hostname}\n"))
            .file(FLEET_FILE, content)
            .sysctl_a("vm.swappiness = 10\n"),
    ))
}

#[tokio::test]
async fn case_21_aggregate() {
    let hosts = [
        fleet_host("host-a", "vm.swappiness = 10\n"),
        fleet_host("host-b", "vm.swappiness = 20\n"),
    ];
    for host in &hosts {
        section(host);
        assert!(
            host.kernel_boot
                .as_ref()
                .is_some_and(|kb| kb.tuned_active.is_empty()),
            "no tuned selection on either host"
        );
    }
    let (merged, _warnings) =
        merge_snapshots(hosts.into(), None, None).expect("the two hosts merge");
    let merged = reload(&merged);

    let variants = config_entries_at(&merged, FLEET_FILE);
    let contents: std::collections::BTreeSet<&str> =
        variants.iter().map(|e| e.content.as_str()).collect();
    assert_eq!(
        contents,
        ["vm.swappiness = 10\n", "vm.swappiness = 20\n"].into(),
        "one content variant per host"
    );
    assert_eq!(variants.len(), 2, "two config variants at {FLEET_FILE}");
    assert_eq!(
        merged
            .kernel_boot
            .as_ref()
            .expect("kernel/boot merges")
            .sysctl,
        None,
        "the fleet carries no sysctl explanation"
    );

    assert!(
        audit(&merged).contains(AGGREGATE_LINE),
        "the aggregate audit prints the fleet line"
    );

    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(session(&merged))),
    });
    let view = handler_json(aggregate_view(axum::extract::State(state)).await).await;
    assert_eq!(view["sysctl"], serde_json::json!({"state": "aggregate"}));

    let sections = view["sections"].as_array().expect("sections array");
    let ids: Vec<&str> = sections
        .iter()
        .map(|s| s["id"].as_str().expect("section id"))
        .collect();
    assert!(
        !ids.contains(&"sysctls") && !ids.contains(&"tuned"),
        "no sysctls or tuned section: {ids:?}"
    );
    let tuning = sections
        .iter()
        .find(|s| s["id"] == "system_tuning")
        .unwrap_or_else(|| panic!("no system_tuning section in {ids:?}"));
    assert_eq!(tuning["is_decision_section"], true);
    assert_eq!(
        tuning["items"],
        serde_json::json!([]),
        "a two-host fleet lists items flat, and has none: {tuning}"
    );
}

// ── The explanation without a kernel/boot section ───────────────────

/// The audit's sysctl block as `html_text` reads the HTML one: heading
/// left out, code spans as their text, paragraphs joined by one space.
/// Only for a block whose host values hold no backtick.
fn audit_sysctl_text(md: &str) -> String {
    sysctl_block(md)[1..]
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| line.replace('`', ""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The Kernel & Boot section of the HTML report, open tag to close tag.
fn html_kernel_section(html: &str) -> &str {
    html.split_once(r#"<details id="kernel-boot">"#)
        .and_then(|(_, rest)| rest.split_once("</details>"))
        .map(|(section, _)| section)
        .expect("the HTML report has a Kernel & Boot section")
}

/// Asserts that the HTML report of `snap`, a host with no kernel/boot
/// section, states the audit's unavailable line, and that the view's
/// reason is the audit's; `failed` says whether the section failed, and
/// so whether the generic failed body shows.
fn assert_unavailable_without_kernel_section(snap: &InspectionSnapshot, failed: bool) {
    assert!(snap.kernel_boot.is_none(), "no section");
    let audit_text = audit_sysctl_text(&audit(snap));
    assert!(
        audit_text.starts_with("Sysctl explanation unavailable: ")
            && audit_text.ends_with(UNAVAILABLE_TAIL),
        "{audit_text}"
    );

    let report = html(snap);
    assert_eq!(
        html_text(html_sysctl_block(&report)),
        audit_text,
        "the HTML states what the audit states"
    );
    assert_eq!(
        html_kernel_section(&report).contains("Data collection failed for this section."),
        failed,
        "the generic failed body shows only for a failed section"
    );

    let sysctl = sysctl_view(&session(snap));
    assert_eq!(sysctl["state"], "unavailable");
    let reason = sysctl["reason"].as_str().expect("a reason");
    assert!(
        audit_text.contains(&format!("unavailable: {}.", literal(reason))),
        "the view's reason {reason:?} is the audit's: {audit_text}"
    );
}

#[test]
fn case_16_kernel_section_absent_html() {
    assert_unavailable_without_kernel_section(
        &reload(&scan_without_kernel_boot(failure_host())),
        false,
    );
}

#[test]
fn case_16_kernel_section_failed_html() {
    let snap = reload(&scan_with_failed_kernel_boot(failure_host()));
    assert!(
        audit_sysctl_text(&audit(&snap)).contains(KERNEL_BOOT_FAILURE),
        "the audit names the failure"
    );
    assert_unavailable_without_kernel_section(&snap, true);
}

/// A host value as every surface prints it (`md_literal::literal_text`'s
/// empty case; these reasons hold no control character).
fn literal(value: &str) -> &str {
    if value.is_empty() { "(empty)" } else { value }
}

#[tokio::test]
async fn case_21_aggregate_without_kernel_section_html() {
    let hosts = ["host-a", "host-b"].map(|hostname| {
        reload(&scan_without_kernel_boot(
            SysctlHost::new()
                .file("/etc/hostname", &format!("{hostname}\n"))
                .file(FLEET_FILE, &format!("# {hostname}\nvm.swappiness = 10\n")),
        ))
    });
    let (merged, _warnings) =
        merge_snapshots(hosts.into(), None, None).expect("the two hosts merge");
    let merged = reload(&merged);
    assert!(merged.aggregate_meta.is_some(), "an aggregate");
    assert!(
        merged.kernel_boot.is_none(),
        "no input has a kernel/boot section"
    );

    assert_eq!(audit_sysctl_text(&audit(&merged)), AGGREGATE_LINE);
    assert_eq!(
        html_text(html_sysctl_block(&html(&merged))),
        AGGREGATE_LINE,
        "the HTML states the fleet line the audit states"
    );
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(session(&merged))),
    });
    let view = handler_json(aggregate_view(axum::extract::State(state)).await).await;
    assert_eq!(view["sysctl"], serde_json::json!({"state": "aggregate"}));
}

// ── System Tuning batch toggle ───────────────────────────────────────

/// The session's timeline, generation and redo state, as one value.
fn history(state: &AppState) -> (serde_json::Value, u64, bool) {
    let session = state.session.lock().unwrap();
    (
        serde_json::to_value(session.timeline_history()).expect("the timeline serializes"),
        session.generation(),
        session.can_redo(),
    )
}

#[tokio::test]
async fn system_tuning_toggle_without_a_tuned_profile_records_nothing_groups() {
    const SYSCTL_FILE: &str = "/etc/sysctl.d/90-x.conf";
    let snap = reload(&scan(
        SysctlHost::new()
            .file(SYSCTL_FILE, "vm.swappiness = 10\n")
            .sysctl_a("vm.swappiness = 10\n"),
    ));
    let kernel = snap.kernel_boot.as_ref().expect("kernel/boot is collected");
    assert!(kernel.tuned_active.is_empty(), "no tuned selection");
    assert!(
        !kernel.tuned_disposition.is_advisory(),
        "the collector leaves a non-advisory disposition, which is what made the empty op possible"
    );

    let mut held = session(&snap);
    // A pending redo entry: one config decision, undone.
    held.apply(RefinementOp::SetInclude {
        item_id: ItemId::Config {
            path: SYSCTL_FILE.into(),
        },
        include: false,
    })
    .expect("the config op applies");
    held.undo().expect("the op undoes");
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(held)),
    });
    let before = history(&state);
    assert!(before.2, "the setup leaves a pending redo");

    for include in [true, false] {
        let body = serde_json::to_vec(&serde_json::json!({ "include": include })).unwrap();
        let response = inspectah_web::handlers::batch_toggle_group(
            axum::extract::State(state.clone()),
            axum::extract::Path("system_tuning".to_string()),
            body.into(),
        )
        .await
        .into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "include {include}: the toggle is accepted"
        );
        assert_eq!(
            history(&state),
            before,
            "include {include}: no tuned row, so no op, no generation step, and the redo survives"
        );
    }
}
