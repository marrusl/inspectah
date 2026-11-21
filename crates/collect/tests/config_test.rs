//! Integration tests for ConfigInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::modules::parse_rpm_va;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::config::{ConfigCategory, ConfigFileKind, ConfigSection};
use inspectah_core::types::finding::{AdvisoryType, FindingKind};
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

/// The `rpm -qa` query the config inspector lists `%ghost` files with:
/// package, `fflags`, path.
const GHOST_QUERY: &str = "rpm -qa --queryformat [%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES}\\n]";

/// `mock` answering [`GHOST_QUERY`] with `stdout`.
fn with_ghosts(mock: MockExecutor, stdout: &str) -> MockExecutor {
    mock.with_command(
        GHOST_QUERY,
        inspectah_core::traits::executor::ExecResult {
            stdout: stdout.into(),
            exit_code: 0,
            ..Default::default()
        },
    )
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
    with_ghosts(MockExecutor::new(), "")
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

/// A config file of an installed RPM whose content the operator changed is
/// included at collect, the same as the package that owns it. The scan
/// renders straight from collect dispositions, so this is what puts the
/// file in the scan's `config/` tree.
///
/// The verification results come from raw `rpm -Va` text through the
/// production parser, not from hand-built entries. Package attribution is
/// left out: the pipeline adds it later from file ownership, and the
/// disposition does not depend on it.
#[test]
fn a_content_changed_rpm_config_is_included_at_collect() {
    const RPM_VA: &str = "S.5....T.  c /etc/ssh/sshd_config\n\
                          ..5....T.  c /etc/httpd/conf/httpd.conf\n";
    let exec = MockExecutor::new()
        .with_dir("/etc", vec![])
        .with_file("/etc/ssh/sshd_config", "Port 2222\n")
        .with_file("/etc/httpd/conf/httpd.conf", HTTPD_CONF);
    let rpm_state = RpmState {
        verification_results: parse_rpm_va(RPM_VA),
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
    let SectionData::Config(section) = &output.section else {
        panic!("expected SectionData::Config, got {:?}", output.section);
    };

    for path in ["/etc/httpd/conf/httpd.conf", "/etc/ssh/sshd_config"] {
        let entry = section
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is collected: {:?}", section.files));
        assert_eq!(entry.kind, ConfigFileKind::RpmOwnedModified, "{path}");
        assert_eq!(
            entry.disposition,
            FindingKind::included(),
            "{path}: a user-modified RPM config is included by default"
        );
    }
}

/// One byte over the inspector's 256 KiB content cap.
const OVERSIZED_LEN: usize = 256 * 1024 + 1;

/// A config whose content the collector could not capture is an advisory,
/// never an included entry with empty content: the export would copy that
/// empty string over the package's file. A file that really is empty is
/// captured content and stays included.
///
/// Covers both config kinds that read content: RPM-owned files from raw
/// `rpm -Va` text through the production parser, and unowned files from
/// the `/etc` walk.
#[test]
fn a_config_whose_content_was_not_captured_is_an_advisory() {
    const RPM_VA: &str = "S.5....T.  c /etc/httpd/conf.d/ssl.conf\n\
                          S.5....T.  c /etc/httpd/conf.d/welcome.conf\n\
                          S.5....T.  c /etc/httpd/conf.d/autoindex.conf\n";
    let oversized = "#".repeat(OVERSIZED_LEN);
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["vendor"])
        .with_dir("/etc/vendor", vec!["big.conf", "agent.db", "empty.conf"])
        // RPM-owned, content-modified
        .with_file("/etc/httpd/conf.d/ssl.conf", &oversized)
        .with_file("/etc/httpd/conf.d/welcome.conf", "unreadable\n")
        .with_file_error(
            "/etc/httpd/conf.d/welcome.conf",
            std::io::ErrorKind::PermissionDenied,
        )
        .with_file("/etc/httpd/conf.d/autoindex.conf", "")
        // Unowned
        .with_file("/etc/vendor/big.conf", &oversized)
        .with_file("/etc/vendor/agent.db", "")
        .with_file_error("/etc/vendor/agent.db", std::io::ErrorKind::InvalidData)
        .with_file("/etc/vendor/empty.conf", "");
    let rpm_state = RpmState {
        verification_results: parse_rpm_va(RPM_VA),
        ..Default::default()
    };
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let section = match ConfigInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { partial, reason }) => {
            for path in ["/etc/httpd/conf.d/ssl.conf", "/etc/vendor/big.conf"] {
                assert!(reason.contains(path), "{path} is named in: {reason}");
            }
            match partial.section {
                SectionData::Config(s) => s,
                other => panic!("expected SectionData::Config, got {other:?}"),
            }
        }
        other => panic!("an oversized config degrades the inspector, got {other:?}"),
    };
    let entry = |path: &str| {
        section
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is collected: {:?}", section.files))
    };

    for (path, kind, cause) in [
        (
            "/etc/httpd/conf.d/ssl.conf",
            ConfigFileKind::RpmOwnedModified,
            "too large",
        ),
        (
            "/etc/httpd/conf.d/welcome.conf",
            ConfigFileKind::RpmOwnedModified,
            "permission denied",
        ),
        ("/etc/vendor/big.conf", ConfigFileKind::Unowned, "too large"),
        (
            "/etc/vendor/agent.db",
            ConfigFileKind::Unowned,
            "could not be read",
        ),
    ] {
        let e = entry(path);
        assert_eq!(e.kind, kind, "{path}");
        match &e.disposition {
            FindingKind::Advisory {
                advisory_type: AdvisoryType::ContentNotCaptured,
                rationale,
            } => assert!(
                rationale.contains(cause),
                "{path}: the rationale says why: {rationale}"
            ),
            other => panic!("{path}: an uncaptured config is an advisory, got {other:?}"),
        }
        assert!(
            !e.disposition.is_included(),
            "{path} must never be materialized"
        );
    }

    for path in ["/etc/httpd/conf.d/autoindex.conf", "/etc/vendor/empty.conf"] {
        let e = entry(path);
        assert_eq!(e.content, "", "{path}");
        assert_eq!(
            e.disposition,
            FindingKind::included(),
            "{path}: a file that is really empty was captured and stays included"
        );
    }
}

