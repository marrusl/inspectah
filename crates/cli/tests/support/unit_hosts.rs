//! Collector-driven systemd unit hosts and the scan path they run through.
//!
//! A [`UnitHost`] describes a host's units as `systemctl list-unit-files`
//! lists them, its preset rules, the unit files under `/etc/systemd/system`
//! and the vendor directory, any other file under `/etc`, and who owns
//! each vendor file. [`scan`] runs the real RPM, config, units and
//! scheduled-task inspectors over it through the scan command's own calls
//! (`collect()`, `validate`, `redact`), and the remaining functions carry
//! the snapshot to every later surface: a JSON reload, the rendered export, a refine
//! session loaded from the scan tarball, and that session's export.
//!
//! Shared by `crates/cli/tests` and, through `#[path]`, by other test
//! crates, so it imports only crates every such target has:
//! `inspectah-core`, `inspectah-collect`, `inspectah-pipeline`,
//! `inspectah-refine`, `tempfile` and `serde_json`.

// Each test target that includes this module uses a different subset of it.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use inspectah_collect::executor::mock::MockExecutor;
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::scheduled::ScheduledTasksInspector;
use inspectah_collect::inspectors::units::UnitsInspector;
use inspectah_core::baseline::{BaselineData, BaselinePackageEntry};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::executor::{ExecResult, Executor};
use inspectah_core::traits::inspector::Inspector;
use inspectah_core::traits::progress::NullProgress;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::os::OsRelease;
use inspectah_core::types::system::SourceSystem;
use inspectah_pipeline::build::extract::TarballExtractor;
use inspectah_pipeline::collect::collect;
use inspectah_pipeline::redaction::{RedactOptions, redact};
use inspectah_pipeline::render;
use inspectah_pipeline::render::tarball::create_tarball;
use inspectah_pipeline::validate::validate;
use inspectah_refine::session::RefineSession;

/// The administrator's unit directory.
pub const ETC_UNIT_DIR: &str = "/etc/systemd/system";
/// The directory packages install unit files into.
pub const VENDOR_UNIT_DIR: &str = "/usr/lib/systemd/system";
/// The one preset file the host's rules are written to.
const PRESET_FILE: &str = "/usr/lib/systemd/system-preset/90-default.preset";

/// The `rpm -qa` query the RPM inspector lists packages with.
const RPM_QA: &str = "rpm -qa --queryformat %{EPOCH}:%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n";
/// The `rpm -qa` query the RPM inspector lists owned paths with: a
/// `@@<package>` line, then one owned path per line.
const RPM_FILE_OWNERSHIP: &str = "rpm -qa --queryformat @@%{NAME}\\n[%{FILENAMES}\\n]";
/// The query the units inspector asks a vendor unit file's owner with,
/// before the path.
const RPM_QF: &str = "rpm -qf --queryformat %{NAME}\n";
/// The query the RPM inspector reads each package's source repo with,
/// before the package names.
const DNF_SOURCE_REPO: &str = "dnf repoquery --installed --queryformat";
/// The repo every [`UnitHost::package`] is installed from.
const PACKAGE_REPO: &str = "appstream";
/// The listing the units inspector reads unit states from.
const LIST_UNIT_FILES: &str = "systemctl list-unit-files --no-pager";
/// The `list-unit-files` header systemd 252 (RHEL 9) prints. Newer
/// releases name the third column `PRESET`; the parser reads neither name.
const LIST_UNIT_FILES_HEADER: &str =
    "UNIT FILE                                  STATE           VENDOR PRESET";
/// Unit file states `systemctl list-unit-files` prints no preset for,
/// writing `-` in the column instead.
const STATES_WITHOUT_PRESET: &[&str] = &["alias", "static", "generated", "transient"];
/// The path a `/dev/null` mask resolves to.
const DEV_NULL: &str = "/dev/null";
/// The hostname the scan tarball is packed under.
const TARBALL_PREFIX: &str = "unit-host";

