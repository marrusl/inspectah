use inspectah_core::traits::executor::{ExecResult, Executor};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Per-command timeout. Commands exceeding this are killed and return
/// exit_code=-1 with a descriptive stderr message.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum bytes read from stdout before truncation (64 MB).
const STDOUT_SIZE_CAP: usize = 64 * 1024 * 1024;

/// Maximum file size for `read_file()` (1 MB).
const FILE_SIZE_CAP: u64 = 1024 * 1024;

/// Phase 1: live-host only. No --host-root flag — all commands and
/// file reads target /. Containerized/offline inspection is deferred.
pub struct RealExecutor;

impl Default for RealExecutor {
    fn default() -> Self {
        Self
    }
}

impl RealExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Executor for RealExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        // Fixed argv via Command::new + args — never shell strings.
        // LC_ALL=C ensures deterministic, locale-independent output.
        let child = Command::new(cmd)
            .args(args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                return ExecResult {
                    stderr: e.to_string(),
                    exit_code: 127,
                    ..Default::default()
                };
            }
        };

        // Wait with timeout. On timeout, kill and report.
        match child.wait_timeout(COMMAND_TIMEOUT) {
            Ok(Some(status)) => {
                // Child exited within timeout. Read output.
                let stdout_raw = child
                    .stdout
                    .take()
                    .map(|mut r| {
                        let mut buf = Vec::new();
                        let _ = io::Read::read_to_end(&mut r, &mut buf);
                        buf
                    })
                    .unwrap_or_default();

                let stderr_raw = child
                    .stderr
                    .take()
                    .map(|mut r| {
                        let mut buf = Vec::new();
                        let _ = io::Read::read_to_end(&mut r, &mut buf);
                        buf
                    })
                    .unwrap_or_default();

                let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                    let mut s = String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned();
                    s.push_str("\n[output truncated at 64 MB]");
                    s
                } else {
                    String::from_utf8_lossy(&stdout_raw).into_owned()
                };

                ExecResult {
                    stdout,
                    stderr: String::from_utf8_lossy(&stderr_raw).into_owned(),
                    exit_code: status.code().unwrap_or(-1),
                }
            }
            Ok(None) => {
                // Timeout — kill the child.
                let _ = child.kill();
                let _ = child.wait(); // reap
                ExecResult {
                    stderr: format!(
                        "command timed out after {}s: {} {}",
                        COMMAND_TIMEOUT.as_secs(),
                        cmd,
                        args.join(" ")
                    ),
                    exit_code: -1,
                    ..Default::default()
                }
            }
            Err(e) => ExecResult {
                stderr: format!("failed to wait on child process: {e}"),
                exit_code: -1,
                ..Default::default()
            },
        }
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        let metadata = std::fs::metadata(path)?;
        if metadata.len() > FILE_SIZE_CAP {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "file exceeds 1 MB size cap ({} bytes): {}",
                    metadata.len(),
                    path.display()
                ),
            ));
        }
        std::fs::read_to_string(path)
    }

    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// Returns successfully-read entries. Individual entry errors (e.g.,
    /// permission denied on one file in a readable directory) are silently
    /// skipped — the caller gets a partial list. This matches the Go
    /// behavior where os.ReadDir errors are filtered, not fatal.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let entries = std::fs::read_dir(path)?;
        entries
            .filter_map(|e| e.ok())
            .map(|e| Ok(e.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        let target = std::fs::read_link(path)?;
        Ok(target.to_string_lossy().into_owned())
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::traits::executor::Executor;

    #[test]
    fn test_real_executor_host_root() {
        let exec = RealExecutor::new();
        assert_eq!(exec.host_root(), std::path::Path::new("/"));
    }

    #[test]
    fn test_real_executor_file_exists() {
        let exec = RealExecutor::new();
        // /etc/os-release exists on all Linux and macOS
        assert!(exec.file_exists(std::path::Path::new("/etc")));
    }

    #[test]
    fn test_real_executor_read_dir_returns_entries() {
        let exec = RealExecutor::new();
        let entries = exec.read_dir(std::path::Path::new("/tmp")).unwrap();
        // /tmp always exists — the point is it doesn't error
        let _ = entries; // may be empty, that's fine
    }

    #[test]
    fn test_real_executor_read_dir_nonexistent_errors() {
        let exec = RealExecutor::new();
        let result = exec.read_dir(std::path::Path::new("/nonexistent_dir_abc123"));
        assert!(result.is_err());
    }

    #[test]
    fn test_real_executor_run_echo() {
        let exec = RealExecutor::new();
        let result = exec.run("echo", &["hello"]);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim() == "hello");
    }

    #[test]
    fn test_real_executor_run_nonexistent_command() {
        let exec = RealExecutor::new();
        let result = exec.run("nonexistent_command_xyz789", &[]);
        assert_ne!(result.exit_code, 0);
    }

    #[test]
    fn test_real_executor_read_link() {
        use std::fs;
        let dir = std::env::temp_dir().join("inspectah_test_link");
        let target = dir.join("target.txt");
        let link = dir.join("link.txt");
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(&target, "content");
        let _ = std::os::unix::fs::symlink(&target, &link);

        let exec = RealExecutor::new();
        if link.exists() {
            let result = exec.read_link(&link).unwrap();
            assert!(result.contains("target.txt"));
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
