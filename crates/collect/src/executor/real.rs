// The real executor is the one place collect may open files and run
// commands directly: every other module reaches the host through it, and
// inspectors only through the machine-state read gate around it
// (`crates/collect/clippy.toml`). Its own tests build it.
#![allow(clippy::disallowed_methods)]

use inspectah_core::traits::executor::{ExecResult, Executor, FileKind, FileMetadata, HostRead};
use std::io;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
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
///
/// The private field keeps it from being built by a struct literal, and
/// there is no `Default`: [`RealExecutor::new`] is the only constructor,
/// and `crates/collect/clippy.toml` refuses it in this crate, so no
/// collector builds an executor that skips the machine-state read gate.
/// `run_scan` builds the only production one and wraps it at once.
pub struct RealExecutor {
    _private: (),
}

impl RealExecutor {
    #[allow(clippy::new_without_default)] // no Default on purpose: see the type's doc
    pub fn new() -> Self {
        Self { _private: () }
    }
}

/// Resolve a command name to an absolute path by probing standard
/// system directories. Falls back to the bare command name for
/// macOS test environments or non-standard locations.
fn resolve_command(cmd: &str) -> std::path::PathBuf {
    // Skip resolution for commands that are already absolute paths
    if cmd.starts_with('/') {
        return std::path::PathBuf::from(cmd);
    }
    for prefix in &["/usr/bin", "/usr/sbin"] {
        let path = Path::new(prefix).join(cmd);
        if path.exists() {
            return path;
        }
    }
    // Fallback to bare command (needed for macOS test environment)
    std::path::PathBuf::from(cmd)
}

impl Executor for RealExecutor {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        // Resolve command against /usr/bin, /usr/sbin before falling
        // back to bare name. This ensures deterministic resolution
        // regardless of the ambient PATH.
        let resolved = resolve_command(cmd);
        // Fixed argv via Command::new + args — never shell strings.
        // LC_ALL=C ensures deterministic, locale-independent output.
        let child = Command::new(&resolved)
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

        // Take stdout/stderr handles BEFORE waiting so we can drain
        // them concurrently with the child process. If we wait first,
        // a command that fills the OS pipe buffer blocks on write,
        // never exits, and we get a false timeout.
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        std::thread::scope(|s| {
            // Drain stdout in a scoped thread (bounded by STDOUT_SIZE_CAP).
            let stdout_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(r) = stdout_handle {
                    let mut limited = io::Read::take(r, STDOUT_SIZE_CAP as u64 + 1);
                    let _ = io::Read::read_to_end(&mut limited, &mut buf);
                }
                buf
            });

