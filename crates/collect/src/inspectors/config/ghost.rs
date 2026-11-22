//! The `%ghost` branch of the config inspector: owned files whose content
//! rpm never verifies.
//!
//! A package lists a `%ghost` file without shipping its content, so rpm
//! has nothing to compare it with: `rpm -V` checks a ghost file's mode,
//! owner and group but never its digest or size, and the unowned walk
//! skips it because a package owns it. Before this branch an admin's
//! edit to a ghost file under `/etc` reached no collector, and a ghost
//! file whose mode had also changed (`.M.......  g`, measured with
//! `chmod` on RHEL 9.8 and 10.2) was read by the `rpm -Va` branch as
//! metadata-only and dropped. Measured on
//! RHEL 9.8 and 10.2: `/etc/sssd/sssd.conf` (flags `cng`), authselect's
//! `/etc/authselect/authselect.conf` and `nsswitch.conf` (flag `g`), and
//! `/etc/default/grub` (`cng`). The authselect files carry no config flag,
//! so the branch takes every ghost file under `/etc`, not only `%config`
//! ones.
//!
//! The files come from one `rpm -qa` query that prints each packaged
//! file's flags ([`RPM_FILE_FLAGS_FORMAT`]); the branch in `mod.rs` then
//! applies the same skips as the unowned walk, plus the machine-state
//! members ([`is_machine_state_member`]) and [`GHOST_SKIP_EXACT`].

use super::walk::match_unowned_glob;

/// `rpm -qa` query format for every packaged file with its flags: the
/// owning package, rpm's `fflags` rendering of `FILEFLAGS`, then the path
/// shell-quoted, one record per line.
///
/// `%{=NAME}` repeats the scalar tag on each iteration of the file
/// arrays, which `[%{NAME} ...]` cannot (rpm refuses arrays of different
/// sizes in one iterator). A path can hold any byte but NUL, and rpm's
/// queryformat cannot print a NUL (`\0` comes out as a literal `0`,
/// measured on rpm 4.16.1.3 and 4.19.1.1), so no plain delimiter can end
/// a path. `:shescape` wraps the path in single quotes instead and writes
/// an embedded quote as `'\''`, leaving every other byte as it is, a
/// newline included (rpm `lib/formats.c`, `shescapeFormat`). The quoted
/// string ends at the first quote that does not start `'\''`, and the
/// record's line break follows it, so a newline or a record-like line
/// inside a path can neither end a record nor start one.
pub(crate) const RPM_FILE_FLAGS_FORMAT: &str =
    "[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES:shescape}\\n]";

/// The letter rpm's `fflags` formatter prints for `RPMFILE_GHOST`. The
/// whole alphabet is `dcsmnglra` (rpm `lib/formats.c`, `fflagsFormat`);
/// the parser accepts any lowercase letter, so a letter a later rpm adds
/// does not make every record unreadable.
const GHOST_FLAG: char = 'g';

/// The directory the config inspector covers.
const ETC_PREFIX: &str = "/etc/";

/// What `shescape` writes, after the quote that starts it, for one quote
/// inside the quoted string.
const SHESCAPED_QUOTE_TAIL: &str = "\\''";

/// A `%ghost` file under `/etc` and the package that lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GhostFile {
    pub path: String,
    pub package: String,
}

/// The ghost files under `/etc` in one run of the query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GhostQuery {
    /// Sorted by path. A path two packages list is kept once, under the
    /// first package in name order.
    pub files: Vec<GhostFile>,
    /// Ghost paths under `/etc` holding a control character (a newline,
    /// a carriage return, a tab), sorted. No export can stage such a
    /// path, so these are reported and never read.
    pub refused: Vec<String>,
}

/// Why the query output could not be read as records. A record that does
/// not parse leaves the rest of the output unaligned, so nothing after it
/// is trusted either.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("record {record}: {what}")]
pub(crate) struct GhostQueryError {
    /// 1-based.
    record: usize,
    what: &'static str,
}

