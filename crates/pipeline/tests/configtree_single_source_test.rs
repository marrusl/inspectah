//! § 8.1: the kernel/boot content `write_config_tree` puts under `/etc`
//! has exactly one source -- `config.files`, gated on `is_included()`.
//!
//! The ungated kernel snippet block these tests exist to catch was deleted
//! one task earlier, when the fields it read were removed, so they are green
//! from the moment they are written. They stand as a regression guard
//! against a second writer for the paths they cover: a kernel-side path
//! rule that materializes a `config.files` entry the decision excluded.
//!
//! **Scope.** That is a claim about these paths, not about the function.
//! `write_config_tree` does still hold another ungated `/etc` writer: the
//! `systemd_timers` loop writes `etc/systemd/system/<name>.timer` and
//! `.service` for every `source == "local"` timer with no disposition
//! check at all. These fixtures carry no `scheduled_tasks` section and
//! would not catch it. Backlog:
//! `inspectah-configtree-writes-local-timers-with-no-decision`.
//!
//! Their teeth were measured by mutation, not by a red run — reintroducing a
//! path-rule writer inside the kernel/boot block turns
//! `a_config_exclusion_is_not_overridden_by_a_kernel_list` red while the rest
//! of the workspace stays green.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
use inspectah_core::types::kernelboot::KernelBootSection;
use inspectah_core::types::units::{SystemdDropIn, UnitSection};
use inspectah_core::types::{AdvisoryType, FindingKind};
use inspectah_pipeline::render::configtree::{
    Destination, PathError, StagedFileKind, WithheldWhy, staged_writes, write_config_tree,
};
use inspectah_pipeline::render::containerfile::render_containerfile;
use tempfile::TempDir;

/// The kernel snippet the config walker decided on. A `modules-load.d`
/// file is the contradiction case: the deleted block wrote this directory
/// on a path rule, and the config loop writes it on a decision.
const SNIPPET_PATH: &str = "/etc/modules-load.d/driftify.conf";
const SNIPPET_CONTENT: &str = "overlay\nbr_netfilter\n";

/// Where the config loop materializes `SNIPPET_PATH`.
const SNIPPET_DEST: &str = "config/etc/modules-load.d/driftify.conf";

/// A snapshot with a populated kernel/boot section carrying one
/// `config.files` entry for a kernel snippet at `disposition`.
///
/// `kernel_boot` is `Some` on purpose: the deleted writer lived inside
/// `if let Some(ref kb) = snap.kernel_boot`, so a fixture that leaves the
/// section unset cannot reach it and cannot catch its return.
///
/// `content` is a parameter because the advisory case carries none: the
/// config walker records a cross-tree symlink with `content:
/// String::new()`, and that empty content is what makes an admitted
/// advisory worse than an admitted exclusion.
fn snapshot_with(disposition: FindingKind, content: &str) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![ConfigFileEntry {
            path: SNIPPET_PATH.to_string(),
            category: ConfigCategory::KernelModule,
            content: content.to_string(),
            disposition,
            ..Default::default()
        }],
    });
    snap.kernel_boot = Some(KernelBootSection {
        // Every karg here is bootloader-owned, so `operator_kargs` yields
        // nothing and no kargs drop-in lands under `config/usr/`. That keeps
        // the materialized root list a statement about the config loop alone.
        cmdline: "ro quiet root=/dev/mapper/rhel-root".to_string(),
        tuned_active: "throughput-performance".to_string(),
        ..Default::default()
    });
    snap
}

#[test]
fn a_config_exclusion_is_not_overridden_by_a_kernel_list() {
    let snap = snapshot_with(FindingKind::excluded(), SNIPPET_CONTENT);
    let dir = TempDir::new().unwrap();
    let roots = write_config_tree(&snap, dir.path()).unwrap();

    assert_eq!(
        (dir.path().join(SNIPPET_DEST).exists(), roots),
        (false, Vec::<String>::new()),
        "an excluded config entry must not be materialized by a kernel-side \
         path rule, and must not promote a config/ root"
    );
}

