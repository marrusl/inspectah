use inspectah_core::traits::executor::{ExecResult, Executor, FileKind, FileMetadata, HostRead};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// The most links one resolution follows before it fails as a loop, the
/// kernel's `ELOOP` limit.
const MAX_SYMLINK_HOPS: usize = 40;

/// The mode a registered file reports when no test set one: a regular
/// file, `rw-r--r--`.
const DEFAULT_FILE_MODE: u32 = 0o100_644;

/// The mode a registered directory reports.
const DIRECTORY_MODE: u32 = 0o040_755;

/// The one device every mock path is on.
const MOCK_DEV: u64 = 1;

/// Which read a [`ReadRecord`] logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadKind {
    /// `read_file`.
    Full,
    /// `read_head` ([`HostRead`]).
    Head,
    /// `open_read` ([`HostRead`]); the record's `bytes` is the file's
    /// length, all of which the reader yields.
    Stream,
}

/// One read of file content, as the path was asked for, with the number
/// of bytes it returned (0 for a failed read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRecord {
    pub path: String,
    pub kind: ReadKind,
    pub bytes: usize,
}

pub struct MockExecutor {
    commands: HashMap<String, ExecResult>,
    /// File content as bytes: `read_file` on content that is not UTF-8
    /// fails with `InvalidData`, as `std::fs::read_to_string` does.
    files: HashMap<String, Vec<u8>>,
    /// Hard links: each path here names the same file as the path it maps
    /// to, so it reads that file's content and shares its inode.
    hard_links: HashMap<String, String>,
    /// Permission and type bits a test set for a file.
    modes: HashMap<String, u32>,
    /// Every `read_file`, `read_head` and `open_read` call, in order.
    read_log: Mutex<Vec<ReadRecord>>,
    dirs: HashMap<String, Vec<String>>,
    /// Entry names that are not UTF-8, by directory, listed after the
    /// directory's registered entries: `read_dir_raw` returns their bytes,
    /// `read_dir` and `read_dir_strict` their lossy decoding, as the real
    /// executor does.
    raw_dir_entries: HashMap<String, Vec<std::ffi::OsString>>,
    links: HashMap<String, String>,
    /// Paths that are a FIFO, socket or device ([`FileKind::Special`]).
    /// Reading one panics: a collector must never open such a path.
    specials: HashSet<String>,
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
            hard_links: HashMap::new(),
            modes: HashMap::new(),
            read_log: Mutex::new(Vec::new()),
            dirs: HashMap::new(),
            raw_dir_entries: HashMap::new(),
            links: HashMap::new(),
            specials: HashSet::new(),
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
        self.files
            .insert(path.to_string(), content.as_bytes().to_vec());
        self
    }

    /// Register a file holding `content`, which need not be UTF-8.
    pub fn with_file_bytes(mut self, path: &str, content: &[u8]) -> Self {
        self.files.insert(path.to_string(), content.to_vec());
        self
    }

    /// Register `path` as a hard link of the registered file `target`:
    /// the two names share one inode and one content.
    pub fn with_hard_link(mut self, path: &str, target: &str) -> Self {
        self.hard_links.insert(path.to_string(), target.to_string());
        self
    }

    /// Set the type and permission bits `file_metadata` reports for a
    /// registered file (`0o100600` for a regular `rw-------` file).
    pub fn with_file_mode(mut self, path: &str, mode: u32) -> Self {
        self.modes.insert(path.to_string(), mode);
        self
    }

    /// Every `read_file`, `read_head` and `open_read` call so far, in order.
    pub fn read_log(&self) -> Vec<ReadRecord> {
        self.read_log.lock().unwrap().clone()
    }

    /// Whether any read, full or head, named `path`.
    pub fn was_read(&self, path: &str) -> bool {
        self.read_log.lock().unwrap().iter().any(|r| r.path == path)
    }

    /// The registered content of the file `path` names: by its own name,
    /// else through registered symlinks (an ancestor's as well), then
    /// through a hard link.
    fn content_of(&self, path: &Path) -> Option<(String, &Vec<u8>)> {
        let key = path.to_str().unwrap_or("");
        let physical = if self.files.contains_key(key) || self.hard_links.contains_key(key) {
            key.to_string()
        } else {
            self.resolve(path, true).ok()?.to_str()?.to_string()
        };
        let canonical = self.hard_links.get(&physical).cloned().unwrap_or(physical);
        self.files.get(&canonical).map(|c| (canonical, c))
    }

    fn log_read(&self, path: &Path, kind: ReadKind, bytes: usize) {
        self.read_log.lock().unwrap().push(ReadRecord {
            path: path.display().to_string(),
            kind,
            bytes,
        });
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

    /// Add an entry whose name is the raw bytes `name`, which need not be
    /// UTF-8, to the listing of the registered directory `path`.
    pub fn with_raw_dir_entry(mut self, path: &str, name: &[u8]) -> Self {
        use std::os::unix::ffi::OsStrExt;
        self.raw_dir_entries
            .entry(path.to_string())
            .or_default()
            .push(std::ffi::OsStr::from_bytes(name).to_os_string());
        self
    }

    pub fn with_link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self
    }

    /// Register a FIFO, socket or device at `path`. `file_kind` reports
    /// it as [`FileKind::Special`], `file_exists` as present, and
    /// `read_file` on it panics, so a test proves a collector never opens
    /// one: opening a FIFO with no writer blocks on a real host.
    pub fn with_special_file(mut self, path: &str) -> Self {
        self.specials.insert(path.to_string());
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
    /// a registered link and over the not-a-link answer. A resolution that
    /// passes through the path fails with the same kind, as `canonicalize`
    /// does when it cannot read a link on the way.
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
        assert!(
            !self.specials.contains(key),
            "read_file opened the special file {key}, which can block on a real host"
        );
        // Explicit error injection takes priority over registered files.
        if let Some(&error_kind) = self.file_errors.get(key) {
            self.log_read(path, ReadKind::Full, 0);
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let Some((_, bytes)) = self.content_of(path) else {
            self.log_read(path, ReadKind::Full, 0);
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ));
        };
        self.log_read(path, ReadKind::Full, bytes.len());
        String::from_utf8(bytes.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )
        })
    }

    /// A registered file or directory exists, as `Path::exists` says for
    /// both on a real host.
    fn file_exists(&self, path: &Path) -> bool {
        let key = path.to_str().unwrap_or("");
        self.files.contains_key(key)
            || self.hard_links.contains_key(key)
            || self.dirs.contains_key(key)
            || self.specials.contains(key)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let key = path.to_str().unwrap_or("");
        // Explicit error injection takes priority over registered dirs.
        if let Some(&error_kind) = self.dir_errors.get(key) {
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let mut entries =
            self.dirs.get(key).cloned().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, path.display().to_string())
            })?;
        entries.extend(
            self.raw_dir_entries
                .get(key)
                .into_iter()
                .flatten()
                .map(|name| name.to_string_lossy().into_owned()),
        );
        Ok(entries)
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

    /// As `lstat`: every component but the last is resolved through
    /// registered links, then the last is classified. A link wins over a
    /// file registered at the same path, as the link is what is there.
    fn file_kind(&self, path: &Path) -> io::Result<FileKind> {
        let physical = self.resolve(path, false)?;
        let key = physical.to_str().unwrap_or("");
        if self.links.contains_key(key) {
            Ok(FileKind::Symlink)
        } else if self.specials.contains(key) {
            Ok(FileKind::Special)
        } else if self.dirs.contains_key(key)
            // Listing fails with `NotADirectory` only on what is not one.
            || self
                .dir_errors
                .get(key)
                .is_some_and(|kind| *kind != io::ErrorKind::NotADirectory)
        {
            Ok(FileKind::Directory)
        } else if self.files.contains_key(key)
            || self.hard_links.contains_key(key)
            || self.file_errors.contains_key(key)
        {
            Ok(FileKind::Regular)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ))
        }
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }

    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf> {
        self.resolve(path, true)
    }

    /// As `stat`: links are followed, a hard link shares its target's
    /// inode, and each other registered path has its own. Only a
    /// registered file, error-injected file or directory has metadata.
    fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        let physical = self.resolve(path, true)?;
        let key = physical.to_str().unwrap_or("").to_string();
        if let Some(&error_kind) = self.file_errors.get(&key)
            && error_kind != io::ErrorKind::PermissionDenied
        {
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let canonical = self.hard_links.get(&key).cloned().unwrap_or(key);
        let (mode, size) = if let Some(content) = self.files.get(&canonical) {
            let mode = self.modes.get(&canonical).copied();
            (mode.unwrap_or(DEFAULT_FILE_MODE), content.len() as u64)
        } else if self.file_errors.contains_key(&canonical) {
            let mode = self.modes.get(&canonical).copied();
            (mode.unwrap_or(DEFAULT_FILE_MODE), 0)
        } else if self.dirs.contains_key(&canonical) {
            (DIRECTORY_MODE, 0)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ));
        };
        let nlink = 1 + self
            .hard_links
            .values()
            .filter(|t| **t == canonical)
            .count() as u64;
        Ok(FileMetadata {
            mode,
            size,
            dev: MOCK_DEV,
            ino: mock_inode(&canonical),
            nlink,
        })
    }
}

