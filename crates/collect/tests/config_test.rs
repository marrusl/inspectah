//! Integration tests for ConfigInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::config::{ConfigCategory, ConfigFileKind, ConfigSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::rpm::RpmVaEntry;
use inspectah_core::types::system::SourceSystem;
use std::collections::HashSet;
use std::path::PathBuf;

// ── Fixtures ────────────────────────────────────────────────────────

const HTTPD_CONF: &str = include_str!("../../../testdata/fixtures/config/httpd-conf.txt");
const SSHD_CONFIG: &str = include_str!("../../../testdata/fixtures/config/sshd-config.txt");
const LOGROTATE_CUSTOM: &str =
    include_str!("../../../testdata/fixtures/config/logrotate-custom.conf");
const CRYPTO_POLICIES: &str = include_str!("../../../testdata/fixtures/config/crypto-policies.txt");

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

/// Build RpmState with owned_paths and verification_results from fixtures.
fn mock_rpm_state() -> RpmState {
    let mut owned = HashSet::new();
    // RPM-owned paths from rpm-qa-file-ownership fixture
    for path in &[
        "/etc/ssh/sshd_config",
        "/etc/ssh/ssh_host_rsa_key",
        "/etc/pam.d/sshd",
        "/etc/httpd/conf/httpd.conf",
        "/etc/httpd/conf.d/autoindex.conf",
        "/etc/httpd/conf.d/welcome.conf",
        "/etc/httpd/conf.d/ssl.conf",
        "/etc/logrotate.d/httpd",
        "/etc/chrony.conf",
        "/etc/chrony.keys",
        "/etc/pam.d/system-auth",
        "/etc/pam.d/password-auth",
        "/etc/cron.d/0hourly",
        "/etc/crontab",
        "/etc/crypto-policies/config",
        "/etc/crypto-policies/back-ends",
        "/etc/passwd",
        "/etc/group",
        "/etc/shadow",
        "/etc/ssh/ssh_config",
        "/etc/ssh/moduli",
    ] {
        owned.insert(PathBuf::from(path));
    }

    // Verification results from rpm-va-output fixture
    let verification_results = vec![
        RpmVaEntry {
            path: "/etc/httpd/conf/httpd.conf".into(),
            flags: "S.5....T.".into(),
            package: Some("httpd".into()),
        },
        RpmVaEntry {
            path: "/etc/ssh/sshd_config".into(),
            flags: "..5....T.".into(),
            package: Some("openssh-server".into()),
        },
        RpmVaEntry {
            path: "/etc/pam.d/sshd".into(),
            flags: ".......T.".into(),
            package: Some("pam".into()),
        },
        RpmVaEntry {
            path: "/etc/logrotate.d/custom-app".into(),
            flags: "S.5....T.".into(),
            package: None,
        },
        RpmVaEntry {
            path: "/etc/chrony.conf".into(),
            flags: "..?......".into(),
            package: Some("chrony".into()),
        },
        RpmVaEntry {
            path: "/etc/sysconfig/network-scripts/ifcfg-eth0".into(),
            flags: "SM5....T.".into(),
            package: None,
        },
    ];

    RpmState {
        owned_paths: owned,
        verification_results,
        ..Default::default()
    }
}

