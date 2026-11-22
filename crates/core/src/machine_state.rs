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

/// How many bytes the keytab header sniff reads. With the unmanaged
/// payload check ([`ScreenedExecutor::screen_payload`]), the only prefix
/// reads the scan makes.
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
    matches!(scan_key_file(content), KeyFileScan::KeyFile { .. })
}

/// What [`scan_key_file`] found in the lines it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyFileScan {
    /// A line no key file holds: the text is configuration.
    Other,
    /// Only key-file lines, and at least one private-key block.
    KeyFile { open_block: bool },
    /// Only key-file lines, and no private-key block yet.
    NoKey { open_block: bool },
}

/// The line-by-line test behind [`is_whole_file_private_key`], also
/// telling whether the text ended inside a PEM block, for a caller that
/// holds only the start of a file ([`payload_credential`]).
fn scan_key_file(content: &str) -> KeyFileScan {
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
                return KeyFileScan::Other;
            }
            open_label = Some(label);
            continue;
        }
        if !is_envelope_line(line) {
            return KeyFileScan::Other;
        }
    }
    let open_block = open_label.is_some();
    if has_key {
        KeyFileScan::KeyFile { open_block }
    } else {
        KeyFileScan::NoKey { open_block }
    }
}

/// A line a key file may hold outside a PEM block: blank, a `#` comment,
/// indented `openssl` attribute text, or an envelope line.
fn is_envelope_line(line: &str) -> bool {
    line.trim().is_empty()
        || line.starts_with('#')
        || line.starts_with([' ', '\t'])
        || ENVELOPE_PREFIXES.iter().any(|p| line.starts_with(p))
}

/// How many bytes of an unmanaged payload [`payload_credential`] judges.
/// A whole-file key is a few kilobytes; a file longer than this is judged
/// by its start.
pub const PAYLOAD_CHECK_LEN: usize = 1 << 20;

/// The credential member a payload is by its bytes, binary-safe: `head`
/// is the whole file when `complete`, else its first
/// [`PAYLOAD_CHECK_LEN`] bytes or more. A keytab header is a keytab in
/// either case. A complete file is judged by [`credential_content`], so
/// bytes that are not UTF-8 are never a PEM key. A longer file is a
/// private key unless its start already rules it out: bytes that are not
/// UTF-8, or a line, the cut-off last one included, that no key file
/// holds. So a large binary or a one-line minified file stays ordinary,
/// and a key file padded past the limit is still refused.
pub fn payload_credential(head: &[u8], complete: bool) -> Option<MachineStateMember> {
    if complete || is_keytab_header(head) {
        return credential_content(head);
    }
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        // The cut fell inside a character: judge the bytes before it.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()])
            .expect("valid_up_to bounds the valid UTF-8 prefix"),
        Err(_) => return None,
    };
    let (lines, cut) = match text.rfind('\n') {
        Some(end) => (&text[..=end], &text[end + 1..]),
        None => ("", text),
    };
    let open_block = match scan_key_file(lines) {
        KeyFileScan::Other => return None,
        KeyFileScan::KeyFile { open_block } | KeyFileScan::NoKey { open_block } => open_block,
    };
    let cut_may_belong = open_block
        || is_envelope_line(cut)
        || cut.starts_with('-')
        || ENVELOPE_PREFIXES.iter().any(|p| p.starts_with(cut));
    cut_may_belong.then_some(MachineStateMember::PrivateKeyFile)
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

/// How deep the screen walks below a `Prefix` rule's directory.
const SCREEN_WALK_DEPTH: usize = 8;

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
}

impl MachineStateScreen {
    /// Stats every host path a withheld rule matches, listing only those
    /// rules' own directories. Reads no file content. `BasenameSuffix`
    /// rules are not enumerated: that would take a walk of `/etc`, and a
    /// keytab linked under another name is caught by its header.
    pub fn build(exec: &dyn Executor) -> Self {
        let mut screen = Self::default();
        for (rule, target) in RULES {
            let RuleTarget::Withheld(member) = *target else {
                continue;
            };
            match *rule {
                PathMatch::Exact(path) => screen.note(exec, path, member),
                PathMatch::Glob(pattern) => {
                    let dir = glob_dir(pattern);
                    screen.walk(exec, dir, member, SCREEN_WALK_DEPTH, &|p| {
                        match_glob(pattern, p)
                    });
                }
                PathMatch::Prefix(prefix) => {
                    let dir = prefix.trim_end_matches('/');
                    screen.walk(exec, dir, member, SCREEN_WALK_DEPTH, &|p| {
                        p.starts_with(prefix)
                    });
                }
                PathMatch::BasenameSuffix(_) => {}
            }
        }
        screen
    }

