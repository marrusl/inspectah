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
//! file's flags ([`RPM_FILE_FLAGS_FORMAT`]). [`classify_ghost`] then
//! decides each path: the branch carries general configuration no other
//! section owns, and leaves out a file another section records, key
//! material, machine-state members and files the host generates.

use super::is_selinux_fcontext_store_file;
use super::walk::{is_dev_artifact, is_excluded_unowned_path, match_unowned_glob};
use crate::inspectors::rpm::repos;
use crate::inspectors::{kernelboot, selinux, storage, users};
use inspectah_core::types::completeness::InspectorId;
use std::collections::BTreeSet;

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

/// Where packages install unit files, with its trailing slash.
const PACKAGED_UNIT_PREFIX: &str = "/usr/lib/systemd/system/";

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
    /// Every path under `/usr/lib/systemd/system/` some package lists,
    /// whatever its flags: what tells a packaged unit file from one an
    /// administrator put there ([`super::audit_rules::source`]).
    pub packaged_units: BTreeSet<String>,
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
        if path.starts_with(PACKAGED_UNIT_PREFIX) {
            query.packaged_units.insert(path);
            continue;
        }
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

// ---------------------------------------------------------------------------
// What the branch does with each ghost path
// ---------------------------------------------------------------------------

/// What the ghost branch does with a `%ghost` path under `/etc`, decided
/// by the path alone. The branch carries general configuration that no
/// other section owns, and nothing else. The full classification of
/// every ghost file on RHEL 9.8 and 10.2 is in
/// `process-docs/skills/ghost-config-files.md`, and
/// `measured_ghost_files_land_in_known_classes` pins it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GhostClass {
    /// General configuration: collected with its content, unless the
    /// content turns out to be a credential ([`is_credential_content`]).
    Config,
    /// Another section records the file ([`OWNED_ELSEWHERE`]). The config
    /// walk never copies it, so it cannot land in the image beside, or
    /// instead of, that section's own decision.
    OwnedBy(InspectorId),
    /// Key material by path ([`GHOST_CREDENTIAL_EXACT`]): never opened,
    /// withheld by name with a scan warning.
    Credential,
    /// A member of the machine-state class by path
    /// ([`is_machine_state_member`]): never collected.
    MachineState,
    /// Not configuration: written on the host by a tool or at run time
    /// from other input that the image writes again, or excluded by the
    /// unowned walk's lists.
    Generated,
    /// `/etc/audit/audit.rules`: generated when the host runs `augenrules`,
    /// the administrator's input when it loads the file directly. How the
    /// host loads its rules decides ([`super::audit_rules::source`]).
    AuditRules,
    /// An open item decides how the image carries it ([`GHOST_DEFERRED`]).
    Deferred,
}

/// The files under `/etc` other sections claim, each with the claiming
/// inspector. Every entry is that inspector's own constant, the one it
/// reads the file by, so the claim and the read cannot drift apart. The
/// one exception is storage's `/etc/crypttab`, which storage claims but
/// does not parse yet.
const OWNED_ELSEWHERE: &[(&[&str], InspectorId)] = &[
    (storage::OWNED_ETC_FILES, InspectorId::Storage),
    (users::OWNED_ETC_FILES, InspectorId::UsersGroups),
    (&[selinux::SELINUX_CONFIG_PATH], InspectorId::Selinux),
    (&[kernelboot::LOCALE_CONF_PATH], InspectorId::KernelBoot),
];

/// Files another section claims by a rule rather than by name, each with
/// the claiming inspector. Each rule is the predicate that inspector reads
/// by, so the claim covers exactly what it reads: a file the rule leaves
/// out stays general configuration.
const OWNED_BY_RULE_ELSEWHERE: &[(PathClaim, InspectorId)] =
    &[(repos::is_repo_file_path, InspectorId::Rpm)];

/// Whether a section claims a path, by the rule it reads files by.
type PathClaim = fn(&str) -> bool;

