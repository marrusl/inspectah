//! A file two sections record, on a fleet aggregate: the config walk and
//! the repo inventory both record `/etc/yum.repos.d/custom.repo`. The
//! merge keeps the config entry's content variants and one representative
//! repo row. The repo section owns the file (Mark Russell, 2026-11-21): the
//! config variants are read-only, the export stages the representative,
//! the most common version, and the Containerfile names the hosts whose
//! version differs. Choosing a version per host waits for the fleet
//! config-variant work.
//!
//! Each host is built from collector output (the RPM, config and units
//! inspectors over a mock host), merged with `merge_snapshots`, opened in
//! a refine session and exported.

use crate::unit_hosts::{UnitHost, export_session, reload, scan, session};
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigShadow;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefineError, RefinementOp};

const REPO: &str = "/etc/yum.repos.d/custom.repo";
const REPO_COPY: &str = "COPY config/etc/yum.repos.d/ /etc/yum.repos.d/";

/// The repo file with `custom` served from `<base>.example`.
fn repo_content(base: &str) -> String {
    format!("[custom]\nbaseurl=https://{base}.example/repo\nenabled=1\n")
}

/// A host named `hostname` whose custom repo is served from `base`.
fn host(hostname: &str, base: &str) -> InspectionSnapshot {
    reload(&scan(
        UnitHost::new()
            .file(REPO, &repo_content(base))
            .file("/etc/hostname", &format!("{hostname}\n")),
    ))
}

fn set_config_include(session: &mut RefineSession, include: bool) -> Result<(), RefineError> {
    session.apply(RefinementOp::SetInclude {
        item_id: ItemId::Config { path: REPO.into() },
        include,
    })
}

/// The bytes the export stages at the repo path, whether some FIXME names
/// the path, and the Containerfile.
fn exported(session: &RefineSession) -> (Option<String>, bool, String) {
    let tree = export_session(session);
    let staged = std::fs::read_to_string(tree.path().join("config").join(&REPO[1..])).ok();
    let containerfile =
        std::fs::read_to_string(tree.path().join("Containerfile")).expect("Containerfile");
    let named = containerfile
        .lines()
        .any(|l| l.starts_with("# FIXME:") && l.contains(REPO));
    (staged, named, containerfile)
}

/// In both hostname assignments: the two config variants are read-only
/// inventory decided under Repositories, and their `SetInclude` is
/// refused; the export stages the repo row's representative payload with
/// the repo `COPY`, and the Containerfile names the host whose version
/// differs.
#[test]
fn the_repo_row_decides_the_repo_files_content_in_both_host_orders() {
    for (first, second) in [
        (("host-a", "beta"), ("host-b", "alpha")),
        (("host-a", "alpha"), ("host-b", "beta")),
    ] {
        let label = format!("{} on {}, {} on {}", first.1, first.0, second.1, second.0);
        let (merged, _) = merge_snapshots(
            vec![host(first.0, first.1), host(second.0, second.1)],
            None,
            None,
        )
        .expect("the two hosts merge");
        let merged = reload(&merged);

        let repo_row = &merged.rpm.as_ref().expect("an rpm section").repo_files;
        assert_eq!(repo_row.len(), 1, "{label}: one repo row");
        let representative = repo_content(first.1);
        assert_eq!(repo_row[0].content, representative, "{label}");
        assert!(repo_row[0].disposition.is_included(), "{label}");
        let variants: Vec<_> = merged
            .config
            .as_ref()
            .expect("a config section")
            .files
            .iter()
            .filter(|f| f.path == REPO)
            .collect();
        assert_eq!(variants.len(), 2, "{label}: two config variants");
        for v in &variants {
            assert!(v.disposition.is_inventory(), "{label}");
            assert_eq!(v.shadowed_by, Some(ConfigShadow::Repositories), "{label}");
        }

        let mut session = session(&merged);
        for include in [true, false] {
            assert!(
                matches!(
                    set_config_include(&mut session, include),
                    Err(RefineError::InventoryNotToggleable(_))
                ),
                "{label}: the config row is read-only"
            );
        }
        let (staged, named, containerfile) = exported(&session);
        assert_eq!(staged, Some(representative), "{label}");
        assert!(!named, "{label}");
        assert!(containerfile.contains(REPO_COPY), "{label}");
        let line = format!(
            "# {REPO}: the export copies the version 1 of 2 hosts with this file have; a \
             different version is on {}",
            second.0
        );
        assert!(
            containerfile.lines().any(|l| l == line),
            "{label}:\n{containerfile}"
        );
    }
}
