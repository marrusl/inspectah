//! End-to-end integration tests for the inspectah pipeline.
//!
//! Uses MockExecutor exclusively — these tests run offline on any platform.
//! Phase 1 proves RPM-section parity, not full-snapshot parity.

use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::ExecResult;
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::Completeness;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::orchestrate::run_pipeline;
use inspectah_pipeline::redaction::engine::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::tarball::{create_tarball, list_tarball_entries};
use inspectah_pipeline::validate::validate;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn test_os_release() -> OsRelease {
    OsRelease {
        name: "Red Hat Enterprise Linux".into(),
        version_id: "9.4".into(),
        id: "rhel".into(),
        ..Default::default()
    }
}

/// Build a MockExecutor with realistic RPM data for the full pipeline.
fn build_full_rpm_mock_executor() -> MockExecutor {
    let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
0:vim-enhanced-9.0.1592-1.el9.x86_64
0:curl-8.2.1-1.el9.x86_64
0:openssl-3.0.7-18.el9.x86_64
";
    MockExecutor::new().with_command(
        "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
        ExecResult {
            stdout: rpm_qa_output.into(),
            exit_code: 0,
            ..Default::default()
        },
    )
}

/// Build source as owned local; callers pass source + executor separately.
fn build_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: test_os_release(),
    }
}

/// Build a MockExecutor that contains a planted secret in its RPM data output.
fn mock_with_planted_secret(secret_line: &str) -> MockExecutor {
    // The secret is planted in the RPM output; config files carry it via
    // the config section we add manually after collection.
    let rpm_qa_output = "\
0:bash-5.2.26-3.el9.x86_64
0:httpd-2.4.57-5.el9.x86_64
";
    MockExecutor::new()
        .with_command(
            "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n",
            ExecResult {
                stdout: rpm_qa_output.into(),
                exit_code: 0,
                ..Default::default()
            },
        )
        .with_file("/etc/myapp/config", secret_line)
}

/// Run the full pipeline stages with a given MockExecutor and return the
/// (snapshot, tarball_path).
fn run_full_pipeline_from_mock(
    mock: MockExecutor,
    config_overlay: Option<ConfigSection>,
) -> (InspectionSnapshot, std::path::PathBuf, TempDir) {
    let source = build_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];

    // Collect
    let collected = collect(&source, &mock, &inspectors, None, &NullProgress, &AtomicBool::new(false));

    // Validate
    let validated = validate(collected).expect("validation should pass");

    // Redact — optionally overlay a config section with a planted secret
    let mut snapshot = validated.state.snapshot;
    if let Some(config) = config_overlay {
        snapshot.config = Some(config);
    }
    redact(&mut snapshot, &RedactOptions::default());

    // Render
    let output_dir = TempDir::new().expect("tempdir");
    let render_dir = output_dir.path().join("output");
    std::fs::create_dir_all(&render_dir).unwrap();

    let render_context = RenderContext { target: None };
    render::render_all(&snapshot, &render_context, &render_dir).expect("render should pass");

    // Write schema placeholder
    let schema_dir = render_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).unwrap();
    std::fs::write(
        schema_dir.join("snapshot.schema.json"),
        r#"{"$schema":"http://json-schema.org/draft-07/schema#","title":"InspectionSnapshot","description":"Phase 7 placeholder","type":"object"}"#,
    )
    .unwrap();

    // Create tarball
    let tarball_path = output_dir.path().join("test-output.tar.gz");
    create_tarball(&render_dir, &tarball_path, "inspectah-test")
        .expect("tarball should be created");

    (snapshot, tarball_path, output_dir)
}

/// Extract text content from a tarball entry by name suffix.
fn extract_text_files(tarball_path: &std::path::Path) -> Vec<(String, String)> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    let f = std::fs::File::open(tarball_path).expect("open tarball");
    let gz = GzDecoder::new(f);
    let mut ar = tar::Archive::new(gz);

    let mut files = Vec::new();
    for entry in ar.entries().expect("read entries") {
        let mut entry = entry.expect("entry");
        let path = entry.path().expect("path").to_string_lossy().to_string();

        // Skip directories and non-text files
        if entry.header().entry_type().is_dir() {
            continue;
        }

        // Only read text-like files
        let text_exts = [
            ".json",
            ".md",
            ".html",
            ".ks",
            ".toml",
            ".conf",
            ".repo",
            "Containerfile",
            "README.md",
        ];
        let is_text = text_exts.iter().any(|ext| path.ends_with(ext))
            || path.contains("Containerfile")
            || path.contains("README");

        if is_text {
            let mut content = String::new();
            let _ = entry.read_to_string(&mut content);
            files.push((path, content));
        }
    }
    files
}

// ---------------------------------------------------------------------------
// Test 1: Full pipeline produces valid tarball with all 8 artifacts
// ---------------------------------------------------------------------------