/// A modernization pattern that matches a file whose content was not
/// captured adds its advice without replacing the capture failure. The
/// entry stays a `ContentNotCaptured` advisory, so it is never staged and
/// the fleet merge still treats it as no content, and its rationale gives
/// both reasons. A read error other than permission denied does not mark
/// the section degraded, so for this file the advisory is the only record
/// of the failure.
///
/// A captured file under the same pattern takes the modernization
/// advisory, as before.
#[test]
fn a_modernization_match_keeps_the_capture_failure() {
    const UNREADABLE: &str = "/etc/xinetd.d/tftp";
    const READABLE: &str = "/etc/xinetd.d/rsync";
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["xinetd.d"])
        .with_dir("/etc/xinetd.d", vec!["tftp", "rsync"])
        .with_file(UNREADABLE, "")
        .with_file_error(UNREADABLE, std::io::ErrorKind::InvalidData)
        .with_file(READABLE, "service rsync\n{\n}\n");
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
        .expect("a non-permission read error does not degrade the section");
    let section = match output.section {
        SectionData::Config(s) => s,
        other => panic!("expected SectionData::Config, got {other:?}"),
    };
    let entry = |path: &str| {
        section
            .files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path} is collected: {:?}", section.files))
    };

    let unreadable = entry(UNREADABLE);
    match &unreadable.disposition {
        FindingKind::Advisory {
            advisory_type: AdvisoryType::ContentNotCaptured,
            rationale,
        } => {
            assert!(
                rationale.contains("could not be read"),
                "the capture failure survives: {rationale}"
            );
            assert!(
                rationale.contains("xinetd is deprecated"),
                "the modernization advice is added: {rationale}"
            );
        }
        other => panic!("{UNREADABLE}: still a capture failure, got {other:?}"),
    }
    assert!(!unreadable.disposition.is_included());
    assert!(!unreadable.content_captured());

    match &entry(READABLE).disposition {
        FindingKind::Advisory {
            advisory_type: AdvisoryType::Modernization,
            rationale,
        } => assert!(rationale.contains("xinetd is deprecated"), "{rationale}"),
        other => panic!("{READABLE}: a modernization advisory, got {other:?}"),
    }
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
///
/// `/etc/audit/rules.d/audit.rules` is RPM-owned and unmodified, which is
/// the filter item 1.2 needs and the reason no `SnippetOrigin` exists. It
/// is in the fixture because the walker's uniform treatment of these three
/// trees is new: a tree-specific exclusion re-added inside the ownership
/// check would otherwise pass unnoticed.
#[test]
fn pam_audit_and_tuned_are_config_entries() {
    let exec = with_ghosts(MockExecutor::new(), "")
        .with_dir("/etc", vec!["pam.d", "audit", "tuned"])
        .with_dir("/etc/pam.d", vec!["custom-sshd", "su-l"])
        .with_dir("/etc/audit", vec!["rules.d"])
        .with_dir("/etc/audit/rules.d", vec!["audit.rules", "driftify.rules"])
        .with_dir("/etc/tuned", vec!["myapp", "active_profile"])
        .with_dir("/etc/tuned/myapp", vec!["tuned.conf"])
        .with_file("/etc/pam.d/custom-sshd", "auth required pam_faillock.so\n")
        .with_file("/etc/pam.d/su-l", "auth include su\n")
        .with_file("/etc/audit/rules.d/audit.rules", "-D\n-b 8192\n")
        .with_file(
            "/etc/audit/rules.d/driftify.rules",
            "-w /etc/shadow -p wa -k shadow_changes\n",
        )
        .with_file("/etc/tuned/myapp/tuned.conf", "[main]\nsummary=myapp\n")
        .with_file("/etc/tuned/active_profile", "myapp\n");

    let rpm_state = RpmState {
        owned_paths: HashSet::from([PathBuf::from("/etc/audit/rules.d/audit.rules")]),
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
    assert!(
        !section
            .files
            .iter()
            .any(|f| f.path == "/etc/audit/rules.d/audit.rules"),
        "an RPM-owned unmodified audit rule stays out: the walker's \
         ownership check is the filter, not a per-tree exclusion"
    );
}

/// § 3.6's authselect rule. `rpm -Va` reports `....L....` on the five
/// `/etc/pam.d/` files authselect owns: RPM-owned symlinks whose target
/// changed. The customization is the authselect profile, which the
/// `%ghost` branch collects and the export carries as an `authselect
/// select` line, so the symlink must produce no entry of its own.
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

/// The kernel/boot config directories are the walker's, same as every
/// other `/etc` tree: it reads them, categorizes them, and gives each
/// file one decision. The kernel/boot inspector collected them into
/// `KernelBootSection` lists before, which is how the same file came to
/// hold two rows and one decision.
///
/// `50-nouveau-blacklist.conf` is in the fixture because
/// `UNOWNED_EXCLUDE_GLOBS` used to carry `/etc/modprobe.d/*-blacklist.conf`
/// "for the distro files". The glob is only ever reached in the unowned
/// branch, after both ownership checks, so a distro file never saw it and
/// the only files it could drop were admin-authored. It is gone; this
/// case is the guard against it coming back.
#[test]
fn kernel_module_and_initramfs_directories_are_config_entries() {
    let exec = MockExecutor::new()
        .with_dir(
            "/etc",
            vec!["modprobe.d", "modules-load.d", "dracut.conf.d"],
        )
        .with_dir(
            "/etc/modprobe.d",
            vec!["myapp.conf", "50-nouveau-blacklist.conf"],
        )
        .with_dir("/etc/modules-load.d", vec!["br_netfilter.conf"])
        .with_dir("/etc/dracut.conf.d", vec!["50-custom.conf"])
        .with_file("/etc/modprobe.d/myapp.conf", "options myapp debug=1\n")
        .with_file(
            "/etc/modprobe.d/50-nouveau-blacklist.conf",
            "blacklist nouveau\n",
        )
        .with_file("/etc/modules-load.d/br_netfilter.conf", "br_netfilter\n")
        .with_file(
            "/etc/dracut.conf.d/50-custom.conf",
            "add_dracutmodules+=\" lvm \"\n",
        );

    let source = pkg_source();
    let rpm_state = RpmState::default();
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
        category_of("/etc/modprobe.d/myapp.conf"),
        ConfigCategory::KernelModule
    );
    assert_eq!(
        category_of("/etc/modules-load.d/br_netfilter.conf"),
        ConfigCategory::KernelModule
    );
    assert_eq!(
        category_of("/etc/dracut.conf.d/50-custom.conf"),
        ConfigCategory::Initramfs
    );
    assert_eq!(
        category_of("/etc/modprobe.d/50-nouveau-blacklist.conf"),
        ConfigCategory::KernelModule,
        "an unowned blacklist file is the admin's own: no package owns it, \
         so nothing else will put it in the image"
    );
}