/// A host for the scan path, built over [`MockExecutor`].
///
/// Every registered path is listed in its parent directory, up to a
/// top-level directory, so the `/etc` walk and the units scan find it.
///
/// `MockExecutor::read_file` does not follow links, while the real
/// executor's does. [`build`](UnitHost::build) therefore registers, at each
/// link's own path, what a real read there returns: the final target's
/// content, or `""` for `/dev/null`.
///
/// Ownership has one source: [`vendor_unit`](UnitHost::vendor_unit) with an
/// owner records the file in the RPM database the RPM inspector reads, and
/// the units inspector's `rpm -qf` for that file answers with the same
/// package. A vendor unit with no owner is "not owned by any package".
#[derive(Debug, Default)]
pub struct UnitHost {
    files: BTreeMap<String, String>,
    file_errors: BTreeMap<String, std::io::ErrorKind>,
    links: BTreeMap<String, String>,
    dirs: BTreeMap<String, BTreeSet<String>>,
    /// `(name, state)` rows of `systemctl list-unit-files`.
    units: Vec<(String, String)>,
    /// Preset rules, one per line, first match wins.
    presets: Vec<String>,
    /// Owned paths by package, for both ownership queries.
    rpm_owned: BTreeMap<String, Vec<String>>,
    /// Every vendor unit file, for the `rpm -qf` answers.
    vendor_units: BTreeSet<String>,
    /// Installed packages besides `bash`, as `rpm -qa` lists them.
    packages: Vec<String>,
    /// The base image's packages, when the scan has a baseline.
    base_packages: Vec<String>,
}

impl UnitHost {
    /// A host with the RPM prerequisites, an empty `/etc/systemd/system`,
    /// an empty vendor unit directory and no preset rule.
    pub fn new() -> Self {
        let mut host = Self::default();
        host.dirs.insert("/etc".into(), BTreeSet::new());
        host.dir(ETC_UNIT_DIR).dir(VENDOR_UNIT_DIR)
    }

    /// `systemctl list-unit-files` lists `name` in `state` (`enabled`,
    /// `disabled`, `masked`, `static`, ...).
    pub fn unit(mut self, name: &str, state: &str) -> Self {
        self.units.push((name.into(), state.into()));
        self
    }

    /// An installed `x86_64` package named `name`, listed by `rpm -qa`
    /// and installed from the `appstream` repo, so it is not repo-less.
    pub fn package(mut self, name: &str) -> Self {
        self.packages.push(name.into());
        self
    }

    /// A package the base image ships, installed on the host as
    /// [`package`](UnitHost::package) installs it. The scan then records a
    /// baseline, so refine sorts the host's other added packages into its
    /// Site tier rather than Investigate.
    pub fn base_package(mut self, name: &str) -> Self {
        self.base_packages.push(name.into());
        self.package(name)
    }

    /// A preset rule (`enable foo.service`, `disable *`), after the
    /// earlier ones.
    pub fn preset(mut self, rule: &str) -> Self {
        self.presets.push(rule.into());
        self
    }

    /// A unit file under `/etc/systemd/system`.
    pub fn etc_unit(self, name: &str, content: &str) -> Self {
        self.file(&format!("{ETC_UNIT_DIR}/{name}"), content)
    }

    /// A unit file under the vendor directory, owned by `owner` when given.
    pub fn vendor_unit(mut self, name: &str, content: &str, owner: Option<&str>) -> Self {
        let path = format!("{VENDOR_UNIT_DIR}/{name}");
        self.vendor_units.insert(path.clone());
        if let Some(owner) = owner {
            self.rpm_owned
                .entry(owner.into())
                .or_default()
                .push(path.clone());
        }
        self.file(&path, content)
    }

    /// `systemctl mask`: a link from `/etc/systemd/system/<name>` to
    /// `/dev/null`.
    pub fn mask(self, name: &str) -> Self {
        self.link(&format!("{ETC_UNIT_DIR}/{name}"), DEV_NULL)
    }

    /// A regular file anywhere, listed in its directory.
    pub fn file(mut self, path: &str, content: &str) -> Self {
        self.list(path);
        self.files.insert(path.into(), content.into());
        self
    }

    /// A file, listed in its directory, whose read fails with `kind`
    /// (`InvalidData` for content that is not UTF-8 or is over the size cap).
    pub fn file_error(mut self, path: &str, kind: std::io::ErrorKind) -> Self {
        self.list(path);
        self.file_errors.insert(path.into(), kind);
        self
    }

    /// A symlink, listed in its directory. A real read of `path` follows
    /// it, so the build registers that content at `path` as well.
    pub fn link(mut self, path: &str, target: &str) -> Self {
        self.list(path);
        self.links.insert(path.into(), target.into());
        self
    }

