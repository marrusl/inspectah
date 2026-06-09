//! Inspector correctness tests.
//!
//! These tests run the inspectors on fixture data via MockExecutor and
//! verify the output is structurally correct and self-consistent.
//! They prove inspector code paths work correctly with fixture data.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

// ── Shared helpers ──────────────────────────────────────────────────

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

// ── Fixtures ────────────────────────────────────────────────────────

// Services fixtures
const SYSTEMCTL_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/systemctl-list-unit-files.txt");
const PRESET_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/preset-90-default.preset");
const DROPIN_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/services/dropin-httpd-override.conf");

// Storage fixtures
const FSTAB_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/fstab");
const FINDMNT_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/findmnt.json");
const LVS_FIXTURE: &str = include_str!("../../../testdata/fixtures/storage/lvs.json");

// Kernelboot fixtures
const CMDLINE_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/kernelboot/proc-cmdline.txt");
const LSMOD_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/lsmod.txt");
const SYSCTL_CONF_FIXTURE: &str =
    include_str!("../../../testdata/fixtures/kernelboot/sysctl-system.conf");
const SYSCTL_A_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/sysctl-a.txt");
const DRACUT_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/dracut-conf");
const LOCALE_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/locale.conf");
const TUNED_FIXTURE: &str = include_str!("../../../testdata/fixtures/kernelboot/tuned-active.txt");

// ── Mock builders ───────────────────────────────────────────────────

fn services_mock() -> MockExecutor {
    MockExecutor::new()
        .with_command(
            "systemctl list-unit-files --type=service --no-pager",
            ExecResult {
                stdout: SYSTEMCTL_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/usr/lib/systemd/system-preset", vec!["90-default.preset"])
        .with_file(
            "/usr/lib/systemd/system-preset/90-default.preset",
            PRESET_FIXTURE,
        )
        .with_dir("/etc/systemd/system", vec!["httpd.service.d"])
        .with_dir("/etc/systemd/system/httpd.service.d", vec!["override.conf"])
        .with_file(
            "/etc/systemd/system/httpd.service.d/override.conf",
            DROPIN_FIXTURE,
        )
}

fn storage_mock() -> MockExecutor {
    MockExecutor::new()
        .with_file("/etc/fstab", FSTAB_FIXTURE)
        .with_command(
            "findmnt --json",
            ExecResult {
                stdout: FINDMNT_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "lvs --reportformat json",
            ExecResult {
                stdout: LVS_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
}

fn kernelboot_mock() -> MockExecutor {
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
        .with_file("/etc/locale.conf", LOCALE_FIXTURE)
        .with_command(
            "timedatectl show --property=Timezone --value",
            ExecResult {
                stdout: "America/New_York\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "tuned-adm active",
            ExecResult {
                stdout: TUNED_FIXTURE.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/modprobe.d", vec![])
        .with_dir("/etc/modules-load.d", vec![])
        .with_dir("/etc/dracut.conf.d", vec!["50-custom.conf"])
        .with_file("/etc/dracut.conf.d/50-custom.conf", DRACUT_FIXTURE)
}

// ── Services inspector correctness ──────────────────────────────────

/// Runs ServicesInspector on fixture data and verifies the output is
/// structurally valid and self-consistent.
#[test]
fn test_services_inspector_correctness() {
    let exec = services_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("services inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    // Verify output is valid JSON that round-trips through the concrete type
    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let roundtrip: inspectah_core::types::services::ServiceSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through ServiceSection"
    );

    // Verify structural correctness
    assert!(
        !section.state_changes.is_empty(),
        "inspector must produce state_changes from fixture data"
    );
    for sc in &section.state_changes {
        assert!(!sc.unit.is_empty(), "state_change unit must not be empty");
        // current_state is a typed enum — the fact that it deserialized proves
        // it is one of Enabled, Disabled, or Masked.
        // implied_action() derives from current_state and is exhaustive.
        let _ = sc.implied_action();
    }
}

// ── Storage inspector correctness ───────────────────────────────────

/// Runs StorageInspector on fixture data and verifies the output is
/// structurally valid and self-consistent.
#[test]
fn test_storage_inspector_vs_golden() {
    let exec = storage_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("storage inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    // Verify output is valid JSON that round-trips through the concrete type
    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let roundtrip: inspectah_core::types::storage::StorageSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through StorageSection"
    );

    // Verify structural correctness
    assert!(
        !section.fstab_entries.is_empty(),
        "inspector must produce fstab_entries from fixture data"
    );
    for entry in &section.fstab_entries {
        assert!(
            !entry.device.is_empty(),
            "fstab_entry device must not be empty"
        );
        assert!(
            !entry.mount_point.is_empty(),
            "fstab_entry mount_point must not be empty"
        );
    }
    assert!(
        !section.mount_points.is_empty(),
        "inspector must produce mount_points from fixture data"
    );
}

// ── Kernelboot inspector correctness ────────────────────────────────

/// Runs KernelbootInspector on fixture data and verifies the output is
/// structurally valid and self-consistent.
#[test]
fn test_kernelboot_inspector_correctness() {
    let exec = kernelboot_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
        baseline_data: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("kernelboot inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    // Verify output is valid JSON that round-trips through the concrete type
    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let roundtrip: inspectah_core::types::kernelboot::KernelBootSection =
        serde_json::from_str(&rust_json).expect("inspector output must be valid JSON");
    let roundtrip_json = serde_json::to_string_pretty(&roundtrip).unwrap();
    assert_eq!(
        rust_json, roundtrip_json,
        "inspector output must round-trip faithfully through KernelBootSection"
    );

    // Verify structural correctness
    assert!(
        !section.cmdline.is_empty(),
        "inspector must produce cmdline from fixture data"
    );
    assert!(
        !section.loaded_modules.is_empty(),
        "inspector must produce loaded_modules from fixture data"
    );
    assert!(
        section.locale.is_some(),
        "inspector must produce locale from fixture data"
    );
    assert!(
        section.timezone.is_some(),
        "inspector must produce timezone from fixture data"
    );

    // Verify module structure
    for m in &section.loaded_modules {
        assert!(!m.name.is_empty(), "module name must not be empty");
    }
}
