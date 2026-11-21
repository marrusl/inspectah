//! Collector-driven sysctl hosts and the scan path they run through.
//!
//! A [`SysctlHost`] describes a host's files, links, `rpm` answers and
//! `sysctl -a` output; [`scan`] runs the real inspectors over it through
//! the scan command's own calls (`collect()`, `validate`, `redact`), and
//! the remaining functions carry the snapshot to every later surface: a
//! JSON reload, the rendered export, a refine session loaded from the
//! scan tarball, and that session's export.
//!
//! Shared by `crates/cli/tests` and, through `#[path]`, by
//! `crates/web/tests`, so it imports only crates both test targets have:
//! `inspectah-core`, `inspectah-collect`, `inspectah-pipeline`,
//! `inspectah-refine`, `tempfile` and `serde_json`.

// Each test target that includes this module uses a different subset of it.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::kernelboot::KernelbootInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::selinux::SelinuxInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::{ExecResult, Executor};
use inspectah_core::traits::inspector::{
    InspectionContext, Inspector, InspectorError, InspectorOutput,
};
use inspectah_core::traits::progress::{NullProgress, ProgressSink};
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::{InspectorId, SourceSystemKind};
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::build::extract::TarballExtractor;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::tarball::create_tarball;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;

/// The `rpm -qa` query the RPM inspector lists packages with.
const RPM_QA: &str = "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n";
/// The `rpm -qa` query the RPM inspector lists owned paths with: a
/// `@@<package>` line, then one owned path per line.
const RPM_FILE_OWNERSHIP: &str = "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]";
/// The `rpm -qa` query the config inspector lists `%ghost` files with:
/// package, `fflags`, shell-quoted path, one file per record.
const RPM_FILE_FLAGS: &str =
    "rpm -qa --queryformat [%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES:shescape}\\n]";
const CMDLINE: &str = "BOOT_IMAGE=/vmlinuz root=/dev/mapper/rhel-root ro quiet\n";
const LSMOD: &str = "Module                  Size  Used by\nbridge                307200  0\n";
/// The path a `/dev/null` mask resolves to, and what reading it returns.
const DEV_NULL: &str = "/dev/null";
/// The hostname the scan tarball is packed under.
const TARBALL_PREFIX: &str = "sysctl-host";

/// A host for the scan path, built over [`MockExecutor`].
///
/// Every registered path is listed in its parent directory, up to a
/// top-level directory, so the `/etc` walk and the sysctl reader find it.
///
/// `MockExecutor::read_file`, `read_dir` and `file_exists` do not follow
/// links, while the real executor's do. [`build`](SysctlHost::build)
/// therefore registers, at each link's own path, what a real read there
/// returns: the final target's content, or `""` for `/dev/null`; for a
/// link to a directory, that directory's listing and the content of each
/// file directly in it. No test registers a link by hand.
#[derive(Debug, Default)]
pub struct SysctlHost {
    files: BTreeMap<String, String>,
    links: BTreeMap<String, String>,
    dangling_links: BTreeMap<String, String>,
    dirs: BTreeMap<String, BTreeSet<String>>,
    dir_errors: BTreeMap<String, std::io::ErrorKind>,
    link_errors: BTreeMap<String, std::io::ErrorKind>,
    dir_iteration_errors: BTreeMap<String, std::io::ErrorKind>,
    /// Owned paths by package, for the ownership query.
    rpm_owned: BTreeMap<String, Vec<String>>,
    /// `(flags, path)` rows of `rpm -Va`.
    rpm_va: Vec<(String, String)>,
    /// `(package, fflags, path)` rows of the file-flags query.
    file_flags: Vec<(String, String, String)>,
    /// Other commands and their results, by the mock's command key.
    commands: BTreeMap<String, ExecResult>,
    sysctl_a: Option<ExecResult>,
    tuned_active: Option<String>,
    /// `hostnamectl --json=short` stdout.
    hostnamectl: Option<String>,
}

impl SysctlHost {
    /// A host with the RPM prerequisites, `/proc/cmdline`, `lsmod` and an
    /// empty `/etc`. `sysctl -a` is not registered: a host whose files
    /// name a key states its runtime with [`sysctl_a`](Self::sysctl_a).
    pub fn new() -> Self {
        let mut host = Self::default();
        host.dirs.insert("/etc".into(), BTreeSet::new());
        host.file("/proc/cmdline", CMDLINE)
    }

