//! Integration tests for SelinuxInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::selinux::{
    SelinuxInspector, parse_file_contexts_local_line, parse_file_contexts_subs_line,
    parse_semanage_line,
};
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::FindingKind;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::selinux::{
    FcontextFileType, FcontextRule, FcontextSource, SelinuxContext, SelinuxSection,
};
use inspectah_core::types::system::SourceSystem;
use std::collections::HashSet;
use std::path::PathBuf;

// ── Fixtures ────────────────────────────────────────────────────────

const GETENFORCE_ENFORCING: &str =
    include_str!("../../../testdata/fixtures/selinux/getenforce-enforcing.txt");
const GETENFORCE_DISABLED: &str =
    include_str!("../../../testdata/fixtures/selinux/getenforce-disabled.txt");
const SELINUX_CONFIG: &str = include_str!("../../../testdata/fixtures/selinux/selinux-config.txt");
const SEMANAGE_BOOLEAN: &str =
    include_str!("../../../testdata/fixtures/selinux/semanage-boolean-output.txt");
const SEMANAGE_FCONTEXT: &str =
    include_str!("../../../testdata/fixtures/selinux/semanage-fcontext-output.txt");
const SEMANAGE_PORT: &str =
    include_str!("../../../testdata/fixtures/selinux/semanage-port-output.txt");
const AUDIT_RULES: &str =
    include_str!("../../../testdata/fixtures/selinux/audit-rules-custom.rules");
const FIPS_ENABLED: &str = include_str!("../../../testdata/fixtures/selinux/fips-enabled.txt");
const PAM_CUSTOM_SSHD: &str =
    include_str!("../../../testdata/fixtures/selinux/pam-d-custom-sshd.conf");

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
        baseline_data: None,
    };

    let output = SelinuxInspector::new()
        .inspect(&ctx, &NullProgress)
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
        .any(|r| r.path.contains("custom.rules"));
    assert!(has_custom_rules, "should include custom.rules");
    let has_default_rules = section
        .audit_rules
        .iter()
        .any(|r| r.path.contains("audit.rules") && !r.path.contains("custom"));
    assert!(
        !has_default_rules,
        "should NOT include RPM-owned audit.rules"
    );
    // Content must be persisted
    let custom = section
        .audit_rules
        .iter()
        .find(|r| r.path.contains("custom.rules"))
        .expect("custom.rules must exist");
    assert!(
        !custom.content.is_empty(),
        "audit rule file content must be persisted"
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
        .any(|p| p.path.contains("custom-sshd"));
    assert!(has_custom_pam, "should include custom-sshd PAM config");
    // Content must be persisted
    let custom_pam = section
        .pam_configs
        .iter()
        .find(|p| p.path.contains("custom-sshd"))
        .expect("custom-sshd must exist");
    assert!(
        !custom_pam.content.is_empty(),
        "PAM config file content must be persisted"
    );
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
        baseline_data: None,
    };

    // When semanage fails, it falls back to sysfs booleans.
    // If neither works, Degraded is returned.
    let result = SelinuxInspector::new().inspect(&ctx, &NullProgress);

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
        baseline_data: None,
    };

    let result = SelinuxInspector::new().inspect(&ctx, &NullProgress);

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
        baseline_data: None,
    };

    let output = SelinuxInspector::new()
        .inspect(&ctx, &NullProgress)
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

// ── Fcontext rule parsing and ordered collection (Task 10) ───────────

/// Builds a `Label` rule with `FindingKind::included()` disposition, the
/// collector's default for every rule it emits.
fn label(
    spec: &str,
    file_type: FcontextFileType,
    user: &str,
    role: &str,
    type_: &str,
    level: Option<&str>,
) -> FcontextRule {
    FcontextRule::Label {
        spec: spec.to_string(),
        file_type,
        context: SelinuxContext {
            user: user.to_string(),
            role: role.to_string(),
            type_: type_.to_string(),
            level: level.map(str::to_string),
        },
        disposition: FindingKind::included(),
        aggregate: None,
    }
}

