//! The machine-state class's rules: which paths are members, and which
//! content is a credential whatever its path.
//!
//! The collector, the render guard and snapshot validation all read the
//! one table here, so a path cannot be a member to one of them and
//! ordinary to another. The types are in
//! [`crate::types::machine_state`].

use crate::types::machine_state::{CarriableMember, MachineStateMember};

/// How a rule matches a normalized absolute path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMatch {
    /// The whole path.
    Exact(&'static str),
    /// Segment-wise `*` and `?`, as the unowned walk's silent globs; a
    /// trailing `/*` (or a trailing `*` after a fixed stem) matches any
    /// depth below ([`match_glob`]).
    Glob(&'static str),
    /// A path-boundary prefix, written with its trailing slash.
    Prefix(&'static str),
    /// The final component ends with this text.
    BasenameSuffix(&'static str),
}

impl PathMatch {
    /// Whether `path`, already normalized, matches.
    pub fn matches(self, path: &str) -> bool {
        match self {
            Self::Exact(p) => path == p,
            Self::Glob(g) => match_glob(g, path),
            Self::Prefix(p) => path.starts_with(p),
            Self::BasenameSuffix(s) => path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.len() > s.len() && name.ends_with(s)),
        }
    }
}

/// What a matched rule says the path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTarget {
    /// Content never held.
    Withheld(MachineStateMember),
    /// Content held, excluded by default, overridable.
    Carriable(CarriableMember),
}

impl RuleTarget {
    /// The member the rule names.
    pub fn member(self) -> MachineStateMember {
        match self {
            Self::Withheld(m) => m,
            Self::Carriable(m) => MachineStateMember::Carriable(m),
        }
    }
}

use MachineStateMember as M;
use PathMatch::{BasenameSuffix, Exact, Glob, Prefix};
use RuleTarget::{Carriable as C, Withheld as W};

/// The class's path rules. First match wins: the SSH certificate rule
/// precedes the key rule so `*-cert.pub` is not swallowed by it.
pub const RULES: &[(PathMatch, RuleTarget)] = &[
    (Exact("/etc/machine-id"), W(M::MachineId)),
    (
        Exact("/etc/insights-client/machine-id"),
        W(M::InsightsState),
    ),
    (Glob("/etc/insights-client/.*"), W(M::InsightsState)),
    (
        Exact("/etc/rhsm/facts/insights-client.facts"),
        W(M::InsightsState),
    ),
    (Glob("/etc/pki/consumer/*"), W(M::RhsmConsumer)),
    (Glob("/etc/pki/entitlement/*"), W(M::RhsmEntitlement)),
    (Glob("/etc/lvm/devices/*"), W(M::LvmSystemDevices)),
    (Exact("/etc/krb5.keytab"), W(M::KerberosKeytab)),
    (Glob("/etc/krb5.keytab.*"), W(M::KerberosKeytab)),
    (BasenameSuffix(".keytab"), W(M::KerberosKeytab)),
    (
        Glob("/etc/ssh/ssh_host_*-cert.pub"),
        W(M::SshHostCertificate),
    ),
    (Glob("/etc/ssh/ssh_host_*"), W(M::SshHostKey)),
    (Prefix("/etc/pki/tls/private/"), W(M::PrivateKeyFile)),
    (Glob("/etc/cockpit/ws-certs.d/*.key"), W(M::PrivateKeyFile)),
    (Prefix("/etc/pki/libvirt/private/"), W(M::PrivateKeyFile)),
    (Prefix("/etc/puppetlabs/puppet/ssl/"), W(M::ConfigAgentKey)),
    (Prefix("/etc/salt/pki/minion/"), W(M::ConfigAgentKey)),
    (Exact("/etc/chef/client.pem"), W(M::ConfigAgentKey)),
    // Service secrets at fixed paths, which redaction does not parse:
    // chrony's NTP keys (`<id> <hash> HEX:<key>` lines; a `%ghost` on
    // RHEL 10, a plain `%config` file on RHEL 9), bind's rndc HMAC secret
    // (`rndc-confgen` puts it inline in `rndc.conf` when run without
    // `-a`), and the private keys `unbound-control-setup` writes.
    (Exact("/etc/chrony.keys"), W(M::ServiceKey)),
    (Exact("/etc/rndc.key"), W(M::ServiceKey)),
    (Exact("/etc/rndc.conf"), W(M::ServiceKey)),
    (Exact("/etc/unbound/unbound_control.key"), W(M::ServiceKey)),
    (Exact("/etc/unbound/unbound_server.key"), W(M::ServiceKey)),
    (
        Exact("/etc/iscsi/initiatorname.iscsi"),
        C(CarriableMember::IscsiInitiatorName),
    ),
    (
        Exact("/etc/salt/minion_id"),
        C(CarriableMember::SaltMinionId),
    ),
    (
        Exact("/etc/ipa/default.conf"),
        C(CarriableMember::IpaDefaultConf),
    ),
    (Exact("/etc/hostname"), C(CarriableMember::Hostname)),
    (
        Exact("/etc/rhsm/syspurpose/syspurpose.json"),
        C(CarriableMember::Syspurpose),
    ),
    (Exact("/etc/nvme/hostnqn"), C(CarriableMember::NvmeHostId)),
    (Exact("/etc/nvme/hostid"), C(CarriableMember::NvmeHostId)),
    (
        Exact("/etc/authselect/switchable-auth"),
        C(CarriableMember::AuthselectSwitchable),
    ),
    (Exact("/etc/grub.d/00_tuned"), C(CarriableMember::GrubTuned)),
];

/// The class rule `path` matches, after normalizing it. `None` for an
/// ordinary path, or one that does not normalize.
pub fn classify_path(path: &str) -> Option<RuleTarget> {
    let path = normal_path(path)?;
    RULES
        .iter()
        .find(|(m, _)| m.matches(&path))
        .map(|(_, target)| *target)
}

/// Lexical normalization: drop empty and `.` components and lead with
/// `/`. `None` for `..`, a NUL or other control character, or a path with
/// no component. The export's `Destination` performs the same pass, so
/// `etc/ssh/x`, `/etc/./ssh/x` and `/etc//ssh/x` are one path to the
/// classifier and to the stager.
pub fn normal_path(path: &str) -> Option<String> {
    if path
        .chars()
        .any(|c| c.is_control() || is_line_terminator(c))
    {
        return None;
    }
    let mut normal = String::with_capacity(path.len() + 1);
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => return None,
            name => {
                normal.push('/');
                normal.push_str(name);
            }
        }
    }
    (!normal.is_empty()).then_some(normal)
}

