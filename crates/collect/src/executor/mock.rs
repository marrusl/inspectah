use inspectah_core::traits::executor::{ExecResult, Executor};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

pub struct MockExecutor {
    commands: HashMap<String, ExecResult>,
    files: HashMap<String, String>,
    dirs: HashMap<String, Vec<String>>,
    links: HashMap<String, String>,
    /// Commands that simulate a timeout. When `run()` matches one of
    /// these keys, it returns a timeout error result instead of the
    /// normal command lookup. The Duration is recorded for diagnostics
    /// but does not actually sleep.
    timeout_commands: HashMap<String, std::time::Duration>,
    /// Directories that should return a specific error kind when
    /// `read_dir` is called. Lets tests distinguish PermissionDenied
    /// from NotFound without registering actual directory entries.
    dir_errors: HashMap<String, io::ErrorKind>,
    /// Files that should return a specific error kind when
    /// `read_file` is called. Lets tests distinguish PermissionDenied
    /// from NotFound without registering actual file content.
    file_errors: HashMap<String, io::ErrorKind>,
    /// Prefix-based command matching. After exact-match lookup fails,
    /// `run()` checks if the full command string starts with any
    /// registered prefix.
    prefix_commands: HashMap<String, ExecResult>,
    /// Records every `run()` call in order (full cmd + args joined).
    command_log: Mutex<Vec<String>>,
}

impl MockExecutor {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            files: HashMap::new(),
            dirs: HashMap::new(),
            links: HashMap::new(),
            timeout_commands: HashMap::new(),
            dir_errors: HashMap::new(),
            file_errors: HashMap::new(),
            prefix_commands: HashMap::new(),
            command_log: Mutex::new(Vec::new()),
        }
    }
}

impl Default for MockExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl MockExecutor {
    pub fn with_command(mut self, key: &str, result: ExecResult) -> Self {
        self.commands.insert(key.to_string(), result);
        self
    }

    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(path.to_string(), content.to_string());
        self
    }

    pub fn with_dir(mut self, path: &str, entries: Vec<&str>) -> Self {
        self.dirs.insert(
            path.to_string(),
            entries.iter().map(|s| s.to_string()).collect(),
        );
        self
    }

    pub fn with_link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self
    }

    /// Register a command key that should simulate a timeout. When
    /// `run()` matches this key, it returns a timeout error result
    /// (exit_code=-1, descriptive stderr) without any actual delay.
    /// The `duration` is included in the error message for realism.
    pub fn with_timeout_simulation(mut self, cmd_key: &str, duration: std::time::Duration) -> Self {
        self.timeout_commands.insert(cmd_key.to_string(), duration);
        self
    }

    /// Register a directory path that should return a specific error
    /// when `read_dir` is called. This takes priority over both the
    /// `dirs` map and the default NotFound fallback, letting tests
    /// simulate PermissionDenied on directories that exist on disk.
    pub fn with_dir_error(mut self, path: &str, error_kind: io::ErrorKind) -> Self {
        self.dir_errors.insert(path.to_string(), error_kind);
        self
    }

    /// Register a file path that should return a specific error
    /// when `read_file` is called. This takes priority over both the
    /// `files` map and the default NotFound fallback, letting tests
    /// simulate PermissionDenied on files like /etc/shadow.
    pub fn with_file_error(mut self, path: &str, error_kind: io::ErrorKind) -> Self {
        self.file_errors.insert(path.to_string(), error_kind);
        self
    }

    /// Register a prefix→result mapping. In `run()`, after checking
    /// exact match, any command whose full string starts with this
    /// prefix will return the given result.
    pub fn with_command_prefix(mut self, prefix: &str, result: ExecResult) -> Self {
        self.prefix_commands.insert(prefix.to_string(), result);
        self
    }

    /// Returns the ordered log of all commands executed via `run()`.
    pub fn command_log(&self) -> Vec<String> {
        self.command_log.lock().unwrap().clone()
    }
}

impl Executor for MockExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let key = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };

        // Record every command invocation.
        self.command_log.lock().unwrap().push(key.clone());

        // Check for simulated timeout before normal command lookup.
        if let Some(duration) = self.timeout_commands.get(&key) {
            return ExecResult {
                stderr: format!("command timed out after {}s: {}", duration.as_secs(), key),
                exit_code: -1,
                ..Default::default()
            };
        }

        // Exact match first.
        if let Some(result) = self.commands.get(&key) {
            return result.clone();
        }

        // Prefix match fallback.
        for (prefix, result) in &self.prefix_commands {
            if key.starts_with(prefix.as_str()) {
                return result.clone();
            }
        }

        ExecResult {
            stderr: format!("command not found: {key}"),
            exit_code: 127,
            ..Default::default()
        }
    }

    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let result = self.run(cmd, args);
        // Split pre-recorded stderr and call callback per-line.
        for line in result.stderr.lines() {
            on_stderr_line(line);
        }
        result
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        let key = path.to_str().unwrap_or("");
        // Explicit error injection takes priority over registered files.
        if let Some(&error_kind) = self.file_errors.get(key) {
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        self.files
            .get(key)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.files.contains_key(path.to_str().unwrap_or(""))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let key = path.to_str().unwrap_or("");
        // Explicit error injection takes priority over registered dirs.
        if let Some(&error_kind) = self.dir_errors.get(key) {
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        self.dirs
            .get(key)
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        self.links
            .get(path.to_str().unwrap_or(""))
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.display().to_string()))
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }

    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf> {
        let mut current = normalize_mock_path(path);
        let mut visited = HashSet::new();

        loop {
            let key = current.to_str().unwrap_or("").to_string();
            if !visited.insert(key.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink loop detected at {}", current.display()),
                ));
            }

            let target_str = match self.links.get(&key) {
                Some(t) => t.clone(),
                None => return Ok(current),
            };

            let target_path = Path::new(&target_str);
            current = if target_path.is_absolute() {
                normalize_mock_path(target_path)
            } else {
                let parent = current.parent().unwrap_or(Path::new("/"));
                normalize_mock_path(&parent.join(target_path))
            };
        }
    }
}