fn full_mock() -> MockExecutor {
    MockExecutor::new()
        // /etc directory tree -- walk_etc_recursive uses read_dir recursively
        .with_dir(
            "/etc",
            vec![
                "ssh",
                "httpd",
                "pam.d",
                "chrony.conf",
                "crontab",
                "cron.d",
                "logrotate.d",
                "crypto-policies",
                "myapp",
                "custom-service.conf",
                "sysconfig",
            ],
        )
        .with_dir("/etc/ssh", vec!["sshd_config", "ssh_config", "moduli"])
        .with_dir("/etc/httpd", vec!["conf", "conf.d"])
        .with_dir("/etc/httpd/conf", vec!["httpd.conf"])
        .with_dir(
            "/etc/httpd/conf.d",
            vec!["autoindex.conf", "ssl.conf", "welcome.conf"],
        )
        .with_dir("/etc/pam.d", vec!["sshd", "system-auth", "password-auth"])
        .with_dir("/etc/cron.d", vec!["0hourly"])
        .with_dir("/etc/logrotate.d", vec!["httpd", "custom-app"])
        .with_dir("/etc/crypto-policies", vec!["config", "state"])
        .with_dir("/etc/crypto-policies/state", vec!["current"])
        .with_dir("/etc/myapp", vec!["app.conf", "database.yml"])
        .with_dir("/etc/sysconfig", vec!["network-scripts"])
        .with_dir("/etc/sysconfig/network-scripts", vec!["ifcfg-eth0"])
        // File contents for RPM-modified files
        .with_file("/etc/httpd/conf/httpd.conf", HTTPD_CONF)
        .with_file("/etc/ssh/sshd_config", SSHD_CONFIG)
        .with_file("/etc/pam.d/sshd", "auth required pam_sepermit.so\n")
        .with_file("/etc/logrotate.d/custom-app", LOGROTATE_CUSTOM)
        .with_file("/etc/chrony.conf", "server time.example.com iburst\n")
        .with_file(
            "/etc/sysconfig/network-scripts/ifcfg-eth0",
            "DEVICE=eth0\nBOOTPROTO=static\n",
        )
        // Unowned files (not in owned_paths)
        .with_file("/etc/myapp/app.conf", "port=8080\nlog_level=info\n")
        .with_file("/etc/myapp/database.yml", "host: db.example.com\n")
        .with_file("/etc/custom-service.conf", "listen=0.0.0.0:9090\n")
        // RPM-owned unmodified files (exist in owned_paths but not in rpm -Va)
        .with_file("/etc/ssh/ssh_config", "Host *\n")
        .with_file("/etc/ssh/moduli", "# moduli\n")
        .with_file("/etc/httpd/conf.d/autoindex.conf", "# autoindex\n")
        .with_file("/etc/httpd/conf.d/ssl.conf", "# ssl\n")
        .with_file("/etc/httpd/conf.d/welcome.conf", "# welcome\n")
        .with_file("/etc/logrotate.d/httpd", "# httpd logrotate\n")
        .with_file("/etc/pam.d/system-auth", "# system-auth\n")
        .with_file("/etc/pam.d/password-auth", "# password-auth\n")
        .with_file("/etc/cron.d/0hourly", "# 0hourly\n")
        .with_file("/etc/crontab", "# crontab\n")
        .with_file("/etc/crypto-policies/config", "DEFAULT\n")
        .with_file("/etc/crypto-policies/state/current", CRYPTO_POLICIES)
        // NetworkManager connections for DHCP exclusion path
        .with_dir("/etc/NetworkManager", vec!["system-connections"])
        .with_dir("/etc/NetworkManager/system-connections", vec![])
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: three-category classification works.
#[test]
fn test_config_inspector_happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("config inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    assert!(
        !section.files.is_empty(),
        "inspector must produce config files"
    );

    // Check RPM-owned modified files exist
    let rpm_modified: Vec<_> = section
        .files
        .iter()
        .filter(|f| f.kind == ConfigFileKind::RpmOwnedModified)
        .collect();
    assert!(
        !rpm_modified.is_empty(),
        "should have RPM-owned modified files"
    );

    // httpd.conf should be RPM-owned modified
    let httpd = section
        .files
        .iter()
        .find(|f| f.path.contains("httpd.conf") && f.kind == ConfigFileKind::RpmOwnedModified);
    assert!(httpd.is_some(), "httpd.conf should be RPM-owned modified");
    let httpd = httpd.unwrap();
    assert_eq!(httpd.package.as_deref(), Some("httpd"));
    assert!(
        !httpd.content.is_empty(),
        "modified file should have content"
    );

    // Check unowned files exist
    let unowned: Vec<_> = section
        .files
        .iter()
        .filter(|f| f.kind == ConfigFileKind::Unowned)
        .collect();
    assert!(!unowned.is_empty(), "should have unowned files");

    // myapp/app.conf should be unowned
    let myapp = section
        .files
        .iter()
        .find(|f| f.path.contains("myapp/app.conf"));
    assert!(myapp.is_some(), "myapp/app.conf should be present");
    assert_eq!(
        myapp.unwrap().kind,
        ConfigFileKind::Unowned,
        "myapp/app.conf should be unowned"
    );

    // Files should be sorted by path
    let paths: Vec<&str> = section.files.iter().map(|f| f.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted, "files should be sorted by path");
}

