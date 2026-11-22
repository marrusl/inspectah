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
/// the scan makes.
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

/// How many bytes the unmanaged payload check reads at a time. Its memory
/// is this buffer and two PEM labels of at most [`PEM_LABEL_MAX`] bytes,
/// whatever the file's length.
pub const PAYLOAD_READ_CHUNK: usize = 64 * 1024;

/// The longest PEM label [`PemKeyFinder`] reads. RFC 7468's labels, and
/// every private-key label, are far shorter.
const PEM_LABEL_MAX: usize = 64;

/// The most bytes [`PemKeyFinder`] follows between a private-key `BEGIN`
/// line and its `END` line. A 16384-bit RSA key in PEM is about 13 KiB.
const KEY_BODY_MAX: usize = 64 * 1024;

/// The fewest base64 characters a body needs to be a key. A body with
/// none, such as a `BEGIN` line directly followed by its `END` line in a
/// template, is a placeholder that holds no key material.
const KEY_BODY_MIN_BASE64: usize = 1;

/// The run of dashes both PEM markers start with.
const PEM_DASH_RUN: usize = PEM_DASHES.len();

/// The credential member an unmanaged payload is by its bytes, read from
/// `reader` to its end, binary-safe and in bounded memory
/// ([`PAYLOAD_READ_CHUNK`]): a keytab by its first two bytes, which wins,
/// else a private key when a PEM private-key block appears anywhere in it
/// ([`PemKeyFinder`]). The verdict is reached only from what the file
/// holds; no part of a file stands in for the whole of it. DER private
/// keys and PKCS#12 archives are not recognized.
pub fn payload_credential(reader: &mut dyn io::Read) -> io::Result<Option<MachineStateMember>> {
    let mut buf = vec![0; PAYLOAD_READ_CHUNK];
    let mut head = Vec::with_capacity(KEYTAB_HEADER_LEN);
    let mut finder = PemKeyFinder::default();
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let chunk = &buf[..n];
        if head.len() < KEYTAB_HEADER_LEN {
            let take = (KEYTAB_HEADER_LEN - head.len()).min(n);
            head.extend_from_slice(&chunk[..take]);
            if is_keytab_header(&head) {
                return Ok(Some(MachineStateMember::KerberosKeytab));
            }
        }
        if finder.feed(chunk) {
            return Ok(Some(MachineStateMember::PrivateKeyFile));
        }
    }
    Ok(finder
        .finish()
        .then_some(MachineStateMember::PrivateKeyFile))
}

/// Finds a PEM private-key block anywhere in a byte stream fed to it a
/// buffer at a time, so a block split across two buffers is still one
/// block.
///
/// A block is a `-----BEGIN <label>-----` marker whose label ends with
/// `PRIVATE KEY` (plain, encrypted, RSA, EC, DSA and OpenSSH keys alike),
/// at any position in a line, then a body holding base64
/// ([`KEY_BODY_MIN_BASE64`]), then the `END` marker with
/// a private-key label, or the end of the stream: a cut-off key is a key.
/// The body may hold only bytes a key body holds in a PEM file or
/// embedded in text ([`is_key_body_byte`]): indentation, as in a YAML
/// block scalar, `\n` escapes and quotes, as in JSON, and RFC 1421
/// headers. Any other byte ends the candidate, so a `BEGIN` marker
/// quoted in source code or prose followed by anything else is not a key.
/// A certificate, CRL or parameters block is not a candidate at all.
#[derive(Debug, Default)]
pub struct PemKeyFinder {
    state: KeyFind,
    found: bool,
}

/// Where [`PemKeyFinder`] is in the stream.
#[derive(Debug)]
enum KeyFind {
    /// Outside any candidate: how many bytes of [`PEM_BEGIN`] match.
    Seek(usize),
    /// Reading a `BEGIN` marker's label, and the dashes that close it.
    BeginLabel(LabelRead),
    /// Inside a private-key body.
    Body {
        len: usize,
        base64: usize,
        /// How many bytes of [`PEM_END`] match.
        end: usize,
        /// The `END` marker's label, once the marker matched.
        end_label: Option<LabelRead>,
    },
}

impl Default for KeyFind {
    fn default() -> Self {
        Self::Seek(0)
    }
}

/// A PEM label being read, up to the dashes that close it.
#[derive(Debug, Default)]
struct LabelRead {
    label: Vec<u8>,
    dashes: usize,
}

/// What one more byte did to a [`LabelRead`].
enum LabelStep {
    More,
    /// The closing dashes: the label is complete.
    Closed,
    /// A byte no label holds.
    Invalid,
}

impl LabelRead {
    fn step(&mut self, b: u8) -> LabelStep {
        if b == b'-' {
            self.dashes += 1;
            return if self.dashes == PEM_DASH_RUN {
                LabelStep::Closed
            } else {
                LabelStep::More
            };
        }
        let label_byte = b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ';
        if self.dashes > 0 || !label_byte || self.label.len() == PEM_LABEL_MAX {
            return LabelStep::Invalid;
        }
        self.label.push(b);
        LabelStep::More
    }

    fn is_private_key(&self) -> bool {
        self.label.ends_with(PRIVATE_KEY_LABEL_SUFFIX.as_bytes())
    }
}

