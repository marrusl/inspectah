//! The `%ghost` branch of the config inspector: owned files rpm never
//! verifies.
//!
//! A package lists a `%ghost` file without shipping its content, so rpm
//! has nothing to compare it with and `rpm -V` skips it, and the unowned
//! walk skips it because a package owns it. Before this branch an admin's
//! edit to a ghost file under `/etc` reached no collector. Measured on
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
/// owning package, rpm's `fflags` rendering of `FILEFLAGS`, then the path.
///
/// `%{=NAME}` repeats the scalar tag on each iteration of the file
/// arrays, which `[%{NAME} ...]` cannot (rpm refuses arrays of different
/// sizes in one iterator). The path is last, so a path holding a space
/// survives: the line is split at its first two spaces only.
pub(crate) const RPM_FILE_FLAGS_FORMAT: &str = "[%{=NAME} %{FILEFLAGS:fflags} %{FILENAMES}\\n]";

/// The letter rpm's `fflags` formatter prints for `RPMFILE_GHOST`.
const GHOST_FLAG: char = 'g';

/// Every letter `fflags` can print: doc, config, specfile, missingok,
/// noreplace, ghost, license, readme, artifact (rpm `lib/formats.c`,
/// `fflagsFormat`). A flags field with any other byte is not one, and the
/// line is not trusted.
const FFLAGS_LETTERS: &str = "dcsmnglra";

/// The directory the config inspector covers.
const ETC_PREFIX: &str = "/etc/";

/// A `%ghost` file under `/etc` and the package that lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GhostFile {
    pub path: String,
    pub package: String,
}

/// The ghost files under `/etc` in the output of an `rpm -qa` run with
/// [`RPM_FILE_FLAGS_FORMAT`], sorted by path. A path two packages list is
/// kept once, under the first package in path order.
///
/// A line whose flags field holds a byte `fflags` never prints is
/// skipped rather than guessed at.
pub(crate) fn parse_ghost_files(stdout: &str) -> Vec<GhostFile> {
    let mut out: Vec<GhostFile> = stdout
        .lines()
        .filter_map(|line| {
            let (package, rest) = line.split_once(' ')?;
            let (flags, path) = rest.split_once(' ')?;
            if package.is_empty() || !flags.chars().all(|c| FFLAGS_LETTERS.contains(c)) {
                return None;
            }
            (flags.contains(GHOST_FLAG) && path.starts_with(ETC_PREFIX)).then(|| GhostFile {
                path: path.to_string(),
                package: package.to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path).then(a.package.cmp(&b.package)));
    out.dedup_by(|a, b| a.path == b.path);
    out
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

/// Whether the ghost branch leaves `path` alone on [`GHOST_SKIP_EXACT`]
/// or [`GHOST_SKIP_PREFIX`].
pub(crate) fn is_ghost_skip(path: &str) -> bool {
    GHOST_SKIP_EXACT.contains(&path) || GHOST_SKIP_PREFIX.iter().any(|p| path.starts_with(p))
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
// The content sniffs (keytab header, whole-file private key) are the
// class's read gate and are not repeated here; a keytab is binary and is
// not captured as text either way.

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines as `rpm -qa --queryformat` prints them with
    /// [`RPM_FILE_FLAGS_FORMAT`]: an empty flags field leaves two spaces.
    const QUERY_OUTPUT: &str = "\
sssd-common cng /etc/sssd/sssd.conf
sssd-common  /usr/libexec/sssd/sssd_be
authselect-libs g /etc/authselect/nsswitch.conf
authselect-libs g /etc/authselect/authselect.conf
grub2-tools cng /etc/default/grub
systemd cng /etc/machine-id
setup cn /etc/hosts
glibc g /var/cache/ldconfig/aux-cache
myapp g /etc/my app/state file
bogus cXq /etc/not/flags
";

    #[test]
    fn parses_ghost_files_under_etc_only() {
        let got = parse_ghost_files(QUERY_OUTPUT);
        let paths: Vec<(&str, &str)> = got
            .iter()
            .map(|g| (g.path.as_str(), g.package.as_str()))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("/etc/authselect/authselect.conf", "authselect-libs"),
                ("/etc/authselect/nsswitch.conf", "authselect-libs"),
                ("/etc/default/grub", "grub2-tools"),
                ("/etc/machine-id", "systemd"),
                ("/etc/my app/state file", "myapp"),
                ("/etc/sssd/sssd.conf", "sssd-common"),
            ],
            "ghost /etc paths only, a path with spaces intact, a non-fflags field refused"
        );
    }

    #[test]
    fn a_path_two_packages_list_is_kept_once() {
        let got = parse_ghost_files("b g /etc/x\na g /etc/x\n");
        assert_eq!(
            got,
            vec![GhostFile {
                path: "/etc/x".into(),
                package: "a".into()
            }]
        );
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
    fn ghost_skips() {
        assert!(is_ghost_skip("/etc/locale.conf"));
        assert!(is_ghost_skip("/etc/crypto-policies/state/current"));
        assert!(!is_ghost_skip("/etc/crypto-policies/config"));
        assert!(!is_ghost_skip("/etc/sssd/sssd.conf"));
    }
}
