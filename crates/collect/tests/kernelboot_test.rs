//! Integration tests for the Kernelboot inspector.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::{SectionData, SourceSystemKind};
use inspectah_core::types::kernelboot::KernelModule;
use inspectah_core::types::os::OsRelease;
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
        executor: &exec,
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

    // sysctl overrides detected (only where file != runtime)
    assert!(!section.sysctl_overrides.is_empty());

    // locale
    assert_eq!(section.locale, Some("en_US.UTF-8".to_string()));

    // timezone
    assert_eq!(section.timezone, Some("America/New_York".to_string()));

    // tuned profile
    assert_eq!(section.tuned_active, "virtual-guest");
}

// ── Test 2: Sysctl three-way diff ────────────────────────────────────

#[test]
fn sysctl_three_way_diff() {
    let exec = full_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
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

    // vm.swappiness: file=10, runtime=30 → override detected
    let swappiness = section
        .sysctl_overrides
        .iter()
        .find(|o| o.key == "vm.swappiness")
        .expect("swappiness override should exist");
    assert_eq!(swappiness.default, "10"); // file-defined value
    assert_eq!(swappiness.runtime, "30"); // runtime value
    assert!(swappiness.disposition.is_included());

    // kernel.sysrq: file=1, runtime=16 → override detected
    let sysrq = section
        .sysctl_overrides
        .iter()
        .find(|o| o.key == "kernel.sysrq")
        .expect("sysrq override should exist");
    assert_eq!(sysrq.default, "1");
    assert_eq!(sysrq.runtime, "16");

    // net.ipv4.ip_forward: file=1, runtime=1 → NOT an override
    assert!(
        section
            .sysctl_overrides
            .iter()
            .all(|o| o.key != "net.ipv4.ip_forward"),
        "ip_forward matches, should not be an override"
    );
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
        executor: &exec,
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
        executor: &exec,
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
        executor: &exec,
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
        executor: &exec,
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

    assert!(section.sysctl_overrides.is_empty());
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
        executor: &exec,
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
        executor: &exec,
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
        executor: &exec,
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
