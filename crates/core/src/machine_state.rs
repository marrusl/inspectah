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
/// is this buffer, one PEM label of at most [`PEM_LABEL_MAX`] bytes and a
/// few counters, whatever the file's length.
pub const PAYLOAD_READ_CHUNK: usize = 64 * 1024;

/// The longest PEM label [`PemKeyFinder`] reads. RFC 7468's labels, and
/// every private-key label, are far shorter.
const PEM_LABEL_MAX: usize = 64;

/// How many base64 characters of key material make a private key. The
/// smallest private key PEM carries, an Ed25519 or X25519 key in PKCS#8
/// (48 bytes), is 64 base64 characters; every other key is longer. Fewer
/// after a marker is a template, a placeholder or text that quotes the
/// marker.
const KEY_MATERIAL_MIN: usize = 64;

/// The run of dashes both PEM markers start with.
const PEM_DASH_RUN: usize = PEM_DASHES.len();

/// The hex digits of a `\uXXXX` escape.
const UNICODE_ESCAPE_DIGITS: u8 = 4;

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

/// Finds a PEM private key anywhere in a byte stream fed to it a buffer at
/// a time; its state carries across buffers, so a block split between two
/// is still one block.
///
/// Two readers run over every byte. The first looks for a
/// `-----BEGIN <label>-----` marker whose label ends with `PRIVATE KEY`
/// (plain, encrypted, RSA, EC, DSA and OpenSSH keys alike), at any
/// position in a line and whatever came before it, so no earlier text can
/// hide a later marker. Each such marker starts a fresh [`KeyBody`], which
/// reads what follows as a PEM body and counts its key material. A body
/// holding [`KEY_MATERIAL_MIN`] base64 characters is a key, however it
/// ends: at its `END` line, at another marker, at text that is not body,
/// or at the end of the stream, so a cut-off key is a key. A marker
/// followed by anything else, such as a delimiter quoted in JSON, source
/// code, a binary or prose, is not. A certificate, CRL or parameters
/// block is never a candidate.
#[derive(Debug, Default)]
pub struct PemKeyFinder {
    begin: BeginScan,
    body: Option<KeyBody>,
    found: bool,
}

/// Where the `BEGIN` reader is in the stream.
#[derive(Debug)]
enum BeginScan {
    /// How many bytes of [`PEM_BEGIN`] match.
    Seek(usize),
    /// Reading a `BEGIN` marker's label, and the dashes that close it.
    Label(LabelRead),
}