impl PemKeyFinder {
    /// Feeds the next bytes of the stream; true once a private-key block
    /// has been found.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut rest = bytes;
        while !self.found && !rest.is_empty() {
            // Outside a candidate, skip to the next dash at once.
            if let KeyFind::Seek(0) = self.state {
                match rest.iter().position(|&b| b == b'-') {
                    Some(at) => rest = &rest[at..],
                    None => break,
                }
            }
            self.step(rest[0]);
            rest = &rest[1..];
        }
        self.found
    }

    /// Whether the stream held a private-key block, the stream having
    /// ended: a body still open at the end is a cut-off key.
    pub fn finish(&self) -> bool {
        self.found
            || matches!(self.state, KeyFind::Body { base64, .. } if base64 >= KEY_BODY_MIN_BASE64)
    }

    fn step(&mut self, b: u8) {
        self.state = match std::mem::take(&mut self.state) {
            KeyFind::Seek(k) => KeyFind::Seek(marker_step(PEM_BEGIN.as_bytes(), k, b)),
            KeyFind::BeginLabel(mut read) => match read.step(b) {
                LabelStep::More => KeyFind::BeginLabel(read),
                LabelStep::Closed if read.is_private_key() => KeyFind::Body {
                    len: 0,
                    base64: 0,
                    end: 0,
                    end_label: None,
                },
                // The closing dashes may open the next marker.
                LabelStep::Closed => KeyFind::Seek(PEM_DASH_RUN),
                LabelStep::Invalid => KeyFind::Seek(marker_step(PEM_BEGIN.as_bytes(), 0, b)),
            },
            KeyFind::Body {
                len,
                base64,
                end,
                end_label,
            } => self.body_step(b, len + 1, base64, end, end_label),
        };
        if let KeyFind::Seek(k) = self.state
            && k == PEM_BEGIN.len()
        {
            self.state = KeyFind::BeginLabel(LabelRead::default());
        }
    }

    fn body_step(
        &mut self,
        b: u8,
        len: usize,
        mut base64: usize,
        end: usize,
        end_label: Option<LabelRead>,
    ) -> KeyFind {
        if !is_key_body_byte(b) || len > KEY_BODY_MAX {
            return KeyFind::Seek(marker_step(PEM_BEGIN.as_bytes(), 0, b));
        }
        if let Some(mut read) = end_label {
            return match read.step(b) {
                LabelStep::More => KeyFind::Body {
                    len,
                    base64,
                    end: 0,
                    end_label: Some(read),
                },
                LabelStep::Closed => {
                    self.found = read.is_private_key() && base64 >= KEY_BODY_MIN_BASE64;
                    KeyFind::Seek(PEM_DASH_RUN)
                }
                // Not an `END` line after all: still body.
                LabelStep::Invalid => KeyFind::Body {
                    len,
                    base64: base64 + usize::from(is_base64_byte(b)),
                    end: marker_step(PEM_END.as_bytes(), 0, b),
                    end_label: None,
                },
            };
        }
        let end = marker_step(PEM_END.as_bytes(), end, b);
        if end == PEM_END.len() {
            return KeyFind::Body {
                len,
                base64,
                end: 0,
                end_label: Some(LabelRead::default()),
            };
        }
        // A byte that may belong to an `END` marker is not counted.
        if end == 0 && is_base64_byte(b) {
            base64 += 1;
        }
        KeyFind::Body {
            len,
            base64,
            end,
            end_label: None,
        }
    }
}

/// How many bytes of `marker` match after `b`, when `k` matched before.
/// Both markers are a run of dashes then letters, so after a mismatch the
/// only part that can still match is that run.
fn marker_step(marker: &[u8], k: usize, b: u8) -> usize {
    if marker[k] == b {
        k + 1
    } else if b == b'-' && k == PEM_DASH_RUN {
        PEM_DASH_RUN
    } else {
        usize::from(b == b'-')
    }
}

fn is_base64_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')
}

/// A byte a private-key body holds, in a PEM file or embedded in other
/// text: base64; whitespace and line ends, so indentation as in a YAML
/// block scalar; RFC 1421 header text (`Proc-Type: 4,ENCRYPTED`,
/// `DEK-Info: AES-256-CBC,...`); and the escapes, quotes and comment
/// marks that carry a key in JSON, YAML, a shell script or source code
/// (`\n`, `"`, `'`, `#`).
fn is_key_body_byte(b: u8) -> bool {
    is_base64_byte(b)
        || matches!(
            b,
            b' ' | b'\t' | b'\r' | b'\n' | b'\\' | b'"' | b'\'' | b',' | b':' | b'-' | b'#'
        )
}

/// The label of a `-----BEGIN <label>-----` or `-----END <label>-----`
/// line, by `marker`.
fn pem_label<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.trim()
        .strip_prefix(marker)?
        .strip_suffix(PEM_DASHES)
        .filter(|label| !label.is_empty())
}

// ---------------------------------------------------------------------------
// The read gate
// ---------------------------------------------------------------------------

use crate::traits::executor::{ExecResult, Executor, FileKind, FileMetadata, HostRead};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// How many directories deep the screen follows a withheld rule's tree,
/// directory links included, before it gives up on establishing the
/// boundary ([`ScreenGapCause::TooDeep`]). Far deeper than any tool
/// writes its keys.
const SCREEN_MAX_DEPTH: usize = 32;

/// How many directory entries one screen build examines before it gives
/// up ([`ScreenGapCause::TooManyEntries`]): a link from a member
/// directory to `/` must fail fast, not walk the filesystem.
const SCREEN_MAX_ENTRIES: usize = 1 << 16;

/// What the gate says of a path, before opening it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screened {
    /// A withheld member by its path, the path it resolves to, or its
    /// inode. Never opened.
    Withheld(MachineStateMember),
    /// A carriable member by its own path. Read like any file.
    Carriable(CarriableMember),
    /// Not a member by path, alias or inode.
    Ordinary,
}

/// Why a collector reads the host without the gate. Each variant is one
/// reviewed exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenBypass {
    /// The subscription inspector's reads of `/etc/pki/entitlement/` and
    /// `/etc/pki/consumer/cert.pem`, which `--preserve subscription
    /// --ack-sensitive` stores in its own section on purpose. That section
    /// is outside snapshot validation.
    SubscriptionSection,
}

