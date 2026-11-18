//! § 8.1: the kernel/boot content `write_config_tree` puts under `/etc`
//! has exactly one source -- `config.files`, gated on `is_included()`.
//!
//! The ungated kernel snippet block these tests exist to catch was deleted
//! one task earlier, when the fields it read were removed, so they are green
//! from the moment they are written. They stand as a regression guard
//! against a second writer for the paths they cover: a kernel-side path
//! rule that materializes a `config.files` entry the decision excluded.
//!
//! **Scope.** The kernel/boot claim above is about those paths, not about
//! the function. Local timer files under `etc/systemd/system/` are covered
//! separately, by `a_local_timers_file_is_written_from_its_own_row_only`
//! in this file: their only writer is their own `config.files` row.
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

/// The drop-ins section's `COPY` line.
const DROP_INS_COPY: &str = "COPY drop-ins/etc/systemd/system/ /etc/systemd/system/";
/// The scheduled section's `COPY` line.
const SCHEDULED_COPY: &str = "COPY config/etc/systemd/system/ /etc/systemd/system/";

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
        // Nothing was written under `drop-ins/`, so nothing copies it.
        assert!(
            !containerfile.contains(DROP_INS_COPY),
            "{label}: a COPY of a drop-in tree the export did not write:\n{containerfile}"
        );
    }

    // Reversed control: the withheld drop-in beside one that is staged.
    // The COPY is back, and the count is the staged one.
    let snap = staged_snapshot(
        vec![],
        vec![
            drop_in("kdump.service", UNIT, ""),
            drop_in(
                "sshd.service",
                "/etc/systemd/system/sshd.service.d/override.conf",
                "[Service]\nNice=5\n",
            ),
        ],
    );
    let (dir, containerfile) = export(&snap);
    assert!(
        dir.path()
            .join("drop-ins/etc/systemd/system/sshd.service.d/override.conf")
            .is_file()
    );
    assert!(containerfile.contains(DROP_INS_COPY), "{containerfile}");
    assert!(
        containerfile.contains("# 1 unit drop-in(s)"),
        "the count is what the export writes:\n{containerfile}"
    );

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

/// A local timer whose file a drop-in row carries (an administrator's copy
/// of a vendor timer): the file is staged under `drop-ins/` and nothing is
/// written under `config/etc/systemd/system/`, so the scheduled section
/// does not copy that directory. Reversed control: a generated unit is
/// written there and the `COPY` is present.
#[test]
fn the_scheduled_copy_follows_what_the_export_writes() {
    use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer};
    const TIMER: &str = "/etc/systemd/system/backup.timer";
    const BODY: &str = "[Timer]\nOnCalendar=daily\n";
    let with_timer = |mut snap: InspectionSnapshot| {
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "backup".into(),
                source: "local".into(),
                path: TIMER.into(),
                on_calendar: "daily".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    };

    let snap = with_timer(staged_snapshot(
        vec![],
        vec![drop_in("backup.timer", TIMER, BODY)],
    ));
    let (dir, containerfile) = export(&snap);
    assert!(dir.path().join("drop-ins").join(&TIMER[1..]).is_file());
    assert!(!dir.path().join("config/etc/systemd/system").exists());
    assert!(
        !containerfile.contains(SCHEDULED_COPY),
        "a COPY of a directory the export did not write:\n{containerfile}"
    );

    let snap = with_generated(
        staged_snapshot(vec![], vec![drop_in("backup.timer", TIMER, BODY)]),
        &[(GENERATED_TIMER, GENERATED_SERVICE)],
    );
    let (dir, containerfile) = export(&snap);
    assert!(dir.path().join("config").join(&CRON_TIMER[1..]).is_file());
    assert!(containerfile.contains(SCHEDULED_COPY), "{containerfile}");
}

// --- A generated unit and a collected file of its name (§ 10.2) --------

/// The flatpak provisioning service's image path.
const FLATPAK_UNIT: &str = "/etc/systemd/system/flatpak-provision.service";
const FLATPAK_COPY: &str =
    "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service";
const FLATPAK_ENABLE: &str = "RUN systemctl enable -- \\\n    'flatpak-provision.service'";