/// § 5.1's RPM-owned mask rule. RPM verifies a non-regular file through
/// the readLink column only, so `rpm -Va` reports a package symlink an
/// admin re-pointed to `/dev/null` as `....L....` and nothing else. Such a
/// row is a modified config: it is carried as an empty file of the same
/// name, which masks the vendor file the way the link did.
///
/// Each row is its own host. The non-`/dev/null` link and the authselect
/// link are the controls that keep their old result, and the regular file
/// shows the content rule is untouched.
#[test]
fn an_rpm_owned_link_re_pointed_to_dev_null_is_a_modified_config() {
    struct Case {
        name: &'static str,
        rpm_va: &'static str,
        path: &'static str,
        links: &'static [(&'static str, &'static str)],
        /// What the executor reads at `path`; `MockExecutor::read_file`
        /// does not follow links, so a link row registers its target's
        /// content here.
        content: &'static str,
        /// `Some(content)` when the row yields an included
        /// `RpmOwnedModified` entry; `None` when it yields no entry.
        expected: Option<&'static str>,
    }
    const SYSCTL_LINK: &str = "/etc/sysctl.d/99-sysctl.conf";
    const SYSCTL_CONF: &str = "kernel.sysrq = 0\n";
    const DEV_NULL: &str = "/dev/null";

    let cases = [
        Case {
            name: "link to /dev/null",
            rpm_va: "....L....  c /etc/sysctl.d/99-sysctl.conf\n",
            path: SYSCTL_LINK,
            links: &[(SYSCTL_LINK, DEV_NULL)],
            content: "",
            expected: Some(""),
        },
        Case {
            name: "link to a real file",
            rpm_va: "....L....  c /etc/sysctl.d/99-sysctl.conf\n",
            path: SYSCTL_LINK,
            links: &[(SYSCTL_LINK, "/etc/sysctl.conf")],
            content: SYSCTL_CONF,
            expected: None,
        },
        Case {
            name: "two-hop link to /dev/null",
            rpm_va: "....L....  c /etc/sysctl.d/99-sysctl.conf\n",
            path: SYSCTL_LINK,
            links: &[
                (SYSCTL_LINK, "/etc/sysctl.d/50-hop.conf"),
                ("/etc/sysctl.d/50-hop.conf", DEV_NULL),
            ],
            content: "",
            expected: Some(""),
        },
        Case {
            name: "non-sysctl link to /dev/null",
            rpm_va: "....L....  c /etc/systemd/logind.conf.d/x.conf\n",
            path: "/etc/systemd/logind.conf.d/x.conf",
            links: &[("/etc/systemd/logind.conf.d/x.conf", DEV_NULL)],
            content: "",
            expected: Some(""),
        },
        Case {
            name: "authselect PAM link",
            rpm_va: "....L....  c /etc/pam.d/system-auth\n",
            path: "/etc/pam.d/system-auth",
            links: &[("/etc/pam.d/system-auth", "/etc/authselect/system-auth")],
            content: "# authselect-generated\n",
            expected: None,
        },
        Case {
            name: "content-modified regular file",
            rpm_va: "S.5....T.  c /etc/sysctl.conf\n",
            path: "/etc/sysctl.conf",
            links: &[],
            content: SYSCTL_CONF,
            expected: Some(SYSCTL_CONF),
        },
    ];

    for case in cases {
        let mut exec = MockExecutor::new()
            .with_dir("/etc", vec![])
            .with_file(case.path, case.content);
        for (link, target) in case.links {
            exec = exec.with_link(link, target);
        }
        let rpm_state = RpmState {
            verification_results: parse_rpm_va(case.rpm_va),
            ..Default::default()
        };
        let flags = rpm_state.verification_results[0].flags.clone();
        let source = pkg_source();
        let ctx = InspectionContext {
            source_system: &source,
            executor: &exec,
            rpm_state: Some(&rpm_state),
            baseline_data: None,
        };

        let output = ConfigInspector::new()
            .inspect(&ctx, &NullProgress)
            .unwrap_or_else(|e| panic!("{}: inspector should succeed: {e:?}", case.name));
        let section = match &output.section {
            SectionData::Config(s) => s,
            other => panic!("{}: expected SectionData::Config, got {other:?}", case.name),
        };
        let entries: Vec<_> = section
            .files
            .iter()
            .filter(|f| f.path == case.path)
            .collect();

        match case.expected {
            None => assert!(
                entries.is_empty(),
                "{}: no entry expected at {}, got {entries:?}",
                case.name,
                case.path
            ),
            Some(content) => {
                assert_eq!(
                    entries.len(),
                    1,
                    "{}: exactly one entry expected at {}, got {:?}",
                    case.name,
                    case.path,
                    section.files
                );
                let entry = entries[0];
                assert_eq!(
                    entry.kind,
                    ConfigFileKind::RpmOwnedModified,
                    "{}",
                    case.name
                );
                assert!(entry.disposition.is_included(), "{}", case.name);
                assert_eq!(entry.content, content, "{}", case.name);
                assert_eq!(
                    entry.rpm_va_flags.as_deref(),
                    Some(flags.as_str()),
                    "{}",
                    case.name
                );
            }
        }
    }
}

