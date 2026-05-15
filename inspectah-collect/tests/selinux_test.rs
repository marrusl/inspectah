//! Integration tests for SelinuxInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::selinux::SelinuxInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::selinux::SelinuxSection;
use inspectah_core::types::system::SourceSystem;
use std::collections::HashSet;
use std::path::PathBuf;

// ── Fixtures ────────────────────────────────────────────────────────

const GETENFORCE_ENFORCING: &str =
    include_str!("../../testdata/fixtures/selinux/getenforce-enforcing.txt");
const GETENFORCE_DISABLED: &str =
    include_str!("../../testdata/fixtures/selinux/getenforce-disabled.txt");
const SELINUX_CONFIG: &str = include_str!("../../testdata/fixtures/selinux/selinux-config.txt");
const SEMANAGE_BOOLEAN: &str =
    include_str!("../../testdata/fixtures/selinux/semanage-boolean-output.txt");
const SEMANAGE_FCONTEXT: &str =
    include_str!("../../testdata/fixtures/selinux/semanage-fcontext-output.txt");
const SEMANAGE_PORT: &str =
    include_str!("../../testdata/fixtures/selinux/semanage-port-output.txt");
const AUDIT_RULES: &str = include_str!("../../testdata/fixtures/selinux/audit-rules-custom.rules");
const FIPS_ENABLED: &str = include_str!("../../testdata/fixtures/selinux/fips-enabled.txt");
const PAM_CUSTOM_SSHD: &str =
    include_str!("../../testdata/fixtures/selinux/pam-d-custom-sshd.conf");

// ── Helpers ─────────────────────────────────────────────────────────

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

fn mock_rpm_state() -> RpmState {
    let mut owned = HashSet::new();
    // RPM-owned audit rules and PAM configs
    owned.insert(PathBuf::from("/etc/audit/rules.d/audit.rules"));
    owned.insert(PathBuf::from("/etc/pam.d/system-auth"));
    owned.insert(PathBuf::from("/etc/pam.d/password-auth"));
    RpmState {
        owned_paths: owned,
        ..Default::default()
    }
}