/// The `(dev, ino)` of every host file the withheld rules' `Exact`, `Glob`
/// and `Prefix` patterns match, with its member. Built once per scan.
#[derive(Debug, Clone, Default)]
pub struct MachineStateScreen {
    inodes: HashMap<(u64, u64), MachineStateMember>,
    /// Where the build could not list every matching file, if it could
    /// not. The gate then withholds everything ([`ScreenedExecutor::screen`]),
    /// and the scan refuses to run ([`ScreenedExecutor::gap`]).
    gap: Option<ScreenGap>,
}

/// Why the screen could not list every file a withheld rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenGapCause {
    /// The tree goes deeper than the screen follows.
    TooDeep,
    /// The rules' trees hold more entries than one build examines.
    TooManyEntries,
    /// An entry's name is not UTF-8, so it cannot be named to `stat` it.
    NameNotUtf8,
    /// The directory could not be listed in full.
    Unlistable(io::ErrorKind),
}

impl std::fmt::Display for ScreenGapCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooDeep => write!(f, "it is more than {SCREEN_MAX_DEPTH} directories deep"),
            Self::TooManyEntries => write!(f, "it holds more than {SCREEN_MAX_ENTRIES} entries"),
            Self::NameNotUtf8 => write!(f, "an entry's name is not UTF-8"),
            Self::Unlistable(kind) => write!(f, "it could not be listed in full ({kind})"),
        }
    }
}

/// A directory below which the screen could not establish which files
/// are machine-state files: the logical path a withheld rule names, the
/// rule's member, and why. Its text holds paths and fixed words only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenGap {
    pub dir: String,
    pub member: MachineStateMember,
    pub cause: ScreenGapCause,
}

impl std::fmt::Display for ScreenGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot tell which files under {} are machine-state files ({}): {}. \
             Rename or remove the entry, or repair the directory or the link \
             to it, then re-run the scan",
            self.dir.escape_debug(),
            self.member.as_str(),
            self.cause
        )
    }
}

impl std::error::Error for ScreenGap {}

impl MachineStateScreen {
    /// Stats every host file a withheld rule matches, reading no file
    /// content. Each `Glob` and `Prefix` rule's tree is listed from the
    /// rule's own directory, following directory links (each directory
    /// once on any one path, so a link back up ends there) and keeping
    /// the logical spelling the rule matches, and descending only where a
    /// match is still possible. Where the listing cannot be completed the
    /// build records a [`ScreenGap`] and stops. `BasenameSuffix` rules are
    /// not enumerated: that would take a walk of `/etc`, and a keytab
    /// linked under another name is caught by its header.
    pub fn build(exec: &dyn Executor) -> Self {
        let mut screen = Self::default();
        let mut budget = SCREEN_MAX_ENTRIES;
        for (rule, target) in RULES {
            let RuleTarget::Withheld(member) = *target else {
                continue;
            };
            let (root, nfa) = match *rule {
                PathMatch::Exact(path) => {
                    screen.note(exec, Path::new(path), member);
                    continue;
                }
                PathMatch::Glob(pattern) => (glob_dir(pattern), RuleNfa::glob(pattern)),
                PathMatch::Prefix(prefix) => {
                    (prefix.trim_end_matches('/'), RuleNfa::prefix(prefix))
                }
                PathMatch::BasenameSuffix(_) => continue,
            };
            let Ok(meta) = exec.file_metadata(Path::new(root)) else {
                continue;
            };
            let physical = exec
                .resolve_final_target(Path::new(root))
                .unwrap_or_else(|_| PathBuf::from(root));
            let state = nfa.read(&nfa.start(), format!("{root}/").chars().map(PathSym::Char));
            let mut walk = RuleWalk {
                rule: *rule,
                nfa,
                member,
                listed: HashMap::from([((meta.dev, meta.ino), vec![state.clone()])]),
                budget: &mut budget,
            };
            let listed = walk.walk(
                &mut screen.inodes,
                exec,
                root,
                &physical,
                &state,
                SCREEN_MAX_DEPTH,
            );
            if let Err(gap) = listed {
                screen.gap = Some(gap);
                break;
            }
        }
        screen
    }

    fn note(&mut self, exec: &dyn Executor, path: &Path, member: MachineStateMember) {
        if let Ok(meta) = exec.file_metadata(path)
            && meta.is_regular()
        {
            self.inodes.entry((meta.dev, meta.ino)).or_insert(member);
        }
    }

    /// The withheld member whose file has this inode.
    pub fn member_by_inode(&self, meta: &FileMetadata) -> Option<MachineStateMember> {
        self.inodes.get(&(meta.dev, meta.ino)).copied()
    }
}

/// One withheld rule's tree, being listed.
struct RuleWalk<'b> {
    rule: PathMatch,
    nfa: RuleNfa,
    member: MachineStateMember,
    /// Each directory listed so far, by `(dev, ino)`, with the matching
    /// states it was listed under. A directory reached again under a
    /// state one of these covers holds nothing the rule has not already
    /// seen; under any other state it is listed again, since a link can
    /// give the same files a spelling the rule matches (a `.state` link
    /// back to `/etc/insights-client` makes `registration.json` there
    /// `.state/registration.json`). States are finite, so link cycles end.
    listed: HashMap<(u64, u64), Vec<MatchState>>,
    budget: &'b mut usize,
}