/// § 7.7: a mask has one representation, a state. A zero-length regular
/// file with a unit suffix directly under `/etc/systemd/system` is a mask,
/// so the config walk leaves it out as it leaves out a mask link. The
/// controls stay config entries: a non-empty unit file, a zero-length file
/// with no unit suffix, and a zero-length drop-in one level down.
#[test]
fn a_zero_length_unit_file_under_etc_systemd_system_is_no_config_entry() {
    const DIR: &str = "/etc/systemd/system";
    let exec = MockExecutor::new()
        .with_dir("/etc", vec!["systemd"])
        .with_dir("/etc/systemd", vec!["system"])
        .with_dir(
            DIR,
            vec![
                "masked.service",
                "masked.mount",
                "masked.device",
                "local.service",
                "notes.txt",
                "app.service.d",
            ],
        )
        .with_file("/etc/systemd/system/masked.service", "")
        .with_file("/etc/systemd/system/masked.mount", "")
        .with_file("/etc/systemd/system/masked.device", "")
        .with_file("/etc/systemd/system/local.service", "[Unit]\n")
        .with_file("/etc/systemd/system/notes.txt", "")
        .with_dir("/etc/systemd/system/app.service.d", vec!["empty.conf"])
        .with_file("/etc/systemd/system/app.service.d/empty.conf", "");
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
        .expect("the config inspector succeeds");
    let SectionData::Config(section) = output.section else {
        panic!("expected the config section");
    };
    let mut paths: Vec<&str> = section.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort_unstable();

    // Presence first: the controls.
    assert_eq!(
        paths,
        vec![
            "/etc/systemd/system/app.service.d/empty.conf",
            "/etc/systemd/system/local.service",
            "/etc/systemd/system/notes.txt",
        ]
    );
}