            // Drain stderr in a scoped thread (unbounded — stderr is small).
            let stderr_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(mut r) = stderr_handle {
                    let _ = io::Read::read_to_end(&mut r, &mut buf);
                }
                buf
            });

            // Wait for child with timeout on the main thread.
            match child.wait_timeout(COMMAND_TIMEOUT) {
                Ok(Some(status)) => {
                    let stdout_raw = stdout_thread.join().unwrap();
                    let stderr_raw = stderr_thread.join().unwrap();

                    let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                        let s =
                            String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned();
                        format!("{s}\n[output truncated at 64 MB]")
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
                    // Timeout — kill the child, then join drain threads.
                    let _ = child.kill();
                    let _ = child.wait(); // reap
                    // Join threads so they don't leak (scoped threads
                    // require all spawned threads to finish before the
                    // scope exits).
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
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
                Err(e) => {
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    ExecResult {
                        stderr: format!("failed to wait on child process: {e}"),
                        exit_code: -1,
                        ..Default::default()
                    }
                }
            }
        })
    }

    fn run_passthrough_stderr(&self, cmd: &str, args: &[&str]) -> ExecResult {
        let resolved = resolve_command(cmd);
        let child = Command::new(&resolved)
            .args(args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
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

        let stdout_handle = child.stdout.take();

        std::thread::scope(|s| {
            let stdout_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(r) = stdout_handle {
                    let mut limited = io::Read::take(r, STDOUT_SIZE_CAP as u64 + 1);
                    let _ = io::Read::read_to_end(&mut limited, &mut buf);
                }
                buf
            });

            // No stderr thread — stderr is inherited by the child process.
            // Use a longer timeout for image pulls (10 minutes).
            let pull_timeout = Duration::from_secs(600);
            match child.wait_timeout(pull_timeout) {
                Ok(Some(status)) => {
                    let stdout_raw = stdout_thread.join().unwrap();

                    let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                        let s =
                            String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned();
                        format!("{s}\n[output truncated at 64 MB]")
                    } else {
                        String::from_utf8_lossy(&stdout_raw).into_owned()
                    };

                    ExecResult {
                        stdout,
                        stderr: String::new(),
                        exit_code: status.code().unwrap_or(-1),
                    }
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    ExecResult {
                        stderr: format!(
                            "command timed out after {}s: {} {}",
                            pull_timeout.as_secs(),
                            cmd,
                            args.join(" ")
                        ),
                        exit_code: -1,
                        ..Default::default()
                    }
                }
                Err(e) => {
                    let _ = stdout_thread.join();
                    ExecResult {
                        stderr: format!("failed to wait on child process: {e}"),
                        exit_code: -1,
                        ..Default::default()
                    }
                }
            }
        })
    }

    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        let resolved = resolve_command(cmd);
        let child = Command::new(&resolved)
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

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Architecture: main thread reads stderr line-by-line and calls
        // the callback (no Send required). A watchdog thread handles
        // the 600s timeout/kill.
        let pull_timeout = Duration::from_secs(600);

        std::thread::scope(|s| {
            // Drain stdout in a scoped thread.
            let stdout_thread = s.spawn(|| {
                let mut buf = Vec::new();
                if let Some(r) = stdout_handle {
                    let mut limited = io::Read::take(r, STDOUT_SIZE_CAP as u64 + 1);
                    let _ = io::Read::read_to_end(&mut limited, &mut buf);
                }
                buf
            });

            // Watchdog thread: waits for timeout, then kills the child.
            // Uses an Arc<AtomicBool> to coordinate with the main thread.
            let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let finished_clone = finished.clone();
            let child_id = child.id();
            let watchdog = s.spawn(move || {
                let start = std::time::Instant::now();
                while start.elapsed() < pull_timeout {
                    if finished_clone.load(std::sync::atomic::Ordering::Relaxed) {
                        return false; // main thread finished normally
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                // Timeout — kill via signal (child.kill() requires &mut,
                // so we use the raw PID kill).
                unsafe {
                    libc::kill(child_id as i32, libc::SIGKILL);
                }
                true // timed out
            });

            // Main thread: read stderr line-by-line, call callback live.
            let mut stderr_lines = Vec::new();
            if let Some(r) = stderr_handle {
                use std::io::BufRead;
                let reader = io::BufReader::new(r);
                for line in reader.lines() {
                    match line {
                        Ok(l) => {
                            on_stderr_line(&l);
                            stderr_lines.push(l);
                        }
                        Err(_) => break,
                    }
                }
            }

            // Signal watchdog that we're done reading stderr.
            finished.store(true, std::sync::atomic::Ordering::Relaxed);
            let timed_out = watchdog.join().unwrap();

            // Wait for child to exit and collect status.
            let status = child.wait();
            let stdout_raw = stdout_thread.join().unwrap();

            let stdout = if stdout_raw.len() > STDOUT_SIZE_CAP {
                let s = String::from_utf8_lossy(&stdout_raw[..STDOUT_SIZE_CAP]).into_owned();
                format!("{s}\n[output truncated at 64 MB]")
            } else {
                String::from_utf8_lossy(&stdout_raw).into_owned()
            };

            if timed_out {
                let mut full_stderr = stderr_lines.join("\n");
                if !full_stderr.is_empty() {
                    full_stderr.push('\n');
                }
                full_stderr.push_str(&format!(
                    "command timed out after {}s: {} {}",
                    pull_timeout.as_secs(),
                    cmd,
                    args.join(" ")
                ));
                ExecResult {
                    stdout,
                    stderr: full_stderr,
                    exit_code: -1,
                }
            } else {
                let exit_code = match status {
                    Ok(s) => s.code().unwrap_or(-1),
                    Err(_) => -1,
                };
                ExecResult {
                    stdout,
                    stderr: stderr_lines.join("\n"),
                    exit_code,
                }
            }
        })
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
    /// skipped — the caller gets a partial list.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        let entries = std::fs::read_dir(path)?;
        entries
            .filter_map(|e| e.ok())
            .map(|e| Ok(e.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn read_dir_strict(&self, path: &Path) -> io::Result<Vec<String>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect()
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        let target = std::fs::read_link(path)?;
        Ok(target.to_string_lossy().into_owned())
    }

    fn file_kind(&self, path: &Path) -> io::Result<FileKind> {
        let file_type = std::fs::symlink_metadata(path)?.file_type();
        Ok(if file_type.is_symlink() {
            FileKind::Symlink
        } else if file_type.is_dir() {
            FileKind::Directory
        } else if file_type.is_file() {
            FileKind::Regular
        } else {
            FileKind::Special
        })
    }

    fn host_root(&self) -> &Path {
        Path::new("/")
    }

    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        let meta = std::fs::metadata(path)?;
        Ok(FileMetadata {
            mode: meta.mode(),
            size: meta.size(),
            dev: meta.dev(),
            ino: meta.ino(),
            nlink: meta.nlink(),
        })
    }
}

impl HostRead for RealExecutor {
    fn read_head(&self, path: &Path, n: usize) -> io::Result<Vec<u8>> {
        let mut head = Vec::with_capacity(n);
        std::fs::File::open(path)?
            .take(n as u64)
            .read_to_end(&mut head)?;
        Ok(head)
    }

    fn read_dir_raw(&self, path: &Path) -> io::Result<Vec<std::ffi::OsString>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect()
    }

    fn open_read(&self, path: &Path) -> io::Result<Box<dyn io::Read + '_>> {
        Ok(Box::new(std::fs::File::open(path)?))
    }

    fn mount_id(&self, path: &Path) -> Option<u64> {
        mount_id_of(path)
    }
}