/// Read the output of an `rpm -qa` run with [`RPM_FILE_FLAGS_FORMAT`]:
/// `<package> <flags> '<path>'` and a line break, record after record.
/// Every record must parse. The package field holds no line break or
/// quote, and the flags field only lowercase letters (none for a file
/// with no flag, which leaves two spaces).
pub(crate) fn parse_ghost_files(stdout: &str) -> Result<GhostQuery, GhostQueryError> {
    let mut query = GhostQuery::default();
    let mut rest = stdout;
    let mut record = 0;
    while !rest.is_empty() {
        record += 1;
        let fail = |what| GhostQueryError { record, what };
        let (package, after) = rest.split_once(' ').ok_or(fail("no package field"))?;
        if package.is_empty() || package.contains(['\n', '\'']) {
            return Err(fail("the package field is not a package name"));
        }
        let (flags, after) = after.split_once(' ').ok_or(fail("no flags field"))?;
        if !flags.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(fail("the flags field is not fflags letters"));
        }
        let (path, after) =
            take_shell_quoted(after).ok_or(fail("the path is not a shell-quoted string"))?;
        rest = after
            .strip_prefix('\n')
            .ok_or(fail("the record does not end after its path"))?;
        if !flags.contains(GHOST_FLAG) || !path.starts_with(ETC_PREFIX) {
            continue;
        }
        if path.chars().any(char::is_control) {
            query.refused.push(path);
            continue;
        }
        query.files.push(GhostFile {
            path,
            package: package.to_string(),
        });
    }
    query
        .files
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.package.cmp(&b.package)));
    query.files.dedup_by(|a, b| a.path == b.path);
    query.refused.sort();
    query.refused.dedup();
    Ok(query)
}

/// The string `shescape` quoted at the start of `text`, decoded, and what
/// follows its closing quote; `None` when `text` does not start with a
/// quote or the quote is never closed.
fn take_shell_quoted(text: &str) -> Option<(String, &str)> {
    let mut rest = text.strip_prefix('\'')?;
    let mut decoded = String::new();
    loop {
        let quote = rest.find('\'')?;
        decoded.push_str(&rest[..quote]);
        let after_quote = &rest[quote + 1..];
        match after_quote.strip_prefix(SHESCAPED_QUOTE_TAIL) {
            Some(after) => {
                decoded.push('\'');
                rest = after;
            }
            None => return Some((decoded, after_quote)),
        }
    }
}

/// Ghost files the branch leaves alone although no exclusion list names
/// them, each for its own reason:
///
/// - `/etc/locale.conf`, `/etc/vconsole.conf` and
///   `/etc/X11/xorg.conf.d/00-keyboard.conf` are systemd's locale and
///   keymap settings. Whether and how the image carries locale is an open
///   design question of its own, which copying these files would answer
///   by accident.
/// - `/etc/pki/tls/certs/localhost.crt` is the self-signed certificate
///   `sscg` generates beside `/etc/pki/tls/private/localhost.key`, a
///   machine-state member that is never carried; the certificate alone
///   would not match the key the image generates.
const GHOST_SKIP_EXACT: &[&str] = &[
    "/etc/locale.conf",
    "/etc/vconsole.conf",
    "/etc/X11/xorg.conf.d/00-keyboard.conf",
    "/etc/pki/tls/certs/localhost.crt",
];

/// Ghost subtrees generated from a file another section carries:
/// `update-crypto-policies` writes `state/` from
/// `/etc/crypto-policies/config`, which the crypto policy line renders.
const GHOST_SKIP_PREFIX: &[&str] = &["/etc/crypto-policies/state/"];

