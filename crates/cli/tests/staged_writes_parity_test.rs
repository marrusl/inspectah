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