impl HostRead for MockExecutor {
    fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>> {
        let key = path.to_str().unwrap_or("");
        assert!(
            !self.specials.contains(key),
            "read_head opened the special file {key}, which can block on a real host"
        );
        if let Some(&error_kind) = self.file_errors.get(key) {
            self.log_read(path, ReadKind::Head, 0);
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let Some((_, bytes)) = self.content_of(path) else {
            self.log_read(path, ReadKind::Head, 0);
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ));
        };
        let head = bytes[..n.min(bytes.len())].to_vec();
        self.log_read(path, ReadKind::Head, head.len());
        Ok(head)
    }

    fn read_dir_raw(&self, path: &Path) -> io::Result<Vec<std::ffi::OsString>> {
        let key = path.to_str().unwrap_or("");
        let raw = self.raw_dir_entries.get(key);
        let decoded = self.read_dir_strict(path)?;
        // The lossy decodings of the raw entries end the strict listing.
        let registered = decoded.len() - raw.map_or(0, Vec::len);
        Ok(decoded
            .into_iter()
            .take(registered)
            .map(std::ffi::OsString::from)
            .chain(raw.into_iter().flatten().cloned())
            .collect())
    }

    fn open_read(&self, path: &Path) -> io::Result<Box<dyn io::Read + '_>> {
        let key = path.to_str().unwrap_or("");
        assert!(
            !self.specials.contains(key),
            "open_read opened the special file {key}, which can block on a real host"
        );
        if let Some(&error_kind) = self.file_errors.get(key) {
            self.log_read(path, ReadKind::Stream, 0);
            return Err(io::Error::new(error_kind, path.display().to_string()));
        }
        let Some((_, bytes)) = self.content_of(path) else {
            self.log_read(path, ReadKind::Stream, 0);
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                path.display().to_string(),
            ));
        };
        self.log_read(path, ReadKind::Stream, bytes.len());
        Ok(Box::new(io::Cursor::new(bytes.clone())))
    }
}