    /// A regular file, listed in its directory.
    pub fn file(mut self, path: &str, content: &str) -> Self {
        self.list(path);
        self.files.insert(path.into(), content.into());
        self
    }

    /// A symlink, listed in its directory. A real read of `path` follows
    /// it, so the build registers that content at `path` as well.
    pub fn link(mut self, path: &str, target: &str) -> Self {
        self.list(path);
        self.links.insert(path.into(), target.into());
        self
    }

    /// A symlink whose target does not exist: resolving it fails and
    /// reading it fails.
    pub fn dangling_link(mut self, path: &str, target: &str) -> Self {
        self.list(path);
        self.dangling_links.insert(path.into(), target.into());
        self
    }

    /// `package` owns `path` in the RPM database.
    pub fn rpm_owned(mut self, path: &str, package: &str) -> Self {
        self.rpm_owned
            .entry(package.into())
            .or_default()
            .push(path.into());
        self
    }

    /// `rpm -Va` reports `path` with `flags` (`S.5....T.`) as a config file.
    pub fn rpm_va(mut self, flags: &str, path: &str) -> Self {
        self.rpm_va.push((flags.into(), path.into()));
        self
    }

    /// `package` lists `path` as a `%ghost` file with rpm `fflags`
    /// `flags` (`cng`, `g`), so it is owned and rpm never verifies it.
    pub fn ghost(mut self, package: &str, flags: &str, path: &str) -> Self {
        self.file_flags
            .push((package.into(), flags.into(), path.into()));
        self.rpm_owned(path, package)
    }

    /// `command` (the mock's key: the program and its arguments joined
    /// by spaces) exits `exit_code` printing `stdout` and `stderr`.
    pub fn command(mut self, command: &str, exit_code: i32, stdout: &str, stderr: &str) -> Self {
        self.commands.insert(
            command.into(),
            ExecResult {
                stdout: stdout.into(),
                stderr: stderr.into(),
                exit_code,
            },
        );
        self
    }

    /// `sysctl -a` succeeds with `stdout`.
    pub fn sysctl_a(mut self, stdout: &str) -> Self {
        self.sysctl_a = Some(ok(stdout));
        self
    }

    /// `sysctl -a` exits with `exit_code` and no output.
    pub fn sysctl_a_fails(mut self, exit_code: i32) -> Self {
        self.sysctl_a = Some(ExecResult {
            exit_code,
            ..Default::default()
        });
        self
    }

    /// `tuned-adm active` reports `profile` as the active tuned profile.
    pub fn tuned(mut self, profile: &str) -> Self {
        self.tuned_active = Some(profile.into());
        self
    }

    /// hostnamed describes the host's current name as `hostname`, from
    /// `source` (`static`, `transient` or `default`), as
    /// `hostnamectl --json=short` prints it.
    pub fn hostnamectl(mut self, hostname: &str, source: &str) -> Self {
        self.hostnamectl =
            Some(serde_json::json!({"Hostname": hostname, "HostnameSource": source}).to_string());
        self
    }

    /// Listing `dir` fails with `kind`.
    pub fn dir_error(mut self, dir: &str, kind: std::io::ErrorKind) -> Self {
        self.dir_errors.insert(dir.into(), kind);
        self
    }

    /// Listing `dir` yields its entries and then fails with `kind`, as a
    /// real directory iterator can after the directory opened.
    pub fn dir_iteration_error(mut self, dir: &str, kind: std::io::ErrorKind) -> Self {
        self.dir_iteration_errors.insert(dir.into(), kind);
        self
    }

    /// Reading `path` as a link fails with `kind`, an error other than the
    /// not-a-link answer a regular file gives.
    pub fn link_error(mut self, path: &str, kind: std::io::ErrorKind) -> Self {
        self.link_errors.insert(path.into(), kind);
        self
    }

