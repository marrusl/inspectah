//! Spec cases for the sysctl explanation, end to end through the scan path.
//!
//! Every host is built with [`SysctlHost`] and run through the real
//! inspectors (`collect()`, `validate`, `redact`); nothing here builds a
//! `SysctlSection` by hand. Each case has one function per surface:
//! `case_NN_<layout>` for the snapshot and export, then `_audit`, `_html`,
//! `_view` and `_groups` for the surfaces.

#[path = "support/sysctl_hosts.rs"]
mod sysctl_hosts;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlCarriage, SysctlDir, SysctlExplanation, SysctlFileBody,
    SysctlFileRef, SysctlKey, SysctlLine, SysctlLineRef, SysctlName, SysctlOutcome, SysctlSection,
    SysctlSetting, UnknownInput, VendorReplacement,
};
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