/// The third disposition `is_included()` refuses, and the one with the
/// worst consequence if a writer ever admits it.
///
/// The config walker gives a cross-tree symlink `content: String::new()`,
/// so a writer that admitted advisories would lay a zero-byte file at the
/// real path and `COPY config/etc/ /etc/` would put it over whatever the
/// base image ships there. An advisory is also a finding the operator
/// cannot toggle, so such a file is one nobody chose.
#[test]
fn an_advisory_config_entry_is_not_written() {
    let snap = snapshot_with(
        FindingKind::advisory(
            AdvisoryType::CrossTreeSymlink,
            "Symlink crosses /etc → /var: config is stateful via /var persistence, \
             not subject to /etc 3-way merge",
        ),
        "",
    );
    let dir = TempDir::new().unwrap();
    let roots = write_config_tree(&snap, dir.path()).unwrap();

    assert_eq!(
        (dir.path().join(SNIPPET_DEST).exists(), roots),
        (false, Vec::<String>::new()),
        "an advisory config entry carries no decision and must not be \
         materialized, as a zero-byte file or otherwise"
    );
}

#[test]
fn an_included_config_entry_is_written() {
    let snap = snapshot_with(FindingKind::included(), SNIPPET_CONTENT);
    let dir = TempDir::new().unwrap();
    let roots = write_config_tree(&snap, dir.path()).unwrap();

    let written = std::fs::read_to_string(dir.path().join(SNIPPET_DEST))
        .expect("an included kernel snippet is materialized under config/");
    assert_eq!(
        (written.as_str(), roots),
        ("overlay\nbr_netfilter\n", vec!["etc".to_string()]),
        "the config loop is the path that writes the snippet"
    );
}

// --- § 10.2: `staged_writes` is the stager's decision ---------------------
//
// The snapshots below are built by hand, as render-rule tests may be: the
// conflicts and zero-length unit files they hold come from imported or
// hand-edited snapshots and fleet aggregates, which the collector does
// not produce (since the units inspector records a zero-length unit file
// as a mask, it offers no config entry or drop-in for one).

/// The administrator's unit file the conflict cases share.
const WORKER_UNIT: &str = "/etc/systemd/system/worker.service";
/// A config entry that needs `WORKER_UNIT` as a directory.
const WORKER_CHILD: &str = "/etc/systemd/system/worker.service/extra.conf";

