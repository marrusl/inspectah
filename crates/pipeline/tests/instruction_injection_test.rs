//! Snapshot values never add an instruction, directive, key or command.
//!
//! The comment guard in `render::comment::join_lines` keeps a value inside
//! a comment line. It never touches an instruction line, so a value that
//! reaches one is refused or proved before it gets there. Each row below
//! poisons one field that reaches an instruction or directive line,
//! renders the whole file through the real render path for every
//! separator in the row's set, and checks the output split on every line
//! terminator: no line that is not a comment carries the payload, and the
//! row reached its site (a FIXME names the refused value, or a benign
//! value renders the expected line). One field per row, because a fixture
//! that poisons two fields only reaches the first one the renderer
//! refuses.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
use inspectah_core::types::nonrpm::{FileType, UnmanagedFile, UnmanagedFileSection};
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::write_config_tree;
use inspectah_pipeline::render::containerfile::render_containerfile;

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

/// A value that would put the payload on its own line, or in its own
/// word, if the separator got through.
fn hostile(sep: &str) -> String {
    format!("x{sep}{PAYLOAD}")
}

/// How a row proves it reached its site rather than passing vacuously.
enum Reached {
    /// A comment line starting with this text carries the payload.
    Fixme(&'static str),
    /// Rendering this benign value yields a line containing this text.
    Benign(&'static str, &'static str),
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
    None
}

fn check_rows(rows: &[Row], seps: &[String]) {
    let mut failures = Vec::new();
    for row in rows {
        if let Reached::Benign(value, expect) = row.reached {
            let output = (row.render)(value);
            if !output.lines().any(|l| l.contains(expect)) {
                failures.push(format!(
                    "{}: benign value {value:?} did not render {expect:?}\n{output}",
                    row.field
                ));
            }
        }
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
