use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ExecResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

pub trait Executor: Send + Sync {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult;

    /// Run a command with stderr passed through to the terminal in real-time.
    ///
    /// Used for long-running commands (e.g., `podman pull`) where the user
    /// should see live progress output. Stdout is still captured normally.
    /// Default implementation falls back to `run()`.
    fn run_passthrough_stderr(&self, cmd: &str, args: &[&str]) -> ExecResult {
        self.run(cmd, args)
    }

    fn read_file(&self, path: &Path) -> io::Result<String>;
    fn file_exists(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>>;
    fn read_link(&self, path: &Path) -> io::Result<String>;
    fn host_root(&self) -> &Path;
}
