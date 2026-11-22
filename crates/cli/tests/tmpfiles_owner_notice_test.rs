//! A carried tmpfiles.d line naming a local account the image does not
//! create is named in the Containerfile and the audit report, on both
//! export paths.
//!
//! `systemd-tmpfiles` skips a line whose user it cannot resolve and
//! `systemd-tmpfiles-setup` counts that as success, so the directory is
//! silently missing on a deployed host. Users default to Skip, so on the
//! default path this is every local account a carried fragment names.

use crate::sysctl_hosts::{SysctlHost, export_session, render_export, scan_with, session};
use inspectah_collect::inspectors::config::ConfigInspector;
use inspectah_collect::inspectors::rpm::RpmInspector;
use inspectah_collect::inspectors::users::UsersGroupsInspector;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::users::UserContainerfileStrategy;
use inspectah_refine::types::RefinementOp;

const FRAGMENT: &str = "/etc/tmpfiles.d/appone.conf";

/// The local account's line, one naming a base-image account, one naming
/// a system account the scan does not record, and one by number.
const FRAGMENT_CONTENT: &str = "d /var/lib/appone/cache 0750 appuser appgroup 30d\n\
                                d /var/lib/appone/run 0755 root root -\n\
                                d /var/lib/appone/web 0755 apache apache -\n\
                                d /var/lib/appone/spool 0750 1001 1001 -\n";

/// The Containerfile line that names the local account's line.
const CONTAINERFILE_ROW: &str =
    "#   /etc/tmpfiles.d/appone.conf: /var/lib/appone/cache needs user appuser, group appgroup";
/// The audit report row that names it.
const AUDIT_ROW: &str = "- `/etc/tmpfiles.d/appone.conf`: `/var/lib/appone/cache` needs \
                         user `appuser`, group `appgroup`";
/// Present wherever any line is named.
const NOTICE_MARKER: &str = "tmpfiles.d lines name an account";
const AUDIT_HEADING: &str = "#### tmpfiles.d Lines Naming Accounts the Image Lacks";

fn scanned_host() -> InspectionSnapshot {
    let host = SysctlHost::new()
        .file(FRAGMENT, FRAGMENT_CONTENT)
        .file(
            "/etc/passwd",
            "root:x:0:0:root:/root:/bin/bash\n\
             apache:x:48:48:Apache:/usr/share/httpd:/sbin/nologin\n\
             appuser:x:1001:1001::/home/appuser:/bin/bash\n",
        )
        .file("/etc/group", "root:x:0:\napache:x:48:\nappgroup:x:1001:\n");
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(UsersGroupsInspector::new()),
        ],
    )
}

fn read(root: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The lines of `text` that name a carried tmpfiles.d path.
fn named_rows<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    text.lines()
        .filter(|l| l.starts_with(prefix) && l.contains("/var/lib/appone/"))
        .collect()
}

#[test]
fn the_fragment_is_carried() {
    let snap = scanned_host();
    let tree = render_export(&snap);
    assert_eq!(
        read(tree.path(), "config/etc/tmpfiles.d/appone.conf"),
        FRAGMENT_CONTENT,
        "the fixture proves nothing unless the fragment ships"
    );
}

#[test]
fn a_skipped_user_is_named_on_both_export_paths() {
    let snap = scanned_host();
    let scan = render_export(&snap);
    let refine = export_session(&session(&snap));
    for (label, root) in [
        ("scan export", scan.path()),
        ("refine export", refine.path()),
    ] {
        let containerfile = read(root, "Containerfile");
        assert_eq!(
            named_rows(&containerfile, "#   "),
            vec![CONTAINERFILE_ROW],
            "{label}: only the local account's line is named:\n{containerfile}"
        );
        assert!(
            containerfile.contains(
                "# Create the user in refine (useradd) or provision it, or the path will not be created."
            ),
            "{label}:\n{containerfile}"
        );
        let audit = read(root, "audit-report.md");
        assert!(
            audit.contains(&format!("{AUDIT_HEADING} (1)")),
            "{label}:\n{audit}"
        );
        assert_eq!(
            named_rows(&audit, "- "),
            vec![AUDIT_ROW],
            "{label}: only the local account's line is named:\n{audit}"
        );
    }
}

#[test]
fn a_user_the_image_creates_is_not_named_on_either_export_path() {
    let mut snap = scanned_host();
    for user in &mut snap.users_groups.as_mut().expect("users").users {
        if user["name"] == "appuser" {
            user["containerfile_strategy"] = serde_json::json!("useradd");
        }
    }
    let scan = render_export(&snap);

    let mut refine_session = session(&scanned_host());
    refine_session
        .apply(RefinementOp::UserStrategy {
            username: "appuser".into(),
            strategy: UserContainerfileStrategy::Useradd,
        })
        .expect("appuser can be set to useradd");
    let refine = export_session(&refine_session);

    for (label, root) in [
        ("scan export", scan.path()),
        ("refine export", refine.path()),
    ] {
        let containerfile = read(root, "Containerfile");
        assert!(
            containerfile.contains("RUN useradd -M -u 1001 -g appgroup"),
            "{label}: the image creates appuser:\n{containerfile}"
        );
        assert!(
            !containerfile.contains(NOTICE_MARKER),
            "{label}:\n{containerfile}"
        );
        let audit = read(root, "audit-report.md");
        assert!(!audit.contains(AUDIT_HEADING), "{label}:\n{audit}");
    }
}
