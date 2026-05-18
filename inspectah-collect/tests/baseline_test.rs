//! Integration tests for baseline extraction.
//!
//! Verifies the extract_baseline function against MockExecutor,
//! including happy path, command ordering, cleanup behavior,
//! digest capture, and mixed-arch keying.

use inspectah_collect::baseline::{ExtractionError, extract_baseline};
use inspectah_collect::executor::mock::MockExecutor;
use inspectah_core::baseline::NormalizedImageRef;
use inspectah_core::traits::executor::ExecResult;

const TEST_IMAGE: &str = "registry.redhat.io/rhel9/rhel-bootc:9.4";

const NEVRA_OUTPUT: &str = "\
bash\t0\t5.2.26\t3.el9\tx86_64
coreutils\t0\t9.1\t13.el9\tx86_64
glibc\t0\t2.34\t83.el9\tx86_64
";

const TEST_DIGEST: &str = "sha256:abc123def456";

fn ok_result() -> ExecResult {
    ExecResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn ok_with_stdout(s: &str) -> ExecResult {
    ExecResult {
        stdout: s.to_string(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn fail_result(msg: &str) -> ExecResult {
    ExecResult {
        stdout: String::new(),
        stderr: msg.to_string(),
        exit_code: 1,
    }
}

/// Build a MockExecutor wired for the happy path.
fn happy_mock() -> MockExecutor {
    MockExecutor::new()
        // pull
        .with_command_prefix(
            &format!("nsenter -t 1 -m -u -i -n -- podman pull {}", TEST_IMAGE),
            ok_result(),
        )
        // create — use prefix because container name includes timestamp
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create --name inspectah-baseline-",
            ok_with_stdout("container-id-abc123\n"),
        )
        // start — prefix match on container name
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman start inspectah-baseline-",
            ok_result(),
        )
        // exec rpm -qa — prefix match on container name
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman exec inspectah-baseline-",
            ok_with_stdout(NEVRA_OUTPUT),
        )
        // rm -f — prefix match on container name
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman rm -f inspectah-baseline-",
            ok_result(),
        )
        // inspect image digest — exact image ref
        .with_command(
            &format!(
                "nsenter -t 1 -m -u -i -n -- podman inspect --format {{{{.Digest}}}} {}",
                TEST_IMAGE
            ),
            ok_with_stdout(&format!("{}\n", TEST_DIGEST)),
        )
}

#[test]
fn baseline_happy_path_extracts_packages_and_digest() {
    let mock = happy_mock();
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_ok(), "expected Ok, got {:?}", result);

    let data = result.unwrap();
    assert_eq!(data.packages.len(), 3);
    assert!(data.packages.contains_key("bash.x86_64"));
    assert!(data.packages.contains_key("coreutils.x86_64"));
    assert!(data.packages.contains_key("glibc.x86_64"));
    assert_eq!(data.image_digest, TEST_DIGEST);
    assert!(!data.extracted_at.is_empty());

    // Verify package fields.
    let bash = &data.packages["bash.x86_64"];
    assert_eq!(bash.name, "bash");
    assert_eq!(bash.epoch, Some("0".to_string())); // epoch "0" kept as Some("0") to match host RPM parser
    assert_eq!(bash.version, "5.2.26");
    assert_eq!(bash.release, "3.el9");
    assert_eq!(bash.arch, "x86_64");
}

#[test]
fn baseline_command_ordering() {
    let mock = happy_mock();
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_ok());

    let log = mock.command_log();
    assert!(
        log.len() >= 6,
        "expected at least 6 commands, got {}",
        log.len()
    );

    // Verify ordering: pull, create, start, exec, rm, inspect.
    assert!(
        log[0].contains("podman pull"),
        "first command should be pull, got: {}",
        log[0]
    );
    assert!(
        log[1].contains("podman create"),
        "second command should be create, got: {}",
        log[1]
    );
    assert!(
        log[2].contains("podman start"),
        "third command should be start, got: {}",
        log[2]
    );
    assert!(
        log[3].contains("podman exec") && log[3].contains("rpm"),
        "fourth command should be exec rpm, got: {}",
        log[3]
    );
    assert!(
        log[4].contains("podman rm -f"),
        "fifth command should be rm -f, got: {}",
        log[4]
    );
    // The inspect is on the IMAGE ref, not the container name.
    assert!(
        log[5].contains("podman inspect") && log[5].contains(TEST_IMAGE),
        "sixth command should inspect the image, got: {}",
        log[5]
    );
}

#[test]
fn baseline_create_includes_entrypoint_and_network_none() {
    let mock = happy_mock();
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_ok());

    let log = mock.command_log();
    let create_cmd = &log[1];
    assert!(
        create_cmd.contains("--entrypoint"),
        "create must include --entrypoint, got: {}",
        create_cmd
    );
    assert!(
        create_cmd.contains("--network none"),
        "create must include --network none, got: {}",
        create_cmd
    );
}