/// An included config entry at `path`.
fn config_entry(path: &str, content: &str) -> ConfigFileEntry {
    ConfigFileEntry {
        path: path.to_string(),
        category: ConfigCategory::Other,
        content: content.to_string(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// An included drop-in row at `path`, recorded for `unit`.
fn drop_in(unit: &str, path: &str, content: &str) -> SystemdDropIn {
    SystemdDropIn {
        unit: unit.to_string(),
        path: path.to_string(),
        content: content.to_string(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

/// A snapshot with these config entries and drop-in rows and nothing else.
fn staged_snapshot(
    files: Vec<ConfigFileEntry>,
    drop_ins: Vec<SystemdDropIn>,
) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection { files });
    if !drop_ins.is_empty() {
        snap.units = Some(UnitSection {
            drop_ins,
            ..Default::default()
        });
    }
    snap
}

fn dest(path: &str) -> Destination {
    Destination::new(path).expect("a valid destination")
}

/// The export's tree and Containerfile, rendered from the roots the
/// stager returned, as `render_all` does.
fn export(snap: &InspectionSnapshot) -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    let roots = write_config_tree(snap, dir.path()).expect("the config tree writes");
    let containerfile = render_containerfile(snap, Some(&roots), None);
    (dir, containerfile)
}

/// The FIXME line that names `path`, if any.
fn fixme_naming<'a>(containerfile: &'a str, path: &str) -> Option<&'a str> {
    containerfile
        .lines()
        .find(|l| l.starts_with("# FIXME:") && l.contains(path))
}

/// T15: a file and a file under it as a directory, in either order: neither
/// is written, both are withheld as `Conflict` and named in a FIXME.
/// Without the child, the file is written (positive control).
#[test]
fn t15_a_file_and_a_file_under_it_are_both_withheld_in_either_order() {
    for (label, files) in [
        (
            "child first",
            vec![
                config_entry(WORKER_CHILD, "[Service]\n"),
                config_entry(WORKER_UNIT, "[Unit]\n"),
            ],
        ),
        (
            "file first",
            vec![
                config_entry(WORKER_UNIT, "[Unit]\n"),
                config_entry(WORKER_CHILD, "[Service]\n"),
            ],
        ),
    ] {
        let snap = staged_snapshot(files, vec![]);
        let staged = staged_writes(&snap);
        assert!(
            staged.writes.is_empty(),
            "{label}: {:?}",
            staged.writes.keys()
        );
        let mut withheld: Vec<_> = staged
            .withheld
            .iter()
            .map(|(d, k, w)| (d.as_str(), *k, w.clone()))
            .collect();
        withheld.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(
            withheld,
            vec![
                (
                    WORKER_UNIT,
                    StagedFileKind::ConfigFile,
                    WithheldWhy::Conflict {
                        other: dest(WORKER_CHILD)
                    }
                ),
                (
                    WORKER_CHILD,
                    StagedFileKind::ConfigFile,
                    WithheldWhy::Conflict {
                        other: dest(WORKER_UNIT)
                    }
                ),
            ],
            "{label}"
        );

        let (dir, containerfile) = export(&snap);
        assert!(
            !dir.path().join("config").join(&WORKER_UNIT[1..]).exists(),
            "{label}"
        );
        assert!(
            !dir.path().join("config").join(&WORKER_CHILD[1..]).exists(),
            "{label}"
        );
        for path in [WORKER_UNIT, WORKER_CHILD] {
            assert!(
                fixme_naming(&containerfile, path).is_some(),
                "{label}: no FIXME names {path}:\n{containerfile}"
            );
        }
    }

    let snap = staged_snapshot(vec![config_entry(WORKER_UNIT, "[Unit]\n")], vec![]);
    let staged = staged_writes(&snap);
    assert!(staged.withheld.is_empty(), "{:?}", staged.withheld);
    assert!(staged.writes.contains_key(&dest(WORKER_UNIT)));
    let (dir, containerfile) = export(&snap);
    let written = std::fs::read_to_string(dir.path().join("config").join(&WORKER_UNIT[1..]))
        .expect("the unit file is written without the child");
    assert_eq!(written, "[Unit]\n");
    assert!(
        fixme_naming(&containerfile, WORKER_UNIT).is_none(),
        "{containerfile}"
    );
}

/// Two entries at one destination, spelled two ways: both are withheld,
/// neither is written, both are named. One alone is written.
#[test]
fn two_entries_at_one_destination_are_both_withheld() {
    let snap = staged_snapshot(
        vec![
            config_entry("/etc/app/app.conf", "a=1\n"),
            config_entry("/etc//app/./app.conf", "a=2\n"),
        ],
        vec![],
    );
    let staged = staged_writes(&snap);
    assert!(staged.writes.is_empty(), "{:?}", staged.writes.keys());
    assert_eq!(staged.withheld.len(), 2, "{:?}", staged.withheld);
    assert!(
        staged
            .withheld
            .iter()
            .all(|(d, _, w)| d == &dest("/etc/app/app.conf")
                && *w
                    == WithheldWhy::Conflict {
                        other: dest("/etc/app/app.conf")
                    })
    );
    let (dir, containerfile) = export(&snap);
    assert!(!dir.path().join("config/etc/app/app.conf").exists());
    assert!(
        fixme_naming(&containerfile, "/etc/app/app.conf").is_some(),
        "{containerfile}"
    );

    let snap = staged_snapshot(vec![config_entry("/etc/app/app.conf", "a=1\n")], vec![]);
    let (dir, _) = export(&snap);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config/etc/app/app.conf")).unwrap(),
        "a=1\n"
    );
}

/// A destination is a normalized absolute path: two spellings of one file
/// are one destination, and `..`, a NUL byte, a control character and a
/// path that names no file are refused, each naming the rule.
#[test]
fn a_destination_is_one_spelling_of_one_path() {
    let plain = dest("/etc/systemd/system/bootc-x.timer");
    for spelling in [
        "/etc/systemd/./system//bootc-x.timer",
        "//etc/systemd/system/bootc-x.timer",
        "/etc/systemd/system/bootc-x.timer/",
        "etc/systemd/system/bootc-x.timer",
    ] {
        assert_eq!(dest(spelling), plain, "{spelling}");
    }
    assert_eq!(plain.as_str(), "/etc/systemd/system/bootc-x.timer");

    for (path, refusal) in [
        ("/etc/systemd/system/../../shadow", PathError::Traversal),
        ("/etc/a\0b", PathError::NulByte),
        ("/etc/a\nb", PathError::LineBreakOrControl),
        ("/./", PathError::NoFileName),
        ("", PathError::NoFileName),
    ] {
        assert_eq!(Destination::new(path), Err(refusal), "{path:?}");
    }
    assert!(
        PathError::Traversal.to_string().contains("traversal"),
        "the refusal names the rule: {}",
        PathError::Traversal
    );

    // Through the stager: a `..` path is refused, named, and not written.
    let snap = staged_snapshot(vec![config_entry("/etc/../root/x.conf", "x\n")], vec![]);
    let staged = staged_writes(&snap);
    assert!(staged.writes.is_empty());
    assert_eq!(
        staged
            .refused
            .iter()
            .map(|(k, p, e)| (*k, p.as_str(), *e))
            .collect::<Vec<_>>(),
        vec![(
            StagedFileKind::ConfigFile,
            "/etc/../root/x.conf",
            PathError::Traversal
        )]
    );
    let (dir, containerfile) = export(&snap);
    assert!(!dir.path().join("config/root/x.conf").exists());
    assert!(
        fixme_naming(&containerfile, "/etc/../root/x.conf").is_some(),
        "{containerfile}"
    );
}