/// Ghost files whose content is a shared secret or a private key, at
/// paths no machine-state rule names. Each is withheld by name and never
/// opened, so the content sniffs cannot be the only guard.
///
/// - `/etc/chrony.keys`: chrony's NTP authentication keys, one `<id>
///   <hash> HEX:<key>` line each. A `%ghost` of `chrony` on RHEL 10.2
///   (`cg`); a plain `%config` file on RHEL 9.8.
/// - `/etc/rndc.key` and `/etc/rndc.conf`: bind's rndc HMAC secret,
///   written by `rndc-confgen`, which puts the key inline in `rndc.conf`
///   when run without `-a` (`cng` ghosts of `bind` on both releases).
/// - `/etc/unbound/unbound_control.key` and `unbound_server.key`: the
///   private keys `unbound-control-setup` writes (`g` ghosts of `unbound`).
const GHOST_CREDENTIAL_EXACT: &[&str] = &[
    "/etc/chrony.keys",
    "/etc/rndc.key",
    "/etc/rndc.conf",
    "/etc/unbound/unbound_control.key",
    "/etc/unbound/unbound_server.key",
];

/// Ghost files a tool writes on the host, each from input the image has
/// or writes again, so the copy would be stale or wrong in the image:
///
/// - `/etc/pki/tls/certs/localhost.crt`: the self-signed certificate
///   `sscg` generates beside `/etc/pki/tls/private/localhost.key`, a
///   machine-state member that is never carried; the certificate alone
///   would not match the key the image generates. The `unbound_*.pem`
///   certificates are the same case for the unbound keys.
/// - `/etc/avahi/etc/localtime`: `avahi`'s `%post` copies
///   `/etc/localtime` into the daemon's chroot.
/// - `/etc/modprobe.d/kvm.rt.tuned.conf`: written by tuned's
///   `realtime-virtual-host` profile functions; the tuned profile is what
///   the image carries.
/// - `/etc/lvm/cache/.cache`: LVM's device cache.
/// - `/etc/cni/net.d/87-podman-bridge.conflist`: the default network
///   podman's CNI backend wrote on first use.
const GHOST_GENERATED_EXACT: &[&str] = &[
    "/etc/pki/tls/certs/localhost.crt",
    "/etc/unbound/unbound_control.pem",
    "/etc/unbound/unbound_server.pem",
    "/etc/avahi/etc/localtime",
    "/etc/modprobe.d/kvm.rt.tuned.conf",
    "/etc/lvm/cache/.cache",
    "/etc/cni/net.d/87-podman-bridge.conflist",
];

/// Ghost subtrees written on the host: `update-crypto-policies` writes
/// `state/` from `/etc/crypto-policies/config`, which the crypto policy
/// line renders; cups-filters rewrites `hashes.d/hashes.new` while the
/// host runs (an empty file dated after the install on both test hosts).
const GHOST_GENERATED_PREFIX: &[&str] = &["/etc/crypto-policies/state/", "/etc/foomatic/hashes.d/"];

/// systemd's keymap settings, written by `localectl`. Whether and how the
/// image carries locale and keymap is an open item of its own, which
/// copying these files would answer by accident. `/etc/locale.conf` is
/// the kernel and boot section's ([`OWNED_ELSEWHERE`]).
const GHOST_DEFERRED: &[&str] = &[
    "/etc/vconsole.conf",
    "/etc/X11/xorg.conf.d/00-keyboard.conf",
];

