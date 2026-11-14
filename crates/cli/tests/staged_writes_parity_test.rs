//! § 10.2 "`staged_writes` is the stager's decision": the scan export and
//! the refine export ask the same function, so one snapshot stages the same
//! files and names the same withheld files through both (plan review
//! focus 4).
//!
//! The snapshot is built by hand: a file and a file under it, and a
//! zero-length unit file offered as a config entry, come from imported or
//! hand-edited snapshots, not from the collector.

#[path = "support/unit_hosts.rs"]
mod unit_hosts;

use std::collections::BTreeSet;
use std::path::Path;

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::FindingKind;
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
use inspectah_core::types::containers::{ContainerSection, FlatpakApp};
use inspectah_core::types::scheduled::{GeneratedTimerUnit, ScheduledTaskSection};
use inspectah_core::types::units::{SystemdDropIn, UnitSection};
use unit_hosts::{export_session, render_export, session};

const WORKER_UNIT: &str = "/etc/systemd/system/worker.service";
const WORKER_CHILD: &str = "/etc/systemd/system/worker.service/extra.conf";
const ZERO_LENGTH_UNIT: &str = "/etc/systemd/system/kdump.service";
const PLAIN_FILE: &str = "/etc/app/app.conf";
const DROP_IN: &str = "/etc/systemd/system/sshd.service.d/override.conf";

fn entry(path: &str, content: &str) -> ConfigFileEntry {
    ConfigFileEntry {
        path: path.to_string(),
        category: ConfigCategory::Other,
        content: content.to_string(),
        disposition: FindingKind::included(),
        ..Default::default()
    }
}

fn snapshot() -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: vec![
            entry(WORKER_CHILD, "[Service]\n"),
            entry(WORKER_UNIT, "[Unit]\n"),
            entry(ZERO_LENGTH_UNIT, ""),
            entry(PLAIN_FILE, "a=1\n"),
        ],
    });
    snap.units = Some(UnitSection {
        drop_ins: vec![SystemdDropIn {
            unit: "sshd.service".into(),
            path: DROP_IN.into(),
            content: "[Service]\nNice=5\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "cron-backup".into(),
            timer_content: "[Timer]\nOnCalendar=*-*-* 01:00:00\n".into(),
            service_content: "[Service]\nExecStart=/bin/true\n".into(),
            cron_expr: "0 1 * * *".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
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

/// Every regular file under `config/`, `drop-ins/` and `flatpak/`,
/// relative to `root`.
fn staged_files(root: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).expect("under the export");
                out.insert(rel.to_string_lossy().into_owned());
            }
        }
    }
    let mut out = BTreeSet::new();
    for dir in ["config", "drop-ins", "flatpak"] {
        walk(&root.join(dir), root, &mut out);
    }
    out
}

fn containerfile(root: &Path) -> String {
    std::fs::read_to_string(root.join("Containerfile")).expect("the export has a Containerfile")
}

