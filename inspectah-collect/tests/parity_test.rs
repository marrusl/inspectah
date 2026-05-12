//! Inspector-exercising parity gate tests.
//!
//! These tests run the actual Rust inspectors on fixture data via
//! MockExecutor, serialize the output sections to JSON, and diff against
//! the golden files in `testdata/golden/`.
//!
//! The golden files are currently provisional (Rust-authored), so these
//! tests pass trivially. When real Go-captured goldens replace them, any
//! actual divergences will surface as test failures. The key is that the
//! test STRUCTURE exercises the real inspector code path — fixtures in,
//! inspector runs, output compared to golden.
//!
//! For serde roundtrip tests (golden JSON -> Rust type -> JSON), see
//! `inspectah-core/tests/parity_gate.rs`.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::services::ServicesInspector;
use inspectah_collect::inspectors::storage::StorageInspector;
use inspectah_core::normalize::{diff_snapshots, load_divergence_allowlist};
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector};
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use std::collections::BTreeSet;

// ── Shared helpers ──────────────────────────────────────────────────

fn allowlist() -> BTreeSet<String> {
    let md = include_str!("../../testdata/divergences.md");
    load_divergence_allowlist(md)
}

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

fn format_diffs(diffs: &[inspectah_core::normalize::Difference]) -> String {
    diffs
        .iter()
        .map(|d| format!("  {}: golden={}, rust={}", d.path, d.go_value, d.rust_value))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Fixtures ────────────────────────────────────────────────────────

// Services fixtures
const SYSTEMCTL_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/systemctl-list-unit-files.txt");
const PRESET_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/preset-90-default.preset");
const DROPIN_FIXTURE: &str =
    include_str!("../../testdata/fixtures/services/dropin-httpd-override.conf");

// Storage fixtures
const FSTAB_FIXTURE: &str = include_str!("../../testdata/fixtures/storage/fstab");
const FINDMNT_FIXTURE: &str = include_str!("../../testdata/fixtures/storage/findmnt.json");
const LVS_FIXTURE: &str = include_str!("../../testdata/fixtures/storage/lvs.json");

// Kernelboot fixtures
const CMDLINE_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/proc-cmdline.txt");
const LSMOD_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/lsmod.txt");
const SYSCTL_CONF_FIXTURE: &str =
    include_str!("../../testdata/fixtures/kernelboot/sysctl-system.conf");
const SYSCTL_A_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/sysctl-a.txt");
const DRACUT_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/dracut-conf");
const LOCALE_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/locale.conf");
const TUNED_FIXTURE: &str = include_str!("../../testdata/fixtures/kernelboot/tuned-active.txt");

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

// ── Services inspector vs golden ────────────────────────────────────

/// Parity gate: runs ServicesInspector on fixture data, serializes the
/// output section, and diffs against the golden file. Currently goldens
/// are provisional (Rust-authored) so this passes trivially. When real
/// Go-captured goldens replace them, divergences surface here.
#[test]
fn test_services_inspector_vs_golden() {
    let exec = services_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = ServicesInspector::new()
        .inspect(&ctx)
        .expect("services inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Services(s) => s,
        other => panic!("expected SectionData::Services, got {:?}", other),
    };

    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let golden = include_str!("../../testdata/golden/go-v13-services-section.json");
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Services inspector output diverges from golden (undocumented):\n{}",
        format_diffs(&undocumented)
    );
}

// ── Storage inspector vs golden ─────────────────────────────────────

/// Parity gate: runs StorageInspector on fixture data, serializes the
/// output section, and diffs against the golden file.
#[test]
fn test_storage_inspector_vs_golden() {
    let exec = storage_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = StorageInspector::new()
        .inspect(&ctx)
        .expect("storage inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Storage(s) => s,
        other => panic!("expected SectionData::Storage, got {:?}", other),
    };

    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let golden = include_str!("../../testdata/golden/go-v13-storage-section.json");
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Storage inspector output diverges from golden (undocumented):\n{}",
        format_diffs(&undocumented)
    );
}

// ── Kernelboot inspector vs golden ──────────────────────────────────

/// Parity gate: runs KernelbootInspector on fixture data, serializes the
/// output section, and diffs against the golden file.
#[test]
fn test_kernelboot_inspector_vs_golden() {
    let exec = kernelboot_mock();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: None,
    };

    let output = KernelbootInspector::new()
        .inspect(&ctx)
        .expect("kernelboot inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::KernelBoot(s) => s,
        other => panic!("expected SectionData::KernelBoot, got {:?}", other),
    };

    let rust_json = serde_json::to_string_pretty(section).unwrap();
    let golden = include_str!("../../testdata/golden/go-v13-kernelboot-section.json");
    let undocumented = diff_snapshots(golden, &rust_json, &allowlist()).unwrap();

    assert!(
        undocumented.is_empty(),
        "Kernelboot inspector output diverges from golden (undocumented):\n{}",
        format_diffs(&undocumented)
    );
}
