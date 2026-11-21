//! `%ghost` config files, end to end through the scan path.
//!
//! Every host is built with [`SysctlHost`], whose `ghost` rows answer the
//! config inspector's file-flags query the way `rpm -qa --queryformat
//! '[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES}\n]'` prints them, and run
//! through the real inspectors. Each case checks the snapshot, then both
//! export paths: the scan's own export (`render_all`) and a refine
//! session's export of the scan tarball.

use std::path::Path;

use crate::sysctl_hosts::{SysctlHost, export_session, render_export, scan, session};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::{ConfigFileEntry, ConfigFileKind};

/// An SSSD configuration with an AD bind password, as an admin writes it.
const SSSD_CONF: &str = "\
[sssd]
domains = example.com
services = nss, pam

[domain/example.com]
id_provider = ldap
ldap_default_bind_dn = cn=binder,dc=example,dc=com
ldap_default_authtok_type = password
ldap_default_authtok = BindPass123
";

/// The bind password in [`SSSD_CONF`].
const BIND_PASSWORD: &str = "BindPass123";

/// The config entry at `path`, if the scan collected one.
fn entry<'a>(snap: &'a InspectionSnapshot, path: &str) -> Option<&'a ConfigFileEntry> {
    snap.config
        .as_ref()
        .expect("the config section is present")
        .files
        .iter()
        .find(|f| f.path == path)
}

fn containerfile(tree: &Path) -> String {
    std::fs::read_to_string(tree.join("Containerfile")).expect("the export has a Containerfile")
}

/// The two export trees of `snap`: the scan's, then refine's.
fn both_exports(snap: &InspectionSnapshot) -> [(&'static str, tempfile::TempDir); 2] {
    [
        ("scan export", render_export(snap)),
        ("refine export", export_session(&session(snap))),
    ]
}

/// A host joined to a directory: SSSD's `%ghost` config, an unowned
/// `conf.d` snippet, and the `%ghost` machine-id `systemd` lists.
fn sssd_host() -> SysctlHost {
    SysctlHost::new()
        .ghost("sssd-common", "cng", "/etc/sssd/sssd.conf")
        .file("/etc/sssd/sssd.conf", SSSD_CONF)
        .file(
            "/etc/sssd/conf.d/10-site.conf",
            "[pam]\noffline_credentials_expiration = 2\n",
        )
        .ghost("systemd", "cng", "/etc/machine-id")
        .file("/etc/machine-id", "0123456789abcdef0123456789abcdef\n")
}

/// `sssd.conf` is a `%ghost` `%config` file of `sssd-common`, so rpm
/// never verified it; the scan collects it as a modified owned config,
/// redacted, and both exports copy it and give it mode 0600. The
/// `%ghost` machine-id is never collected.
#[test]
fn a_ghost_sssd_conf_is_carried_with_its_mode_on_both_export_paths() {
    let snap = scan(sssd_host());

    let sssd = entry(&snap, "/etc/sssd/sssd.conf").expect("the ghost sssd.conf is collected");
    assert_eq!(sssd.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(sssd.package.as_deref(), Some("sssd-common"));
    assert_eq!(sssd.rpm_va_flags, None);
    assert!(sssd.disposition.is_included());
    assert!(
        !sssd.content.contains(BIND_PASSWORD),
        "the bind password is redacted: {}",
        sssd.content
    );
    assert!(
        sssd.content
            .contains("ldap_default_authtok_type = password")
    );
    assert!(
        entry(&snap, "/etc/machine-id").is_none(),
        "the ghost machine-id is host identity and is not collected"
    );

    for (path, tree) in both_exports(&snap) {
        let staged = std::fs::read_to_string(tree.path().join("config/etc/sssd/sssd.conf"))
            .unwrap_or_else(|e| panic!("{path}: sssd.conf is staged: {e}"));
        assert!(staged.contains("id_provider = ldap"), "{path}");
        assert!(!staged.contains(BIND_PASSWORD), "{path}");
        assert!(
            !tree.path().join("config/etc/machine-id").exists(),
            "{path}: no machine-id is staged"
        );

        let cf = containerfile(tree.path());
        let copy = cf
            .find("COPY config/etc/ /etc/")
            .unwrap_or_else(|| panic!("{path}: the config tree is copied:\n{cf}"));
        let main = cf
            .find("RUN chmod 0600 /etc/sssd/sssd.conf\n")
            .unwrap_or_else(|| panic!("{path}: sssd.conf gets mode 0600:\n{cf}"));
        let snippets = cf
            .find("RUN chmod 0600 /etc/sssd/conf.d/*.conf\n")
            .unwrap_or_else(|| panic!("{path}: conf.d snippets get mode 0600:\n{cf}"));
        assert!(
            copy < main && copy < snippets,
            "{path}: chmod after the COPY"
        );
    }
}

/// With `sssd.conf` excluded in refine, the export neither copies it nor
/// changes its mode.
#[test]
fn an_excluded_sssd_conf_gets_no_chmod() {
    use inspectah_refine::types::{ItemId, RefinementOp};

    let snap = scan(
        SysctlHost::new()
            .ghost("sssd-common", "cng", "/etc/sssd/sssd.conf")
            .file("/etc/sssd/sssd.conf", SSSD_CONF),
    );
    let mut refine = session(&snap);
    refine
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/sssd/sssd.conf".into(),
            },
            include: false,
        })
        .expect("the exclude applies");
    let tree = export_session(&refine);
    assert!(!tree.path().join("config/etc/sssd/sssd.conf").exists());
    assert!(!containerfile(tree.path()).contains("chmod 0600 /etc/sssd"));
}