fn with_flatpak(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
    snap.containers = Some(ContainerSection {
        flatpak_apps: vec![FlatpakApp {
            app_id: "org.example.App".into(),
            remote: "flathub".into(),
            remote_url: "https://dl.flathub.org/repo/".into(),
            branch: "stable".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap
}

/// The generated provisioning service, as the export writes it alone.
fn generated_flatpak_service() -> String {
    let (dir, _) = export(&with_flatpak(staged_snapshot(vec![], vec![])));
    std::fs::read_to_string(dir.path().join("flatpak/flatpak-provision.service"))
        .expect("the generated service is staged")
}

/// A rescan of a host built from an inspectah image records a collected
/// `flatpak-provision.service`. The generated one is withheld as
/// `GeneratedNameCollision` whatever its content, the collected file is
/// written under `config/`, and the containers section neither copies nor
/// enables the generated service and names the declined request in a
/// FIXME (§ 10.2). Control: with no collected file, the generated service
/// is staged, copied and enabled after its `COPY`.
#[test]
fn a_collected_flatpak_service_withholds_the_generated_one() {
    let generated = generated_flatpak_service();
    for (label, collected) in [
        (
            "different content",
            "[Unit]\nDescription=older provisioning\n".to_string(),
        ),
        ("identical content", generated.clone()),
    ] {
        let snap = with_flatpak(staged_snapshot(
            vec![config_entry(FLATPAK_UNIT, &collected)],
            vec![],
        ));
        let staged = staged_writes(&snap);
        assert_eq!(
            staged
                .withheld
                .iter()
                .map(|(at, kind, why)| (at.as_str(), *kind, why.name()))
                .collect::<Vec<_>>(),
            [(
                FLATPAK_UNIT,
                StagedFileKind::FlatpakService,
                "GeneratedNameCollision"
            )],
            "{label}"
        );
        let (dir, containerfile) = export(&snap);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config").join(&FLATPAK_UNIT[1..])).unwrap(),
            collected,
            "{label}: the collected file is written under config/"
        );
        assert!(
            !dir.path()
                .join("flatpak/flatpak-provision.service")
                .exists(),
            "{label}: the generated file is not written"
        );
        assert!(
            !containerfile.contains(FLATPAK_COPY),
            "{label}:\n{containerfile}"
        );
        assert!(
            !containerfile.contains(FLATPAK_ENABLE),
            "{label}:\n{containerfile}"
        );
        assert!(
            containerfile
                .lines()
                .any(|l| l.starts_with("# FIXME: flatpak-provision.service (")),
            "{label}:\n{containerfile}"
        );
    }

    let (dir, containerfile) = export(&with_flatpak(staged_snapshot(vec![], vec![])));
    assert!(
        dir.path()
            .join("flatpak/flatpak-provision.service")
            .is_file()
    );
    let copy = containerfile.find(FLATPAK_COPY).expect("the COPY");
    let enable = containerfile.find(FLATPAK_ENABLE).expect("the enable");
    assert!(copy < enable, "{containerfile}");
}

fn with_generated(mut snap: InspectionSnapshot, units: &[(&str, &str)]) -> InspectionSnapshot {
    use inspectah_core::types::scheduled::{GeneratedTimerUnit, ScheduledTaskSection};
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        generated_timer_units: units
            .iter()
            .map(|(timer, service)| GeneratedTimerUnit {
                name: "cron-backup".into(),
                timer_content: (*timer).into(),
                service_content: (*service).into(),
                cron_expr: "0 1 * * *".into(),
                disposition: FindingKind::included(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    });
    snap
}

/// The configuration section's `COPY` of the `/etc` tree, which carries the
/// host's own unit files.
const CONFIG_ETC_COPY: &str = "COPY config/etc/ /etc/";
const CRON_TIMER: &str = "/etc/systemd/system/cron-backup.timer";
const CRON_SERVICE: &str = "/etc/systemd/system/cron-backup.service";
/// The cron-generated pair named `cron-backup`.
const GENERATED_TIMER: &str = "[Timer]\nOnCalendar=*-*-* 01:00:00\n";
const GENERATED_SERVICE: &str = "[Service]\nExecStart=/bin/true\n";
/// The host's own `cron-backup` pair in `/etc/systemd/system`, as a rescan
/// of a host built from an inspectah image finds it.
const HOST_TIMER: &str = "[Timer]\nOnCalendar=weekly\n";
const HOST_SERVICE: &str = "[Service]\nExecStart=/usr/local/bin/backup\n";

/// The `source: "local"` description the collector records for every timer
/// in `/etc/systemd/system`, here the host's own `cron-backup` timer. It
/// controls nothing and writes nothing (§ 8).
fn with_host_timer(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    use inspectah_core::types::scheduled::SystemdTimer;
    snap.scheduled_tasks
        .get_or_insert_with(Default::default)
        .systemd_timers
        .push(SystemdTimer {
            name: "cron-backup".into(),
            source: "local".into(),
            path: CRON_TIMER[1..].into(),
            on_calendar: "weekly".into(),
            exec_start: "/usr/local/bin/backup".into(),
            ..Default::default()
        });
    snap
}

/// A generated cron unit named like a file the host has never replaces it,
/// and neither of its files lands beside the host's: where a config entry
/// or a drop-in row records the host's file at either path of the
/// generated pair, both generated files are withheld as
/// `GeneratedNameCollision` (§ 10.2, "the paired service of a generated
/// timer included"), the host's content is what the export stages, the
/// generated request is declined with its FIXME and nothing enables the
/// timer from the scheduled section. With the host's service row
/// excluded, as an operator does on a rescan, no file is at the service's
/// path in either tree; the same rescan with the row included (the first
/// case) is its control. The timer's description records no file, so
/// with it alone the generated pair lands and is enabled, as it does with
/// no host file of that name (the control).
#[test]
fn a_generated_timer_never_replaces_the_hosts_own_timer() {
    let generated = &[(GENERATED_TIMER, GENERATED_SERVICE)];
    let excluded = |path: &str, content: &str| {
        let mut entry = config_entry(path, content);
        entry.disposition = FindingKind::excluded();
        entry
    };
    for (label, snap, timer_root, timer, service, copy) in [
        (
            "config entries and the description, as a scan records them",
            with_host_timer(with_generated(
                staged_snapshot(
                    vec![
                        config_entry(CRON_TIMER, HOST_TIMER),
                        config_entry(CRON_SERVICE, HOST_SERVICE),
                    ],
                    vec![],
                ),
                generated,
            )),
            "config",
            HOST_TIMER,
            Some(HOST_SERVICE),
            CONFIG_ETC_COPY,
        ),
        (
            "the service's config row excluded on a rescan",
            with_host_timer(with_generated(
                staged_snapshot(
                    vec![
                        config_entry(CRON_TIMER, HOST_TIMER),
                        excluded(CRON_SERVICE, HOST_SERVICE),
                    ],
                    vec![],
                ),
                generated,
            )),
            "config",
            HOST_TIMER,
            None,
            CONFIG_ETC_COPY,
        ),
        (
            "a drop-in row and the service's config entry",
            with_host_timer(with_generated(
                staged_snapshot(
                    vec![config_entry(CRON_SERVICE, HOST_SERVICE)],
                    vec![drop_in("cron-backup.timer", CRON_TIMER, HOST_TIMER)],
                ),
                generated,
            )),
            "drop-ins",
            HOST_TIMER,
            Some(HOST_SERVICE),
            DROP_INS_COPY,
        ),
        (
            "the timer's config entry alone",
            with_generated(
                staged_snapshot(vec![config_entry(CRON_TIMER, HOST_TIMER)], vec![]),
                generated,
            ),
            "config",
            HOST_TIMER,
            None,
            CONFIG_ETC_COPY,
        ),
        (
            "the description alone",
            with_host_timer(with_generated(staged_snapshot(vec![], vec![]), generated)),
            "config",
            GENERATED_TIMER,
            Some(GENERATED_SERVICE),
            SCHEDULED_COPY,
        ),
        (
            "control: no host file of that name",
            with_generated(staged_snapshot(vec![], vec![]), generated),
            "config",
            GENERATED_TIMER,
            Some(GENERATED_SERVICE),
            SCHEDULED_COPY,
        ),
    ] {
        let collided = timer == HOST_TIMER;
        let staged = staged_writes(&snap);
        if collided {
            assert!(
                !staged.withheld.is_empty()
                    && staged.withheld.iter().all(|(_, kind, why)| {
                        *kind == StagedFileKind::GeneratedUnit
                            && why.name() == "GeneratedNameCollision"
                    }),
                "{label}: {:?}",
                staged.withheld
            );
        } else {
            assert!(staged.withheld.is_empty(), "{label}: {:?}", staged.withheld);
        }
        let (dir, containerfile) = export(&snap);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(timer_root).join(&CRON_TIMER[1..])).unwrap(),
            timer,
            "{label}: the timer that lands"
        );
        match service {
            Some(service) => assert_eq!(
                std::fs::read_to_string(dir.path().join("config").join(&CRON_SERVICE[1..]))
                    .unwrap(),
                service,
                "{label}: the service that lands"
            ),
            None => {
                for tree in ["config", "drop-ins"] {
                    assert!(
                        !dir.path().join(tree).join(&CRON_SERVICE[1..]).exists(),
                        "{label}: a file at the service's path under {tree}/"
                    );
                }
            }
        }
        if timer_root != "config" {
            // The scheduled section's COPY comes after the drop-ins COPY,
            // so a generated file under config/ would replace the host's.
            assert!(
                !dir.path().join("config").join(&CRON_TIMER[1..]).exists(),
                "{label}: nothing under config/ to copy over the host's timer"
            );
        }
        assert!(containerfile.contains(copy), "{label}:\n{containerfile}");
        assert_eq!(
            containerfile.contains("RUN systemctl enable -- \\\n    'cron-backup.timer'"),
            !collided,
            "{label}: the scheduled section enables only its own pair:\n{containerfile}"
        );
        assert_eq!(
            containerfile
                .lines()
                .any(|l| l.starts_with("# FIXME: cron-backup.timer (")),
            collided,
            "{label}: the declined request's FIXME:\n{containerfile}"
        );
    }
}

/// Two generated units of one name (one cron file with two lines):
/// `staged_writes` keeps the last, as the stager has always written them,
/// so the later one is what lands, and nothing is withheld.
#[test]
fn two_generated_units_of_one_name_are_written_in_order() {
    let snap = with_generated(
        staged_snapshot(vec![], vec![]),
        &[
            ("[Timer]\nfirst\n", "[Service]\nfirst\n"),
            ("[Timer]\nsecond\n", "[Service]\nsecond\n"),
        ],
    );
    let staged = staged_writes(&snap);
    assert!(staged.withheld.is_empty(), "{:?}", staged.withheld);
    let (dir, _) = export(&snap);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("config").join(&CRON_TIMER[1..])).unwrap(),
        "[Timer]\nsecond\n"
    );
}