impl RuleWalk<'_> {
    /// Lists `physical`, the directory the logical path `logical` names,
    /// read under the rule's matching `state` after `logical/`, noting
    /// each matching regular file's inode and descending into each
    /// directory, linked or not, below which a match is still possible
    /// and which has not been listed under a covering state.
    fn walk(
        &mut self,
        inodes: &mut HashMap<(u64, u64), MachineStateMember>,
        exec: &dyn Executor,
        logical: &str,
        physical: &Path,
        state: &MatchState,
        depth: usize,
    ) -> Result<(), ScreenGap> {
        let member = self.member;
        let gap = |dir: &str, cause| ScreenGap {
            dir: dir.to_string(),
            member,
            cause,
        };
        let names = match exec.read_dir_strict(physical) {
            Ok(names) => names,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(());
            }
            Err(e) => return Err(gap(logical, ScreenGapCause::Unlistable(e.kind()))),
        };
        for name in names {
            // A name read lossily: the part before the first replaced
            // byte is what the real name starts with.
            if let Some(cut) = name.find(char::REPLACEMENT_CHARACTER) {
                if may_match_below(self.rule, &format!("{logical}/{}", &name[..cut])) {
                    return Err(gap(logical, ScreenGapCause::NameNotUtf8));
                }
                continue;
            }
            *self.budget = self
                .budget
                .checked_sub(1)
                .ok_or_else(|| gap(logical, ScreenGapCause::TooManyEntries))?;
            let child = format!("{logical}/{name}");
            let child_physical = physical.join(&name);
            // A dangling or looping link names no file to read through it.
            let Ok(meta) = exec.file_metadata(&child_physical) else {
                continue;
            };
            if meta.is_regular() {
                if self.rule.matches(&child) {
                    inodes.entry((meta.dev, meta.ino)).or_insert(self.member);
                }
                continue;
            }
            if !meta.is_directory() {
                continue;
            }
            let below = self
                .nfa
                .read(state, name.chars().chain(['/']).map(PathSym::Char));
            if !below.alive_below() {
                continue;
            }
            let seen = self.listed.entry((meta.dev, meta.ino)).or_default();
            if seen.iter().any(|listed| listed.covers(&below)) {
                continue;
            }
            seen.push(below.clone());
            if depth == 0 {
                return Err(gap(&child, ScreenGapCause::TooDeep));
            }
            let next = exec
                .resolve_final_target(&child_physical)
                .unwrap_or(child_physical);
            self.walk(inodes, exec, &child, &next, &below, depth - 1)?;
        }
        Ok(())
    }
}

/// One symbol of a path the rule automaton reads.
#[derive(Debug, Clone, Copy)]
enum PathSym {
    Char(char),
}

/// A walked rule's pattern read as a nondeterministic automaton over a
/// path's characters, with `*` matching any run and `?` one character,
/// as [`glob_match_simple`] does. A `Prefix` rule is its prefix then `*`.
struct RuleNfa {
    pat: Vec<char>,
}

/// The pattern positions live after reading the start of a path; the
/// last position is a whole match. Two paths reaching the same state
/// match exactly the same continuations, and a state that is live
/// wherever another is ([`covers`](Self::covers)) matches every
/// continuation that one does.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchState(Vec<bool>);

impl MatchState {
    /// Whether some longer path can still match: a live position short
    /// of the end.
    fn alive_below(&self) -> bool {
        self.0[..self.0.len() - 1].iter().any(|&live| live)
    }

    /// Whether every position live in `other` is live here.
    fn covers(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(&other.0)
            .all(|(&mine, &theirs)| mine || !theirs)
    }
}

impl RuleNfa {
    /// A `Glob` rule's pattern.
    fn glob(pattern: &str) -> Self {
        Self {
            pat: pattern.chars().collect(),
        }
    }

    /// A `Prefix` rule: everything that starts with `prefix`.
    fn prefix(prefix: &str) -> Self {
        Self {
            pat: prefix.chars().chain(['*']).collect(),
        }
    }

    /// The state before any character.
    fn start(&self) -> MatchState {
        let mut live = vec![false; self.pat.len() + 1];
        live[0] = true;
        self.close(&mut live);
        MatchState(live)
    }

    /// The state after reading `syms` from `state`.
    fn read(&self, state: &MatchState, syms: impl IntoIterator<Item = PathSym>) -> MatchState {
        let mut live = state.0.clone();
        for sym in syms {
            let mut next = vec![false; self.pat.len() + 1];
            for i in (0..self.pat.len()).filter(|&i| live[i]) {
                match (self.pat[i], sym) {
                    ('*', _) => next[i] = true,
                    ('?', _) => next[i + 1] = true,
                    (p, PathSym::Char(c)) if p == c => next[i + 1] = true,
                    _ => {}
                }
            }
            self.close(&mut next);
            live = next;
        }
        MatchState(live)
    }

    /// A `*` may match nothing: the position after a live `*` is live.
    fn close(&self, live: &mut [bool]) {
        for i in 0..self.pat.len() {
            if live[i] && self.pat[i] == '*' {
                live[i + 1] = true;
            }
        }
    }
}

/// Whether some path starting with `start` can match `rule`: the rule's
/// pattern, read from its left, has not yet failed on `start`.
fn may_match_below(rule: PathMatch, start: &str) -> bool {
    match rule {
        PathMatch::Exact(path) => path.starts_with(start),
        PathMatch::Prefix(prefix) => start.starts_with(prefix) || prefix.starts_with(start),
        PathMatch::Glob(pattern) => glob_start_alive(pattern, start),
        PathMatch::BasenameSuffix(_) => true,
    }
}

/// Whether `glob_match_simple(pattern, text)` holds for some `text` that
/// begins with `start` and is longer than it: the pattern positions
/// reachable after reading `start`, short of the end, are not empty.
/// Exact for [`match_glob`], whose other two cases are subsets of the
/// whole-path match.
fn glob_start_alive(pattern: &str, start: &str) -> bool {
    let nfa = RuleNfa::glob(pattern);
    nfa.read(&nfa.start(), start.chars().map(PathSym::Char))
        .alive_below()
}

/// The directory a glob's fixed stem names: everything before the
/// segment holding the first wildcard.
fn glob_dir(pattern: &str) -> &str {
    let first_wild = pattern.find(['*', '?']).unwrap_or(pattern.len());
    match pattern[..first_wild].rfind('/') {
        Some(0) | None => "/",
        Some(slash) => &pattern[..slash],
    }
}

