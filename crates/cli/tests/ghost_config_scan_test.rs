//! `%ghost` config files, end to end through the scan path.
//!
//! Every host is built with [`SysctlHost`], whose `ghost` rows answer the
//! config inspector's file-flags query the way `rpm -qa --queryformat
//! '[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES:shescape}\n]'` prints them, and run
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

/// The line both exports render the measured `authselect.conf`
/// (`sssd`, `with-faillock`, `with-mkhomedir`) as.
const SELECT_LINE: &str = "RUN authselect select sssd with-faillock with-mkhomedir --force\n";

/// A file measured in a rhel-bootc base image after `authselect select
/// sssd with-faillock with-mkhomedir --force`, from
/// `testdata/fixtures/config/authselect/<release>/`.
fn measured(release: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/config/authselect")
        .join(release)
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The releases measured, with the exit code `authselect check` gave
/// after a line was appended to `system-auth`, mode and owner kept:
/// authselect 1.2.6 exits 3 and names the file, 1.5.2 exits 0.
const RELEASES: [(&str, i32); 2] = [("rhel-9.8", 3), ("rhel-10.2", 0)];

/// [`sssd_host`] with authselect's `%ghost` files (flag `g`, measured on
/// RHEL 9.8 and 10.2) as `release` generated them, `system-auth` with the
/// appended line when `edited`, and `authselect check` and `authselect
/// test` answering with the measured output.
fn authselect_host(release: &str, edited: bool, check: (i32, &str, &str)) -> SysctlHost {
    let system_auth = if edited {
        "system-auth.edited"
    } else {
        "system-auth"
    };
    sssd_host()
        .ghost("authselect-libs", "g", "/etc/authselect/authselect.conf")
        .file(
            "/etc/authselect/authselect.conf",
            &measured(release, "authselect.conf"),
        )
        .ghost("authselect-libs", "g", "/etc/authselect/nsswitch.conf")
        .file(
            "/etc/authselect/nsswitch.conf",
            &measured(release, "nsswitch.conf"),
        )
        .ghost("authselect-libs", "g", "/etc/authselect/system-auth")
        .file(
            "/etc/authselect/system-auth",
            &measured(release, system_auth),
        )
        .command("authselect check", check.0, check.1, check.2)
        .command(
            "authselect test --system-auth sssd with-faillock with-mkhomedir",
            0,
            &measured(release, "test-system-auth.stdout"),
            "",
        )
        .command(
            "authselect test --nsswitch sssd with-faillock with-mkhomedir",
            0,
            &measured(release, "test-nsswitch.stdout"),
            "",
        )
}

/// What `authselect check` printed for a configuration as it wrote it.
const CHECK_VALID: (i32, &str, &str) = (0, "Current configuration is valid.\n", "");

/// authselect's selection is rendered as `authselect select` on both
/// export paths and never copied; the generated `nsswitch.conf` is not
/// collected; a hand-edited generated file is an advisory that neither
/// export copies. On RHEL 9 `authselect check` names the edit; on RHEL 10
/// it calls the edited configuration valid, and the comparison with
/// `authselect test` finds it.
#[test]
fn authselect_files_render_as_authselect_select_on_both_export_paths() {
    for (release, content_edit_exit) in RELEASES {
        let check_out = measured(release, "check-content-edit.stdout");
        let check_err = measured(release, "check-content-edit.stderr");
        let snap = scan(authselect_host(
            release,
            true,
            (content_edit_exit, &check_out, &check_err),
        ));

        let conf = entry(&snap, "/etc/authselect/authselect.conf")
            .unwrap_or_else(|| panic!("{release}: the selection is collected"));
        assert!(conf.disposition.is_included(), "{release}");
        assert_eq!(conf.package.as_deref(), Some("authselect-libs"));
        assert!(
            entry(&snap, "/etc/authselect/nsswitch.conf").is_none(),
            "{release}: authselect select writes nsswitch.conf again; it is not collected"
        );
        let edited = entry(&snap, "/etc/authselect/system-auth")
            .unwrap_or_else(|| panic!("{release}: the hand-edited generated file is named"));
        assert!(
            edited.disposition.is_advisory(),
            "{release}: {:?}",
            edited.disposition
        );
        assert!(!edited.disposition.is_included());
        assert!(edited.content.contains("# hand edit"), "{release}");
        assert!(
            !snap
                .warnings
                .iter()
                .any(|w| w.message.contains("authselect")),
            "{release}: {:?}",
            snap.warnings
        );

        for (path, tree) in both_exports(&snap) {
            for staged in ["authselect.conf", "nsswitch.conf", "system-auth"] {
                assert!(
                    !tree
                        .path()
                        .join("config/etc/authselect")
                        .join(staged)
                        .exists(),
                    "{release} {path}: {staged} is not copied"
                );
            }
            let cf = containerfile(tree.path());
            let copy = cf
                .find("COPY config/etc/ /etc/")
                .unwrap_or_else(|| panic!("{release} {path}: the config tree is copied:\n{cf}"));
            let select = cf
                .find(SELECT_LINE)
                .unwrap_or_else(|| panic!("{release} {path}: the profile is selected:\n{cf}"));
            assert!(
                copy < select,
                "{release} {path}: select runs after the COPY"
            );
            assert_eq!(
                cf.matches("authselect select").count(),
                2,
                "{release} {path}: one line and its comment"
            );
        }
    }
}

/// Generated files as authselect wrote them add nothing on either
/// release; excluding the selection in refine removes the line.
#[test]
fn a_clean_authselect_configuration_adds_no_advisory_and_exclusion_drops_the_line() {
    use inspectah_refine::types::{ItemId, RefinementOp};

    for (release, _) in RELEASES {
        let snap = scan(authselect_host(release, false, CHECK_VALID));
        assert!(
            entry(&snap, "/etc/authselect/system-auth").is_none(),
            "{release}"
        );
        assert!(
            !snap
                .warnings
                .iter()
                .any(|w| w.message.contains("authselect")),
            "{release}: {:?}",
            snap.warnings
        );

        let mut refine = session(&snap);
        refine
            .apply(RefinementOp::SetInclude {
                item_id: ItemId::Config {
                    path: "/etc/authselect/authselect.conf".into(),
                },
                include: false,
            })
            .expect("the exclude applies");
        let tree = export_session(&refine);
        assert!(!containerfile(tree.path()).contains("authselect select"));
    }
}

/// With no `authselect` command to answer, nothing is known about the
/// generated files. That is a scan warning naming why, never a clean
/// result, and no file is made an advisory on a guess.
#[test]
fn an_authselect_that_cannot_answer_is_a_warning_not_a_clean_result() {
    let host = sssd_host()
        .ghost("authselect-libs", "g", "/etc/authselect/authselect.conf")
        .file(
            "/etc/authselect/authselect.conf",
            &measured("rhel-9.8", "authselect.conf"),
        )
        .ghost("authselect-libs", "g", "/etc/authselect/system-auth")
        .file(
            "/etc/authselect/system-auth",
            &measured("rhel-9.8", "system-auth.edited"),
        );
    let snap = scan(host);

    assert!(entry(&snap, "/etc/authselect/system-auth").is_none());
    let warning = snap
        .warnings
        .iter()
        .find(|w| w.message.contains("could not all be checked"))
        .unwrap_or_else(|| {
            panic!(
                "a warning says the files were not checked: {:?}",
                snap.warnings
            )
        });
    assert!(
        warning.message.contains("authselect check exited 127"),
        "{}",
        warning.message
    );
    assert!(
        warning
            .message
            .contains("authselect test --system-auth exited 127"),
        "{}",
        warning.message
    );
}

/// An admin's `/etc/default/grub`, edited as driftify edits it.
const DEFAULT_GRUB: &str = "GRUB_TIMEOUT=5\n\
     GRUB_CMDLINE_LINUX=\"rd.lvm.lv=rhel/root rhgb quiet transparent_hugepage=never audit=1\"\n";

/// `/etc/default/grub` is a `%ghost` `%config` file of grub2-tools
/// (`cng`, measured on Fedora 44, RHEL 9.8 and 10.2). The scan collects
/// it with its content as an advisory that names kargs.d, and neither
/// export copies it.
#[test]
fn a_ghost_default_grub_is_an_advisory_and_never_copied() {
    let snap = scan(
        sssd_host()
            .ghost("grub2-tools", "cng", "/etc/default/grub")
            .file("/etc/default/grub", DEFAULT_GRUB),
    );

    let grub = entry(&snap, "/etc/default/grub").expect("the ghost grub defaults are collected");
    assert!(
        grub.content.contains("audit=1"),
        "the content stays visible"
    );
    match &grub.disposition {
        inspectah_core::types::FindingKind::Advisory { rationale, .. } => {
            assert!(rationale.contains("kargs.d"), "{rationale}");
        }
        other => panic!("expected an advisory, got {other:?}"),
    }

    for (path, tree) in both_exports(&snap) {
        assert!(
            !tree.path().join("config/etc/default/grub").exists(),
            "{path}: /etc/default/grub is never copied"
        );
        assert!(
            tree.path().join("config/etc/sssd/sssd.conf").exists(),
            "{path}"
        );
    }
}

/// `rpm -Va` reports a ghost file's mode change (`.M.......`, as rpm
/// printed it for a `chmod`ed ghost file on RHEL 9.8 and 10.2) and never
/// its content. The `rpm -Va` branch drops that row as metadata-only, and
/// the ghost branch still carries the file on both export paths.
#[test]
fn a_ghost_sssd_conf_with_a_mode_change_is_still_carried() {
    let snap = scan(sssd_host().rpm_va(".M.......", "/etc/sssd/sssd.conf"));

    let sssd = entry(&snap, "/etc/sssd/sssd.conf").expect("the ghost sssd.conf is collected");
    assert_eq!(sssd.kind, ConfigFileKind::RpmOwnedModified);
    assert_eq!(sssd.rpm_va_flags, None);
    assert!(sssd.disposition.is_included());
    for (path, tree) in both_exports(&snap) {
        let staged = std::fs::read_to_string(tree.path().join("config/etc/sssd/sssd.conf"))
            .unwrap_or_else(|e| panic!("{path}: sssd.conf is staged: {e}"));
        assert!(staged.contains("id_provider = ldap"), "{path}");
    }
}

/// An encrypted private key, the whole of a file, as `openssl pkcs8
/// -topk8` writes it. The body is not a real key.
const ENCRYPTED_KEY_FILE: &str = "\
-----BEGIN ENCRYPTED PRIVATE KEY-----
MIIFHDBOBgkqhkiG9w0BBQ0wQTApBgkqhkiG9w0BBQwwHAQIl2ghostkeymaterial
-----END ENCRYPTED PRIVATE KEY-----
";

/// A keytab's opening bytes: format version 2, then the start of an
/// entry. Valid UTF-8, so a text read succeeds and only the content
/// says what the file is.
const KEYTAB_CONTENT: &str = "\u{5}\u{2}\0\0\0\u{3c}\0\u{1}\0\u{b}EXAMPLE.COM";

/// A ghost file whose content is a private key and nothing else, or a
/// keytab, is a machine-state credential at any path. Neither path is on
/// the class's path list, so only the content can say so: neither reaches
/// the snapshot or either export.
#[test]
fn ghost_files_holding_a_credential_by_content_are_never_collected() {
    let snap = scan(
        SysctlHost::new()
            .ghost("myapp", "g", "/etc/myapp/server.pem")
            .file("/etc/myapp/server.pem", ENCRYPTED_KEY_FILE)
            .ghost("myapp", "g", "/etc/myapp/service.kt")
            .file("/etc/myapp/service.kt", KEYTAB_CONTENT)
            .ghost("myapp", "cng", "/etc/myapp/app.conf")
            .file("/etc/myapp/app.conf", "listen = 8443\n"),
    );

    assert!(entry(&snap, "/etc/myapp/app.conf").is_some(), "the control");
    for path in ["/etc/myapp/server.pem", "/etc/myapp/service.kt"] {
        assert!(entry(&snap, path).is_none(), "{path} is not collected");
    }
    let json = serde_json::to_string(&snap).expect("the snapshot serializes");
    assert!(!json.contains("ghostkeymaterial"));
    assert!(!json.contains("EXAMPLE.COM"));

    for (path, tree) in both_exports(&snap) {
        assert!(
            tree.path().join("config/etc/myapp/app.conf").exists(),
            "{path}"
        );
        for staged in ["config/etc/myapp/server.pem", "config/etc/myapp/service.kt"] {
            assert!(!tree.path().join(staged).exists(), "{path}: {staged}");
        }
    }
}

/// A ghost configuration file with an encrypted private key inside it is
/// configuration, not a key file, so it is collected; the key block is
/// redacted in the snapshot and in both exports, and the settings around
/// it survive.
#[test]
fn an_encrypted_key_inside_a_ghost_config_is_redacted_on_both_export_paths() {
    let conf = format!("listen = 8443\n{ENCRYPTED_KEY_FILE}workers = 4\n");
    let snap = scan(
        SysctlHost::new()
            .ghost("myapp", "cng", "/etc/myapp/tls.conf")
            .file("/etc/myapp/tls.conf", &conf),
    );

    let tls = entry(&snap, "/etc/myapp/tls.conf").expect("the ghost config is collected");
    assert!(!tls.content.contains("ghostkeymaterial"), "{}", tls.content);
    assert!(
        tls.content.contains("REDACTED_PRIVATEKEY_"),
        "{}",
        tls.content
    );
    for (path, tree) in both_exports(&snap) {
        let staged = std::fs::read_to_string(tree.path().join("config/etc/myapp/tls.conf"))
            .unwrap_or_else(|e| panic!("{path}: tls.conf is staged: {e}"));
        assert!(!staged.contains("ghostkeymaterial"), "{path}: {staged}");
        assert!(staged.contains("listen = 8443\n"), "{path}");
        assert!(staged.contains("workers = 4\n"), "{path}");
    }
}

/// Four LDAP domains whose bind passwords hold what the generic
/// `password=` pattern mishandles: spaces, a tab, quotes, and a password
/// spelled like an NSS source.
const SSSD_AWKWARD_PASSWORDS: &str = "\
[sssd]
domains = a, b, c, d
services = nss, pam

[domain/a]
id_provider = ldap
ldap_default_authtok_type = password
ldap_default_authtok = Bind Secret Tail

[domain/b]
id_provider = ldap
ldap_default_authtok =\tTabbed\tWord

[domain/c]
id_provider = ldap
ldap_default_authtok = \"Quoted Phrase\"

[domain/d]
id_provider = ldap
ldap_default_authtok = files
";

/// Each SSSD bind password is redacted whole, from the collected ghost
/// file to both exports: no word of any of them survives, every line
/// keeps its key, and the file keeps its line count.
#[test]
fn sssd_bind_passwords_are_redacted_whole_on_both_export_paths() {
    let snap = scan(
        SysctlHost::new()
            .ghost("sssd-common", "cng", "/etc/sssd/sssd.conf")
            .file("/etc/sssd/sssd.conf", SSSD_AWKWARD_PASSWORDS),
    );
    let sssd = entry(&snap, "/etc/sssd/sssd.conf").expect("the ghost sssd.conf is collected");

    let mut texts = vec![("snapshot".to_string(), sssd.content.clone())];
    for (path, tree) in both_exports(&snap) {
        let staged = std::fs::read_to_string(tree.path().join("config/etc/sssd/sssd.conf"))
            .unwrap_or_else(|e| panic!("{path}: sssd.conf is staged: {e}"));
        texts.push((path.to_string(), staged));
    }
    for (where_, text) in texts {
        for word in [
            "Bind", "Secret", "Tail", "Tabbed", "Word", "Quoted", "Phrase",
        ] {
            assert!(!text.contains(word), "{where_}: {word} survived:\n{text}");
        }
        assert!(
            !text.contains("ldap_default_authtok = files"),
            "{where_}: the NSS-word password survived:\n{text}"
        );
        assert_eq!(
            text.lines()
                .filter(|l| l.starts_with("ldap_default_authtok ="))
                .count(),
            4,
            "{where_}:\n{text}"
        );
        assert!(
            text.contains("ldap_default_authtok_type = password\n"),
            "{where_}"
        );
        assert_eq!(
            text.lines().count(),
            SSSD_AWKWARD_PASSWORDS.lines().count(),
            "{where_}"
        );
    }
}

/// The files `authselect create-profile corp -b sssd --symlink-meta
/// --symlink-pam` links into the shipped profile, as listed on RHEL 9.8
/// (RHEL 10.2 adds `switchable-auth`).
const LINKED_PROFILE_FILES: [&str; 7] = [
    "README",
    "REQUIREMENTS",
    "fingerprint-auth",
    "password-auth",
    "postlogin",
    "smartcard-auth",
    "system-auth",
];

/// The files the same command copies into the custom profile.
const COPIED_PROFILE_FILES: [&str; 3] = ["dconf-db", "dconf-locks", "nsswitch.conf"];

/// A host that selected `custom/corp` with `with-faillock`, made as
/// above: links into `/usr/share/authselect/vendor/sssd/` and copied
/// files, each holding a line that names it.
fn custom_profile_host() -> SysctlHost {
    let mut host = sssd_host()
        .ghost("authselect-libs", "g", "/etc/authselect/authselect.conf")
        .file(
            "/etc/authselect/authselect.conf",
            "custom/corp\nwith-faillock\n",
        )
        .command(
            "authselect check",
            CHECK_VALID.0,
            CHECK_VALID.1,
            CHECK_VALID.2,
        );
    for name in LINKED_PROFILE_FILES {
        let shipped = format!("/usr/share/authselect/vendor/sssd/{name}");
        host = host
            .file(&shipped, &format!("# shipped {name}\n"))
            .link(&format!("/etc/authselect/custom/corp/{name}"), &shipped);
    }
    for name in COPIED_PROFILE_FILES {
        host = host.file(
            &format!("/etc/authselect/custom/corp/{name}"),
            &format!("# copied {name}\n"),
        );
    }
    host
}

/// A custom profile made with `--symlink-meta --symlink-pam` is carried
/// whole: each link into the shipped profiles is collected as a file
/// with what it resolves to, both exports stage every file of the
/// profile, and `authselect select custom/corp` runs after the COPY. A
/// build from such a context selected the profile and passed `authselect
/// check` in rhel-bootc 9.8 and 10.2.
#[test]
fn a_custom_profile_with_shipped_links_is_carried_whole_on_both_export_paths() {
    let snap = scan(custom_profile_host());

    for name in LINKED_PROFILE_FILES {
        let path = format!("/etc/authselect/custom/corp/{name}");
        let file = entry(&snap, &path).unwrap_or_else(|| panic!("{path} is collected"));
        assert!(
            file.disposition.is_included(),
            "{path}: {:?}",
            file.disposition
        );
        assert_eq!(file.content, format!("# shipped {name}\n"), "{path}");
    }
    for (path, tree) in both_exports(&snap) {
        for name in LINKED_PROFILE_FILES.iter().chain(&COPIED_PROFILE_FILES) {
            let staged = tree
                .path()
                .join("config/etc/authselect/custom/corp")
                .join(name);
            assert!(
                std::fs::symlink_metadata(&staged).is_ok_and(|m| m.is_file()),
                "{path}: {name} is staged as a file"
            );
        }
        let cf = containerfile(tree.path());
        let copy = cf
            .find("COPY config/etc/ /etc/")
            .unwrap_or_else(|| panic!("{path}: the config tree is copied:\n{cf}"));
        let select = cf
            .find("RUN authselect select custom/corp with-faillock --force\n")
            .unwrap_or_else(|| panic!("{path}: the custom profile is selected:\n{cf}"));
        assert!(copy < select, "{path}");
        assert!(!cf.contains("FIXME: authselect"), "{path}:\n{cf}");
    }
}

/// With a file of the custom profile left out of the export, here the
/// `README` excluded in refine, `authselect select` would fail the build
/// (README) or write an empty PAM stack (a template). The export names
/// the missing file in a FIXME and renders no select line.
#[test]
fn a_custom_profile_missing_a_file_is_a_fixme_not_a_select_line() {
    use inspectah_refine::types::{ItemId, RefinementOp};

    let snap = scan(custom_profile_host());
    let mut refine = session(&snap);
    refine
        .apply(RefinementOp::SetInclude {
            item_id: ItemId::Config {
                path: "/etc/authselect/custom/corp/README".into(),
            },
            include: false,
        })
        .expect("the exclude applies");
    let cf = containerfile(export_session(&refine).path());
    assert!(!cf.contains("RUN authselect select"), "{cf}");
    let fixme = cf
        .lines()
        .find(|l| l.starts_with("# FIXME: authselect profile custom/corp not selected"))
        .unwrap_or_else(|| panic!("a FIXME names the profile:\n{cf}"));
    assert!(
        fixme.contains("/etc/authselect/custom/corp/README"),
        "{fixme}"
    );
    assert!(
        fixme.contains("authselect select custom/corp with-faillock --force"),
        "{fixme}"
    );
}

/// `/etc/fstab` is a `%ghost` file of `setup` on RHEL 9.8 and 10.2, and
/// `redhat.repo` one of `subscription-manager`. Storage and the repos
/// section own them, so the ghost branch never copies either into the
/// image. `/etc/chrony.keys` (a ghost of `chrony` on RHEL 10.2) is key
/// material: withheld, named by a scan warning, its content nowhere.
#[test]
fn ghost_fstab_repo_and_key_files_are_never_copied_on_either_export_path() {
    const CHRONY_KEY: &str = "HEX:4D6F6E6B65795365637265744B6579";
    let snap = scan(
        sssd_host()
            .ghost("setup", "cmng", "/etc/fstab")
            .file(
                "/etc/fstab",
                "UUID=0a1b2c3d-0000-4000-8000-000000000001 / xfs defaults 0 0\n",
            )
            .ghost("subscription-manager", "g", "/etc/yum.repos.d/redhat.repo")
            .file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-sub]\nname=Subscribed\nbaseurl=https://cdn.example.com/rhel\nenabled=0\n",
            )
            .ghost("chrony", "cg", "/etc/chrony.keys")
            .file("/etc/chrony.keys", &format!("1 SHA1 {CHRONY_KEY}\n")),
    );

    for path in [
        "/etc/fstab",
        "/etc/yum.repos.d/redhat.repo",
        "/etc/chrony.keys",
    ] {
        assert!(entry(&snap, path).is_none(), "{path} is not a config entry");
    }
    let json = serde_json::to_string(&snap).expect("the snapshot serializes");
    assert!(
        !json.contains(CHRONY_KEY),
        "the key is nowhere in the snapshot"
    );
    assert!(
        snap.warnings
            .iter()
            .any(|w| w.message.starts_with("/etc/chrony.keys holds key material")),
        "{:?}",
        snap.warnings
    );

    for (path, tree) in both_exports(&snap) {
        for staged in [
            "config/etc/fstab",
            "config/etc/chrony.keys",
            "config/etc/yum.repos.d/redhat.repo",
        ] {
            assert!(!tree.path().join(staged).exists(), "{path}: {staged}");
        }
        assert!(
            tree.path().join("config/etc/sssd/sssd.conf").exists(),
            "{path}: the control"
        );
        let containerfile = containerfile(tree.path());
        assert!(!containerfile.contains("chrony.keys"), "{path}");
        assert!(!containerfile.contains(CHRONY_KEY), "{path}");
    }
}