/// Lexical path normalization for mock symlink resolution.
/// Resolves `.` and `..` components without filesystem access.
fn normalize_mock_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !components.is_empty() {
                    components.pop();
                }
            }
            Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::traits::executor::Executor;
    use std::path::Path;

    #[test]
    fn test_mock_command_lookup() {
        let mock = MockExecutor::new().with_command(
            "rpm -qa",
            ExecResult {
                stdout: "bash-5.2.26-3.el9.x86_64\n".into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let result = mock.run("rpm", &["-qa"]);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("bash"));
    }

    #[test]
    fn test_mock_unknown_command() {
        let mock = MockExecutor::new();
        let result = mock.run("nonexistent", &[]);
        assert_eq!(result.exit_code, 127);
    }

    #[test]
    fn test_mock_file_read() {
        let mock = MockExecutor::new().with_file("/etc/os-release", "ID=rhel\nVERSION_ID=9.4\n");
        let content = mock.read_file(Path::new("/etc/os-release")).unwrap();
        assert!(content.contains("ID=rhel"));
    }

    #[test]
    fn test_mock_file_not_found() {
        let mock = MockExecutor::new();
        assert!(mock.read_file(Path::new("/nonexistent")).is_err());
        assert!(!mock.file_exists(Path::new("/nonexistent")));
    }

    #[test]
    fn test_mock_read_dir() {
        let mock =
            MockExecutor::new().with_dir("/etc/yum.repos.d", vec!["redhat.repo", "epel.repo"]);
        let entries = mock.read_dir(Path::new("/etc/yum.repos.d")).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&"redhat.repo".to_string()));
    }

    #[test]
    fn test_mock_read_dir_not_found() {
        let mock = MockExecutor::new();
        assert!(mock.read_dir(Path::new("/nonexistent")).is_err());
    }

    #[test]
    fn test_mock_read_link() {
        let mock = MockExecutor::new().with_link(
            "/etc/resolv.conf",
            "../run/systemd/resolve/stub-resolv.conf",
        );
        let target = mock.read_link(Path::new("/etc/resolv.conf")).unwrap();
        assert_eq!(target, "../run/systemd/resolve/stub-resolv.conf");
    }

    #[test]
    fn test_mock_host_root() {
        let mock = MockExecutor::new();
        assert_eq!(mock.host_root(), Path::new("/"));
    }

    #[test]
    fn test_mock_resolve_final_target_no_link() {
        let mock = MockExecutor::new();
        let result = mock
            .resolve_final_target(Path::new("/etc/rhsm/rhsm.conf"))
            .unwrap();
        assert_eq!(result, PathBuf::from("/etc/rhsm/rhsm.conf"));
    }

    #[test]
    fn test_mock_resolve_final_target_single_hop() {
        let mock = MockExecutor::new().with_link(
            "/etc/pki/entitlement/link.pem",
            "/etc/pki/entitlement/real.pem",
        );
        let result = mock
            .resolve_final_target(Path::new("/etc/pki/entitlement/link.pem"))
            .unwrap();
        assert_eq!(result, PathBuf::from("/etc/pki/entitlement/real.pem"));
    }

    #[test]
    fn test_mock_resolve_final_target_multi_hop() {
        let mock = MockExecutor::new()
            .with_link("/etc/pki/entitlement/a.pem", "/etc/pki/entitlement/b.pem")
            .with_link("/etc/pki/entitlement/b.pem", "/etc/shadow");
        let result = mock
            .resolve_final_target(Path::new("/etc/pki/entitlement/a.pem"))
            .unwrap();
        assert_eq!(result, PathBuf::from("/etc/shadow"));
    }

    #[test]
    fn test_mock_resolve_final_target_relative() {
        let mock = MockExecutor::new().with_link("/etc/pki/entitlement/link.pem", "../../shadow");
        let result = mock
            .resolve_final_target(Path::new("/etc/pki/entitlement/link.pem"))
            .unwrap();
        assert_eq!(result, PathBuf::from("/etc/shadow"));
    }

    #[test]
    fn test_mock_resolve_final_target_loop() {
        let mock = MockExecutor::new()
            .with_link("/etc/pki/entitlement/a.pem", "/etc/pki/entitlement/b.pem")
            .with_link("/etc/pki/entitlement/b.pem", "/etc/pki/entitlement/a.pem");
        let result = mock.resolve_final_target(Path::new("/etc/pki/entitlement/a.pem"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("loop"));
    }

    #[test]
    fn test_mock_line_callback() {
        let mock = MockExecutor::new().with_command(
            "podman pull quay.io/test:latest",
            ExecResult {
                stderr: "Copying blob sha256:aaa... done\nCopying blob sha256:bbb... skipped\n"
                    .into(),
                exit_code: 0,
                ..Default::default()
            },
        );
        let mut lines = Vec::new();
        let result =
            mock.run_with_line_callback("podman", &["pull", "quay.io/test:latest"], &mut |line| {
                lines.push(line.to_string())
            });
        assert_eq!(result.exit_code, 0);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("aaa"));
        assert!(lines[1].contains("bbb"));
    }
}
