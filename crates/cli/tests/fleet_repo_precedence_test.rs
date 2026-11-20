//! A file two sections record, on a fleet aggregate: the config walk and
//! the repo inventory both record `/etc/yum.repos.d/custom.repo`. The
//! merge keeps the config entry's content variants and one representative
//! repo row, so the operator's selected config variant can differ from the
//! repo row's bytes. The config entry decides the content: the export
//! stages one file at the repo path and keeps the repo `COPY`, so the
//! packages from that repository still install.
//!
//! Each host is built from collector output (the RPM, config and units
//! inspectors over a mock host), merged with `merge_snapshots`, opened in
//! a refine session and exported.

use crate::unit_hosts::{UnitHost, export_session, reload, scan, session};
use inspectah_core::aggregate::merge_snapshots;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_refine::session::RefineSession;
use inspectah_refine::types::{ItemId, RefinementOp};

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

fn set_config_include(session: &mut RefineSession, include: bool) {
    session
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config { path: REPO.into() },
            include,
        })
        .unwrap_or_else(|e| panic!("SetInclude({REPO}, {include}): {e:?}"));
}

/// The bytes the export stages at the repo path, whether some FIXME names
/// the path, and whether the repo `COPY` is present.
fn exported(session: &RefineSession) -> (Option<String>, bool, bool) {
    let tree = export_session(session);
    let staged = std::fs::read_to_string(tree.path().join("config").join(&REPO[1..])).ok();
    let containerfile =
        std::fs::read_to_string(tree.path().join("Containerfile")).expect("Containerfile");
    let named = containerfile
        .lines()
        .any(|l| l.starts_with("# FIXME:") && l.contains(REPO));
    (staged, named, containerfile.contains(REPO_COPY))
}

/// In both hostname assignments: the default export stages the repo row's
/// content with the repo `COPY`; including the config entry stages the
/// config variant refine includes, withholds nothing and keeps the `COPY`;
/// excluding it again goes back to the repo row's content.
///
/// The merged repo row is the first host's payload, and refine includes the
/// first-ranked config variant (ranked by count, then by content hash), so
/// one assignment includes a variant that differs from the repo row and
/// the other includes the identical one: each is asserted as it is.
#[test]
fn a_config_variant_decides_the_repo_files_content_in_both_host_orders() {
    let alpha = repo_content("alpha");
    let beta = repo_content("beta");
    for (first, second, differs) in [
        (("host-a", "beta"), ("host-b", "alpha"), true),
        (("host-a", "alpha"), ("host-b", "beta"), false),
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
        let variants: Vec<&str> = merged
            .config
            .as_ref()
            .expect("a config section")
            .files
            .iter()
            .filter(|f| f.path == REPO)
            .map(|f| f.content.as_str())
            .collect();
        assert_eq!(variants.len(), 2, "{label}: two config variants");

        let mut session = session(&merged);
        assert_eq!(
            exported(&session),
            (Some(representative.clone()), false, true),
            "{label}: default"
        );

        set_config_include(&mut session, true);
        let included: Vec<String> = session
            .snapshot_projected()
            .config
            .as_ref()
            .expect("a config section")
            .files
            .iter()
            .filter(|f| f.path == REPO && f.disposition.is_included())
            .map(|f| f.content.clone())
            .collect();
        assert_eq!(included.len(), 1, "{label}: one variant included");
        assert_eq!(included[0] != representative, differs, "{label}");
        assert_eq!(included[0], alpha, "{label}: the first-ranked variant");
        assert_ne!(included[0], beta, "{label}");
        assert_eq!(
            exported(&session),
            (Some(included[0].clone()), false, true),
            "{label}: the config variant included"
        );

        set_config_include(&mut session, false);
        assert_eq!(
            exported(&session),
            (Some(representative), false, true),
            "{label}: excluded again"
        );
    }
}