    fn note(&mut self, exec: &dyn Executor, path: &str, member: MachineStateMember) {
        if let Ok(meta) = exec.file_metadata(Path::new(path))
            && meta.is_regular()
        {
            self.inodes.entry((meta.dev, meta.ino)).or_insert(member);
        }
    }

    fn walk(
        &mut self,
        exec: &dyn Executor,
        dir: &str,
        member: MachineStateMember,
        depth: usize,
        wanted: &dyn Fn(&str) -> bool,
    ) {
        let Ok(names) = exec.read_dir(Path::new(dir)) else {
            return;
        };
        for name in names {
            let path = format!("{dir}/{name}");
            if wanted(&path) {
                self.note(exec, &path, member);
            }
            if depth > 0 && matches!(exec.file_kind(Path::new(&path)), Ok(FileKind::Directory)) {
                self.walk(exec, &path, member, depth - 1, wanted);
            }
        }
    }

    /// The withheld member whose file has this inode.
    pub fn member_by_inode(&self, meta: &FileMetadata) -> Option<MachineStateMember> {
        self.inodes.get(&(meta.dev, meta.ino)).copied()
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

    /// Logical path, resolved path, then inode. Opens nothing. Resolution
    /// and inode add withheld answers only.
    pub fn screen(&self, path: &str) -> Screened {
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
    /// regular file, the credential detectors over its bytes
    /// ([`payload_credential`], one read of at most
    /// [`PAYLOAD_CHECK_LEN`] + 1 bytes). The unmanaged walk asks it before
    /// recording an item, and the bundler asks it again before copying.
    /// An error is a file the check could not read; it is not a
    /// classification, and the caller decides what an unchecked file
    /// means.
    pub fn screen_payload(&self, path: &str) -> io::Result<Screened> {
        let screened = self.screen(path);
        if matches!(screened, Screened::Withheld(_)) {
            return Ok(screened);
        }
        let file = Path::new(path);
        if !self.inner.file_metadata(file)?.is_regular() {
            return Ok(screened);
        }
        let head = self.inner.read_head(file, PAYLOAD_CHECK_LEN + 1)?;
        let complete = head.len() <= PAYLOAD_CHECK_LEN;
        Ok(match payload_credential(&head, complete) {
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

    /// A payload's start decides a long file: a key file padded past the
    /// limit and a keytab are refused; a binary, a one-line minified file
    /// and a configuration file are not. A complete payload is judged as
    /// `credential_content` judges it, and bytes that are not UTF-8 are
    /// never a PEM key.
    #[test]
    fn payload_credential_judges_a_long_file_by_its_start() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3Bl\n-----END OPENSSH PRIVATE KEY-----\n";
        let padded = format!("{key}{}", "# padding\n".repeat(PAYLOAD_CHECK_LEN / 8));
        let head = &padded.as_bytes()[..=PAYLOAD_CHECK_LEN];
        let cut_in_block = format!("-----BEGIN PRIVATE KEY-----\n{}", "QUJD\n".repeat(1 << 19));
        let cases: &[(&str, &[u8], bool, Option<MachineStateMember>)] = &[
            (
                "complete key",
                key.as_bytes(),
                true,
                Some(M::PrivateKeyFile),
            ),
            ("padded key", head, false, Some(M::PrivateKeyFile)),
            (
                "cut inside a key block",
                &cut_in_block.as_bytes()[..=PAYLOAD_CHECK_LEN],
                false,
                Some(M::PrivateKeyFile),
            ),
            (
                "keytab",
                &[0x05, 0x01, 0xff],
                false,
                Some(M::KerberosKeytab),
            ),
            (
                "complete keytab",
                &[0x05, 0x02],
                true,
                Some(M::KerberosKeytab),
            ),
            ("complete binary", b"\x7fELF\x02\xff\xfe", true, None),
            ("long binary", b"\x7fELF\x02\xff\xfe\x00", false, None),
            ("minified, one line", b"{\"a\":1,\"b\":[1,2,3", false, None),
            ("configuration", b"listen = 443\nworkers = 4\n", false, None),
            (
                "certificate",
                b"-----BEGIN CERTIFICATE-----\nMII=\n-----END CERTIFICATE-----\nkey = x",
                false,
                None,
            ),
        ];
        for (name, bytes, complete, want) in cases {
            assert_eq!(payload_credential(bytes, *complete), *want, "{name}");
        }
        // A cut inside a multi-byte character is judged by the bytes before it.
        let mut split = key.as_bytes().to_vec();
        split.extend_from_slice("# é".as_bytes());
        split.pop();
        assert_eq!(payload_credential(&split, false), Some(M::PrivateKeyFile));
    }
}