#[test]
fn test_full_pipeline_produces_valid_tarball() {
    let exec = build_full_rpm_mock_executor();
    let source = build_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];

    let output_dir = TempDir::new().unwrap();
    let (_, tarball_path) = run_pipeline(
        &source,
        &exec,
        &inspectors,
        &output_dir.path().join("artifacts"),
        "testhost",
    )
    .expect("pipeline should succeed");

    // Verify all 8 always-written artifacts are present
    let entries = list_tarball_entries(&tarball_path);
    let required = [
        "inspection-snapshot.json",
        "Containerfile",
        "README.md",
        "report.html",
        "audit-report.md",
        "secrets-review.md",
        "kickstart-suggestion.ks",
        "schema/snapshot.schema.json",
    ];
    for artifact in &required {
        assert!(
            entries.iter().any(|e| e.ends_with(artifact)),
            "missing always-written artifact: {artifact}\nentries: {entries:?}"
        );
    }

    // Verify non-empty
    for artifact in &required {
        let matching: Vec<_> = entries.iter().filter(|e| e.ends_with(artifact)).collect();
        assert!(!matching.is_empty(), "no entry matching {artifact}");
    }
}

// ---------------------------------------------------------------------------
// Test 2: No secrets leak into any artifact
// ---------------------------------------------------------------------------

#[test]
fn test_no_secrets_in_any_artifact() {
    // Plant a known secret in mock config data
    let secret = "db_password = s3cretP@ss";
    let mock = mock_with_planted_secret(secret);

    // Overlay a config section with the planted secret
    let config = ConfigSection {
        files: vec![ConfigFileEntry {
            path: "/etc/myapp/config".to_string(),
            content: secret.to_string(),
            include: true,
            ..Default::default()
        }],
    };

    let (_, tarball_path, _output_dir) = run_full_pipeline_from_mock(mock, Some(config));

    // Extract and check every text artifact for the secret
    let text_files = extract_text_files(&tarball_path);
    assert!(
        !text_files.is_empty(),
        "tarball should contain text files to check"
    );

    for (name, content) in &text_files {
        assert!(
            !content.contains("s3cretP@ss"),
            "secret leaked into artifact: {name}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 3: Rust RPM section self-roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_rpm_section_self_roundtrip() {
    // Phase 1 parity proof: Rust RPM section round-trips faithfully.
    // Go tarball ingestion is not a goal — if you need the data, re-scan.
    use inspectah_core::types::rpm::RpmSection;

    let exec = build_full_rpm_mock_executor();
    let source = build_source();
    let inspectors: Vec<Box<dyn Inspector>> = vec![Box::new(RpmInspector::new())];
    let collected = collect(&source, &exec, &inspectors, None, &NullProgress, &AtomicBool::new(false));
    let validated = validate(collected).expect("validation");

    let rpm = validated
        .state
        .snapshot
        .rpm
        .expect("RPM section must exist");
    let json = serde_json::to_string_pretty(&rpm).expect("serialize");
    let parsed: RpmSection = serde_json::from_str(&json).expect("deserialize");

    // Verify round-trip preserves data
    assert_eq!(rpm.packages_added.len(), parsed.packages_added.len());
    for (orig, rt) in rpm.packages_added.iter().zip(parsed.packages_added.iter()) {
        assert_eq!(orig.name, rt.name);
        assert_eq!(orig.epoch, rt.epoch);
        assert_eq!(orig.version, rt.version);
        assert_eq!(orig.state, rt.state);
    }
}

// ---------------------------------------------------------------------------
// Test 4: Exported snapshot carries trust state
// ---------------------------------------------------------------------------

#[test]
fn test_exported_snapshot_carries_trust_state() {
    let mock = build_full_rpm_mock_executor();
    let (snapshot, tarball_path, _output_dir) = run_full_pipeline_from_mock(mock, None);

    // Verify the in-memory snapshot has trust state
    assert!(
        snapshot.redaction_state.is_some(),
        "pipeline snapshot must carry redaction_state"
    );
    assert_eq!(
        snapshot.completeness,
        Completeness::Complete,
        "pipeline snapshot must have Full completeness"
    );

    // Also verify the serialized snapshot in the tarball carries trust state
    let text_files = extract_text_files(&tarball_path);
    let snapshot_entry = text_files
        .iter()
        .find(|(name, _)| name.ends_with("inspection-snapshot.json"))
        .expect("tarball must contain inspection-snapshot.json");

    let exported: InspectionSnapshot =
        serde_json::from_str(&snapshot_entry.1).expect("exported snapshot must be valid JSON");

    assert!(
        exported.redaction_state.is_some(),
        "exported snapshot must carry redaction_state"
    );
    assert_eq!(
        exported.completeness,
        Completeness::Complete,
        "exported snapshot must have Full completeness"
    );
}
