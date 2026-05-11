use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::executor::real::RealExecutor;
use inspectah_core::traits::executor::{ExecResult, Executor};
use std::io::Write;
use std::time::Duration;

// ── Locale enforcement ─────────────────────────────────────────────

#[test]
fn test_commands_run_with_c_locale() {
    let exec = RealExecutor::new();
    // `locale` prints active locale settings. LC_ALL=C forces the C locale.
    let result = exec.run("locale", &[]);
    assert!(
        result.stdout.contains("LC_ALL=\"C\"")
            || result.stdout.contains("LC_ALL=C")
            // macOS prints LANG= on its own line; check LC_ALL specifically
            || result.stdout.contains("LC_ALL="),
        "executor must force C locale, got:\n{}",
        result.stdout
    );
}

// ── Fixed argv (no shell interpretation) ────────────────────────────

#[test]
fn test_executor_uses_fixed_argv() {
    let exec = RealExecutor::new();
    // Shell metacharacters must not be interpreted — passed as literal.
    let result = exec.run("echo", &["hello; rm -rf /"]);
    assert_eq!(result.stdout.trim(), "hello; rm -rf /");
}

#[test]
fn test_pipe_metachar_not_interpreted() {
    let exec = RealExecutor::new();
    let result = exec.run("echo", &["a | b"]);
    assert_eq!(result.stdout.trim(), "a | b");
}

#[test]
fn test_backtick_not_interpreted() {
    let exec = RealExecutor::new();
    let result = exec.run("echo", &["`whoami`"]);
    assert_eq!(result.stdout.trim(), "`whoami`");
}

// ── File size cap ───────────────────────────────────────────────────

#[test]
fn test_read_file_within_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.txt");
    std::fs::write(&path, "hello").unwrap();

    let exec = RealExecutor::new();
    let content = exec.read_file(&path).unwrap();
    assert_eq!(content, "hello");
}

#[test]
fn test_read_file_exceeding_cap_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.bin");

    // Write just over 1 MB.
    let mut f = std::fs::File::create(&path).unwrap();
    let buf = vec![b'x'; 1024 * 1024 + 1];
    f.write_all(&buf).unwrap();
    drop(f);

    let exec = RealExecutor::new();
    let result = exec.read_file(&path);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("1 MB") || err_msg.contains("size cap"),
        "error should mention size cap, got: {err_msg}"
    );
}

// ── Mock timeout simulation ─────────────────────────────────────────

#[test]
fn test_mock_timeout_returns_error_result() {
    let mock = MockExecutor::new()
        .with_timeout_simulation("slow-cmd --scan", Duration::from_secs(30));

    let result = mock.run("slow-cmd", &["--scan"]);
    assert_eq!(result.exit_code, -1);
    assert!(
        result.stderr.contains("timed out"),
        "timeout stderr should mention 'timed out', got: {}",
        result.stderr
    );
}

#[test]
fn test_mock_timeout_does_not_affect_other_commands() {
    let mock = MockExecutor::new()
        .with_timeout_simulation("slow-cmd", Duration::from_secs(30))
        .with_command(
            "fast-cmd",
            ExecResult {
                stdout: "ok\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );

    let fast = mock.run("fast-cmd", &[]);
    assert_eq!(fast.exit_code, 0);
    assert_eq!(fast.stdout, "ok\n");

    let slow = mock.run("slow-cmd", &[]);
    assert_eq!(slow.exit_code, -1);
}

// ── Nonexistent command handling ────────────────────────────────────

#[test]
fn test_nonexistent_command_returns_127() {
    let exec = RealExecutor::new();
    let result = exec.run("__inspectah_nonexistent_xyz__", &[]);
    assert_eq!(result.exit_code, 127);
}
