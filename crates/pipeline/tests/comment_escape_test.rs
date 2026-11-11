//! Snapshot values in comment lines stay inside their comment.
//!
//! A rendered comment line that carries a snapshot value ends early when
//! the value holds a line terminator, and whatever follows becomes an
//! instruction, or at least a line a reviewer is shown that the file does
//! not hold. Each row below poisons one field that reaches a comment, for
//! every terminator in `LINE_TERMINATORS` and for CRLF, renders the whole
//! file, and checks the result split on every terminator: the payload is
//! present, it never opens a line, and every line holding it is a comment.
//! One field per row, because a fixture that poisons two fields only
//! reaches the first one the renderer refuses.

use std::collections::HashMap;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
use inspectah_core::types::group_render::{DegradationReason, GroupRenderState, RenderContext};
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::network::{NMConnection, NetworkSection, ProxyEntry, StaticRouteFile};
use inspectah_core::types::nonrpm::{
    FileType, NonRpmItem, NonRpmSoftwareSection, UnmanagedFile, UnmanagedFileSection,
};
use inspectah_core::types::redaction::{DetectionMethod, RedactionFinding, RedactionKind};
use inspectah_core::types::rpm::{InstalledGroup, PackageEntry, PackageState, RpmSection};
use inspectah_core::types::storage::{FstabEntry, StorageSection, VarDirBacking, VarDirectory};
use inspectah_core::util::{METHOD_GEM_LOCKFILE, METHOD_NPM_LOCKFILE, METHOD_PYTHON_VENV};
use inspectah_pipeline::render::comment::LINE_TERMINATORS;
use inspectah_pipeline::render::configtree::write_config_tree;
use inspectah_pipeline::render::containerfile::render_containerfile;
use inspectah_pipeline::render::kickstart::render_kickstart;
use inspectah_pipeline::render::write_tmpfiles_conf;

/// What the hostile value tries to start a line with.
const PAYLOAD: &str = "RUN echo pwned";

/// Every separator a row is driven with: each terminator alone, and CRLF.
fn separators() -> Vec<String> {
    LINE_TERMINATORS
        .iter()
        .map(char::to_string)
        .chain(std::iter::once("\r\n".to_string()))
        .collect()
}

/// A value whose terminator would open a line with the payload.
fn hostile(sep: &str) -> String {
    format!("x{sep}{PAYLOAD}")
}

/// One poisoned field: the comment that names it, and how to render the
/// whole file with `value` in that field.
struct Row {
    field: &'static str,
    /// The start of the comment line the value must land in, which proves
    /// the row reached its site rather than passing vacuously.
    comment: &'static str,
    render: fn(&str) -> String,
}

/// Why `output` fails the row, or `None` when the value stayed inside its
/// comment.
fn escape_failure(comment: &str, output: &str) -> Option<String> {
    let lines: Vec<&str> = output.split(LINE_TERMINATORS).collect();
    if !lines
        .iter()
        .any(|l| l.trim_start().starts_with(comment) && l.contains(PAYLOAD))
    {
        return Some(format!("no `{comment}` line carries the value"));
    }
    for line in lines.iter().map(|l| l.trim_start()) {
        if line.starts_with(PAYLOAD) {
            return Some(format!("the value opened a line: {line:?}"));
        }
        if line.contains(PAYLOAD) && !line.starts_with('#') {
            return Some(format!(
                "the value reached a line that is not a comment: {line:?}"
            ));
        }
    }
    None
}