#[test]
fn fcontext_parser_table() {
    let cases: &[(&str, FcontextRule)] = &[
        // the host's padded Label row
        (
            "/srv/www(/.*)?                                     all files          system_u:object_r:httpd_sys_content_t:s0",
            label(
                "/srv/www(/.*)?",
                FcontextFileType::All,
                "system_u",
                "object_r",
                "httpd_sys_content_t",
                Some("s0"),
            ),
        ),
        // <<None>> -> Unlabel; "regular file" -> RegularFile
        (
            "/srv/private(/.*)?                                 regular file       <<None>>",
            FcontextRule::Unlabel {
                spec: "/srv/private(/.*)?".into(),
                file_type: FcontextFileType::RegularFile,
                disposition: FindingKind::included(),
                aggregate: None,
            },
        ),
        // a user_u:object_r:foo_t:s0-s0:c0 context with the level kept intact
        (
            "/srv/foo(/.*)?                                     all files          user_u:object_r:foo_t:s0-s0:c0",
            label(
                "/srv/foo(/.*)?",
                FcontextFileType::All,
                "user_u",
                "object_r",
                "foo_t",
                Some("s0-s0:c0"),
            ),
        ),
        // "/home1 = /home" -> Equivalence { subject: "/home1", labeled_like: "/home" }
        (
            "/home1 = /home",
            FcontextRule::Equivalence {
                subject: "/home1".into(),
                labeled_like: "/home".into(),
                disposition: FindingKind::included(),
                aggregate: None,
            },
        ),
        // a stray line -> Unparsed
        (
            "this is not a valid fcontext line",
            FcontextRule::Unparsed {
                raw: "this is not a valid fcontext line".into(),
            },
        ),
    ];
    for (line, want) in cases {
        assert_eq!(parse_semanage_line(line), Some(want.clone()), "{line}");
    }
}

