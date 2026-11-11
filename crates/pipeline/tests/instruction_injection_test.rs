//! Snapshot values never add an instruction, directive, key or command.
//!
//! The comment guard in `render::comment::join_lines` keeps a value inside
//! a comment line. It never touches an instruction line, so a value that
//! reaches one is refused or proved before it gets there. Each row below
//! poisons one field that reaches an instruction or directive line,
//! renders the whole file through the real render path for every
//! separator in the row's set, and checks the output split on every line
//! terminator: no line that is not a comment carries the payload, and the
//! row reached its site (a FIXME names the refused value or, for a field
//! the FIXME does not quote, a FIXME is present). Each group also has a
//! test that a benign value still renders its line. One field per row, because a fixture
//! that poisons two fields only reaches the first one the renderer
//! refuses.

use std::collections::HashMap;

use inspectah_core::baseline::{ResolutionStrategy, TargetImageIdentity};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::SYNTHESIZED_SYSCTL_FILE;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
use inspectah_core::types::group_render::{GroupRenderState, RenderContext};
use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
use inspectah_core::types::network::{FirewallZone, NMConnection, NetworkSection};
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
use inspectah_core::types::rpm::{
    EnabledModuleStream, InstalledGroup, PackageEntry, PackageState, RepoFile, RpmSection,
    VersionLockEntry,
};
use inspectah_core::types::scheduled::{GeneratedTimerUnit, ScheduledTaskSection, SystemdTimer};
use inspectah_core::types::selinux::{SelinuxPortLabel, SelinuxSection};
use inspectah_core::types::services::{
    ServiceSection, ServiceStateChange, ServiceUnitState, SystemdDropIn,
};
use inspectah_core::types::users::UserGroupSection;
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::write_config_tree;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::kickstart::render_kickstart;
use inspectah_pipeline::render::users;

/// What the hostile value tries to place on a line of its own.
const PAYLOAD: &str = "RUN echo pwned";

/// Every line terminator alone, and CRLF.
fn terminators() -> Vec<String> {
    LINE_TERMINATORS
        .iter()
        .map(char::to_string)
        .chain(std::iter::once("\r\n".to_string()))
        .collect()
}

/// The terminators plus what breaks a word or a quote, or starts a
/// substitution or a systemd specifier, on a single line.
fn word_breakers() -> Vec<String> {
    terminators()
        .into_iter()
        .chain(
            [" ", "'", "\"", "$(", "`", "%", "\\"]
                .iter()
                .map(|s| s.to_string()),
        )
        .collect()
}

/// The terminators plus what ends or substitutes inside a double-quoted
/// shell word. A space is left out: it is legal inside the quotes.
fn double_quote_breakers() -> Vec<String> {
    terminators()
        .into_iter()
        .chain(["\"", "$(", "`", "\\"].iter().map(|s| s.to_string()))
        .collect()
}

/// A value that would put the payload on its own line, or in its own
/// word, if the separator got through.
fn hostile(sep: &str) -> String {
    format!("x{sep}{PAYLOAD}")
}