/// The Unicode line and paragraph separators, which are not `char::is_control`.
fn is_line_terminator(c: char) -> bool {
    matches!(c, '\u{2028}' | '\u{2029}' | '\u{85}')
}

/// Matches a path against a glob pattern.
///
/// Patterns ending in `*` after a fixed stem match any path starting with
/// the stem, at any depth. A `*` in an intermediate segment matches
/// exactly one segment.
pub fn match_glob(pattern: &str, path: &str) -> bool {
    if glob_match_simple(pattern, path) {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*')
        && path.starts_with(prefix)
    {
        return true;
    }
    let pat_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    match_parts(&pat_parts, &path_parts)
}

/// Matches path segments against pattern segments. A `*` in a
/// non-terminal position matches exactly one segment; in the terminal
/// position, one or more remaining segments.
fn match_parts(pat: &[&str], path: &[&str]) -> bool {
    let mut pi = 0;
    let mut qi = 0;
    while pi < pat.len() && qi < path.len() {
        if pi == pat.len() - 1 && pat[pi] == "*" {
            return true;
        }
        if !glob_match_simple(pat[pi], path[qi]) {
            return false;
        }
        pi += 1;
        qi += 1;
    }
    pi == pat.len() && qi == path.len()
}

/// `*` (any sequence) and `?` (one character) over one string.
fn glob_match_simple(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (usize::MAX, 0);
    while ti < txt.len() {
        if pi < pat.len() && (pat[pi] == '?' || pat[pi] == txt[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == '*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == '*' {
        pi += 1;
    }
    pi == pat.len()
}

/// How many bytes the keytab header sniff reads: the only prefix read
/// anything in the scan makes.
pub const KEYTAB_HEADER_LEN: usize = 2;

/// The first two bytes of a Kerberos keytab: file format version 1 or 2.
const KEYTAB_HEADERS: [[u8; KEYTAB_HEADER_LEN]; 2] = [[0x05, 0x01], [0x05, 0x02]];

/// Whether `head` starts with a keytab header.
pub fn is_keytab_header(head: &[u8]) -> bool {
    KEYTAB_HEADERS.iter().any(|h| head.starts_with(h))
}

/// The credential member `content` is by its bytes, whatever its path: a
/// keytab by its header, or a file that is a private key and nothing else
/// ([`is_whole_file_private_key`]).
pub fn credential_content(content: &[u8]) -> Option<MachineStateMember> {
    if is_keytab_header(content) {
        return Some(MachineStateMember::KerberosKeytab);
    }
    let text = std::str::from_utf8(content).ok()?;
    is_whole_file_private_key(text).then_some(MachineStateMember::PrivateKeyFile)
}

/// What every private-key PEM label ends with: `PRIVATE KEY`, `ENCRYPTED
/// PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`, `DSA PRIVATE KEY`,
/// `OPENSSH PRIVATE KEY`.
const PRIVATE_KEY_LABEL_SUFFIX: &str = "PRIVATE KEY";

/// PEM labels that may sit beside a private key in a key file.
const COMPANION_LABELS: &[&str] = &[
    "CERTIFICATE",
    "TRUSTED CERTIFICATE",
    "X509 CRL",
    "EC PARAMETERS",
    "DH PARAMETERS",
    "DSA PARAMETERS",
];

/// Lines `openssl pkcs12` writes before each block, matched at the start
/// of an unindented line.
const ENVELOPE_PREFIXES: &[&str] = &["Bag Attributes", "Key Attributes", "subject=", "issuer="];

const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";
const PEM_DASHES: &str = "-----";

/// Every line outside a PEM block is blank, a `#` comment or `openssl`
/// envelope metadata, every block's label is a private-key label or a
/// companion, and at least one block is a private key. Any other line (an
/// application setting) makes the file configuration, and a key embedded
/// in it is left to redaction. A block that never ends counts as a block
/// to the end of the file, so a cut-off key is still a key.
pub fn is_whole_file_private_key(content: &str) -> bool {
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
    use crate::types::machine_state::MachineStateMember;

    #[test]
    fn classifies_by_table_order_and_normalizes_first() {
        let cases: &[(&str, Option<MachineStateMember>)] = &[
            ("/etc/machine-id", Some(M::MachineId)),
            ("etc/ssh/ssh_host_ed25519_key", Some(M::SshHostKey)),
            ("/etc/ssh/./ssh_host_ed25519_key", Some(M::SshHostKey)),
            ("/etc//ssh/ssh_host_ed25519_key", Some(M::SshHostKey)),
            ("/etc/ssh/ssh_host_rsa_key.pub", Some(M::SshHostKey)),
            (
                "/etc/ssh/ssh_host_rsa_key-cert.pub",
                Some(M::SshHostCertificate),
            ),
            ("/etc/myapp/service.keytab", Some(M::KerberosKeytab)),
            ("/etc/krb5.keytab.ad", Some(M::KerberosKeytab)),
            ("/etc/pki/tls/private/server.key", Some(M::PrivateKeyFile)),
            ("/etc/pki/tls/private", None),
            ("/etc/chrony.keys", Some(M::ServiceKey)),
            ("/etc/rndc.conf", Some(M::ServiceKey)),
            ("/etc/unbound/unbound_server.key", Some(M::ServiceKey)),
            (
                "/etc/iscsi/initiatorname.iscsi",
                Some(M::Carriable(CarriableMember::IscsiInitiatorName)),
            ),
            (
                "/etc/hostname",
                Some(M::Carriable(CarriableMember::Hostname)),
            ),
            ("/etc/insights-client/.registered", Some(M::InsightsState)),
            ("/etc/insights-client/tags.yaml", None),
            ("/etc/myapp/app.conf", None),
            ("/etc/../etc/machine-id", None),
            ("/etc/.keytab", None),
        ];
        for (path, want) in cases {
            assert_eq!(classify_path(path).map(RuleTarget::member), *want, "{path}");
        }
    }

    #[test]
    fn every_member_is_the_target_of_a_rule() {
        let targets: Vec<MachineStateMember> = RULES.iter().map(|(_, t)| t.member()).collect();
        for member in MachineStateMember::WITHHELD {
            assert!(targets.contains(member), "{member:?} has no rule");
        }
        for member in CarriableMember::ALL {
            assert!(
                targets.contains(&MachineStateMember::Carriable(*member)),
                "{member:?} has no rule"
            );
        }
        let cert = RULES
            .iter()
            .position(|(_, t)| t.member() == M::SshHostCertificate)
            .unwrap();
        let key = RULES
            .iter()
            .position(|(_, t)| t.member() == M::SshHostKey)
            .unwrap();
        assert!(cert < key, "the certificate rule precedes the key rule");
        for (_, target) in RULES {
            if let RuleTarget::Withheld(m) = target {
                assert!(
                    !matches!(m, M::Carriable(_)),
                    "withheld rules name withheld members"
                );
            }
        }
    }

    #[test]
    fn normal_path_refuses_traversal_and_controls() {
        assert_eq!(normal_path("etc/x").as_deref(), Some("/etc/x"));
        assert_eq!(normal_path("/etc/./x/").as_deref(), Some("/etc/x"));
        assert_eq!(normal_path("/etc/../x"), None);
        assert_eq!(normal_path("/etc/x\0"), None);
        assert_eq!(normal_path("/etc/x\ny"), None);
        assert_eq!(normal_path("/etc/x\u{2028}"), None);
        assert_eq!(normal_path("//"), None);
    }

    #[test]
    fn credential_content_by_header_and_whole_file_key() {
        assert_eq!(
            credential_content(&[0x05, 0x02, 0xff, 0x00]),
            Some(M::KerberosKeytab)
        );
        assert_eq!(credential_content(b"\x05\x01abc"), Some(M::KerberosKeytab));
        let key =
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nMIIB\n-----END ENCRYPTED PRIVATE KEY-----\n";
        assert_eq!(credential_content(key.as_bytes()), Some(M::PrivateKeyFile));
        let ec = "-----BEGIN EC PARAMETERS-----\nBgg=\n-----END EC PARAMETERS-----\n\
                  -----BEGIN EC PRIVATE KEY-----\nMHc=\n-----END EC PRIVATE KEY-----\n";
        assert_eq!(credential_content(ec.as_bytes()), Some(M::PrivateKeyFile));
        let p12 = "Bag Attributes\n    localKeyID: 01\nKey Attributes: <No Attributes>\n\
                   -----BEGIN PRIVATE KEY-----\nMII=\n-----END PRIVATE KEY-----\n\
                   subject=CN = x\nissuer=CN = x\n-----BEGIN CERTIFICATE-----\nMII=\n-----END CERTIFICATE-----\n";
        assert_eq!(credential_content(p12.as_bytes()), Some(M::PrivateKeyFile));
        let cert = "-----BEGIN CERTIFICATE-----\nMII=\n-----END CERTIFICATE-----\n";
        assert_eq!(credential_content(cert.as_bytes()), None);
        let embedded = format!("listen = 443\n{key}");
        assert_eq!(credential_content(embedded.as_bytes()), None);
        assert_eq!(credential_content(b"plain text"), None);
    }
}
