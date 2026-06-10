use std::io;
use std::path::{Path, PathBuf};

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

    /// Run a command, calling `on_stderr_line` for each line of stderr output.
    ///
    /// Used for long-running commands (e.g., `podman pull`) where the caller
    /// wants streaming stderr access for progress display. Full stderr is
    /// still captured in `ExecResult.stderr`. Uses the same 600s timeout
    /// as `run_passthrough_stderr`.
    ///
    /// # Contract
    ///
    /// - Callback is called per-line **live** as stderr is produced, not
    ///   accumulated and replayed after completion.
    /// - Callback runs on the main thread. No `Send` required.
    /// - Full stderr transcript is always available in `ExecResult.stderr`
    ///   for error diagnostics regardless of callback behavior.
    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult;

    fn read_file(&self, path: &Path) -> io::Result<String>;
    fn file_exists(&self, path: &Path) -> bool;
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>>;
    fn read_link(&self, path: &Path) -> io::Result<String>;
    fn host_root(&self) -> &Path;

    /// Resolve the final target of a path by following the entire symlink chain.
    ///
    /// For real executors this uses `std::fs::canonicalize()`. Mock executors
    /// walk their `links` map iteratively with cycle detection.
    ///
    /// Returns the fully-resolved absolute path, or an error if the chain is
    /// broken (dangling symlink), loops, or hits a permission wall.
    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf>;
}