/// The files `authselect select` writes from the selected profile. Each
/// is a `%ghost` file of `authselect-libs`. The export carries the
/// selection itself
/// ([`AUTHSELECT_CONF_PATH`](inspectah_core::types::config::AUTHSELECT_CONF_PATH)) as an `authselect select`
/// line, which writes these again in the image, so copying them would
/// only give authselect files it then reports as changed. An edit made to
/// one outside authselect is reported by `authselect check` and becomes
/// an advisory (`authselect_edit_advisories` in `mod.rs`).
///
/// `user-nsswitch.conf` is not here: it is the admin's input to the
/// generated `nsswitch.conf`, and is collected like any config file.
const AUTHSELECT_GENERATED: &[&str] = &[
    "/etc/authselect/nsswitch.conf",
    "/etc/authselect/system-auth",
    "/etc/authselect/password-auth",
    "/etc/authselect/fingerprint-auth",
    "/etc/authselect/smartcard-auth",
    "/etc/authselect/postlogin",
    "/etc/authselect/dconf-db",
    "/etc/authselect/dconf-locks",
];

/// Whether the ghost branch leaves `path` alone on [`GHOST_SKIP_EXACT`],
/// [`GHOST_SKIP_PREFIX`] or [`AUTHSELECT_GENERATED`].
pub(crate) fn is_ghost_skip(path: &str) -> bool {
    GHOST_SKIP_EXACT.contains(&path)
        || GHOST_SKIP_PREFIX.iter().any(|p| path.starts_with(p))
        || AUTHSELECT_GENERATED.contains(&path)
}

// ---------------------------------------------------------------------------
// authselect check
// ---------------------------------------------------------------------------

/// What `authselect check` prints when the files it generated were
/// changed outside it: "Current configuration is not valid. It was
/// probably modified outside authselect."
const AUTHSELECT_NOT_VALID: &str = "not valid";

/// What `authselect check` found, from its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthselectCheck {
    /// The configuration is as authselect wrote it, or authselect could
    /// not say (not installed, no configuration).
    Clean,
    /// Changed outside authselect; the files it named, which may be none.
    Edited { paths: Vec<String> },
}

/// Read `authselect check`'s exit code and output. authselect names each
/// file it finds changed in brackets (`[/etc/authselect/system-auth] has
/// unexpected content!`), on stdout or stderr depending on the version,
/// so both are read and every bracketed absolute path is taken.
pub(crate) fn parse_authselect_check(
    exit_code: i32,
    stdout: &str,
    stderr: &str,
) -> AuthselectCheck {
    if exit_code == 0 {
        return AuthselectCheck::Clean;
    }
    let mut paths: Vec<String> = Vec::new();
    for text in [stdout, stderr] {
        let mut rest = text;
        while let Some(open) = rest.find("[/") {
            let after = &rest[open + 1..];
            let Some(close) = after.find(']') else {
                break;
            };
            let path = &after[..close];
            if !path.contains('\n') && !paths.iter().any(|p| p == path) {
                paths.push(path.to_string());
            }
            rest = &after[close + 1..];
        }
    }
    let said_not_valid =
        stdout.contains(AUTHSELECT_NOT_VALID) || stderr.contains(AUTHSELECT_NOT_VALID);
    if paths.is_empty() && !said_not_valid {
        AuthselectCheck::Clean
    } else {
        AuthselectCheck::Edited { paths }
    }
}

// ---------------------------------------------------------------------------
// Machine-state members (interim)
// ---------------------------------------------------------------------------
//
// The members of the machine-state class, as § 5.2 of
// `process-docs/specs/proposed/2026-11-21-machine-state-class.md` lists
// them: host identity, host credentials and generated files. The ghost
// branch collects none of them, so nothing host-identifying starts
// shipping through it before that class lands. When it does, the class's
// `classify_path` and `ConfigCollector::admit` replace this list: the
// branch hands every surviving candidate to `admit`, which records a
// member as a withheld or machine-state row instead of skipping it.
// Two members are defined by content as well as by path: a Kerberos
// keytab and a whole-file private key (§§ 4.5 and 5.3). A ghost file can
// sit at any path under `/etc`, so the branch checks its content too
// ([`is_credential_content`]) and drops such a file before its content
// is kept. The class's read gate will sniff before reading; this interim
// check reads the file, then discards it.