/// T12, the staging half: a zero-length file at a unit path is a mask, so
/// it is withheld whichever section offers it, however the path is spelled
/// and whatever unit a drop-in row names. A non-empty file there, and a
/// zero-length file that is not at a unit path, are written.
#[test]
fn t12_a_zero_length_unit_file_is_withheld_from_every_offer() {
    const UNIT: &str = "/etc/systemd/system/kdump.service";
    for (label, spelling, as_drop_in, unit_field) in [
        ("config entry", UNIT, false, ""),
        ("drop-in", UNIT, true, "kdump.service"),
        ("drop-in naming another unit", UNIT, true, "sshd.service"),
        (
            "interior .",
            "/etc/systemd/./system/kdump.service",
            false,
            "",
        ),
        (
            "doubled separator",
            "/etc/systemd//system/kdump.service",
            true,
            "kdump.service",
        ),
    ] {
        let snap = if as_drop_in {
            staged_snapshot(vec![], vec![drop_in(unit_field, spelling, "")])
        } else {
            staged_snapshot(vec![config_entry(spelling, "")], vec![])
        };
        let kind = if as_drop_in {
            StagedFileKind::DropIn
        } else {
            StagedFileKind::ConfigFile
        };
        let staged = staged_writes(&snap);
        assert!(
            staged.writes.is_empty(),
            "{label}: {:?}",
            staged.writes.keys()
        );
        assert_eq!(
            staged.withheld,
            vec![(dest(UNIT), kind, WithheldWhy::ZeroLengthUnitFile)],
            "{label}"
        );
        let (dir, containerfile) = export(&snap);
        for root in ["config", "drop-ins"] {
            assert!(
                !dir.path().join(root).join(&UNIT[1..]).exists(),
                "{label}: {root}"
            );
        }
        assert!(
            fixme_naming(&containerfile, UNIT).is_some(),
            "{label}:\n{containerfile}"
        );
    }

    // Positive controls: the same offers with content, and empty files
    // that are not unit files.
    let snap = staged_snapshot(
        vec![
            config_entry("/etc/systemd/./system/kdump.service", "[Unit]\n"),
            config_entry("/etc/systemd/system/kdump.service.d/empty.conf", ""),
            config_entry("/etc/app/empty.conf", ""),
        ],
        vec![drop_in(
            "sshd.service",
            "/etc/systemd/system/sshd.service",
            "[Unit]\n",
        )],
    );
    let staged = staged_writes(&snap);
    assert!(staged.withheld.is_empty(), "{:?}", staged.withheld);
    let (dir, _) = export(&snap);
    for written in [
        "config/etc/systemd/system/kdump.service",
        "config/etc/systemd/system/kdump.service.d/empty.conf",
        "config/etc/app/empty.conf",
        "drop-ins/etc/systemd/system/sshd.service",
    ] {
        assert!(dir.path().join(written).is_file(), "{written} is written");
    }
}

/// A write that does not land fails the export, naming the path: here the
/// export directory's `config/` exists and is read-only.
#[test]
fn a_write_into_a_read_only_directory_fails_the_export() {
    use std::os::unix::fs::PermissionsExt;

    let snap = staged_snapshot(vec![config_entry("/etc/app/app.conf", "a=1\n")], vec![]);
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes through a read-only directory, so the case cannot be made.
    if std::fs::create_dir(config.join("probe")).is_ok() {
        eprintln!("skipping: running as root, a read-only directory does not refuse writes");
        return;
    }

    let result = write_config_tree(&snap, dir.path());
    std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = result.expect_err("a write into a read-only directory fails the export");
    assert!(
        err.to_string().contains("etc/app"),
        "the error names the path: {err}"
    );
}
