//! A carried tmpfiles.d line naming a local account the image does not
//! create is named in the Containerfile and the audit report, on both
//! export paths, in the scan's HTML audit report (the refine export
//! writes none), and in refine's decision data.
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
use inspectah_pipeline::render::tmpfiles_accounts::MissingOwner;
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
/// The HTML audit report's heading and row for the same line.
const HTML_HEADING: &str = "tmpfiles.d Lines Naming Accounts the Image Lacks";
const HTML_ROW: &str = "<li><code>/etc/tmpfiles.d/appone.conf</code>: \
                        <code>/var/lib/appone/cache</code> needs user <code>appuser</code>, \
                        group <code>appgroup</code></li>";
/// What the operator does about a named line, in the Containerfile.
const CONTAINERFILE_REMEDY: &str = "# Create each missing user or group (useradd in refine) or provision it, so\n\
     # its line can run. Otherwise systemd-tmpfiles skips the line, and\n\
     # systemd-tmpfiles-setup.service still reports success.\n";
/// The same, in the audit report.
const AUDIT_REMEDY: &str = "Create each missing user or group (useradd in refine) or \
                            provision it, so its line can run. Otherwise systemd-tmpfiles \
                            skips the line, and systemd-tmpfiles-setup.service still \
                            reports success.";

fn scanned_host() -> InspectionSnapshot {
    scan_host(
        FRAGMENT_CONTENT,
        "root:x:0:0:root:/root:/bin/bash\n\
         apache:x:48:48:Apache:/usr/share/httpd:/sbin/nologin\n\
         appuser:x:1001:1001::/home/appuser:/bin/bash\n",
        "root:x:0:\napache:x:48:\nappgroup:x:1001:\n",
    )
}

/// A host carrying `fragment` at [`FRAGMENT`], with these account files.
fn scan_host(fragment: &str, passwd: &str, group: &str) -> InspectionSnapshot {
    let host = SysctlHost::new()
        .file(FRAGMENT, fragment)
        .file("/etc/passwd", passwd)
        .file("/etc/group", group);
    scan_with(
        host,
        vec![
            Box::new(RpmInspector::new()),
            Box::new(ConfigInspector::new()),
            Box::new(UsersGroupsInspector::new()),
        ],
    )
}

/// `snap` with `user` set to useradd on the scan path.
fn with_useradd(mut snap: InspectionSnapshot, user: &str) -> InspectionSnapshot {
    for row in &mut snap.users_groups.as_mut().expect("users").users {
        if row["name"] == user {
            row["containerfile_strategy"] = serde_json::json!("useradd");
        }
    }
    snap
}

