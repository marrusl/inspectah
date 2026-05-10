use inspectah_core::traits::executor::{ExecResult, Executor};
use std::io;
use std::path::Path;
use std::process::Command;

/// Phase 1: live-host only. No --host-root flag — all commands and
/// file reads target /. Containerized/offline inspection is deferred.
pub struct RealExecutor;

impl RealExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Executor for RealExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let result = Command::new(cmd)
            .args(args)
            .output();
        match result {
            Ok(output) => ExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            },
            Err(e) => ExecResult {
                stderr: e.to_string(),
                exit_code: 127,
                ..Default::default()
            },
        }
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
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