#[test]
fn policy_store_forms_and_order() {
    // "-d" flag -> Directory
    assert_eq!(
        parse_file_contexts_local_line(
            "/srv/www(/.*)?  -d  system_u:object_r:httpd_sys_content_t:s0"
        ),
        Some(label(
            "/srv/www(/.*)?",
            FcontextFileType::Directory,
            "system_u",
            "object_r",
            "httpd_sys_content_t",
            Some("s0"),
        )),
    );

    // "--" -> RegularFile
    assert_eq!(
        parse_file_contexts_local_line(
            "/srv/www(/.*)?  --  system_u:object_r:httpd_sys_content_t:s0"
        ),
        Some(label(
            "/srv/www(/.*)?",
            FcontextFileType::RegularFile,
            "system_u",
            "object_r",
            "httpd_sys_content_t",
            Some("s0"),
        )),
    );

    // a file_contexts.subs line -> Equivalence
    assert_eq!(
        parse_file_contexts_subs_line("/home1 /home"),
        Some(FcontextRule::Equivalence {
            subject: "/home1".into(),
            labeled_like: "/home".into(),
            disposition: FindingKind::included(),
            aggregate: None,
        }),
    );

    // A `#`-comment line is skipped (None), in both store files.
    assert_eq!(
        parse_file_contexts_local_line("#/srv/x  -d  system_u:object_r:foo_t:s0"),
        None,
    );
    assert_eq!(parse_file_contexts_subs_line("# myalias"), None);

    // Anything else this shape cannot place becomes Unparsed, not a
    // silent drop -- the preferred source must not lose data the `-l -C`
    // fallback would have kept as Unparsed.
    assert_eq!(
        parse_file_contexts_local_line("a b c d"),
        Some(FcontextRule::Unparsed {
            raw: "a b c d".into(),
        }),
        "too many tokens",
    );
    assert_eq!(
        parse_file_contexts_local_line("/srv/x -z system_u:object_r:foo_t:s0"),
        Some(FcontextRule::Unparsed {
            raw: "/srv/x -z system_u:object_r:foo_t:s0".into(),
        }),
        "unrecognized flag token",
    );
    assert_eq!(
        parse_file_contexts_local_line("/srv/x -d a:b"),
        Some(FcontextRule::Unparsed {
            raw: "/srv/x -d a:b".into(),
        }),
        "context missing the type field",
    );
    assert_eq!(
        parse_file_contexts_subs_line("/home1"),
        Some(FcontextRule::Unparsed {
            raw: "/home1".into(),
        }),
        "missing labeled_like token",
    );
    assert_eq!(
        parse_file_contexts_subs_line("/home1 /home extra"),
        Some(FcontextRule::Unparsed {
            raw: "/home1 /home extra".into(),
        }),
        "extra token",
    );

    // With file_contexts.local readable the collector runs no `semanage`
    // and sets PolicyStore, and two rules written in the order
    // "/srv/www(/.*)?", "/srv/.*" come out in that order.
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
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.local",
            "/srv/www(/.*)?  system_u:object_r:httpd_sys_content_t:s0\n/srv/.*  system_u:object_r:var_t:s0\n",
        )
        .with_dir("/etc/pam.d", vec![])
        .with_dir("/sys/fs/selinux/booleans", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output = SelinuxInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed with policy store files present");
    let section = match &output.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected Selinux, got {:?}", other),
    };

    assert_eq!(section.fcontext_source, FcontextSource::PolicyStore);
    let specs: Vec<&str> = section
        .fcontext_rules
        .iter()
        .map(|r| match r {
            FcontextRule::Label { spec, .. } => spec.as_str(),
            other => panic!("expected Label, got {:?}", other),
        })
        .collect();
    assert_eq!(
        specs,
        vec!["/srv/www(/.*)?", "/srv/.*"],
        "collected order must match file order, not sorted order"
    );
    assert!(
        !exec
            .command_log()
            .iter()
            .any(|c| c.contains("semanage fcontext")),
        "policy store files present -- collector must not shell out to semanage"
    );

    // With neither store file readable it runs `semanage fcontext -l -C`
    // and sets SemanageList.
    let exec2 = MockExecutor::new()
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_ENFORCING.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        .with_command(
            "chroot / semanage fcontext -l -C",
            ExecResult {
                stdout: SEMANAGE_FCONTEXT.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/etc/pam.d", vec![])
        .with_dir("/sys/fs/selinux/booleans", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");
    let ctx2 = InspectionContext {
        source_system: &source,
        executor: &exec2,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output2 = SelinuxInspector::new()
        .inspect(&ctx2, &NullProgress)
        .expect("inspector should succeed via semanage fallback");
    let section2 = match &output2.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected Selinux, got {:?}", other),
    };
    assert_eq!(section2.fcontext_source, FcontextSource::SemanageList);
    assert!(
        exec2
            .command_log()
            .iter()
            .any(|c| c == "chroot / semanage fcontext -l -C"),
        "neither store file readable -- collector must fall back to semanage"
    );

    // With both file_contexts.local and file_contexts.subs readable, local
    // rules come first and subs rules after -- order is the point of this
    // task and spans both files, not just one (round 1 review, item 4).
    let exec3 = MockExecutor::new()
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_ENFORCING.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.local",
            "/srv/www(/.*)?  system_u:object_r:httpd_sys_content_t:s0\n",
        )
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.subs",
            "/home1 /home\n",
        )
        .with_dir("/etc/pam.d", vec![])
        .with_dir("/sys/fs/selinux/booleans", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");
    let ctx3 = InspectionContext {
        source_system: &source,
        executor: &exec3,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output3 = SelinuxInspector::new()
        .inspect(&ctx3, &NullProgress)
        .expect("inspector should succeed with both store files present");
    let section3 = match &output3.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected Selinux, got {:?}", other),
    };
    assert_eq!(section3.fcontext_source, FcontextSource::PolicyStore);
    assert!(
        matches!(
            section3.fcontext_rules.as_slice(),
            [FcontextRule::Label { .. }, FcontextRule::Equivalence { .. }]
        ),
        "local rules must precede subs rules, got {:?}",
        section3.fcontext_rules
    );
    assert!(
        !exec3
            .command_log()
            .iter()
            .any(|c| c.contains("semanage fcontext")),
        "both store files present -- collector must not shell out to semanage"
    );

    // With only file_contexts.subs readable (local unreadable), the
    // collector still takes PolicyStore and does not fall back to
    // semanage. file_contexts.subs_dist is registered with a different
    // equivalence to prove it is never read -- if the collector read it
    // instead of (or in addition to) subs, this assertion would see a
    // second rule or the wrong one.
    let exec4 = MockExecutor::new()
        .with_command(
            "getenforce",
            ExecResult {
                stdout: GETENFORCE_ENFORCING.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/selinux/config", SELINUX_CONFIG)
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.subs",
            "/home1 /home\n",
        )
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.subs_dist",
            "/distro1 /distro\n",
        )
        .with_dir("/etc/pam.d", vec![])
        .with_dir("/sys/fs/selinux/booleans", vec![])
        .with_file("/proc/sys/crypto/fips_enabled", "0\n");
    let ctx4 = InspectionContext {
        source_system: &source,
        executor: &exec4,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };
    let output4 = SelinuxInspector::new()
        .inspect(&ctx4, &NullProgress)
        .expect("inspector should succeed with only file_contexts.subs present");
    let section4 = match &output4.section {
        SectionData::Selinux(s) => s,
        other => panic!("expected Selinux, got {:?}", other),
    };
    assert_eq!(section4.fcontext_source, FcontextSource::PolicyStore);
    assert_eq!(
        section4.fcontext_rules,
        vec![FcontextRule::Equivalence {
            subject: "/home1".into(),
            labeled_like: "/home".into(),
            disposition: FindingKind::included(),
            aggregate: None,
        }],
        "subs_dist must never be read, only subs"
    );
    assert!(
        !exec4
            .command_log()
            .iter()
            .any(|c| c.contains("semanage fcontext")),
        "subs alone readable -- collector must not shell out to semanage"
    );
}