/// Classify a ghost path under `/etc`. Checked in order, so a path two
/// rules name takes the first: a credential is never treated as anything
/// else.
pub(crate) fn classify_ghost(path: &str) -> GhostClass {
    if GHOST_CREDENTIAL_EXACT.contains(&path) {
        return GhostClass::Credential;
    }
    if is_machine_state_member(path) {
        return GhostClass::MachineState;
    }
    if let Some(owner) = owner_elsewhere(path) {
        return GhostClass::OwnedBy(owner);
    }
    if GHOST_GENERATED_EXACT.contains(&path)
        || GHOST_GENERATED_PREFIX.iter().any(|p| path.starts_with(p))
        || AUTHSELECT_GENERATED
            .iter()
            .any(|(generated, _)| *generated == path)
        || is_excluded_unowned_path(path)
        || is_dev_artifact(path)
        || is_selinux_fcontext_store_file(path)
    {
        return GhostClass::Generated;
    }
    if GHOST_DEFERRED.contains(&path) {
        return GhostClass::Deferred;
    }
    if path == super::audit_rules::AUDIT_RULES_PATH {
        return GhostClass::AuditRules;
    }
    GhostClass::Config
}

/// The inspector that claims `path`, if one does.
fn owner_elsewhere(path: &str) -> Option<InspectorId> {
    let by_file = OWNED_ELSEWHERE
        .iter()
        .find(|(paths, _)| paths.contains(&path))
        .map(|(_, owner)| *owner);
    by_file.or_else(|| {
        OWNED_BY_RULE_ELSEWHERE
            .iter()
            .find(|(claims, _)| claims(path))
            .map(|(_, owner)| *owner)
    })
}

/// The files `authselect select` writes from the selected profile, each
/// with the `authselect test` option that prints what it would write
/// there. Each is a `%ghost` file of `authselect-libs`. The export
/// carries the selection itself
/// ([`AUTHSELECT_CONF_PATH`](inspectah_core::types::config::AUTHSELECT_CONF_PATH))
/// as an `authselect select` line, which writes these again in the
/// image, so copying them would only give authselect files it then
/// reports as changed. An edit made to one outside authselect becomes an
/// advisory (`authselect_edit_advisories` in `mod.rs`).
///
/// `user-nsswitch.conf` is not here: it is the admin's input to the
/// generated `nsswitch.conf`, and is collected like any config file.
pub(crate) const AUTHSELECT_GENERATED: &[(&str, &str)] = &[
    ("/etc/authselect/nsswitch.conf", "--nsswitch"),
    ("/etc/authselect/system-auth", "--system-auth"),
    ("/etc/authselect/password-auth", "--password-auth"),
    ("/etc/authselect/fingerprint-auth", "--fingerprint-auth"),
    ("/etc/authselect/smartcard-auth", "--smartcard-auth"),
    ("/etc/authselect/postlogin", "--postlogin"),
    ("/etc/authselect/dconf-db", "--dconf-db"),
    ("/etc/authselect/dconf-locks", "--dconf-lock"),
];

// ---------------------------------------------------------------------------
// authselect check and authselect test
// ---------------------------------------------------------------------------
//
// `authselect check` does not find every hand edit. Measured on
// rhel-bootc 9.8 (authselect 1.2.6) and 10.2 (1.5.2), 2026-11-21, after
// `authselect select sssd with-faillock with-mkhomedir --force` and a
// line appended to `/etc/authselect/system-auth` with its mode and owner
// kept: 1.2.6 exits 3 and names the file; 1.5.2 exits 0 and prints
// `Current configuration is valid.`, since it checks the generated
// files' metadata and links but not their content. Both name a changed
// mode and a replaced link. So the content is compared as well, against
// what `authselect test` prints for the selected profile.

/// What `authselect check` prints when the files it generated were
/// changed outside it: "Current configuration is not valid. It was
/// probably modified outside authselect."
const AUTHSELECT_NOT_VALID: &str = "not valid";

/// What `authselect check` found, from its output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthselectCheck {
    /// authselect reports the configuration as it wrote it. On 1.5 that
    /// covers metadata and links only, never content.
    Valid,
    /// Changed outside authselect; the files it named, which may be none.
    Edited { paths: Vec<String> },
    /// The check did not give either answer: authselect is missing, or it
    /// failed. Nothing is known about the files.
    Failed { exit_code: i32, message: String },
}