    /// A directory, listed in its parent, empty unless files are added.
    pub fn dir(mut self, path: &str) -> Self {
        self.list(path);
        self.dirs.entry(path.into()).or_default();
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

        let mut listing = format!("{LIST_UNIT_FILES_HEADER}\n");
        for (name, state) in &self.units {
            let preset = if STATES_WITHOUT_PRESET.contains(&state.as_str()) {
                "-"
            } else {
                self.preset_for(name)
            };
            listing.push_str(&format!("{name} {state} {preset}\n"));
        }
        listing.push_str(&format!("\n{} unit files listed.\n", self.units.len()));

        let presets: String = self.presets.iter().map(|r| format!("{r}\n")).collect();
        let installed: String = std::iter::once("0:bash-5.2.26-3.el9.x86_64".to_owned())
            .chain(
                self.packages
                    .iter()
                    .map(|name| format!("0:{name}-1.0-1.el9.x86_64")),
            )
            .map(|nevra| format!("{nevra}\n"))
            .collect();
        let source_repos: String = self
            .packages
            .iter()
            .map(|name| format!("{name} {PACKAGE_REPO}\n"))
            .collect();
        let this = self.file(PRESET_FILE, &presets);

        let mut mock = MockExecutor::new()
            .with_command(RPM_QA, ok(&installed))
            .with_command("rpm -Va", ok(""))
            .with_command(RPM_FILE_OWNERSHIP, ok(&ownership))
            .with_command(LIST_UNIT_FILES, ok(&listing))
            .with_command_prefix(DNF_SOURCE_REPO, ok(&source_repos));
        // One answer per vendor file: a batch query over several files is
        // unregistered, so the inspector falls back to these.
        for path in &this.vendor_units {
            let owner = this
                .rpm_owned
                .iter()
                .find(|(_, paths)| paths.contains(path))
                .map(|(package, _)| package);
            let answer = match owner {
                Some(package) => ok(&format!("{package}\n")),
                None => ExecResult {
                    stdout: format!("file {path} is not owned by any package\n"),
                    exit_code: 1,
                    ..Default::default()
                },
            };
            mock = mock.with_command(&format!("{RPM_QF} {path}"), answer);
        }
        for (dir, entries) in &this.dirs {
            mock = mock.with_dir(dir, entries.iter().map(String::as_str).collect());
        }
        for (path, content) in &this.files {
            mock = mock.with_file(path, content);
        }
        for (path, kind) in &this.file_errors {
            mock = mock.with_file_error(path, *kind);
        }
        for (path, target) in &this.links {
            mock = mock.with_link(path, target);
        }
        // What a real read of each link returns, now that every link is
        // registered and chains resolve.
        let mut through_files: Vec<(String, String)> = Vec::new();
        for path in this.links.keys() {
            let Ok(target) = mock.resolve_final_target(Path::new(path)) else {
                continue;
            };
            let target = target.to_str().expect("UTF-8 path");
            if target == DEV_NULL {
                through_files.push((path.clone(), String::new()));
            } else if let Some(content) = this.files.get(target) {
                through_files.push((path.clone(), content.clone()));
            }
        }
        for (path, content) in through_files {
            mock = mock.with_file(&path, &content);
        }
        mock
    }

    /// The preset column systemd prints for `name`: the first matching
    /// rule's action, `enabled` when no rule matches (systemd's default).
    fn preset_for(&self, name: &str) -> &'static str {
        let action = self.presets.iter().find_map(|rule| {
            let (action, pattern) = rule.split_once(char::is_whitespace)?;
            glob_matches(pattern.trim(), name).then_some(action)
        });
        match action {
            Some("disable") => "disabled",
            Some("ignore") => "ignored",
            _ => "enabled",
        }
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

/// Whether `name` matches a preset pattern, with `*` and `?` as
/// `fnmatch` reads them.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let mut rest_of_pattern = pattern.chars();
    match rest_of_pattern.next() {
        None => name.is_empty(),
        Some('*') => name
            .char_indices()
            .map(|(i, _)| i)
            .chain([name.len()])
            .any(|i| glob_matches(rest_of_pattern.as_str(), &name[i..])),
        Some(wanted) => {
            let mut rest_of_name = name.chars();
            match rest_of_name.next() {
                Some(c) if wanted == '?' || wanted == c => {
                    glob_matches(rest_of_pattern.as_str(), rest_of_name.as_str())
                }
                _ => false,
            }
        }
    }
}

/// A token planted in [`continued_mount_host`]'s unit file, in a line
/// that reads like a second `Where=` but continues the `What=` value.
pub const CONTINUED_MOUNT_TOKEN: &str = "mount-secret-91c2";
/// The one unit [`continued_mount_host`] has.
pub const CONTINUED_MOUNT_UNIT: &str = "srv-data.mount";
/// The mount point systemd reads from [`continued_mount_host`]'s unit.
pub const CONTINUED_MOUNT_POINT: &str = "/srv/data";