/// The scan export and the refine export of `snap`, each with `useradd`
/// set on that path when it names a user.
fn both_exports(
    snap: &InspectionSnapshot,
    useradd: Option<&str>,
) -> [(&'static str, tempfile::TempDir); 2] {
    let scan_snap = match useradd {
        Some(user) => with_useradd(snap.clone(), user),
        None => snap.clone(),
    };
    let scan = render_export(&scan_snap);
    let mut refine_session = session(snap);
    if let Some(user) = useradd {
        refine_session
            .apply(RefinementOp::UserStrategy {
                username: user.into(),
                strategy: UserContainerfileStrategy::Useradd,
            })
            .expect("the user can be set to useradd");
    }
    let refine = export_session(&refine_session);
    [("scan export", scan), ("refine export", refine)]
}

/// Refine's decision data for `snap`: the rows it names, checked to be
/// the rows the web view sends the client.
fn refine_rows(snap: &InspectionSnapshot) -> Vec<MissingOwner> {
    let session = session(snap);
    let rows = session.decisions().tmpfiles_missing_owners.clone();
    let view = serde_json::to_value(inspectah_web::adapter::build_web_view(&session))
        .expect("the view serializes");
    let sent: Vec<MissingOwner> = view
        .get("tmpfiles_missing_owners")
        .map(|v| serde_json::from_value(v.clone()).expect("rows deserialize"))
        .unwrap_or_default();
    assert_eq!(rows, sent, "the web view carries refine's rows");
    rows
}

/// The row naming `path` for `user` and `group` in the fragment.
fn row(path: &str, user: Option<&str>, group: Option<&str>) -> MissingOwner {
    MissingOwner {
        file: FRAGMENT.into(),
        path: path.into(),
        user: user.map(Into::into),
        group: group.map(Into::into),
    }
}

/// `text` with its whitespace collapsed, so a row reads as one line
/// whatever the template's indentation, and the `/` the HTML escaper
/// writes as `&#x2f;` read back.
fn collapsed(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("&#x2f;", "/")
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
            containerfile.contains(CONTAINERFILE_REMEDY),
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
    let html = collapsed(&read(scan.path(), "audit-report.html"));
    assert!(html.contains(&format!("{HTML_HEADING} (1)")), "{html}");
    assert!(html.contains(HTML_ROW), "{html}");
    assert!(html.contains(AUDIT_REMEDY), "{html}");
    assert_eq!(html.matches("<code>/var/lib/appone/").count(), 1, "{html}");
    assert_eq!(
        refine_rows(&snap),
        vec![row(
            "/var/lib/appone/cache",
            Some("appuser"),
            Some("appgroup")
        )]
    );
}

/// Quoted and escaped fields are read as systemd reads them, so a line
/// whose path holds a space, or whose owner is quoted, is named with the
/// owner systemd resolves, in every place the notice appears.
#[test]
fn a_quoted_line_is_named_with_the_owner_systemd_reads() {
    let fragment = "d \"/var/lib/appone/cache dir\" 0750 appuser appgroup 30d\n\
                    d /var/lib/appone/x\\x20y 0750 'appuser' - -\n";
    let snap = scan_host(
        fragment,
        "root:x:0:0:root:/root:/bin/bash\n\
         appuser:x:1001:1001::/home/appuser:/bin/bash\n",
        "root:x:0:\nappgroup:x:1001:\n",
    );
    assert_eq!(
        refine_rows(&snap),
        vec![
            row(
                "/var/lib/appone/cache dir",
                Some("appuser"),
                Some("appgroup")
            ),
            row("/var/lib/appone/x y", Some("appuser"), None),
        ]
    );
    for (label, root) in both_exports(&snap, None) {
        let containerfile = read(root.path(), "Containerfile");
        assert_eq!(
            named_rows(&containerfile, "#   "),
            vec![
                "#   /etc/tmpfiles.d/appone.conf: /var/lib/appone/cache dir needs user appuser, \
                 group appgroup",
                "#   /etc/tmpfiles.d/appone.conf: /var/lib/appone/x y needs user appuser",
            ],
            "{label}:\n{containerfile}"
        );
        let audit = read(root.path(), "audit-report.md");
        assert!(
            audit.contains(&format!("{AUDIT_HEADING} (2)")),
            "{label}:\n{audit}"
        );
        assert!(
            audit.contains("`/var/lib/appone/cache dir` needs user `appuser`, group `appgroup`"),
            "{label}:\n{audit}"
        );
    }
    let html = collapsed(&read(render_export(&snap).path(), "audit-report.html"));
    assert!(html.contains(&format!("{HTML_HEADING} (2)")), "{html}");
    assert!(
        html.contains("<code>/var/lib/appone/x y</code> needs user <code>appuser</code></li>"),
        "{html}"
    );
}

#[test]
fn a_user_the_image_creates_is_not_named_on_either_export_path() {
    for (label, root) in both_exports(&scanned_host(), Some("appuser")) {
        let containerfile = read(root.path(), "Containerfile");
        assert!(
            containerfile.contains("RUN useradd -M -u 1001 -g appgroup"),
            "{label}: the image creates appuser:\n{containerfile}"
        );
        assert!(
            !containerfile.contains(NOTICE_MARKER),
            "{label}:\n{containerfile}"
        );
        let audit = read(root.path(), "audit-report.md");
        assert!(!audit.contains(AUDIT_HEADING), "{label}:\n{audit}");
    }
    let html = read(
        render_export(&with_useradd(scanned_host(), "appuser")).path(),
        "audit-report.html",
    );
    assert!(!html.contains(HTML_HEADING));
}

/// A quoted path holds whitespace, so splitting on whitespace alone reads
/// the mode as the user and the user as the group. With `appuser`
/// created and an unrelated host group also named `appuser` left on Skip,
/// that misreading would claim the line needs group `appuser`.
#[test]
fn a_quoted_path_never_names_a_misread_owner_on_either_export_path() {
    let snap = scan_host(
        "d \"/var/lib/appone/cache dir\" 0750 appuser appgroup 30d\n",
        "root:x:0:0:root:/root:/bin/bash\n\
         appuser:x:1001:1001::/home/appuser:/bin/bash\n",
        "root:x:0:\nappgroup:x:1001:\nappuser:x:1002:\n",
    );
    for (label, root) in both_exports(&snap, Some("appuser")) {
        let containerfile = read(root.path(), "Containerfile");
        assert!(
            containerfile.contains("RUN useradd -M -u 1001 -g appgroup"),
            "{label}: the image creates appuser:\n{containerfile}"
        );
        assert!(
            !containerfile.contains(NOTICE_MARKER),
            "{label}:\n{containerfile}"
        );
        let audit = read(root.path(), "audit-report.md");
        assert!(!audit.contains(AUDIT_HEADING), "{label}:\n{audit}");
    }
}

/// `redis` in the target baseline creates the `redis` account in the
/// image, so a host `redis` account on Skip is not missing there, even
/// though the package is not in `packages_added`.
#[test]
fn an_account_a_base_image_package_creates_is_not_named_on_either_export_path() {
    let host = || {
        scan_host(
            "d /var/lib/appone/redis 0750 redis redis -\n",
            "root:x:0:0:root:/root:/bin/bash\n\
             redis:x:1001:1001:Redis:/var/lib/redis:/sbin/nologin\n",
            "root:x:0:\nredis:x:1001:\n",
        )
    };
    let mut in_baseline = host();
    let rpm = in_baseline.rpm.as_mut().expect("rpm section");
    assert!(
        !rpm.packages_added.iter().any(|p| p.name == "redis"),
        "the fixture proves nothing if redis is carried"
    );
    rpm.baseline_package_names = Some(vec!["redis".into()]);

    // Without the baseline the account is named, so the baseline is
    // what suppresses it.
    for (snap, named) in [(host(), true), (in_baseline, false)] {
        for (label, root) in both_exports(&snap, None) {
            let containerfile = read(root.path(), "Containerfile");
            assert_eq!(
                containerfile.contains(NOTICE_MARKER),
                named,
                "{label}:\n{containerfile}"
            );
            let audit = read(root.path(), "audit-report.md");
            assert_eq!(audit.contains(AUDIT_HEADING), named, "{label}:\n{audit}");
        }
    }
}

/// `z` adjusts a path that already exists, so the notice says the line is
/// skipped, not that a path is never created.
#[test]
fn an_adjust_only_line_gets_operation_neutral_wording_on_both_export_paths() {
    let snap = scan_host(
        "z /var/lib/appone/cache 0750 appuser appgroup -\n",
        "root:x:0:0:root:/root:/bin/bash\n\
         appuser:x:1001:1001::/home/appuser:/bin/bash\n",
        "root:x:0:\nappgroup:x:1001:\n",
    );
    for (label, root) in both_exports(&snap, None) {
        let containerfile = read(root.path(), "Containerfile");
        assert_eq!(
            named_rows(&containerfile, "#   "),
            vec![CONTAINERFILE_ROW],
            "{label}:\n{containerfile}"
        );
        assert!(
            containerfile.contains(CONTAINERFILE_REMEDY),
            "{label}:\n{containerfile}"
        );
        let audit = read(root.path(), "audit-report.md");
        assert_eq!(
            named_rows(&audit, "- "),
            vec![AUDIT_ROW],
            "{label}:\n{audit}"
        );
        assert!(audit.contains(AUDIT_REMEDY), "{label}:\n{audit}");
        for text in [&containerfile, &audit] {
            assert!(!text.contains("not be created"), "{label}:\n{text}");
        }
    }
}