/// How a row proves it reached its site rather than passing vacuously.
enum Reached {
    /// A comment line starting with this text carries the payload.
    Fixme(&'static str),
    /// A comment line starting with this text is present. For a field the
    /// FIXME does not quote (it names the user, not the refused home).
    Refused(&'static str),
}

struct Row {
    field: &'static str,
    render: fn(&str) -> String,
    reached: Reached,
}

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Why `output` fails the row, or `None` when the value stayed out of
/// every instruction line.
fn failure(row: &Row, output: &str) -> Option<String> {
    let lines: Vec<&str> = output.split(LINE_TERMINATORS).collect();
    for line in &lines {
        if line.contains(PAYLOAD) && !is_comment(line) {
            return Some(format!("the value reached a non-comment line: {line:?}"));
        }
    }
    if let Reached::Fixme(prefix) = row.reached
        && !lines
            .iter()
            .any(|l| l.trim_start().starts_with(prefix) && l.contains(PAYLOAD))
    {
        return Some(format!("no `{prefix}` line names the refused value"));
    }
    if let Reached::Refused(prefix) = row.reached
        && !lines.iter().any(|l| l.trim_start().starts_with(prefix))
    {
        return Some(format!("no `{prefix}` line"));
    }
    None
}

fn check_rows(rows: &[Row], seps: &[String]) {
    let mut failures = Vec::new();
    for row in rows {
        for sep in seps {
            let output = (row.render)(&hostile(sep));
            if let Some(why) = failure(row, &output) {
                failures.push(format!("{} ({sep:?}): {why}\n{output}", row.field));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Unmanaged files and configuration roots (Containerfile COPY and RUN ln)
// ---------------------------------------------------------------------------

fn unmanaged(file: UnmanagedFile) -> UnmanagedFile {
    UnmanagedFile {
        disposition: FindingKind::included(),
        ..file
    }
}

fn with_unmanaged(items: Vec<UnmanagedFile>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: items.into_iter().map(unmanaged).collect(),
        ..Default::default()
    });
    snap
}

fn regular(path: String) -> UnmanagedFile {
    UnmanagedFile {
        path,
        file_type: FileType::ElfBinary,
        ..Default::default()
    }
}

fn symlink(path: String, target: String) -> UnmanagedFile {
    UnmanagedFile {
        path,
        file_type: FileType::Symlink,
        link_target: target,
        ..Default::default()
    }
}

fn with_config_file(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: path.into(),
            content: "x=1\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
    });
    snap
}

/// The Containerfile from the roots `write_config_tree` materialized, as
/// the export renders it.
fn materialized_containerfile(snap: &InspectionSnapshot) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let roots = write_config_tree(snap, dir.path()).expect("write config tree");
    render_containerfile(snap, Some(&roots), None)
}

const COPY_ROWS: &[Row] = &[
    Row {
        field: "unmanaged single file path",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![regular(format!("/opt/{v}"))]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged file"),
    },
    Row {
        field: "unmanaged directory of several files",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![
                    regular(format!("/opt/{v}/a")),
                    regular(format!("/opt/{v}/b")),
                ]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged files under"),
    },
    Row {
        field: "unmanaged symlink path",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![symlink(format!("/opt/{v}"), "/usr/bin/true".into())]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged symlink"),
    },
    Row {
        field: "unmanaged symlink target",
        render: |v| {
            render_containerfile(
                &with_unmanaged(vec![symlink("/opt/link".into(), v.to_string())]),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: unmanaged symlink"),
    },
    Row {
        field: "config file top-level directory (snapshot roots)",
        render: |v| render_containerfile(&with_config_file(&format!("/{v}/f.conf")), None, None),
        reached: Reached::Fixme("# FIXME: config"),
    },
    Row {
        field: "config file top-level directory (materialized roots)",
        render: |v| materialized_containerfile(&with_config_file(&format!("/{v}/f.conf"))),
        reached: Reached::Fixme("# FIXME: config"),
    },
];

#[test]
fn copy_lines_refuse_every_hostile_path() {
    check_rows(COPY_ROWS, &terminators());
}

#[test]
fn copy_lines_render_benign_paths() {
    let snap = with_unmanaged(vec![
        regular("/opt/app/bin/one".into()),
        symlink("/opt/app/current".into(), "../app-1.2".into()),
    ]);
    let out = render_containerfile(&snap, None, None);
    assert!(
        out.contains("COPY unmanaged/opt/app/bin/one /opt/app/bin/one"),
        "{out}"
    );
    assert!(
        out.contains("RUN ln -sf '../app-1.2' '/opt/app/current'"),
        "{out}"
    );
    let out = materialized_containerfile(&with_config_file("/etc/app/app.conf"));
    assert!(out.contains("COPY config/etc/ /etc/"), "{out}");
}

/// A `COPY` source is a wildcard pattern to the builder, so a path holding
/// `*`, `?` or `[` would copy every file it matched. No separator row
/// covers this: the hostile values above all hold a space too.
#[test]
fn copy_lines_refuse_wildcard_paths() {
    let mut failures = Vec::new();
    for segment in ["app*", "app?", "[ab]"] {
        // The config `COPY` names only the top-level directory, so the
        // wildcard goes there; the unmanaged one names the whole path.
        let cases = [
            (
                format!("/opt/{segment}"),
                render_containerfile(
                    &with_unmanaged(vec![regular(format!("/opt/{segment}"))]),
                    None,
                    None,
                ),
            ),
            (
                format!("/{segment}"),
                render_containerfile(&with_config_file(&format!("/{segment}/f.conf")), None, None),
            ),
            (
                format!("/{segment}"),
                materialized_containerfile(&with_config_file(&format!("/{segment}/f.conf"))),
            ),
        ];
        for (path, out) in cases {
            let path = path.as_str();
            let named = out
                .lines()
                .any(|l| l.starts_with("COPY") && l.contains(path));
            let fixme = out
                .lines()
                .any(|l| l.starts_with("# FIXME") && l.contains(path));
            if named || !fixme {
                failures.push(format!("{path}: named={named} fixme={fixme}\n{out}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Files the config tree stager writes at their own path
// ---------------------------------------------------------------------------

fn with_repo_file(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        repo_files: vec![RepoFile {
            path: path.into(),
            content: "[app]\nbaseurl=https://example.com/\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn with_gpg_key(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        gpg_keys: vec![RepoFile {
            path: path.into(),
            content: "key".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn with_firewall_zone(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(NetworkSection {
        firewall_zones: vec![FirewallZone {
            path: path.into(),
            name: "app".into(),
            content: "<zone/>".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

fn with_drop_in(path: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        drop_ins: vec![SystemdDropIn {
            unit: "app.service".into(),
            path: path.into(),
            content: "[Service]\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

/// A file the stager refuses by path is not written, and the Containerfile
/// names it in a FIXME, whichever kind it is and whichever way the roots
/// are found. Without the FIXME the file would leave the image silently.
const STAGED_ROWS: &[Row] = &[
    Row {
        field: "repo file path (snapshot roots)",
        render: |v| {
            render_containerfile(
                &with_repo_file(&format!("/etc/yum.repos.d/{v}.repo")),
                None,
                None,
            )
        },
        reached: Reached::Fixme("# FIXME: repo file"),
    },
    Row {
        field: "repo file path (materialized roots)",
        render: |v| {
            materialized_containerfile(&with_repo_file(&format!("/etc/yum.repos.d/{v}.repo")))
        },
        reached: Reached::Fixme("# FIXME: repo file"),
    },
    Row {
        field: "GPG key path",
        render: |v| materialized_containerfile(&with_gpg_key(&format!("/etc/pki/rpm-gpg/{v}"))),
        reached: Reached::Fixme("# FIXME: GPG key"),
    },
    Row {
        field: "firewall zone path",
        render: |v| {
            materialized_containerfile(&with_firewall_zone(&format!(
                "/etc/firewalld/zones/{v}.xml"
            )))
        },
        reached: Reached::Fixme("# FIXME: firewall zone"),
    },
    Row {
        field: "service drop-in path",
        render: |v| {
            materialized_containerfile(&with_drop_in(&format!(
                "/etc/systemd/system/app.service.d/{v}.conf"
            )))
        },
        reached: Reached::Fixme("# FIXME: service drop-in"),
    },
];

#[test]
fn refused_staged_paths_are_named_not_dropped() {
    check_rows(STAGED_ROWS, &terminators());
}

/// The refused file is not in the export, and the benign one still is.
#[test]
fn stager_writes_only_the_paths_it_accepts() {
    let mut snap = with_repo_file("/etc/yum.repos.d/good.repo");
    if let Some(rpm) = snap.rpm.as_mut() {
        rpm.repo_files.push(RepoFile {
            path: "/etc/yum.repos.d/x\nRUN echo pwned.repo".into(),
            content: "x".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        });
    }
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_tree(&snap, dir.path()).expect("write config tree");
    let repos = dir.path().join("config/etc/yum.repos.d");
    let mut names: Vec<String> = std::fs::read_dir(&repos)
        .expect("repo dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["good.repo"]);
}

// ---------------------------------------------------------------------------
// Flatpak provisioning unit (systemd ExecStartPre= / ExecStart=, root at boot)
// ---------------------------------------------------------------------------

fn flatpak_app() -> FlatpakApp {
    FlatpakApp {
        app_id: "org.example.App".into(),
        branch: "stable".into(),
        remote: "flathub".into(),
        remote_url: "https://dl.flathub.org/repo/".into(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

fn flatpak_unit(app: FlatpakApp) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![app],
        ..Default::default()
    });
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_tree(&snap, dir.path()).expect("write config tree");
    std::fs::read_to_string(dir.path().join("flatpak/flatpak-provision.service"))
        .expect("unit written")
}

const FLATPAK_ROWS: &[Row] = &[
    Row {
        field: "flatpak remote name",
        render: |v| {
            flatpak_unit(FlatpakApp {
                remote: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak"),
    },
    Row {
        field: "flatpak remote URL",
        render: |v| {
            flatpak_unit(FlatpakApp {
                remote_url: format!("https://example.com/{v}"),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak remote"),
    },
    Row {
        field: "flatpak app id",
        render: |v| {
            flatpak_unit(FlatpakApp {
                app_id: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak app"),
    },
    Row {
        field: "flatpak branch",
        render: |v| {
            flatpak_unit(FlatpakApp {
                branch: v.into(),
                ..flatpak_app()
            })
        },
        reached: Reached::Fixme("# FIXME: flatpak app"),
    },
];

#[test]
fn flatpak_unit_refuses_every_hostile_value() {
    check_rows(FLATPAK_ROWS, &word_breakers());
}

#[test]
fn flatpak_unit_renders_benign_values() {
    let unit = flatpak_unit(flatpak_app());
    assert!(
        unit.contains(
            "ExecStartPre=/usr/bin/flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/\n"
        ),
        "{unit}"
    );
    assert!(
        unit.contains("ExecStart=/usr/bin/flatpak install -y --noninteractive flathub org.example.App//stable\n"),
        "{unit}"
    );
}

// ---------------------------------------------------------------------------
// Containerfile FROM and `dnf group install`
// ---------------------------------------------------------------------------

fn with_target_image(image_ref: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.target_image = Some(TargetImageIdentity {
        image_ref: image_ref.into(),
        strategy: ResolutionStrategy::CliOverride,
    });
    snap
}

/// A renderable package group named `name`, rendered through the real
/// Containerfile path with a render context that marks it renderable.
fn group_containerfile(name: &str) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: vec![PackageEntry {
            name: "vim".into(),
            version: "1.0".into(),
            release: "1.el9".into(),
            arch: "x86_64".into(),
            state: PackageState::Added,
            disposition: FindingKind::included(),
            source_repo: "appstream".into(),
            ..Default::default()
        }],
        installed_groups: Some(vec![InstalledGroup {
            name: name.into(),
            members: vec!["vim".into()],
            optional_installed: Vec::new(),
        }]),
        ..Default::default()
    });
    let ctx = RenderContext {
        group_states: HashMap::from([(name.to_string(), GroupRenderState::Renderable)]),
        ..Default::default()
    };
    render_containerfile(&snap, None, Some(&ctx))
}

const FROM_ROWS: &[Row] = &[Row {
    field: "target image reference",
    render: |v| render_containerfile(&with_target_image(&format!("quay.io/{v}")), None, None),
    reached: Reached::Fixme("# FIXME: target image"),
}];

const GROUP_ROWS: &[Row] = &[Row {
    field: "package group name",
    render: group_containerfile,
    reached: Reached::Fixme("# FIXME: package group"),
}];

#[test]
fn from_line_refuses_every_hostile_image_reference() {
    check_rows(FROM_ROWS, &word_breakers());
}

#[test]
fn group_install_refuses_every_hostile_group_name() {
    check_rows(GROUP_ROWS, &double_quote_breakers());
}

#[test]
fn from_and_group_lines_render_benign_values() {
    let out = render_containerfile(
        &with_target_image("quay.io/centos-bootc/centos-bootc:stream10"),
        None,
        None,
    );
    assert!(
        out.contains("FROM quay.io/centos-bootc/centos-bootc:stream10"),
        "{out}"
    );
    let out = group_containerfile("Development Tools");
    assert!(out.contains("    \"Development Tools\" \\"), "{out}");
    // A refused group's member is installed on its own instead.
    let out = group_containerfile("x\nRUN y");
    assert!(out.lines().any(|l| l.trim() == "vim \\"), "{out}");
}

// ---------------------------------------------------------------------------
// Operands of RUN commands (dnf, systemctl, setsebool, semanage)
// ---------------------------------------------------------------------------

/// Words a command would read as an option, or the shell as the start of
/// a comment, when they stand as an operand.
const OPERAND_VALUES: &[&str] = &["--nogpgcheck", "-oalice", "#foo"];

/// A row for one operand site: the FIXME prefix the refusal writes.
struct OperandRow {
    field: &'static str,
    render: fn(&str) -> String,
    fixme: &'static str,
}

/// Why `output` fails the row for `value`, or `None`. A comment here is a
/// line that starts with `#` at the margin: an indented `#foo \` inside a
/// joined `RUN` is exactly what the row is looking for, so it counts.
fn operand_failure(row: &OperandRow, value: &str, output: &str) -> Option<String> {
    for line in output.split(LINE_TERMINATORS) {
        if line.starts_with('#') {
            continue;
        }
        if line.split_whitespace().any(|w| w.contains(value)) {
            return Some(format!("the value reached a command line: {line:?}"));
        }
    }
    let fixme = output
        .split(LINE_TERMINATORS)
        .any(|l| l.starts_with(row.fixme) && l.contains(value));
    (!fixme).then(|| format!("no `{}` line names the value", row.fixme))
}

fn rpm_snap(rpm: RpmSection) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(rpm);
    snap
}

fn package(name: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        version: "1.0".into(),
        release: "1.el9".into(),
        arch: "x86_64".into(),
        state: PackageState::Added,
        disposition: FindingKind::included(),
        source_repo: "appstream".into(),
        ..Default::default()
    }
}

fn module(name: &str, stream: &str, profile: &str) -> String {
    render_containerfile(
        &rpm_snap(RpmSection {
            module_streams: vec![EnabledModuleStream {
                module_name: name.into(),
                stream: stream.into(),
                profiles: vec![profile.into()],
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        None,
        None,
    )
}

fn selinux(boolean: (&str, &str), port: (&str, &str, &str)) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.selinux = Some(SelinuxSection {
        boolean_overrides: vec![serde_json::json!({
            "name": boolean.0,
            "current_value": boolean.1,
            "non_default": true,
            "include": true,
        })],
        port_labels: vec![SelinuxPortLabel {
            label_type: port.0.into(),
            protocol: port.1.into(),
            port: port.2.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    render_containerfile(&snap, None, None)
}

fn service(unit: &str, state: ServiceUnitState) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.services = Some(ServiceSection {
        state_changes: vec![ServiceStateChange {
            unit: unit.into(),
            current_state: state,
            default_state: None,
            disposition: FindingKind::included(),
            locked: false,
            owning_package: None,
            aggregate: None,
            attention_reason: None,
            shadow_type: None,
            shadow_rationale: None,
        }],
        ..Default::default()
    });
    render_containerfile(&snap, None, None)
}

fn scheduled(st: ScheduledTaskSection) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.scheduled_tasks = Some(st);
    render_containerfile(&snap, None, None)
}

const OPERAND_ROWS: &[OperandRow] = &[
    OperandRow {
        field: "dnf install package name",
        render: |v| {
            render_containerfile(
                &rpm_snap(RpmSection {
                    packages_added: vec![package("vim"), package(v)],
                    ..Default::default()
                }),
                None,
                None,
            )
        },
        fixme: "# FIXME: package",
    },
    OperandRow {
        field: "dnf module name",
        render: |v| module(v, "1.0", "common"),
        fixme: "# FIXME: module",
    },
    OperandRow {
        field: "dnf module stream",
        render: |v| module("nginx", v, "common"),
        fixme: "# FIXME: module",
    },
    OperandRow {
        field: "dnf module profile",
        render: |v| module("nginx", "1.24", v),
        fixme: "# FIXME: module",
    },
    OperandRow {
        field: "dnf versionlock pattern",
        render: |v| {
            render_containerfile(
                &rpm_snap(RpmSection {
                    version_locks: vec![VersionLockEntry {
                        raw_pattern: v.into(),
                        disposition: FindingKind::included(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                None,
                None,
            )
        },
        fixme: "# FIXME: version lock",
    },
    OperandRow {
        field: "dnf group install name",
        render: group_containerfile,
        fixme: "# FIXME: package group",
    },
    OperandRow {
        field: "setsebool name",
        render: |v| selinux((v, "on"), ("http_port_t", "tcp", "8080")),
        fixme: "# FIXME: boolean",
    },
    OperandRow {
        field: "setsebool value",
        render: |v| {
            selinux(
                ("httpd_can_network_connect", v),
                ("http_port_t", "tcp", "8080"),
            )
        },
        fixme: "# FIXME: boolean",
    },
    OperandRow {
        field: "semanage port type",
        render: |v| selinux(("httpd_can_network_connect", "on"), (v, "tcp", "8080")),
        fixme: "# FIXME: port label",
    },
    OperandRow {
        field: "semanage port protocol",
        render: |v| {
            selinux(
                ("httpd_can_network_connect", "on"),
                ("http_port_t", v, "8080"),
            )
        },
        fixme: "# FIXME: port label",
    },
    OperandRow {
        field: "semanage port number",
        render: |v| {
            selinux(
                ("httpd_can_network_connect", "on"),
                ("http_port_t", "tcp", v),
            )
        },
        fixme: "# FIXME: port label",
    },
    OperandRow {
        field: "systemctl enable unit",
        render: |v| service(&format!("{v}.service"), ServiceUnitState::Enabled),
        fixme: "# FIXME: unit",
    },
    OperandRow {
        field: "systemctl disable unit",
        render: |v| service(&format!("{v}.service"), ServiceUnitState::Disabled),
        fixme: "# FIXME: unit",
    },
    OperandRow {
        field: "systemctl mask unit",
        render: |v| service(&format!("{v}.service"), ServiceUnitState::Masked),
        fixme: "# FIXME: unit",
    },
    OperandRow {
        field: "local timer unit",
        render: |v| {
            scheduled(ScheduledTaskSection {
                systemd_timers: vec![SystemdTimer {
                    name: v.into(),
                    source: "local".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            })
        },
        fixme: "# FIXME: Timer unit name",
    },
    OperandRow {
        field: "cron-converted timer unit",
        render: |v| {
            scheduled(ScheduledTaskSection {
                generated_timer_units: vec![GeneratedTimerUnit {
                    name: v.into(),
                    cron_expr: "0 * * * *".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            })
        },
        fixme: "# FIXME: Timer unit name",
    },
    OperandRow {
        field: "@reboot service unit",
        render: |v| {
            scheduled(ScheduledTaskSection {
                generated_timer_units: vec![GeneratedTimerUnit {
                    name: v.into(),
                    cron_expr: "@reboot".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            })
        },
        fixme: "# FIXME: Timer unit name",
    },
];

#[test]
fn operands_never_start_like_an_option_or_a_comment() {
    let mut failures = Vec::new();
    for row in OPERAND_ROWS {
        for value in OPERAND_VALUES {
            let output = (row.render)(value);
            if let Some(why) = operand_failure(row, value, &output) {
                failures.push(format!("{} ({value:?}): {why}\n{output}", row.field));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The benign value of each operand row still renders its command.
#[test]
fn operands_render_benign_values() {
    let cases: &[(String, &str)] = &[
        ((OPERAND_ROWS[0].render)("httpd"), "    httpd \\"),
        (
            module("nginx", "1.24", "common"),
            "RUN dnf module enable -y nginx:1.24/common",
        ),
        (
            selinux(
                ("httpd_can_network_connect", "on"),
                ("http_port_t", "tcp", "8080"),
            ),
            "RUN setsebool -P httpd_can_network_connect on",
        ),
        (
            selinux(
                ("httpd_can_network_connect", "on"),
                ("http_port_t", "tcp", "8080"),
            ),
            "RUN semanage port -a -t http_port_t -p tcp 8080",
        ),
        (
            service("httpd.service", ServiceUnitState::Enabled),
            "    httpd.service",
        ),
    ];
    for (out, line) in cases {
        assert!(out.lines().any(|l| l == *line), "missing {line:?}\n{out}");
    }
}

// ---------------------------------------------------------------------------
// Kickstart suggestion (anaconda directives and the %post shell)
// ---------------------------------------------------------------------------

fn kickstart_with(network: NetworkSection) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(network);
    render_kickstart(&snap)
}

fn dhcp_connection(name: &str) -> String {
    kickstart_with(NetworkSection {
        connections: vec![NMConnection {
            name: name.into(),
            method: "auto".into(),
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn hosts_addition(line: &str) -> String {
    kickstart_with(NetworkSection {
        hosts_additions: vec![line.into()],
        ..Default::default()
    })
}

const KICKSTART_DEVICE_ROWS: &[Row] = &[Row {
    field: "DHCP connection name in network --device=",
    render: dhcp_connection,
    reached: Reached::Fixme("# FIXME: network"),
}];

const KICKSTART_HOSTS_ROWS: &[Row] = &[Row {
    field: "/etc/hosts addition in %post",
    render: hosts_addition,
    reached: Reached::Fixme("# FIXME: /etc/hosts"),
}];

#[test]
fn kickstart_network_refuses_every_hostile_device_name() {
    check_rows(KICKSTART_DEVICE_ROWS, &word_breakers());
}

#[test]
fn kickstart_hosts_refuses_every_line_break() {
    check_rows(KICKSTART_HOSTS_ROWS, &terminators());
}

#[test]
fn kickstart_renders_benign_network_values() {
    let out = dhcp_connection("eth0");
    assert!(
        out.contains("network --bootproto=dhcp --device=eth0 --activate"),
        "{out}"
    );
}

/// A hosts line keeps every character but a line break, so it is quoted
/// rather than refused. Run the rendered `printf` through `sh` and check
/// it prints the value back unchanged: nothing in it was substituted or
/// split, and no command ran.
#[test]
fn kickstart_hosts_line_reaches_the_shell_as_one_literal() {
    for sep in [" ", "\t", "'", "\"", "$(", "`", "\\", "%", ";", "&&", "#"] {
        let value = format!("10.0.0.1 db{sep}$(echo pwned) `echo pwned`");
        let out = hosts_addition(&value);
        let line = out
            .lines()
            .find(|l| l.starts_with("printf "))
            .unwrap_or_else(|| panic!("{sep:?}: no printf line\n{out}"));
        let command = line
            .strip_suffix(" >> /etc/hosts")
            .unwrap_or_else(|| panic!("{sep:?}: unexpected line {line:?}"));
        let printed = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .expect("run sh");
        assert_eq!(
            String::from_utf8_lossy(&printed.stdout),
            format!("{value}\n"),
            "{sep:?}: {line}"
        );
    }
}

// ---------------------------------------------------------------------------
// Users kickstart (anaconda directives) and blueprint TOML
// ---------------------------------------------------------------------------

fn base_user() -> serde_json::Value {
    serde_json::json!({
        "name": "alice",
        "uid": 1001,
        "gid": 1001,
        "home": "/home/alice",
        "shell": "/bin/bash",
        "include": true,
        "supplementary_groups": ["wheel"],
        "password_hash": "$6$salt$hash",
        "ssh_keys": ["ssh-ed25519 AAAAC3Nz alice@host"]
    })
}

fn base_group() -> serde_json::Value {
    serde_json::json!({"name": "devs", "gid": 1010, "source": "custom", "include": true})
}

fn users_snap(user: serde_json::Value, group: serde_json::Value) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(UserGroupSection {
        users: vec![user],
        groups: vec![group],
        ..Default::default()
    });
    snap
}

/// `base_user()` with `field` set to `value`.
fn user_with(field: &str, value: serde_json::Value) -> serde_json::Value {
    let mut u = base_user();
    u[field] = value;
    u
}

fn ks_user(field: &str, value: serde_json::Value) -> String {
    users::render_kickstart(&users_snap(user_with(field, value), base_group()))
}

const USERS_KS_ROWS: &[Row] = &[
    Row {
        field: "kickstart group name",
        render: |v| {
            let mut g = base_group();
            g["name"] = v.into();
            users::render_kickstart(&users_snap(base_user(), g))
        },
        reached: Reached::Fixme("# FIXME: group"),
    },
    Row {
        field: "kickstart user name",
        render: |v| ks_user("name", v.into()),
        reached: Reached::Fixme("# FIXME: user"),
    },
    Row {
        field: "kickstart supplementary group",
        render: |v| ks_user("supplementary_groups", serde_json::json!([v])),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart home",
        render: |v| ks_user("home", format!("/home/{v}").into()),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart shell",
        render: |v| ks_user("shell", format!("/bin/{v}").into()),
        reached: Reached::Refused("# FIXME: user"),
    },
    Row {
        field: "kickstart password hash",
        render: |v| ks_user("password_hash", format!("$6$s${v}").into()),
        reached: Reached::Refused("# FIXME: password"),
    },
];

/// An SSH key holds spaces legitimately, so its row is driven by what
/// ends or acts inside the double quotes it is written in.
const USERS_KS_KEY_ROWS: &[Row] = &[Row {
    field: "kickstart sshkey",
    render: |v| {
        ks_user(
            "ssh_keys",
            serde_json::json!([format!("ssh-ed25519 AAAA {v}")]),
        )
    },
    reached: Reached::Fixme("# FIXME: SSH key"),
}];

#[test]
fn users_kickstart_refuses_every_hostile_value() {
    check_rows(USERS_KS_ROWS, &word_breakers());
    check_rows(USERS_KS_KEY_ROWS, &double_quote_breakers());
}

#[test]
fn users_kickstart_renders_benign_values() {
    let out = users::render_kickstart(&users_snap(base_user(), base_group()));
    for line in [
        "group --name=devs --gid=1010",
        "user --name=alice --uid=1001 --gid=1001 --shell=/bin/bash --homedir=/home/alice --groups=wheel --iscrypted --password=$6$salt$hash",
        "sshkey --username=alice \"ssh-ed25519 AAAAC3Nz alice@host\"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}\n{out}");
    }
}

/// The user names `out` carries: each `user --name=` line of a kickstart,
/// or each `[[customizations.user]]` name of a blueprint.
fn carried_users(kickstart: &str, blueprint: &str) -> (Vec<String>, Vec<String>) {
    let ks = kickstart
        .lines()
        .filter_map(|l| l.strip_prefix("user --name="))
        .map(|rest| rest.split(' ').next().unwrap_or("").to_string())
        .collect();
    let doc: toml::Table = blueprint.parse().expect("blueprint is TOML");
    let bp = doc
        .get("customizations")
        .and_then(|c| c.get("user"))
        .and_then(|u| u.as_array())
        .map(|users| {
            users
                .iter()
                .filter_map(|u| u["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    (ks, bp)
}

/// Every key, at every depth, of a parsed TOML value.
fn all_keys(value: &toml::Value, out: &mut Vec<String>) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                out.push(k.clone());
                all_keys(v, out);
            }
        }
        toml::Value::Array(a) => a.iter().for_each(|v| all_keys(v, out)),
        _ => {}
    }
}

/// Builds the JSON value a user field holds from a hostile string.
type FieldValue = fn(&str) -> serde_json::Value;

/// The blueprint proves what the kickstart proves. A user whose name,
/// supplementary group, home or shell the kickstart refuses, a group whose
/// name it refuses, and a hash it refuses, get the same FIXME here and are
/// left out; the fragment still parses and adds no key. Both fragments
/// carry the same users.
#[test]
fn users_blueprint_refuses_what_the_kickstart_refuses() {
    let mut values: Vec<String> = word_breakers()
        .iter()
        .chain(["=", "[x]", "\"\"\""].map(String::from).iter())
        .map(|sep| format!("x{sep}injected = true"))
        .collect();
    // Values an account name may not hold but a path segment may, so they
    // drive the name fields only.
    let names_only = ["-oalice", "1000", "..", "host$"];
    values.extend(names_only.map(String::from));
    let user_fields: &[(&str, FieldValue, &str)] = &[
        ("name", |v| v.into(), "# FIXME: user"),
        (
            "supplementary_groups",
            |v| serde_json::json!([v]),
            "# FIXME: user",
        ),
        ("home", |v| format!("/home/{v}").into(), "# FIXME: user"),
        ("shell", |v| format!("/bin/{v}").into(), "# FIXME: user"),
        (
            "password_hash",
            |v| format!("$6$s${v}").into(),
            "# FIXME: password",
        ),
    ];
    let mut failures = Vec::new();
    for value in &values {
        let name_value = names_only.contains(&value.as_str());
        let mut cases: Vec<(String, InspectionSnapshot, &str)> = user_fields
            .iter()
            .filter(|(f, _, _)| !name_value || !matches!(*f, "home" | "shell" | "password_hash"))
            .map(|(f, make, fixme)| {
                (
                    f.to_string(),
                    users_snap(user_with(f, make(value)), base_group()),
                    *fixme,
                )
            })
            .collect();
        let mut g = base_group();
        g["name"] = value.clone().into();
        cases.push((
            "group name".into(),
            users_snap(base_user(), g),
            "# FIXME: group",
        ));

        for (field, snap, fixme) in cases {
            let out = users::render_blueprint_toml(&snap);
            let doc: toml::Value = match out.parse::<toml::Table>() {
                Ok(d) => toml::Value::Table(d),
                Err(e) => {
                    failures.push(format!("{field} ({value:?}): not TOML: {e}\n{out}"));
                    continue;
                }
            };
            let mut keys = Vec::new();
            all_keys(&doc, &mut keys);
            if keys.iter().any(|k| k == "injected") {
                failures.push(format!("{field} ({value:?}): added a key\n{out}"));
            }
            if !out.lines().any(|l| l.starts_with(fixme)) {
                failures.push(format!("{field} ({value:?}): no `{fixme}` line\n{out}"));
            }
            let ks = users::render_kickstart(&snap);
            let (ks_users, bp_users) = carried_users(&ks, &out);
            if ks_users != bp_users {
                failures.push(format!(
                    "{field} ({value:?}): kickstart carries {ks_users:?}, blueprint {bp_users:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// The SSH key is the one blueprint value carried on escaping alone: it
/// holds spaces legitimately. Poisoned with every terminator, CRLF, a
/// quote, a backslash and DEL, the fragment must parse, hold exactly the
/// keys a benign user produces, and carry the key back unchanged.
#[test]
fn users_blueprint_key_is_escaped_not_refused() {
    let mut seps = terminators();
    seps.extend(
        ["\"", "\\", "'", "\"\"\"", "=", "[x]", "\u{7f}"]
            .iter()
            .map(|s| s.to_string()),
    );
    let user_keys = [
        "gid", "groups", "home", "key", "name", "password", "shell", "uid",
    ];
    let mut failures = Vec::new();
    for sep in &seps {
        let value = format!("ssh-ed25519 AAAA x{sep}injected = true");
        let snap = users_snap(
            user_with("ssh_keys", serde_json::json!([value])),
            base_group(),
        );
        let out = users::render_blueprint_toml(&snap);
        let doc: toml::Table = match out.parse() {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{sep:?}: not TOML: {e}\n{out}"));
                continue;
            }
        };
        let user = doc["customizations"]["user"][0]
            .as_table()
            .expect("user table");
        let mut ukeys: Vec<&str> = user.keys().map(String::as_str).collect();
        ukeys.sort();
        if ukeys != user_keys {
            failures.push(format!("{sep:?}: keys changed\n{out}"));
        } else if user["key"].as_str() != Some(value.as_str()) {
            failures.push(format!("{sep:?}: key not carried\n{out}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn users_blueprint_renders_benign_values() {
    let out = users::render_blueprint_toml(&users_snap(base_user(), base_group()));
    for line in [
        "name = \"devs\"",
        "name = \"alice\"",
        "home = \"/home/alice\"",
        "shell = \"/bin/bash\"",
        "groups = [\"wheel\"]",
        "password = \"$6$salt$hash\"",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?}\n{out}");
    }
}

// ---------------------------------------------------------------------------
// Synthesized sysctl drop-in (`key = value`, applied at boot)
// ---------------------------------------------------------------------------

fn sysctl_dropin(key: &str, runtime: &str) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.kernel_boot = Some(KernelBootSection {
        sysctl_overrides: vec![SysctlOverride {
            key: key.into(),
            runtime: runtime.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let dir = tempfile::tempdir().expect("tempdir");
    write_config_tree(&snap, dir.path()).expect("write config tree");
    std::fs::read_to_string(
        dir.path()
            .join("sysctl/etc/sysctl.d")
            .join(SYNTHESIZED_SYSCTL_FILE),
    )
    .expect("drop-in written")
}

const SYSCTL_KEY_ROWS: &[Row] = &[Row {
    field: "sysctl key",
    render: |v| sysctl_dropin(&format!("net.ipv4.{v}"), "1"),
    reached: Reached::Fixme("# FIXME: sysctl"),
}];

/// The FIXME names the key, not the refused value.
const SYSCTL_VALUE_ROWS: &[Row] = &[Row {
    field: "sysctl value",
    render: |v| sysctl_dropin("net.ipv4.ip_forward", v),
    reached: Reached::Refused("# FIXME: sysctl"),
}];

#[test]
fn sysctl_dropin_refuses_every_hostile_key_and_value() {
    let mut key_seps = terminators();
    key_seps.extend([" ", "=", "#", ";"].iter().map(|s| s.to_string()));
    check_rows(SYSCTL_KEY_ROWS, &key_seps);
    check_rows(SYSCTL_VALUE_ROWS, &terminators());
}

/// No sysctl key starts with `/` or holds `..`, so neither is carried,
/// whatever a reader would make of it.
#[test]
fn sysctl_dropin_refuses_absolute_and_climbing_keys() {
    for key in [
        "../../etc/passwd",
        "net/../../etc/passwd",
        "net.ipv4..conf",
        "/proc/sys/kernel/x",
        "/etc/passwd",
    ] {
        let out = sysctl_dropin(key, "1");
        assert!(
            out.lines().all(|l| l.starts_with('#') || !l.contains(key)),
            "{key}: {out:?}"
        );
        assert!(
            out.lines()
                .any(|l| l.starts_with("# FIXME: sysctl") && l.contains(key)),
            "{key}: {out:?}"
        );
    }
}

#[test]
fn sysctl_dropin_renders_benign_values() {
    let out = sysctl_dropin("net.ipv4.ip_local_port_range", "32768\t60999");
    assert!(
        out.lines()
            .any(|l| l == "net.ipv4.ip_local_port_range = 32768\t60999"),
        "{out:?}"
    );
    let out = sysctl_dropin("net/ipv4/conf/eth0.100/rp_filter", "2");
    assert!(
        out.lines()
            .any(|l| l == "net/ipv4/conf/eth0.100/rp_filter = 2"),
        "{out:?}"
    );
    // Sysctl values are trusted host configuration: the renderer keeps
    // them on their line and does not judge them, so a pipe core_pattern
    // is carried as the host had it.
    let pattern = "|/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h";
    let out = sysctl_dropin("kernel.core_pattern", pattern);
    assert!(
        out.lines()
            .any(|l| l == format!("kernel.core_pattern = {pattern}")),
        "{out:?}"
    );
}