/// The error the gate returns for a refused read. Its text names the
/// member, never the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineStateRefusal {
    pub member: MachineStateMember,
}

impl std::fmt::Display for MachineStateRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "machine-state file ({}): contents not captured",
            self.member.as_str()
        )
    }
}

impl std::error::Error for MachineStateRefusal {}

fn refused(member: MachineStateMember) -> io::Error {
    io::Error::other(MachineStateRefusal { member })
}

/// The member a read was refused for, from an error the gate returned.
pub fn refusal(err: &io::Error) -> Option<MachineStateMember> {
    err.get_ref()?
        .downcast_ref::<MachineStateRefusal>()
        .map(|r| r.member)
}

/// The text an inspector records as a degraded reason when the gate
/// refused one of its reads. Fixed text and the path, never content.
pub fn left_out_reason(path: &str) -> String {
    format!("machine-state file left out: {path}")
}

/// The machine-state read gate: the only executor an inspector sees.
///
/// `read_file` screens a file before opening it, by its path, the path
/// it resolves to and its inode; then sniffs a two-byte keytab header;
/// then checks what it read for a whole-file private key. A withheld
/// answer is an error that names the member ([`refusal`]), so no
/// collector that reads through the gate can hold a member's bytes. Every
/// other method delegates: none returns a file's content. The gate does
/// not implement [`HostRead`], so it offers no prefix read.
pub struct ScreenedExecutor<'a> {
    inner: &'a dyn HostRead,
    screen: MachineStateScreen,
}

impl<'a> ScreenedExecutor<'a> {
    /// Wraps `inner`, building the screen from the host (no content read).
    pub fn new(inner: &'a dyn HostRead) -> Self {
        let screen = MachineStateScreen::build(inner);
        Self { inner, screen }
    }

    /// Where the screen could not list every file a withheld rule
    /// matches, if anywhere. The scan checks this before it reads
    /// anything and refuses to run, since a hard link or bind mount of a
    /// file it could not list would not be recognized. A gate with a gap
    /// withholds every path, so no other caller reads past it either.
    pub fn gap(&self) -> Option<&ScreenGap> {
        self.screen.gap.as_ref()
    }

    /// Logical path, resolved path, then inode. Opens nothing. Resolution
    /// and inode add withheld answers only. With a [`gap`](Self::gap),
    /// every path is withheld.
    pub fn screen(&self, path: &str) -> Screened {
        if let Some(gap) = &self.screen.gap {
            return Screened::Withheld(gap.member);
        }
        let by_path = classify_path(path);
        if let Some(RuleTarget::Withheld(member)) = by_path {
            return Screened::Withheld(member);
        }
        if let Ok(resolved) = self.inner.resolve_final_target(Path::new(path))
            && let Some(RuleTarget::Withheld(member)) = classify_path(&resolved.to_string_lossy())
        {
            return Screened::Withheld(member);
        }
        if let Ok(meta) = self.inner.file_metadata(Path::new(path))
            && let Some(member) = self.screen.member_by_inode(&meta)
        {
            return Screened::Withheld(member);
        }
        match by_path {
            Some(RuleTarget::Carriable(member)) => Screened::Carriable(member),
            _ => Screened::Ordinary,
        }
    }

    /// The decision for an unmanaged payload, a file the scan copies into
    /// its tarball byte for byte: [`screen`](Self::screen), then, for a
    /// regular file, the credential detectors over all of its bytes
    /// ([`payload_credential`], one streaming read to the end, in bounded
    /// memory). The unmanaged walk asks it before recording an item, and
    /// the bundler asks it again before copying. An error is a file the
    /// check could not read to its end; it is not a classification, and
    /// the caller decides what an unchecked file means.
    pub fn screen_payload(&self, path: &str) -> io::Result<Screened> {
        let screened = self.screen(path);
        if matches!(screened, Screened::Withheld(_)) {
            return Ok(screened);
        }
        let file = Path::new(path);
        if !self.inner.file_metadata(file)?.is_regular() {
            return Ok(screened);
        }
        let mut reader = self.inner.open_read(file)?;
        Ok(match payload_credential(&mut reader)? {
            Some(member) => Screened::Withheld(member),
            None => screened,
        })
    }

    /// The unscreened executor, for one of the reasons [`ScreenBypass`]
    /// names.
    pub fn bypass(&self, why: ScreenBypass) -> &dyn Executor {
        match why {
            ScreenBypass::SubscriptionSection => self.inner,
        }
    }
}