/// Control: two collected entries with different content at the flatpak
/// service's path, both from one section, are both withheld, and the FIXME
/// says another offer there has different content.
#[test]
fn two_collected_files_at_a_generated_units_path_are_withheld() {
    let snap = staged_snapshot(
        vec![
            config_entry(FLATPAK_UNIT, "[Unit]\nDescription=a\n"),
            config_entry(
                "/etc/systemd//system/flatpak-provision.service",
                "[Unit]\nDescription=b\n",
            ),
        ],
        vec![],
    );
    let staged = staged_writes(&snap);
    assert!(staged.writes.is_empty(), "{:?}", staged.writes.keys());
    assert_eq!(staged.withheld.len(), 2, "{:?}", staged.withheld);
    let (_, containerfile) = export(&snap);
    let fixme = fixme_naming(&containerfile, FLATPAK_UNIT).expect("named in a FIXME");
    assert!(
        fixme.contains("another offer at this path has different content; exclude one of the two"),
        "{fixme}"
    );
}

// --- The identical-content and local-timer rules -------------------------

/// The config walk records an unowned file under `/etc`, and the repo,
/// GPG key or firewall inventory records the same file again. Same bytes:
/// one write, nothing withheld. Different bytes (a fleet aggregate keeps
/// the config entry's variants and one representative inventory row): the
/// config entry's content is the one write and nothing is withheld. With no
/// config entry included, the inventory copy is written. Two included
/// config entries with different content still conflict.
#[test]
fn a_file_two_sections_record_with_one_content_is_written_once() {
    use inspectah_core::types::network::{FirewallZone, NetworkSection};
    use inspectah_core::types::rpm::{RepoFile, RpmSection};
    const REPO: &str = "/etc/yum.repos.d/custom.repo";
    const KEY: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom";
    const ZONE: &str = "/etc/firewalld/zones/public.xml";

    let offer = |kind: StagedFileKind, path: &str, content: &str, config: &str| {
        let mut snap = staged_snapshot(vec![config_entry(path, config)], vec![]);
        match kind {
            StagedFileKind::RepoFile | StagedFileKind::GpgKey => {
                let file = RepoFile {
                    path: path.into(),
                    content: content.into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                };
                let mut rpm = RpmSection::default();
                if kind == StagedFileKind::RepoFile {
                    rpm.repo_files.push(file);
                } else {
                    rpm.gpg_keys.push(file);
                }
                snap.rpm = Some(rpm);
            }
            _ => {
                snap.network = Some(NetworkSection {
                    firewall_zones: vec![FirewallZone {
                        path: path.into(),
                        name: "public".into(),
                        content: content.into(),
                        disposition: FindingKind::included(),
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
        }
        snap
    };

    for (kind, path, body) in [
        (
            StagedFileKind::RepoFile,
            REPO,
            "[custom]\nbaseurl=https://repo.example/\n",
        ),
        (
            StagedFileKind::GpgKey,
            KEY,
            "-----BEGIN PGP PUBLIC KEY BLOCK-----\n",
        ),
        (
            StagedFileKind::FirewallZone,
            ZONE,
            "<zone><service name=\"ssh\"/></zone>\n",
        ),
    ] {
        let snap = offer(kind, path, body, body);
        let staged = staged_writes(&snap);
        assert!(
            staged.withheld.is_empty(),
            "{kind:?}: {:?}",
            staged.withheld
        );
        assert_eq!(staged.writes.len(), 1, "{kind:?}: one write");
        let (dir, containerfile) = export(&snap);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config").join(&path[1..])).unwrap(),
            body,
            "{kind:?}"
        );
        assert!(
            fixme_naming(&containerfile, path).is_none(),
            "{kind:?}:\n{containerfile}"
        );
        if kind == StagedFileKind::RepoFile {
            assert!(
                containerfile.contains("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/"),
                "{containerfile}"
            );
        }

        // The two records disagree: the config entry decides the content.
        let snap = offer(kind, path, body, "changed by hand\n");
        let staged = staged_writes(&snap);
        assert!(
            staged.withheld.is_empty(),
            "{kind:?}: {:?}",
            staged.withheld
        );
        assert_eq!(
            staged
                .writes
                .get(&dest(path))
                .map(|w| (w.source, w.content.as_ref())),
            Some((StagedFileKind::ConfigFile, "changed by hand\n")),
            "{kind:?}"
        );
        let (dir, containerfile) = export(&snap);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config").join(&path[1..])).unwrap(),
            "changed by hand\n",
            "{kind:?}"
        );
        assert!(
            fixme_naming(&containerfile, path).is_none(),
            "{kind:?}:\n{containerfile}"
        );
        if kind == StagedFileKind::RepoFile {
            assert!(
                containerfile.contains("COPY config/etc/yum.repos.d/ /etc/yum.repos.d/"),
                "{containerfile}"
            );
        }

        // With the config entry excluded, the inventory copy is written.
        let mut snap = offer(kind, path, body, "changed by hand\n");
        for f in &mut snap.config.as_mut().unwrap().files {
            f.disposition = FindingKind::excluded();
        }
        let staged = staged_writes(&snap);
        assert!(staged.withheld.is_empty(), "{kind:?}");
        assert_eq!(
            staged
                .writes
                .get(&dest(path))
                .map(|w| (w.source, w.content.as_ref())),
            Some((kind, body)),
            "{kind:?}"
        );

        // Two included config entries that disagree still conflict. The
        // inventory copy yields to them, so only they are withheld.
        let mut snap = offer(kind, path, body, "changed by hand\n");
        snap.config
            .as_mut()
            .unwrap()
            .files
            .push(config_entry(path, "changed again\n"));
        let staged = staged_writes(&snap);
        assert!(
            staged.writes.is_empty(),
            "{kind:?}: {:?}",
            staged.writes.keys()
        );
        assert_eq!(staged.withheld.len(), 2, "{kind:?}: {:?}", staged.withheld);
        assert!(
            staged
                .withheld
                .iter()
                .all(|(_, k, _)| *k == StagedFileKind::ConfigFile),
            "{kind:?}: {:?}",
            staged.withheld
        );
    }
}

/// A local timer's file is written from the row that records it, a config
/// entry or a drop-in row when it shadows a vendor timer, and from nothing
/// else (§ 8): with that row excluded, or with the timer's description
/// alone, nothing is written at its path. Control: the included row's
/// content lands.
#[test]
fn a_local_timers_file_is_written_from_its_own_row_only() {
    use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer};
    const TIMER: &str = "/etc/systemd/system/report.timer";
    const COLLECTED: &str = "[Timer]\nOnCalendar=daily\n";
    let with_timer = |mut snap: InspectionSnapshot| {
        snap.scheduled_tasks = Some(ScheduledTaskSection {
            systemd_timers: vec![SystemdTimer {
                name: "report".into(),
                source: "local".into(),
                path: TIMER.into(),
                on_calendar: "daily".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    };
    let excluded = |mut snap: InspectionSnapshot| {
        if let Some(config) = snap.config.as_mut() {
            for f in &mut config.files {
                f.disposition = FindingKind::excluded();
            }
        }
        if let Some(units) = snap.units.as_mut() {
            for d in &mut units.drop_ins {
                d.disposition = FindingKind::excluded();
            }
        }
        snap
    };

    for (label, snap, root) in [
        (
            "config entry",
            with_timer(staged_snapshot(
                vec![config_entry(TIMER, COLLECTED)],
                vec![],
            )),
            "config",
        ),
        (
            "drop-in row",
            with_timer(staged_snapshot(
                vec![],
                vec![drop_in("report.timer", TIMER, COLLECTED)],
            )),
            "drop-ins",
        ),
    ] {
        let staged = staged_writes(&snap);
        assert!(staged.withheld.is_empty(), "{label}: {:?}", staged.withheld);
        let (dir, _) = export(&snap);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(root).join(&TIMER[1..])).unwrap(),
            COLLECTED,
            "{label}"
        );

        let (dir, _) = export(&excluded(snap));
        for tree in ["config", "drop-ins"] {
            assert!(
                !dir.path().join(tree).join(&TIMER[1..]).exists(),
                "{label} excluded: written under {tree}/"
            );
        }
    }

    let (dir, _) = export(&with_timer(staged_snapshot(vec![], vec![])));
    for tree in ["config", "drop-ins"] {
        assert!(
            !dir.path().join(tree).join(&TIMER[1..]).exists(),
            "the description alone: written under {tree}/"
        );
    }
}

/// A write that does not land fails the export without needing
/// permissions: a regular file sits where the stager needs a directory.
#[test]
fn a_file_where_the_stager_needs_a_directory_fails_the_export() {
    let snap = staged_snapshot(vec![config_entry("/etc/app/app.conf", "a=1\n")], vec![]);
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("config/etc")).unwrap();
    std::fs::write(dir.path().join("config/etc/app"), "in the way").unwrap();

    let err = write_config_tree(&snap, dir.path())
        .expect_err("a write whose parent is a file fails the export");
    assert!(
        err.to_string().contains("etc/app"),
        "the error names the path: {err}"
    );
}

// --- The generated kargs file goes through the decision ------------------

/// Where the export writes the kernel arguments inspectah migrates.
const KARGS: &str = "/usr/lib/bootc/kargs.d/inspectah-migrated.toml";
const KARGS_COPY: &str =
    "COPY config/usr/lib/bootc/kargs.d/inspectah-migrated.toml /usr/lib/bootc/kargs.d/";

/// `snap` with an operator kernel argument on the cmdline, so the export
/// generates the kargs file.
fn with_kargs(mut snap: InspectionSnapshot) -> InspectionSnapshot {
    snap.kernel_boot = Some(KernelBootSection {
        cmdline: "ro quiet hugepages=16".to_string(),
        ..Default::default()
    });
    snap
}

/// The generated kargs file is an offer of the stager's decision: alone, it
/// is a write whose `COPY` is rendered. Beside a collected file at its path
/// with other content, or under one (the path needed as a directory), both
/// are withheld as a `Conflict` and named, nothing lands at the path and
/// no kargs `COPY` is rendered, where before the generated file silently
/// replaced the collected one or the write failed late.
#[test]
fn the_kargs_file_is_staged_through_the_decision() {
    let snap = with_kargs(staged_snapshot(vec![], vec![]));
    let staged = staged_writes(&snap);
    let write = staged
        .writes
        .get(&dest(KARGS))
        .expect("the kargs file is a write");
    assert_eq!(write.source, StagedFileKind::KargsFile);
    assert!(
        write.content.contains("append = [\"hugepages=16\"]"),
        "{}",
        write.content
    );
    let (dir, containerfile) = export(&snap);
    assert!(dir.path().join("config").join(&KARGS[1..]).is_file());
    assert!(containerfile.contains(KARGS_COPY), "{containerfile}");

    for (label, collected) in [
        ("a collected file at its path", KARGS.to_owned()),
        ("a collected file under its path", format!("{KARGS}/extra")),
    ] {
        let snap = with_kargs(staged_snapshot(
            vec![config_entry(
                &collected,
                "[kargs]\nappend = [\"other=1\"]\n",
            )],
            vec![],
        ));
        let staged = staged_writes(&snap);
        assert!(
            staged.writes.is_empty(),
            "{label}: {:?}",
            staged.writes.keys()
        );
        let mut withheld: Vec<_> = staged
            .withheld
            .iter()
            .map(|(d, k, w)| (d.as_str().to_owned(), *k, w.name()))
            .collect();
        withheld.sort();
        let mut expected = vec![
            (KARGS.to_owned(), StagedFileKind::KargsFile, "Conflict"),
            (collected.clone(), StagedFileKind::ConfigFile, "Conflict"),
        ];
        expected.sort();
        assert_eq!(withheld, expected, "{label}");

        let (dir, containerfile) = export(&snap);
        assert!(
            !dir.path().join("config").join(&KARGS[1..]).exists(),
            "{label}: nothing lands at the path"
        );
        for path in [KARGS, collected.as_str()] {
            assert!(
                fixme_naming(&containerfile, path).is_some(),
                "{label}: {path} is named:\n{containerfile}"
            );
        }
        assert!(
            !containerfile.contains(KARGS_COPY),
            "{label}:\n{containerfile}"
        );
    }
}

// --- Two paths the staging filesystem treats as one ----------------------

/// Whether the filesystem `dir` is on treats names that differ only in
/// case as one name, as macOS does by default.
fn case_insensitive(dir: &std::path::Path) -> bool {
    std::fs::write(dir.join("case-probe"), "").expect("write the probe");
    let insensitive = dir.join("CASE-PROBE").exists();
    std::fs::remove_file(dir.join("case-probe")).expect("remove the probe");
    insensitive
}

/// Two included config entries whose paths are different files on Linux
/// and one file on a case-insensitive staging filesystem: once through a
/// directory spelled two ways, once as two file names in one directory.
/// The stager never writes one offer's bytes under the other's path. On a
/// case-insensitive staging root the export fails with an error naming
/// both staging paths; on a case-sensitive one both files land with their
/// own bytes. The test reads which kind its temporary directory is and
/// asserts that outcome, so it is meaningful on macOS and on Linux.
#[test]
fn two_paths_the_staging_filesystem_treats_as_one_never_overwrite() {
    for (label, first, second) in [
        (
            "a directory spelled two ways",
            "/etc/NetworkManager/conf.d/20-security.conf",
            "/etc/networkmanager/conf.d/20-security.conf",
        ),
        (
            "two file names in one directory",
            "/etc/app/Settings.conf",
            "/etc/app/settings.conf",
        ),
    ] {
        let snap = staged_snapshot(
            vec![
                config_entry(first, "first=1\n"),
                config_entry(second, "second=2\n"),
            ],
            vec![],
        );
        // Two destinations in the decision either way: Linux names differ.
        let staged = staged_writes(&snap);
        assert_eq!(staged.writes.len(), 2, "{label}");
        assert!(staged.withheld.is_empty(), "{label}");

        let dir = TempDir::new().unwrap();
        let result = write_config_tree(&snap, dir.path());
        if case_insensitive(dir.path()) {
            let err = result.expect_err("an alias collision fails the export");
            let message = err.to_string();
            for path in [first, second] {
                assert!(
                    message.contains(&path[1..]),
                    "{label}: the error names {path}: {message}"
                );
            }
            // Whatever landed holds the first offer's bytes, never the
            // second's under the first's name.
            let landed = std::fs::read_to_string(dir.path().join("config").join(&first[1..]));
            assert_ne!(landed.ok().as_deref(), Some("second=2\n"), "{label}");
        } else {
            result.expect("both files are written");
            for (path, content) in [(first, "first=1\n"), (second, "second=2\n")] {
                assert_eq!(
                    std::fs::read_to_string(dir.path().join("config").join(&path[1..])).unwrap(),
                    content,
                    "{label}: {path}"
                );
            }
        }
    }
}

// --- A path longer than a filesystem allows ------------------------------

/// A path longer than Linux's `PATH_MAX`, or with a component longer than
/// `NAME_MAX`, is refused by `Destination::new` naming the rule, so the
/// offer shows as refused and never reaches the ancestor walk or a write.
/// Control: an ordinary path, and the longest accepted component, are
/// destinations.
#[test]
fn a_path_longer_than_a_filesystem_allows_is_refused() {
    // Roughly 1 MiB of `/a` components: quadratic work in the ancestor walk
    // if it got there.
    let deep = "/a".repeat(512 * 1024);
    let long_name = format!("/etc/{}", "n".repeat(256));
    for (path, rule) in [(&deep, "TooLong"), (&long_name, "ComponentTooLong")] {
        // Not `expect_err`: its message would print the whole path.
        let Err(err) = Destination::new(path) else {
            panic!("{rule}: a {}-byte path is a destination", path.len());
        };
        assert_eq!(err.name(), rule);

        let snap = staged_snapshot(vec![config_entry(path, "x\n")], vec![]);
        let staged = staged_writes(&snap);
        assert!(
            staged.writes.is_empty() && staged.withheld.is_empty(),
            "{rule}"
        );
        assert_eq!(
            staged
                .refused
                .iter()
                .map(|(kind, p, e)| (*kind, p.len(), e.name()))
                .collect::<Vec<_>>(),
            vec![(StagedFileKind::ConfigFile, path.len(), rule)],
        );
    }

    assert!(Destination::new("/etc/app/app.conf").is_ok());
    let longest_name = format!("/etc/{}", "n".repeat(255));
    assert!(Destination::new(&longest_name).is_ok());
}

// --- Counts describe what the export stages ------------------------------

/// The GPG keys heading and the firewall zones comment count the files the
/// stager writes: a key or zone it withholds (a config entry needs its path
/// as a directory) is named in the configuration FIXME and not counted.
/// Control: without the conflicting entry, both are counted. An included
/// key with an empty path, which the stager refuses, is named in the GPG
/// section with the rule; a key with an ordinary path gets no such line.
#[test]
fn key_and_zone_counts_follow_what_the_export_stages() {
    use inspectah_core::types::network::{FirewallZone, NetworkSection};
    use inspectah_core::types::rpm::{RepoFile, RpmSection};
    const KEY_A: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-a";
    const KEY_B: &str = "/etc/pki/rpm-gpg/RPM-GPG-KEY-b";
    const ZONE_A: &str = "/etc/firewalld/zones/a.xml";
    const ZONE_B: &str = "/etc/firewalld/zones/b.xml";
    let snapshot = |config: Vec<ConfigFileEntry>, keys: &[&str]| {
        let mut snap = staged_snapshot(config, vec![]);
        snap.rpm = Some(RpmSection {
            gpg_keys: keys
                .iter()
                .map(|path| RepoFile {
                    path: (*path).into(),
                    content: "-----BEGIN PGP PUBLIC KEY BLOCK-----\n".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        snap.network = Some(NetworkSection {
            firewall_zones: [ZONE_A, ZONE_B]
                .iter()
                .map(|path| FirewallZone {
                    path: (*path).into(),
                    name: "zone".into(),
                    content: "<zone/>\n".into(),
                    disposition: FindingKind::included(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        snap
    };
    let zones_line =
        |n: usize| format!("# {n} custom firewall zone(s): included in COPY config/etc/ below");

    let conflicts = vec![
        config_entry(&format!("{KEY_B}/x"), "x\n"),
        config_entry(&format!("{ZONE_B}/x"), "x\n"),
    ];
    let (_, containerfile) = export(&snapshot(conflicts, &[KEY_A, KEY_B]));
    assert!(
        containerfile.contains("# === GPG Keys (1) ==="),
        "{containerfile}"
    );
    assert!(containerfile.contains(&zones_line(1)), "{containerfile}");
    for path in [KEY_B, ZONE_B] {
        assert!(
            fixme_naming(&containerfile, path).is_some(),
            "{containerfile}"
        );
    }

    let (_, containerfile) = export(&snapshot(vec![], &[KEY_A, KEY_B]));
    assert!(
        containerfile.contains("# === GPG Keys (2) ==="),
        "{containerfile}"
    );
    assert!(containerfile.contains(&zones_line(2)), "{containerfile}");

    let refused = "# FIXME: GPG key  not copied: path names no file";
    let (_, containerfile) = export(&snapshot(vec![], &[KEY_A, ""]));
    assert!(
        containerfile.lines().any(|l| l == refused),
        "the empty path is named:\n{containerfile}"
    );
    let (_, containerfile) = export(&snapshot(vec![], &[KEY_A]));
    assert!(
        !containerfile.contains("not copied: path names no file"),
        "{containerfile}"
    );
}