#[test]
fn baseline_pull_fails_no_rm_attempted() {
    let mock = MockExecutor::new().with_command_prefix(
        "nsenter -t 1 -m -u -i -n -- podman pull",
        fail_result("pull failed: unauthorized"),
    );
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_err());

    match result.unwrap_err() {
        ExtractionError::PullFailed { .. } => {}
        other => panic!("expected PullFailed, got {:?}", other),
    }

    // No rm should have been attempted (no container was created).
    let log = mock.command_log();
    assert!(
        !log.iter().any(|c| c.contains("podman rm")),
        "rm should not be attempted when pull fails"
    );
}

#[test]
fn baseline_create_fails_no_rm_attempted() {
    let mock = MockExecutor::new()
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman pull", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            fail_result("create failed: quota exceeded"),
        );
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_err());

    match result.unwrap_err() {
        ExtractionError::CreateFailed(_) => {}
        other => panic!("expected CreateFailed, got {:?}", other),
    }

    let log = mock.command_log();
    assert!(
        !log.iter().any(|c| c.contains("podman rm")),
        "rm should not be attempted when create fails"
    );
}

#[test]
fn baseline_start_fails_rm_runs() {
    let mock = MockExecutor::new()
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman pull", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            ok_with_stdout("ctr-id\n"),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman start",
            fail_result("start failed: OCI runtime error"),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman rm -f", ok_result());
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_err());

    match result.unwrap_err() {
        ExtractionError::StartFailed(_) => {}
        other => panic!("expected StartFailed, got {:?}", other),
    }

    let log = mock.command_log();
    assert!(
        log.iter().any(|c| c.contains("podman rm -f")),
        "rm -f should run when start fails (cleanup guard)"
    );
}

#[test]
fn baseline_exec_fails_rm_runs() {
    let mock = MockExecutor::new()
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman pull", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            ok_with_stdout("ctr-id\n"),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman start", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman exec",
            fail_result("exec failed: rpm not found"),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman rm -f", ok_result());
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_err());

    match result.unwrap_err() {
        ExtractionError::ExecFailed(_) => {}
        other => panic!("expected ExecFailed, got {:?}", other),
    }

    let log = mock.command_log();
    assert!(
        log.iter().any(|c| c.contains("podman rm -f")),
        "rm -f should run when exec fails (cleanup guard)"
    );
}

#[test]
fn baseline_digest_fallback_repo_digests() {
    // Primary digest returns empty, fallback returns repo digest with @.
    let mock = MockExecutor::new()
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman pull",
            ok_result(),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            ok_with_stdout("ctr-id\n"),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman start",
            ok_result(),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman exec",
            ok_with_stdout(NEVRA_OUTPUT),
        )
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman rm -f",
            ok_result(),
        )
        // Primary digest returns empty.
        .with_command(
            &format!(
                "nsenter -t 1 -m -u -i -n -- podman inspect --format {{{{.Digest}}}} {}",
                TEST_IMAGE
            ),
            ok_with_stdout("\n"),
        )
        // Fallback returns repo digest.
        .with_command(
            &format!(
                "nsenter -t 1 -m -u -i -n -- podman inspect --format {{{{index .RepoDigests 0}}}} {}",
                TEST_IMAGE
            ),
            ok_with_stdout(&format!(
                "registry.redhat.io/rhel9/rhel-bootc@sha256:fallback999\n"
            )),
        );
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_ok(), "expected Ok, got {:?}", result);

    let data = result.unwrap();
    assert_eq!(data.image_digest, "sha256:fallback999");
}

#[test]
fn baseline_mixed_arch_keys() {
    let mixed_output = "\
bash\t0\t5.2.26\t3.el9\taarch64
coreutils\t0\t9.1\t13.el9\taarch64
glibc\t0\t2.34\t83.el9\taarch64
";

    let mock = MockExecutor::new()
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman pull", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman create",
            ok_with_stdout("ctr-id\n"),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman start", ok_result())
        .with_command_prefix(
            "nsenter -t 1 -m -u -i -n -- podman exec",
            ok_with_stdout(mixed_output),
        )
        .with_command_prefix("nsenter -t 1 -m -u -i -n -- podman rm -f", ok_result())
        .with_command(
            &format!(
                "nsenter -t 1 -m -u -i -n -- podman inspect --format {{{{.Digest}}}} {}",
                TEST_IMAGE
            ),
            ok_with_stdout(&format!("{}\n", TEST_DIGEST)),
        );
    let normalized = NormalizedImageRef::from_validated(TEST_IMAGE.to_string());

    let result = extract_baseline(&mock, &normalized, &mut |_| {});
    assert!(result.is_ok());

    let data = result.unwrap();
    // Keys should use the arch from the output, not host arch.
    assert!(
        data.packages.contains_key("bash.aarch64"),
        "expected bash.aarch64 key"
    );
    assert!(
        !data.packages.contains_key("bash.x86_64"),
        "should not have bash.x86_64 key"
    );
    assert_eq!(data.packages.len(), 3);
}