#[test]
fn the_scan_export_and_the_refine_export_stage_the_same_files() {
    let snap = snapshot();
    let scan_tree = render_export(&snap);
    let refine_tree = export_session(&session(&snap));

    let staged = staged_files(scan_tree.path());
    let expected: BTreeSet<String> = [
        "config/etc/app/app.conf",
        "config/etc/systemd/system/cron-backup.service",
        "config/etc/systemd/system/cron-backup.timer",
        "drop-ins/etc/systemd/system/sshd.service.d/override.conf",
        "flatpak/flatpak-install.json",
        "flatpak/flatpak-provision.service",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(staged, expected, "what the scan export stages");

    let scan_cf = containerfile(scan_tree.path());
    for path in [WORKER_UNIT, WORKER_CHILD, ZERO_LENGTH_UNIT] {
        assert!(
            scan_cf
                .lines()
                .any(|l| l.starts_with("# FIXME:") && l.contains(path)),
            "the scan export names {path}:\n{scan_cf}"
        );
    }
    for line in [
        "COPY drop-ins/etc/systemd/system/ /etc/systemd/system/",
        "COPY config/etc/systemd/system/ /etc/systemd/system/",
        "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service",
    ] {
        assert!(scan_cf.contains(line), "{line}:\n{scan_cf}");
    }

    assert_eq!(staged_files(refine_tree.path()), staged, "same staged tree");
    assert_eq!(
        containerfile(refine_tree.path()),
        scan_cf,
        "same Containerfile"
    );
}

// --- A generated unit the stager withholds is neither copied nor enabled --

const FLATPAK_UNIT: &str = "/etc/systemd/system/flatpak-provision.service";
const FLATPAK_STAGED: &str = "flatpak/flatpak-provision.service";
const FLATPAK_COPY: &str =
    "COPY flatpak/flatpak-provision.service /etc/systemd/system/flatpak-provision.service";
const CRON_TIMER: &str = "/etc/systemd/system/cron-backup.timer";
const CRON_SERVICE: &str = "/etc/systemd/system/cron-backup.service";
const REPORT_TIMER: &str = "/etc/systemd/system/report.timer";

/// A file under `path` as a directory, which conflicts with a file at it.
fn child_of(path: &str) -> String {
    format!("{path}/extra.conf")
}

/// A snapshot holding `files` as included config entries, plus what
/// `with` adds.
fn units_snapshot(files: &[&str], with: fn(&mut InspectionSnapshot)) -> InspectionSnapshot {
    let mut snap = InspectionSnapshot::new();
    snap.config = Some(ConfigSection {
        files: files.iter().map(|p| entry(p, "[Service]\n")).collect(),
    });
    with(&mut snap);
    snap
}

fn with_flatpak(snap: &mut InspectionSnapshot) {
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
}

/// A generated `cron-backup` unit from a cron entry with `cron_expr`: a
/// timer and its service, or the service alone for `@reboot`.
fn with_cron(snap: &mut InspectionSnapshot, cron_expr: &str) {
    let timer = if cron_expr == "@reboot" {
        ""
    } else {
        "[Timer]\nOnCalendar=*-*-* 01:00:00\n"
    };
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        generated_timer_units: vec![GeneratedTimerUnit {
            name: "cron-backup".into(),
            timer_content: timer.into(),
            service_content: "[Service]\nExecStart=/bin/true\n".into(),
            cron_expr: cron_expr.into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
}

/// The host's own `report.timer`, a local timer row with no service file.
fn with_local_timer(snap: &mut InspectionSnapshot) {
    use inspectah_core::types::scheduled::SystemdTimer;
    snap.scheduled_tasks = Some(ScheduledTaskSection {
        systemd_timers: vec![SystemdTimer {
            name: "report".into(),
            source: "local".into(),
            path: REPORT_TIMER.into(),
            timer_content: "[Timer]\nOnCalendar=daily\n".into(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
        ..Default::default()
    });
}

/// The scan export and the refine export of `snap`, each with its label.
fn both_exports(snap: &InspectionSnapshot) -> [(&'static str, tempfile::TempDir); 2] {
    [
        ("scan export", render_export(snap)),
        ("refine export", export_session(&session(snap))),
    ]
}

/// Whether some `# FIXME:` line names `path`.
fn fixme_names(containerfile: &str, path: &str) -> bool {
    containerfile
        .lines()
        .any(|l| l.starts_with("# FIXME:") && l.contains(path))
}

/// The flatpak provisioning service, withheld beside a file that needs its
/// path as a directory: neither its `COPY` nor its enable is written, on
/// both routes, while the withheld file is named and the manifest still
/// lands. Reversed control: with the conflict removed, the service is
/// staged, copied and enabled.
#[test]
fn a_withheld_flatpak_service_is_neither_copied_nor_enabled() {
    let conflict = child_of(FLATPAK_UNIT);
    let snap = units_snapshot(&[&conflict], with_flatpak);
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile(tree.path());
        assert!(fixme_names(&cf, FLATPAK_UNIT), "{route}: named:\n{cf}");
        assert!(
            tree.path().join("flatpak/flatpak-install.json").is_file(),
            "{route}: the manifest still lands"
        );
        assert!(!tree.path().join(FLATPAK_STAGED).exists(), "{route}");
        assert!(!cf.contains(FLATPAK_COPY), "{route}:\n{cf}");
        assert!(
            unit_hosts::in_no_systemctl_instruction(&cf, "flatpak-provision.service"),
            "{route}:\n{cf}"
        );
    }

    let snap = units_snapshot(&[], with_flatpak);
    for (route, tree) in both_exports(&snap) {
        let cf = containerfile(tree.path());
        assert!(tree.path().join(FLATPAK_STAGED).is_file(), "{route}");
        assert!(cf.contains(FLATPAK_COPY), "{route}:\n{cf}");
        assert!(
            unit_hosts::acts(&cf, "enable", "flatpak-provision.service"),
            "{route}:\n{cf}"
        );
    }
}

/// What a case adds to the snapshot.
type AddUnits = fn(&mut InspectionSnapshot);

/// A scheduled unit whose file the stager withholds is not enabled: a
/// generated timer withheld itself, a generated timer whose paired service
/// is withheld, an `@reboot` service, and the host's own local timer. Each
/// on both routes, with the withheld file named. Reversed control: with
/// the conflict removed, the same unit is enabled.
#[test]
fn a_withheld_scheduled_unit_is_not_enabled() {
    let cases: [(&str, &str, &str, AddUnits); 4] = [
        ("generated timer", CRON_TIMER, "cron-backup.timer", |s| {
            with_cron(s, "0 1 * * *")
        }),
        (
            "the generated timer's service",
            CRON_SERVICE,
            "cron-backup.timer",
            |s| with_cron(s, "0 1 * * *"),
        ),
        (
            "@reboot service",
            CRON_SERVICE,
            "cron-backup.service",
            |s| with_cron(s, "@reboot"),
        ),
        (
            "local timer",
            REPORT_TIMER,
            "report.timer",
            with_local_timer,
        ),
    ];
    for (label, withheld, unit, with) in cases {
        let conflict = child_of(withheld);
        let snap = units_snapshot(&[&conflict], with);
        for (route, tree) in both_exports(&snap) {
            let cf = containerfile(tree.path());
            assert!(fixme_names(&cf, withheld), "{label}, {route}:\n{cf}");
            assert!(
                unit_hosts::in_no_systemctl_instruction(&cf, unit),
                "{label}, {route}: {unit} is enabled:\n{cf}"
            );
        }

        let snap = units_snapshot(&[], with);
        for (route, tree) in both_exports(&snap) {
            let cf = containerfile(tree.path());
            assert!(
                unit_hosts::acts(&cf, "enable", unit),
                "{label}, {route}: control:\n{cf}"
            );
        }
    }
}

// --- `/var` backing reads the staging decision --------------------------

const JOB_FRAGMENT: &str = "/etc/tmpfiles.d/job.conf";
const JOB_CACHE: &str = "/var/cache/job";
const WORKER_STATE: &str = "/var/lib/worker";

/// A snapshot whose one `/var` directory, `dir`, was created on the host
/// by `backing`, with `files` as included config entries.
fn var_snapshot(
    files: &[&str],
    dir: &str,
    backing: inspectah_core::types::storage::VarDirBacking,
) -> InspectionSnapshot {
    use inspectah_core::types::storage::{StorageSection, VarDirectory};
    let mut snap = units_snapshot(files, |_| {});
    snap.storage = Some(StorageSection {
        var_directories: vec![VarDirectory {
            path: dir.into(),
            backing: Some(backing),
            mode: Some("0755".into()),
            owner_uid: Some(0),
            owner_gid: Some(0),
            owner_name: Some("root".into()),
            group_name: Some("root".into()),
        }],
        ..Default::default()
    });
    snap
}

/// The provisioning line `/var/<dir>` gets in the export's tmpfiles.d
/// fragment, if any.
fn provisioned(tree: &tempfile::TempDir, dir: &str) -> bool {
    std::fs::read_to_string(
        tree.path()
            .join("config/usr/lib/tmpfiles.d/inspectah-var.conf"),
    )
    .is_ok_and(|conf| {
        conf.lines()
            .any(|l| l == format!("d {dir} 0755 root root -"))
    })
}

/// Whether a `/var` directory's creator reaches the image is the stager's
/// answer, by destination: a tmpfiles.d fragment or a unit file the
/// stager withholds (a file under it needs its path as a directory) does
/// not carry its directory, which the export then provisions on both
/// routes and the refine reference view calls `Provision`. A unit file
/// staged under another spelling of the path does carry it. Reversed
/// control for each: the file staged under the declared spelling, with no
/// conflict, carries the directory and nothing is provisioned.
#[test]
fn var_backing_follows_what_the_stager_writes() {
    use inspectah_core::types::storage::VarDirBacking;
    use inspectah_pipeline::render::var_dirs::EffectiveBacking;

    let fragment = || VarDirBacking::Tmpfiles {
        conf: JOB_FRAGMENT.into(),
        line: format!("d {JOB_CACHE} 0755 root root -"),
    };
    let state = || VarDirBacking::StateDirectory {
        declared_by: vec![WORKER_UNIT.into()],
    };
    let job_child = child_of(JOB_FRAGMENT);
    let worker_child = child_of(WORKER_UNIT);
    let lost = |creator: &str| EffectiveBacking::Provision {
        lost: Some(creator.into()),
    };
    let cases = [
        (
            "withheld fragment",
            var_snapshot(&[JOB_FRAGMENT, &job_child], JOB_CACHE, fragment()),
            JOB_CACHE,
            lost(JOB_FRAGMENT),
        ),
        (
            "control: staged fragment",
            var_snapshot(&[JOB_FRAGMENT], JOB_CACHE, fragment()),
            JOB_CACHE,
            EffectiveBacking::Carried,
        ),
        (
            "withheld unit file",
            var_snapshot(&[WORKER_UNIT, &worker_child], WORKER_STATE, state()),
            WORKER_STATE,
            // A declaring unit is named by its unit name.
            lost("worker.service"),
        ),
        (
            "unit file staged under another spelling",
            var_snapshot(
                &["/etc/systemd/./system//worker.service"],
                WORKER_STATE,
                state(),
            ),
            WORKER_STATE,
            EffectiveBacking::Carried,
        ),
        (
            "control: staged unit file",
            var_snapshot(&[WORKER_UNIT], WORKER_STATE, state()),
            WORKER_STATE,
            EffectiveBacking::Carried,
        ),
    ];
    for (label, snap, dir, outcome) in cases {
        let session = session(&snap);
        let reference = session.reference();
        let view: Vec<_> = reference
            .storage
            .var_directories
            .iter()
            .map(|d| (d.path.as_str(), &d.outcome))
            .collect();
        assert_eq!(view, vec![(dir, &outcome)], "{label}: reference view");
        let provisions = matches!(outcome, EffectiveBacking::Provision { .. });
        for (route, tree) in [
            ("scan export", render_export(&snap)),
            ("refine export", export_session(&session)),
        ] {
            assert_eq!(
                provisioned(&tree, dir),
                provisions,
                "{label}, {route}:\n{}",
                containerfile(tree.path())
            );
        }
    }
}