    /// The executor the inspectors run over.
    pub fn build(self) -> MockExecutor {
        let mut ownership = String::from("@@bash\n/usr/bin/bash\n");
        for (package, paths) in &self.rpm_owned {
            ownership.push_str(&format!("@@{package}\n"));
            for path in paths {
                ownership.push_str(&format!("{path}\n"));
            }
        }
        let rpm_va: String = self
            .rpm_va
            .iter()
            .map(|(flags, path)| format!("{flags}  c {path}\n"))
            .collect();

        let mut file_flags = String::from("bash  '/usr/bin/bash'\n");
        for (package, flags, path) in &self.file_flags {
            // As rpm's `shescape`: single quotes, an embedded quote as `'\''`.
            let quoted = path.replace('\'', r"'\''");
            file_flags.push_str(&format!("{package} {flags} '{quoted}'\n"));
        }

        let mut mock = MockExecutor::new()
            .with_command(RPM_FILE_FLAGS, ok(&file_flags))
            .with_command(RPM_QA, ok("0:bash-5.2.26-3.el9.x86_64\n"))
            .with_command("rpm -Va", ok(&rpm_va))
            .with_command(RPM_FILE_OWNERSHIP, ok(&ownership))
            .with_command("lsmod", ok(LSMOD));
        for (command, result) in self.commands {
            mock = mock.with_command(&command, result);
        }
        if let Some(result) = self.sysctl_a {
            mock = mock.with_command("sysctl -a", result);
        }
        if let Some(stdout) = &self.hostnamectl {
            mock = mock.with_command("hostnamectl --json=short", ok(stdout));
        }
        if let Some(profile) = &self.tuned_active {
            mock = mock.with_command(
                "tuned-adm active",
                ok(&format!("Current active profile: {profile}\n")),
            );
        }
        for (dir, entries) in &self.dirs {
            mock = mock.with_dir(dir, entries.iter().map(String::as_str).collect());
        }
        for (dir, kind) in &self.dir_errors {
            mock = mock.with_dir_error(dir, *kind);
        }
        for (path, content) in &self.files {
            mock = mock.with_file(path, content);
        }
        for (dir, kind) in &self.dir_iteration_errors {
            mock = mock.with_dir_iteration_error(dir, *kind);
        }
        for (path, kind) in &self.link_errors {
            mock = mock.with_link_error(path, *kind);
        }
        for (path, target) in &self.dangling_links {
            mock = mock.with_dangling_link(path, target);
        }
        for (path, target) in &self.links {
            mock = mock.with_link(path, target);
        }
        // What a real read of each link returns, now that every link is
        // registered and chains resolve.
        let mut through_files: Vec<(String, String)> = Vec::new();
        let mut through_dirs: Vec<(String, Vec<String>)> = Vec::new();
        for path in self.links.keys() {
            let Ok(target) = mock.resolve_final_target(Path::new(path)) else {
                continue;
            };
            let target = target.to_str().expect("UTF-8 path");
            if target == DEV_NULL {
                through_files.push((path.clone(), String::new()));
            } else if let Some(content) = self.files.get(target) {
                through_files.push((path.clone(), content.clone()));
            } else if let Some(entries) = self.dirs.get(target) {
                through_dirs.push((path.clone(), entries.iter().cloned().collect()));
                through_files.extend(entries.iter().filter_map(|name| {
                    let content = self.files.get(&format!("{target}/{name}"))?;
                    Some((format!("{path}/{name}"), content.clone()))
                }));
            }
        }
        for (path, content) in through_files {
            mock = mock.with_file(&path, &content);
        }
        for (path, entries) in through_dirs {
            mock = mock.with_dir(&path, entries.iter().map(String::as_str).collect());
        }
        mock
    }

    /// List `path` in its parent directory, and each ancestor in its own,
    /// up to a top-level directory.
    fn list(&mut self, path: &str) {
        let mut child = Path::new(path);
        while let Some(parent) = child.parent() {
            if parent == Path::new("/") {
                break;
            }
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .expect("a host path is absolute UTF-8");
            self.dirs
                .entry(parent.to_str().expect("UTF-8 path").to_string())
                .or_default()
                .insert(name.to_string());
            child = parent;
        }
    }
}

fn ok(stdout: &str) -> ExecResult {
    ExecResult {
        stdout: stdout.into(),
        exit_code: 0,
        ..Default::default()
    }
}

fn pkg_source() -> SourceSystem {
    SourceSystem::PackageBased {
        os_release: OsRelease {
            name: "Red Hat Enterprise Linux".into(),
            version_id: "9.4".into(),
            id: "rhel".into(),
            ..Default::default()
        },
    }
}

/// `collect()` then `validate` then `redact`: the three calls the scan
/// command makes, in its order.
fn scan_with(host: SysctlHost, inspectors: Vec<Box<dyn Inspector>>) -> InspectionSnapshot {
    let exec = host.build();
    let collected = collect(
        &pkg_source(),
        &exec,
        &inspectors,
        None,
        &NullProgress,
        &AtomicBool::new(false),
    );
    let validated = validate(collected).expect("the snapshot validates");
    let mut snapshot = validated.state.snapshot;
    redact(&mut snapshot, &RedactOptions::default());
    snapshot
}

