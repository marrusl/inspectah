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

/// The withheld machine-state row at `path`.
fn withheld<'a>(
    snap: &'a InspectionSnapshot,
    path: &str,
) -> Option<&'a inspectah_core::types::machine_state::WithheldConfigEntry> {
    snap.config
        .as_ref()
        .expect("the config section is present")
        .withheld
        .iter()
        .find(|w| w.path == path)
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
/// (`cng`, measured on Fedora 44, RHEL 9.8 and 10.2). Its
/// `GRUB_CMDLINE_LINUX` sets two arguments the running kernel
/// ([`SysctlHost`]'s `/proc/cmdline`) lacks, so the scan collects it with
/// its content as an advisory that names them, without their values, and
/// kargs.d; neither export copies it.
#[test]
fn a_ghost_default_grub_with_arguments_not_running_is_an_advisory_and_never_copied() {
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
            assert!(
                rationale.contains("does not carry them: transparent_hugepage, audit."),
                "{rationale}"
            );
            assert!(!rationale.contains("never"), "no value: {rationale}");
            assert!(!rationale.contains("audit=1"), "no value: {rationale}");
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

/// A `/etc/default/grub` whose kernel arguments all run now tells the
/// user nothing: the running ones are carried in kargs.d. The scan keeps
/// no row for it, and neither export copies it.
#[test]
fn a_ghost_default_grub_matching_the_running_kernel_is_not_reported_or_copied() {
    let snap = scan(
        sssd_host()
            .file(
                "/proc/cmdline",
                "BOOT_IMAGE=/vmlinuz root=/dev/mapper/rhel-root ro rd.lvm.lv=rhel/root rhgb \
                 quiet transparent_hugepage=never audit=1\n",
            )
            .ghost("grub2-tools", "cng", "/etc/default/grub")
            .file("/etc/default/grub", DEFAULT_GRUB),
    );

    assert!(
        entry(&snap, "/etc/default/grub").is_none(),
        "no advisory and no row for a file that matches the running kernel"
    );
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
///
/// The same files under a directory alias (`/etc/myapp/live -> .`), which
/// the unowned walk enters, are refused by the read gate the same way:
/// every branch reads through it, so the alias spelling is a withheld row
/// and carries no byte of either credential.
#[test]
fn ghost_files_holding_a_credential_by_content_are_never_collected() {
    let snap = scan(
        SysctlHost::new()
            .ghost("myapp", "g", "/etc/myapp/server.pem")
            .file("/etc/myapp/server.pem", ENCRYPTED_KEY_FILE)
            .ghost("myapp", "g", "/etc/myapp/service.kt")
            .file("/etc/myapp/service.kt", KEYTAB_CONTENT)
            .ghost("myapp", "cng", "/etc/myapp/app.conf")
            .file("/etc/myapp/app.conf", "listen = 8443\n")
            .link("/etc/myapp/live", "."),
    );

    assert!(entry(&snap, "/etc/myapp/app.conf").is_some(), "the control");
    for (path, member) in [
        ("/etc/myapp/server.pem", "private_key_file"),
        ("/etc/myapp/service.kt", "kerberos_keytab"),
        ("/etc/myapp/live/server.pem", "private_key_file"),
        ("/etc/myapp/live/service.kt", "kerberos_keytab"),
    ] {
        assert!(entry(&snap, path).is_none(), "{path} is not collected");
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member.as_str(), member, "{path}");
    }
    for (where_, text) in everywhere(&snap) {
        assert!(!text.contains("ghostkeymaterial"), "{where_}");
        assert!(!text.contains("EXAMPLE.COM"), "{where_}");
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
/// material: a withheld machine-state row, its content nowhere.
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
        withheld(&snap, "/etc/chrony.keys").is_some(),
        "the key file is a withheld machine-state row"
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

/// `/etc/chrony.keys` content: an NTP authentication key that no
/// redaction pattern recognizes, so only admission keeps it out.
const CHRONY_KEY: &str = "HEX:4D6F6E6B65795365637265744B6579";
/// A shadow hash, an outside-`/etc` token and an Insights machine ID,
/// each unique to the case that plants it.
const SHADOW_HASH: &str = "$6$saltsalt$AncestorAliasHash";
const VAR_TOKEN: &str = "token-behind-an-ancestor-link";
const INSIGHTS_ID: &str = "insights-id-through-a-final-link";
const ROOT_SECRET: &str = "root-secret-through-a-final-link";

/// The snapshot as JSON, and every file of both export trees, so a test
/// can say a value is nowhere.
fn everywhere(snap: &InspectionSnapshot) -> Vec<(String, String)> {
    let mut texts = vec![(
        "snapshot".to_string(),
        serde_json::to_string(snap).expect("the snapshot serializes"),
    )];
    for (path, tree) in both_exports(snap) {
        let mut dirs = vec![tree.path().to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("an export directory lists") {
                let entry = entry.expect("an export entry reads");
                let file = entry.path();
                if file.is_dir() {
                    dirs.push(file);
                } else if let Ok(text) = std::fs::read_to_string(&file) {
                    texts.push((format!("{path}: {}", file.display()), text));
                }
            }
        }
    }
    texts
}

/// Paths under `/etc` that name a file elsewhere through a directory
/// link are never read, whatever rule the file they lead to falls under:
/// a credential (`chrony.keys`), host identity (`machine-id`), another
/// section's file (`shadow`), and a file outside `/etc`. A credential or
/// identity file is a machine-state member under either name: the read
/// gate withholds it by its resolved path, and it is a withheld row under
/// the alias. Another section's file and one outside `/etc` are packaged
/// `%ghost` paths named by a scan warning. A final link to a member, a
/// modified packaged file re-pointed at the key file or an unowned link
/// to the Insights id, is a withheld row too, and a link leaving `/etc`
/// is a content-free advisory.
#[test]
fn a_path_through_a_linked_directory_or_to_a_withheld_file_is_never_read() {
    let snap = scan(
        sssd_host()
            .ghost("chrony", "cg", "/etc/chrony.keys")
            .file("/etc/chrony.keys", &format!("1 SHA1 {CHRONY_KEY}\n"))
            .rpm_owned("/etc/shadow", "setup")
            .file(
                "/etc/shadow",
                &format!("root:{SHADOW_HASH}:19000:0:99999:7:::\n"),
            )
            .file("/var/lib/app/token", &format!("{VAR_TOKEN}\n"))
            .link("/etc/app/state", "/etc")
            .link("/etc/app/data", "/var/lib/app")
            .ghost("app", "g", "/etc/app/state/chrony.keys")
            .ghost("app", "g", "/etc/app/state/machine-id")
            .ghost("app", "g", "/etc/app/state/shadow")
            .ghost("app", "g", "/etc/app/data/token")
            .rpm_owned("/etc/app/app.conf", "app")
            .rpm_va("S.5....T.", "/etc/app/app.conf")
            .link("/etc/app/app.conf", "/etc/chrony.keys")
            .file(
                "/etc/insights-client/machine-id",
                &format!("{INSIGHTS_ID}\n"),
            )
            .link("/etc/app/insights-id", "/etc/insights-client/machine-id")
            .file("/root/token", &format!("{ROOT_SECRET}\n"))
            .link("/etc/app/root-token", "/root/token"),
    );

    for (where_, text) in everywhere(&snap) {
        for secret in [CHRONY_KEY, SHADOW_HASH, VAR_TOKEN, INSIGHTS_ID, ROOT_SECRET] {
            assert!(!text.contains(secret), "{where_}: {secret}");
        }
    }
    for (path, member) in [
        ("/etc/app/state/chrony.keys", "service_key"),
        ("/etc/app/state/machine-id", "machine_id"),
        ("/etc/app/app.conf", "service_key"),
        ("/etc/app/insights-id", "insights_state"),
    ] {
        assert!(entry(&snap, path).is_none(), "{path} is not a config entry");
        let row = withheld(&snap, path).unwrap_or_else(|| panic!("{path} is withheld"));
        assert_eq!(row.member.as_str(), member, "{path}");
    }
    for path in ["/etc/app/state/shadow", "/etc/app/data/token"] {
        assert!(entry(&snap, path).is_none(), "{path} is not a config entry");
        assert!(
            snap.warnings.iter().any(|w| w.message.starts_with(&format!(
                "{path} was not read and is not carried: a directory above it is a symbolic link"
            ))),
            "{path} is named: {:?}",
            snap.warnings
        );
    }
    let root_link = entry(&snap, "/etc/app/root-token").expect("the link is an advisory");
    assert!(root_link.content.is_empty());
    assert!(!root_link.disposition.is_included(), "{root_link:?}");

    for (path, tree) in both_exports(&snap) {
        assert!(!tree.path().join("config/etc/app").exists(), "{path}");
        assert!(
            tree.path().join("config/etc/sssd/sssd.conf").exists(),
            "{path}: the control"
        );
    }
}

/// An alias of the repo directory: the walk reaches `redhat.repo` under
/// `/etc/repo-alias/`, a spelling no ownership or section claim matches.
/// The repos section reads the file under its own name and leaves it out
/// of both exports as the subscription's default repo; the alias is never
/// collected, so it cannot carry the file in by another name.
#[test]
fn a_repo_directory_alias_does_not_carry_an_excluded_repo() {
    const REPO_URL: &str = "https://cdn.example.com/rhel-through-alias";
    let snap = scan(
        sssd_host()
            .ghost("subscription-manager", "g", "/etc/yum.repos.d/redhat.repo")
            .file(
                "/etc/yum.repos.d/redhat.repo",
                &format!("[rhel-sub]\nname=Subscribed\nbaseurl={REPO_URL}\nenabled=1\n"),
            )
            .link("/etc/repo-alias", "yum.repos.d"),
    );
    let config = &snap.config.as_ref().expect("a config section").files;
    assert!(
        !config
            .iter()
            .any(|f| f.path.starts_with("/etc/repo-alias/")),
        "{:?}",
        config.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    let repo = snap
        .rpm
        .as_ref()
        .expect("an rpm section")
        .repo_files
        .iter()
        .find(|r| r.path == "/etc/yum.repos.d/redhat.repo")
        .expect("the repos section reads redhat.repo");
    assert!(
        !repo.disposition.is_included(),
        "the default repo is left out"
    );

    for (path, tree) in both_exports(&snap) {
        assert!(
            !tree.path().join("config/etc/repo-alias").exists(),
            "{path}"
        );
        assert!(
            !tree
                .path()
                .join("config/etc/yum.repos.d/redhat.repo")
                .exists(),
            "{path}"
        );
        assert!(!containerfile(tree.path()).contains("repo-alias"), "{path}");
    }
}

/// No read path opens a FIFO, socket or device: DHCP detection over a
/// NetworkManager connection, a ghost file rpm reported only for its
/// metadata (`.M.......`), and orphan detection for a removed package,
/// each on a host of its own. `special_file` paths panic when read.
#[test]
fn no_read_path_opens_a_special_file() {
    let control = || SysctlHost::new().file("/etc/app.conf", "setting=value\n");
    let hosts = [
        (
            "DHCP detection",
            control().special_file("/etc/NetworkManager/system-connections/control.nmconnection"),
        ),
        (
            "metadata-only rpm -Va row",
            control()
                .ghost("sssd-common", "cng", "/etc/sssd/sssd.conf")
                .special_file("/etc/sssd/sssd.conf")
                .rpm_va(".M.......", "/etc/sssd/sssd.conf"),
        ),
        (
            "orphan detection",
            control().special_file("/etc/oldapp.fifo").command(
                "dnf history list --reverse",
                0,
                "oldapp Removed\n",
                "",
            ),
        ),
    ];
    for (case, host) in hosts {
        let snap = scan(host);
        let paths: Vec<&str> = snap
            .config
            .as_ref()
            .unwrap_or_else(|| panic!("{case}: a config section"))
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(paths, ["/etc/app.conf"], "{case}");
        for (path, tree) in both_exports(&snap) {
            assert!(
                tree.path().join("config/etc/app.conf").exists(),
                "{case}: {path}"
            );
        }
    }
}

/// The packaged audit units, as measured on the test hosts.
fn audit_unit(release: &str, name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/fixtures/config/audit")
        .join(release)
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The rules `augenrules` wrote, or the admin keeps, in `audit.rules`.
const AUDIT_RULES: &str = "## This file is automatically generated from /etc/audit/rules.d\n-D\n-b 8192\n-w /etc/passwd -p wa -k identity\n";

/// Where the packaged audit units live.
const UNIT_DIR: &str = "/usr/lib/systemd/system";

/// A RHEL 9.8 host as packaged: the `%ghost` `audit.rules`, a `rules.d`
/// input, and `auditd.service` listed by `audit`, which runs `augenrules
/// --load`.
fn audit_host_el9() -> SysctlHost {
    sssd_host()
        .ghost("audit", "cng", "/etc/audit/audit.rules")
        .file("/etc/audit/audit.rules", AUDIT_RULES)
        .ghost("audit", "cng", "/etc/audit/rules.d/audit.rules")
        .file(
            "/etc/audit/rules.d/audit.rules",
            "-w /etc/passwd -p wa -k identity\n",
        )
        .packaged(
            "audit",
            &format!("{UNIT_DIR}/auditd.service"),
            &audit_unit("rhel-9.8", "auditd.service"),
        )
}

/// A RHEL 10.2 host as packaged: `audit-rules.service`, listed by
/// `audit-rules`, runs `augenrules --load`, and `auditd.service` loads
/// nothing.
fn audit_host_el10() -> SysctlHost {
    sssd_host()
        .ghost("audit-rules", "cng", "/etc/audit/audit.rules")
        .file("/etc/audit/audit.rules", AUDIT_RULES)
        .ghost("audit-rules", "cng", "/etc/audit/rules.d/audit.rules")
        .file(
            "/etc/audit/rules.d/audit.rules",
            "-w /etc/passwd -p wa -k identity\n",
        )
        .packaged(
            "audit",
            &format!("{UNIT_DIR}/auditd.service"),
            &audit_unit("rhel-10.2", "auditd.service"),
        )
        .packaged(
            "audit-rules",
            &format!("{UNIT_DIR}/audit-rules.service"),
            &audit_unit("rhel-10.2", "audit-rules.service"),
        )
}

/// Whether both exports stage `audit.rules` with the host's rules.
fn audit_rules_staged(snap: &InspectionSnapshot) -> [(&'static str, bool); 2] {
    both_exports(snap).map(|(path, tree)| {
        let staged = std::fs::read_to_string(tree.path().join("config/etc/audit/audit.rules"));
        (path, staged.is_ok_and(|text| text == AUDIT_RULES))
    })
}

/// With the loader units exactly as packaged, `augenrules` writes
/// `audit.rules` from `rules.d` at every start, so it is not carried on
/// RHEL 9 or 10, and no warning names it.
#[test]
fn audit_rules_with_the_packaged_units_are_not_carried() {
    for (release, host) in [
        ("rhel-9.8", audit_host_el9()),
        ("rhel-10.2", audit_host_el10()),
    ] {
        let snap = scan(host);
        assert!(
            entry(&snap, "/etc/audit/audit.rules").is_none(),
            "{release}"
        );
        assert!(
            !snap
                .warnings
                .iter()
                .any(|w| w.message.contains("audit.rules")),
            "{release}: {:?}",
            snap.warnings
        );
        for (path, staged) in audit_rules_staged(&snap) {
            assert!(!staged, "{release} {path}");
        }
    }
}

/// Anything that makes the loader units differ from the packages, found
/// by its presence and never read, carries `audit.rules` with a warning
/// that names the file and what was found: the documented `/etc` copy,
/// drop-ins (named, dash-prefix, type-wide, hidden, in `/run`), masks of
/// both kinds, a changed packaged unit, an unpackaged one, and a host
/// with no `rules.d`. Several of these load the file directly through
/// syntax a reader could misparse (a quoted `"--check"`, a continued
/// line); none is parsed.
#[test]
fn audit_rules_with_any_unit_change_are_carried_with_a_warning() {
    const DIRECT_EL10: &str =
        "[Service]\nExecStart=\nExecStart=/usr/sbin/auditctl -R /etc/audit/audit.rules\n";
    const QUOTED_CHECK_EL9: &str = "[Service]\nExecStartPre=-/sbin/augenrules \"--check\"\n\
         ExecStartPost=\nExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules\n";
    const CONTINUED_CHECK_EL9: &str = "[Service]\nExecStartPost=\n\
         ExecStartPost=-/sbin/augenrules \\\n    --check\n\
         ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules\n";
    let documented_el9 = audit_unit("rhel-9.8", "auditd.service")
        .replace(
            "ExecStartPost=-/sbin/augenrules --load",
            "#ExecStartPost=-/sbin/augenrules --load",
        )
        .replace(
            "#ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
            "ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
        );
    let cases: Vec<(&str, SysctlHost, &str)> = vec![
        (
            "documented /etc copy",
            audit_host_el9().file("/etc/systemd/system/auditd.service", &documented_el9),
            "/etc/systemd/system/auditd.service",
        ),
        (
            "named drop-in",
            audit_host_el10().file(
                "/etc/systemd/system/audit-rules.service.d/50-direct.conf",
                DIRECT_EL10,
            ),
            "/etc/systemd/system/audit-rules.service.d",
        ),
        (
            "dash-prefix drop-in",
            audit_host_el10().file(
                "/etc/systemd/system/audit-.service.d/50-direct.conf",
                DIRECT_EL10,
            ),
            "/etc/systemd/system/audit-.service.d",
        ),
        (
            "quoted --check",
            audit_host_el9().file(
                "/etc/systemd/system/auditd.service.d/50-check.conf",
                QUOTED_CHECK_EL9,
            ),
            "/etc/systemd/system/auditd.service.d",
        ),
        (
            "continued --check line",
            audit_host_el9().file(
                "/etc/systemd/system/auditd.service.d/50-check.conf",
                CONTINUED_CHECK_EL9,
            ),
            "/etc/systemd/system/auditd.service.d",
        ),
        (
            "type-wide drop-in",
            audit_host_el9().file("/etc/systemd/system/service.d/50-env.conf", "[Service]\n"),
            "/etc/systemd/system/service.d",
        ),
        (
            "hidden drop-in",
            audit_host_el9().file(
                "/etc/systemd/system/auditd.service.d/.50-direct.conf",
                "[Service]\n",
            ),
            "/etc/systemd/system/auditd.service.d",
        ),
        (
            "/run override",
            audit_host_el10().file(
                "/run/systemd/system/audit-rules.service.d/50-direct.conf",
                DIRECT_EL10,
            ),
            "/run/systemd/system/audit-rules.service.d",
        ),
        (
            "zero-length mask",
            audit_host_el10().file("/etc/systemd/system/audit-rules.service", ""),
            "/etc/systemd/system/audit-rules.service",
        ),
        (
            "/dev/null mask",
            audit_host_el9().link("/etc/systemd/system/auditd.service", "/dev/null"),
            "/etc/systemd/system/auditd.service",
        ),
        (
            "changed packaged unit",
            audit_host_el9().rpm_va("S.5....T.", "/usr/lib/systemd/system/auditd.service"),
            "/usr/lib/systemd/system/auditd.service",
        ),
        (
            "unpackaged unit under /usr/lib",
            audit_host_el9().file(
                "/usr/lib/systemd/system/audit-rules.service",
                "[Service]\nExecStart=/usr/sbin/auditctl -R /etc/audit/audit.rules\n",
            ),
            "/usr/lib/systemd/system/audit-rules.service",
        ),
        (
            "no rules.d",
            sssd_host()
                .ghost("audit", "cng", "/etc/audit/audit.rules")
                .file("/etc/audit/audit.rules", AUDIT_RULES)
                .packaged(
                    "audit",
                    &format!("{UNIT_DIR}/auditd.service"),
                    &audit_unit("rhel-9.8", "auditd.service"),
                ),
            "/etc/audit/rules.d",
        ),
    ];
    for (case, host, found) in cases {
        let snap = scan(host);
        let rules = entry(&snap, "/etc/audit/audit.rules")
            .unwrap_or_else(|| panic!("{case}: audit.rules is collected"));
        assert_eq!(rules.kind, ConfigFileKind::RpmOwnedModified, "{case}");
        assert!(rules.disposition.is_included(), "{case}");
        let warning = snap
            .warnings
            .iter()
            .find(|w| {
                w.message
                    .starts_with("/etc/audit/audit.rules is carried as it is")
            })
            .unwrap_or_else(|| panic!("{case}: a warning names the file: {:?}", snap.warnings));
        assert!(
            warning.message.contains(found),
            "{case}: {}",
            warning.message
        );
        for (path, staged) in audit_rules_staged(&snap) {
            assert!(staged, "{case} {path}");
        }
    }
}

/// The repos section reads only `*.repo` files directly in
/// `/etc/yum.repos.d`, and claims only those: a nested ghost file and one
/// with another suffix are carried as configuration, never dropped by
/// both.
#[test]
fn ghost_files_in_the_repo_directory_are_read_by_the_repos_section_or_carried() {
    let snap = scan(
        sssd_host()
            .ghost("subscription-manager", "g", "/etc/yum.repos.d/redhat.repo")
            .file(
                "/etc/yum.repos.d/redhat.repo",
                "[rhel-sub]\nname=Subscribed\nbaseurl=https://cdn.example.com/rhel\nenabled=0\n",
            )
            .ghost("app", "g", "/etc/yum.repos.d/sub/nested.repo")
            .file("/etc/yum.repos.d/sub/nested.repo", "[nested]\nname=N\n")
            .ghost("app", "g", "/etc/yum.repos.d/repos.conf")
            .file("/etc/yum.repos.d/repos.conf", "[conf]\nname=C\n")
            .ghost("app", "g", "/etc/yum.repos.d/redhat.repo.bak")
            .file("/etc/yum.repos.d/redhat.repo.bak", "[old]\nname=Old\n"),
    );
    let repos: Vec<&str> = snap
        .rpm
        .as_ref()
        .expect("an rpm section")
        .repo_files
        .iter()
        .map(|r| r.path.as_str())
        .collect();
    assert_eq!(repos, ["/etc/yum.repos.d/redhat.repo"]);
    assert!(entry(&snap, "/etc/yum.repos.d/redhat.repo").is_none());
    for path in [
        "/etc/yum.repos.d/sub/nested.repo",
        "/etc/yum.repos.d/repos.conf",
        "/etc/yum.repos.d/redhat.repo.bak",
    ] {
        let file = entry(&snap, path).unwrap_or_else(|| panic!("{path} is carried"));
        assert!(file.disposition.is_included(), "{path}");
        for (export, tree) in both_exports(&snap) {
            let staged = tree.path().join("config").join(&path[1..]);
            assert!(staged.exists(), "{export}: {path}");
        }
    }
}

/// What `/etc/crypto-policies/config` names when it is the policy file.
const CRYPTO_POLICY_PATH: &str = "/etc/crypto-policies/config";
/// A file outside `/etc` whose first line passes as a policy name.
const POLICY_LIKE_SECRET: &str = "ROOTONLYPOLICYLIKESECRET";

/// The crypto-policy check reads the policy file before any config is
/// collected, and its warning used to quote the first line. Through a
/// final link or a linked directory to key material or a file outside
/// `/etc`, it reads nothing: neither the key nor the outside secret is
/// in the snapshot JSON or any file of either export. A policy file with
/// an unexpected value is named with a fixed reason, never quoted.
#[test]
fn the_crypto_policy_check_never_reads_or_quotes_a_withheld_file() {
    let keys = || {
        sssd_host()
            .ghost("chrony", "cg", "/etc/chrony.keys")
            .file("/etc/chrony.keys", &format!("1 SHA1 {CHRONY_KEY}\n"))
            .file("/root/policy", &format!("{POLICY_LIKE_SECRET}\n"))
            .file("/root/cp/config", &format!("{POLICY_LIKE_SECRET}\n"))
    };
    let cases = [
        (
            "final link to key material",
            keys().link(CRYPTO_POLICY_PATH, "/etc/chrony.keys"),
        ),
        (
            "final link out of /etc",
            keys().link(CRYPTO_POLICY_PATH, "/root/policy"),
        ),
        (
            "linked directory out of /etc",
            keys().link("/etc/crypto-policies", "/root/cp"),
        ),
        (
            "linked directory to a link to key material",
            keys()
                .link("/etc/crypto-policies", "/etc/cp-live")
                .link("/etc/cp-live/config", "/etc/chrony.keys"),
        ),
    ];
    for (case, host) in cases {
        let snap = scan(host);
        for (where_, text) in everywhere(&snap) {
            for secret in [CHRONY_KEY, POLICY_LIKE_SECRET] {
                assert!(!text.contains(secret), "{case}: {where_}: {secret}");
            }
        }
        assert!(
            !snap
                .warnings
                .iter()
                .any(|w| w.message.contains("crypto policy")),
            "{case}: {:?}",
            snap.warnings
        );
    }

    const ODD_VALUE: &str = "DEFAULT; echo odd-policy-value";
    let snap = scan(sssd_host().file(CRYPTO_POLICY_PATH, &format!("{ODD_VALUE}\n")));
    let warning = snap
        .warnings
        .iter()
        .find(|w| w.message.starts_with(CRYPTO_POLICY_PATH))
        .unwrap_or_else(|| panic!("a warning names the policy file: {:?}", snap.warnings));
    assert!(
        !warning.message.contains("odd-policy-value"),
        "{}",
        warning.message
    );
}

/// An administrator's layout: `/etc/httpd/conf.d` links to
/// `../httpd-live`, which holds an unowned site configuration. httpd
/// reads `/etc/httpd/conf.d/*.conf`, and the directory link itself is
/// never carried, so the file is carried at that path, the one the
/// application reads, and under its own name as well: both exports stage
/// both.
#[test]
fn config_under_a_linked_directory_is_carried_at_the_path_the_application_reads() {
    const SITE: &str = "<VirtualHost *:443>\n    ServerName site.example.com\n</VirtualHost>\n";
    let snap = scan(
        sssd_host()
            .file(
                "/etc/httpd/conf/httpd.conf",
                "IncludeOptional conf.d/*.conf\n",
            )
            .file("/etc/httpd-live/site.conf", SITE)
            .link("/etc/httpd/conf.d", "../httpd-live"),
    );
    for path in ["/etc/httpd/conf.d/site.conf", "/etc/httpd-live/site.conf"] {
        let site = entry(&snap, path).unwrap_or_else(|| panic!("{path} is collected"));
        assert_eq!(site.content, SITE, "{path}");
        assert!(site.disposition.is_included(), "{path}");
        for (export, tree) in both_exports(&snap) {
            let staged = std::fs::read_to_string(tree.path().join("config").join(&path[1..]));
            assert_eq!(staged.ok().as_deref(), Some(SITE), "{export}: {path}");
        }
    }
}

/// A credential whose first line passes the policy-name check.
const POLICY_SHAPED_PASSWORD: &str = "ROOTONLYSECRET2026";

/// `/etc/crypto-policies/config` linked to an admin's credentials file
/// under `/etc`: both admission checks pass it as configuration, and its
/// first line, `PASSWORD:<secret>`, passes the policy-name check. The
/// non-default warning used to quote it, and redaction never scans
/// warnings. No warning quotes the policy file, so the secret is in no
/// part of the snapshot JSON or of either export.
#[test]
fn no_warning_quotes_the_crypto_policy_value() {
    let snap = scan(
        sssd_host()
            .file(
                "/etc/app/credentials",
                &format!("PASSWORD:{POLICY_SHAPED_PASSWORD}\n"),
            )
            .link(CRYPTO_POLICY_PATH, "/etc/app/credentials"),
    );
    for (where_, text) in everywhere(&snap) {
        assert!(!text.contains(POLICY_SHAPED_PASSWORD), "{where_}");
    }
    assert!(
        snap.warnings
            .iter()
            .any(|w| w.message.starts_with(CRYPTO_POLICY_PATH)),
        "the non-default policy is still named by path: {:?}",
        snap.warnings
    );

    let snap = scan(sssd_host().file(CRYPTO_POLICY_PATH, "FUTURE\n"));
    let warning = snap
        .warnings
        .iter()
        .find(|w| w.message.starts_with(CRYPTO_POLICY_PATH))
        .unwrap_or_else(|| panic!("a warning names the policy file: {:?}", snap.warnings));
    assert!(!warning.message.contains("FUTURE"), "{}", warning.message);
}

/// `authselect check` failing with a diagnostic that quotes what it read
/// from `authselect.conf`, as its unknown-profile error does. The scan
/// warning gives the exit code only, so the quoted text is in no part of
/// the snapshot JSON or of either export.
#[test]
fn no_warning_quotes_an_authselect_diagnostic() {
    let snap = scan(
        sssd_host()
            .ghost("authselect-libs", "g", "/etc/authselect/authselect.conf")
            .file(
                "/etc/authselect/authselect.conf",
                &measured("rhel-9.8", "authselect.conf"),
            )
            .command(
                "authselect check",
                1,
                "",
                &format!("[error] Unable to find profile [PASSWORD:{POLICY_SHAPED_PASSWORD}]\n"),
            ),
    );
    for (where_, text) in everywhere(&snap) {
        assert!(!text.contains(POLICY_SHAPED_PASSWORD), "{where_}");
    }
    assert!(
        snap.warnings
            .iter()
            .any(|w| w.message.contains("authselect check exited 1")),
        "{:?}",
        snap.warnings
    );
}

/// A rule kept only in `audit.rules`, where `rules.d` does not have it.
const ADMIN_ONLY_RULE: &str = "-w /etc/sudoers -p wa -k admin-only-rule\n";

/// The packaged RHEL 9 `auditd.service` edited in place to load
/// `audit.rules` directly, and `rpm -Va` failing, so nothing says the
/// unit changed. Failed verification is not "unmodified":
/// `audit.rules` is carried with a warning, and the rule only it holds
/// is in the snapshot and staged by both exports.
#[test]
fn audit_rules_are_carried_when_verification_failed() {
    let direct = audit_unit("rhel-9.8", "auditd.service")
        .replace(
            "ExecStartPost=-/sbin/augenrules --load",
            "#ExecStartPost=-/sbin/augenrules --load",
        )
        .replace(
            "#ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
            "ExecStartPost=-/sbin/auditctl -R /etc/audit/audit.rules",
        );
    let rules = format!("{AUDIT_RULES}{ADMIN_ONLY_RULE}");
    for (case, exit_code) in [("rpm -Va exited 2", 2), ("rpm -Va killed", -1)] {
        let snap = scan(
            audit_host_el9()
                .file(&format!("{UNIT_DIR}/auditd.service"), &direct)
                .file("/etc/audit/audit.rules", &rules)
                .command("rpm -Va", exit_code, "", ""),
        );
        let carried = entry(&snap, "/etc/audit/audit.rules")
            .unwrap_or_else(|| panic!("{case}: audit.rules is collected"));
        assert!(carried.content.contains(ADMIN_ONLY_RULE), "{case}");
        assert!(carried.disposition.is_included(), "{case}");
        assert!(
            snap.warnings.iter().any(|w| w
                .message
                .starts_with("/etc/audit/audit.rules is carried as it is")),
            "{case}: {:?}",
            snap.warnings
        );
        for (export, tree) in both_exports(&snap) {
            let staged = std::fs::read_to_string(tree.path().join("config/etc/audit/audit.rules"))
                .unwrap_or_default();
            assert!(staged.contains(ADMIN_ONLY_RULE), "{case} {export}");
        }
    }
}

/// Whether both exports stage `path` holding `content`.
fn assert_staged_everywhere(snap: &InspectionSnapshot, path: &str, content: &str) {
    let file = entry(snap, path).unwrap_or_else(|| panic!("{path} is collected"));
    assert_eq!(file.content, content, "{path}");
    assert!(file.disposition.is_included(), "{path}");
    for (export, tree) in both_exports(snap) {
        let staged = std::fs::read_to_string(tree.path().join("config").join(&path[1..]));
        assert_eq!(staged.ok().as_deref(), Some(content), "{export}: {path}");
    }
}

/// `/etc/httpd/live.d -> conf.d`, and httpd told to read `live.d`. The
/// physical files are already carried by another branch: a modified
/// `mod_ssl` file by `rpm -Va`, an admin's `%ghost` file by the ghost
/// branch. The directory link is never carried, so each is carried under
/// the alias as well, the path httpd reads, with the same content.
#[test]
fn an_alias_of_config_another_branch_carries_is_carried_too() {
    const SSL: &str = "Listen 8443 https\nSSLProtocol -all +TLSv1.3\n";
    const GHOST: &str = "Header always set X-Site live\n";
    let snap = scan(
        sssd_host()
            .rpm_owned("/etc/httpd/conf/httpd.conf", "httpd")
            .rpm_va("S.5....T.", "/etc/httpd/conf/httpd.conf")
            .file(
                "/etc/httpd/conf/httpd.conf",
                "IncludeOptional live.d/*.conf\n",
            )
            .rpm_owned("/etc/httpd/conf.d/ssl.conf", "mod_ssl")
            .rpm_va("S.5....T.", "/etc/httpd/conf.d/ssl.conf")
            .file("/etc/httpd/conf.d/ssl.conf", SSL)
            .ghost("httpd", "g", "/etc/httpd/conf.d/headers.conf")
            .file("/etc/httpd/conf.d/headers.conf", GHOST)
            .link("/etc/httpd/live.d", "conf.d"),
    );
    for (path, content) in [
        ("/etc/httpd/conf.d/ssl.conf", SSL),
        ("/etc/httpd/live.d/ssl.conf", SSL),
        ("/etc/httpd/conf.d/headers.conf", GHOST),
        ("/etc/httpd/live.d/headers.conf", GHOST),
    ] {
        assert_staged_everywhere(&snap, path, content);
    }
}

/// `/etc/httpd/conf.d -> .`: a link to the directory holding it, and
/// httpd reading `conf.d/site.conf`. The walk enters the link once, so
/// the site file is carried at the path httpd reads and under its own
/// name, and does not go through the same link again inside it.
#[test]
fn a_link_to_its_own_directory_is_walked_once() {
    const SITE: &str = "<VirtualHost *:80>\n    ServerName self.example.com\n</VirtualHost>\n";
    let snap = scan(
        sssd_host()
            .file(
                "/etc/httpd/conf/httpd.conf",
                "IncludeOptional conf.d/site.conf\n",
            )
            .file("/etc/httpd/site.conf", SITE)
            .link("/etc/httpd/conf.d", "."),
    );
    for path in ["/etc/httpd/site.conf", "/etc/httpd/conf.d/site.conf"] {
        assert_staged_everywhere(&snap, path, SITE);
    }
}

/// `/etc/app/live.conf -> active.conf.<suffix>`: an admin who activated
/// rpm's copy by linking the live name to it. The application reads the
/// live name, so it is carried with the copy's content; the copy's own
/// row stays out, as every rpm leftover does.
#[test]
fn a_live_link_to_an_rpm_backup_copy_is_carried() {
    const ACTIVE: &str = "listen = 0.0.0.0:8080\n";
    for suffix in ["rpmsave", "rpmnew", "rpmorig"] {
        let copy = format!("/etc/app/active.conf.{suffix}");
        let snap = scan(
            sssd_host()
                .file(&copy, ACTIVE)
                .link("/etc/app/live.conf", &format!("active.conf.{suffix}")),
        );
        assert_staged_everywhere(&snap, "/etc/app/live.conf", ACTIVE);
        assert!(entry(&snap, &copy).is_none(), "{copy} is not a config row");
        for (export, tree) in both_exports(&snap) {
            assert!(
                !tree.path().join("config").join(&copy[1..]).exists(),
                "{export}: {copy}"
            );
        }
    }
}