impl Default for BeginScan {
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
    /// Feeds the next bytes of the stream; true once a private key has
    /// been found.
    pub fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut rest = bytes;
        while !self.found && !rest.is_empty() {
            // Outside any body and any marker, skip to the next dash.
            if self.body.is_none() && matches!(self.begin, BeginScan::Seek(0)) {
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

    /// Whether the stream held a private key, the stream having ended:
    /// a body still open at the end is a cut-off key when it holds enough
    /// key material, its last line included.
    pub fn finish(&self) -> bool {
        self.found || self.body.as_ref().is_some_and(KeyBody::is_key_at_end)
    }

    fn step(&mut self, b: u8) {
        if let Some(body) = &mut self.body {
            match body.step(b) {
                BodyStep::More => {}
                BodyStep::Key => {
                    self.found = true;
                    return;
                }
                BodyStep::Ended => self.body = None,
            }
        }
        if self.begin_step(b) {
            self.body = Some(KeyBody::default());
        }
    }

    /// Feeds `b` to the `BEGIN` reader; true when it closed a private-key
    /// marker.
    fn begin_step(&mut self, b: u8) -> bool {
        let begin = PEM_BEGIN.as_bytes();
        let (next, opened) = match std::mem::take(&mut self.begin) {
            BeginScan::Seek(k) => (marker_step(begin, k, b), false),
            BeginScan::Label(mut read) => match read.step(b) {
                LabelStep::More => {
                    self.begin = BeginScan::Label(read);
                    return false;
                }
                // The closing dashes may open the next marker.
                LabelStep::Closed => (PEM_DASH_RUN, read.is_private_key()),
                LabelStep::Invalid => (marker_step(begin, 0, b), false),
            },
        };
        self.begin = if next == begin.len() {
            BeginScan::Label(LabelRead::default())
        } else {
            BeginScan::Seek(next)
        };
        opened
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

/// The text after a private-key `BEGIN` marker, read as a PEM body in
/// constant memory.
///
/// A body is lines. A line ends at a line feed or carriage return, at an
/// escaped one (`\n`, `\r`, `\u000a`), or where one quoted string ends and
/// the next begins, as in a JSON array of lines. Escapes are decoded
/// first (`\n`, `\r`, `\t`, `\f`, `\v`, `\/`, escaped quotes, `\uXXXX`,
/// and runs of backslashes, so a doubly escaped string reads the same),
/// and every byte at or below ASCII space other than a line end is
/// whitespace, as OpenSSL strips it. Each line is:
///
/// - a lead: whitespace, one opening quote, `#` comment marks;
/// - then nothing, base64, or, before any key line, an RFC 1421 header
///   (`Proc-Type: 4,ENCRYPTED`, `DEK-Info: AES-256-CBC,...`);
/// - then whitespace, and after a closing quote one `,`, `+` or `;`, as a
///   string list or concatenation writes it.
///
/// Any other byte, a dash included, ends the body: an `END` line ends it
/// like anything else. A line that is one run of base64 counts its
/// characters as key material; a run of 64 base64 characters counts
/// anywhere, so a key whose lines were joined by spaces or not at all is
/// still a key. A line of several words counts for nothing, and neither
/// does text on the marker's own line short of such a run, so prose and
/// quoted delimiters never add up to a key.
#[derive(Debug)]
struct KeyBody {
    /// Base64 characters on the key lines so far.
    material: usize,
    /// The base64 characters in the current run.
    run: usize,
    /// Still on the `BEGIN` marker's own line, whose text is not a key
    /// line.
    begin_line: bool,
    /// No line with content yet: an RFC 1421 header line may come.
    headers_open: bool,
    /// What the current line adds to `material` when it ends, once its
    /// content is over.
    line_material: usize,
    escape: Escape,
    line: Line,
}

impl Default for KeyBody {
    fn default() -> Self {
        Self {
            material: 0,
            run: 0,
            begin_line: true,
            headers_open: true,
            line_material: 0,
            escape: Escape::None,
            line: Line::Content {
                clean: false,
                space: false,
                header: Header::No,
            },
        }
    }
}

/// Where a [`KeyBody`] is in an escape.
#[derive(Debug, Clone, Copy)]
enum Escape {
    None,
    /// After one or more backslashes.
    Backslash,
    /// After `\u`: how many hex digits have been read, and their value.
    Unicode {
        digits: u8,
        value: u32,
    },
}

/// Where a [`KeyBody`] is in a line.
#[derive(Debug, Clone, Copy)]
enum Line {
    /// Before the line's content. `quoted` once an opening quote was read.
    Lead { quoted: bool },
    /// In the line's content. `clean` while the content is one run of
    /// base64 from the lead on; `space` after whitespace inside it.
    Content {
        clean: bool,
        space: bool,
        header: Header,
    },
    /// After a closing quote. `sep` once a separator was read.
    Trail { sep: bool },
}

/// Whether a line's content may be an RFC 1421 header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Header {
    /// Letters, digits and dashes from the lead on: a header name so far.
    Name,
    /// After the name's colon.
    Value,
    No,
}

/// One body symbol, its escape decoded.
#[derive(Debug, Clone, Copy)]
enum Sym {
    Break,
    Space,
    Quote,
    Byte(u8),
}

/// What one byte did to a [`KeyBody`].
enum BodyStep {
    More,
    /// The body holds a key.
    Key,
    /// A byte no body holds: the body is over and was not a key.
    Ended,
}

impl Sym {
    fn of(b: u8) -> Self {
        match b {
            b'\n' | b'\r' => Self::Break,
            b'"' | b'\'' | b'`' => Self::Quote,
            0..=b' ' => Self::Space,
            _ => Self::Byte(b),
        }
    }
}

impl KeyBody {
    fn step(&mut self, b: u8) -> BodyStep {
        match self.unescape(b) {
            Some(Some(sym)) => self.read(sym),
            Some(None) => BodyStep::More,
            None => BodyStep::Ended,
        }
    }

    /// The symbol `b` completes: `Some(None)` inside an escape, `None`
    /// for an escape no body holds.
    fn unescape(&mut self, b: u8) -> Option<Option<Sym>> {
        let sym = match self.escape {
            Escape::None if b == b'\\' => {
                self.escape = Escape::Backslash;
                return Some(None);
            }
            Escape::None => Sym::of(b),
            Escape::Backslash => {
                self.escape = Escape::None;
                match b {
                    b'\\' => {
                        self.escape = Escape::Backslash;
                        return Some(None);
                    }
                    b'u' => {
                        self.escape = Escape::Unicode {
                            digits: 0,
                            value: 0,
                        };
                        return Some(None);
                    }
                    b'n' | b'r' | b'\n' | b'\r' => Sym::Break,
                    b't' | b'f' | b'v' => Sym::Space,
                    b'/' | b'"' | b'\'' | b'`' => Sym::of(b),
                    _ => return None,
                }
            }
            Escape::Unicode { digits, value } => {
                self.escape = Escape::None;
                let digit = char::from(b).to_digit(16)?;
                let value = value * 16 + digit;
                if digits + 1 < UNICODE_ESCAPE_DIGITS {
                    self.escape = Escape::Unicode {
                        digits: digits + 1,
                        value,
                    };
                    return Some(None);
                }
                match u8::try_from(value) {
                    Ok(v) if v.is_ascii() && v != b'\\' => Sym::of(v),
                    _ => return None,
                }
            }
        };
        Some(Some(sym))
    }

    fn read(&mut self, sym: Sym) -> BodyStep {
        if let Sym::Break = sym {
            return self.end_line(Line::Lead { quoted: false });
        }
        match self.line {
            Line::Lead { quoted } => match sym {
                Sym::Space | Sym::Byte(b'#') => BodyStep::More,
                Sym::Quote if !quoted => {
                    self.line = Line::Lead { quoted: true };
                    BodyStep::More
                }
                // An empty string.
                Sym::Quote => self.close_content(),
                Sym::Byte(b) => {
                    let name = self.headers_open && b.is_ascii_alphanumeric();
                    self.line = Line::Content {
                        clean: true,
                        space: false,
                        header: if name { Header::Name } else { Header::No },
                    };
                    self.content(b)
                }
                Sym::Break => unreachable!("handled above"),
            },
            Line::Content { header, .. } => match sym {
                Sym::Space => {
                    if let Line::Content { space, header, .. } = &mut self.line {
                        *space = true;
                        if *header == Header::Name {
                            *header = Header::No;
                        }
                    }
                    BodyStep::More
                }
                Sym::Quote if header != Header::Value => self.close_content(),
                Sym::Quote => BodyStep::Ended,
                Sym::Byte(b) => self.content(b),
                Sym::Break => unreachable!("handled above"),
            },
            Line::Trail { sep } => match sym {
                Sym::Space => BodyStep::More,
                // The next quoted string: a new line.
                Sym::Quote => self.end_line(Line::Lead { quoted: true }),
                Sym::Byte(b',' | b'+' | b';') if !sep => {
                    self.line = Line::Trail { sep: true };
                    BodyStep::More
                }
                Sym::Byte(_) => BodyStep::Ended,
                Sym::Break => unreachable!("handled above"),
            },
        }
    }

    /// A byte of the current line's content.
    fn content(&mut self, b: u8) -> BodyStep {
        let Line::Content {
            clean,
            space,
            header,
        } = &mut self.line
        else {
            unreachable!("called in a line's content");
        };
        if *header == Header::Value {
            return if b.is_ascii_alphanumeric() || matches!(b, b'-' | b',') {
                BodyStep::More
            } else {
                BodyStep::Ended
            };
        }
        if is_base64_byte(b) {
            if *space {
                // Whitespace inside the line: words, not one run.
                *clean = false;
                *space = false;
                self.run = 0;
            }
            if *header == Header::Name && !b.is_ascii_alphanumeric() {
                *header = Header::No;
            }
            self.run = self.run.saturating_add(1);
            return if self.run >= KEY_MATERIAL_MIN {
                BodyStep::Key
            } else {
                BodyStep::More
            };
        }
        match (b, *header) {
            (b'-', Header::Name) => {
                *clean = false;
                self.run = 0;
                BodyStep::More
            }
            (b':', Header::Name) => {
                *header = Header::Value;
                BodyStep::More
            }
            _ => BodyStep::Ended,
        }
    }

    /// What the current line adds to the key material when it ends.
    fn line_adds(&self) -> usize {
        match self.line {
            Line::Lead { .. } => 0,
            Line::Content { clean, header, .. } => {
                if clean && !self.begin_line && header != Header::Value {
                    self.run
                } else {
                    0
                }
            }
            Line::Trail { .. } => self.line_material,
        }
    }

    /// The line's content is over at a closing quote.
    fn close_content(&mut self) -> BodyStep {
        self.line_material = self.line_adds();
        self.note_content();
        self.line = Line::Trail { sep: false };
        BodyStep::More
    }

    /// A line with content other than a header closes the header block.
    fn note_content(&mut self) {
        if let Line::Content { header, .. } = self.line
            && header != Header::Value
            && !self.begin_line
        {
            self.headers_open = false;
        }
    }

    /// The current line ends; `next` is where the next one starts.
    fn end_line(&mut self, next: Line) -> BodyStep {
        self.material = self.material.saturating_add(self.line_adds());
        self.note_content();
        self.begin_line = false;
        self.run = 0;
        self.line_material = 0;
        self.line = next;
        if self.material >= KEY_MATERIAL_MIN {
            BodyStep::Key
        } else {
            BodyStep::More
        }
    }

    /// Whether the body is a key, the stream ending here: its last line
    /// counts as ended.
    fn is_key_at_end(&self) -> bool {
        self.material.saturating_add(self.line_adds()) >= KEY_MATERIAL_MIN
    }
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
use std::ffi::OsStr;
use std::io;
use std::os::unix::ffi::OsStrExt;
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
    /// An entry's name is not UTF-8 and a rule could match it, or match
    /// below it, so the rule's spelling of it cannot be recorded.
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
    /// rule's own directory, by the raw bytes of each entry's name,
    /// following directory links (each directory once per mount and
    /// matching state, so a link cycle ends; every time where the mount
    /// is unknown) and keeping the logical spelling the rule matches, and
    /// descending only where a match is still possible. Where the listing cannot be completed the build records
    /// a [`ScreenGap`] and stops. `BasenameSuffix` rules are
    /// not enumerated: that would take a walk of `/etc`, and a keytab
    /// linked under another name is caught by its header.
    pub fn build(exec: &dyn HostRead) -> Self {
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
                listed: HashMap::new(),
                budget: &mut budget,
            };
            if let Some(mount) = exec.mount_id(Path::new(root)) {
                walk.listed
                    .insert((meta.dev, meta.ino, mount), vec![state.clone()]);
            }
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

/// Which listing a directory shows: its `(dev, ino)` and the id of the
/// mount it is reached on ([`HostRead::mount_id`]). Inode identity alone
/// is not enough: two bind mounts of one directory share its inode, but a
/// mount below one view need not be below the other, so their entries can
/// differ. A mount shows one set of entries for one directory, so the
/// three together name a listing.
type ListingKey = (u64, u64, u64);

/// One withheld rule's tree, being listed.
struct RuleWalk<'b> {
    rule: PathMatch,
    nfa: RuleNfa,
    member: MachineStateMember,
    /// Each directory listed so far, by [`ListingKey`], with the matching
    /// states it was listed under. A directory reached again on the same
    /// mount under a state one of these covers holds nothing the rule has
    /// not already seen; under any other state it is listed again, since
    /// a link can give the same files a spelling the rule matches (a
    /// `.state` link back to `/etc/insights-client` makes
    /// `registration.json` there `.state/registration.json`). States and
    /// mounts are finite, so link cycles end. A directory whose mount is
    /// unknown is never in here: it is listed every time it is reached,
    /// and a cycle through it ends at [`SCREEN_MAX_DEPTH`] as a gap.
    listed: HashMap<ListingKey, Vec<MatchState>>,
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
        exec: &dyn HostRead,
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
        let names = match exec.read_dir_raw(physical) {
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
        for raw in names {
            *self.budget = self
                .budget
                .checked_sub(1)
                .ok_or_else(|| gap(logical, ScreenGapCause::TooManyEntries))?;
            // A name that is not UTF-8 has no spelling the rule's string
            // can match or the gate can name, so it is decided from its
            // bytes and what the entry is: a gap where the rule matches a
            // regular file's whole path, or could match a path below a
            // directory; else nothing. A UTF-8 name, a literal U+FFFD
            // included, is an ordinary name.
            let Some(name) = raw.to_str() else {
                if self.raw_entry_could_match(exec, physical, &raw, state) {
                    return Err(gap(logical, ScreenGapCause::NameNotUtf8));
                }
                continue;
            };
            let child = format!("{logical}/{name}");
            let child_physical = physical.join(name);
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
            if let Some(mount) = exec.mount_id(&child_physical) {
                let seen = self.listed.entry((meta.dev, meta.ino, mount)).or_default();
                if seen.iter().any(|listed| listed.covers(&below)) {
                    continue;
                }
                seen.push(below.clone());
            }
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

impl RuleWalk<'_> {
    /// Whether the rule could match the entry `raw`, a name that is not
    /// UTF-8 in `physical` read under `state`: a regular file only when
    /// its whole path matches, a directory when some path below it
    /// could. An entry that cannot be statted (a dangling link) names no
    /// file, as for a UTF-8 name; nor does anything else.
    fn raw_entry_could_match(
        &self,
        exec: &dyn HostRead,
        physical: &Path,
        raw: &OsStr,
        state: &MatchState,
    ) -> bool {
        let Ok(meta) = exec.file_metadata(&physical.join(raw)) else {
            return false;
        };
        let syms = raw_name_syms(raw);
        if meta.is_regular() {
            self.nfa.read(state, syms).accepts()
        } else if meta.is_directory() {
            self.nfa
                .read(state, syms.into_iter().chain([PathSym::Char('/')]))
                .alive_below()
        } else {
            false
        }
    }
}

/// One symbol of a path the rule automaton reads.
#[derive(Debug, Clone, Copy)]
enum PathSym {
    Char(char),
    /// A run of bytes in a raw name that is not UTF-8. No pattern
    /// character equals it, since every character is valid UTF-8; a `*`
    /// or `?` matches it.
    NotUtf8,
}

/// A raw directory entry name as automaton symbols: its valid UTF-8 runs
/// character by character, each invalid run as one [`PathSym::NotUtf8`].
fn raw_name_syms(name: &OsStr) -> Vec<PathSym> {
    let mut syms = Vec::new();
    for chunk in name.as_bytes().utf8_chunks() {
        syms.extend(chunk.valid().chars().map(PathSym::Char));
        if !chunk.invalid().is_empty() {
            syms.push(PathSym::NotUtf8);
        }
    }
    syms
}

/// A walked rule's pattern read as a nondeterministic automaton over a
/// path's characters, with `*` matching any run and `?` one character,
/// as [`glob_match_simple`] does. That is exact for [`match_glob`], whose
/// other two cases are subsets of the whole-path match. A `Prefix` rule
/// is its prefix then `*`.
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

    /// Whether the path read so far matches.
    fn accepts(&self) -> bool {
        self.0.last().copied().unwrap_or(false)
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

    fn nfa(rule: PathMatch) -> RuleNfa {
        match rule {
            Glob(pattern) => RuleNfa::glob(pattern),
            Prefix(prefix) => RuleNfa::prefix(prefix),
            other => panic!("only Glob and Prefix rules are walked: {other:?}"),
        }
    }

    fn state(rule: PathMatch, start: &str) -> MatchState {
        let nfa = nfa(rule);
        nfa.read(&nfa.start(), start.chars().map(PathSym::Char))
    }

    /// The inventory descends only where a rule can still match, judged
    /// as `match_glob` judges a whole path.
    #[test]
    fn the_rule_automaton_prunes_only_dead_branches() {
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
            assert_eq!(state(*rule, start).alive_below(), *want, "{rule:?} {start}");
        }
        // Agrees with match_glob on what it lets through.
        assert!(match_glob(
            "/etc/cockpit/ws-certs.d/*.key",
            "/etc/cockpit/ws-certs.d/sub/a.key"
        ));
    }

    /// A directory reached again under a state an earlier listing covers
    /// holds nothing new; one reached under a state no earlier listing
    /// covers, such as `.state/` linked back to its own directory, does.
    #[test]
    fn a_matching_state_covers_only_what_it_matches() {
        let insights = Glob("/etc/insights-client/.*");
        let root = state(insights, "/etc/insights-client/");
        let dot_state = state(insights, "/etc/insights-client/.state/");
        let twice = state(insights, "/etc/insights-client/.state/.state/");
        assert!(!root.covers(&dot_state));
        assert!(!dot_state.covers(&root));
        assert!(dot_state.covers(&twice));
        assert!(root.covers(&root));
    }

    /// A raw name is matched by its bytes: a run that is not UTF-8 equals
    /// no pattern character but a wildcard matches it.
    /// A raw name is read with each invalid run as a symbol only a
    /// wildcard takes. As a regular file it matters only when the rule
    /// matches its whole path; as a directory, when a path below it could
    /// match: a Latin-1 certificate under the `*.key` rule's directory is
    /// no match as a file, but a directory of that name could hold one.
    #[test]
    fn a_raw_name_could_match_only_where_a_wildcard_takes_it() {
        use std::os::unix::ffi::OsStrExt;
        let keytab = Glob("/etc/krb5.keytab.*");
        let private = Prefix("/etc/pki/tls/private/");
        let insights = Glob("/etc/insights-client/.*");
        let cockpit = Glob("/etc/cockpit/ws-certs.d/*.key");
        let certs = "/etc/cockpit/ws-certs.d/";
        // (rule, directory, name, matches as a file, could match below as a directory)
        let cases: &[(PathMatch, &str, &[u8], bool, bool)] = &[
            (keytab, "/etc/", b"\xe9quipe.conf", false, false),
            (keytab, "/etc/", b"krb5.keytab.\xff", true, true),
            (keytab, "/etc/", b"krb5\xff", false, false),
            (private, "/etc/pki/tls/private/", b"k\xff.pem", true, true),
            (
                insights,
                "/etc/insights-client/",
                b"\xffstate",
                false,
                false,
            ),
            (insights, "/etc/insights-client/", b".\xff", true, true),
            (cockpit, certs, b"caf\xe9.crt", false, true),
            (cockpit, certs, b"README\xff.txt", false, true),
            (cockpit, certs, b"caf\xe9.key", true, true),
        ];
        for (rule, dir, name, file, below) in cases {
            let nfa = nfa(*rule);
            let at = state(*rule, dir);
            let syms = raw_name_syms(OsStr::from_bytes(name));
            let read = nfa.read(&at, syms.iter().copied());
            assert_eq!(read.accepts(), *file, "{rule:?} {dir} {name:?} as a file");
            let under = nfa.read(&at, syms.into_iter().chain([PathSym::Char('/')]));
            assert_eq!(
                under.alive_below(),
                *below,
                "{rule:?} {dir} {name:?} as a directory"
            );
        }
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

    /// Deterministic base64 text of `chars` characters, ending `=` padded
    /// like a real body. Not a real key.
    fn key_material(chars: usize) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text: String = (0..chars)
            .map(|i| char::from(ALPHABET[(i * 7 + i / 3) % ALPHABET.len()]))
            .collect();
        text.replace_range(chars - 2.., "==");
        text
    }

    /// `material` wrapped `width` characters a line, each line led by
    /// `lead`, ended by `trail` and `eol`, between private-key markers
    /// labelled `label`.
    fn wrapped_key(
        label: &str,
        material: &str,
        width: usize,
        lead: &str,
        trail: &str,
        eol: &str,
    ) -> String {
        let mut text = format!("{lead}-----BEGIN {label}-----{eol}");
        for line in material.as_bytes().chunks(width) {
            text.push_str(lead);
            text.push_str(std::str::from_utf8(line).expect("ASCII"));
            text.push_str(trail);
            text.push_str(eol);
        }
        text.push_str(&format!("{lead}-----END {label}-----{eol}"));
        text
    }

    /// The PEM structure decides, whatever surrounds the block: a key is a
    /// private-key `BEGIN` marker and at least 64 characters of base64
    /// lines, in any whitespace OpenSSL reads, wrapping, escaped or
    /// quoted line form, however the block ends; a marker with no key
    /// lines after it is text, however it is quoted and whatever an
    /// earlier marker left open.
    #[test]
    fn the_verdict_follows_the_pem_structure() {
        let material = key_material(1_600);
        let one_line = format!("-----BEGIN PRIVATE KEY-----\n{}\n", key_material(64));
        let mut ff_vt = wrapped_key("PRIVATE KEY", &material, 64, "", "", "\n");
        ff_vt = ff_vt
            .replacen('\n', "\x0c\n", 2)
            .replacen("==\n", "==\x0b\n", 1);
        let key_json = wrapped_key("PRIVATE KEY", &material, 64, "", "", "\n").replace('\n', "\\n");
        let decoy = |padding: usize| {
            format!(
                "{{\"header\":\"-----BEGIN PRIVATE KEY-----\",\"padding\":\"{}\",\"key\":\"{key_json}\"}}",
                "A".repeat(padding)
            )
        };
        let python = format!(
            "KEY = (\n{}\n)\n",
            wrapped_key("PRIVATE KEY", &material, 64, "", "", "\n")
                .lines()
                .map(|l| format!("    \"{l}\\n\""))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let array = format!(
            "[{}]",
            wrapped_key("PRIVATE KEY", &material, 64, "", "", "\n")
                .lines()
                .map(|l| format!("\"{l}\""))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let dotnet = key_json.replace('+', "\\u002B").replace('/', "\\/");
        let commented = wrapped_key("RSA PRIVATE KEY", &material, 64, "# ", "", "\n");
        let space_joined = format!(
            "KEY=\"-----BEGIN PRIVATE KEY----- {} -----END PRIVATE KEY-----\"\n",
            material
                .as_bytes()
                .chunks(64)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect::<Vec<_>>()
                .join(" ")
        );

        let keys: Vec<(&str, String)> = vec![
            ("form feed and vertical tab before line ends", ff_vt),
            (
                "one character a line, 64 trailing spaces",
                wrapped_key("RSA PRIVATE KEY", &material, 1, "", &" ".repeat(64), "\n"),
            ),
            (
                "trailing control bytes OpenSSL strips",
                wrapped_key("EC PRIVATE KEY", &material, 64, "", "\x00\x01\x1f ", "\r\n"),
            ),
            ("the smallest key, one line", one_line),
            ("JSON string", format!("{{\"k\": \"{key_json}\"}}")),
            (
                "JSON with \\u and \\/ escapes",
                format!("{{\"k\": \"{dotnet}\"}}"),
            ),
            ("Python string per line", python),
            ("array of lines", array),
            ("commented out", commented),
            ("lines joined by spaces", space_joined),
            (
                "cut off by another marker",
                format!(
                    "-----BEGIN PRIVATE KEY-----\n{}\n-----BEGIN CERTIFICATE-----\nMII=\n",
                    key_material(128)
                ),
            ),
        ];
        let mut padded: Vec<(String, String)> = Vec::new();
        for padding in (65_480..=65_560).step_by(4).chain([0, 1, 131_000]) {
            padded.push((format!("decoy padding {padding}"), decoy(padding)));
        }
        let mut wrong = Vec::new();
        for (name, text) in keys
            .iter()
            .map(|(n, t)| (n.to_string(), t.clone()))
            .chain(padded)
        {
            for read in [PAYLOAD_READ_CHUNK, 13] {
                if judge(text.as_bytes(), read) != Some(M::PrivateKeyFile) {
                    wrong.push(format!("{name}, {read}-byte reads: not a key"));
                }
            }
        }

        let mut binary =
            b"\x7fELF\x02\x01\x00\x00-----BEGIN PRIVATE KEY-----\x00-----END PRIVATE KEY-----\x00"
                .to_vec();
        binary.extend_from_slice(b"\xff\xfe-----BEGIN PRIVATE KEY-----\n\x00\x02\x7fELF\x00");
        let text: Vec<(&str, Vec<u8>)> = vec![
            (
                "delimiter-only JSON",
                br#"{"begin":"-----BEGIN PRIVATE KEY-----","end":"-----END PRIVATE KEY-----"}"#.to_vec(),
            ),
            (
                "minified JS variables",
                br#"var a="-----BEGIN PRIVATE KEY-----",b="-----END PRIVATE KEY-----";"#.to_vec(),
            ),
            (
                "minified JS object",
                br#"e.exports={begin:"-----BEGIN PRIVATE KEY-----",end:"-----END PRIVATE KEY-----"};"#.to_vec(),
            ),
            (
                "minified JS wrapping a variable",
                br#"return"-----BEGIN PRIVATE KEY-----\n"+t+"\n-----END PRIVATE KEY-----\n"}"#.to_vec(),
            ),
            (
                "JS array of markers",
                br#"["-----BEGIN PRIVATE KEY-----","-----END PRIVATE KEY-----"]"#.to_vec(),
            ),
            ("binary resource", binary),
            (
                "comment at the end of a script",
                b"#!/bin/sh\nopenssl pkey -in \"$1\"\n# \"-----BEGIN PRIVATE KEY-----\" is a delimiter".to_vec(),
            ),
            (
                "unquoted comment at the end",
                b"# -----BEGIN PRIVATE KEY----- is a delimiter\n".to_vec(),
            ),
            (
                "prose between markers",
                b"# -----BEGIN PRIVATE KEY-----\n# The key goes between these markers\n\
                  # and must be base64 encoded, one line after another\n# -----END PRIVATE KEY-----\n"
                    .to_vec(),
            ),
            (
                "shell template",
                b"cat <<EOF\n-----BEGIN PRIVATE KEY-----\n${KEY}\n-----END PRIVATE KEY-----\nEOF\n".to_vec(),
            ),
            (
                "decoy padding then text",
                format!(
                    "{{\"header\":\"-----BEGIN PRIVATE KEY-----\",\"padding\":\"{}\"}}\n{}",
                    "A".repeat(65_500),
                    "B".repeat(100)
                )
                .into_bytes(),
            ),
        ];
        for (name, bytes) in &text {
            for read in [PAYLOAD_READ_CHUNK, 13] {
                if judge(bytes, read).is_some() {
                    wrong.push(format!("{name}, {read}-byte reads: a key"));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    mod pem_properties {
        use super::super::*;
        use proptest::prelude::*;

        const LABELS: &[&str] = &[
            "PRIVATE KEY",
            "ENCRYPTED PRIVATE KEY",
            "RSA PRIVATE KEY",
            "EC PRIVATE KEY",
            "DSA PRIVATE KEY",
            "OPENSSH PRIVATE KEY",
        ];

        fn base64(bytes: &[u8]) -> String {
            const ALPHABET: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in bytes.chunks(3) {
                let n = chunk
                    .iter()
                    .enumerate()
                    .fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
                for i in 0..4 {
                    if i <= chunk.len() {
                        out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
                    } else {
                        out.push('=');
                    }
                }
            }
            out
        }

        /// How a generated key's lines are written.
        #[derive(Debug, Clone, Copy)]
        enum Form {
            /// Real line ends, LF or CRLF.
            Plain,
            /// A JSON string: `\n` escapes.
            Json,
            /// One quoted string per line, as Python or C concatenates them.
            Quoted,
        }

        fn whitespace() -> impl Strategy<Value = String> {
            proptest::collection::vec(
                prop::sample::select(vec![" ", "\t", "\x0b", "\x0c", "\x00"]),
                0..8,
            )
            .prop_map(|w| w.concat())
        }

        fn text_without_markers() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9 .,;:!?(){}<>\\n]{0,200}"
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(256))]

            /// A key of any size, wrapped at any width, with any leading
            /// and trailing whitespace OpenSSL accepts, in any line form,
            /// with or without its `END` line, anywhere in a file, read in
            /// any buffer size, is a key.
            #[test]
            fn a_wrapped_key_is_a_key(
                key in proptest::collection::vec(any::<u8>(), 48..1_200),
                label in prop::sample::select(LABELS),
                width in 1usize..=80,
                lead in "[ \t]{0,6}",
                trail in whitespace(),
                crlf in any::<bool>(),
                form in prop::sample::select(vec![Form::Plain, Form::Json, Form::Quoted]),
                end in any::<bool>(),
                prefix in text_without_markers(),
                suffix in text_without_markers(),
                read in 1usize..=4_096,
            ) {
                let material = base64(&key);
                let mut lines = vec![format!("-----BEGIN {label}-----")];
                for chunk in material.as_bytes().chunks(width) {
                    lines.push(std::str::from_utf8(chunk).expect("base64").to_string());
                }
                if end {
                    lines.push(format!("-----END {label}-----"));
                }
                let eol = if crlf { "\r\n" } else { "\n" };
                let block = match form {
                    Form::Plain => lines
                        .iter()
                        .map(|l| format!("{lead}{l}{trail}{eol}"))
                        .collect::<String>(),
                    Form::Json => format!(
                        "{{\"key\": \"{}\"}}",
                        lines.iter().map(|l| format!("{l}{}\\n", " ".repeat(trail.len()))).collect::<String>()
                    ),
                    Form::Quoted => lines
                        .iter()
                        .map(|l| format!("{lead}\"{l}\\n\"{eol}"))
                        .collect::<String>(),
                };
                let text = format!("{prefix}\n{block}{suffix}");
                let mut finder = PemKeyFinder::default();
                let found = text.as_bytes().chunks(read).any(|c| finder.feed(c)) || finder.finish();
                prop_assert!(found, "{text:?}");
            }

            /// Text holding private-key markers but no base64 line between
            /// them, on one line or many, is not a key.
            #[test]
            fn markers_around_text_are_not_a_key(
                label in prop::sample::select(LABELS),
                words in proptest::collection::vec(
                    ("[a-zA-Z0-9]{1,12}", prop::sample::select(vec![" ", ". ", ", ", "! ", "(", ")", "<", ">", "@", "&", "*", "%", "$", ":", ";", "_", "~", "|", "[", "]", "{", "}"])),
                    0..40,
                ),
                lines in proptest::collection::vec(
                    ("[ \t]{0,4}", prop::sample::select(vec!["", "# "]), "[a-zA-Z0-9]{1,12}", prop::sample::select(vec![" ", ".", "!", "(", "<", "@", "&", "*", "%", "_", "~", "|", ":", ","]), "[a-zA-Z0-9 ]{1,40}"),
                    0..20,
                ),
                many in any::<bool>(),
                end in any::<bool>(),
                prefix in text_without_markers(),
                suffix in text_without_markers(),
                read in 1usize..=4_096,
            ) {
                let filler = if many {
                    lines
                        .iter()
                        .map(|(lead, hash, first, sep, rest)| format!("\n{lead}{hash}{first}{sep}{rest}"))
                        .collect::<String>()
                } else {
                    words.iter().map(|(w, sep)| format!("{w}{sep}")).collect::<String>()
                };
                let end = if end { format!("-----END {label}-----") } else { String::new() };
                let text = format!("{prefix}-----BEGIN {label}-----{filler}\n{end}{suffix}");
                let mut finder = PemKeyFinder::default();
                let found = text.as_bytes().chunks(read).any(|c| finder.feed(c)) || finder.finish();
                prop_assert!(!found, "{text:?}");
            }
        }
    }
}
