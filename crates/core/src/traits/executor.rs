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

/// What a path is, read without following a final symlink and without
/// opening it ([`Executor::file_kind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A regular file: the only kind whose content a collector reads.
    Regular,
    Directory,
    Symlink,
    /// A FIFO, socket, or block or character device. Opening a FIFO with
    /// no writer blocks, so a collector never opens one of these.
    Special,
}

/// What `stat` says of the file a path resolves to, read without opening
/// it ([`Executor::file_metadata`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMetadata {
    /// `st_mode`: the file type bits and the permission bits.
    pub mode: u32,
    pub size: u64,
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
}

/// The file type bits of `st_mode`.
const S_IFMT: u32 = 0o170_000;
/// The file type of a regular file in `st_mode`.
const S_IFREG: u32 = 0o100_000;
/// The file type of a directory in `st_mode`.
const S_IFDIR: u32 = 0o040_000;
/// The permission bits of `st_mode`, setuid, setgid and sticky included.
const PERMISSION_BITS: u32 = 0o7777;

impl FileMetadata {
    /// Whether the path resolves to a regular file.
    pub fn is_regular(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    /// Whether the path resolves to a directory.
    pub fn is_directory(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    /// The permission bits alone, as `chmod` takes them.
    pub fn permissions(&self) -> u32 {
        self.mode & PERMISSION_BITS
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

    /// Every entry name in `path`, or an error when the directory cannot
    /// be opened or any entry cannot be read. Unlike [`read_dir`], which
    /// returns the entries it could read, this never returns a partial
    /// listing: for a caller whose result is wrong when one entry is
    /// missing.
    ///
    /// [`read_dir`]: Executor::read_dir
    fn read_dir_strict(&self, path: &Path) -> io::Result<Vec<String>>;
    fn read_link(&self, path: &Path) -> io::Result<String>;

    /// What `path` is, from `lstat`: a final symlink is reported as one,
    /// not followed, and nothing is opened. A collector that is about to
    /// read a file it did not list itself calls this first and reads only
    /// a [`FileKind::Regular`].
    fn file_kind(&self, path: &Path) -> io::Result<FileKind>;
    fn host_root(&self) -> &Path;

    /// Resolve the final target of a path by following the entire symlink chain.
    ///
    /// For real executors this uses `std::fs::canonicalize()`. Mock executors
    /// walk their `links` map iteratively with cycle detection.
    ///
    /// Returns the fully-resolved absolute path, or an error if the chain is
    /// broken (dangling symlink), loops, or hits a permission wall.
    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf>;

    /// `stat`, following symlinks: type and permission bits, size, device,
    /// inode and link count of the file the path resolves to, without
    /// opening it.
    fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata>;
}

/// A host executor the machine-state read gate can wrap
/// ([`crate::machine_state::ScreenedExecutor`]). The real and mock
/// executors implement it; the gate does not, so no inspector holds a
/// value it can call these on. The gate's own reads are the only partial
/// and raw reads a scan makes: the two-byte keytab sniff, and the
/// unmanaged payload check
/// ([`crate::machine_state::ScreenedExecutor::screen_payload`]), which
/// reads a file to its end a buffer at a time.
pub trait HostRead: Executor {
    /// At most `n` bytes from the start of the file.
    fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>>;

    /// The file, open for reading from its start. The caller reads it a
    /// buffer at a time, so a file of any length costs bounded memory.
    fn open_read(&self, path: &Path) -> io::Result<Box<dyn io::Read + '_>>;
}