// ---------------------------------------------------------------------------
// The `%ghost` branch
// ---------------------------------------------------------------------------

/// Ghost-file query output for [`ghost_host`], as rpm prints it: package,
/// `fflags` (`cng` is config, noreplace, ghost; measured on RHEL 9.8 and
/// 10.2 for `sssd.conf` and `/etc/machine-id`), path.
const GHOST_ROWS: &str = "\
sssd-common cng /etc/sssd/sssd.conf
sssd-common  /usr/sbin/sssd
systemd cng /etc/machine-id
systemd cng /etc/hostname
systemd cng /etc/locale.conf
systemd cng /etc/localtime
grub2-pc g /etc/grub2.cfg
myapp g /etc/myapp/state.d
myapp cng /etc/myapp/absent.conf
myapp cng /etc/myapp/edited.conf
openssh-server g /etc/ssh/ssh_host_ed25519_key
mod_ssl g /etc/pki/tls/private/localhost.key
glibc g /usr/lib64/gconv/gconv-modules.cache
";

/// A host whose ghost files exist as the rows above describe: regular
/// files, one symlink, one directory, one absent. `myapp/edited.conf` is
/// also reported by `rpm -Va`, so it belongs to that branch.
fn ghost_host(ghost_query: inspectah_core::traits::executor::ExecResult) -> MockExecutor {
    MockExecutor::new()
        .with_command(GHOST_QUERY, ghost_query)
        .with_dir(
            "/etc",
            vec![
                "sssd",
                "machine-id",
                "hostname",
                "locale.conf",
                "localtime",
                "grub2.cfg",
                "myapp",
                "ssh",
                "pki",
            ],
        )
        .with_dir("/etc/sssd", vec!["sssd.conf"])
        .with_file(
            "/etc/sssd/sssd.conf",
            "[sssd]\ndomains = example.com\n\n[domain/example.com]\nid_provider = ad\n",
        )
        .with_file("/etc/machine-id", "0123456789abcdef0123456789abcdef\n")
        .with_file("/etc/hostname", "host1.example.com\n")
        .with_file("/etc/locale.conf", "LANG=en_GB.UTF-8\n")
        .with_link("/etc/localtime", "../usr/share/zoneinfo/Europe/London")
        .with_link("/etc/grub2.cfg", "../boot/grub2/grub.cfg")
        .with_dir("/etc/myapp", vec!["state.d", "edited.conf"])
        .with_dir("/etc/myapp/state.d", vec![])
        .with_file("/etc/myapp/edited.conf", "edited=1\n")
        .with_dir("/etc/ssh", vec!["ssh_host_ed25519_key"])
        .with_file("/etc/ssh/ssh_host_ed25519_key", "host key\n")
        .with_dir("/etc/pki", vec!["tls"])
        .with_dir("/etc/pki/tls", vec!["private"])
        .with_dir("/etc/pki/tls/private", vec!["localhost.key"])
        .with_file("/etc/pki/tls/private/localhost.key", "key\n")
}