/// `file_contexts.local`/`file_contexts.subs` are derived SELinux
/// policy-store state, collected separately as typed `FcontextRule`s (task
/// 10, item 1.4). Even when `rpm -Va` reports one as RPM-owned-modified
/// (true on a host where policycoreutils owns and ships it), it must not
/// also enter `config.files` -- that would be the same fact represented
/// twice, once correctly typed and once as an opaque config blob.
#[test]
fn test_config_inspector_excludes_selinux_fcontext_store_file() {
    let exec = MockExecutor::new()
        // An empty but present /etc -- ConfigInspector early-exits on a
        // missing /etc, which would make this test pass vacuously
        // regardless of the fcontext-store skip under test.
        .with_dir("/etc", vec![])
        .with_file(
            "/etc/selinux/targeted/contexts/files/file_contexts.local",
            "/srv/www(/.*)?  system_u:object_r:httpd_sys_content_t:s0\n",
        )
        // Positive control (round 1 review, item 5): an ordinary modified
        // config with the same rpm -Va flags, so a future change that
        // empties the whole section fails on this assertion instead of
        // passing quietly on the absence check alone.
        .with_file("/etc/ssh/sshd_config", "Port 22\n");

    let rpm_state = RpmState {
        verification_results: vec![
            RpmVaEntry {
                path: "/etc/selinux/targeted/contexts/files/file_contexts.local".into(),
                flags: "S.5....T.".into(),
                package: Some("policycoreutils".into()),
            },
            RpmVaEntry {
                path: "/etc/ssh/sshd_config".into(),
                flags: "S.5....T.".into(),
                package: Some("openssh-server".into()),
            },
        ],
        ..Default::default()
    };

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    assert!(
        !section
            .files
            .iter()
            .any(|f| f.path.contains("file_contexts.local")),
        "file_contexts.local is derived policy-store state, not config"
    );
    assert!(
        section.files.iter().any(|f| f.path.contains("sshd_config")),
        "an ordinary RPM-modified config must still be collected"
    );
}

/// No /etc directory -- produces empty section.
#[test]
fn test_config_inspector_no_etc() {
    let exec = MockExecutor::new();
    // No /etc registered at all -- read_dir returns NotFound

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed with no /etc");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    assert!(section.files.is_empty(), "no /etc means no config files");
}

/// PermissionDenied during /etc walk produces Degraded output.
#[test]
fn test_config_inspector_degraded_permissions() {
    let exec = MockExecutor::new()
        // /etc exists but walk gets permission denied
        .with_dir("/etc", vec!["ssh"])
        .with_dir_error("/etc/ssh", std::io::ErrorKind::PermissionDenied);

    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let result = ConfigInspector::new().inspect(&ctx, &NullProgress);

    match result {
        Ok(_) => {
            // Inspector may still succeed with partial data
        }
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("permission denied") || reason.contains("degraded"),
                "degraded reason should mention permission: {reason}"
            );
            match &partial.section {
                SectionData::Config(_) => {}
                other => panic!("expected SectionData::Config in partial, got {:?}", other),
            }
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

/// Output serializes and deserializes cleanly.
#[test]
fn test_config_inspector_json_roundtrip() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = mock_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    let json = serde_json::to_string_pretty(section).expect("section must serialize to JSON");
    let roundtrip: ConfigSection = serde_json::from_str(&json).expect("JSON must deserialize back");
    let roundtrip_json =
        serde_json::to_string_pretty(&roundtrip).expect("roundtrip must serialize");

    assert_eq!(
        json, roundtrip_json,
        "inspector output must round-trip faithfully through ConfigSection"
    );
}