/// Read `authselect check`'s exit code and output. authselect names each
/// file it finds changed in brackets (`[error] [/etc/authselect/system-auth]
/// has unexpected content!`, on stderr in 1.2.6 and 1.5.2), so both
/// streams are read and every bracketed absolute path is taken. A
/// non-zero exit that names no file and does not say "not valid" is a
/// failure, not an answer.
pub(crate) fn parse_authselect_check(
    exit_code: i32,
    stdout: &str,
    stderr: &str,
) -> AuthselectCheck {
    if exit_code == 0 {
        return AuthselectCheck::Valid;
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
        let message = [stderr, stdout]
            .iter()
            .flat_map(|text| text.lines())
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no output")
            .to_string();
        AuthselectCheck::Failed { exit_code, message }
    } else {
        AuthselectCheck::Edited { paths }
    }
}

/// The first line of every file authselect generates, in 1.2.6 (`# Generated
/// by authselect on <date>`) and 1.5.2 (`# Generated by authselect`). The
/// header runs to the first blank line.
const GENERATED_HEADER: &str = "# Generated by authselect";

/// What `authselect test` prints before a file's content: `File <path>:`,
/// where the path is where the file is linked from (`/etc/pam.d/...`).
const TEST_FILE_PREFIX: &str = "File ";
const TEST_FILE_SUFFIX: &str = ":";

/// The content of a file authselect generated, without its header and
/// trailing line breaks: what `authselect test` prints for it.
pub(crate) fn generated_body(content: &str) -> &str {
    let body = if content.starts_with(GENERATED_HEADER) {
        content.split_once("\n\n").map_or("", |(_, body)| body)
    } else {
        content
    };
    body.trim_end_matches('\n')
}

/// The file content `authselect test --<file>` printed, without its
/// `File <path>:` line and trailing line breaks; `None` when the output
/// does not start with that line.
pub(crate) fn test_body(stdout: &str) -> Option<&str> {
    let (first, body) = stdout.split_once('\n')?;
    (first.starts_with(TEST_FILE_PREFIX) && first.ends_with(TEST_FILE_SUFFIX))
        .then(|| body.trim_end_matches('\n'))
}

