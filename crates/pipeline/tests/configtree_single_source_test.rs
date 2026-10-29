//! § 8.1: `write_config_tree` has exactly one source for `/etc` content —
//! `config.files`, gated on `is_included()`.
//!
//! The ungated kernel snippet block these tests exist to catch was deleted
//! one task earlier, when the fields it read were removed, so they are green
//! from the moment they are written. They stand as a regression guard:
//! `write_config_tree` must never regrow a second writer for `/etc`.
//!
//! Their teeth were measured by mutation, not by a red run — reintroducing a
//! path-rule writer inside the kernel/boot block turns
//! `a_config_exclusion_is_not_overridden_by_a_kernel_list` red while the rest
//! of the workspace stays green.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigCategory, ConfigFileEntry, ConfigSection};
use inspectah_core::types::kernelboot::{KernelBootSection, SysctlOverride};
use inspectah_core::types::{AdvisoryType, FindingKind};
use inspectah_pipeline::render::configtree::write_config_tree;
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
        // The synthesized sysctl file is written under `sysctl/`, not
        // `config/`, and its source path is not the snippet path, so it
        // neither adds a root nor collides with the entry under test.
        sysctl_overrides: vec![SysctlOverride {
            key: "net.ipv4.ip_forward".to_string(),
            runtime: "1".to_string(),
            default: "0".to_string(),
            source: "/etc/sysctl.d/99-custom.conf".to_string(),
            disposition: FindingKind::included(),
            ..Default::default()
        }],
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