fn full_mock() -> MockExecutor {
    MockExecutor::new()
        // getenforce command
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_ENFORCING.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // /etc/selinux/config for policy type
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        // Custom modules at priority 400
        .with_dir(
            "/etc/selinux/targeted/active/modules/400",
            vec!["myapp", "custom_ports"],
        )
        // semanage boolean -l via chroot
        .with_command(
            "chroot / semanage boolean -l",
            ExecResult {
                stdout: SEMANAGE_BOOLEAN.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // semanage fcontext -l -C via chroot
        .with_command(
            "chroot / semanage fcontext -l -C",
            ExecResult {
                stdout: SEMANAGE_FCONTEXT.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // semanage port -l -C via chroot
        .with_command(
            "chroot / semanage port -l -C",
            ExecResult {
                stdout: SEMANAGE_PORT.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Audit rules directory
        .with_dir("/etc/audit/rules.d", vec!["audit.rules", "custom.rules"])
        .with_file("/etc/audit/rules.d/audit.rules", "# default audit rules\n")
        .with_file("/etc/audit/rules.d/custom.rules", AUDIT_RULES)
        // FIPS mode
        .with_file("/proc/sys/crypto/fips_enabled", FIPS_ENABLED)
        // PAM configs
        .with_dir(
            "/etc/pam.d",
            vec!["system-auth", "password-auth", "custom-sshd"],
        )
        .with_file("/etc/pam.d/system-auth", "# system-auth\n")
        .with_file("/etc/pam.d/password-auth", "# password-auth\n")
        .with_file("/etc/pam.d/custom-sshd", PAM_CUSTOM_SSHD)
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: full data collection.
#[test]
fn test_selinux_inspector_happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let output = SelinuxInspector::new()
        .inspect(&ctx)
        .expect("selinux inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected SectionData::Selinux, got {:?}", other),
    };

    // Mode detection
    assert_eq!(
        section.mode, "Enforcing",
        "mode should be Enforcing from getenforce"
    );

    // Custom modules
    assert!(
        !section.custom_modules.is_empty(),
        "should find custom modules"
    );
    assert!(
        section.custom_modules.contains(&"myapp".to_string()),
        "should find myapp module"
    );

    // Boolean overrides (only non-default booleans)
    assert!(
        !section.boolean_overrides.is_empty(),
        "should have boolean overrides"
    );
    // httpd_can_network_connect is on but default off -- should appear
    let has_httpd_bool = section.boolean_overrides.iter().any(|b| {
        b.get("name")
            .and_then(|v| v.as_str())
            .map(|n| n == "httpd_can_network_connect")
            .unwrap_or(false)
    });
    assert!(
        has_httpd_bool,
        "httpd_can_network_connect override should be detected"
    );

    // Fcontext rules
    assert!(
        !section.fcontext_rules.is_empty(),
        "should have fcontext rules"
    );

    // Port labels
    assert!(!section.port_labels.is_empty(), "should have port labels");

    // Audit rules: custom.rules is NOT rpm-owned, should be collected
    // audit.rules IS rpm-owned, should be skipped
    assert!(
        !section.audit_rules.is_empty(),
        "should have custom audit rules"
    );
    let has_custom_rules = section
        .audit_rules
        .iter()
        .any(|r| r.contains("custom.rules"));
    assert!(has_custom_rules, "should include custom.rules");
    let has_default_rules = section
        .audit_rules
        .iter()
        .any(|r| r.contains("audit.rules") && !r.contains("custom"));
    assert!(
        !has_default_rules,
        "should NOT include RPM-owned audit.rules"
    );

    // FIPS mode
    assert!(section.fips_mode, "FIPS should be enabled from fixture");

    // PAM configs: custom-sshd is NOT rpm-owned, should be collected
    assert!(
        !section.pam_configs.is_empty(),
        "should have custom PAM configs"
    );
    let has_custom_pam = section
        .pam_configs
        .iter()
        .any(|p| p.contains("custom-sshd"));
    assert!(has_custom_pam, "should include custom-sshd PAM config");
}

/// SELinux disabled -- minimal output (mode is Disabled, fewer fields populated).
#[test]
fn test_selinux_inspector_disabled() {
    let exec = MockExecutor::new()
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_DISABLED.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        // No modules, no booleans via semanage, no fcontext, no ports
        .with_dir("/etc/pam.d", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    // When semanage fails, it falls back to sysfs booleans.
    // If neither works, Degraded is returned.
    let result = SelinuxInspector::new().inspect(&ctx);

    let section = match &result {
        Ok(output) => match &output.section {
            SectionData::Selinux(s) => s,
            other => panic!("expected Selinux, got {:?}", other),
        },
        Err(InspectorError::Degraded { partial, .. }) => match &partial.section {
            SectionData::Selinux(s) => s,
            other => panic!("expected Selinux in partial, got {:?}", other),
        },
        Err(other) => panic!("unexpected error: {other}"),
    };

    assert_eq!(section.mode, "Disabled", "mode should be Disabled");
    assert!(!section.fips_mode, "FIPS should be disabled");
}

/// semanage unavailable -- fallback behavior, may produce Degraded.
#[test]
fn test_selinux_inspector_degraded_no_semanage() {
    let exec = MockExecutor::new()
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_ENFORCING.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        // No semanage commands -- all will return 127
        // Sysfs booleans fallback
        .with_dir(
            "/sys/fs/selinux/booleans",
            vec![
                "httpd_can_network_connect",
                "virt_use_nfs",
                "container_manage_cgroup",
            ],
        )
        .with_file("/sys/fs/selinux/booleans/httpd_can_network_connect", "1 0")
        .with_file("/sys/fs/selinux/booleans/virt_use_nfs", "1 0")
        .with_file("/sys/fs/selinux/booleans/container_manage_cgroup", "0 1")
        .with_dir("/etc/pam.d", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", FIPS_ENABLED);

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let result = SelinuxInspector::new().inspect(&ctx);

    // Should succeed or degrade -- both are acceptable
    let section = match &result {
        Ok(output) => match &output.section {
            SectionData::Selinux(s) => s,
            other => panic!("expected Selinux, got {:?}", other),
        },
        Err(InspectorError::Degraded { partial, .. }) => match &partial.section {
            SectionData::Selinux(s) => s,
            other => panic!("expected Selinux in partial, got {:?}", other),
        },
        Err(other) => panic!("unexpected error: {other}"),
    };

    assert_eq!(section.mode, "Enforcing");

    // Should still get boolean overrides from sysfs fallback
    // httpd_can_network_connect: current=1, pending=0 -> non-default
    assert!(
        !section.boolean_overrides.is_empty(),
        "sysfs fallback should produce boolean overrides"
    );
}

/// Output serializes and deserializes cleanly.
#[test]
fn test_selinux_inspector_json_roundtrip() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
    };

    let output = SelinuxInspector::new()
        .inspect(&ctx)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected SectionData::Selinux, got {:?}", other),
    };

    let json = serde_json::to_string_pretty(section).expect("section must serialize to JSON");
    let roundtrip: SelinuxSection =
        serde_json::from_str(&json).expect("JSON must deserialize back");
    let roundtrip_json =
        serde_json::to_string_pretty(&roundtrip).expect("roundtrip must serialize");

    assert_eq!(
        json, roundtrip_json,
        "inspector output must round-trip faithfully through SelinuxSection"
    );
}