/// The profile and features `authselect.conf` records, as arguments for
/// `authselect test`: the profile on the first line that is not blank or
/// a `#` comment, a feature on each line after it. `None` when the file
/// names no profile or a word could be read as an option or is not one
/// word, so host text never reaches authselect as anything but a
/// profile or feature name.
pub(crate) fn selection_args(authselect_conf: &str) -> Option<Vec<&str>> {
    let words: Vec<&str> = authselect_conf
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let plain = |word: &&str| !word.starts_with('-') && !word.contains(char::is_whitespace);
    (!words.is_empty() && words.iter().all(plain)).then_some(words)
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

    /// Bytes measured in rhel-bootc 9.8 (authselect 1.2.6) and 10.2
    /// (1.5.2) on 2026-11-21, after `authselect select sssd with-faillock
    /// with-mkhomedir --force`: the generated files, `authselect test`
    /// output, and `authselect check` after edits.
    mod measured {
        macro_rules! fixture {
            ($release:literal, $name:literal) => {
                include_str!(concat!(
                    "../../../../../testdata/fixtures/config/authselect/",
                    $release,
                    "/",
                    $name
                ))
            };
        }
        pub(super) struct Release {
            pub conf: &'static str,
            pub system_auth: &'static str,
            pub system_auth_edited: &'static str,
            pub nsswitch: &'static str,
            pub test_system_auth: &'static str,
            pub test_nsswitch: &'static str,
            /// Exit code, stdout and stderr after a line was appended to
            /// `system-auth`, mode and owner kept.
            pub check_content_edit: (i32, &'static str, &'static str),
            /// The same after that, a `passwd:` edit through
            /// `/etc/nsswitch.conf` (`sed -i`, which replaces the link), a
            /// `chmod 0600` of `password-auth`, and a regular file copied
            /// over the `/etc/pam.d/smartcard-auth` link.
            pub check_mode_and_links: (i32, &'static str, &'static str),
        }
        macro_rules! release {
            ($release:literal, $content_exit:literal) => {
                Release {
                    conf: fixture!($release, "authselect.conf"),
                    system_auth: fixture!($release, "system-auth"),
                    system_auth_edited: fixture!($release, "system-auth.edited"),
                    nsswitch: fixture!($release, "nsswitch.conf"),
                    test_system_auth: fixture!($release, "test-system-auth.stdout"),
                    test_nsswitch: fixture!($release, "test-nsswitch.stdout"),
                    check_content_edit: (
                        $content_exit,
                        fixture!($release, "check-content-edit.stdout"),
                        fixture!($release, "check-content-edit.stderr"),
                    ),
                    check_mode_and_links: (
                        3,
                        fixture!($release, "check-mode-and-links.stdout"),
                        fixture!($release, "check-mode-and-links.stderr"),
                    ),
                }
            };
        }
        pub(super) const EL9: Release = release!("rhel-9.8", 3);
        pub(super) const EL10: Release = release!("rhel-10.2", 0);
    }

    /// 1.2.6 names a content edit; 1.5.2 calls the same edit valid. Both
    /// name a changed mode and replaced links.
    #[test]
    fn authselect_check_on_both_releases() {
        let (code, out, err) = measured::EL9.check_content_edit;
        assert_eq!(
            parse_authselect_check(code, out, err),
            AuthselectCheck::Edited {
                paths: vec!["/etc/authselect/system-auth".into()]
            }
        );
        let (code, out, err) = measured::EL10.check_content_edit;
        assert_eq!(
            parse_authselect_check(code, out, err),
            AuthselectCheck::Valid,
            "1.5.2 does not compare content"
        );

        let (code, out, err) = measured::EL9.check_mode_and_links;
        assert_eq!(
            parse_authselect_check(code, out, err),
            AuthselectCheck::Edited {
                paths: vec![
                    "/etc/authselect/system-auth".into(),
                    "/etc/authselect/password-auth".into(),
                    "/etc/pam.d/smartcard-auth".into(),
                    "/etc/nsswitch.conf".into(),
                ]
            }
        );
        let (code, out, err) = measured::EL10.check_mode_and_links;
        assert_eq!(
            parse_authselect_check(code, out, err),
            AuthselectCheck::Edited {
                paths: vec![
                    "/etc/authselect/password-auth".into(),
                    "/etc/pam.d/smartcard-auth".into(),
                    "/etc/nsswitch.conf".into(),
                ]
            }
        );
    }

    /// A check that does not answer is a failure, never a clean result.
    #[test]
    fn authselect_check_failures_are_not_valid_results() {
        assert_eq!(
            parse_authselect_check(127, "", "command not found: authselect check"),
            AuthselectCheck::Failed {
                exit_code: 127,
                message: "command not found: authselect check".into()
            }
        );
        assert_eq!(
            parse_authselect_check(2, "System was not configured with authselect.\n", ""),
            AuthselectCheck::Failed {
                exit_code: 2,
                message: "System was not configured with authselect.".into()
            }
        );
        assert_eq!(
            parse_authselect_check(
                3,
                "Current configuration is not valid. It was probably modified outside authselect.\n",
                ""
            ),
            AuthselectCheck::Edited { paths: vec![] }
        );
    }

    /// What `authselect test` prints equals the generated file once the
    /// header and trailing line breaks are set aside, on both releases;
    /// the edited file does not.
    #[test]
    fn generated_files_match_authselect_test_output() {
        for (name, release) in [("EL9", measured::EL9), ("EL10", measured::EL10)] {
            assert_eq!(
                test_body(release.test_system_auth),
                Some(generated_body(release.system_auth)),
                "{name} system-auth"
            );
            assert_eq!(
                test_body(release.test_nsswitch),
                Some(generated_body(release.nsswitch)),
                "{name} nsswitch.conf"
            );
            assert_ne!(
                test_body(release.test_system_auth),
                Some(generated_body(release.system_auth_edited)),
                "{name} edited system-auth"
            );
            assert_eq!(
                selection_args(release.conf),
                Some(vec!["sssd", "with-faillock", "with-mkhomedir"]),
                "{name}"
            );
        }
        assert_eq!(test_body("auth required pam_env.so\n"), None);
    }

    #[test]
    fn selection_args_refuse_option_like_words() {
        assert_eq!(selection_args("local\n\n"), Some(vec!["local"]));
        assert_eq!(
            selection_args("# c\ncustom/corp\nwith-sudo\n"),
            Some(vec!["custom/corp", "with-sudo"])
        );
        for conf in [
            "",
            "# only\n",
            "sssd\n--all\n",
            "-sssd\n",
            "sssd\nwith faillock\n",
        ] {
            assert_eq!(selection_args(conf), None, "{conf:?}");
        }
    }

    #[test]
    fn classify_ghost_by_rule() {
        use GhostClass::*;
        for (path, class) in [
            ("/etc/sssd/sssd.conf", Config),
            (AUTHSELECT_CONF_PATH, Config),
            ("/etc/authselect/user-nsswitch.conf", Config),
            ("/etc/crypto-policies/config", Config),
            ("/etc/systemd/journald.conf", Config),
            ("/etc/fstab", OwnedBy(InspectorId::Storage)),
            ("/etc/crypttab", OwnedBy(InspectorId::Storage)),
            ("/etc/shadow", OwnedBy(InspectorId::UsersGroups)),
            ("/etc/selinux/config", OwnedBy(InspectorId::Selinux)),
            ("/etc/locale.conf", OwnedBy(InspectorId::KernelBoot)),
            ("/etc/yum.repos.d/redhat.repo", OwnedBy(InspectorId::Rpm)),
            ("/etc/yum.repos.d/sub/x.repo", Config),
            ("/etc/yum.repos.d/redhat.repo.bak", Config),
            ("/etc/yum.repos.d/repos.conf", Config),
            ("/etc/yum.repos.d.bak", Config),
            ("/etc/yum.repos.d.bak/x.repo", Config),
            ("/etc/chrony.keys", Credential),
            ("/etc/rndc.key", Credential),
            ("/etc/machine-id", MachineState),
            ("/etc/krb5.keytab", MachineState),
            ("/etc/authselect/nsswitch.conf", Generated),
            ("/etc/crypto-policies/state/current", Generated),
            ("/etc/crypto-policies/back-ends/nss.config", Generated),
            ("/etc/audit/audit.rules", AuditRules),
            ("/etc/audit/rules.d/audit.rules", Config),
            ("/etc/vconsole.conf", Deferred),
        ] {
            assert_eq!(classify_ghost(path), class, "{path}");
        }
    }

    /// The expected class of every ghost file in the measured lists. An
    /// entry ending in `/` covers every path under it.
    const MEASURED_CLASSES: &[(&str, GhostClass)] = {
        use GhostClass::*;
        &[
            // Carried as configuration.
            ("/etc/audit/rules.d/audit.rules", Config),
            ("/etc/authselect/authselect.conf", Config),
            ("/etc/authselect/user-nsswitch.conf", Config),
            ("/etc/cockpit/disallowed-users", Config),
            ("/etc/containers/containers.conf", Config),
            ("/etc/containers/storage.conf", Config),
            ("/etc/crypto-policies/config", Config),
            ("/etc/default/grub", Config),
            ("/etc/gai.conf", Config),
            ("/etc/gnupg/gpgconf.conf", Config),
            ("/etc/NetworkManager/VPN", Config),
            ("/etc/npmignore", Config),
            ("/etc/sssd/sssd.conf", Config),
            ("/etc/systemd/coredump.conf", Config),
            ("/etc/systemd/journald.conf", Config),
            ("/etc/systemd/logind.conf", Config),
            ("/etc/systemd/pstore.conf", Config),
            ("/etc/systemd/resolved.conf", Config),
            ("/etc/systemd/sleep.conf", Config),
            ("/etc/systemd/system.conf", Config),
            ("/etc/systemd/user.conf", Config),
            ("/etc/udev/iocost.conf", Config),
            ("/etc/udev/udev.conf", Config),
            // Another section's.
            ("/etc/crypttab", OwnedBy(InspectorId::Storage)),
            ("/etc/fstab", OwnedBy(InspectorId::Storage)),
            ("/etc/locale.conf", OwnedBy(InspectorId::KernelBoot)),
            ("/etc/selinux/config", OwnedBy(InspectorId::Selinux)),
            ("/etc/yum.repos.d/redhat.repo", OwnedBy(InspectorId::Rpm)),
            // Key material.
            ("/etc/chrony.keys", Credential),
            // Machine state.
            ("/etc/hostname", MachineState),
            ("/etc/machine-id", MachineState),
            // Written by augenrules, or the admin's: the host decides.
            ("/etc/audit/audit.rules", AuditRules),
            // Written on the host.
            ("/etc/authselect/dconf-db", Generated),
            ("/etc/authselect/dconf-locks", Generated),
            ("/etc/authselect/fingerprint-auth", Generated),
            ("/etc/authselect/nsswitch.conf", Generated),
            ("/etc/authselect/password-auth", Generated),
            ("/etc/authselect/postlogin", Generated),
            ("/etc/authselect/smartcard-auth", Generated),
            ("/etc/authselect/system-auth", Generated),
            ("/etc/avahi/etc/localtime", Generated),
            ("/etc/cni/net.d/87-podman-bridge.conflist", Generated),
            ("/etc/crypto-policies/back-ends/nss.config", Generated),
            ("/etc/crypto-policies/state/", Generated),
            ("/etc/foomatic/hashes.d/hashes.new", Generated),
            ("/etc/ld.so.cache", Generated),
            ("/etc/lvm/cache/.cache", Generated),
            ("/etc/machine-info", Generated),
            ("/etc/modprobe.d/kvm.rt.tuned.conf", Generated),
            ("/etc/pki/ca-trust/extracted/", Generated),
            ("/etc/selinux/targeted/contexts/files/", Generated),
            ("/etc/udev/hwdb.bin", Generated),
            // The locale item's.
            ("/etc/vconsole.conf", Deferred),
            ("/etc/X11/xorg.conf.d/00-keyboard.conf", Deferred),
        ]
    };

    /// Every ghost file measured on the two test hosts lands in the class
    /// the table above gives it. A ghost path a later measurement adds
    /// fails here until it is placed in the table, so it is classified on
    /// purpose, not by default.
    #[test]
    fn measured_ghost_files_land_in_known_classes() {
        let lists = [
            (
                "rhel-9.8",
                include_str!("../../../../../testdata/fixtures/config/ghost/rhel-9.8.txt"),
            ),
            (
                "rhel-10.2",
                include_str!("../../../../../testdata/fixtures/config/ghost/rhel-10.2.txt"),
            ),
        ];
        let mut used = vec![false; MEASURED_CLASSES.len()];
        for (release, list) in lists {
            let records: Vec<&str> = list.lines().filter(|l| !l.starts_with('#')).collect();
            assert!(records.len() > 150, "{release}: the whole list is read");
            for record in records {
                let path = record.split('|').next().expect("a path field");
                let entry = MEASURED_CLASSES.iter().position(|(rule, _)| {
                    rule == &path || (rule.ends_with('/') && path.starts_with(rule))
                });
                let Some(entry) = entry else {
                    panic!("{release}: {path} is in no class; add it to MEASURED_CLASSES");
                };
                used[entry] = true;
                assert_eq!(
                    classify_ghost(path),
                    MEASURED_CLASSES[entry].1,
                    "{release}: {path}"
                );
            }
        }
        for ((rule, _), used) in MEASURED_CLASSES.iter().zip(used) {
            assert!(used, "{rule} matches no measured path");
        }
    }
}