/// The scan path with the RPM, config and kernel/boot inspectors.
pub fn scan(host: SysctlHost) -> InspectionSnapshot {
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(KernelbootInspector::new()),
        ],
    )
}

/// The scan path with the RPM, config and SELinux inspectors.
pub fn scan_selinux(host: SysctlHost) -> InspectionSnapshot {
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(SelinuxInspector::new()),
        ],
    )
}

/// The scan path with the RPM and kernel/boot inspectors only, so the
/// snapshot's `config` is `None`.
pub fn scan_without_config(host: SysctlHost) -> InspectionSnapshot {
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(KernelbootInspector::new()),
        ],
    )
}

/// The scan path with the RPM and config inspectors only, so the
/// snapshot's `kernel_boot` is `None`. The scan command always runs the
/// kernel/boot inspector, and that inspector returns its section even when
/// a source fails, so this is the only way a host reaches a snapshot with
/// no kernel/boot section.
pub fn scan_without_kernel_boot(host: SysctlHost) -> InspectionSnapshot {
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
        ],
    )
}

/// Why [`FailedKernelBoot`] fails.
pub const KERNEL_BOOT_FAILURE: &str = "kernel/boot collection failed";

/// Stands in for the kernel/boot inspector and fails, as `collect()`
/// records a failed or panicked inspector: no section, the id in
/// `failed_sections`, and a warning under the inspector's name.
/// `KernelbootInspector` itself never returns `Failed`; a panic is the
/// only route to a failed kernel/boot section in a real scan.
struct FailedKernelBoot;

impl Inspector for FailedKernelBoot {
    fn id(&self) -> InspectorId {
        InspectorId::KernelBoot
    }

    fn applicable_to(&self) -> &[SourceSystemKind] {
        &[SourceSystemKind::PackageBased]
    }

    fn inspect(
        &self,
        _ctx: &InspectionContext<'_>,
        _progress: &dyn ProgressSink,
    ) -> Result<InspectorOutput, InspectorError> {
        Err(InspectorError::Failed {
            reason: KERNEL_BOOT_FAILURE.into(),
        })
    }
}

/// The scan path with the RPM and config inspectors and a kernel/boot
/// inspector that fails, so the kernel/boot section's state is `failed`.
pub fn scan_with_failed_kernel_boot(host: SysctlHost) -> InspectionSnapshot {
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(FailedKernelBoot),
        ],
    )
}

/// The snapshot as written and loaded again: serialized, then through
/// `InspectionSnapshot::load`, the path every consumer reads.
pub fn reload(snap: &InspectionSnapshot) -> InspectionSnapshot {
    let json = serde_json::to_string(snap).expect("the snapshot serializes");
    InspectionSnapshot::load(&json).expect("the written snapshot loads")
}

/// The scan's export tree, as `render_all` writes it.
pub fn render_export(snap: &InspectionSnapshot) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    render::render_all(snap, &RenderContext { target: None }, dir.path()).expect("render");
    dir
}

/// A refine session loaded from the scan's tarball: rendered, packed and
/// opened the way `inspectah refine` opens it.
pub fn session(snap: &InspectionSnapshot) -> RefineSession {
    let tree = render_export(snap);
    let packed = tempfile::tempdir().expect("tempdir");
    let tarball = packed.path().join("scan.tar.gz");
    create_tarball(tree.path(), &tarball, TARBALL_PREFIX).expect("the scan tarball packs");
    inspectah_refine::tarball::from_tarball(&tarball).expect("refine opens the scan")
}

/// `session` exported at its current generation, unpacked with the
/// tarball's top-level directory stripped, so the tree has the layout
/// [`render_export`] writes.
pub fn export_session(session: &RefineSession) -> tempfile::TempDir {
    let packed = tempfile::tempdir().expect("tempdir");
    let tarball = packed.path().join("export.tar.gz");
    session
        .export_plan(session.generation())
        .expect("the generation matches")
        .render(&tarball)
        .expect("the export writes");
    let tree = tempfile::tempdir().expect("tempdir");
    TarballExtractor::new(tree.path().to_path_buf())
        .extract(&tarball)
        .expect("the export unpacks");
    tree
}
