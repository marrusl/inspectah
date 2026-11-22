//! Integration tests for the Kernelboot inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::kernelboot::{KernelBootSection, KernelModule};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::sysctl::{
    RuntimeObservation, SysctlExplanation, SysctlFileBody, SysctlOutcome,
};
use inspectah_core::types::system::SourceSystem;

/// Fixture: /proc/cmdline
const CMDLINE_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/kernelboot/proc-cmdline.txt");

/// Fixture: lsmod output
const LSMOD_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/lsmod.txt");

/// Fixture: sysctl.d config file
const SYSCTL_CONF_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/kernelboot/sysctl-system.conf");

/// Fixture: sysctl -a output
const SYSCTL_A_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/sysctl-a.txt");

/// Fixture: dracut.conf.d snippet
const DRACUT_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/dracut-conf");

/// Fixture: locale.conf
const LOCALE_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/locale.conf");

/// Fixture: tuned-adm active output
const TUNED_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/tuned-active.txt");

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

/// Build a MockExecutor with the full fixture set for the happy path.
fn full_mock() -> MockExecutor {
    MockExecutor::new()
        // /proc/cmdline
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        // lsmod
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // sysctl -a
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // sysctl.d dirs
        .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
        .with_file("/etc/sysctl.d/99-custom.conf", SYSCTL_CONF_FIXTURE)
        .with_dir("/usr/lib/sysctl.d", vec![])
        // locale
        .with_file("/etc/locale.conf", LOCALE_FIXTURE)
        // timezone
        .with_command(
            "timedatectl show --property=Timezone --value",
            ExecResult {
                stdout: "America/New_York\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // tuned
        .with_command(
            "tuned-adm active",
            ExecResult {
                stdout: TUNED_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // modprobe.d
        .with_dir("/etc/modprobe.d", vec![])
        // modules-load.d
        .with_dir("/etc/modules-load.d", vec![])
        // dracut.conf.d
        .with_dir("/etc/dracut.conf.d", vec!["50-custom.conf"])
        .with_file("/etc/dracut.conf.d/50-custom.conf", DRACUT_FIXTURE)
}

// ── Test 1: Happy path ──────────────────────────────────────────────

#[test]
fn happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    // cmdline
    assert!(section.cmdline.contains("BOOT_IMAGE="));
    assert!(section.cmdline.contains("crashkernel="));

    // lsmod parsed
    assert!(!section.loaded_modules.is_empty());
    let bridge = section
        .loaded_modules
        .iter()
        .find(|m| m.name == "bridge")
        .expect("bridge module should be parsed");
    assert_eq!(bridge.size, "307200");
    assert_eq!(bridge.used_by, "0");
    assert!(
        bridge.disposition.is_inventory(),
        "the collector records a loaded module as inventory, got {:?}",
        bridge.disposition
    );

    // sysctl search path collected
    assert!(section.sysctl.is_some());

    // locale
    assert_eq!(section.locale, Some("en_US.UTF-8".to_string()));

    // timezone
    assert_eq!(section.timezone, Some("America/New_York".to_string()));

    // tuned profile
    assert_eq!(section.tuned_active, "virtual-guest");
}

// ── Test 3: lsmod failure → Degraded ─────────────────────────────────

#[test]
fn lsmod_failure_returns_degraded() {
    let exec = MockExecutor::new()
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        .with_command(
            "lsmod",
            ExecResult {
                stderr: "lsmod: command not found".into(),
                exit_code: 127,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
        .with_file("/etc/sysctl.d/99-custom.conf", SYSCTL_CONF_FIXTURE)
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let result = KernelbootInspector::new().inspect(&ctx, &NullProgress);
    match result {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("lsmod"),
                "reason should mention lsmod, got: {reason}"
            );
            // Partial should still have cmdline
            if let SectionData::KernelBoot(s) = &partial.section {
                assert!(!s.cmdline.is_empty(), "partial should contain cmdline");
            } else {
                panic!("partial should be KernelBoot section");
            }
        }
        other => panic!("expected Degraded, got {:?}", other),
    }
}

// ── Test 5: tuned not installed → no error ───────────────────────────

#[test]
fn tuned_not_installed() {
    let exec = MockExecutor::new()
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec!["99-custom.conf"])
        .with_file("/etc/sysctl.d/99-custom.conf", SYSCTL_CONF_FIXTURE)
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![]);
    // tuned-adm not registered → MockExecutor returns exit 127

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    assert_eq!(section.tuned_active, "");
}

// ── Test 7: cmdline with password → RedactionHint ────────────────────

#[test]
fn cmdline_with_password() {
    let exec = MockExecutor::new()
        .with_file(
            "/proc/cmdline",
            "BOOT_IMAGE=/vmlinuz root=UUID=abc password=hunter2 quiet\n",
        )
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec![])
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();

    let hint = output
        .redaction_hints
        .iter()
        .find(|h| h.path.contains("/proc/cmdline"))
        .expect("should have RedactionHint for cmdline password");
    assert!(hint.reason.contains("password"));
}

