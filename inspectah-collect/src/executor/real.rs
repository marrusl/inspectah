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
