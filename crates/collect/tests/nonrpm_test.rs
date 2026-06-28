//! Integration tests for NonRpmInspector.
//!
//! Runs the actual inspector on fixture data via MockExecutor
//! and verifies output is structurally correct.

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::nonrpm::NonRpmInspector;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::{InspectionContext, Inspector, InspectorError, RpmState};
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::types::completeness::SectionData;
use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;

// ── Fixtures ────────────────────────────────────────────────────────

const READELF_SECTIONS_GO: &str =
    include_str!("../../../testdata/fixtures/nonrpm/readelf-sections-go.txt");
const STRINGS_VERSION: &str = include_str!("../../../testdata/fixtures/nonrpm/strings-version.txt");
const PYVENV_CFG: &str = include_str!("../../../testdata/fixtures/nonrpm/pyvenv.cfg");
const PACKAGE_LOCK: &str = include_str!("../../../testdata/fixtures/nonrpm/package-lock.json");
const GEMFILE_LOCK: &str = include_str!("../../../testdata/fixtures/nonrpm/gemfile.lock");
const ENV_FILE: &str = include_str!("../../../testdata/fixtures/nonrpm/env-file.txt");
const GIT_CONFIG: &str = include_str!("../../../testdata/fixtures/nonrpm/git-config");

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

fn empty_rpm_state() -> RpmState {
    RpmState::default()
}