/// The mount id the kernel reports for `path`: the `mnt_id` line of
/// `/proc/self/fdinfo` for an `O_PATH` handle to it, which opens nothing
/// for reading and needs no read permission. The kernel has written that
/// line since Linux 3.15, so every supported RHEL has it, and reading it
/// needs no libc wrapper: `statx` with `STATX_MNT_ID`, the other source,
/// needs Linux 5.8 and has no binding in the libc crate for the static
/// musl build.
#[cfg(target_os = "linux")]
fn mount_id_of(path: &Path) -> Option<u64> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH)
        .open(path)
        .ok()?;
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", handle.as_raw_fd())).ok()?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
}

/// No mount ids off Linux.
#[cfg(not(target_os = "linux"))]
fn mount_id_of(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::traits::executor::Executor;

    /// A directory and its parent on one mount share a mount id; `/proc`
    /// is a mount of its own; a path that does not exist has none.
    #[cfg(target_os = "linux")]
    #[test]
    fn mount_ids_come_from_the_kernel() {
        let exec = RealExecutor::new();
        let tmp = tempfile::tempdir().expect("a temp directory");
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let dir = exec.mount_id(tmp.path()).expect("a mount id");
        assert_eq!(exec.mount_id(&tmp.path().join("sub")), Some(dir));
        let proc = exec.mount_id(Path::new("/proc")).expect("a mount id");
        assert_ne!(exec.mount_id(Path::new("/")), Some(proc));
        assert_eq!(exec.mount_id(&tmp.path().join("missing")), None);
    }

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
    fn test_real_executor_read_dir_strict() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.conf"), "").unwrap();
        let exec = RealExecutor::new();
        assert_eq!(exec.read_dir_strict(dir.path()).unwrap(), ["a.conf"]);
        let missing = exec
            .read_dir_strict(&dir.path().join("missing"))
            .unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
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

    /// A command that produces more output than a single pipe buffer
    /// (typically 64 KB on macOS/Linux) must complete without timeout.
    /// Before the streamed-drain fix, the child would block on write,
    /// never exit, and RealExecutor would report a false timeout.
    #[test]
    fn test_large_stdout_completes_without_timeout() {
        let exec = RealExecutor::new();
        // Generate 128 KB of output — well above the pipe buffer size.
        let result = exec.run("dd", &["if=/dev/zero", "bs=1024", "count=128"]);
        assert_eq!(
            result.exit_code, 0,
            "large-output command must complete successfully, stderr: {}",
            result.stderr
        );
        assert!(
            result.stdout.len() >= 128 * 1024,
            "stdout must contain at least 128 KB, got {} bytes",
            result.stdout.len()
        );
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

    /// `file_kind` reads each kind with `lstat` and opens nothing: a
    /// socket and a device are `Special`, and a link is a `Symlink`
    /// whatever it points at.
    #[test]
    fn test_real_executor_file_kind() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("file");
        std::fs::write(&file, "content").expect("write file");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&file, &link).expect("make link");
        let socket = dir.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind socket");

        let exec = RealExecutor::new();
        for (path, kind) in [
            (file.as_path(), FileKind::Regular),
            (dir.path(), FileKind::Directory),
            (link.as_path(), FileKind::Symlink),
            (socket.as_path(), FileKind::Special),
            (Path::new("/dev/null"), FileKind::Special),
        ] {
            assert_eq!(exec.file_kind(path).unwrap(), kind, "{}", path.display());
        }
        assert_eq!(
            exec.file_kind(&dir.path().join("missing"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
}