/// Run every row for every separator and report all the failures at once.
fn check_rows(rows: &[Row]) {
    let mut failures = Vec::new();
    for row in rows {
        for sep in separators() {
            let output = (row.render)(&hostile(&sep));
            if let Some(why) = escape_failure(row.comment, &output) {
                failures.push(format!("{} ({sep:?}): {why}\n{output}", row.field));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

// ---------------------------------------------------------------------------
// Snapshot builders
// ---------------------------------------------------------------------------

fn package(name: &str, arch: &str, state: PackageState, source_repo: &str) -> PackageEntry {
    PackageEntry {
        name: name.into(),
        version: "1.0".into(),
        release: "1.el9".into(),
        arch: arch.into(),
        state,
        disposition: FindingKind::included(),
        source_repo: source_repo.into(),
        ..Default::default()
    }
}

fn with_packages(packages: Vec<PackageEntry>) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(RpmSection {
        packages_added: packages,
        ..Default::default()
    });
    snap
}

fn with_group(name: &str) -> InspectionSnapshot {
    let mut snap = with_packages(vec![package(
        "vim",
        "x86_64",
        PackageState::Added,
        "appstream",
    )]);
    if let Some(rpm) = snap.rpm.as_mut() {
        rpm.installed_groups = Some(vec![InstalledGroup {
            name: name.into(),
            members: vec!["vim".into()],
            optional_installed: vec!["vim".into()],
        }]);
    }
    snap
}

fn degraded_context(group: &str) -> RenderContext {
    RenderContext {
        group_states: HashMap::from([(
            group.to_string(),
            GroupRenderState::Degraded {
                reason: DegradationReason::MemberExcluded,
            },
        )]),
        ..Default::default()
    }
}

fn with_unmanaged(file: UnmanagedFile) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.unmanaged_files = Some(UnmanagedFileSection {
        items: vec![UnmanagedFile {
            disposition: FindingKind::included(),
            ..file
        }],
        ..Default::default()
    });
    snap
}

fn with_non_rpm(item: NonRpmItem) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.non_rpm_software = Some(NonRpmSoftwareSection {
        items: vec![NonRpmItem {
            disposition: FindingKind::included(),
            ..item
        }],
        ..Default::default()
    });
    snap
}

fn language_item(method: &str, path: &str, confidence: &str, manifests: &[&str]) -> NonRpmItem {
    NonRpmItem {
        path: path.into(),
        name: "app".into(),
        method: method.into(),
        confidence: confidence.into(),
        manifest_files: manifests
            .iter()
            .map(|m| (m.to_string(), "{}".to_string()))
            .collect(),
        ..Default::default()
    }
}

fn migration_item(path: &str, method: &str, notes: &str, shared_libs: &[&str]) -> NonRpmItem {
    NonRpmItem {
        path: path.into(),
        name: "tool".into(),
        method: method.into(),
        review_status: "migration_planned".into(),
        notes: notes.into(),
        shared_libs: shared_libs.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

fn with_network(network: NetworkSection) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.network = Some(network);
    snap
}

fn redaction(path: &str, remediation: &str, kind: RedactionKind) -> RedactionFinding {
    RedactionFinding {
        path: path.into(),
        source: "file".into(),
        kind,
        pattern: "password".into(),
        remediation: remediation.into(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: None,
        finding_kind: None,
    }
}

fn with_redaction(finding: RedactionFinding) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.redactions = vec![finding];
    snap
}

fn containerfile(snap: &InspectionSnapshot) -> String {
    render_containerfile(snap, None, None)
}

// ---------------------------------------------------------------------------
// Containerfile
// ---------------------------------------------------------------------------

const CONTAINERFILE_ROWS: &[Row] = &[
    Row {
        field: "locally installed package name",
        comment: "# TODO:",
        render: |v| {
            containerfile(&with_packages(vec![package(
                v,
                "x86_64",
                PackageState::LocalInstall,
                "",
            )]))
        },
    },
    Row {
        field: "repo-less package arch",
        comment: "# TODO:",
        render: |v| {
            containerfile(&with_packages(vec![package(
                "tool",
                v,
                PackageState::NoRepo,
                "",
            )]))
        },
    },
    Row {
        field: "package name with no recorded source repo",
        comment: "# TODO:",
        render: |v| {
            containerfile(&with_packages(vec![package(
                v,
                "x86_64",
                PackageState::Added,
                "",
            )]))
        },
    },
    Row {
        field: "degraded group name",
        comment: "# \"x",
        render: |v| render_containerfile(&with_group(v), None, Some(&degraded_context(v))),
    },
    Row {
        field: "group name with optional members",
        comment: "# Optional members from",
        render: |v| render_containerfile(&with_group(v), None, Some(&RenderContext::default())),
    },
    Row {
        field: "unmanaged path under /srv",
        comment: "#   /srv/x",
        render: |v| {
            containerfile(&with_unmanaged(UnmanagedFile {
                path: format!("/srv/{v}"),
                ..Default::default()
            }))
        },
    },
    Row {
        field: "unmanaged symlink with no target",
        comment: "# SYMLINK:",
        render: |v| {
            containerfile(&with_unmanaged(UnmanagedFile {
                path: format!("/opt/{v}"),
                file_type: FileType::Symlink,
                ..Default::default()
            }))
        },
    },
    Row {
        field: "low-confidence pip path",
        comment: "# pip packages:",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_PYTHON_VENV,
                &format!("opt/{v}"),
                "low",
                &[],
            )))
        },
    },
    Row {
        field: "low-confidence npm path",
        comment: "# npm packages:",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_NPM_LOCKFILE,
                &format!("opt/{v}"),
                "low",
                &[],
            )))
        },
    },
    Row {
        field: "low-confidence gem path",
        comment: "# gem packages:",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_GEM_LOCKFILE,
                &format!("opt/{v}"),
                "low",
                &[],
            )))
        },
    },
    Row {
        field: "high-confidence venv path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_PYTHON_VENV,
                &format!("opt/{v}"),
                "high",
                &["requirements.txt"],
            )))
        },
    },
    Row {
        field: "medium-confidence venv path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_PYTHON_VENV,
                &format!("opt/{v}"),
                "medium",
                &[],
            )))
        },
    },
    Row {
        field: "high-confidence npm project path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_NPM_LOCKFILE,
                &format!("opt/{v}"),
                "high",
                &["package.json", "package-lock.json"],
            )))
        },
    },
    Row {
        field: "medium-confidence npm project path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_NPM_LOCKFILE,
                &format!("opt/{v}"),
                "medium",
                &[],
            )))
        },
    },
    Row {
        field: "high-confidence gem project path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_GEM_LOCKFILE,
                &format!("opt/{v}"),
                "high",
                &["Gemfile", "Gemfile.lock"],
            )))
        },
    },
    Row {
        field: "medium-confidence gem project path",
        comment: "# WARNING: path contains unsafe characters",
        render: |v| {
            containerfile(&with_non_rpm(language_item(
                METHOD_GEM_LOCKFILE,
                &format!("opt/{v}"),
                "medium",
                &[],
            )))
        },
    },
    Row {
        field: "migration-planned binary path",
        comment: "# /opt/x",
        render: |v| {
            containerfile(&with_non_rpm(migration_item(
                &format!("/opt/{v}"),
                "elf",
                "",
                &[],
            )))
        },
    },
    Row {
        field: "migration-planned binary method",
        comment: "# /opt/tool",
        render: |v| containerfile(&with_non_rpm(migration_item("/opt/tool", v, "", &[]))),
    },
    Row {
        field: "migration-planned binary notes",
        comment: "# /opt/tool",
        render: |v| containerfile(&with_non_rpm(migration_item("/opt/tool", "elf", v, &[]))),
    },
    Row {
        field: "migration-planned binary shared library",
        comment: "# /opt/tool",
        render: |v| containerfile(&with_non_rpm(migration_item("/opt/tool", "elf", "", &[v]))),
    },
    Row {
        field: "static route file path",
        comment: "# Static route file:",
        render: |v| {
            containerfile(&with_network(NetworkSection {
                static_routes: vec![StaticRouteFile {
                    path: v.into(),
                    name: "route-eth0".into(),
                }],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "/etc/hosts addition",
        comment: "#   x",
        render: |v| {
            containerfile(&with_network(NetworkSection {
                hosts_additions: vec![v.into()],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "proxy source",
        comment: "# x",
        render: |v| {
            containerfile(&with_network(NetworkSection {
                proxy: vec![ProxyEntry {
                    source: v.into(),
                    line: "http_proxy=http://proxy:3128".into(),
                }],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "proxy line",
        comment: "# /etc/environment:",
        render: |v| {
            containerfile(&with_network(NetworkSection {
                proxy: vec![ProxyEntry {
                    source: "/etc/environment".into(),
                    line: v.into(),
                }],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "excluded secret path",
        comment: "#   x",
        render: |v| {
            containerfile(&with_redaction(redaction(
                v,
                "move it to a secret store",
                RedactionKind::Excluded,
            )))
        },
    },
    Row {
        field: "excluded secret remediation",
        comment: "#   /etc/app.conf",
        render: |v| {
            containerfile(&with_redaction(redaction(
                "/etc/app.conf",
                v,
                RedactionKind::Excluded,
            )))
        },
    },
    Row {
        field: "flagged secret path",
        comment: "#   x",
        render: |v| containerfile(&with_redaction(redaction(v, "", RedactionKind::Flagged))),
    },
    Row {
        field: "tuned profile",
        comment: "# FIXME: tuned profile name",
        render: |v| {
            let mut snap = InspectionSnapshot::new();
            snap.kernel_boot = Some(KernelBootSection {
                tuned_active: v.into(),
                tuned_disposition: FindingKind::included(),
                ..Default::default()
            });
            containerfile(&snap)
        },
    },
];

#[test]
fn containerfile_comments_keep_every_hostile_value_inside_the_comment() {
    check_rows(CONTAINERFILE_ROWS);
}

// ---------------------------------------------------------------------------
// Kickstart suggestion
// ---------------------------------------------------------------------------

fn with_fstab(device: &str, mount_point: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        fstab_entries: vec![FstabEntry {
            device: device.into(),
            mount_point: mount_point.into(),
            fstype: "nfs".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

const KICKSTART_ROWS: &[Row] = &[
    Row {
        field: "static connection name",
        comment: "# network --bootproto=static",
        render: |v| {
            render_kickstart(&with_network(NetworkSection {
                connections: vec![NMConnection {
                    name: v.into(),
                    method: "manual".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "source hostname",
        comment: "# FIXME: source hostname",
        render: |v| {
            let mut snap = with_network(NetworkSection::default());
            snap.meta.insert("hostname".into(), v.into());
            render_kickstart(&snap)
        },
    },
    Row {
        field: "static route file path",
        comment: "# FIXME: review x",
        render: |v| {
            render_kickstart(&with_network(NetworkSection {
                static_routes: vec![StaticRouteFile {
                    path: v.into(),
                    name: "route-eth0".into(),
                }],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "ip rule",
        comment: "# ip rule:",
        render: |v| {
            render_kickstart(&with_network(NetworkSection {
                ip_rules: vec![v.into()],
                ..Default::default()
            }))
        },
    },
    Row {
        field: "NFS device",
        comment: "# NFS:",
        render: |v| render_kickstart(&with_fstab(v, "/mnt/data")),
    },
    Row {
        field: "NFS mount point",
        comment: "# NFS:",
        render: |v| render_kickstart(&with_fstab("server:/export", v)),
    },
];

#[test]
fn kickstart_comments_keep_every_hostile_value_inside_the_comment() {
    check_rows(KICKSTART_ROWS);
}

// ---------------------------------------------------------------------------
// /var tmpfiles.d fragment
// ---------------------------------------------------------------------------

/// The `/var` fragment as `write_tmpfiles_conf` stages it, for a snapshot
/// whose one directory has a mode and is not created by anything the
/// export carries, so it renders here rather than as a `RUN mkdir`.
fn var_fragment(path: &str, backing: VarDirBacking) -> String {
    let mut snap = InspectionSnapshot::new();
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: path.into(),
            backing: Some(backing),
            mode: Some("0750".into()),
            ..Default::default()
        }],
        ..Default::default()
    });
    let dir = tempfile::tempdir().expect("tempdir");
    write_tmpfiles_conf(&snap, dir.path()).expect("tmpfiles fragment");
    std::fs::read_to_string(
        dir.path()
            .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
    )
    .expect("fragment written")
}

const VAR_FRAGMENT_ROWS: &[Row] = &[
    Row {
        field: "refused /var path",
        comment: "# FIXME: create /var/lib/x",
        render: |v| var_fragment(&format!("/var/lib/{v}"), VarDirBacking::Unbacked),
    },
    Row {
        field: "tmpfiles.d fragment the export does not carry",
        comment: "# /var/lib/app: created on the host by /etc/tmpfiles.d/x",
        render: |v| {
            var_fragment(
                "/var/lib/app",
                VarDirBacking::Tmpfiles {
                    conf: format!("/etc/tmpfiles.d/{v}"),
                    line: "d /var/lib/app 0750 root root -".into(),
                },
            )
        },
    },
];

#[test]
fn var_fragment_comments_keep_every_hostile_value_inside_the_comment() {
    check_rows(VAR_FRAGMENT_ROWS);
}

// ---------------------------------------------------------------------------
// Flatpak provisioning unit
// ---------------------------------------------------------------------------

/// The remote comment in the generated unit stays one line. Only that line
/// is checked: the same remote name also reaches the unit's `ExecStart`
/// directive, which is not a comment site.
#[test]
fn flatpak_unit_remote_comment_keeps_a_hostile_remote_inside_the_comment() {
    for sep in separators() {
        let remote = hostile(&sep);
        let mut snap = InspectionSnapshot::new();
        snap.containers = Some(ContainerSection {
            flatpak_apps: vec![FlatpakApp {
                app_id: "org.example.App".into(),
                branch: "stable".into(),
                remote: remote.clone(),
                disposition: FindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let dir = tempfile::tempdir().expect("tempdir");
        write_config_tree(&snap, dir.path()).expect("config tree");
        let unit = std::fs::read_to_string(dir.path().join("flatpak/flatpak-provision.service"))
            .expect("unit written");

        let comments: Vec<&str> = unit
            .split('\n')
            .filter(|l| l.starts_with("# Remote "))
            .collect();
        assert_eq!(comments.len(), 1, "{sep:?}:\n{unit}");
        assert!(
            !comments[0].contains(LINE_TERMINATORS) && comments[0].contains(PAYLOAD),
            "{sep:?}: the remote left its comment: {:?}",
            comments[0]
        );
    }
}