impl Executor for ScreenedExecutor<'_> {
    fn run(&self, cmd: &str, args: &[&str]) -> ExecResult {
        self.inner.run(cmd, args)
    }

    fn run_passthrough_stderr(&self, cmd: &str, args: &[&str]) -> ExecResult {
        self.inner.run_passthrough_stderr(cmd, args)
    }

    fn run_with_line_callback(
        &self,
        cmd: &str,
        args: &[&str],
        on_stderr_line: &mut dyn FnMut(&str),
    ) -> ExecResult {
        self.inner.run_with_line_callback(cmd, args, on_stderr_line)
    }

    /// Screen, sniff the keytab header, read, check the content. The
    /// header sniff runs only on a regular file, so the gate never opens a
    /// FIFO a caller did not; a failed sniff falls through to the read,
    /// which reports its own error.
    fn read_file(&self, path: &Path) -> io::Result<String> {
        let text = path.to_string_lossy();
        if let Screened::Withheld(member) = self.screen(&text) {
            return Err(refused(member));
        }
        let regular = self
            .inner
            .file_metadata(path)
            .map(|m| m.is_regular())
            .unwrap_or(true);
        if regular
            && let Ok(head) = self.inner.read_head(path, KEYTAB_HEADER_LEN)
            && is_keytab_header(&head)
        {
            return Err(refused(MachineStateMember::KerberosKeytab));
        }
        let content = self.inner.read_file(path)?;
        if let Some(member) = credential_content(content.as_bytes()) {
            drop(content);
            return Err(refused(member));
        }
        Ok(content)
    }

    fn file_exists(&self, path: &Path) -> bool {
        self.inner.file_exists(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<String>> {
        self.inner.read_dir(path)
    }

    fn read_dir_strict(&self, path: &Path) -> io::Result<Vec<String>> {
        self.inner.read_dir_strict(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<String> {
        self.inner.read_link(path)
    }

    fn file_kind(&self, path: &Path) -> io::Result<FileKind> {
        self.inner.file_kind(path)
    }

    fn host_root(&self) -> &Path {
        self.inner.host_root()
    }

    fn resolve_final_target(&self, path: &Path) -> io::Result<PathBuf> {
        self.inner.resolve_final_target(path)
    }

    fn file_metadata(&self, path: &Path) -> io::Result<FileMetadata> {
        self.inner.file_metadata(path)
    }
}

// ---------------------------------------------------------------------------
// Snapshot validation
// ---------------------------------------------------------------------------

use crate::snapshot::{InspectionSnapshot, SnapshotError};
use crate::types::FindingKind;
use std::collections::{BTreeMap, BTreeSet};

/// The top-level section validation leaves alone: `--preserve
/// subscription --ack-sensitive` stores certificates and keys there on
/// purpose.
const UNCHECKED_SECTION: &str = "subscription";

fn violation(path: &str, section: &str, why: &str) -> SnapshotError {
    SnapshotError::MachineState {
        path: path.to_string(),
        section: section.to_string(),
        why: why.to_string(),
    }
}

/// Checks a whole snapshot against the class, whatever any include flag
/// or variant selection says:
///
/// 1. no `config.files` entry sits at a withheld rule's path;
/// 2. a `config.files` entry at a carriable rule's path is a
///    `MachineState` entry for that member, and a `MachineState` entry
///    sits at its member's path;
/// 3. no string anywhere outside `subscription` is a keytab or a
///    whole-file private key ([`credential_content`]);
/// 4. a file row (any object holding a string `path` beside a string
///    `content` or `raw_content`, in any section) is never at a withheld
///    class path, and a carriable class path has exactly one, its own
///    `MachineState` entry; no path is in both `config.files` and
///    `config.withheld`, and none is in `config.withheld` twice;
/// 5. a path in `unmanaged_files.withheld` is in no `items` entry and is
///    listed once.
///
/// Because rules 3 and 4 walk the serialized value, a field or section
/// added later is checked with no list to update.
pub fn validate(snap: &InspectionSnapshot) -> Result<(), SnapshotError> {
    let norm = |p: &str| normal_path(p).unwrap_or_else(|| p.to_string());
    let mut withheld_paths: BTreeSet<String> = BTreeSet::new();
    let mut carriable_paths: BTreeSet<String> = BTreeSet::new();

    if let Some(config) = &snap.config {
        let mut ordinary: BTreeSet<String> = BTreeSet::new();
        for entry in &config.files {
            let path = norm(&entry.path);
            match (classify_path(&path), &entry.disposition) {
                (Some(RuleTarget::Withheld(m)), _) => {
                    return Err(violation(
                        &entry.path,
                        "config.files",
                        &format!("a {} file is never held with content", m.as_str()),
                    ));
                }
                (Some(RuleTarget::Carriable(m)), FindingKind::MachineState { member, .. })
                    if *member == m => {}
                (Some(RuleTarget::Carriable(m)), _) => {
                    return Err(violation(
                        &entry.path,
                        "config.files",
                        &format!("a {} file must be a machine_state entry", m.as_str()),
                    ));
                }
                (None, FindingKind::MachineState { member, .. }) => {
                    return Err(violation(
                        &entry.path,
                        "config.files",
                        &format!("a machine_state {} entry at another path", member.as_str()),
                    ));
                }
                (None, _) => {}
            }
            if entry.disposition.is_machine_state() {
                carriable_paths.insert(path.clone());
            }
            ordinary.insert(path);
        }
        for row in &config.withheld {
            let path = norm(&row.path);
            if ordinary.contains(&path) {
                return Err(violation(
                    &row.path,
                    "config",
                    "the path is both withheld and a config entry",
                ));
            }
            if !withheld_paths.insert(path) {
                return Err(violation(&row.path, "config.withheld", "listed twice"));
            }
        }
    }

    if let Some(unmanaged) = &snap.unmanaged_files {
        let items: BTreeSet<String> = unmanaged.items.iter().map(|i| norm(&i.path)).collect();
        let mut seen = BTreeSet::new();
        for row in &unmanaged.withheld {
            let path = norm(&row.path);
            if items.contains(&path) {
                return Err(violation(
                    &row.path,
                    "unmanaged_files",
                    "the path is both withheld and an item",
                ));
            }
            if !seen.insert(path.clone()) {
                return Err(violation(
                    &row.path,
                    "unmanaged_files.withheld",
                    "listed twice",
                ));
            }
            withheld_paths.insert(path);
        }
    }

    let value = serde_json::to_value(snap)?;
    let mut file_rows: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    if let serde_json::Value::Object(sections) = &value {
        for (section, body) in sections {
            if section == UNCHECKED_SECTION {
                continue;
            }
            walk_value(body, section, None, &mut file_rows)?;
        }
    }

    for (path, rows) in &file_rows {
        let (section, _) = &rows[0];
        if withheld_paths.contains(path)
            || matches!(classify_path(path), Some(RuleTarget::Withheld(_)))
        {
            return Err(violation(
                path,
                section,
                "a file row at a withheld machine-state path",
            ));
        }
        if carriable_paths.contains(path)
            && (rows.len() != 1 || !rows[0].1 || rows[0].0 != "config")
        {
            return Err(violation(
                path,
                section,
                "a machine-state path held by a row other than its own entry",
            ));
        }
    }
    Ok(())
}

/// Rule 3 over every string under `value`, and the file rows rule 4
/// checks: `(section, is a machine_state entry)` by normalized path.
fn walk_value(
    value: &serde_json::Value,
    section: &str,
    near_path: Option<&str>,
    file_rows: &mut BTreeMap<String, Vec<(String, bool)>>,
) -> Result<(), SnapshotError> {
    match value {
        serde_json::Value::String(text) => {
            if let Some(member) = credential_content(text.as_bytes()) {
                return Err(violation(
                    near_path.unwrap_or("(no path)"),
                    section,
                    &format!("holds a credential ({})", member.as_str()),
                ));
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk_value(item, section, near_path, file_rows)?;
            }
        }
        serde_json::Value::Object(fields) => {
            let own_path = fields.get("path").and_then(|p| p.as_str());
            let holds_text = ["content", "raw_content"]
                .iter()
                .any(|k| fields.get(*k).is_some_and(|v| v.is_string()));
            if let (Some(path), true) = (own_path, holds_text) {
                let is_class = fields
                    .get("disposition")
                    .and_then(|d| d.get("kind"))
                    .and_then(|k| k.as_str())
                    == Some("machine_state");
                let key = normal_path(path).unwrap_or_else(|| path.to_string());
                file_rows
                    .entry(key)
                    .or_default()
                    .push((section.to_string(), is_class));
            }
            for field in fields.values() {
                walk_value(field, section, own_path.or(near_path), file_rows)?;
            }
        }
        _ => {}
    }
    Ok(())
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

    /// The inventory descends only where a rule can still match, judged
    /// as `match_glob` judges a whole path.
    #[test]
    fn may_match_below_prunes_only_dead_branches() {
        let cases: &[(PathMatch, &str, bool)] = &[
            (Glob("/etc/krb5.keytab.*"), "/etc/krb5.keytab.d/", true),
            (Glob("/etc/krb5.keytab.*"), "/etc/ssh/", false),
            (Glob("/etc/krb5.keytab.*"), "/etc/krb5.keytab", true),
            (
                Glob("/etc/cockpit/ws-certs.d/*.key"),
                "/etc/cockpit/ws-certs.d/sub/",
                true,
            ),
            (
                Glob("/etc/ssh/ssh_host_*"),
                "/etc/ssh/sshd_config.d/",
                false,
            ),
            (Glob("/etc/ssh/ssh_host_*"), "/etc/ssh/ssh_host_keys/", true),
            (
                Glob("/etc/insights-client/.*"),
                "/etc/insights-client/.state/",
                true,
            ),
            (
                Glob("/etc/insights-client/.*"),
                "/etc/insights-client/tags/",
                false,
            ),
            (
                Prefix("/etc/pki/tls/private/"),
                "/etc/pki/tls/private/x/",
                true,
            ),
            (Prefix("/etc/pki/tls/private/"), "/etc/pki/", true),
            (
                Prefix("/etc/pki/tls/private/"),
                "/etc/pki/tls/certs/",
                false,
            ),
        ];
        for (rule, start, want) in cases {
            assert_eq!(may_match_below(*rule, start), *want, "{rule:?} {start}");
        }
        // Agrees with match_glob on what it lets through.
        assert!(match_glob(
            "/etc/cockpit/ws-certs.d/*.key",
            "/etc/cockpit/ws-certs.d/sub/a.key"
        ));
    }

    #[test]
    fn glob_dir_is_the_fixed_stem() {
        assert_eq!(glob_dir("/etc/ssh/ssh_host_*"), "/etc/ssh");
        assert_eq!(glob_dir("/etc/insights-client/.*"), "/etc/insights-client");
        assert_eq!(
            glob_dir("/etc/cockpit/ws-certs.d/*.key"),
            "/etc/cockpit/ws-certs.d"
        );
    }

    #[test]
    fn refusal_round_trips_through_io_error() {
        let err = refused(M::PrivateKeyFile);
        assert_eq!(refusal(&err), Some(M::PrivateKeyFile));
        assert!(!err.to_string().is_empty());
        assert_eq!(refusal(&io::Error::other("x")), None);
        assert_eq!(refusal(&io::Error::from(io::ErrorKind::NotFound)), None);
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

    /// A reader that yields at most `n` bytes per call, as a pipe or a
    /// short read can.
    struct Chunked<'a> {
        bytes: &'a [u8],
        n: usize,
    }

    impl std::io::Read for Chunked<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let take = self.n.min(buf.len()).min(self.bytes.len());
            buf[..take].copy_from_slice(&self.bytes[..take]);
            self.bytes = &self.bytes[take..];
            Ok(take)
        }
    }

    fn judge(bytes: &[u8], n: usize) -> Option<MachineStateMember> {
        payload_credential(&mut Chunked { bytes, n }).expect("an in-memory read")
    }

    const MIB: usize = 1 << 20;

    /// A synthetic key body: base64 lines, not a real key.
    fn body() -> String {
        "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\n".repeat(4)
    }

    /// Every payload is read to its end: a PEM private-key block anywhere
    /// in it is a key, whether the whole file, embedded in YAML or JSON,
    /// after more than a MiB of text, after binary bytes, or cut off at
    /// the end; a keytab header wins. Certificates, CRLs, large indented
    /// or space-led JSON, binaries, placeholders and quoted markers are
    /// not keys.
    #[test]
    fn payload_credential_reads_the_whole_payload() {
        let key =
            |label: &str| format!("-----BEGIN {label}-----\n{}-----END {label}-----\n", body());
        let yaml = format!(
            "server:\n  port: 443\ntls:\n  private_key: |\n{}  cert: x\n",
            key("PRIVATE KEY")
                .lines()
                .map(|l| format!("    {l}\n"))
                .collect::<String>()
        );
        let json = format!(
            "{{\"type\": \"service_account\", \"private_key\": \"{}\", \"client_email\": \"a@b\"}}",
            key("PRIVATE KEY").replace('\n', "\\n")
        );
        let late = format!(
            "{}{}",
            "setting = value\n".repeat(MIB / 15),
            key("EC PRIVATE KEY")
        );
        let rsa_encrypted = format!(
            "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n\
             DEK-Info: AES-256-CBC,0123456789ABCDEF0123456789ABCDEF\n\n{}\
             -----END RSA PRIVATE KEY-----\n",
            body()
        );
        let mut in_binary = b"\x7fELF\x02\x01\xff\xfe\x00".to_vec();
        in_binary.extend_from_slice(key("OPENSSH PRIVATE KEY").as_bytes());
        in_binary.extend_from_slice(b"\x00\x80\x81");
        let mut keytab_with_key = vec![0x05, 0x02, 0x00];
        keytab_with_key.extend_from_slice(key("PRIVATE KEY").as_bytes());

        let cert = format!(
            "-----BEGIN CERTIFICATE-----\n{}-----END CERTIFICATE-----\n",
            body()
        );
        let big_crl = format!(
            "-----BEGIN X509 CRL-----\n{}-----END X509 CRL-----\n",
            "MIIBpzCBkAIBATANBgkqhkiG9w0BAQsFADA\n".repeat(MIB / 30)
        );
        let big_bundle = cert.repeat(MIB / cert.len() + 1);
        let indented_json = format!(
            "{{\n{}  \"end\": 0\n}}\n",
            "  \"item\": [1, 2, 3],\n".repeat(MIB / 20)
        );
        let spaced_json = format!(" {{\"items\":[{}]}}\n", ["0"; 600_000].join(","));
        let source_regex = "re = /^-----BEGIN OPENSSH PRIVATE KEY-----(?:\\r\\n|\\n)([\\s\\S]+)\
                            (?:\\r\\n|\\n)-----END OPENSSH PRIVATE KEY-----$/;\n";

        let cases: Vec<(&str, Vec<u8>, Option<MachineStateMember>)> = vec![
            (
                "whole-file key",
                key("ENCRYPTED PRIVATE KEY").into(),
                Some(M::PrivateKeyFile),
            ),
            (
                "RSA with headers",
                rsa_encrypted.into(),
                Some(M::PrivateKeyFile),
            ),
            ("YAML block scalar", yaml.into(), Some(M::PrivateKeyFile)),
            ("JSON string", json.into(), Some(M::PrivateKeyFile)),
            ("past the first MiB", late.into(), Some(M::PrivateKeyFile)),
            ("inside a binary", in_binary, Some(M::PrivateKeyFile)),
            (
                "cut off at the end",
                format!("note\n-----BEGIN PRIVATE KEY-----\n{}", body()).into(),
                Some(M::PrivateKeyFile),
            ),
            ("keytab", vec![0x05, 0x01, 0xff], Some(M::KerberosKeytab)),
            (
                "keytab holding PEM text",
                keytab_with_key,
                Some(M::KerberosKeytab),
            ),
            ("certificate", cert.into(), None),
            ("CRL over a MiB", big_crl.into(), None),
            ("certificate bundle over a MiB", big_bundle.into(), None),
            ("indented JSON over a MiB", indented_json.into(), None),
            ("space-led JSON over a MiB", spaced_json.into(), None),
            (
                "binary",
                b"\x7fELF\x02\x01\x01\x00\xff\xfe\x80 ordinary".to_vec(),
                None,
            ),
            (
                "placeholder",
                b"key: |\n  -----BEGIN PRIVATE KEY-----\n  -----END PRIVATE KEY-----\n".to_vec(),
                None,
            ),
            ("marker in a regex", source_regex.into(), None),
            (
                "marker in prose",
                format!(
                    "Paste the -----BEGIN PRIVATE KEY----- block here. {}",
                    body()
                )
                .into(),
                None,
            ),
            ("empty", Vec::new(), None),
        ];
        for (name, bytes, want) in &cases {
            assert_eq!(judge(bytes, PAYLOAD_READ_CHUNK), *want, "{name}");
            assert_eq!(judge(bytes, 7), *want, "{name}, 7-byte reads");
        }
    }

    /// A block split between two reads at any byte, the markers and
    /// labels included, is still one block; a negative control split the
    /// same way stays ordinary.
    #[test]
    fn a_block_split_across_reads_is_found() {
        let key = format!(
            "a: 1\nk: |\n  -----BEGIN OPENSSH PRIVATE KEY-----\n  {}  -----END OPENSSH PRIVATE KEY-----\n",
            body().replace('\n', "\n  ")
        );
        let cert = format!(
            "x = 1\n-----BEGIN CERTIFICATE-----\n{}-----END CERTIFICATE-----\n",
            body()
        );
        for split in 0..=key.len() {
            let (a, b) = key.as_bytes().split_at(split);
            let mut finder = PemKeyFinder::default();
            let found = finder.feed(a) || finder.feed(b) || finder.finish();
            assert!(found, "split at {split}");
        }
        for split in 0..=cert.len() {
            let (a, b) = cert.as_bytes().split_at(split);
            let mut finder = PemKeyFinder::default();
            assert!(
                !(finder.feed(a) || finder.feed(b) || finder.finish()),
                "split at {split}"
            );
        }
        assert_eq!(judge(key.as_bytes(), 1), Some(M::PrivateKeyFile));
    }
}