// ── Test 8: Empty system ─────────────────────────────────────────────

#[test]
fn empty_system() {
    let exec = MockExecutor::new()
        .with_file("/proc/cmdline", "root=UUID=abc ro quiet\n")
        .with_command(
            "lsmod",
            ExecResult {
                stdout: "Module                  Size  Used by\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: "kernel.sysrq = 16\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec![])
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![]);

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    assert!(section.loaded_modules.is_empty());
    assert!(section.tuned_active.is_empty());
    assert!(output.redaction_hints.is_empty());
}

// ── Test 8b: loaded module disposition defaults to inventory ─────────

/// A loaded module is inventory: it is displayed and never mounted on a
/// decision. A snapshot that omits the key must not read back as an
/// actionable, included finding.
#[test]
fn absent_loaded_module_disposition_deserializes_as_inventory() {
    let json = r#"{"name":"bridge","size":"307200","used_by":"0"}"#;
    let module: KernelModule = serde_json::from_str(json).unwrap();
    assert!(
        module.disposition.is_inventory(),
        "absent disposition must deserialize as inventory, got {:?}",
        module.disposition
    );
}

// ── Test 9: Applicability ────────────────────────────────────────────

#[test]
fn applicability_package_mode_only() {
    let inspector = KernelbootInspector::new();
    assert_eq!(inspector.applicable_to(), &[SourceSystemKind::PackageBased]);
}

// ── Test 10: Snapshot test ───────────────────────────────────────────

#[test]
fn kernelboot_snapshot() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    insta::assert_json_snapshot!(section);
}

// ── Test 11: tuned_include behavior ──────────────────────────────────

#[test]
fn tuned_detected_profile_auto_includes() {
    let exec = MockExecutor::new()
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec![])
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![])
        .with_command(
            "tuned-adm active",
            ExecResult {
                stdout: "Current active profile: throughput-performance\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    assert_eq!(section.tuned_active, "throughput-performance");
    assert!(
        section.tuned_disposition.is_included(),
        "any detected tuned profile should auto-include"
    );
}

#[test]
fn tuned_no_profile_does_not_include() {
    let exec = MockExecutor::new()
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: SYSCTL_A_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/sysctl.d", vec![])
        .with_dir("/usr/lib/sysctl.d", vec![])
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec![]);
    // tuned-adm not registered → returns exit 127

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(&exec),
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .unwrap();
    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    assert!(section.tuned_active.is_empty());
    assert!(
        !section.tuned_disposition.is_included(),
        "no tuned profile detected should not include"
    );
}

// ── Sysctl section: entries, links and `sysctl -a` noise ─────────────

/// The kernel/boot primaries other than sysctl, answering normally.
fn sysctl_base_mock() -> MockExecutor {
    MockExecutor::new()
        .with_file("/proc/cmdline", CMDLINE_FIXTURE)
        .with_command(
            "lsmod",
            ExecResult {
                stdout: LSMOD_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
}

fn sysctl_a_ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

fn inspect(exec: &MockExecutor) -> Result<InspectorOutput, InspectorError> {
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &inspectah_core::machine_state::ScreenedExecutor::new(exec),
        rpm_state: None,
        baseline_data: None,
    };
    KernelbootInspector::new().inspect(&ctx, &NullProgress)
}

/// The kernel/boot section of a successful inspection.
fn kernel_boot(output: &InspectorOutput) -> &KernelBootSection {
    match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {other:?}"),
    }
}