/// Build a MockExecutor with ELF binary + pip venv + npm + gem + env + git.
fn full_mock() -> MockExecutor {
    MockExecutor::new()
        // readelf and file probes succeed
        .with_command(
            "readelf --version",
            ExecResult {
                stdout: "GNU readelf version 2.40\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "file --version",
            ExecResult {
                stdout: "file-5.39\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // /opt has a Go binary
        .with_dir("/opt", vec!["myapp"])
        .with_dir("/opt/myapp", vec!["bin", ".env", ".git"])
        .with_dir("/opt/myapp/bin", vec!["myapp"])
        // readelf -S for section headers (Go binary)
        .with_command(
            "readelf -S /opt/myapp/bin/myapp",
            ExecResult {
                stdout: READELF_SECTIONS_GO.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // readelf -d for dynamic linking
        .with_command(
            "readelf -d /opt/myapp/bin/myapp",
            ExecResult {
                stdout: "There is no dynamic section in this file.\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // file command for binary type check
        .with_command(
            "file /opt/myapp/bin/myapp",
            ExecResult {
                stdout: "/opt/myapp/bin/myapp: ELF 64-bit LSB executable, x86-64\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // strings for version extraction
        .with_command(
            "strings /opt/myapp/bin/myapp",
            ExecResult {
                stdout: STRINGS_VERSION.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // .env file
        .with_dir("/opt/myapp/.env", vec![])
        .with_file("/opt/myapp/.env", ENV_FILE)
        // git repo
        .with_dir("/opt/myapp/.git", vec!["config"])
        .with_file("/opt/myapp/.git/config", GIT_CONFIG)
        // /srv has a pip venv
        .with_dir("/srv", vec!["webapp"])
        .with_dir("/srv/webapp", vec!["venv"])
        .with_dir("/srv/webapp/venv", vec!["pyvenv.cfg", "lib"])
        .with_file("/srv/webapp/venv/pyvenv.cfg", PYVENV_CFG)
        .with_dir("/srv/webapp/venv/lib", vec!["python3.9"])
        .with_dir("/srv/webapp/venv/lib/python3.9", vec!["site-packages"])
        .with_dir(
            "/srv/webapp/venv/lib/python3.9/site-packages",
            vec!["flask-2.3.3.dist-info", "requests-2.31.0.dist-info"],
        )
        .with_dir(
            "/srv/webapp/venv/lib/python3.9/site-packages/flask-2.3.3.dist-info",
            vec!["METADATA"],
        )
        .with_file(
            "/srv/webapp/venv/lib/python3.9/site-packages/flask-2.3.3.dist-info/METADATA",
            "Name: flask\nVersion: 2.3.3\n",
        )
        .with_dir(
            "/srv/webapp/venv/lib/python3.9/site-packages/requests-2.31.0.dist-info",
            vec!["METADATA"],
        )
        .with_file(
            "/srv/webapp/venv/lib/python3.9/site-packages/requests-2.31.0.dist-info/METADATA",
            "Name: requests\nVersion: 2.31.0\n",
        )
        // /usr/local has npm and gem projects
        .with_dir("/usr/local", vec!["nodeapp", "rubyapp"])
        .with_dir("/usr/local/nodeapp", vec!["package-lock.json"])
        .with_file("/usr/local/nodeapp/package-lock.json", PACKAGE_LOCK)
        .with_dir("/usr/local/rubyapp", vec!["Gemfile.lock"])
        .with_file("/usr/local/rubyapp/Gemfile.lock", GEMFILE_LOCK)
}

// ── Tests ───────────────────────────────────────────────────────────

/// Happy path: ELF + pip + npm + gem + env + git all detected.
#[test]
fn test_nonrpm_inspector_happy_path() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = empty_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = NonRpmInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("nonrpm inspector should succeed on full fixture set");

    let section = match &output.section {
        SectionData::NonRpmSoftware(s) => s,
        other => panic!("expected SectionData::NonRpmSoftware, got {:?}", other),
    };

    // Should have items (ELF binaries, venvs, npm, gem, git)
    assert!(
        !section.items.is_empty(),
        "inspector must produce items from fixture data"
    );

    // Should have env files
    assert!(
        !section.env_files.is_empty(),
        "inspector must find .env files"
    );

    // Check that env file content is captured
    let env = &section.env_files[0];
    assert!(
        env.content.contains("DATABASE_URL"),
        "env file should have content from fixture"
    );
}

/// Empty system: no /opt, /srv, /usr/local targets.
#[test]
fn test_nonrpm_inspector_empty_system() {
    let exec = MockExecutor::new()
        // readelf available
        .with_command(
            "readelf --version",
            ExecResult {
                stdout: "GNU readelf version 2.40\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "file --version",
            ExecResult {
                stdout: "file-5.39\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        // Empty scan roots
        .with_dir("/opt", vec![])
        .with_dir("/srv", vec![])
        .with_dir("/usr/local", vec![]);

    let source = pkg_source();
    let rpm_state = empty_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = NonRpmInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed on empty system");

    let section = match &output.section {
        SectionData::NonRpmSoftware(s) => s,
        other => panic!("expected SectionData::NonRpmSoftware, got {:?}", other),
    };

    assert!(
        section.items.is_empty(),
        "empty system should have no items"
    );
    assert!(
        section.env_files.is_empty(),
        "empty system should have no env files"
    );
}

/// readelf unavailable -- returns Degraded when there are still other items.
#[test]
fn test_nonrpm_inspector_degraded_no_readelf() {
    let exec = MockExecutor::new()
        // readelf NOT available (returns 127)
        // file NOT available either
        // But we have an .env file and git repo so partial data exists
        .with_dir("/opt", vec!["myapp"])
        .with_dir("/opt/myapp", vec![".env", ".git"])
        .with_file("/opt/myapp/.env", ENV_FILE)
        .with_dir("/opt/myapp/.git", vec!["config"])
        .with_file("/opt/myapp/.git/config", GIT_CONFIG)
        .with_dir("/srv", vec![])
        .with_dir("/usr/local", vec![]);

    let source = pkg_source();
    let rpm_state = empty_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let result = NonRpmInspector::new().inspect(&ctx, &NullProgress);

    match result {
        Err(InspectorError::Degraded { partial, reason }) => {
            assert!(
                reason.contains("readelf"),
                "degraded reason should mention readelf, got: {reason}"
            );
            let section = match &partial.section {
                SectionData::NonRpmSoftware(s) => s,
                other => panic!("expected NonRpmSoftware in partial, got {:?}", other),
            };
            // Should still have env files even without readelf
            assert!(
                !section.env_files.is_empty(),
                "partial output should still have env files"
            );
        }
        Ok(output) => {
            // If env/git produced no items, Ok is also acceptable
            let section = match &output.section {
                SectionData::NonRpmSoftware(s) => s,
                other => panic!("expected NonRpmSoftware, got {:?}", other),
            };
            // Should have a warning about readelf
            assert!(
                output
                    .warnings
                    .iter()
                    .any(|w| w.message.contains("readelf")),
                "should warn about readelf being unavailable"
            );
            // Items may be empty (no ELF classification)
            let _ = section;
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

/// Output serializes and deserializes cleanly.
#[test]
fn test_nonrpm_inspector_json_roundtrip() {
    let exec = full_mock();
    let source = pkg_source();
    let rpm_state = empty_rpm_state();
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = NonRpmInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::NonRpmSoftware(s) => s,
        other => panic!("expected SectionData::NonRpmSoftware, got {:?}", other),
    };

    let json = serde_json::to_string_pretty(section).expect("section must serialize to JSON");
    let roundtrip: NonRpmSoftwareSection =
        serde_json::from_str(&json).expect("JSON must deserialize back");
    let roundtrip_json =
        serde_json::to_string_pretty(&roundtrip).expect("roundtrip must serialize");

    assert_eq!(
        json, roundtrip_json,
        "inspector output must round-trip faithfully through NonRpmSoftwareSection"
    );
}

/// RpmState with installed_packages affects pip filtering —
/// packages matching `python3-<name>` RPMs are excluded from pip inventory.
#[test]
fn test_nonrpm_uses_rpm_state_for_pip_filtering() {
    let exec_a = full_mock();
    let exec_b = full_mock();
    let source = pkg_source();

    // Run 1: empty rpm_state — no pip packages filtered.
    let rpm_state_a = RpmState::default();
    let ctx_a = InspectionContext {
        source_system: &source,
        executor: &exec_a,
        rpm_state: Some(&rpm_state_a),
        baseline_data: None,
    };

    let output_a = NonRpmInspector::new()
        .inspect(&ctx_a, &NullProgress)
        .expect("run A should succeed");

    // Run 2: rpm_state with non-Python RPMs — should NOT affect pip output.
    let mut owned = std::collections::HashSet::new();
    owned.insert(std::path::PathBuf::from("/etc/httpd/conf/httpd.conf"));
    let rpm_state_b = RpmState {
        installed_packages: ["httpd", "openssh-server", "vim"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        owned_paths: owned,
        ..Default::default()
    };
    let ctx_b = InspectionContext {
        source_system: &source,
        executor: &exec_b,
        rpm_state: Some(&rpm_state_b),
        baseline_data: None,
    };

    let output_b = NonRpmInspector::new()
        .inspect(&ctx_b, &NullProgress)
        .expect("run B should succeed");

    // Both should produce the same items (no python3-* RPMs in either).
    let json_a = serde_json::to_string(&output_a.section).expect("A must serialize");
    let json_b = serde_json::to_string(&output_b.section).expect("B must serialize");

    assert_eq!(
        json_a, json_b,
        "NonRpm output should be identical when no python3-* RPMs are installed"
    );
}

/// When `installed_packages` contains `python3-requests`, the `requests`
/// dist-info package should be excluded from pip inventory while `flask`
/// (not RPM-owned) survives.
#[test]
fn test_nonrpm_pip_filtering_excludes_rpm_owned() {
    // Minimal mock: only system-level pip dist-info under /usr/lib/python3.9.
    let exec = MockExecutor::new()
        .with_command(
            "readelf --version",
            ExecResult {
                stdout: "GNU readelf version 2.40\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_command(
            "file --version",
            ExecResult {
                stdout: "file-5.39\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_dir("/opt", vec![])
        .with_dir("/srv", vec![])
        .with_dir("/usr/local", vec![])
        // System-level python with two dist-info packages.
        .with_dir("/usr/lib", vec!["python3.9"])
        .with_dir("/usr/lib/python3.9", vec!["site-packages"])
        .with_dir(
            "/usr/lib/python3.9/site-packages",
            vec!["requests-2.31.0.dist-info", "flask-2.3.3.dist-info"],
        );

    let source = pkg_source();

    // RpmState with python3-requests installed — should exclude `requests`.
    let rpm_state = RpmState {
        installed_packages: ["python3-requests"].iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let ctx = InspectionContext {
        source_system: &source,
        executor: &exec,
        rpm_state: Some(&rpm_state),
        baseline_data: None,
    };

    let output = NonRpmInspector::new()
        .inspect(&ctx, &NullProgress)
        .expect("inspector should succeed");

    let section = match &output.section {
        SectionData::NonRpmSoftware(s) => s,
        other => panic!("expected SectionData::NonRpmSoftware, got {:?}", other),
    };

    // `flask` should survive (not RPM-owned).
    let pip_items: Vec<_> = section
        .items
        .iter()
        .filter(|i| i.method == "pip dist-info")
        .collect();
    assert_eq!(
        pip_items.len(),
        1,
        "only flask should survive RPM filtering, got: {:?}",
        pip_items.iter().map(|i| &i.name).collect::<Vec<_>>()
    );
    assert_eq!(pip_items[0].name, "flask");
    assert!(
        pip_items[0].rpm_filtered,
        "surviving pip items should have rpm_filtered=true"
    );
    assert_eq!(
        pip_items[0].confidence, "medium",
        "dist-info + RPM cross-ref should be medium confidence"
    );

    // Verify `requests` was excluded.
    let requests_items: Vec<_> = section
        .items
        .iter()
        .filter(|i| i.name == "requests")
        .collect();
    assert!(
        requests_items.is_empty(),
        "requests should be excluded because python3-requests RPM is installed"
    );
}
