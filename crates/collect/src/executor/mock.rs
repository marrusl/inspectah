use inspectah_core::traits::executor::{ExecResult, Executor};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// The most links one resolution follows before it fails as a loop, the
/// kernel's `ELOOP` limit.
const MAX_SYMLINK_HOPS: usize = 40;

pub struct MockExecutor {
    commands: HashMap<String, ExecResult>,
    files: HashMap<String, String>,
    dirs: HashMap<String, Vec<String>>,
    links: HashMap<String, String>,
    /// Link paths whose chain ends at a path that does not exist, so
    /// `resolve_final_target` fails on them as `canonicalize` does.
    dangling_links: HashSet<String>,
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
    /// Directories whose listing yields the registered entries and then
    /// fails with a specific error kind partway, as a real directory
    /// iterator can. `read_dir` drops the failure, as the real one does;
    /// `read_dir_strict` returns it.
    dir_iteration_errors: HashMap<String, io::ErrorKind>,
    /// Paths whose `read_link` fails with a specific error kind, other
    /// than the `InvalidInput` (`EINVAL`) a path that is not a link gives.
    link_errors: HashMap<String, io::ErrorKind>,
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
            dangling_links: HashSet::new(),
            timeout_commands: HashMap::new(),
            dir_errors: HashMap::new(),
            file_errors: HashMap::new(),
            dir_iteration_errors: HashMap::new(),
            link_errors: HashMap::new(),
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

    /// Add `entry` to the listing of the directory `path`, once, keeping
    /// what is already listed there; the listing is created when there is
    /// none.
    pub fn with_dir_entry(mut self, path: &str, entry: &str) -> Self {
        let entries = self.dirs.entry(path.to_string()).or_default();
        if !entries.iter().any(|e| e == entry) {
            entries.push(entry.to_string());
        }
        self
    }

