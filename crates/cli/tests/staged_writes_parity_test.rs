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
use unit_hosts::{export_session, render_export, session};

const WORKER_UNIT: &str = "/etc/systemd/system/worker.service";
const WORKER_CHILD: &str = "/etc/systemd/system/worker.service/extra.conf";
const ZERO_LENGTH_UNIT: &str = "/etc/systemd/system/kdump.service";
const PLAIN_FILE: &str = "/etc/app/app.conf";

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
    snap
}

/// Every regular file under `config/` and `drop-ins/`, relative to `root`.
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
    for dir in ["config", "drop-ins"] {
        walk(&root.join(dir), root, &mut out);
    }
    out
}

/// The Containerfile's FIXME lines.
fn fixmes(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("Containerfile"))
        .expect("the export has a Containerfile")
        .lines()
        .filter(|l| l.starts_with("# FIXME:"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_scan_export_and_the_refine_export_stage_the_same_files() {
    let snap = snapshot();
    let scan_tree = render_export(&snap);
    let refine_tree = export_session(&session(&snap));

    let staged = staged_files(scan_tree.path());
    assert_eq!(
        staged,
        BTreeSet::from(["config/etc/app/app.conf".to_string()]),
        "only the file with no conflict and content is staged"
    );
    let named = fixmes(scan_tree.path());
    for path in [WORKER_UNIT, WORKER_CHILD, ZERO_LENGTH_UNIT] {
        assert!(
            named.iter().any(|l| l.contains(path)),
            "the scan export names {path}: {named:?}"
        );
    }
    assert!(!named.iter().any(|l| l.contains(PLAIN_FILE)), "{named:?}");

    assert_eq!(staged_files(refine_tree.path()), staged, "same staged tree");
    assert_eq!(fixmes(refine_tree.path()), named, "same FIXME lines");
}