/// A stable inode number for a canonical mock path.
fn mock_inode(path: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

impl MockExecutor {
    /// Resolve `path` through registered links, an ancestor directory's
    /// as well as the path's own, as `canonicalize` does; the last
    /// component is followed only when `follow_last` is set. A link
    /// registered with [`with_dangling_link`] fails with `NotFound`, one
    /// registered with [`with_link_error`] with its kind; more than
    /// [`MAX_SYMLINK_HOPS`] links, or a path seen twice, is a loop, which
    /// fails with `ELOOP` as `canonicalize` does.
    ///
    /// [`with_dangling_link`]: MockExecutor::with_dangling_link
    /// [`with_link_error`]: MockExecutor::with_link_error
    fn resolve(&self, path: &Path, follow_last: bool) -> io::Result<PathBuf> {
        let mut current = normalize_mock_path(path);
        let mut visited = HashSet::new();
        'resolve: loop {
            if !visited.insert(current.clone()) || visited.len() > MAX_SYMLINK_HOPS {
                return Err(io::Error::from_raw_os_error(libc::ELOOP));
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
                if let Some(&error_kind) = self.link_errors.get(key) {
                    return Err(io::Error::new(error_kind, prefix.display().to_string()));
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
            || self.hard_links.contains_key(path)
            || self.specials.contains(path)
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
        assert_eq!(err.raw_os_error(), Some(libc::ELOOP));
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