    pub fn with_link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self
    }

    /// Register a symlink whose target does not exist. `read_link`
    /// returns `target` and `resolve_final_target` fails with `NotFound`,
    /// as `canonicalize` does on a real host. A plain [`with_link`] to an
    /// unregistered path resolves, because the mock cannot tell a missing
    /// target from one a test did not bother to register.
    ///
    /// [`with_link`]: MockExecutor::with_link
    pub fn with_dangling_link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self.dangling_links.insert(path.to_string());
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

    /// Register a directory whose listing yields its registered entries
    /// (see [`with_dir`]) and then fails with `error_kind`, as a real
    /// directory iterator can after a successful open.
    ///
    /// [`with_dir`]: MockExecutor::with_dir
    pub fn with_dir_iteration_error(mut self, path: &str, error_kind: io::ErrorKind) -> Self {
        self.dir_iteration_errors
            .insert(path.to_string(), error_kind);
        self
    }

    /// Register a path whose `read_link` fails with `error_kind`, as an
    /// I/O or permission failure does on a real host. Takes priority over
    /// a registered link and over the not-a-link answer.
    pub fn with_link_error(mut self, path: &str, error_kind: io::ErrorKind) -> Self {
        self.link_errors.insert(path.to_string(), error_kind);
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

    fn read_dir_strict(&self, path: &Path) -> io::Result<Vec<String>> {
        let entries = self.read_dir(path)?;
        match self.dir_iteration_errors.get(path.to_str().unwrap_or("")) {
            Some(&error_kind) => Err(io::Error::new(error_kind, path.display().to_string())),
            None => Ok(entries),
        }
    }

    /// As `readlink(2)`: every component but the last is resolved through
    /// registered links, then the last is read. A registered path that is
    /// not a link fails with `InvalidInput` (`EINVAL`), an unregistered
    /// one with `NotFound`.
    fn read_link(&self, path: &Path) -> io::Result<String> {
        if let Some(&error_kind) = self.link_errors.get(path.to_str().unwrap_or("")) {
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let physical = self.resolve(path, false)?;
        let key = physical.to_str().unwrap_or("");
        if let Some(target) = self.links.get(key) {
            return Ok(target.clone());
        }
        let kind = if self.is_registered(key) {
            io::ErrorKind::InvalidInput
        } else {
            io::ErrorKind::NotFound
        };
        Err(io::Error::new(kind, path.display().to_string()))
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }

    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf> {
        self.resolve(path, true)
    }
}

impl MockExecutor {
    /// Resolve `path` through registered links, an ancestor directory's
    /// as well as the path's own, as `canonicalize` does; the last
    /// component is followed only when `follow_last` is set. A link
    /// registered with [`with_dangling_link`] fails with `NotFound`; more
    /// than [`MAX_SYMLINK_HOPS`] links, or a path seen twice, is a loop.
    ///
    /// [`with_dangling_link`]: MockExecutor::with_dangling_link
    fn resolve(&self, path: &Path, follow_last: bool) -> io::Result<PathBuf> {
        let mut current = normalize_mock_path(path);
        let mut visited = HashSet::new();
        'resolve: loop {
            if !visited.insert(current.clone()) || visited.len() > MAX_SYMLINK_HOPS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("symlink loop detected at {}", current.display()),
                ));
            }
            let components: Vec<Component> = current.components().collect();
            let mut prefix = PathBuf::new();
            for (i, component) in components.iter().enumerate() {
                prefix.push(component);
                if i + 1 == components.len() && !follow_last {
                    break;
                }
                let key = prefix.to_str().unwrap_or("");
                if self.dangling_links.contains(key) {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("dangling symlink at {}", prefix.display()),
                    ));
                }
                if let Some(target) = self.links.get(key) {
                    // An absolute target replaces the parent in `join`.
                    let mut next = prefix.parent().unwrap_or(Path::new("/")).join(target);
                    next.extend(&components[i + 1..]);
                    current = normalize_mock_path(&next);
                    continue 'resolve;
                }
            }
            return Ok(current);
        }
    }

    /// Whether `path` is registered as anything: a file, a directory, a
    /// link, or an injected read error on one.
    fn is_registered(&self, path: &str) -> bool {
        self.files.contains_key(path)
            || self.file_errors.contains_key(path)
            || self.dirs.contains_key(path)
            || self.dir_errors.contains_key(path)
            || self.links.contains_key(path)
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
    fn test_mock_read_dir_iteration_error() {
        let mock = MockExecutor::new()
            .with_dir("/run/sysctl.d", vec!["50-early.conf"])
            .with_dir_iteration_error("/run/sysctl.d", io::ErrorKind::Other);
        let dir = Path::new("/run/sysctl.d");
        assert_eq!(mock.read_dir(dir).unwrap(), ["50-early.conf"]);
        assert_eq!(
            mock.read_dir_strict(dir).unwrap_err().kind(),
            io::ErrorKind::Other
        );
        assert_eq!(
            mock.read_dir_strict(Path::new("/missing"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
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
    fn test_mock_resolve_final_target_dangling() {
        let mock = MockExecutor::new()
            .with_link("/etc/sysctl.d/a.conf", "/etc/sysctl.d/b.conf")
            .with_dangling_link("/etc/sysctl.d/b.conf", "/etc/sysctl.d/gone.conf");
        for path in ["/etc/sysctl.d/a.conf", "/etc/sysctl.d/b.conf"] {
            let err = mock.resolve_final_target(Path::new(path)).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{path}");
        }
        assert_eq!(
            mock.read_link(Path::new("/etc/sysctl.d/b.conf")).unwrap(),
            "/etc/sysctl.d/gone.conf"
        );
    }

    #[test]
    fn test_mock_resolve_final_target_through_ancestor_link() {
        let mock = MockExecutor::new()
            .with_link("/etc/sysctl.d", "/var/lib/app")
            .with_link("/var/lib/app/b.conf", "../shared/b.conf")
            .with_file("/var/lib/app/a.conf", "");
        let cases = [
            ("/etc/sysctl.d/a.conf", "/var/lib/app/a.conf"),
            ("/etc/sysctl.d/b.conf", "/var/lib/shared/b.conf"),
            ("/etc/sysctl.d", "/var/lib/app"),
        ];
        for (path, expected) in cases {
            let resolved = mock.resolve_final_target(Path::new(path)).unwrap();
            assert_eq!(resolved, PathBuf::from(expected), "{path}");
        }
    }

    #[test]
    fn test_mock_read_link_kinds() {
        let mock = MockExecutor::new()
            .with_link("/etc/sysctl.d", "/var/lib/app")
            .with_link("/var/lib/app/b.conf", "/etc/b.conf")
            .with_file("/var/lib/app/a.conf", "")
            .with_file("/etc/c.conf", "")
            .with_link_error("/etc/d.conf", io::ErrorKind::PermissionDenied);
        // Every component but the last resolves, as readlink(2) does.
        assert_eq!(
            mock.read_link(Path::new("/etc/sysctl.d/b.conf")).unwrap(),
            "/etc/b.conf"
        );
        assert_eq!(
            mock.read_link(Path::new("/etc/sysctl.d")).unwrap(),
            "/var/lib/app"
        );
        let cases = [
            ("/etc/sysctl.d/a.conf", io::ErrorKind::InvalidInput),
            ("/etc/c.conf", io::ErrorKind::InvalidInput),
            ("/etc/missing.conf", io::ErrorKind::NotFound),
            ("/etc/d.conf", io::ErrorKind::PermissionDenied),
        ];
        for (path, kind) in cases {
            let err = mock.read_link(Path::new(path)).unwrap_err();
            assert_eq!(err.kind(), kind, "{path}");
        }
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
