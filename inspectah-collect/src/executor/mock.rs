use inspectah_core::traits::executor::{ExecResult, Executor};
use std::collections::HashMap;
use std::io;
use std::path::Path;

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
}

impl Executor for MockExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let key = if args.is_empty() {
            cmd.to_string()
        } else {
            format!("{} {}", cmd, args.join(" "))
        };

        // Check for simulated timeout before normal command lookup.
        if let Some(duration) = self.timeout_commands.get(&key) {
            return ExecResult {
                stderr: format!("command timed out after {}s: {}", duration.as_secs(), key),
                exit_code: -1,
                ..Default::default()
            };
        }

        self.commands
            .get(&key)
            .cloned()
            .unwrap_or_else(|| ExecResult {
                stderr: format!("command not found: {key}"),
                exit_code: 127,
                ..Default::default()
            })
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
}