/// § 3.6 and § 8.11: the config walker owns `/etc/pam.d/`,
/// `/etc/audit/rules.d/` and the custom tuned profiles. Each file arrives
/// as an ordinary `ConfigFileEntry` carrying the category that names it,
/// which is what gives it a disposition.
///
/// The three tuned runtime-state files stay excluded (`walk.rs`'s
/// `UNOWNED_EXCLUDE_EXACT`) because `tuned_disposition` renders them;
/// `active_profile` is in the fixture so that a change deleting that
/// exclusion along with the profile glob fails here.
#[test]
fn pam_audit_and_tuned_are_config_entries() {
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["pam.d", "audit", "tuned"])
        .with_dir("/etc/pam.d", vec!["custom-sshd", "su-l"])
        .with_dir("/etc/audit", vec!["rules.d"])
        .with_dir("/etc/audit/rules.d", vec!["driftify.rules"])
        .with_dir("/etc/tuned", vec!["myapp", "active_profile"])
        .with_dir("/etc/tuned/myapp", vec!["tuned.conf"])
        .with_file("/etc/pam.d/custom-sshd", "auth required pam_faillock.so\n")
        .with_file("/etc/pam.d/su-l", "auth include su\n")
        .with_file(
            "/etc/audit/rules.d/driftify.rules",
            "-w /etc/shadow -p wa -k shadow_changes\n",
        )
        .with_file("/etc/tuned/myapp/tuned.conf", "[main]\nsummary=myapp\n")
        .with_file("/etc/tuned/active_profile", "myapp\n");

    let rpm_state = RpmState::default();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    let category_of = |path: &str| {
        section
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "{path} must be a config entry; got {:?}",
                    section.files.iter().map(|f| &f.path).collect::<Vec<_>>()
                )
            })
            .category
            .clone()
    };

    assert_eq!(
        category_of("/etc/pam.d/custom-sshd"),
        ConfigCategory::Pam,
        "a custom PAM config is categorized Pam"
    );
    assert_eq!(
        category_of("/etc/audit/rules.d/driftify.rules"),
        ConfigCategory::Audit,
        "a custom audit rule file is categorized Audit"
    );
    assert_eq!(
        category_of("/etc/tuned/myapp/tuned.conf"),
        ConfigCategory::Tuned,
        "a custom tuned profile is categorized Tuned"
    );

    assert!(
        !section
            .files
            .iter()
            .any(|f| f.path == "/etc/tuned/active_profile"),
        "tuned runtime state stays excluded: tuned_disposition renders it"
    );
    assert!(
        !section.files.iter().any(|f| f.path == "/etc/pam.d/su-l"),
        "the eight base PAM configs stay excluded by exact match"
    );
}

/// § 3.6's authselect rule. `rpm -Va` reports `....L....` on the five
/// `/etc/pam.d/` files authselect owns: RPM-owned symlinks whose target
/// changed. The customization lives in `/etc/authselect/`, which the walker
/// collects on its own, so the symlink must produce no entry of its own.
///
/// The custom file in the same fixture is the sibling control: it differs
/// unaided, so an empty section cannot pass this test.
#[test]
fn an_authselect_symlink_is_not_a_local_pam_file() {
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["pam.d", "authselect"])
        .with_dir("/etc/pam.d", vec!["system-auth", "custom-sshd"])
        .with_dir("/etc/authselect", vec!["system-auth"])
        .with_link("/etc/pam.d/system-auth", "/etc/authselect/system-auth")
        .with_file("/etc/pam.d/system-auth", "# authselect-generated\n")
        .with_file("/etc/authselect/system-auth", "# authselect-generated\n")
        .with_file("/etc/pam.d/custom-sshd", "auth required pam_faillock.so\n");

    let rpm_state = RpmState {
        verification_results: vec![RpmVaEntry {
            path: "/etc/pam.d/system-auth".into(),
            flags: "....L....".into(),
            package: Some("authselect-libs".into()),
        }],
        ..Default::default()
    };

    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {:?}", other),
    };

    assert!(
        !section
            .files
            .iter()
            .any(|f| f.path == "/etc/pam.d/system-auth"),
        "an authselect symlink is not a local PAM file; got {:?}",
        section.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    assert!(
        section
            .files
            .iter()
            .any(|f| f.path == "/etc/pam.d/custom-sshd"),
        "the sibling custom PAM file must still be collected"
    );
    assert!(
        section
            .files
            .iter()
            .any(|f| f.path == "/etc/authselect/system-auth"),
        "the customization itself is collected from /etc/authselect/"
    );
}