/// Every `/etc` path the ghost rows list is RPM-owned, as the ownership
/// query reports it; `myapp/edited.conf` is also an `rpm -Va` row.
fn ghost_rpm_state() -> RpmState {
    let owned = [
        "/etc/sssd/sssd.conf",
        "/etc/machine-id",
        "/etc/hostname",
        "/etc/locale.conf",
        "/etc/localtime",
        "/etc/grub2.cfg",
        "/etc/myapp/state.d",
        "/etc/myapp/absent.conf",
        "/etc/myapp/edited.conf",
        "/etc/ssh/ssh_host_ed25519_key",
        "/etc/pki/tls/private/localhost.key",
    ];
    RpmState {
        owned_paths: owned.iter().map(PathBuf::from).collect(),
        verification_results: vec![RpmVaEntry {
            path: "/etc/myapp/edited.conf".into(),
            flags: "S.5....T.".into(),
            package: Some("myapp".into()),
        }],
        ..Default::default()
    }
}

fn ok_stdout(stdout: &str) -> inspectah_core::traits::executor::ExecResult {
    inspectah_core::traits::executor::ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

/// A ghost file that exists as a regular file is collected as a modified
/// owned config with its content and its package; the machine-id, the
/// hostname, SSH host keys and TLS private keys (machine-state members),
/// the locale file, a ghost symlink, a ghost directory and a ghost file
/// that is absent are not; a path `rpm -Va` reported is collected once,
/// by that branch.
#[test]
fn a_ghost_config_on_the_host_is_collected_and_host_identity_is_not() {
    let exec = ghost_host(ok_stdout(GHOST_ROWS));
    let rpm_state = ghost_rpm_state();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = ConfigInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("the inspector succeeds");
    let SectionData::Config(section) = &output.section else {
        panic!("expected a config section");
    };
    let paths: Vec<&str> = section.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["/etc/myapp/edited.conf", "/etc/sssd/sssd.conf"],
        "only the ghost sssd.conf and the rpm -Va row"
    );

    let sssd = &section.files[1];
    assert_eq!(sssd.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(sssd.package.as_deref(), Some("sssd-common"));
    assert_eq!(sssd.rpm_va_flags, None, "rpm never verified a ghost file");
    assert!(sssd.content.contains("id_provider = ad"));
    assert!(sssd.disposition.is_included());

    let edited = &section.files[0];
    assert_eq!(edited.rpm_va_flags.as_deref(), Some("S.5....T."));
}

/// A ghost-file query that fails leaves the section degraded and says
/// which files went unchecked.
#[test]
fn a_failed_ghost_query_degrades_the_section() {
    let exec = ghost_host(inspectah_core::traits::executor::ExecResult {
        exit_code: 1,
        stderr: "error: rpmdb open failed\n".into(),
        ..Default::default()
    });
    let rpm_state = ghost_rpm_state();
    let source = pkg_source();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    match ConfigInspector::new().inspect(&ctx, &NullProgress) {
        Err(InspectorError::Degraded { reason, .. }) => {
            assert!(reason.contains("%ghost"), "{reason}");
        }
        other => panic!("expected a degraded section, got {other:?}"),
    }
}

/// Only a regular file reaches content capture. The ghost rows name a
/// FIFO, a socket and a device (all registered with
/// `with_special_file`, whose `read_file` panics), a directory, a symlink
/// to a regular file, and one regular file. Opening the FIFO with no
/// writer would block collection on a real host.
#[test]
fn a_ghost_path_that_is_not_a_regular_file_is_never_read() {
    let rows = "\
myapp g /etc/myapp/control
myapp g /etc/myapp/socket
myapp g /etc/myapp/device
myapp g /etc/myapp/state.d
myapp g /etc/myapp/current.conf
myapp g /etc/myapp/real.conf
";
    let exec = with_ghosts(MockExecutor::new(), rows)
        .with_dir("/etc", vec!["myapp"])
        .with_dir(
            "/etc/myapp",
            vec![
                "control",
                "socket",
                "device",
                "state.d",
                "current.conf",
                "real.conf",
            ],
        )
        .with_special_file("/etc/myapp/control")
        .with_special_file("/etc/myapp/socket")
        .with_special_file("/etc/myapp/device")
        .with_dir("/etc/myapp/state.d", vec![])
        .with_link("/etc/myapp/current.conf", "real.conf")
        .with_file("/etc/myapp/real.conf", "mode=fast\n");
    let rpm_state = RpmState {
        owned_paths: [
            "/etc/myapp/control",
            "/etc/myapp/socket",
            "/etc/myapp/device",
            "/etc/myapp/state.d",
            "/etc/myapp/current.conf",
            "/etc/myapp/real.conf",
        ]
        .iter()
        .map(PathBuf::from)
        .collect(),
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
        .expect("the inspector succeeds");
    let SectionData::Config(section) = &output.section else {
        panic!("expected a config section");
    };
    let paths: Vec<&str> = section.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["/etc/myapp/real.conf"]);
    assert_eq!(section.files[0].content, "mode=fast\n");
}