/// Exact member paths.
const MACHINE_STATE_EXACT: &[&str] = &[
    "/etc/machine-id",
    "/etc/insights-client/machine-id",
    "/etc/rhsm/facts/insights-client.facts",
    "/etc/krb5.keytab",
    "/etc/chef/client.pem",
    "/etc/iscsi/initiatorname.iscsi",
    "/etc/salt/minion_id",
    "/etc/ipa/default.conf",
    "/etc/hostname",
    "/etc/rhsm/syspurpose/syspurpose.json",
    "/etc/nvme/hostnqn",
    "/etc/nvme/hostid",
    "/etc/authselect/switchable-auth",
    "/etc/grub.d/00_tuned",
];

/// Member globs, matched as the unowned walk matches its globs.
const MACHINE_STATE_GLOBS: &[&str] = &[
    "/etc/insights-client/.*",
    "/etc/pki/consumer/*",
    "/etc/pki/entitlement/*",
    "/etc/lvm/devices/*",
    "/etc/krb5.keytab.*",
    "/etc/ssh/ssh_host_*",
    "/etc/cockpit/ws-certs.d/*.key",
];

/// Member subtrees, each ending on a path boundary.
const MACHINE_STATE_PREFIX: &[&str] = &[
    "/etc/pki/tls/private/",
    "/etc/pki/libvirt/private/",
    "/etc/puppetlabs/puppet/ssl/",
    "/etc/salt/pki/minion/",
];

/// File-name suffix of a Kerberos keytab anywhere under `/etc`.
const KEYTAB_SUFFIX: &str = ".keytab";

/// Whether `path` is a member of the machine-state class by its path.
pub(crate) fn is_machine_state_member(path: &str) -> bool {
    MACHINE_STATE_EXACT.contains(&path)
        || MACHINE_STATE_GLOBS
            .iter()
            .any(|pattern| match_unowned_glob(pattern, path))
        || MACHINE_STATE_PREFIX.iter().any(|p| path.starts_with(p))
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.ends_with(KEYTAB_SUFFIX))
}

/// The first two bytes of a Kerberos keytab: file format version 1 or 2
/// (§ 4.5 of the machine-state class spec).
const KEYTAB_HEADERS: [[u8; 2]; 2] = [[0x05, 0x01], [0x05, 0x02]];

/// What every private-key PEM label ends with: `PRIVATE KEY`, `ENCRYPTED
/// PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY`,
/// `OPENSSH PRIVATE KEY` (§ 5.3).
const PRIVATE_KEY_LABEL_SUFFIX: &str = "PRIVATE KEY";

/// PEM labels that may sit beside a private key in a key file (§ 5.3).
const COMPANION_LABELS: &[&str] = &[
    "CERTIFICATE",
    "TRUSTED CERTIFICATE",
    "X509 CRL",
    "EC PARAMETERS",
    "DH PARAMETERS",
    "DSA PARAMETERS",
];

/// Lines `openssl pkcs12` writes before each block (§ 5.3), matched at
/// the start of an unindented line.
const ENVELOPE_PREFIXES: &[&str] = &["Bag Attributes", "Key Attributes", "subject=", "issuer="];

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const PEM_DASHES: &str = "-----";

/// Whether a file's content makes it a machine-state credential whatever
/// its path: a keytab, or a file that is a private key and nothing else.
pub(crate) fn is_credential_content(content: &str) -> bool {
    KEYTAB_HEADERS
        .iter()
        .any(|header| content.as_bytes().starts_with(header))
        || is_whole_file_private_key(content)
}