/// Review focus 2: only `*.conf` entries are read, and an unreadable
/// entry of another name does not make the explanation unavailable.
#[test]
fn non_conf_entries_are_ignored() {
    let exec = sysctl_base_mock()
        .with_dir(
            "/etc/sysctl.d",
            vec!["50-x.conf", "50-x.conf.rpmnew", "50-x.conf~", "README"],
        )
        .with_file("/etc/sysctl.d/50-x.conf", "vm.swappiness = 10\n")
        .with_file_error(
            "/etc/sysctl.d/50-x.conf.rpmnew",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_file_error(
            "/etc/sysctl.d/50-x.conf~",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_file_error("/etc/sysctl.d/README", std::io::ErrorKind::PermissionDenied)
        .with_command("sysctl -a", sysctl_a_ok("vm.swappiness = 10\n"));

    let output = inspect(&exec).expect("unreadable non-.conf entries are not failures");
    let sysctl = kernel_boot(&output)
        .sysctl
        .as_ref()
        .expect("the sysctl section is collected");
    let names: Vec<&str> = sysctl.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["50-x.conf"]);
}

/// Review focus 3: a chain ending at `/dev/null` is a mask, a chain
/// ending under `/etc` is read through and records the final target, and
/// a loop is a read failure.
///
/// `MockExecutor::read_file` does not follow links, so each link that
/// resolves also registers, at its own path, what a real read returns
/// there (`""` for `/dev/null`).
#[test]
fn link_chains_and_loops() {
    const ENTRY: &str = "/etc/sysctl.d/50-x.conf";

    let mask = sysctl_base_mock()
        .with_dir("/etc/sysctl.d", vec!["50-x.conf"])
        .with_link(ENTRY, "/etc/alt/x.conf")
        .with_link("/etc/alt/x.conf", "/dev/null")
        .with_file(ENTRY, "")
        .with_file("/etc/alt/x.conf", "");
    let output = inspect(&mask).expect("a two-hop mask is collected");
    let sysctl = kernel_boot(&output)
        .sysctl
        .as_ref()
        .expect("sysctl section");
    assert_eq!(sysctl.files[0].body, SysctlFileBody::Mask);

    let content = "vm.swappiness = 10\n";
    let read_through = sysctl_base_mock()
        .with_dir("/etc/sysctl.d", vec!["50-x.conf"])
        .with_link(ENTRY, "/etc/alt/a.conf")
        .with_link("/etc/alt/a.conf", "../tuning/b.conf")
        .with_file("/etc/tuning/b.conf", content)
        .with_file("/etc/alt/a.conf", content)
        .with_file(ENTRY, content)
        .with_command("sysctl -a", sysctl_a_ok("vm.swappiness = 10\n"));
    let output = inspect(&read_through).expect("a two-hop chain under /etc is read");
    let sysctl = kernel_boot(&output)
        .sysctl
        .as_ref()
        .expect("sysctl section");
    assert_eq!(
        sysctl.files[0].body,
        SysctlFileBody::Read {
            link_target: Some("/etc/tuning/b.conf".into()),
            content: content.into(),
            redacted: false,
        }
    );

    let looped = sysctl_base_mock()
        .with_dir("/etc/sysctl.d", vec!["50-x.conf"])
        .with_link(ENTRY, "/etc/alt/a.conf")
        .with_link("/etc/alt/a.conf", ENTRY);
    match inspect(&looped) {
        Err(InspectorError::Degraded { reason, partial }) => {
            assert!(
                reason.contains("sysctl: cannot read /etc/sysctl.d/50-x.conf:"),
                "the loop is named as a read failure, got: {reason}"
            );
            match &partial.section {
                SectionData::KernelBoot(s) => {
                    assert_eq!(s.sysctl, None, "no partial sysctl section is stored")
                }
                other => panic!("expected SectionData::KernelBoot, got {other:?}"),
            }
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

/// Review focus 5: `sysctl -a` that exits 0 with per-key read errors on
/// stderr and a stdout line with no `=` is a success. Listed keys are
/// used; an unlisted key is not exposed.
#[test]
fn sysctl_a_stderr_noise_is_not_a_failure() {
    let exec = sysctl_base_mock()
        .with_dir("/etc/sysctl.d", vec!["50-x.conf"])
        .with_file(
            "/etc/sysctl.d/50-x.conf",
            "vm.swappiness = 10\nnet.ipv6.conf.all.stable_secret = abc\n",
        )
        .with_command(
            "sysctl -a",
            ExecResult {
                stdout: "vm.swappiness = 10\nnet.ipv6.conf.all.stable_secret\n".into(),
                stderr: "sysctl: reading key \"net.ipv6.conf.all.stable_secret\"\n".into(),
                exit_code: 0,
            },
        );

    let output = inspect(&exec).expect("stderr noise is not a failure");
    let sysctl = kernel_boot(&output)
        .sysctl
        .as_ref()
        .expect("sysctl section");
    let settings = match sysctl.effective() {
        SysctlExplanation::Resolved(settings) => settings,
        other => panic!("expected a resolved explanation, got {other:?}"),
    };
    let runtime_of = |key: &str| match &settings
        .iter()
        .find(|s| s.key.as_str() == key)
        .unwrap_or_else(|| panic!("no row for {key}"))
        .outcome
    {
        SysctlOutcome::Writes { runtime, .. } => runtime.clone(),
        other => panic!("{key}: expected Writes, got {other:?}"),
    };
    assert_eq!(runtime_of("vm.swappiness"), RuntimeObservation::Matches);
    assert_eq!(
        runtime_of("net.ipv6.conf.all.stable_secret"),
        RuntimeObservation::NotExposed
    );
}