/// A host with one local mount unit, enabled, whose `What=` ends in a
/// backslash, so the next line, `Where=/opt/token=<token>`, is the rest of
/// the `What=` value as systemd reads it. The token belongs to the mount
/// source; the unit's mount point is [`CONTINUED_MOUNT_POINT`].
pub fn continued_mount_host() -> UnitHost {
    UnitHost::new()
        .unit(CONTINUED_MOUNT_UNIT, "enabled")
        .preset("disable *")
        .etc_unit(
            CONTINUED_MOUNT_UNIT,
            &format!(
                "[Unit]\nDescription=Data volume\n\n[Mount]\n\
                 Where={CONTINUED_MOUNT_POINT}\n\
                 What=https://files.example/volume?\\\n\
                 Where=/opt/token={CONTINUED_MOUNT_TOKEN}\n\
                 Options=nofail\n\n\
                 [Install]\nWantedBy=multi-user.target\n"
            ),
        )
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

/// `collect()` then `validate`: the scan command's first two calls, with
/// the RPM, config, units and scheduled-task inspectors. The snapshot is
/// not redacted yet.
pub fn scan_unredacted(host: UnitHost) -> InspectionSnapshot {
    let baseline = (!host.base_packages.is_empty()).then(|| BaselineData {
        image_digest: "sha256:0000".into(),
        packages: host
            .base_packages
            .iter()
            .map(|name| {
                (
                    format!("{name}.x86_64"),
                    BaselinePackageEntry {
                        name: name.clone(),
                        epoch: None,
                        version: "1.0".into(),
                        release: "1.el9".into(),
                        arch: "x86_64".into(),
                    },
                )
            })
            .collect(),
        extracted_at: "2026-11-14T00:00:00Z".into(),
    });
    let exec = host.build();
    let inspectors: Vec<Box<dyn Inspector>> = vec![
        Box::new(RpmInspector::new()),
        Box::new(ConfigInspector::new()),
        Box::new(UnitsInspector::new()),
        Box::new(ScheduledTasksInspector::new()),
    ];
    let collected = collect(
        &pkg_source(),
        &exec,
        &inspectors,
        baseline.as_ref(),
        &NullProgress,
        &AtomicBool::new(false),
    );
    let mut snapshot = validate(collected)
        .expect("the snapshot validates")
        .state
        .snapshot;
    // The scan command records the baseline it collected against.
    snapshot.baseline = baseline;
    snapshot
}

/// The scan path: `collect()` then `validate` then `redact`, the three
/// calls the scan command makes, in its order.
pub fn scan(host: UnitHost) -> InspectionSnapshot {
    let mut snapshot = scan_unredacted(host);
    redact(&mut snapshot, &RedactOptions::default());
    snapshot
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

/// Every `systemctl` instruction in `containerfile`: each non-comment
/// instruction joined with its `\` continuation lines, kept when it runs
/// `systemctl`, wherever in the file it is.
pub fn systemctl_instructions(containerfile: &str) -> Vec<String> {
    let mut instructions = Vec::new();
    let mut current = String::new();
    for line in containerfile.lines() {
        if current.is_empty() && line.trim_start().starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        current.push_str(line.trim_end_matches('\\'));
        current.push(' ');
        if !line.ends_with('\\') {
            let instruction = std::mem::take(&mut current);
            if instruction.contains("systemctl") {
                instructions.push(instruction);
            }
        }
    }
    if current.contains("systemctl") {
        instructions.push(current);
    }
    instructions
}

/// Whether a `RUN systemctl <verb> --` instruction in `containerfile`
/// names `unit` as one of its quoted operands after `--`.
pub fn acts(containerfile: &str, verb: &str, unit: &str) -> bool {
    let prefix = format!("RUN systemctl {verb} -- ");
    let operand = format!("'{unit}'");
    systemctl_instructions(containerfile).iter().any(|i| {
        i.strip_prefix(&prefix)
            .is_some_and(|operands| operands.split_whitespace().any(|w| w == operand))
    })
}

/// Whether `unit`'s name appears in no `systemctl` instruction of
/// `containerfile`, whatever the verb and in any form.
pub fn in_no_systemctl_instruction(containerfile: &str, unit: &str) -> bool {
    systemctl_instructions(containerfile)
        .iter()
        .all(|i| !i.contains(unit))
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