/// § 5.3: every line outside a PEM block is blank, a `#` comment or
/// `openssl` envelope metadata, every block's label is a private-key
/// label or a companion, and at least one block is a private key. Any
/// other line (an application setting) makes the file configuration, and
/// a key embedded in it is left to redaction. A block that never ends
/// counts as a block to the end of the file, so a cut-off key is still a
/// key.
pub(crate) fn is_whole_file_private_key(content: &str) -> bool {
    let mut open_label: Option<&str> = None;
    let mut has_key = false;
    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(label) = open_label {
            if pem_label(line, PEM_END) == Some(label) {
                open_label = None;
            }
            continue;
        }
        if let Some(label) = pem_label(line, PEM_BEGIN) {
            if label.ends_with(PRIVATE_KEY_LABEL_SUFFIX) {
                has_key = true;
            } else if !COMPANION_LABELS.contains(&label) {
                return false;
            }
            open_label = Some(label);
            continue;
        }
        let envelope = line.trim().is_empty()
            || line.starts_with('#')
            || line.starts_with([' ', '\t'])
            || ENVELOPE_PREFIXES.iter().any(|p| line.starts_with(p));
        if !envelope {
            return false;
        }
    }
    has_key
}

/// The label of a `-----BEGIN <label>-----` or `-----END <label>-----`
/// line, by `marker`.
fn pem_label<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.trim()
        .strip_prefix(marker)?
        .strip_suffix(PEM_DASHES)
        .filter(|label| !label.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::config::AUTHSELECT_CONF_PATH;

    /// Records copied byte for byte from `rpm -q authselect-libs
    /// sssd-common grub2-tools systemd setup --queryformat` with
    /// [`RPM_FILE_FLAGS_FORMAT`] on RHEL 9.8 (rpm 4.16.1.3), 2026-11-21,
    /// a subset of whole records. An empty flags field leaves two spaces.
    const EL9_RECORDS: &str = "\
authselect-libs g '/etc/authselect/authselect.conf'
authselect-libs g '/etc/authselect/nsswitch.conf'
authselect-libs g '/etc/authselect/user-nsswitch.conf'
sssd-common  '/etc/sssd'
sssd-common cng '/etc/sssd/sssd.conf'
sssd-common  '/usr/sbin/sssd'
grub2-tools cng '/etc/default/grub'
systemd cng '/etc/machine-id'
setup cn '/etc/hosts'
";

    /// The same query on RHEL 10.2 (rpm 4.19.1.1), 2026-11-21. authselect
    /// 1.5 lists `/etc/nsswitch.conf` and the `/etc/pam.d/` links as
    /// ghosts, and no longer `user-nsswitch.conf`.
    const EL10_RECORDS: &str = "\
authselect-libs g '/etc/authselect/authselect.conf'
authselect-libs g '/etc/authselect/nsswitch.conf'
authselect-libs g '/etc/nsswitch.conf'
authselect-libs g '/etc/pam.d/system-auth'
sssd-common  '/etc/sssd'
sssd-common cng '/etc/sssd/sssd.conf'
sssd-common  '/usr/sbin/sssd'
grub2-tools cng '/etc/default/grub'
systemd cng '/etc/machine-id'
setup cn '/etc/hosts'
";

    fn paths(query: &GhostQuery) -> Vec<(&str, &str)> {
        query
            .files
            .iter()
            .map(|g| (g.path.as_str(), g.package.as_str()))
            .collect()
    }

    #[test]
    fn parses_the_measured_records_of_both_releases() {
        let el9 = parse_ghost_files(EL9_RECORDS).expect("EL9 records parse");
        assert_eq!(
            paths(&el9),
            vec![
                ("/etc/authselect/authselect.conf", "authselect-libs"),
                ("/etc/authselect/nsswitch.conf", "authselect-libs"),
                ("/etc/authselect/user-nsswitch.conf", "authselect-libs"),
                ("/etc/default/grub", "grub2-tools"),
                ("/etc/machine-id", "systemd"),
                ("/etc/sssd/sssd.conf", "sssd-common"),
            ],
            "ghost /etc paths only: not a directory without g, a /usr path or a cn file"
        );
        assert!(el9.refused.is_empty());

        let el10 = parse_ghost_files(EL10_RECORDS).expect("EL10 records parse");
        assert_eq!(
            paths(&el10),
            vec![
                ("/etc/authselect/authselect.conf", "authselect-libs"),
                ("/etc/authselect/nsswitch.conf", "authselect-libs"),
                ("/etc/default/grub", "grub2-tools"),
                ("/etc/machine-id", "systemd"),
                ("/etc/nsswitch.conf", "authselect-libs"),
                ("/etc/pam.d/system-auth", "authselect-libs"),
                ("/etc/sssd/sssd.conf", "sssd-common"),
            ]
        );
    }

    /// Paths with quotes, spaces and line breaks, as `shescape` writes
    /// them, decode whole. A newline inside the quotes, a carriage return,
    /// and a continuation spelled like a record are all part of the path,
    /// so `/etc/app` is read only from its own record. A decoded path
    /// holding a control character is refused, not read.
    #[test]
    fn shell_quoted_paths_decode_whole() {
        let out = concat!(
            "myapp g '/etc/my app/state file'\n",
            r"myapp g '/etc/it'\''s/a '\'''\''quoted'\'' name'",
            "\n",
            "myapp g '/etc/app\nsettings.conf'\n",
            "myapp g '/etc/app\npkg g /etc/other'\n",
            "myapp g '/etc/crlf.conf\r'\n",
            "myapp g '/etc/tab\there'\n",
            "myapp g '/etc/app'\n",
        );
        let query = parse_ghost_files(out).expect("every record parses");
        assert_eq!(
            paths(&query),
            vec![
                ("/etc/app", "myapp"),
                ("/etc/it's/a ''quoted' name", "myapp"),
                ("/etc/my app/state file", "myapp"),
            ]
        );
        assert_eq!(
            query.refused,
            vec![
                "/etc/app\npkg g /etc/other".to_string(),
                "/etc/app\nsettings.conf".to_string(),
                "/etc/crlf.conf\r".to_string(),
                "/etc/tab\there".to_string(),
            ]
        );
    }

    #[test]
    fn a_path_two_packages_list_is_kept_once() {
        let got = parse_ghost_files("b g '/etc/x'\na g '/etc/x'\n").unwrap();
        assert_eq!(
            got.files,
            vec![GhostFile {
                path: "/etc/x".into(),
                package: "a".into()
            }]
        );
    }

    /// Output that is not the format fails as a whole, naming the record,
    /// rather than being split into guesses.
    #[test]
    fn malformed_output_is_refused() {
        for (out, record) in [
            ("a g /etc/unquoted\n", 1),
            ("a g '/etc/x'\nb cXq '/etc/y'\n", 2),
            ("a g '/etc/never closed\n", 1),
            ("a g '/etc/x' trailing\n", 1),
            ("a g '/etc/x'", 1),
            ("a g '/etc/x'\nlonely\n", 2),
        ] {
            let err = parse_ghost_files(out).expect_err(out);
            assert_eq!(err.record, record, "{out:?}: {err}");
        }
        assert_eq!(parse_ghost_files(""), Ok(GhostQuery::default()));
    }

    /// § 5.3 by example. A key file is a key whatever its label or
    /// companions; a key inside a configuration file is configuration.
    #[test]
    fn whole_file_private_keys() {
        let encrypted = "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIFHDBOBgkqhkiG9w0BBQ0w\n-----END ENCRYPTED PRIVATE KEY-----\n";
        let pkcs12 = "Bag Attributes\n    localKeyID: 01 02\n    friendlyName: server\nsubject=CN = host\nissuer=CN = ca\n-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\nKey Attributes: <No Attributes>\n-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----\n";
        let ec = "# generated\n\n-----BEGIN EC PARAMETERS-----\nBggq\n-----END EC PARAMETERS-----\n-----BEGIN EC PRIVATE KEY-----\nMHcC\n-----END EC PRIVATE KEY-----\n";
        let crlf = "-----BEGIN RSA PRIVATE KEY-----\r\nMIIE\r\n-----END RSA PRIVATE KEY-----\r\n";
        let cut_off = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl\n";
        for content in [encrypted, pkcs12, ec, crlf, cut_off] {
            assert!(is_whole_file_private_key(content), "{content:?}");
            assert!(is_credential_content(content), "{content:?}");
        }

        let embedded = "listen = 443\nkey = <<EOF\n-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----\n";
        let cert_only = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        let other_label = "-----BEGIN PGP PUBLIC KEY BLOCK-----\nmQIN\n-----END PGP PUBLIC KEY BLOCK-----\n-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----\n";
        for content in [embedded, cert_only, other_label, "", "[sssd]\n"] {
            assert!(!is_whole_file_private_key(content), "{content:?}");
        }
    }

    #[test]
    fn keytab_headers() {
        assert!(is_credential_content("\u{5}\u{2}\0\0\0\u{3c}"));
        assert!(is_credential_content("\u{5}\u{1}"));
        assert!(!is_credential_content("\u{5}\u{3}"));
        assert!(!is_credential_content("0502"));
    }

    #[test]
    fn machine_state_members_by_path() {
        for (path, member) in [
            ("/etc/machine-id", true),
            ("/etc/hostname", true),
            ("/etc/krb5.keytab", true),
            ("/etc/httpd/conf/http.keytab", true),
            ("/etc/ssh/ssh_host_ed25519_key", true),
            ("/etc/pki/tls/private/localhost.key", true),
            ("/etc/cockpit/ws-certs.d/0-self-signed.key", true),
            ("/etc/iscsi/initiatorname.iscsi", true),
            ("/etc/authselect/switchable-auth", true),
            ("/etc/sssd/sssd.conf", false),
            ("/etc/default/grub", false),
            ("/etc/authselect/authselect.conf", false),
            ("/etc/cockpit/ws-certs.d/0-self-signed.cert", false),
        ] {
            assert_eq!(is_machine_state_member(path), member, "{path}");
        }
    }

    #[test]
    fn authselect_check_output() {
        assert_eq!(
            parse_authselect_check(0, "Current configuration is valid.\n", ""),
            AuthselectCheck::Clean
        );
        assert_eq!(
            parse_authselect_check(127, "", "authselect: command not found\n"),
            AuthselectCheck::Clean,
            "no authselect is nothing to report"
        );
        assert_eq!(
            parse_authselect_check(
                1,
                "",
                "[error] [/etc/authselect/system-auth] has unexpected content!\n\
                 [error] [/etc/nsswitch.conf] has unexpected content!\n\
                 [error] [/etc/authselect/system-auth] has unexpected content!\n\
                 Current configuration is not valid. It was probably modified outside authselect.\n",
            ),
            AuthselectCheck::Edited {
                paths: vec![
                    "/etc/authselect/system-auth".into(),
                    "/etc/nsswitch.conf".into()
                ]
            }
        );
        assert_eq!(
            parse_authselect_check(
                1,
                "Current configuration is not valid. It was probably modified outside authselect.\n",
                ""
            ),
            AuthselectCheck::Edited { paths: vec![] }
        );
    }

    #[test]
    fn ghost_skips() {
        assert!(is_ghost_skip("/etc/authselect/nsswitch.conf"));
        assert!(!is_ghost_skip(AUTHSELECT_CONF_PATH));
        assert!(!is_ghost_skip("/etc/authselect/user-nsswitch.conf"));
        assert!(is_ghost_skip("/etc/locale.conf"));
        assert!(is_ghost_skip("/etc/crypto-policies/state/current"));
        assert!(!is_ghost_skip("/etc/crypto-policies/config"));
        assert!(!is_ghost_skip("/etc/sssd/sssd.conf"));
    }
}
