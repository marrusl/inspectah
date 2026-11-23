use inspectah_core::machine_state::CLOSED_KEY_MATERIAL_MIN;
use inspectah_core::redaction::{
    is_compose_file_name, scrub_compose_secrets, scrub_repo_credentials,
};
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionHint, RedactionKind,
    RedactionState,
};
use inspectah_core::types::sysctl::{SysctlFileBody, SysctlSection};
use regex::{Captures, Regex};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use crate::redaction::patterns::{
    GENERIC_CREDENTIAL_PATTERN, PATTERNS, SECRET_GROUP, SecretPattern, is_credential_name,
    scan_shadow, value_runs_to_line_end,
};
use crate::render::language_packages::scrub_manifest_auth;
use crate::render::safety::{KernelArg, kernel_args};
use inspectah_core::types::destination::Destination;
use inspectah_core::types::repo::{SectionId, is_repo_id, parse_repo_sections};

/// Paths that are known false-positive sources for credential scanning.
/// Files under these prefixes use keywords like "password", "auth", and
/// "credential" as PAM module type tokens — not actual secrets.
/// Listed WITHOUT a leading slash — the check handles both absolute
/// (`/etc/pam.d/...`) and relative (`etc/pam.d/...`) forms.
const REDACTION_ALLOWLIST: &[&str] = &["etc/pam.d/"];

/// What `render::language_packages::scrub_url_auth` leaves in place of an
/// authority-embedded credential. Used to tell a real scrub apart from the
/// incidental reformatting the manifest scrubbers also do.
const URL_AUTH_REDACTED_MARKER: &str = "://REDACTED@";

/// What `inspectah_core::redaction::scrub_compose_secrets` leaves in place
/// of a secret value. Same job as `URL_AUTH_REDACTED_MARKER`: that scrubber
/// also rewrites content it did not scrub, so the marker is the signal.
const COMPOSE_REDACTED_MARKER: &str = "<REDACTED>";

/// Where dnf reads `.repo` files from.
const YUM_REPOS_DIR: &str = "/etc/yum.repos.d/";

/// Whether `path` is a `.repo` file dnf reads: a `*.repo` directly in
/// `/etc/yum.repos.d/`, in any spelling [`Destination`] reads as that, so
/// a repo inventory entry and its Configuration row (`etc/yum.repos.d/x.repo`
/// on an ostree host) are both one.
fn is_repo_file_path(path: &str) -> bool {
    Destination::new(path).is_ok_and(|d| {
        d.as_str()
            .strip_prefix(YUM_REPOS_DIR)
            .is_some_and(|name| !name.contains('/') && name.ends_with(".repo"))
    })
}

/// `text`, a `.repo` file or its Configuration row, with the userinfo of
/// every URL in it replaced by `REDACTED` ([`scrub_repo_credentials`]):
/// `https://user:pass@host/` and `https://token@host/` both become
/// `https://REDACTED@host/`, a placeholder the operator fills in with the
/// credential at build time. A userinfo runs to the last `@` before the
/// first `/` or `?` of its option value, through commas, `#`, `;`,
/// whitespace and continuation lines, whichever option holds the URL: the
/// same rule the collector names key references by, so no piece of a
/// password a list split or comment cut is left in either. `None` when no
/// URL held a credential.
///
/// `PATTERNS` has no rule for a password in an arbitrary URL's authority,
/// so without this an ordinary redacted scan kept a repo's password in
/// both snapshot copies and in the staged `.repo` file.
fn scrub_repo_urls(text: &str, path: &str) -> Option<(String, RedactionFinding)> {
    let Cow::Owned(scrubbed) = scrub_repo_credentials(text) else {
        return None;
    };
    let finding = RedactionFinding {
        path: path.to_string(),
        source: "repo_url_credential".into(),
        kind: RedactionKind::Inline,
        pattern: "url_credential".into(),
        remediation: "Remove the credential from the repo URL; supply it to the build \
                      as a secret instead of baking it into the image"
            .to_string(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: Some(Confidence::High),
        finding_kind: Some(FindingKind::Password),
    };
    Some((scrubbed, finding))
}

/// The sections `after`, a `.repo` file as redaction left it, still
/// defines whose enabled state or source differs from what `before`, the
/// file as collected, defined, or that `before` did not define: the ones
/// whose stored definition is not the host's
/// ([`RepoFile::redaction_changed`]). A section `after` no longer defines
/// is not named: its id may be what a pattern replaced.
///
/// [`RepoFile::redaction_changed`]: inspectah_core::types::rpm::RepoFile::redaction_changed
fn sections_redaction_changed(before: &str, after: &str) -> BTreeSet<SectionId> {
    if before == after {
        return BTreeSet::new();
    }
    let host: HashMap<SectionId, (bool, bool)> = parse_repo_sections(before)
        .into_iter()
        .map(|sec| (sec.id, (sec.enabled, sec.has_source)))
        .collect();
    parse_repo_sections(after)
        .into_iter()
        .filter(|sec| host.get(&sec.id) != Some(&(sec.enabled, sec.has_source)))
        .map(|sec| sec.id)
        .collect()
}

/// Returns true if `path` matches a known false-positive prefix.
/// Handles both absolute and relative path forms.
fn is_allowlisted_path(path: &str) -> bool {
    let normalized = path.strip_prefix('/').unwrap_or(path);
    REDACTION_ALLOWLIST
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
}

/// Compiled regex for proxy credential masking.
/// Matches `://user:password@` in proxy URLs, capturing three groups:
///   1. `://user:` (scheme + username + colon)
///   2. the password segment
///   3. `@`
///
/// The password runs to the last `@` of the authority, which ends at the
/// first `/`, `?`, `#` or whitespace, as `scrub_url_auth` reads it: a
/// password may hold an `@` (`deploy:p@s3cret@host`), and stopping at the
/// first one shipped the tail. Bounding it by the authority keeps an `@`
/// in the path or query from taking the host. Only when the authority
/// holds no `@` does the second branch take the first `@` on the line, as
/// this pattern always did, so a raw `/`, `?` or `#` in a password
/// (`u:pa/ss@host`) stays masked.
///
/// The user name may be empty (`https://:pw@host`) and may hold a raw `?`
/// or `#` (`https://u?name:pw@host`): such a URL is malformed, but its
/// password is still a password, and excluding those characters from the
/// user name once left it unmasked.
static PROXY_CRED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(://[^:/@\s]*:)([^/?#\s]+|[^@\s]+)(@)").expect("proxy regex"));

/// Compiled regex for bare `proxy_password=VALUE` lines in DNF/Yum configs.
/// Spacing around `=` is `[ \t]*`, not `\s*`, so an empty value never takes
/// the next line as the password (see the generic credential patterns).
/// A quoted value is taken whole, quotes included; any other runs to the
/// next whitespace, which is all this masker can know of a key inside
/// another line ([`PROXY_PASSWORD_LINE_RE`] reads one that opens its
/// line). Captures two groups:
///   1. `proxy_password=` (the key prefix)
///   2. the password value
static PROXY_PASSWORD_KV_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(proxy_password[ \t]*=[ \t]*)("(?:[^"\\\r\n]|\\[^\r\n]?)+"?\S*|'[^'\r\n]+'?\S*|\S+)"#,
    )
    .expect("proxy_password key-value regex")
});

/// Compiled regex for a `proxy_password=` that opens its line, as dnf's
/// ini reader reads one: the value is the rest of the line, spaces
/// included, to its last non-blank character. `\S+` alone shipped
/// `bravo` from `proxy_password=alpha bravo`. Applied only in a file
/// [`value_runs_to_line_end`] names. Captures the same two groups as
/// [`PROXY_PASSWORD_KV_RE`].
static PROXY_PASSWORD_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)^[ \t]*(proxy_password[ \t]*=[ \t]*)([^\r\n]*[^ \t\r\n])")
        .expect("proxy_password line regex")
});

/// Compiled regex for username-only token remotes: `://token@host`.
/// Matches URLs where the username looks like a token (known prefixes or
/// long alphanumeric strings) with no colon/password separator.
/// Captures three groups:
///   1. `://` (scheme separator)
///   2. the token value (username portion), running on through any later
///      `@` of the authority, as [`PROXY_CRED_RE`]'s password does
///   3. `@` (delimiter before host)
static TOKEN_USERNAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(://)({TOKEN_USER_NAME}(?:@[^/?#@\s]*)*)(@)"))
        .expect("token username regex")
});

/// A user name that looks like a token: a known token prefix, or a long
/// run of letters, digits, `_` and `-`.
const TOKEN_USER_NAME: &str = r"(?:ghp_[A-Za-z0-9_]{36,}|gho_[A-Za-z0-9_]{36,}|glpat-[A-Za-z0-9_\-]{20,}|github_pat_[A-Za-z0-9_]{22,}|[A-Za-z0-9_\-]{20,})";

/// Compiled regex for a token user name with a password after it:
/// `://token:x-oauth-basic@host`, or an empty one, `://token:@host`.
/// [`PROXY_CRED_RE`] masks the password; the token is a credential too, so
/// this masks it, as capture group 2. The password part reads to the
/// authority's last `@`, or to the line's first, as [`PROXY_CRED_RE`]'s
/// does, so a long host name before a port (`://long-host-name:8081/`)
/// with no `@` after it is not taken for a token.
static TOKEN_WITH_PASSWORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(://)({TOKEN_USER_NAME})(:(?:[^/?#\s]*|[^@\s]*)@)"
    ))
    .expect("token with password regex")
});

/// A value that is one URL and nothing else: a scheme, `://` and no
/// whitespace.
static URL_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.\-]*://\S*$").expect("URL value regex"));

/// Options controlling redaction sensitivity.
#[derive(Debug, Clone)]
pub struct RedactOptions {
    pub sensitivity: Sensitivity,
}

impl Default for RedactOptions {
    fn default() -> Self {
        Self {
            sensitivity: Sensitivity::Default,
        }
    }
}

/// Sensitivity presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// Standard sensitivity — all patterns active.
    Default,
    /// Strict — future: lower confidence thresholds, more aggressive matching.
    Strict,
}

/// Counter registry for deterministic redaction tokens.
/// Same secret value always maps to the same `REDACTED_TYPE_N` token.
#[derive(Debug, Default)]
struct CounterRegistry {
    /// Maps (finding_kind_label, secret_value) -> counter
    seen: HashMap<(String, String), u32>,
    /// Per-type counters
    type_counters: HashMap<String, u32>,
    /// Every token this registry has handed out. A value equal to one of
    /// these was put there by this run; a value merely spelled like one
    /// may be the host's own.
    issued: HashSet<String>,
}

impl CounterRegistry {
    /// Get or assign a deterministic token for a secret value.
    fn token_for(&mut self, kind_label: &str, secret_value: &str) -> String {
        let key = (kind_label.to_string(), secret_value.to_string());
        if let Some(&n) = self.seen.get(&key) {
            format!("REDACTED_{}_{}", kind_label.to_uppercase(), n)
        } else {
            let counter = self
                .type_counters
                .entry(kind_label.to_string())
                .or_insert(0);
            *counter += 1;
            let n = *counter;
            self.seen.insert(key, n);
            let token = format!("REDACTED_{}_{}", kind_label.to_uppercase(), n);
            self.issued.insert(token.clone());
            token
        }
    }
}

/// A match that survived all eligibility filters, as positions only.
///
/// `start..end` is the secret only: the pattern's [`SECRET_GROUP`] when it
/// has one, otherwise the whole match. Every caller replaces exactly that
/// span, so a key, delimiter or URI around the secret stays as written.
/// `claim` is the whole match; it decides which of two matches with
/// overlapping secrets wins (see [`dedup_overlapping_matches`]). The
/// secret's text is not copied: [`EligibleMatch::secret`] reads it from the
/// content the match was found in, so only what a caller keeps is copied.
struct EligibleMatch {
    claim: Range<usize>,
    start: usize,
    end: usize,
    line_num: usize,
}

impl EligibleMatch {
    /// The secret's text in `content`, which must be the content the match
    /// was found in, or a copy edited only after `end`.
    fn secret<'c>(&self, content: &'c str) -> &'c str {
        &content[self.start..self.end]
    }
}

/// Known non-secret values that appear after `password:` or `passwd:`
/// in NSS, PAM, and similar config files. Checked case-insensitively.
static FALSE_POSITIVE_VALUES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "files",
        "compat",
        "sss",
        "ldap",
        "nis",
        "hesiod",
        "systemd",
        "nisplus",
        "winbind",
        "required",
        "sufficient",
        "optional",
        "include",
        "substack",
        "pam_unix.so",
        "pam_sss.so",
        "pam_deny.so",
        "pam_permit.so",
        "pam_env.so",
        "requisite",
    ]
    .into_iter()
    .collect()
});

/// The most characters any of [`FALSE_POSITIVE_VALUES`] has.
static LONGEST_FALSE_POSITIVE_VALUE: LazyLock<usize> = LazyLock::new(|| {
    FALSE_POSITIVE_VALUES
        .iter()
        .map(|v| v.chars().count())
        .max()
        .unwrap_or(0)
});

/// Returns true if `value` is a known NSS/PAM token, not a real secret.
///
/// Lowercasing never maps one character to none, so a value with more
/// characters than the longest token cannot be one, and is not copied to
/// find out.
fn is_false_positive_value(value: &str) -> bool {
    let value = value.trim();
    value.chars().nth(*LONGEST_FALSE_POSITIVE_VALUE).is_none()
        && FALSE_POSITIVE_VALUES.contains(value.to_lowercase().as_str())
}

/// Line-comment marker common to nearly every `/etc` file format.
const HASH_COMMENT: &str = "#";
/// Line-comment marker for INI-style files.
const SEMICOLON_COMMENT: &str = ";";
/// Line-comment marker for C-style configs such as BIND's `named.conf`.
const DOUBLE_SLASH_COMMENT: &str = "//";

/// INI-style extensions whose parsers treat a leading `;` as a comment
/// (php.ini, MySQL/MariaDB `.cnf`, dnf/yum `.repo`).
const SEMICOLON_COMMENT_EXTENSIONS: &[&str] = &[".ini", ".cnf", ".repo"];
/// File names whose parsers treat a leading `;` as a comment.
const SEMICOLON_COMMENT_FILE_NAMES: &[&str] = &["smb.conf"];
/// File names whose parsers treat a leading `//` as a comment.
const DOUBLE_SLASH_COMMENT_FILE_NAMES: &[&str] = &["named.conf"];

/// Directory name that marks a systemd configuration tree:
/// `/etc/systemd/`, `/usr/lib/systemd/`, `/run/systemd/`,
/// `/etc/containers/systemd/` (quadlets) and the per-user trees.
const SYSTEMD_DIR: &str = "systemd";
/// Unit file suffixes read with systemd's unit-file parser: the unit
/// types, networkd's `.network`/`.netdev`/`.link`, `systemd-nspawn`'s
/// `.nspawn`, and the quadlet types.
const SYSTEMD_UNIT_SUFFIXES: &[&str] = &[
    ".service",
    ".socket",
    ".device",
    ".mount",
    ".automount",
    ".swap",
    ".target",
    ".path",
    ".timer",
    ".slice",
    ".scope",
    ".network",
    ".netdev",
    ".link",
    ".nspawn",
    ".container",
    ".volume",
    ".kube",
    ".pod",
    ".image",
    ".build",
];
/// Suffix of a drop-in directory (`foo.service.d/`, `journald.conf.d/`).
const DROP_IN_DIR_SUFFIX: &str = ".d";
/// Suffix of a drop-in file and of a systemd daemon's own configuration.
const CONF_SUFFIX: &str = ".conf";
/// Pseudo-path prefix redaction uses for unit content inspectah generates
/// itself (cron jobs converted to timers), which is staged as a unit.
const GENERATED_UNIT_PREFIX: &str = "generated:";

/// Returns true if systemd reads the file at `path` with the syntax of
/// `systemd.syntax(7)`, where a line starting with `#` or `;` is a
/// comment: a unit under a systemd tree, a `.conf` drop-in in a `*.d/`
/// directory under one, a daemon configuration such as
/// `/etc/systemd/journald.conf`, or a generated unit. A `.conf` anywhere
/// else is not assumed to share that syntax.
fn is_systemd_syntax(path: &str) -> bool {
    if let Some(name) = path.strip_prefix(GENERATED_UNIT_PREFIX) {
        return SYSTEMD_UNIT_SUFFIXES.iter().any(|s| name.ends_with(s));
    }
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let Some((file_name, dirs)) = components.split_last() else {
        return false;
    };
    if !dirs.contains(&SYSTEMD_DIR) {
        return false;
    }
    if SYSTEMD_UNIT_SUFFIXES.iter().any(|s| file_name.ends_with(s)) {
        return true;
    }
    file_name.ends_with(CONF_SUFFIX)
        && dirs
            .last()
            .is_some_and(|parent| parent.ends_with(DROP_IN_DIR_SUFFIX) || *parent == SYSTEMD_DIR)
}

/// Returns the line-comment markers that are real comment syntax for
/// the file at `path`. `#` applies everywhere but the kernel command
/// line, which has no comments: a `#` there is an argument, and treating
/// a line that starts with one as a comment left every secret on it.
/// `;` and `//` apply only to file types that use them, because
/// elsewhere a line starting with them (`//srv/share/...`) is data and
/// must still be scanned.
fn comment_markers(path: Option<&str>) -> Vec<&'static str> {
    if is_kernel_cmdline(path) {
        return Vec::new();
    }
    let mut markers = vec![HASH_COMMENT];
    let Some(path) = path else {
        return markers;
    };
    let file_name = path.rsplit('/').next().unwrap_or(path);
    if SEMICOLON_COMMENT_FILE_NAMES.contains(&file_name)
        || SEMICOLON_COMMENT_EXTENSIONS
            .iter()
            .any(|ext| file_name.ends_with(ext))
        || is_systemd_syntax(path)
    {
        markers.push(SEMICOLON_COMMENT);
    }
    if DOUBLE_SLASH_COMMENT_FILE_NAMES.contains(&file_name) {
        markers.push(DOUBLE_SLASH_COMMENT);
    }
    markers
}

/// Line facts for byte positions visited in increasing order.
///
/// A scan can find thousands of matches on one long line. Reading back to
/// the line start, or counting newlines from the start of the content, for
/// each of them made the scan quadratic in the line's length; the cursor
/// reads each byte once while positions only increase.
struct LineCursor<'c> {
    content: &'c str,
    /// The position the fields below describe.
    pos: usize,
    /// Byte offset of the start of the line holding `pos`.
    start: usize,
    /// The 1-based number of that line: one more than the newlines before
    /// `pos`.
    number: usize,
    /// Offset of the first non-whitespace character at or after `start`,
    /// once something has asked for it.
    first_text: Option<usize>,
}

impl<'c> LineCursor<'c> {
    fn new(content: &'c str) -> Self {
        Self {
            content,
            pos: 0,
            start: 0,
            number: 1,
            first_text: None,
        }
    }

    /// Move to `pos`. A position before the current one starts over from
    /// the beginning of the content, so a caller that goes back gets a
    /// slow answer, never a wrong one.
    fn advance(&mut self, pos: usize) {
        if pos < self.pos {
            *self = Self::new(self.content);
        }
        let skipped = &self.content[self.pos..pos];
        if let Some(last) = skipped.rfind('\n') {
            self.number += skipped.matches('\n').count();
            self.start = self.pos + last + 1;
            self.first_text = None;
        }
        self.pos = pos;
    }

    /// Whether the line holding the current position is a `[section]`
    /// header whose id dnf accepts ([`is_repo_id_header`]).
    fn is_repo_id_header(&self) -> bool {
        let rest = &self.content[self.start..];
        is_repo_id_header(&rest[..rest.find(['\r', '\n']).unwrap_or(rest.len())])
    }

    /// Returns true if the match whose whole match starts at `claim_start`
    /// falls on a comment line: the text of the current line before the
    /// match, trimmed, starts with one of `markers` (see
    /// [`comment_markers`]).
    ///
    /// Call it with the cursor on the secret's start. The line is the
    /// secret's, never the line the match starts on. A pattern can consume
    /// a boundary byte before its secret, and when that byte is the newline
    /// ending a comment, reading the match's line took the comment above
    /// for the secret's own line and exempted it. Context the match
    /// consumed on the secret's own line is not part of the prefix, so a
    /// marker the pattern took as its boundary (`#AKIA...`) does not make
    /// the line a comment.
    fn is_comment(&mut self, claim_start: usize, markers: &[&str]) -> bool {
        let (content, start) = (self.content, self.start);
        let first_text = *self.first_text.get_or_insert_with(|| {
            let rest = &content[start..];
            start + rest.len() - rest.trim_start().len()
        });
        // The prefix is `content[start..claim_start]` trimmed, which is
        // empty when the match starts before the line's first text.
        markers
            .iter()
            .any(|m| first_text + m.len() <= claim_start && content[first_text..].starts_with(m))
    }
}

/// Whether `line`, trimmed, is `[`, an id dnf accepts ([`is_repo_id`]),
/// then `]`: a header that names a repo. A bracketed line holding anything
/// else (`[{"token":"x"}]`) names no repo dnf reads, whatever the scan's
/// permissive parser makes of it.
fn is_repo_id_header(line: &str) -> bool {
    line.trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(is_repo_id)
}

/// `em`, a match in a `.repo` file, as the pieces of its secret that hold
/// no repo-id header line ([`is_repo_id_header`]), each a match of its own
/// with `em`'s claim.
///
/// A multi-line secret (a PEM block between `name=-----BEGIN ...` and a
/// later `name=-----END ...`) replaced whole took every header between
/// its ends, and with them sections the collector had attributed packages
/// to. A header line is no part of a secret: its characters (letters,
/// digits, `-_.:`) hold no PEM or key material, so keeping it leaves no
/// secret byte. The line breaks on either side of a kept header stay too,
/// and a `\r` before one, so the header stays a line of its own. The
/// options between headers are still replaced, which `redact()` records
/// in `redaction_changed`.
fn around_repo_id_headers(em: EligibleMatch, content: &str) -> Vec<EligibleMatch> {
    let secret = &content[em.start..em.end];
    if !secret.contains('\n') {
        return vec![em];
    }
    // Each line the secret touches, as the part of it inside the secret,
    // with whether the whole line is a repo-id header.
    let mut lines: Vec<(Range<usize>, bool)> = Vec::new();
    let mut pos = em.start;
    loop {
        let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
        let line_end = content[pos..].find('\n').map_or(content.len(), |i| pos + i);
        let header = is_repo_id_header(&content[line_start..line_end]);
        lines.push((pos..line_end.min(em.end), header));
        if line_end >= em.end {
            break;
        }
        pos = line_end + 1;
    }
    if !lines.iter().any(|(_, header)| *header) {
        return vec![em];
    }
    let mut pieces = Vec::new();
    let mut run: Option<Range<usize>> = None;
    let mut flush = |run: &mut Option<Range<usize>>, before_header: bool| {
        if let Some(mut piece) = run.take() {
            if before_header && content[..piece.end].ends_with('\r') {
                piece.end -= 1;
            }
            if piece.start < piece.end {
                pieces.push(EligibleMatch {
                    claim: em.claim.clone(),
                    start: piece.start,
                    end: piece.end,
                    line_num: em.line_num + content[em.start..piece.start].matches('\n').count(),
                });
            }
        }
    };
    for (span, header) in lines {
        if header {
            flush(&mut run, true);
        } else {
            run = Some(match run {
                Some(r) => r.start..span.end,
                None => span,
            });
        }
    }
    flush(&mut run, false);
    pieces
}

/// Offset of the first line break (`\r` or `\n`) at or after `pos`, or the
/// end of `content`.
fn line_break_at_or_after(content: &str, pos: usize) -> usize {
    content[pos..]
        .find(['\r', '\n'])
        .map_or(content.len(), |i| pos + i)
}

/// How many matches on one line may be found inside text an earlier match
/// on that line already read, without adding anything to what is replaced,
/// before the rest of the line is replaced instead of searched.
///
/// Each such search can re-read the rest of the line, so this bounds the
/// work on a line at a small multiple of its length. Reaching it takes
/// more credential keys than this inside one value on one line, as in
/// `password=password=...`, with the value ending before the line does.
/// Past it the rest of the line is replaced, which can take text no
/// pattern would have, but never leaves a byte one would have taken.
const MAX_REDUNDANT_RESCANS_PER_LINE: usize = 4;

/// Collect regex matches from `content` for a single `pat`, filtering out:
/// - Matches on comment lines (`#`, plus `;` / `//` where `path` uses them;
///   none on the kernel command line)
/// - Matches whose value is a known NSS/PAM token, for a pattern with
///   `filter_nss_pam_tokens` set, except on the kernel command line
/// - PasswordHash matches when `path` is a shadow file
///
/// Every pattern goes through this one path, with or without a
/// [`SECRET_GROUP`], so every filter applies to every pattern; the filters
/// read the secret's text, never the key's.
///
/// This is the ONLY function that should run a pattern's regex over content.
/// Callers must invoke this for EVERY pattern, merge the results into a
/// single `Vec<(usize, EligibleMatch)>` (tagged with the pattern index),
/// then run `dedup_overlapping_matches` on the merged list before
/// processing matches.
///
/// # Where each search starts
///
/// Each match is searched for from where the previous match's secret
/// starts, not from where the whole match ends. A secret can run on past
/// its own value: the JSON pattern's `\S*` after a closing quote takes
/// `"alpha","token":"beta` from `{"password":"alpha","token":"beta gamma"}`.
/// `captures_iter` resumes after that, inside the next key's value, so
/// `token` was never matched and `gamma` shipped. Resuming at the secret
/// looks for every key from the end of the key before it. A match whose
/// secret is the whole match (no key before it) resumes after its end, as
/// `captures_iter` does.
///
/// # Keeping that linear
///
/// Resuming inside a secret re-reads it, and with keys packed into one
/// value (`password=password=...x`) every key's secret is the rest of the
/// line: thousands of searches, each reading to the end of the line.
/// Three rules bound that, and none of them replaces less than resuming at
/// every secret does:
///
/// - **Only positions are kept.** A match is its ranges and line number;
///   no secret text is copied here or in the merge (see
///   [`EligibleMatch::secret`]).
/// - **A secret inside what this pattern already covers is not kept.**
///   Secret starts only increase, so the eligible secrets found so far
///   that overlap form one range. A secret inside it adds no byte, and its
///   whole match is shorter than that of the secret it sits in, so it
///   would lose the merge and change neither the span nor the token.
/// - **A covered line is not searched again.** No keyed pattern's match
///   or secret crosses a line break, so once that range reaches the break
///   ending the line, nothing found later on the line can add to it, and
///   the search moves to the break.
///
/// Those leave a value that stops before the end of the line, as in
/// `password=password=...x tail`, where the keys inside it still each read
/// to `x`. After [`MAX_REDUNDANT_RESCANS_PER_LINE`] such matches the rest
/// of the line, from where the next search would start, becomes one
/// eligible secret of this pattern, without filters. Every match that
/// search could still find on the line lies inside it, so this replaces
/// more than an unbounded search would, never less.
fn collect_eligible_matches(
    pat: &SecretPattern,
    content: &str,
    path: Option<&str>,
) -> Vec<EligibleMatch> {
    let markers = comment_markers(path);
    let repo_file = path.is_some_and(is_repo_file_path);
    // Repo-id headers this pattern leaves alone: only the generic one's,
    // and only in a `.repo` file dnf reads.
    let headers_kept = pat.spares_repo_id_headers && repo_file;
    let mut lines = LineCursor::new(content);
    let mut found = Vec::new();
    // The union of the eligible secrets kept last, which overlap one
    // another; empty until one is kept.
    let mut covered = 0..0;
    // How far the whole matches found so far reach.
    let mut read_to = 0;
    let mut line_end = line_break_at_or_after(content, 0);
    let mut redundant = 0;
    let mut pos = 0;
    while pos <= content.len() {
        if pos > line_end {
            line_end = line_break_at_or_after(content, pos);
            redundant = 0;
        }
        if pos < line_end && pos < covered.end && covered.end >= line_end {
            pos = line_end;
            continue;
        }
        if pos < line_end && redundant >= MAX_REDUNDANT_RESCANS_PER_LINE {
            lines.advance(pos);
            if headers_kept && lines.is_repo_id_header() {
                pos = line_end;
                continue;
            }
            found.push(EligibleMatch {
                claim: pos..line_end,
                start: pos,
                end: line_end,
                line_num: lines.number,
            });
            covered = if pos < covered.end {
                covered.start..line_end
            } else {
                pos..line_end
            };
            pos = line_end;
            continue;
        }
        let Some(caps) = pat.regex.captures_at(content, pos) else {
            break;
        };
        let Some(whole) = caps.get(0) else {
            break;
        };
        let secret = caps.name(SECRET_GROUP).unwrap_or(whole);
        let nested = whole.start() < read_to && whole.start() < line_end;
        read_to = read_to.max(whole.end());

        let added = match eligible_match(
            pat,
            content,
            &caps,
            path,
            &markers,
            headers_kept,
            &mut lines,
        ) {
            Some(em) if em.start < covered.end && em.end <= covered.end => false,
            Some(em) => {
                covered = if em.start < covered.end {
                    covered.start..em.end
                } else {
                    em.start..em.end
                };
                if repo_file {
                    // No pattern's secret takes a repo-id header line.
                    found.extend(around_repo_id_headers(em, content));
                } else {
                    found.push(em);
                }
                true
            }
            None => false,
        };
        if nested && !added {
            redundant += 1;
        }

        pos = if secret.start() > whole.start() {
            secret.start()
        } else if whole.end() > whole.start() {
            whole.end()
        } else {
            // An empty match: step past one character so the search moves.
            whole.end()
                + content[whole.end()..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8)
        };
    }
    found
}

/// The match `caps` as an [`EligibleMatch`], or `None` when a filter of
/// [`collect_eligible_matches`] drops it. `lines` must not be past the
/// secret's start.
fn eligible_match(
    pat: &SecretPattern,
    content: &str,
    caps: &Captures<'_>,
    path: Option<&str>,
    markers: &[&str],
    headers_kept: bool,
    lines: &mut LineCursor<'_>,
) -> Option<EligibleMatch> {
    let whole = caps.get(0)?;
    let secret = caps.name(SECRET_GROUP).unwrap_or(whole);
    let text = secret.as_str();

    // Filter 0: the pattern's own condition on the match
    if let Some(accept) = pat.accept
        && !accept(content, caps)
    {
        return None;
    }

    // Filter 0b: a pattern that describes one file format applies only to
    // a path in that format (`SecretPattern::paths`).
    if let Some(applies) = pat.paths
        && !path.is_some_and(applies)
    {
        return None;
    }

    // Filter 1: skip matches on comment lines
    lines.advance(secret.start());
    if lines.is_comment(whole.start(), markers) {
        return None;
    }

    // Filter 1b: for the generic pattern in a `.repo` file
    // (`headers_kept`), skip matches on a header whose id dnf accepts. A
    // repo id can spell a credential key: in
    // `[copr:copr.fedorainfracloud.org:token:vendor]` the generic pattern
    // took `vendor]`, the file then defined no section, and every
    // decision read from the stored file lost the repo. Any other
    // bracketed line, and every other pattern, is searched as usual: an
    // exemption for every `[...]` line kept `[{"token":"x"}]` whole.
    if headers_kept && lines.is_repo_id_header() {
        return None;
    }

    // No filter skips a value spelled like a redaction token. The
    // spelling is not provenance: `password=REDACTED_COMPANY_2026`
    // can be the host's real password. `redact()` runs once per
    // snapshot; a second pass over its output reports most tokens
    // again and replaces each with a token, usually itself, and can
    // change the text around one (see
    // `test_a_second_pass_is_not_guaranteed_to_return_the_same_text`).

    // Filter 2: skip matches whose value is a known NSS/PAM
    // false-positive token, for the patterns that ask for it
    // (`SecretPattern::filter_nss_pam_tokens`). Not on the kernel command
    // line: the words are nsswitch and PAM syntax, and there a pattern's
    // secret can be only the first word of a quoted value
    // (`"rd.iscsi.password=files bravo"`), so skipping it left the whole
    // value unredacted. `files` alone is a password there too.
    if pat.filter_nss_pam_tokens && !is_kernel_cmdline(path) && is_false_positive_value(text) {
        return None;
    }

    // Filter 3: skip PasswordHash matches when path is a shadow
    // file — scan_shadow owns those with richer classification
    if pat.finding_kind == FindingKind::PasswordHash
        && let Some(p) = path
        && (p.ends_with("/shadow") || p.ends_with("/shadow-"))
    {
        return None;
    }

    Some(EligibleMatch {
        claim: whole.range(),
        start: secret.start(),
        end: secret.end(),
        line_num: lines.number,
    })
}

/// Resolve overlapping matches from different patterns so no two replaced
/// spans overlap, without losing any secret byte. Input must be the combined
/// matches from ALL patterns for a single content blob. Only positions are
/// merged; a caller reads a kept span's text with [`EligibleMatch::secret`].
///
/// Two matches conflict when their secret spans (what gets replaced; see
/// [`EligibleMatch`]) overlap. The conflict goes to the larger whole match,
/// then the longer secret, then the earlier pattern in `PATTERNS`: so a JDBC
/// URL beats the generic `password=` inside it. The winner keeps its pattern
/// (and so its token kind), but its span grows to the union of both secrets:
/// a loser whose secret runs past the winner's still has those bytes
/// replaced. Dropping the loser instead left them in place, so
/// `jdbc:...?token=a&password=b` shipped `a` when only `b` was the JDBC
/// secret. A match whose secret overlaps nothing is kept as it is, even when
/// it sits inside another pattern's whole match. Output is ordered by secret
/// start.
///
/// A candidate can overlap several kept spans that do not overlap each
/// other: it bridges them. All of them become one span, from the smallest
/// start to the largest end. Its text, read from the content under that
/// span, is never stitched from the pieces: stitching assumed each pair
/// overlapped, and a bridge merged after the spans it joined broke that and
/// panicked.
fn dedup_overlapping_matches(matches: &mut Vec<(usize, EligibleMatch)>) {
    let mut by_priority: Vec<(usize, EligibleMatch)> = std::mem::take(matches);
    // Stable sort: equal keys keep input (pattern) order.
    by_priority.sort_by(|a, b| {
        b.1.claim
            .len()
            .cmp(&a.1.claim.len())
            .then_with(|| (b.1.end - b.1.start).cmp(&(a.1.end - a.1.start)))
    });
    for (pat_idx, candidate) in by_priority {
        // Kept spans are pushed in priority order and only ever merged
        // into, so the first one the candidate overlaps outranks the rest.
        let overlapping: Vec<usize> = matches
            .iter()
            .enumerate()
            .filter(|(_, (_, kept))| candidate.start < kept.end && kept.start < candidate.end)
            .map(|(i, _)| i)
            .collect();
        let Some((&winner, others)) = overlapping.split_first() else {
            matches.push((pat_idx, candidate));
            continue;
        };
        // Every span here overlaps the candidate, so together they cover
        // one contiguous range.
        let mut start = candidate.start;
        let mut end = candidate.end;
        let mut claim = candidate.claim;
        for &i in &overlapping {
            let kept = &matches[i].1;
            start = start.min(kept.start);
            end = end.max(kept.end);
            claim = claim.start.min(kept.claim.start)..claim.end.max(kept.claim.end);
        }
        // Highest index first, so each removal leaves `winner` in place.
        for &i in others.iter().rev() {
            matches.remove(i);
        }
        // The winner keeps its line number and pattern.
        let kept = &mut matches[winner].1;
        kept.start = start;
        kept.end = end;
        kept.claim = claim;
    }
    matches.sort_by_key(|(_, em)| em.start);
}

/// Build the replacement for a matched secret: the token for its text in
/// `content`. It replaces `em.start..em.end`, which is the secret alone (see
/// [`EligibleMatch`]), so the key or URI around it stays as written.
fn build_replacement(
    em: &EligibleMatch,
    content: &str,
    registry: &mut CounterRegistry,
    kind_label: &str,
) -> String {
    registry.token_for(kind_label, em.secret(content))
}

/// Apply pattern-based redaction to a string.
/// Returns `Cow::Borrowed` if no findings — zero-copy for clean content.
pub fn redact_string(content: &str) -> Cow<'_, str> {
    let mut has_match = false;
    for pat in PATTERNS.iter() {
        if pat.regex.is_match(content) {
            has_match = true;
            break;
        }
    }

    if !has_match {
        return Cow::Borrowed(content);
    }

    // At least one pattern matched — clone and redact.
    let mut result = content.to_string();
    let mut registry = CounterRegistry::default();

    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
    for (idx, pat) in PATTERNS.iter().enumerate() {
        for em in collect_eligible_matches(pat, &result, None) {
            all_matches.push((idx, em));
        }
    }
    dedup_overlapping_matches(&mut all_matches);
    if all_matches.is_empty() {
        // Regex matched but all hits were filtered (comment lines, false
        // positives, etc.) — no actual redaction needed.
        return Cow::Borrowed(content);
    }
    // Sort descending by offset so replacements don't invalidate earlier offsets
    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
    for (pat_idx, em) in all_matches {
        let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
        let replacement = build_replacement(&em, &result, &mut registry, &kind_label);
        result.replace_range(em.start..em.end, &replacement);
    }

    Cow::Owned(result)
}

/// The path of the kernel command line, where a high-confidence hint is
/// raised for each sensitive parameter.
const KERNEL_CMDLINE_PATH: &str = "/proc/cmdline";

/// Whether `path` is the kernel command line, which the filters for
/// config-file syntax (comments, NSS/PAM words) do not apply to.
fn is_kernel_cmdline(path: Option<&str>) -> bool {
    path == Some(KERNEL_CMDLINE_PATH)
}

/// Redacts a kernel command line one kernel argument at a time.
///
/// [`kernel_cmdline_matches`] finds where a secret is, but a pattern's
/// secret can stop short of what the kernel reads as the value: in
/// `"rd.iscsi.password=alpha bravo"` the whole argument is quoted, so the
/// value is `alpha bravo` while the pattern stops at `alpha`. So each
/// argument ([`kernel_args`]) a secret falls in has its whole parsed value
/// replaced by one token, or, when the secret reaches into the name or
/// the argument has no value, the whole argument. Quoting around a value
/// stays. No part of a secret argument's value survives, whatever the
/// quoting.
fn redact_kernel_cmdline(cmdline: &str, registry: &mut CounterRegistry) -> String {
    let matches = kernel_cmdline_matches(cmdline);
    let args = kernel_args(cmdline);
    let overlaps = |a: &Range<usize>, b: &Range<usize>| a.start < b.end && b.start < a.end;
    // Each span to replace, with the pattern that found its secret.
    let mut spans: Vec<(Range<usize>, usize)> = Vec::new();
    for (pat_idx, em) in &matches {
        let secret = em.start..em.end;
        let mut in_an_arg = false;
        for arg in args.iter().filter(|arg| overlaps(&arg.raw_span(), &secret)) {
            in_an_arg = true;
            // A secret that reaches past the name (into the `=` or the
            // value's quotes) still takes only the value.
            let span = match arg.value_span() {
                Some(v) if !overlaps(&arg.name_span(), &secret) => v,
                _ => arg.raw_span(),
            };
            spans.push((span, *pat_idx));
        }
        // Only whitespace lies between arguments; a secret there is
        // still replaced as found.
        if !in_an_arg {
            spans.push((secret, *pat_idx));
        }
    }

    // A value span lies inside its argument's span, and two arguments
    // never overlap, so merging overlaps leaves one span per argument.
    spans.sort_by_key(|(span, _)| (span.start, std::cmp::Reverse(span.end)));
    let mut merged: Vec<(Range<usize>, usize)> = Vec::new();
    for (span, pat_idx) in spans {
        match merged.last_mut() {
            Some((last, _)) if span.start < last.end => last.end = last.end.max(span.end),
            _ => merged.push((span, pat_idx)),
        }
    }

    let mut content = cmdline.to_string();
    for (span, pat_idx) in merged.into_iter().rev() {
        let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
        let token = registry.token_for(&kind_label, &cmdline[span.clone()]);
        content.replace_range(span, &token);
    }
    content
}

/// The kernel command line as [`redact`] writes it back to the snapshot,
/// for a pass that runs before redaction and must know which arguments
/// the kargs.d writer will find redacted. The tokens come from a registry
/// of their own, so they may be numbered differently from the ones
/// [`redact`] writes; only where they fall matters.
pub(crate) fn redacted_kernel_cmdline(cmdline: &str) -> Cow<'_, str> {
    if scan_content(cmdline, KERNEL_CMDLINE_PATH).is_empty() {
        return Cow::Borrowed(cmdline);
    }
    Cow::Owned(redact_kernel_cmdline(
        cmdline,
        &mut CounterRegistry::default(),
    ))
}

/// Returns true if the kernel reads `arg` as a credential: its name is one
/// the generic credential pattern treats as a credential key. The kernel
/// compares parameter names with `-` and `_` as one character (`parameq`),
/// so `api-key` is `api_key`.
fn is_credential_kernel_arg(arg: &KernelArg<'_>) -> bool {
    is_credential_name(&arg.name().replace('-', "_"))
}

/// The secrets on a kernel command line, for detection and replacement
/// alike.
///
/// Every pattern searches each kernel argument ([`kernel_args`]) on its
/// own, as it searches a file, and never reads across the whitespace
/// between two arguments. Searching the whole line let a match run from
/// one argument into the next: in `rd.iscsi.password=
/// systemd.setenv=DB_PASSWORD=bravo7k` the generic pattern read past the
/// space and took the next argument as the empty password's value.
///
/// An argument with a credential name ([`is_credential_kernel_arg`]) is
/// found by its name instead of by the generic pattern, and its whole
/// non-empty value is the secret, whatever the value holds. The generic
/// pattern needs `\S` right after the `=`, so a value starting with a
/// Unicode space (`"password=\u{2003}x"`) was found by no pattern and kept
/// whole, while it takes the quotes of an empty `password=""` for a value.
/// Leaving the generic pattern off such an argument cannot lose a secret:
/// its search stays inside the argument, and past the name the argument
/// holds only its `=`, quotes and value, which the name match covers
/// whole. An empty value (`password=`, `password=""`) holds nothing to
/// redact and is not a finding; neither is a credential name with no `=`.
/// The other patterns still search a credential argument.
fn kernel_cmdline_matches(cmdline: &str) -> Vec<(usize, EligibleMatch)> {
    let generic = *GENERIC_CREDENTIAL_PATTERN;
    let line_num = |pos: usize| cmdline[..pos].matches('\n').count() + 1;
    let mut matches: Vec<(usize, EligibleMatch)> = Vec::new();
    for arg in kernel_args(cmdline) {
        let raw = arg.raw_span();
        let by_name = is_credential_kernel_arg(&arg);
        for (idx, pat) in PATTERNS.iter().enumerate() {
            if by_name && idx == generic {
                continue;
            }
            for em in collect_eligible_matches(pat, arg.raw(), Some(KERNEL_CMDLINE_PATH)) {
                let start = raw.start + em.start;
                matches.push((
                    idx,
                    EligibleMatch {
                        claim: raw.start + em.claim.start..raw.start + em.claim.end,
                        start,
                        end: raw.start + em.end,
                        line_num: line_num(start),
                    },
                ));
            }
        }
        if !by_name {
            continue;
        }
        let Some(value) = arg.value_span().filter(|v| !v.is_empty()) else {
            continue;
        };
        matches.push((
            generic,
            EligibleMatch {
                claim: raw,
                start: value.start,
                end: value.end,
                line_num: line_num(value.start),
            },
        ));
    }
    dedup_overlapping_matches(&mut matches);
    matches
}

/// Where a hinted value ends, which is what must have been replaced for the
/// hint to count as handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintValueEnd {
    /// At the end of the kernel argument, less the double quotes around
    /// it: the kernel command line, whose arguments are separated by
    /// whitespace outside quotes ([`kernel_args`]).
    KernelArgument,
    /// At the end of the line, or at a `#` comment after whitespace: a
    /// `.env` file, where the value runs to the end of the line.
    EndOfLine,
}

impl HintValueEnd {
    /// The end that applies to content collected at `path`. Only the
    /// kernel command line is known to end a value at the end of its
    /// argument; anywhere else the conservative end is the end of the
    /// line.
    fn for_path(path: &str) -> Self {
        if path == KERNEL_CMDLINE_PATH {
            Self::KernelArgument
        } else {
            Self::EndOfLine
        }
    }
}

/// Returns true if `keyword` appears in `content` anywhere its value was
/// not replaced by this run.
///
/// Redaction keeps the key and replaces only the value, so a hint's keyword
/// (`password`, `DB_PASSWORD`, `key=`) survives a successful pass. An
/// occurrence counts as handled only when its value, past optional spacing
/// and one `=` or `:`, is a token in `issued` (one this run's registry
/// handed out) up to the next whitespace (under
/// [`HintValueEnd::KernelArgument`], to the end of the kernel argument,
/// quotes aside), and, under [`HintValueEnd::EndOfLine`], nothing but whitespace
/// or a `#` comment follows it on the line. A value that only starts with
/// a token (`REDACTED_PASSWORD_1/hunter2`, `REDACTED_PASSWORD_1
/// still-secret` in a `.env` file or inside a quoted kernel argument) or
/// is only spelled like one is the host's, and so is
/// anything else (a value left in place, a longer identifier such as
/// `tokenizer`, a comment mentioning the word); any of these leaves the
/// hint unresolved.
fn keyword_survives_with_its_value(
    content: &str,
    keyword: &str,
    issued: &HashSet<String>,
    value_end: HintValueEnd,
) -> bool {
    content.match_indices(keyword).any(|(i, _)| {
        let rest = content[i + keyword.len()..].trim_start_matches([' ', '\t']);
        let rest = rest
            .strip_prefix(['=', ':'])
            .unwrap_or(rest)
            .trim_start_matches([' ', '\t']);
        let token_end = match value_end {
            // The value runs to the end of the kernel argument it starts
            // in, which whitespace inside quotes does not end.
            HintValueEnd::KernelArgument => {
                let at = content.len() - rest.len();
                kernel_args(content)
                    .iter()
                    .find(|arg| arg.raw_span().contains(&at))
                    .map_or(0, |arg| arg.raw_span().end - at)
            }
            HintValueEnd::EndOfLine => rest.find(char::is_whitespace).unwrap_or(rest.len()),
        };
        let value = &rest[..token_end];
        let value = match value_end {
            // The double quotes around a kernel argument or its value are
            // quoting, not value: `"password=TOKEN"`, `password="TOKEN"`.
            HintValueEnd::KernelArgument => {
                let value = value.strip_prefix('"').unwrap_or(value);
                value.strip_suffix('"').unwrap_or(value)
            }
            HintValueEnd::EndOfLine => value,
        };
        if !issued.contains(value) {
            return true;
        }
        match value_end {
            HintValueEnd::KernelArgument => false,
            HintValueEnd::EndOfLine => {
                let after = &rest[token_end..];
                let line_rest = after[..after.find('\n').unwrap_or(after.len())].trim_start();
                !(line_rest.is_empty() || line_rest.starts_with('#'))
            }
        }
    })
}

/// What a URL credential masker writes in place of the credential.
const URL_CREDENTIAL_MASK: &str = "[REDACTED]";

/// One credential a URL credential masker found in a line: the bytes of
/// the credential alone, which [`URL_CREDENTIAL_MASK`] replaces, and its
/// finding.
struct CredentialEdit {
    range: Range<usize>,
    finding: RedactionFinding,
}

/// One edit per match of `re` in `line`, each replacing capture group 2,
/// the credential. Every match is taken, not only the first: a line can
/// hold several credentials.
fn credential_edits(re: &Regex, line: &str, finding: &RedactionFinding) -> Vec<CredentialEdit> {
    re.captures_iter(line)
        .filter_map(|caps| caps.get(2))
        .map(|m| CredentialEdit {
            range: m.range(),
            finding: finding.clone(),
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    /// How many candidates [`merge_credential_edits`] has examined, so a
    /// test can hold the merge to linear work without timing it.
    static MERGE_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Count one step of [`merge_credential_edits`] in a test build.
fn note_merge_step() {
    #[cfg(test)]
    MERGE_STEPS.with(|n| n.set(n.get() + 1));
}

/// The credential edits of one line as one ordered, disjoint list.
///
/// `url` holds the URL maskers' edits, `key_values` the `proxy_password=`
/// values. A value that is itself one URL (`proxy_password=http://u:p@h`)
/// and holds a URL edit is that URL's credential, so it is dropped and the
/// URL keeps its user and host. Any other overlap becomes one edit over
/// the union of both ranges, never a drop: an edit is dropped only when
/// another covers all of its bytes, since dropping a partly covered one
/// shipped the bytes only it covered. The merged edit keeps the finding of
/// the edit that starts first, a URL edit before a value on a tie.
///
/// Every list is walked once in order, so the work is linear in the
/// number of edits after the sort, however densely they sit on the line.
fn merge_credential_edits(
    line: &str,
    mut url: Vec<CredentialEdit>,
    mut key_values: Vec<CredentialEdit>,
) -> Vec<CredentialEdit> {
    url.sort_by_key(|e| e.range.start);
    key_values.sort_by_key(|e| e.range.start);
    // The first URL edit that can still overlap a value: one ending at or
    // before a value's start ends before every later value's start too.
    let mut next_url = 0;
    key_values.retain(|kv| {
        note_merge_step();
        while url
            .get(next_url)
            .is_some_and(|u| u.range.end <= kv.range.start)
        {
            note_merge_step();
            next_url += 1;
        }
        let holds_url_edit = url
            .get(next_url)
            .is_some_and(|u| u.range.start < kv.range.end);
        !(holds_url_edit && URL_VALUE_RE.is_match(&line[kv.range.clone()]))
    });
    let mut edits = url;
    edits.append(&mut key_values);
    // Stable: a URL edit stays ahead of a value with the same range.
    edits.sort_by_key(|e| (e.range.start, std::cmp::Reverse(e.range.end)));
    let mut merged: Vec<CredentialEdit> = Vec::with_capacity(edits.len());
    for edit in edits {
        note_merge_step();
        match merged.last_mut() {
            Some(last) if edit.range.start < last.range.end => {
                last.range.end = last.range.end.max(edit.range.end);
            }
            _ => merged.push(edit),
        }
    }
    merged
}

/// `line` with every edit's range replaced by [`URL_CREDENTIAL_MASK`].
/// The edits are in order and do not overlap.
fn apply_credential_edits<'a>(line: &'a str, edits: &[CredentialEdit]) -> Cow<'a, str> {
    if edits.is_empty() {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&line[at..edit.range.start]);
        out.push_str(URL_CREDENTIAL_MASK);
        at = edit.range.end;
    }
    out.push_str(&line[at..]);
    Cow::Owned(out)
}

/// The proxy credentials in `line`, in order: every password in a
/// `://user:password@` URL, and every `proxy_password=VALUE` value
/// ([`merge_credential_edits`] decides their overlaps).
fn proxy_credential_edits(line: &str, source_path: &str) -> Vec<CredentialEdit> {
    let (passwords, key_values) = proxy_credential_parts(line, source_path);
    merge_credential_edits(line, passwords, key_values)
}

/// The URL passwords and the `proxy_password=` values in `line`, apart,
/// each in order.
fn proxy_credential_parts(
    line: &str,
    source_path: &str,
) -> (Vec<CredentialEdit>, Vec<CredentialEdit>) {
    let finding = |remediation: &str| RedactionFinding {
        path: source_path.to_string(),
        source: "proxy_credential".into(),
        kind: RedactionKind::Inline,
        pattern: "proxy_password".into(),
        remediation: remediation.to_string(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: Some(Confidence::High),
        finding_kind: Some(FindingKind::Password),
    };
    let passwords = credential_edits(
        &PROXY_CRED_RE,
        line,
        &finding(
            "Remove embedded credentials from proxy URL; use environment-specific auth configuration",
        ),
    );
    let key_value_finding = finding(
        "Remove plaintext proxy_password from DNF/Yum configuration; use repository-level auth instead",
    );
    let mut key_values = credential_edits(&PROXY_PASSWORD_KV_RE, line, &key_value_finding);
    // Only where the file's format reads a value to the end of its line,
    // as the INI end-of-line pattern is scoped: in a shell file or a
    // command, whitespace ends the value and what follows is the command,
    // which taking the rest of the line deleted.
    if value_runs_to_line_end(source_path) {
        key_values.extend(credential_edits(
            &PROXY_PASSWORD_LINE_RE,
            line,
            &key_value_finding,
        ));
    }
    (passwords, key_values)
}

/// The token user names in `line`, in order: every `://token@` URL, and
/// every token before a password, `://token:password@`.
fn token_username_edits(line: &str, source_path: &str) -> Vec<CredentialEdit> {
    let finding = RedactionFinding {
        path: source_path.to_string(),
        source: "token_credential".into(),
        kind: RedactionKind::Inline,
        pattern: "token_username".into(),
        remediation: "Remove embedded token from URL; use credential helpers or SSH keys"
            .to_string(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: Some(Confidence::High),
        finding_kind: Some(FindingKind::Password),
    };
    let mut edits = credential_edits(&TOKEN_USERNAME_RE, line, &finding);
    edits.extend(credential_edits(&TOKEN_WITH_PASSWORD_RE, line, &finding));
    edits.sort_by_key(|e| e.range.start);
    edits
}

/// Mask embedded credentials in a proxy URL line.
///
/// Replaces only the password segment in `://user:password@` patterns,
/// preserving the rest of the URL structure, and the value of a bare
/// `proxy_password=VALUE`. Every credential on the line is masked, each
/// with its own finding. This is a dedicated function because `PATTERNS`
/// has no rule for a password in an arbitrary URL's authority, only for
/// PostgreSQL, MongoDB, Redis and JDBC URLs, so `redact_string()`, which
/// also replaces only the secret, would not find this one.
///
/// Returns the line, borrowed when nothing matched, and one finding per
/// credential masked.
pub fn mask_proxy_credentials<'a>(
    line: &'a str,
    source_path: &str,
) -> (Cow<'a, str>, Vec<RedactionFinding>) {
    let edits = proxy_credential_edits(line, source_path);
    let masked = apply_credential_edits(line, &edits);
    (masked, edits.into_iter().map(|e| e.finding).collect())
}

/// Mask username-only token credentials in URLs.
///
/// Catches `://ghp_xxx@github.com`, `://glpat-xxx@gitlab.com`, and generic
/// long alphanumeric usernames (>= 20 chars) used as tokens, with no
/// password or before one (`://token:x-oauth-basic@`, `://token:@`), which
/// `mask_proxy_credentials()` masks apart. Every token on the line is
/// masked, each with its own finding.
///
/// Returns the line, borrowed when nothing matched, and one finding per
/// token masked.
pub fn mask_token_username<'a>(
    line: &'a str,
    source_path: &str,
) -> (Cow<'a, str>, Vec<RedactionFinding>) {
    let edits = token_username_edits(line, source_path);
    let masked = apply_credential_edits(line, &edits);
    (masked, edits.into_iter().map(|e| e.finding).collect())
}

/// Whether `path` is a shadow file, whose hashes [`scan_shadow`] reports.
fn is_shadow_path(path: &str) -> bool {
    path.ends_with("/shadow") || path == "/etc/shadow"
}

/// Scan content for secrets. Returns findings for all detected patterns.
pub fn scan_content(content: &str, path: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();

    // Shadow-specific logic for /etc/shadow paths
    if is_shadow_path(path) {
        findings.extend(scan_shadow(content, path));
    }

    // Generic pattern matching: collect from all patterns, then dedup
    // overlaps. The kernel command line also finds credentials by name.
    let all_matches = if is_kernel_cmdline(Some(path)) {
        kernel_cmdline_matches(content)
    } else {
        let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
        for (idx, pat) in PATTERNS.iter().enumerate() {
            for em in collect_eligible_matches(pat, content, Some(path)) {
                all_matches.push((idx, em));
            }
        }
        dedup_overlapping_matches(&mut all_matches);
        all_matches
    };
    for (pat_idx, em) in all_matches {
        findings.push(pattern_finding(&PATTERNS[pat_idx], em.line_num, path));
    }

    findings
}

/// Redact inline secrets from fstab mount options.
///
/// Replaces `password=<value>` with `password=REDACTED_MOUNT_PASSWORD_N`
/// and `credentials=<value>` containing inline passwords similarly.
/// Other options are preserved verbatim.
fn redact_mount_options(options: &str, registry: &mut CounterRegistry) -> String {
    options
        .split(',')
        .map(|opt| {
            if let Some(value) = opt.strip_prefix("password=") {
                let token = registry.token_for("mount_password", value);
                format!("password={token}")
            } else if let Some(value) = opt.strip_prefix("credentials=") {
                // Credential path references are flagged but the path itself
                // is not a secret — only redact if it looks like an inline
                // password rather than a file path (no leading /).
                if !value.starts_with('/') {
                    let token = registry.token_for("mount_credential", value);
                    format!("credentials={token}")
                } else {
                    opt.to_string()
                }
            } else {
                opt.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Extract a searchable keyword from a hint's reason string.
///
/// Hint reasons follow common patterns:
/// - "kernel cmdline contains sensitive parameter: rd.luks.key"
///   → extracts "rd.luks.key"
/// - "environment variable 'DB_PASSWORD' may contain a secret"
///   → extracts "DB_PASSWORD"
/// - "kernel cmdline contains key=" → extracts "key="
/// - "password detected" → extracts "password"
///
/// Returns `None` if no keyword can be extracted.
fn extract_hint_keyword(reason: &str) -> Option<String> {
    // Pattern 1: "sensitive parameter: <keyword>"
    if let Some(rest) = reason.strip_suffix('"').or(Some(reason))
        && let Some(idx) = rest.find("parameter: ")
    {
        let kw = rest[idx + "parameter: ".len()..].trim().trim_matches('"');
        if !kw.is_empty() {
            return Some(kw.to_string());
        }
    }

    // Pattern 2: "contains key=" or "contains <word>="
    if let Some(idx) = reason.find("contains ") {
        let rest = reason[idx + "contains ".len()..].trim();
        // Take the first token (up to whitespace or end)
        let token = rest.split_whitespace().next().unwrap_or(rest);
        let token = token.trim_matches('"').trim_matches('\'');
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    // Pattern 3: "variable 'NAME'" or "variable NAME"
    if let Some(idx) = reason.find("variable ") {
        let rest = reason[idx + "variable ".len()..].trim();
        let token = rest.split_whitespace().next().unwrap_or(rest);
        let token = token.trim_matches('\'').trim_matches('"');
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    // Pattern 4: first recognized sensitive keyword in the reason
    for kw in ["password", "secret", "token", "credential", "key="] {
        if reason.to_lowercase().contains(kw) {
            return Some(kw.to_string());
        }
    }

    None
}

/// Redact one content-bearing field in place: the pass every field of
/// host text takes. Returns the findings, and leaves `content` untouched
/// when there are none.
///
/// Every edit is found in the original text before any is applied, as
/// [`redact_snapshot_text`] finds them: the pattern pass
/// ([`pattern_edits`]) and the URL credential maskers
/// ([`url_credential_edits`]). The patterns cover a password in a URL of
/// a few schemes only, so a field without the URL pass shipped
/// `HTTPS_PROXY=https://u:secret@host` whole. Overlapping edits become
/// one, under the pattern token ([`apply_payload_edits`]). A shadow file
/// also gets its hash findings, as [`scan_content`] reports them; its
/// hashes are replaced by the shadow pass.
fn scan_and_redact_field(
    content: &mut String,
    source_path: &str,
    registry: &mut CounterRegistry,
) -> Vec<RedactionFinding> {
    let mut findings = if is_shadow_path(source_path) {
        scan_shadow(content, source_path)
    } else {
        Vec::new()
    };
    let mut edits = pattern_edits(content, source_path, registry);
    let (url_edits, unedited) = url_credential_edits(content, source_path);
    findings.extend(unedited);
    edits.extend(url_edits);
    if !edits.is_empty() {
        let (text, kept) = apply_payload_edits(content, edits);
        *content = text;
        findings.extend(kept);
    }
    findings
}

/// The pattern pass's replacements in `content`, as edits of that text,
/// each carrying its match's finding: what [`scan_and_redact_field`]
/// replaces, its tokens minted in the same order.
fn pattern_edits(
    content: &str,
    source_path: &str,
    registry: &mut CounterRegistry,
) -> Vec<PayloadEdit> {
    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
    for (idx, pat) in PATTERNS.iter().enumerate() {
        for em in collect_eligible_matches(pat, content, Some(source_path)) {
            all_matches.push((idx, em));
        }
    }
    dedup_overlapping_matches(&mut all_matches);
    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
    all_matches
        .into_iter()
        .map(|(pat_idx, em)| {
            let pat = &PATTERNS[pat_idx];
            let kind_label = format!("{:?}", pat.finding_kind).to_lowercase();
            PayloadEdit {
                range: em.start..em.end,
                text: build_replacement(&em, content, registry, &kind_label),
                token: true,
                findings: vec![pattern_finding(pat, em.line_num, source_path)],
            }
        })
        .collect()
}

/// The edits the compose pass (`scrub_compose_secrets`) makes in
/// `content`, each read off its line of the original text. That pass
/// keys on its own name list and rewrites credentials in a URL of any
/// scheme. It rewrites line by line, so each line is scrubbed alone; a
/// line it only trims, or whose value is already its marker, is not an
/// edit, which keeps a secret-free compose file byte-identical. Line
/// endings are left as they are.
fn compose_scrub_edits(content: &str) -> Vec<PayloadEdit> {
    let mut edits = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let scrubbed = scrub_compose_secrets(body);
        if scrubbed != body && scrubbed != body.trim_end() {
            let (range, text) = line_edit(body, &scrubbed);
            edits.push(PayloadEdit {
                range: offset + range.start..offset + range.end,
                text,
                token: false,
                findings: Vec::new(),
            });
        }
        offset += line.len();
    }
    edits
}

/// The finding for a compose file whose credentials only the compose
/// pass (`scrub_compose_secrets`) replaced.
fn compose_secret_finding(path: &str) -> RedactionFinding {
    RedactionFinding {
        path: path.to_owned(),
        source: "compose_env".into(),
        kind: RedactionKind::Inline,
        pattern: "compose_secret_value".into(),
        remediation: "Move the secret out of the compose file: use a compose secret, an \
             env_file outside the image, or a systemd credential"
            .to_string(),
        line: None,
        replacement: None,
        detection_method: DetectionMethod::Pattern,
        confidence: Some(Confidence::High),
        finding_kind: Some(FindingKind::Password),
    }
}

/// A snapshot field that holds a whole file's text, which `redact`
/// redacts by that field's pass ([`redact_snapshot_text`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotText {
    /// An `.env` file's `content` in `non_rpm_software.env_files`.
    EnvFile,
    /// A compose file's `raw_content` in `containers.compose_files`.
    ComposeFile,
}

/// The pass `redact` runs on a `kind` field holding the text of the file
/// at `path`: the one function both `redact` and
/// [`is_redacted_copy_of`] call, so the two cannot drift apart.
///
/// Every edit is found in the original text before any is applied, so no
/// pass can hide a match from another: the pattern pass
/// ([`pattern_edits`]), the URL credential maskers
/// ([`url_credential_edits`]), which cover a password in a URL of any
/// scheme where the patterns cover a few, and for a compose file the
/// compose pass ([`compose_scrub_edits`]). Overlapping edits become one,
/// under the pattern token ([`apply_payload_edits`]). A compose pass that
/// rewrote a line before the patterns read it removed a private key's
/// `BEGIN` marker and left its body.
///
/// Every finding rides on its edit and is kept only when that edit's text
/// is what the file holds: a masker's rewrite merged into a pattern edit
/// is reported by the pattern's finding, under the token the file shows,
/// and a pattern token merged under another token is not reported, since
/// the file shows only the other. The compose pass emits no
/// finding of its own: when it alone replaced something, the file gets
/// one compose finding, so one secret is not reported twice and a file
/// with a secret is never reported as clean.
fn redact_snapshot_text(
    kind: SnapshotText,
    content: &mut String,
    path: &str,
    registry: &mut CounterRegistry,
) -> Vec<RedactionFinding> {
    let mut edits = pattern_edits(content, path, registry);
    let (url_edits, mut findings) = url_credential_edits(content, path);
    edits.extend(url_edits);
    let mut compose_scrubbed = false;
    if kind == SnapshotText::ComposeFile {
        let scrub_edits = compose_scrub_edits(content);
        compose_scrubbed = !scrub_edits.is_empty();
        edits.extend(scrub_edits);
    }
    if !edits.is_empty() {
        let (text, kept) = apply_payload_edits(content, edits);
        *content = text;
        findings.extend(kept);
    }
    if compose_scrubbed && findings.is_empty() {
        findings.push(compose_secret_finding(path));
    }
    findings
}

/// A placeholder's number, which depends on what else the snapshot holds.
static TOKEN_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(REDACTED_[A-Z0-9]+)_[0-9]+").expect("the token number pattern compiles")
});

/// Whether `copy`, a `kind` field `redact` redacted for the file at
/// `path`, is `original` as that pass redacts it: the snapshot read the
/// same text the caller holds. Placeholder numbers are not compared,
/// since `redact` numbers them across the whole snapshot.
pub fn is_redacted_copy_of(kind: SnapshotText, path: &str, copy: &str, original: &str) -> bool {
    let mut redacted = original.to_owned();
    redact_snapshot_text(kind, &mut redacted, path, &mut CounterRegistry::default());
    TOKEN_NUMBER.replace_all(&redacted, "$1") == TOKEN_NUMBER.replace_all(copy, "$1")
}

/// The index in [`PATTERNS`] of the PEM private-key block pattern.
static PRIVATE_KEY_PATTERN: LazyLock<usize> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .position(|pat| pat.finding_kind == FindingKind::PrivateKey)
        .expect("PATTERNS holds the private key pattern")
});

/// A private-key `BEGIN` marker, with the labels the private key pattern
/// reads.
static PRIVATE_KEY_BEGIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"-----BEGIN\s+(?:(?:[A-Z0-9]+ )*PRIVATE KEY|PGP (?:PRIVATE|SECRET) KEY BLOCK)-----")
        .expect("the private key marker pattern compiles")
});

/// An RFC 1421 or OpenPGP armor header line in a key body:
/// `Proc-Type: 4,ENCRYPTED`, `DEK-Info: ...`, `Version: ...`.
static KEY_HEADER_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]*[A-Za-z][A-Za-z0-9-]*:[ \t]").expect("the key header pattern compiles")
});

const PEM_BEGIN: &str = "-----BEGIN";
const PEM_END: &str = "-----END";
const PEM_DASHES: &str = "-----";

/// Whether `b` is in the base64 alphabet, padding included.
fn is_base64_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')
}

/// How many base64 characters `text` holds, wherever they are: key
/// material is counted in any spacing and with any separators.
fn base64_count(text: &str) -> usize {
    text.bytes().filter(|&b| is_base64_byte(b)).count()
}

/// Whether `b` may sit in a key body after a marker that nothing closes:
/// base64, a space or tab, or a backslash (a `\n` escape in a quoted
/// string).
fn is_body_byte(b: u8) -> bool {
    is_base64_byte(b) || matches!(b, b' ' | b'\t' | b'\r' | b'\\')
}

/// The 1-based line `offset` is on in `content`.
fn line_at(content: &str, offset: usize) -> usize {
    content[..offset].matches('\n').count() + 1
}

/// `range` in `content` without the whitespace at either end.
fn trimmed(content: &str, range: Range<usize>) -> Range<usize> {
    let text = &content[range.clone()];
    let start = range.start + (text.len() - text.trim_start().len());
    start..start + text.trim().len()
}

/// A private-key pattern match `em` narrowed to the text between its
/// markers, trimmed, or `None` when that text holds fewer than
/// [`CLOSED_KEY_MATERIAL_MIN`] base64 characters: no key is that small, so
/// the markers are delimiters (`{"begin":"-----BEGIN PRIVATE
/// KEY-----","end":"-----END PRIVATE KEY-----"}`). Anything else between
/// them is replaced, a key whose body is split by spaces or by other text
/// included. The match keeps its whole block as its claim, so it wins
/// over a credential inside it and takes that span too.
fn private_key_body(content: &str, em: EligibleMatch) -> Option<EligibleMatch> {
    let block = &content[em.claim.clone()];
    let after_begin =
        PEM_BEGIN.len() + block.get(PEM_BEGIN.len()..)?.find(PEM_DASHES)? + PEM_DASHES.len();
    let before_end = block.rfind(PEM_END)?;
    if before_end < after_begin
        || base64_count(&block[after_begin..before_end]) < CLOSED_KEY_MATERIAL_MIN
    {
        return None;
    }
    let body = trimmed(
        content,
        em.claim.start + after_begin..em.claim.start + before_end,
    );
    Some(EligibleMatch {
        start: body.start,
        end: body.end,
        ..em
    })
}

/// Key material after each private-key `BEGIN` marker that no block of
/// the private key pattern closes: a cut-off key, or one whose `END`
/// marker is missing. The body is the rest of the marker's line while it
/// holds only base64, spaces and backslashes, and, when that reaches the
/// line's end, each following line that holds only those or is a key
/// header. One holding [`CLOSED_KEY_MATERIAL_MIN`] base64 characters is
/// replaced; a marker standing alone, or followed by a quote or other
/// text, is not. Comment lines are read too.
fn unclosed_private_key_bodies(content: &str) -> Vec<EligibleMatch> {
    let pattern = &PATTERNS[*PRIVATE_KEY_PATTERN].regex;
    let closed: Vec<Range<usize>> = pattern.find_iter(content).map(|m| m.range()).collect();
    let mut found = Vec::new();
    for marker in PRIVATE_KEY_BEGIN.find_iter(content) {
        if closed
            .iter()
            .any(|block| block.start <= marker.start() && marker.end() <= block.end)
        {
            continue;
        }
        let mut end = marker.end();
        let first_line_end = line_break_at_or_after(content, end);
        end += content[end..first_line_end]
            .bytes()
            .position(|b| !is_body_byte(b))
            .unwrap_or(first_line_end - end);
        if end == first_line_end {
            while end < content.len() {
                let next_start = end + 1;
                let next_end = line_break_at_or_after(content, next_start).max(next_start);
                let line = &content[next_start.min(content.len())..next_end.min(content.len())];
                if line.bytes().all(is_body_byte) || KEY_HEADER_LINE.is_match(line) {
                    end = next_end.min(content.len());
                } else {
                    break;
                }
            }
        }
        let body = trimmed(content, marker.end()..end);
        if base64_count(&content[body.clone()]) >= CLOSED_KEY_MATERIAL_MIN {
            found.push(EligibleMatch {
                claim: marker.start()..body.end,
                start: body.start,
                end: body.end,
                line_num: line_at(content, marker.start()),
            });
        }
    }
    found
}

/// Every match a payload pass replaces in `content`, with overlaps
/// resolved: the one set both its findings and its replacements come
/// from. A certificate is public and left in place: a CA bundle or a
/// server certificate breaks with a placeholder in it. A private-key
/// block keeps its markers and has the text between them replaced,
/// unless that text holds fewer base64 characters than the smallest
/// whole key ([`CLOSED_KEY_MATERIAL_MIN`]): then the markers are
/// delimiters in code or data, and the match is left out. A pattern left
/// out is left out before overlaps are resolved, so a certificate or a
/// delimiter pair around a credential never takes that credential's place
/// and hides it.
fn payload_matches(content: &str, path: &str) -> Vec<(usize, EligibleMatch)> {
    let mut all: Vec<(usize, EligibleMatch)> = Vec::new();
    for (idx, pat) in PATTERNS.iter().enumerate() {
        if pat.finding_kind == FindingKind::Certificate {
            continue;
        }
        for em in collect_eligible_matches(pat, content, Some(path)) {
            if idx == *PRIVATE_KEY_PATTERN {
                all.extend(private_key_body(content, em).map(|em| (idx, em)));
            } else {
                all.push((idx, em));
            }
        }
    }
    all.extend(
        unclosed_private_key_bodies(content)
            .into_iter()
            .map(|em| (*PRIVATE_KEY_PATTERN, em)),
    );
    dedup_overlapping_matches(&mut all);
    all
}

/// The finding for a pattern match at `path`, as [`scan_content`]
/// records one.
fn pattern_finding(pat: &SecretPattern, line_num: usize, path: &str) -> RedactionFinding {
    RedactionFinding {
        path: path.to_string(),
        source: "pattern".into(),
        kind: RedactionKind::Inline,
        pattern: format!("{:?}", pat.finding_kind).to_lowercase(),
        remediation: pat.remediation.to_string(),
        line: i32::try_from(line_num).ok(),
        replacement: None,
        detection_method: pat.detection_method.clone(),
        confidence: Some(pat.confidence),
        finding_kind: Some(pat.finding_kind.clone()),
    }
}

/// One replacement in a payload's original text.
struct PayloadEdit {
    /// The bytes replaced, in the original text.
    range: Range<usize>,
    /// What replaces them.
    text: String,
    /// Whether `text` is a placeholder token from the pattern pass, rather
    /// than a masker's rewrite of the span, which keeps the bytes around
    /// the credential it masked.
    token: bool,
    /// The findings that name `text`'s placeholder: a pattern match's
    /// own, or a masker's. They are reported only when `text` is what the
    /// file ends up holding.
    findings: Vec<RedactionFinding>,
}

/// `content` with every edit applied once, and the findings of the edits
/// whose text it holds: the findings reported are exactly those whose
/// replacement is in the output. Edits that overlap become one edit over
/// all their bytes, replaced by a pattern token: a masker's rewrite keeps
/// text the pattern edit removes, so it never stands for the merged span.
/// Every byte any edit replaces is replaced. An edit whose text is not
/// written, a masker's or a second pattern token's, drops its findings,
/// which would name a placeholder the file does not hold.
fn apply_payload_edits(
    content: &str,
    mut edits: Vec<PayloadEdit>,
) -> (String, Vec<RedactionFinding>) {
    edits.sort_by_key(|e| (e.range.start, std::cmp::Reverse(e.range.end)));
    let mut merged: Vec<PayloadEdit> = Vec::new();
    for edit in edits {
        match merged.last_mut() {
            Some(last) if edit.range.start < last.range.end => {
                last.range.end = last.range.end.max(edit.range.end);
                if edit.token && !last.token {
                    last.text = edit.text;
                    last.token = true;
                    last.findings = edit.findings;
                }
            }
            _ => merged.push(edit),
        }
    }
    let mut out = String::with_capacity(content.len());
    let mut findings = Vec::new();
    let mut at = 0;
    for edit in merged {
        out.push_str(&content[at..edit.range.start]);
        out.push_str(&edit.text);
        findings.extend(edit.findings);
        at = edit.range.end;
    }
    out.push_str(&content[at..]);
    (out, findings)
}

/// The one edit that turns `line` into `masked`: the bytes between their
/// common prefix and their common suffix, on character boundaries in
/// both.
fn line_edit(line: &str, masked: &str) -> (Range<usize>, String) {
    let mut prefix = line
        .bytes()
        .zip(masked.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !(line.is_char_boundary(prefix) && masked.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let mut suffix = line
        .bytes()
        .rev()
        .zip(masked.bytes().rev())
        .take(line.len().min(masked.len()) - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    while !(line.is_char_boundary(line.len() - suffix)
        && masked.is_char_boundary(masked.len() - suffix))
    {
        suffix -= 1;
    }
    (
        prefix..line.len() - suffix,
        masked[prefix..masked.len() - suffix].to_owned(),
    )
}

/// The edits that mask a credential in a URL's user information on each
/// line of `content`, as the git remote pass masks a remote: every
/// password ([`mask_proxy_credentials`]) and every token standing as the
/// user name ([`mask_token_username`]). The generic patterns cover such a
/// password for a few schemes only. Each edit is read off the original
/// line, so it never decides what the pattern pass sees.
///
/// Each credential is its own edit, carrying its own finding
/// ([`apply_payload_edits`] keeps it only if the rewrite is written), so
/// a masker edit spans only the credential it masked and never joins two
/// pattern matches on one line. Overlapping credentials are one edit over
/// both ([`merge_credential_edits`]): a token match inside a password is
/// that password's credential and is not a second edit. A credential the
/// line already holds masked is not rewritten; its finding is returned
/// apart.
fn url_credential_edits(content: &str, path: &str) -> (Vec<PayloadEdit>, Vec<RedactionFinding>) {
    let mut edits = Vec::new();
    let mut unedited = Vec::new();
    let mut offset = 0;
    for (number, line) in content.split_inclusive('\n').enumerate() {
        let line_number = i32::try_from(number + 1).ok();
        let (mut url, key_values) = proxy_credential_parts(line, path);
        url.extend(token_username_edits(line, path));
        let found = merge_credential_edits(line, url, key_values);
        for CredentialEdit { range, finding } in found {
            let finding = RedactionFinding {
                line: line_number,
                ..finding
            };
            if &line[range.clone()] == URL_CREDENTIAL_MASK {
                unedited.push(finding);
                continue;
            }
            edits.push(PayloadEdit {
                range: offset + range.start..offset + range.end,
                text: URL_CREDENTIAL_MASK.to_owned(),
                token: false,
                findings: vec![finding],
            });
        }
        offset += line.len();
    }
    (edits, unedited)
}

/// Redact the text of a file only the bundle holds, at `path`. Every
/// match is found in the original text, before anything is replaced, so
/// no pass can hide a match from another: the pattern matches
/// ([`payload_matches`]) and the URL credential maskers
/// ([`url_credential_edits`]), all applied once. A
/// compose file by name then takes the compose pass
/// (`scrub_compose_secrets`) over that result, which can only replace
/// more. Path-scoped format rules apply as they do to a config file.
/// Returns the redacted text and its findings, or `None` when nothing in
/// it changed.
///
/// For an unmanaged payload the snapshot holds no copy of: one it does
/// hold ships that copy instead ([`is_redacted_copy_of`]). Its tokens are
/// numbered from 1 for this file alone, so the same secret elsewhere in
/// the snapshot can carry another number.
pub fn redact_payload(content: &str, path: &str) -> Option<(String, Vec<RedactionFinding>)> {
    let mut registry = CounterRegistry::default();
    let is_compose = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_compose_file_name);
    let mut matches = payload_matches(content, path);
    // Tokens are minted last match first, as every pattern pass mints
    // them.
    matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
    let mut edits: Vec<PayloadEdit> = matches
        .iter()
        .map(|(idx, em)| {
            let pat = &PATTERNS[*idx];
            let kind_label = format!("{:?}", pat.finding_kind).to_lowercase();
            PayloadEdit {
                range: em.start..em.end,
                text: build_replacement(em, content, &mut registry, &kind_label),
                token: true,
                findings: vec![pattern_finding(pat, em.line_num, path)],
            }
        })
        .collect();
    let (url_edits, mut findings) = url_credential_edits(content, path);
    edits.extend(url_edits);
    let (mut text, kept) = apply_payload_edits(content, edits);
    findings.extend(kept);
    if is_compose {
        let scrubbed = scrub_compose_secrets(&text);
        if scrubbed.matches(COMPOSE_REDACTED_MARKER).count()
            > text.matches(COMPOSE_REDACTED_MARKER).count()
        {
            text = scrubbed;
            if findings.is_empty() {
                findings.push(compose_secret_finding(path));
            }
        }
    }
    (text != content).then_some((text, findings))
}

/// Append findings recorded after `redact` ran, such as an unmanaged
/// payload's, and settle `redaction_state` again over the whole list, as
/// `redact` settles it.
pub fn record_late_findings(snapshot: &mut InspectionSnapshot, findings: Vec<RedactionFinding>) {
    if findings.is_empty() {
        return;
    }
    let mut all_findings = std::mem::take(&mut snapshot.redactions);
    all_findings.extend(findings);
    settle_redaction_state(snapshot, all_findings);
}

/// The path a sysctl runtime value is scanned under: it came from the
/// kernel's `/proc/sys` tree, not from a file.
const SYSCTL_RUNTIME_PATH: &str = "/proc/sys";

/// Scrub every host-text field of the sysctl section in place and return
/// the findings.
///
/// - A read file's `content` is scanned and replaced under the file's own
///   path; `redacted` records whether anything in it changed, which is
///   what makes `SysctlSection::effective()` report the file as unknown.
///   When a config entry at the same path held byte-identical text before
///   either pass scrubbed it (`config_originals`), the config pass has
///   already recorded these very matches, so a content finding equal to
///   one in `recorded` at that path is not returned again, one for one.
///   A finding carries no matched text, so equal findings alone do not
///   prove the same match: when the two reads differ (the file changed
///   between the collectors' reads), every finding is kept. The content
///   is scrubbed either way; only the second record of a match goes.
/// - `link_target` and `OutsideLink.target` are scanned as one line under
///   the entry's path. A path is not a line systemd-sysctl parses, so a
///   change there does not set `redacted`.
/// - Each runtime value is scanned as the line `<key> = <value>` under
///   [`SYSCTL_RUNTIME_PATH`]. On any finding the value is removed from
///   `runtime` and its key recorded in `redacted_runtime`: nothing reads a
///   scrubbed remainder, so none is stored, and no token is minted for it.
///
/// File names, directories and kernel key names are not scanned, for the
/// reason `ConfigFileEntry.path` is not.
fn redact_sysctl_section(
    section: &mut SysctlSection,
    registry: &mut CounterRegistry,
    recorded: &[RedactionFinding],
    config_originals: &HashMap<String, Vec<String>>,
) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();
    for file in &mut section.files {
        let path = file.path();
        match &mut file.body {
            SysctlFileBody::Mask | SysctlFileBody::Withheld { .. } => {}
            SysctlFileBody::OutsideLink { target } => {
                findings.extend(scan_and_redact_field(target, &path, registry));
            }
            SysctlFileBody::Read {
                link_target,
                content,
                redacted,
            } => {
                if let Some(target) = link_target {
                    findings.extend(scan_and_redact_field(target, &path, registry));
                }
                let before = content.clone();
                let content_findings = scan_and_redact_field(content, &path, registry);
                // Sticky: a later pass that changes nothing does not
                // make the file as read again.
                *redacted |= *content != before;
                let same_text_as_config = config_originals
                    .get(&path)
                    .is_some_and(|originals| originals.contains(&before));
                if !same_text_as_config {
                    findings.extend(content_findings);
                    continue;
                }
                // One finding per match per path: each recorded finding
                // at this path stands for one of ours, at most once.
                let mut unmatched: Vec<&RedactionFinding> =
                    recorded.iter().filter(|f| f.path == path).collect();
                findings.extend(content_findings.into_iter().filter(|finding| {
                    match unmatched.iter().position(|r| *r == finding) {
                        Some(i) => {
                            unmatched.swap_remove(i);
                            false
                        }
                        None => true,
                    }
                }));
            }
        }
    }

    let mut removed = Vec::new();
    section.runtime.retain(|key, value| {
        let line = format!("{} = {value}", key.as_str());
        let line_findings = scan_content(&line, SYSCTL_RUNTIME_PATH);
        if line_findings.is_empty() {
            return true;
        }
        findings.extend(line_findings);
        removed.push(key.clone());
        false
    });
    section.redacted_runtime.extend(removed);
    section.redacted_runtime.sort();
    section.redacted_runtime.dedup();
    findings
}

/// The unscrubbed text of each config entry whose path is also a read
/// sysctl file, by path. Only those paths are kept, so the clone stays
/// small.
fn config_originals_at_sysctl_paths(snapshot: &InspectionSnapshot) -> HashMap<String, Vec<String>> {
    let mut originals: HashMap<String, Vec<String>> = HashMap::new();
    let (Some(config), Some(sysctl)) = (
        snapshot.config.as_ref(),
        snapshot
            .kernel_boot
            .as_ref()
            .and_then(|kb| kb.sysctl.as_ref()),
    ) else {
        return originals;
    };
    let sysctl_paths: HashSet<String> = sysctl
        .files
        .iter()
        .filter(|f| matches!(f.body, SysctlFileBody::Read { .. }))
        .map(|f| f.path())
        .collect();
    for entry in &config.files {
        if sysctl_paths.contains(&entry.path) {
            originals
                .entry(entry.path.clone())
                .or_default()
                .push(entry.content.clone());
        }
    }
    originals
}

/// Redact a snapshot in place, setting its `redaction_state` and populating `redactions`.
///
/// - Scans config file contents and shadow entries for secrets.
/// - High-confidence findings are auto-resolved (content redacted inline).
/// - Low-confidence findings remain unresolved → PartiallyRedacted state.
/// - If all findings are high-confidence and resolved → FullyRedacted state.
pub fn redact(snapshot: &mut InspectionSnapshot, _opts: &RedactOptions) {
    let mut all_findings: Vec<RedactionFinding> = Vec::new();
    let mut registry = CounterRegistry::default();
    // Taken before the config pass scrubs anything: the sysctl pass needs
    // the text as read to know whether a config entry is the same copy.
    let config_originals = config_originals_at_sysctl_paths(snapshot);
    // A `.repo` file's Configuration row holds the same URLs as its repo
    // inventory entry, and both reach the snapshot.
    let repo_files: HashSet<Destination> = snapshot
        .rpm
        .iter()
        .flat_map(|rpm| &rpm.repo_files)
        .filter_map(|f| Destination::new(&f.path).ok())
        .collect();

    // Scan config file contents
    if let Some(ref mut config) = snapshot.config {
        for file in &mut config.files {
            if Destination::new(&file.path).is_ok_and(|d| repo_files.contains(&d))
                && let Some((scrubbed, finding)) = scrub_repo_urls(&file.content, &file.path)
            {
                file.content = scrubbed;
                all_findings.push(finding);
            }
            all_findings.extend(scan_and_redact_field(
                &mut file.content,
                &file.path,
                &mut registry,
            ));
        }
    }

    // Scan repo file contents for embedded credentials
    if let Some(ref mut rpm) = snapshot.rpm {
        for repo_file in &mut rpm.repo_files {
            let before = repo_file.content.clone();
            if let Some((scrubbed, finding)) = scrub_repo_urls(&repo_file.content, &repo_file.path)
            {
                repo_file.content = scrubbed;
                all_findings.push(finding);
            }
            all_findings.extend(scan_and_redact_field(
                &mut repo_file.content,
                &repo_file.path,
                &mut registry,
            ));
            repo_file
                .redaction_changed
                .extend(sections_redaction_changed(&before, &repo_file.content));
        }

        // Scan GPG key contents
        for gpg_key in &mut rpm.gpg_keys {
            all_findings.extend(scan_and_redact_field(
                &mut gpg_key.content,
                &gpg_key.path,
                &mut registry,
            ));
        }
    }

    // Scan systemd drop-in contents in services section. The units
    // section's other rows hold no file content: a mount finding has no
    // copy of `What=`, which can hold a credential (the unit file's config
    // entry carries it, scrubbed by the config pass), and its
    // `mount_point` is a path, classified as an identifier.
    if let Some(ref mut services) = snapshot.units {
        for drop_in in &mut services.drop_ins {
            all_findings.extend(scan_and_redact_field(
                &mut drop_in.content,
                &drop_in.path,
                &mut registry,
            ));
        }
    }

    // Scan fstab mount options and credential refs in storage section.
    // CRITICAL: Actually redact inline secrets from entry.options so they
    // don't survive into exported artifacts (snapshot JSON, audit report, HTML).
    if let Some(ref mut storage) = snapshot.storage {
        for entry in &mut storage.fstab_entries {
            if entry.options.contains("credentials=") || entry.options.contains("password=") {
                // Redact inline password= values from mount options
                let redacted_options = redact_mount_options(&entry.options, &mut registry);
                all_findings.push(RedactionFinding {
                    path: "/etc/fstab".to_string(),
                    source: "mount_options".into(),
                    kind: RedactionKind::Inline,
                    pattern: "credential_mount_option".into(),
                    remediation: format!(
                        "Mount point '{}': credential reference in mount options — use systemd credentials or a mount helper",
                        entry.mount_point
                    ),
                    line: None,
                    replacement: None,
                    detection_method: DetectionMethod::Heuristic,
                    confidence: Some(Confidence::High),
                    finding_kind: Some(FindingKind::GenericCredential),
                });
                entry.options = redacted_options;
            }
        }
        for cred in &storage.credential_refs {
            all_findings.push(RedactionFinding {
                path: "/etc/fstab".to_string(),
                source: "credential_ref".into(),
                kind: RedactionKind::Flagged,
                pattern: "credential_file_ref".into(),
                remediation: format!(
                    "Mount point '{}': references credential file '{}' — ensure it is not included in snapshot",
                    cred.mount_point, cred.credential_path
                ),
                line: None,
                replacement: None,
                detection_method: DetectionMethod::PathBased,
                confidence: Some(Confidence::Medium),
                finding_kind: Some(FindingKind::GenericCredential),
            });
        }
    }

    // Scan the kernel cmdline in the kernelboot section
    if let Some(ref mut kernelboot) = snapshot.kernel_boot {
        // Scan cmdline for password=, key=, secret= etc.
        let cmdline_findings = scan_content(&kernelboot.cmdline, KERNEL_CMDLINE_PATH);
        if !cmdline_findings.is_empty() {
            kernelboot.cmdline = redact_kernel_cmdline(&kernelboot.cmdline, &mut registry);
            all_findings.extend(cmdline_findings);
        }

        // The sysctl section: vendor, runtime and link-read files and
        // kernel values that no other pass sees.
        if let Some(ref mut sysctl) = kernelboot.sysctl {
            let sysctl_findings =
                redact_sysctl_section(sysctl, &mut registry, &all_findings, &config_originals);
            all_findings.extend(sysctl_findings);
        }

        // The dracut, modprobe, modules-load and tuned files are
        // `config.files` entries, scanned by the config pass above —
        // which exempts comment lines, as it does for every config file.
    }

    // Scan proxy lines for embedded credentials (dedicated masker, NOT generic pass).
    if let Some(ref mut network) = snapshot.network {
        for entry in &mut network.proxy {
            let (masked, findings) = mask_proxy_credentials(&entry.line, &entry.source);
            if !findings.is_empty() {
                entry.line = masked.into_owned();
                all_findings.extend(findings);
            }
        }
    }

    // Scan container env vars for secrets: each entry is its own field,
    // with its own token numbering.
    if let Some(ref mut containers) = snapshot.containers {
        for container in &mut containers.running_containers {
            let source_path = format!("container:{}", container.name);
            for env_entry in &mut container.env {
                all_findings.extend(scan_and_redact_field(
                    env_entry,
                    &source_path,
                    &mut CounterRegistry::default(),
                ));
            }
        }

        // Quadlet unit file content. This is raw systemd unit text, the
        // same thing `units.drop_ins[].content` holds and is scanned
        // for above, and it travels further than any other field here:
        // `configtree` writes it to `quadlet/<name>` and the renderer
        // emits `COPY quadlet/ /etc/containers/systemd/`, so an
        // unredacted `Environment=DB_PASSWORD=…` line is copied into the
        // built image at a path systemd reads at boot. The quadlet
        // collector emits no redaction hints either, so nothing else
        // holds the state down for it.
        for unit in &mut containers.quadlet_units {
            let source_path = if unit.path.is_empty() {
                format!("quadlet:{}", unit.name)
            } else {
                unit.path.clone()
            };
            all_findings.extend(scan_and_redact_field(
                &mut unit.content,
                &source_path,
                &mut registry,
            ));
        }

        // Compose file content: see `redact_snapshot_text`.
        for compose in &mut containers.compose_files {
            let Some(raw) = compose.raw_content.as_mut() else {
                continue;
            };
            all_findings.extend(redact_snapshot_text(
                SnapshotText::ComposeFile,
                raw,
                &compose.path,
                &mut registry,
            ));
        }
    }

    // Scan scheduled task commands and service-unit content blobs for secrets.
    if let Some(ref mut sched) = snapshot.scheduled_tasks {
        // GeneratedTimerUnit.command fields (cron commands converted to timers)
        for unit in &mut sched.generated_timer_units {
            all_findings.extend(scan_and_redact_field(
                &mut unit.command,
                &unit.source_path,
                &mut registry,
            ));

            // GeneratedTimerUnit.service_content — raw unit file text that
            // may contain secrets in ExecStart= or Environment= lines.
            if !unit.service_content.is_empty() {
                let svc_path = format!("generated:{}.service", unit.name);
                all_findings.extend(scan_and_redact_field(
                    &mut unit.service_content,
                    &svc_path,
                    &mut registry,
                ));
            }

            // GeneratedTimerUnit.timer_content — the sibling field of
            // service_content above, and written to the same place:
            // `configtree` stages it as
            // `config/etc/systemd/system/<name>.timer`, which the config
            // tree COPYs into the image. A `.timer` carries schedule
            // rather than ExecStart, so the secret density is lower than
            // a `.service`, but the destination is identical.
            if !unit.timer_content.is_empty() {
                let timer_path = format!("generated:{}.timer", unit.name);
                all_findings.extend(scan_and_redact_field(
                    &mut unit.timer_content,
                    &timer_path,
                    &mut registry,
                ));
            }
        }

        // AtJob.command fields
        for at_job in &mut sched.at_jobs {
            all_findings.extend(scan_and_redact_field(
                &mut at_job.command,
                &at_job.file,
                &mut registry,
            ));
        }

        // SystemdTimer.exec_start fields
        for timer in &mut sched.systemd_timers {
            let source_path = if timer.path.is_empty() {
                format!("timer:{}", timer.name)
            } else {
                timer.path.clone()
            };
            all_findings.extend(scan_and_redact_field(
                &mut timer.exec_start,
                &source_path,
                &mut registry,
            ));

            // SystemdTimer.description — the unit's `Description=`,
            // parsed out into its own field. Classified exempt as
            // "prose, not a value" and measured otherwise: a `.timer`
            // has no ExecStart= or Environment=, so `Description=` is
            // the one line in it that carries free host-authored text,
            // and it reaches both the snapshot JSON and the rendered
            // audit report as a table cell.
            if !timer.description.is_empty() {
                let desc_path = if timer.path.is_empty() {
                    format!("timer:{}", timer.name)
                } else {
                    timer.path.clone()
                };
                all_findings.extend(scan_and_redact_field(
                    &mut timer.description,
                    &desc_path,
                    &mut registry,
                ));
            }
        }
    }

    // Scan non-RPM software surfaces: .env file content and git remote URLs.
    if let Some(ref mut nrs) = snapshot.non_rpm_software {
        // .env file content (reuses ConfigFileEntry with .content field)
        for env_file in &mut nrs.env_files {
            all_findings.extend(redact_snapshot_text(
                SnapshotText::EnvFile,
                &mut env_file.content,
                &env_file.path,
                &mut registry,
            ));
        }

        // Git remote URLs — scan for embedded credentials (user:pass@host
        // and token-as-username ://token@host patterns)
        for item in &mut nrs.items {
            // Language-package manifests (requirements.txt, package.json,
            // package-lock.json, Gemfile, Gemfile.lock) carry registry URLs
            // that can embed auth credentials. Scrub them on the snapshot
            // object here rather than where each export path writes its
            // sidecar: the snapshot itself is what goes into
            // inspection-snapshot.json, the tarball, and `aggregate`, so
            // owning it in redact() is what makes every export path inherit
            // one decision instead of each one re-solving it.
            //
            // The generic regex pass is deliberately not used here: PATTERNS
            // has no `https://user:pass@host` rule (only postgres/mongodb/
            // redis/jdbc), so it would neither detect nor scrub a private
            // registry credential. scrub_manifest_auth is the authority for
            // this content.
            for (filename, content) in item.manifest_files.iter_mut() {
                let manifest_path = format!("{}/{}", item.path.trim_end_matches('/'), filename);
                let outcome = scrub_manifest_auth(filename, content);

                // Detect on the marker, not on string inequality: a
                // scrubber rebuilds the lines it handles, so the string
                // can differ without anything having been removed.
                // `scrub_url_auth` is the only operation in any of the
                // five scrubbers that removes a credential and it always
                // leaves this marker, so a rise in the marker count is
                // exactly "scrub_url_auth fired at least once." It is
                // not on its own a guarantee the manifest is clean; the
                // unlocatable report below is what covers the rest.
                if outcome.content.matches(URL_AUTH_REDACTED_MARKER).count()
                    > content.matches(URL_AUTH_REDACTED_MARKER).count()
                {
                    all_findings.push(RedactionFinding {
                        path: manifest_path.clone(),
                        source: "manifest_auth".into(),
                        kind: RedactionKind::Inline,
                        pattern: "manifest_registry_auth".into(),
                        remediation:
                            "Remove embedded credentials from the registry URL; use a credential \
                             helper or a machine-local netrc instead of inlining them in the manifest"
                                .to_string(),
                        line: None,
                        replacement: None,
                        detection_method: DetectionMethod::Pattern,
                        confidence: Some(Confidence::High),
                        finding_kind: Some(FindingKind::Password),
                    });
                }

                // A line that matched a key and whose value the scrubber
                // could not then locate. Low confidence on purpose: the
                // state machine treats anything below High as unresolved,
                // which is what downgrades the snapshot out of
                // `FullyRedacted`. That is the whole point — the label
                // the import path trusts must stop being applied over
                // content nothing managed to examine.
                for miss in &outcome.unscrubbed {
                    all_findings.push(RedactionFinding {
                        path: manifest_path.clone(),
                        source: "manifest_auth".into(),
                        kind: RedactionKind::Flagged,
                        pattern: "manifest_value_not_located".into(),
                        remediation: format!(
                            "Line {}: the `{}` value could not be located, so it was not checked \
                             for credentials. Review this line by hand before sharing the export.",
                            miss.line, miss.key
                        ),
                        line: Some(miss.line as i32),
                        replacement: None,
                        detection_method: DetectionMethod::Heuristic,
                        confidence: Some(Confidence::Low),
                        finding_kind: Some(FindingKind::GenericCredential),
                    });
                }

                *content = outcome.content;
            }

            if !item.git_remote.is_empty() {
                let (masked, findings) = mask_proxy_credentials(&item.git_remote, &item.path);
                if !findings.is_empty() {
                    item.git_remote = masked.into_owned();
                    all_findings.extend(findings);
                }
                // Username-only token remotes: ://ghp_xxx@host, ://glpat-xxx@host
                let (masked, findings) = mask_token_username(&item.git_remote, &item.path);
                if !findings.is_empty() {
                    item.git_remote = masked.into_owned();
                    all_findings.extend(findings);
                }
                // Also run generic pattern scan for other secret patterns
                let findings = scan_content(&item.git_remote, &item.path);
                if !findings.is_empty() {
                    let mut content = item.git_remote.clone();
                    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                    for (idx, pat) in PATTERNS.iter().enumerate() {
                        for em in collect_eligible_matches(pat, &content, Some(&item.path)) {
                            all_matches.push((idx, em));
                        }
                    }
                    dedup_overlapping_matches(&mut all_matches);
                    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                    for (pat_idx, em) in all_matches {
                        let kind_label =
                            format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                        let token = registry.token_for(&kind_label, em.secret(&content));
                        content.replace_range(em.start..em.end, &token);
                    }
                    item.git_remote = content;
                    all_findings.extend(findings);
                }
            }
        }
    }

    // Scan sudoers rules for secrets via generic pattern pass.
    // Commented rules (# prefix) still need redaction — a commented-out
    // password is still a secret in the file.
    if let Some(ref mut users) = snapshot.users_groups {
        for rule in &mut users.sudoers_rules {
            let redacted = redact_string(rule);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(rule, "/etc/sudoers");
                all_findings.extend(findings);
                *rule = new_val.clone();
            } else if let Some(body) = rule.strip_prefix('#') {
                // Comment-line filter skipped this — retry on the body.
                let body_trimmed = body.trim_start();
                let prefix_len = rule.len() - body_trimmed.len();
                let body_redacted = redact_string(body_trimmed);
                if let Cow::Owned(ref new_body) = body_redacted {
                    let findings = scan_content(body_trimmed, "/etc/sudoers");
                    all_findings.extend(findings);
                    *rule = format!("{}{}", &rule[..prefix_len], new_body);
                }
            }
        }
    }

    // Scan shadow entries in users_groups section.
    // When preserved_credentials is true, record findings but skip the
    // actual hash replacement — the operator chose to retain credentials.
    let skip_shadow_redaction = snapshot.preserved_credentials;
    if let Some(ref mut users) = snapshot.users_groups {
        // Collect scan results first, then mutate (avoids borrow conflict).
        let scan_results: Vec<(usize, Vec<RedactionFinding>)> = users
            .shadow_entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (i, scan_content(entry, "/etc/shadow")))
            .filter(|(_, findings)| !findings.is_empty())
            .collect();

        for (i, findings) in scan_results {
            if !skip_shadow_redaction {
                let mut content = users.shadow_entries[i].clone();
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High)
                        && finding.finding_kind == Some(FindingKind::ShadowHash)
                    {
                        // Replace the hash field with a redaction token
                        let fields: Vec<&str> = content.split(':').collect();
                        if fields.len() >= 3 {
                            let kind_label = "shadowhash";
                            let token = registry.token_for(kind_label, fields[1]);
                            let mut new_fields: Vec<String> =
                                fields.iter().map(|f| f.to_string()).collect();
                            new_fields[1] = token;
                            content = new_fields.join(":");
                        }
                    }
                }
                users.shadow_entries[i] = content;
            }
            all_findings.extend(findings);
        }
    }

    // Handle SSH keys in user objects.
    // When preserved_ssh_keys is true, skip redaction — the operator chose
    // to retain SSH keys. When false, remove the ssh_keys content entirely.
    // SSH public keys are not cryptographic secrets, but they can reveal
    // server access patterns and should be handled as sensitive data.
    if let Some(ref mut users) = snapshot.users_groups {
        for user in &mut users.users {
            if user.get("ssh_keys").is_some() && !snapshot.preserved_ssh_keys {
                // Strip SSH keys when not explicitly preserved
                if let Some(user_obj) = user.as_object_mut() {
                    user_obj.remove("ssh_keys");
                }
            }
            // When preserved_ssh_keys is true, keys are intentionally
            // retained and pass through unchanged.
        }
    }

    // Build a map of path → post-redaction content so that hint resolution
    // can check whether flagged sensitive content survived the regex pass.
    let mut post_redaction_content: HashMap<String, String> = HashMap::new();
    if let Some(ref config) = snapshot.config {
        for file in &config.files {
            post_redaction_content.insert(file.path.clone(), file.content.clone());
        }
    }
    if let Some(ref rpm) = snapshot.rpm {
        for repo_file in &rpm.repo_files {
            post_redaction_content.insert(repo_file.path.clone(), repo_file.content.clone());
        }
        for gpg_key in &rpm.gpg_keys {
            post_redaction_content.insert(gpg_key.path.clone(), gpg_key.content.clone());
        }
    }
    if let Some(ref services) = snapshot.units {
        for drop_in in &services.drop_ins {
            post_redaction_content.insert(drop_in.path.clone(), drop_in.content.clone());
        }
    }
    if let Some(ref kernelboot) = snapshot.kernel_boot {
        post_redaction_content.insert("/proc/cmdline".to_string(), kernelboot.cmdline.clone());
        // After the config files above, so an `/etc` sysctl file that also
        // has a config entry keeps that entry's content in its slot.
        if let Some(ref sysctl) = kernelboot.sysctl {
            for file in &sysctl.files {
                if let SysctlFileBody::Read { content, .. } = &file.body {
                    post_redaction_content
                        .entry(file.path())
                        .or_insert_with(|| content.clone());
                }
            }
        }
    }
    if let Some(ref sched) = snapshot.scheduled_tasks {
        for unit in &sched.generated_timer_units {
            post_redaction_content.insert(unit.source_path.clone(), unit.command.clone());
            if !unit.service_content.is_empty() {
                let svc_path = format!("generated:{}.service", unit.name);
                post_redaction_content.insert(svc_path, unit.service_content.clone());
            }
        }
        for at_job in &sched.at_jobs {
            post_redaction_content.insert(at_job.file.clone(), at_job.command.clone());
        }
        // An instance timer's description shares its template's `path`
        // (the scheduled inspector records it from the template), so two
        // descriptions can hold one path with different commands. Each
        // command is kept, so a hint at the path is resolved only when its
        // keyword survives in neither.
        for timer in &sched.systemd_timers {
            let source_path = if timer.path.is_empty() {
                format!("timer:{}", timer.name)
            } else {
                timer.path.clone()
            };
            post_redaction_content
                .entry(source_path)
                .and_modify(|content| {
                    content.push('\n');
                    content.push_str(&timer.exec_start);
                })
                .or_insert_with(|| timer.exec_start.clone());
        }
    }
    if let Some(ref nrs) = snapshot.non_rpm_software {
        for env_file in &nrs.env_files {
            post_redaction_content.insert(env_file.path.clone(), env_file.content.clone());
        }
        for item in &nrs.items {
            if !item.git_remote.is_empty() {
                post_redaction_content.insert(item.path.clone(), item.git_remote.clone());
            }
        }
    }

    // Convert inspector-emitted redaction hints into findings.
    // Hints represent inspector-detected content that may need redaction
    // but wasn't caught by pattern scanning (e.g., Environment=DB_KEY=value).
    //
    // HONESTY RULE (per-hint, not per-path): A high-confidence hint is
    // only "resolved" if the regex pass actually redacted THIS hint's
    // specific sensitive content. We verify by re-scanning the
    // post-redaction content at the hint's path: if scan_content still
    // finds zero regex matches AND the hint's path has no regex-sourced
    // findings, the hint is unresolved. If regex findings exist at the
    // path, we additionally check whether the hint's sensitive keyword
    // still survives in the post-redaction content — if it does, this
    // specific hint was not addressed by the regex pass.
    for hint in &snapshot.redaction_hints {
        // Suppress hints for allowlisted paths entirely — no finding,
        // no unresolved count increment, no PartiallyRedacted.
        if is_allowlisted_path(&hint.path) {
            continue;
        }

        let effective_confidence = if hint.confidence == Some(Confidence::High) {
            // Per-hint resolution: check if the hint's specific flagged
            // content still survives in the post-redaction content.
            let hint_resolved = if let Some(content) = post_redaction_content.get(&hint.path) {
                // Re-scan the redacted content. If it produces ANY regex
                // findings, the content still has secrets the regex covers.
                // But this hint is only resolved if the HINT's specific
                // sensitive content was part of what got redacted.
                //
                // Strategy: extract a keyword from the hint reason and
                // check if it still appears in the post-redaction content.
                // If the keyword survives → hint unresolved.
                let keyword = extract_hint_keyword(&hint.reason);
                if let Some(ref kw) = keyword {
                    // Every place the hint's keyword appears now carries,
                    // as its whole value, a token this run issued → the
                    // regex pass handled it.
                    !keyword_survives_with_its_value(
                        content,
                        kw,
                        &registry.issued,
                        HintValueEnd::for_path(&hint.path),
                    )
                } else {
                    // No extractable keyword — fall back to checking if
                    // ANY regex finding exists at this path.
                    all_findings
                        .iter()
                        .any(|f| f.source != "inspector_hint" && f.path == hint.path)
                }
            } else {
                // No post-redaction content available at this path →
                // the hint's path was never scanned → unresolved.
                false
            };

            if hint_resolved {
                Some(Confidence::High)
            } else {
                // The hint's secret was NOT regex-redacted → unresolved.
                Some(Confidence::Medium)
            }
        } else {
            hint.confidence
        };

        all_findings.push(RedactionFinding {
            path: hint.path.clone(),
            source: "inspector_hint".into(),
            kind: RedactionKind::Flagged,
            pattern: "inspector_hint".into(),
            remediation: hint.reason.clone(),
            line: None,
            replacement: None,
            detection_method: DetectionMethod::Heuristic,
            confidence: effective_confidence,
            finding_kind: Some(FindingKind::GenericCredential),
        });
    }

    settle_redaction_state(snapshot, all_findings);
}

/// Set `redaction_state` from `all_findings` and store them as the
/// snapshot's `redactions`.
fn settle_redaction_state(snapshot: &mut InspectionSnapshot, all_findings: Vec<RedactionFinding>) {
    let unresolved: Vec<&RedactionFinding> = all_findings
        .iter()
        .filter(|f| f.confidence != Some(Confidence::High))
        .collect();

    let config_hash = format!("{:x}", all_findings.len()); // simple hash for now
    let redacted_by = format!("inspectah {}", env!("CARGO_PKG_VERSION"));

    let unresolved_count = unresolved.len() as u32;
    let unresolved_hints: Vec<RedactionHint> = unresolved
        .iter()
        .map(|f| RedactionHint {
            path: f.path.clone(),
            reason: f.remediation.clone(),
            confidence: f.confidence,
        })
        .collect();

    if snapshot.sensitive_snapshot {
        // Operator chose to retain sensitive material — always SensitiveRetained.
        snapshot.redaction_state = Some(RedactionState::SensitiveRetained {
            redacted_by,
            config_hash,
            unresolved_count,
            unresolved_hints,
        });
    } else if all_findings.is_empty() || unresolved.is_empty() {
        // Nothing to redact, or all findings auto-resolved → fully clean
        snapshot.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by,
            config_hash,
        });
    } else {
        // Some findings need operator triage
        snapshot.redaction_state = Some(RedactionState::PartiallyRedacted {
            redacted_by,
            config_hash,
            unresolved_count,
            unresolved_hints,
        });
    }

    snapshot.redactions = all_findings;
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::FindingKind as SnapshotFindingKind;
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
    use inspectah_core::types::nonrpm::{NonRpmItem, NonRpmSoftwareSection};
    use inspectah_core::types::users::UserGroupSection;

    // -- scan_content tests (plan steps 1-3 + 5) --

    #[test]
    fn test_detect_private_key() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpA...\n-----END RSA PRIVATE KEY-----\n";
        let findings = scan_content(content, "/etc/ssl/private/key.pem");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_kind, Some(FindingKind::PrivateKey));
    }

    #[test]
    fn test_detect_password_in_config() {
        let content = "db_password = s3cretP@ss\n";
        let findings = scan_content(content, "/etc/myapp/config");
        assert!(!findings.is_empty());
    }

    #[test]
    fn test_shadow_locked_not_flagged() {
        let content = "root:!!:19000:0:99999:7:::\nnobody:*:19000:0:99999:7:::\n";
        let findings = scan_content(content, "/etc/shadow");
        // !! = locked, * = disabled -> neither is a secret
        assert!(
            findings.is_empty(),
            "locked/disabled accounts must not be flagged"
        );
    }

    #[test]
    fn test_shadow_hash_is_flagged() {
        let content = "admin:$6$rounds=65536$salt$hash...:19000:0:99999:7:::\n";
        let findings = scan_content(content, "/etc/shadow");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_kind, Some(FindingKind::ShadowHash));
    }

    #[test]
    fn test_shadow_empty_produces_low_confidence_finding() {
        let content = "nobody::19000:0:99999:7:::\n";
        let findings = scan_content(content, "/etc/shadow");
        // Empty password field MUST produce a low-confidence finding -- not silence.
        assert_eq!(
            findings.len(),
            1,
            "empty shadow must produce exactly one finding"
        );
        assert_eq!(findings[0].finding_kind, Some(FindingKind::NoPassword));
        assert_eq!(findings[0].confidence, Some(Confidence::Low));
    }

    // -- Cow<str> zero-copy test (plan step 6) --

    #[test]
    fn test_cow_no_clone_when_clean() {
        let clean = "no secrets here";
        let result = redact_string(clean);
        // Cow::Borrowed means no allocation
        assert!(matches!(result, Cow::Borrowed(_)));
    }

    #[test]
    fn test_cow_owned_when_secret_present() {
        let dirty = "password = hunter2";
        let result = redact_string(dirty);
        assert!(matches!(result, Cow::Owned(_)));
        assert!(!result.contains("hunter2"));
    }

    // -- Counter registry determinism --

    #[test]
    fn test_counter_same_value_same_token() {
        let mut reg = CounterRegistry::default();
        let t1 = reg.token_for("password", "hunter2");
        let t2 = reg.token_for("password", "hunter2");
        assert_eq!(t1, t2, "same secret value must produce same token");
    }

    #[test]
    fn test_counter_different_values_different_tokens() {
        let mut reg = CounterRegistry::default();
        let t1 = reg.token_for("password", "hunter2");
        let t2 = reg.token_for("password", "p@ssw0rd");
        assert_ne!(t1, t2, "different values must produce different tokens");
    }

    // -- Snapshot-level redaction tests (plan steps 4-5) --

    fn test_snapshot_with_empty_shadow() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            shadow_entries: vec![
                "root:!!:19000:0:99999:7:::".to_string(),
                "nobody::19000:0:99999:7:::".to_string(), // empty password
            ],
            ..Default::default()
        });
        snap
    }

    fn test_snapshot_with_known_secrets() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/config".to_string(),
                content: "db_password = s3cretP@ss\n".to_string(),
                ..Default::default()
            }],
        });
        snap
    }

    #[test]
    fn test_partially_redacted_with_guaranteed_unresolved() {
        // Fixture contains an empty-password shadow entry -> low-confidence finding
        // that cannot be auto-resolved without operator triage.
        let mut snapshot = test_snapshot_with_empty_shadow();
        redact(
            &mut snapshot,
            &RedactOptions {
                sensitivity: Sensitivity::Default,
            },
        );
        // UNCONDITIONAL assertion -- not gated on "if unresolved happen to exist"
        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count, ..
            }) => {
                assert!(*unresolved_count > 0, "empty shadow must remain unresolved");
            }
            other => panic!("expected PartiallyRedacted, got {other:?}"),
        }
    }

    #[test]
    fn test_fully_redacted_when_all_resolved() {
        let mut snapshot = test_snapshot_with_known_secrets();
        redact(
            &mut snapshot,
            &RedactOptions {
                sensitivity: Sensitivity::Default,
            },
        );
        match &snapshot.redaction_state {
            Some(RedactionState::FullyRedacted { redacted_by, .. }) => {
                assert!(redacted_by.contains("inspectah"));
            }
            other => panic!("expected FullyRedacted, got {other:?}"),
        }
    }

    #[test]
    fn test_redact_populates_findings() {
        let mut snapshot = test_snapshot_with_known_secrets();
        redact(&mut snapshot, &RedactOptions::default());
        assert!(
            !snapshot.redactions.is_empty(),
            "redactions vec must contain findings"
        );
    }

    #[test]
    fn test_redact_modifies_content_inline() {
        let mut snapshot = test_snapshot_with_known_secrets();
        redact(&mut snapshot, &RedactOptions::default());
        let config = snapshot.config.as_ref().unwrap();
        assert!(
            !config.files[0].content.contains("s3cretP@ss"),
            "secret must be redacted from config content"
        );
        assert!(
            config.files[0].content.contains("REDACTED_"),
            "redacted content must contain token"
        );
    }

    /// The `[server]` section of the `rhsm.conf` that subscription-manager
    /// ships on RHEL: the proxy credential keys are present with empty values.
    const STOCK_RHSM_CONF: &str = "\
# Red Hat Subscription Manager Configuration File:

# Unified Entitlement Platform Configuration
[server]
# Server hostname:
hostname = subscription.rhsm.redhat.com

# Server prefix:
prefix = /subscription

# Server port:
port = 443

# Set to 1 to disable certificate validation:
insecure = 0

# an http proxy server to use
proxy_hostname =

# The scheme to use for the proxy when updating repo definitions, if needed
# e.g. http or https
proxy_scheme = http

# port for http proxy server
proxy_port =

# user name for authenticating to an http proxy, if needed
proxy_user =

# password for basic http proxy auth, if needed
proxy_password =

# host/domain suffix blocklist for proxy, if needed
no_proxy =

[rhsm]
# Content base URL:
baseurl = https://cdn.redhat.com
";

    /// Keys of every `key = value` line in INI text, in order. Panics on a
    /// line that is not blank, a `#`/`;` comment, a `[section]` header or a
    /// line with a non-empty key before `=`: the line a match that ran
    /// across a newline left behind. It is not a model of `configparser`:
    /// it does not check that assignments sit inside a section, reject
    /// duplicate options, or accept `:` delimiters and continuation lines.
    /// Pair it with a byte comparison of the lines redaction should not
    /// touch.
    fn assignment_keys(text: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
                continue;
            }
            if t.starts_with('[') && t.ends_with(']') {
                continue;
            }
            let key = t
                .split_once('=')
                .map(|(k, _)| k.trim())
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| panic!("line {} is not valid INI: {line:?}", n + 1));
            keys.push(key.to_string());
        }
        keys
    }

    #[test]
    fn test_redact_leaves_stock_rhsm_conf_parseable() {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/rhsm/rhsm.conf".to_string(),
                content: STOCK_RHSM_CONF.to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snap, &RedactOptions::default());
        let out = &snap.config.as_ref().unwrap().files[0].content;
        assert_eq!(
            assignment_keys(out),
            assignment_keys(STOCK_RHSM_CONF),
            "redaction must keep every key line of rhsm.conf"
        );
        assert_eq!(
            out, STOCK_RHSM_CONF,
            "an empty credential value is not a secret: nothing to redact"
        );
    }

    #[test]
    fn test_credential_value_redaction_stays_on_its_line() {
        // A set value is still redacted, and only on its own line: the
        // blank line and comment after it survive.
        let content = "proxy_password = hunter2\n\n# host/domain suffix blocklist\nno_proxy =\n";
        let out = redact_string(content);
        assert!(!out.contains("hunter2"), "set value must be redacted");
        assert!(
            out.ends_with("\n\n# host/domain suffix blocklist\nno_proxy =\n"),
            "lines after the match must be untouched, got {out:?}"
        );
    }

    #[test]
    fn test_key_value_redaction_keeps_key_delimiter_and_spacing() {
        let wg_key = "A".repeat(43) + "=";
        let cases: Vec<(String, String)> = vec![
            (
                "DB_PASSWORD=x".into(),
                "DB_PASSWORD=REDACTED_PASSWORD_1".into(),
            ),
            (
                "proxy_password = hunter2".into(),
                "proxy_password = REDACTED_PASSWORD_1".into(),
            ),
            (
                "API_TOKEN: abc".into(),
                "API_TOKEN: REDACTED_PASSWORD_1".into(),
            ),
            (
                "Environment=API_TOKEN=abc".into(),
                "Environment=API_TOKEN=REDACTED_PASSWORD_1".into(),
            ),
            (
                "      POSTGRES_PASSWORD: pg123".into(),
                "      POSTGRES_PASSWORD: REDACTED_PASSWORD_1".into(),
            ),
            (
                "secret\t=\tabc".into(),
                "secret\t=\tREDACTED_PASSWORD_1".into(),
            ),
            ("psk=s3cretpsk".into(), "psk=REDACTED_WIFIPSK_1".into()),
            (
                "wpa-psk = s3cretpsk".into(),
                "wpa-psk = REDACTED_WIFIPSK_1".into(),
            ),
            (
                format!("PrivateKey = {wg_key}"),
                "PrivateKey = REDACTED_WIREGUARDKEY_1".into(),
            ),
            (
                "jdbc:mysql://db:3306/app?user=u&password=s3cret&ssl=1".into(),
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1".into(),
            ),
            (
                "aws_access_key_id = AKIAABCDEFGHIJKLMNOP\nregion = x".into(),
                "aws_access_key_id = REDACTED_AWSKEY_1\nregion = x".into(),
            ),
        ];
        for (input, want) in &cases {
            assert_eq!(redact_string(input), want.as_str(), "input {input:?}");
        }
    }

    #[test]
    fn test_redacted_rhsm_conf_keeps_proxy_password_key() {
        let content = STOCK_RHSM_CONF.replace("proxy_password =\n", "proxy_password = hunter2\n");
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/rhsm/rhsm.conf".to_string(),
                content: content.clone(),
                ..Default::default()
            }],
        });
        redact(&mut snap, &RedactOptions::default());
        let out = &snap.config.as_ref().unwrap().files[0].content;
        assert!(!out.contains("hunter2"), "the value must be redacted");
        assert_eq!(
            assignment_keys(out),
            assignment_keys(&content),
            "redaction must keep every key line of rhsm.conf"
        );
        for (got, want) in out.lines().zip(content.lines()) {
            if want == "proxy_password = hunter2" {
                assert_eq!(got, "proxy_password = REDACTED_PASSWORD_1");
            } else {
                assert_eq!(got, want, "only the proxy_password line changes");
            }
        }
        assert_eq!(out.lines().count(), content.lines().count());
    }

    #[test]
    fn test_hint_keyword_is_handled_only_where_its_value_is_a_token() {
        let mut registry = CounterRegistry::default();
        registry.token_for("password", "a");
        registry.token_for("password", "b");
        // (content, keyword, survives to the end of the line, survives to
        // the end of the kernel argument)
        let cases = [
            (
                "proxy_password = REDACTED_PASSWORD_1",
                "password",
                false,
                false,
            ),
            (
                "DB_PASSWORD=REDACTED_PASSWORD_2",
                "DB_PASSWORD",
                false,
                false,
            ),
            (
                "DB_PASSWORD=REDACTED_PASSWORD_2 # rotated",
                "DB_PASSWORD",
                false,
                false,
            ),
            (
                "DB_PASSWORD=REDACTED_PASSWORD_2 \t\r\nnext=1",
                "DB_PASSWORD",
                false,
                false,
            ),
            ("access_key=REDACTED_PASSWORD_1", "key=", false, false),
            // On the kernel command line `ro` is the next parameter; in a
            // `.env` file it is more of the value.
            (
                "quiet password=REDACTED_PASSWORD_1 ro",
                "password",
                true,
                false,
            ),
            ("password: files", "password", true, true),
            ("rd.luks.key=/path/to/key", "rd.luks.key", true, true),
            ("tokenizer = REDACTED_PASSWORD_1", "token", true, true),
            (
                "# password for basic http proxy auth",
                "password",
                true,
                true,
            ),
            (
                "a_password = REDACTED_PASSWORD_1\nb_password = left",
                "password",
                true,
                true,
            ),
            // A token followed by more of the value is not a replacement.
            ("DB_KEY=REDACTED_PASSWORD_1/hunter2", "DB_KEY", true, true),
            (
                "DB_KEY=REDACTED_PASSWORD_1:still-secret",
                "DB_KEY",
                true,
                true,
            ),
            ("password=REDACTED_PASSWORD_1&ssl=1", "password", true, true),
            (
                "DB_KEY=REDACTED_PASSWORD_1 still-secret",
                "DB_KEY",
                true,
                false,
            ),
            // Kernel argument quoting around a token is not more value.
            (
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\" quiet",
                "rd.iscsi.password",
                true,
                false,
            ),
            (
                "ro rd.iscsi.password=\"REDACTED_PASSWORD_1\"",
                "rd.iscsi.password",
                true,
                false,
            ),
            (
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1 bravo\"",
                "rd.iscsi.password",
                true,
                true,
            ),
            // Spelled like a token, but this run never issued it.
            ("DB_KEY=REDACTED_PASSWORD_3", "DB_KEY", true, true),
            ("DB_KEY=REDACTED_COMPANY_2026", "DB_KEY", true, true),
        ];
        for (content, kw, to_line_end, to_arg_end) in cases {
            for (end, survives) in [
                (HintValueEnd::EndOfLine, to_line_end),
                (HintValueEnd::KernelArgument, to_arg_end),
            ] {
                assert_eq!(
                    keyword_survives_with_its_value(content, kw, &registry.issued, end),
                    survives,
                    "{kw:?} in {content:?} under {end:?}"
                );
            }
        }
        assert_eq!(
            HintValueEnd::for_path(KERNEL_CMDLINE_PATH),
            HintValueEnd::KernelArgument
        );
        assert_eq!(
            HintValueEnd::for_path("opt/app/.env"),
            HintValueEnd::EndOfLine
        );
    }

    #[test]
    fn test_key_keeping_path_still_applies_false_positive_filters() {
        for input in [
            "password: files",
            "passwd: files sss",
            "password=files",
            "password: sufficient",
            "password=pam_unix.so",
            "password: FILES",
        ] {
            assert_eq!(
                redact_string(input),
                input,
                "a known NSS/PAM token is not a secret: {input:?}"
            );
            assert!(
                scan_content(input, "/etc/nsswitch.conf").is_empty(),
                "no finding for {input:?}"
            );
        }
    }

    #[test]
    fn test_uri_redaction_stays_on_its_line() {
        for nl in ["\n", "\r\n"] {
            // A password-less URI followed later by an `x: y@z` line is
            // not a credential, and nothing changes.
            let input =
                format!("url = postgres://db.example.com/app{nl}contact: ops@example.com{nl}");
            assert_eq!(redact_string(&input), input.as_str(), "{input:?}");

            // The line before a JDBC credential survives byte for byte,
            // and the credential is reported once, as JDBC.
            let first = "primary=jdbc:mysql://db/one";
            let input = format!("{first}{nl}secondary=jdbc:mysql://db/two?password=hunter2{nl}");
            let out = redact_string(&input).into_owned();
            assert_eq!(
                out,
                format!("{first}{nl}secondary=jdbc:mysql://REDACTED_JDBCPASSWORD_1{nl}"),
                "{input:?}"
            );
            let kinds: Vec<Option<FindingKind>> = scan_content(&input, "/etc/app.conf")
                .into_iter()
                .map(|f| f.finding_kind)
                .collect();
            assert_eq!(kinds, vec![Some(FindingKind::JdbcPassword)], "{input:?}");
        }
    }

    #[test]
    fn test_a_match_inside_another_patterns_claim_is_still_redacted() {
        // The PostgreSQL match runs from `postgres:` to the `@`, so its
        // whole match contains the AWS key id used as the user name, which
        // is outside its secret. Deciding overlaps on whole matches would
        // let the PostgreSQL match swallow the AWS match and ship the key.
        let input = "postgres://AKIAABCDEFGHIJKLMNOP:pw@db";
        let out = redact_string(input);
        assert_eq!(
            out,
            "postgres://REDACTED_AWSKEY_1:REDACTED_POSTGRESPASSWORD_1@db"
        );
        let mut kinds: Vec<Option<FindingKind>> = scan_content(input, "/etc/app.conf")
            .into_iter()
            .map(|f| f.finding_kind)
            .collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(
            kinds,
            vec![
                Some(FindingKind::AwsKey),
                Some(FindingKind::PostgresPassword)
            ],
            "one finding per secret"
        );
    }

    #[test]
    fn test_overlap_winner_absorbs_the_losers_secret() {
        let content = "0123456789abcdefghij";
        let em = |claim: Range<usize>, secret: Range<usize>| EligibleMatch {
            claim,
            start: secret.start,
            end: secret.end,
            line_num: 1,
        };
        // The larger claim wins; the smaller one's secret runs past the
        // winner's on the right, a third one's on the left, and a fourth
        // overlaps nothing.
        let mut matches = vec![
            (1, em(8..14, 12..18)),
            (0, em(0..20, 10..15)),
            (2, em(6..12, 7..11)),
            (3, em(18..20, 19..20)),
        ];
        dedup_overlapping_matches(&mut matches);
        let got: Vec<(usize, Range<usize>, &str)> = matches
            .iter()
            .map(|(p, em)| (*p, em.start..em.end, em.secret(content)))
            .collect();
        assert_eq!(got, vec![(0, 7..18, "789abcdefgh"), (3, 19..20, "j")]);
    }

    /// A candidate can overlap two kept secrets that do not overlap each
    /// other. All of them become one span, whichever side the winner is on
    /// and however long the chain of bridges is.
    #[test]
    fn test_a_bridge_between_kept_secrets_merges_all_of_them() {
        let em = |claim: Range<usize>, secret: Range<usize>| EligibleMatch {
            claim,
            start: secret.start,
            end: secret.end,
            line_num: 1,
        };
        type Case = (
            &'static str,
            Vec<(usize, EligibleMatch)>,
            Vec<(usize, Range<usize>)>,
        );
        let cases: Vec<Case> = vec![
            (
                "winner left of the bridge",
                vec![
                    (0, em(0..30, 2..5)),
                    (1, em(0..20, 8..11)),
                    (2, em(0..10, 4..9)),
                ],
                vec![(0, 2..11)],
            ),
            (
                "winner right of the bridge",
                vec![
                    (0, em(0..30, 8..11)),
                    (1, em(0..20, 2..5)),
                    (2, em(0..10, 4..9)),
                ],
                vec![(0, 2..11)],
            ),
            (
                "one bridge across three kept secrets",
                vec![
                    (0, em(0..30, 12..15)),
                    (1, em(0..25, 2..5)),
                    (2, em(0..20, 7..10)),
                    (3, em(0..5, 4..13)),
                    (4, em(0..3, 20..22)),
                ],
                vec![(0, 2..15), (4, 20..22)],
            ),
            (
                "a chain of bridges across four kept secrets",
                vec![
                    (0, em(0..35, 15..18)),
                    (1, em(0..34, 0..3)),
                    (2, em(0..33, 5..8)),
                    (3, em(0..32, 10..13)),
                    (4, em(0..12, 2..6)),
                    (5, em(0..11, 12..16)),
                    (6, em(0..10, 7..11)),
                ],
                vec![(0, 0..18)],
            ),
        ];
        for (name, mut matches, want) in cases {
            dedup_overlapping_matches(&mut matches);
            let got: Vec<(usize, Range<usize>)> = matches
                .iter()
                .map(|(p, em)| (*p, em.start..em.end))
                .collect();
            assert_eq!(got, want, "{name}");
        }
    }

    /// The two host strings that first showed a bridge: each has a match
    /// overlapping two secrets that were kept separately, and merging them
    /// in the wrong order panicked on every public path.
    #[test]
    fn test_a_match_bridging_two_secrets_is_redacted_on_every_path() {
        let cases = [
            (
                "db_password=postgres://u:a token=b@abcdefghijklmnopqrstuvwxyz",
                "db_password=REDACTED_PASSWORD_1",
                &["a token=b", "abcdefghijklmnopqrstuvwxyz"][..],
            ),
            (
                "postgres://longusername:token=a@redis://:b longsecret@h",
                "postgres://longusername:REDACTED_POSTGRESPASSWORD_1@h",
                &["token=a", "b longsecret", "redis"][..],
            ),
        ];
        for (input, want, secrets) in cases {
            assert_eq!(redact_string(input), want);
            assert_eq!(scan_content(input, "/etc/app.conf").len(), 1, "{input:?}");

            let mut snap = config_snapshot("/etc/app/app.conf", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(snap.config.as_ref().unwrap().files[0].content, want);
            let written = serde_json::to_string(&snap).unwrap();
            for secret in secrets {
                assert!(!written.contains(secret), "{secret:?} in {written}");
            }
            assert!(matches!(
                snap.redaction_state,
                Some(RedactionState::FullyRedacted { .. })
            ));
        }
    }

    /// Fragments of every URI and key pattern, each carrying the marker
    /// `zq` only where a secret goes, so that joining them at random
    /// builds the overlaps and bridges that hand-picked cases miss.
    const PROP_PIECES: &[&str] = &[
        "password=zqa",
        "token=zqb",
        "postgres://u:zqc",
        "redis://:zqd",
        "mongodb://u:zqe@h",
        "jdbc:mysql://u:zqf@db/app?password=zqg",
        "AKIAABCDEFGHIJKLMNOP",
        "psk=zqh",
        "password=\"zqj\"",
        "secret='zqk'",
        "\"token\": \"zql\"",
        "password: \"zqm zqn\"",
        "secret='zqo zqp'",
        "@h",
        "zqi",
    ];
    const PROP_SEPARATORS: &[&str] = &["", " ", "&", "@", ":", "\n", "\"", "'", ">", "<"];
    const PROP_MARKER: &str = "zq";
    /// A bridge needs a few pieces in a particular order. At the default
    /// 256 cases this test missed the merge panic in most runs; at this
    /// count it found it in most. The named bridge tests are the
    /// deterministic guard.
    const PROP_CASES: u32 = 10_000;

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(PROP_CASES))]

        /// Over random joins of the pieces above: matching and redaction
        /// never panic, the kept spans are disjoint and cover every
        /// eligible secret, and every marker inside an eligible secret is
        /// gone from the output while every other marker is still there.
        #[test]
        fn prop_redaction_covers_every_captured_secret_without_panicking(
            parts in proptest::collection::vec(
                (0..PROP_PIECES.len(), 0..PROP_SEPARATORS.len()),
                1..6,
            )
        ) {
            let input: String = parts
                .iter()
                .map(|&(p, s)| format!("{}{}", PROP_PIECES[p], PROP_SEPARATORS[s]))
                .collect();

            let mut matches: Vec<(usize, EligibleMatch)> = Vec::new();
            for (idx, pat) in PATTERNS.iter().enumerate() {
                for em in collect_eligible_matches(pat, &input, None) {
                    matches.push((idx, em));
                }
            }
            let secrets: Vec<Range<usize>> =
                matches.iter().map(|(_, em)| em.start..em.end).collect();
            dedup_overlapping_matches(&mut matches);
            for pair in matches.windows(2) {
                proptest::prop_assert!(pair[0].1.end <= pair[1].1.start, "{input:?}");
            }
            for s in &secrets {
                proptest::prop_assert!(
                    matches.iter().any(|(_, em)| em.start <= s.start && s.end <= em.end),
                    "secret {s:?} of {input:?} is not covered"
                );
            }

            let outside = input
                .match_indices(PROP_MARKER)
                .filter(|(i, m)| {
                    !secrets.iter().any(|s| *i < s.end && s.start < i + m.len())
                })
                .count();
            let out = redact_string(&input);
            proptest::prop_assert_eq!(
                out.matches(PROP_MARKER).count(),
                outside,
                "{:?} -> {:?}",
                input,
                out
            );
            scan_content(&input, "/etc/app.conf");
        }
    }

    /// Every eligible secret of `pat` in `content` as the unbounded search
    /// finds them: each match searched for from the previous secret's
    /// start, with no skipping and no cap. Slow on purpose; it is the
    /// reference the bounded search in `collect_eligible_matches` must
    /// cover.
    fn unbounded_eligible_secrets(
        pat: &SecretPattern,
        content: &str,
        path: Option<&str>,
    ) -> Vec<Range<usize>> {
        let markers = comment_markers(path);
        let mut secrets = Vec::new();
        let mut pos = 0;
        while pos <= content.len() {
            let Some(caps) = pat.regex.captures_at(content, pos) else {
                break;
            };
            let whole = caps.get(0).unwrap();
            let secret = caps.name(SECRET_GROUP).unwrap_or(whole);
            let mut lines = LineCursor::new(content);
            let headers_kept = path.is_some_and(is_repo_file_path);
            if let Some(em) = eligible_match(
                pat,
                content,
                &caps,
                path,
                &markers,
                headers_kept,
                &mut lines,
            ) {
                secrets.push(em.start..em.end);
            }
            pos = if secret.start() > whole.start() {
                secret.start()
            } else if whole.end() > whole.start() {
                whole.end()
            } else {
                whole.end()
                    + content[whole.end()..]
                        .chars()
                        .next()
                        .map_or(1, char::len_utf8)
            };
        }
        secrets
    }

    /// Asserts that the bounded search replaces every byte the unbounded
    /// one would, pattern by pattern. Returns true when it replaces exactly
    /// those bytes.
    fn assert_bounded_search_covers_unbounded(content: &str, path: Option<&str>) -> bool {
        let mut exact = true;
        for (idx, pat) in PATTERNS.iter().enumerate() {
            let mut bounded = vec![false; content.len()];
            for em in collect_eligible_matches(pat, content, path) {
                bounded[em.start..em.end].fill(true);
            }
            let mut unbounded = vec![false; content.len()];
            for s in unbounded_eligible_secrets(pat, content, path) {
                unbounded[s].fill(true);
            }
            for (b, (&u, &k)) in unbounded.iter().zip(&bounded).enumerate() {
                assert!(
                    !u || k,
                    "pattern {idx}: byte {b} of {content:?} is not covered"
                );
            }
            exact &= bounded == unbounded;
        }
        exact
    }

    /// Keys packed into one value make every key's secret the rest of the
    /// line, so resuming the search at each secret found thousands of
    /// matches and copied each one's text: about 3.5 GiB for
    /// `password=password=...x` under the collector's 256 KiB limit. Each
    /// input here is under that limit and must come out fully redacted, by
    /// `redact_string()` and by `redact()`, with a handful of matches kept
    /// and within a time bound. Unbounded, the first input took minutes in
    /// a release build. Bounded, each input takes about a second in a debug
    /// build, nearly all of it the regex crate's capture search over the
    /// one long match; the limit leaves room for a loaded machine running
    /// the suite in parallel.
    ///
    /// The last two values stop before the end of the line, so the keys
    /// inside them are each searched until the per-line bound replaces the
    /// rest of the line: ` zqtail` is replaced too, which is accepted.
    #[test]
    fn test_dense_repeated_keys_redact_in_bounded_time() {
        const COLLECTOR_LIMIT: usize = 256 * 1024;
        const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
        const MAX_KEPT: usize = 2;
        let cases = [
            (
                "password=".repeat(29_000) + "x",
                "password=REDACTED_PASSWORD_1",
                "password=password=",
            ),
            (
                "token=x&".repeat(32_768),
                "token=REDACTED_PASSWORD_1",
                "x&token=x",
            ),
            (
                "{".to_string() + &"\"password\":\"zq\",".repeat(15_000) + "\"zqtail\":1}",
                "{\"password\":REDACTED_PASSWORD_1",
                "zq",
            ),
            (
                "jdbc:mysql://".repeat(20_000) + "u:zq@db/zqtail",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                "zq",
            ),
            (
                "password=".repeat(29_000) + "zq zqtail",
                "password=REDACTED_PASSWORD_1",
                "zq",
            ),
            (
                "jdbc:mysql://".repeat(20_000) + "u:zq@db zqtail",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                "zq",
            ),
        ];
        for (input, want, probe) in cases {
            let name = format!("{}... ({} bytes)", &input[..20], input.len());
            assert!(
                input.len() <= COLLECTOR_LIMIT,
                "{name}: {} bytes",
                input.len()
            );
            let started = std::time::Instant::now();

            let kept: usize = PATTERNS
                .iter()
                .map(|pat| collect_eligible_matches(pat, &input, None).len())
                .sum();
            assert!(kept <= MAX_KEPT, "{name}: kept {kept} matches");

            assert_eq!(redact_string(&input), want, "{name}");

            let mut snap = config_snapshot("/etc/app/app.conf", &input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(
                snap.config.as_ref().unwrap().files[0].content,
                want,
                "{name}"
            );
            let written = serde_json::to_string(&snap).unwrap();
            assert!(!written.contains(probe), "{name}: {probe:?} survived");
            assert!(matches!(
                snap.redaction_state,
                Some(RedactionState::FullyRedacted { .. })
            ));

            let elapsed = started.elapsed();
            assert!(elapsed < TIME_LIMIT, "{name}: took {elapsed:?}");
        }
    }

    /// The per-line bound and the skip over a covered line replace at
    /// least what an unbounded search would, at every chain length on
    /// either side of the bound, after a comment marker, and with the line
    /// continuing past the value or on the next line. Below the bound they
    /// replace exactly what it would.
    #[test]
    fn test_bounded_search_covers_what_an_unbounded_one_finds() {
        let units = [
            "password=",
            "token=x&",
            "\"password\":\"zq\",",
            "jdbc:mysql://",
            "postgres://u:",
            "secret='zq'",
            "password=\"zq ",
        ];
        let ends = [
            "zq",
            "u:zq@h",
            "zq tail",
            "zq\npassword=zq",
            "zq \t",
            "zq\r\nx",
        ];
        let path = Some("/etc/app/app.conf");
        for unit in units {
            for n in 1..=(MAX_REDUNDANT_RESCANS_PER_LINE * 2 + 3) {
                for end in ends {
                    for lead in ["", "# ", "a=1 "] {
                        let input = format!("{lead}{}{end}", unit.repeat(n));
                        let exact = assert_bounded_search_covers_unbounded(&input, path);
                        assert!(
                            exact || n > MAX_REDUNDANT_RESCANS_PER_LINE,
                            "{input:?} replaced more than an unbounded search"
                        );
                    }
                }
            }
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(PROP_CASES))]

        /// Over long random joins of the pieces, long enough to pass the
        /// per-line bound, the bounded search replaces every byte the
        /// unbounded one would.
        #[test]
        fn prop_bounded_search_covers_what_an_unbounded_one_finds(
            parts in proptest::collection::vec(
                (0..PROP_PIECES.len(), 0..PROP_SEPARATORS.len()),
                1..40,
            )
        ) {
            let input: String = parts
                .iter()
                .map(|&(p, s)| format!("{}{}", PROP_PIECES[p], PROP_SEPARATORS[s]))
                .collect();
            assert_bounded_search_covers_unbounded(&input, None);
        }
    }

    /// A value spelled like a token is redacted like any other: the
    /// spelling does not show the engine wrote it. A second pass over
    /// redacted output therefore reports the tokens again. When it meets
    /// the same matches in the same order, as here, it rewrites each token
    /// to itself and the text does not change. That is this input's
    /// property, not a guarantee; the test after this one shows inputs
    /// where it does not hold.
    #[test]
    fn test_second_pass_rewrites_each_token_to_itself() {
        let input = "DB_PASSWORD=x\npsk=abc\npostgres://u:pw@db/app\n\
                     access_key=AKIAABCDEFGHIJKLMNOP\ntoken=y\nsecret=x\n";
        let once = redact_string(input).into_owned();
        assert_eq!(redact_string(&once), once.as_str());
        assert_eq!(
            scan_content(&once, "/etc/app.conf").len(),
            scan_content(input, "/etc/app.conf").len(),
            "every token is found again: {once:?}"
        );

        let mut snap = config_snapshot("/etc/app/a.conf", input);
        snap.config.as_mut().unwrap().files.push(ConfigFileEntry {
            path: "/etc/app/b.conf".to_string(),
            content: "password=y\npassword=z\n".to_string(),
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());
        let first = snap.config.clone();
        redact(&mut snap, &RedactOptions::default());
        assert_eq!(snap.config, first, "a second snapshot pass changes no text");
    }

    /// No production path runs a second pass, and a second pass is not
    /// guaranteed to return the same text. A token sits in a context the
    /// host's value did not have, and patterns can match it differently
    /// there: the generic `\S+` value runs from the token on through an
    /// `@db` the first pass left, and a JDBC URL reduced to its scheme has
    /// no `@` or `password=` left to be found. A second pass can change the
    /// text around a token and can report fewer findings; it never brings
    /// a secret back, as it only replaces.
    #[test]
    fn test_a_second_pass_is_not_guaranteed_to_return_the_same_text() {
        // (input, first pass, second pass, findings on the first pass's output)
        let cases = [
            (
                "password=postgres://u:p w@db",
                "password=REDACTED_PASSWORD_1@db",
                "password=REDACTED_PASSWORD_1",
                1,
            ),
            (
                "jdbc:mysql://u:pw@db/app?password=x",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                0,
            ),
        ];
        for (input, first, second, refound) in cases {
            let once = redact_string(input).into_owned();
            assert_eq!(once, first, "{input:?}");
            assert_eq!(redact_string(&once), second, "{input:?}");
            assert_eq!(
                scan_content(&once, "/etc/app.conf").len(),
                refound,
                "{once:?}"
            );
        }
    }

    #[test]
    fn test_redact_scans_repo_file_content() {
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        let mut snapshot = InspectionSnapshot::new();
        snapshot.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/custom.repo".into(),
                content: "password = s3cretP@ss\nbaseurl=https://repo.example.com/\n".into(),
                disposition: SnapshotFindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut snapshot, &RedactOptions::default());
        let rpm = snapshot.rpm.as_ref().unwrap();
        assert!(
            !rpm.repo_files[0].content.contains("s3cretP@ss"),
            "password in repo file must be redacted"
        );
        assert!(
            rpm.repo_files[0].content.contains("REDACTED_"),
            "repo file content must contain redaction token"
        );
        assert!(
            !snapshot.redactions.is_empty(),
            "redactions must contain findings from repo file"
        );
    }

    /// A `.repo` file's `[section]` header whose id dnf accepts is spared
    /// only the generic `key: value` pattern, which reads a COPR id's
    /// `token:` as a credential key, in the repo inventory and in its
    /// Configuration row; the values under it are still searched. A
    /// bracketed line that is not a repo id, and every other detector on a
    /// line that is, are searched as on any line, so a token in one is
    /// replaced. Another file's lines are searched whole.
    #[test]
    fn test_redact_spares_only_a_repo_id_header_from_the_generic_pattern() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        // `(header, kept as written, the secret it holds)`.
        let headers = [
            ("[copr:copr.fedorainfracloud.org:token:vendor]", true, ""),
            ("  [vendor-9.x_86_64:token:a]  ", true, ""),
            ("[{\"token\":\"tulip17\"}]", false, "tulip17"),
            ("  [password=hunter2]  ", false, "hunter2"),
            ("[api_key: k1 token:t2 secret=s3 token=tok5]", false, "tok5"),
            ("[AKIAIOSFODNN7EXAMPLE]", false, "AKIAIOSFODNN7EXAMPLE"),
        ];
        for (header, kept, secret) in headers {
            let content = format!("{header}\nbaseurl=https://v.example/\npassword=orchid92\n");
            for path in [
                "/etc/yum.repos.d/vendor.repo",
                "etc/yum.repos.d//vendor.repo",
            ] {
                let mut snapshot = InspectionSnapshot::new();
                snapshot.rpm = Some(RpmSection {
                    repo_files: vec![RepoFile {
                        path: path.into(),
                        content: content.clone(),
                        disposition: SnapshotFindingKind::included(),
                        ..Default::default()
                    }],
                    ..Default::default()
                });
                snapshot.config = Some(ConfigSection {
                    files: vec![ConfigFileEntry {
                        path: path.into(),
                        content: content.clone(),
                        ..Default::default()
                    }],
                    ..Default::default()
                });
                redact(&mut snapshot, &RedactOptions::default());
                for (what, got) in [
                    (
                        "repo",
                        &snapshot.rpm.as_ref().unwrap().repo_files[0].content,
                    ),
                    (
                        "config",
                        &snapshot.config.as_ref().unwrap().files[0].content,
                    ),
                ] {
                    assert_eq!(
                        got.starts_with(&format!("{header}\n")),
                        kept,
                        "{what} {path}: {got}"
                    );
                    assert!(!got.contains("orchid92"), "{what} {path}: {got}");
                    if !kept {
                        assert!(!got.contains(secret), "{what} {path}: {got}");
                    }
                }
            }
        }
        // Not a `.repo` file dnf reads: its lines are searched whole.
        let other = redact_string("[token:vendor]\n");
        assert_ne!(other, "[token:vendor]\n");
    }

    /// No secret that spans lines in a `.repo` file takes a repo-id
    /// header line: the parts of the secret on either side are replaced,
    /// the header and its line breaks stay, and no byte of the secret
    /// survives. A bracketed line that is no repo id goes with the secret,
    /// and a `.repo` file is recorded with the sections whose definition
    /// the pass changed. Another file's span is replaced whole.
    #[test]
    fn test_no_multi_line_secret_takes_a_repo_id_header() {
        use inspectah_core::types::repo::SectionId;
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        const CERT: (&str, &str) = ("-----BEGIN CERTIFICATE-----", "-----END CERTIFICATE-----");
        const KEY: (&str, &str) = (
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        );
        // `(what, content, headers kept, sections recorded changed)`.
        let cases: Vec<(&str, String, Vec<&str>, Vec<&str>)> = vec![
            (
                "a certificate across one header",
                format!(
                    "[metadata]\nenabled=0\nname={}\n[site]\nbaseurl=https://lower.example/\n\
                     enabled=1\ngpgcheck=0\nname={}\n",
                    CERT.0, CERT.1
                ),
                vec!["[metadata]", "[site]"],
                vec!["site"],
            ),
            (
                "a private key across two adjacent headers",
                format!(
                    "[a]\nname={}\nMIIEsecretA\n[b]\n[c]\nMIIEsecretB\nbaseurl=https://c.example/\n{}\n",
                    KEY.0, KEY.1
                ),
                vec!["[a]", "[b]", "[c]"],
                vec!["c"],
            ),
            (
                "CRLF line breaks",
                format!(
                    "[a]\r\nname={}\r\nMIIEsecretA\r\n[site]\r\nbaseurl=https://s.example/\r\n\
                     MIIEsecretB\r\n{}\r\n",
                    KEY.0, KEY.1
                ),
                vec!["[a]", "[site]"],
                vec!["site"],
            ),
            (
                "a bracketed line that is no repo id",
                format!(
                    "[a]\nname={}\n[not a repo]\nMIIEsecretA\n{}\n",
                    KEY.0, KEY.1
                ),
                vec!["[a]"],
                vec![],
            ),
        ];
        for (what, content, kept, changed) in cases {
            let mut snapshot = InspectionSnapshot::new();
            snapshot.rpm = Some(RpmSection {
                repo_files: vec![RepoFile {
                    path: "/etc/yum.repos.d/a.repo".into(),
                    content: content.clone(),
                    disposition: SnapshotFindingKind::included(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            redact(&mut snapshot, &RedactOptions::default());
            let file = &snapshot.rpm.as_ref().unwrap().repo_files[0];
            let got = &file.content;
            let lines: Vec<&str> = got.lines().map(|l| l.trim_end_matches('\r')).collect();
            for header in &kept {
                assert!(lines.contains(header), "{what}: {header} in {got:?}");
            }
            for secret in [
                "MIIEsecret",
                "BEGIN",
                "END",
                "lower.example",
                "s.example",
                "c.example",
            ] {
                assert!(!got.contains(secret), "{what}: {secret} in {got:?}");
            }
            assert!(!got.contains("[not a repo]"), "{what}: {got:?}");
            assert!(
                !got.contains("\n\r") && got.matches('\r').count() == got.matches("\r\n").count(),
                "{what}: line breaks kept whole: {got:?}"
            );
            let want: BTreeSet<SectionId> = changed.into_iter().map(SectionId::new).collect();
            assert_eq!(file.redaction_changed, want, "{what}");
        }

        // Not a `.repo` file: the span is replaced whole.
        let plain = format!("{}\n[site]\nx\n{}\n", CERT.0, CERT.1);
        let other = redact_string(&plain);
        assert!(!other.contains("[site]"), "{other}");
    }

    /// A credential in any URL of a `.repo` file, and of its Configuration
    /// row, is replaced in the authority, whatever option holds it, however
    /// many URLs a value lists and across continuation lines. The rest of
    /// each URL, and a URL with no credential, is left as written.
    #[test]
    fn test_redact_scrubs_credentials_in_repo_urls() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        const PATH: &str = "/etc/yum.repos.d/custom.repo";
        let content = "[custom]\n\
            baseurl=https://build:orchid92@repo.example.invalid/rpm/\n\
            \x20 https://mirror.example.invalid/rpm/, ftp://ftpuser:tulip17@ftp.example.invalid/rpm/\n\
            mirrorlist=https://ml-user:violet55@mirrors.example.invalid/list?arch=x86_64\n\
            metalink=https://ghp_0123456789abcdefghijklmnopqrstuvwxyzAB@meta.example.invalid/m\n\
            gpgkey=https://keys:p@ss@keys.example.invalid/RPM-GPG-KEY file:///etc/pki/rpm-gpg/K\n";
        let mut snapshot = InspectionSnapshot::new();
        snapshot.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: PATH.into(),
                content: content.into(),
                disposition: SnapshotFindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snapshot.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "etc/yum.repos.d//custom.repo".into(),
                content: content.into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut snapshot, &RedactOptions::default());
        let expected = "[custom]\n\
            baseurl=https://REDACTED@repo.example.invalid/rpm/\n\
            \x20 https://mirror.example.invalid/rpm/, ftp://REDACTED@ftp.example.invalid/rpm/\n\
            mirrorlist=https://REDACTED@mirrors.example.invalid/list?arch=x86_64\n\
            metalink=https://REDACTED@meta.example.invalid/m\n\
            gpgkey=https://REDACTED@keys.example.invalid/RPM-GPG-KEY file:///etc/pki/rpm-gpg/K\n";
        assert_eq!(
            snapshot.rpm.as_ref().unwrap().repo_files[0].content,
            expected
        );
        assert_eq!(snapshot.config.as_ref().unwrap().files[0].content, expected);
        let repo_findings = snapshot
            .redactions
            .iter()
            .filter(|f| f.pattern == "url_credential")
            .count();
        assert_eq!(repo_findings, 2, "{:?}", snapshot.redactions);
        assert!(
            matches!(
                snapshot.redaction_state,
                Some(RedactionState::FullyRedacted { .. })
            ),
            "{:?}",
            snapshot.redaction_state
        );

        // A comma is userinfo, not a list separator, in every option.
        let mut comma = InspectionSnapshot::new();
        comma.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: PATH.into(),
                content: "[custom]\nmirrorlist=https://build:orchid,92@repo.example.invalid/list\n\
                          baseurl=https://a.example.invalid/r,https://u:tulip,17@b.example.invalid/r\n"
                    .into(),
                disposition: SnapshotFindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut comma, &RedactOptions::default());
        assert_eq!(
            comma.rpm.as_ref().unwrap().repo_files[0].content,
            "[custom]\nmirrorlist=https://REDACTED@repo.example.invalid/list\n\
             baseurl=https://a.example.invalid/r,https://REDACTED@b.example.invalid/r\n"
        );

        // An ordinary config file's URLs are left to the generic patterns.
        let mut other = InspectionSnapshot::new();
        other.config = Some(ConfigSection {
            files: vec![ConfigFileEntry {
                path: "/etc/app.conf".into(),
                content: "url=https://example.invalid/\n".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut other, &RedactOptions::default());
        assert!(other.redactions.is_empty());
    }

    #[test]
    fn test_redact_scans_gpg_key_content() {
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        let mut snapshot = InspectionSnapshot::new();
        snapshot.rpm = Some(RpmSection {
            gpg_keys: vec![RepoFile {
                path: "/etc/pki/rpm-gpg/RPM-GPG-KEY-custom".into(),
                content: "password = hunter2\n".into(),
                disposition: SnapshotFindingKind::included(),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut snapshot, &RedactOptions::default());
        let rpm = snapshot.rpm.as_ref().unwrap();
        assert!(
            !rpm.gpg_keys[0].content.contains("hunter2"),
            "password in gpg key content must be redacted"
        );
        assert!(
            rpm.gpg_keys[0].content.contains("REDACTED_"),
            "gpg key content must contain redaction token"
        );
    }

    // --- Inspector hint consumption tests ---

    #[test]
    fn test_pam_allowlist_suppresses_hints() {
        let mut snapshot = InspectionSnapshot::new();
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/etc/pam.d/password-auth".into(),
            reason: "file content may contain credentials (matched 'password')".into(),
            confidence: Some(Confidence::Medium),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        // The allowlisted hint must NOT produce a finding or unresolved state.
        assert!(
            snapshot.redactions.is_empty(),
            "allowlisted hint must not produce a finding"
        );
        match &snapshot.redaction_state {
            Some(RedactionState::FullyRedacted { .. }) => {}
            other => panic!("expected FullyRedacted when only hint is allowlisted, got {other:?}"),
        }
    }

    #[test]
    fn test_hints_converted_to_findings() {
        let mut snapshot = InspectionSnapshot::new();
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/etc/systemd/system/app.service.d/env.conf".into(),
            reason: "environment variable DB_PASSWORD may contain a secret".into(),
            confidence: Some(Confidence::Medium),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        // The hint must appear as a finding
        let hint_findings: Vec<&RedactionFinding> = snapshot
            .redactions
            .iter()
            .filter(|f| f.source == "inspector_hint")
            .collect();
        assert_eq!(
            hint_findings.len(),
            1,
            "inspector hint must be converted to a finding"
        );
        assert_eq!(
            hint_findings[0].confidence,
            Some(Confidence::Medium),
            "hint confidence must be preserved"
        );
    }

    #[test]
    fn test_hints_cause_partially_redacted() {
        let mut snapshot = InspectionSnapshot::new();
        // Medium-confidence hint = unresolvable by auto-redaction
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/etc/systemd/system/app.service.d/env.conf".into(),
            reason: "environment variable DB_KEY may contain a secret".into(),
            confidence: Some(Confidence::Medium),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count, ..
            }) => {
                assert!(
                    *unresolved_count > 0,
                    "medium-confidence hint must produce unresolved finding"
                );
            }
            other => {
                panic!("expected PartiallyRedacted due to medium-confidence hint, got {other:?}")
            }
        }
    }

    // --- Storage inline secret redaction tests (Fix A) ---

    #[test]
    fn test_fstab_password_redacted_from_options() {
        use inspectah_core::types::storage::{FstabEntry, StorageSection};
        let mut snapshot = InspectionSnapshot::new();
        snapshot.storage = Some(StorageSection {
            fstab_entries: vec![FstabEntry {
                device: "//server/share".into(),
                mount_point: "/mnt/smb".into(),
                fstype: "cifs".into(),
                options: "password=hunter2,uid=1000".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        redact(&mut snapshot, &RedactOptions::default());

        // The secret value must be removed from the options string
        let storage = snapshot.storage.as_ref().unwrap();
        assert!(
            !storage.fstab_entries[0].options.contains("hunter2"),
            "password value must be redacted from fstab options, got: {}",
            storage.fstab_entries[0].options
        );
        assert!(
            storage.fstab_entries[0]
                .options
                .contains("REDACTED_MOUNT_PASSWORD_"),
            "options must contain redaction token"
        );
        // Other options must survive
        assert!(
            storage.fstab_entries[0].options.contains("uid=1000"),
            "non-secret options must be preserved"
        );

        // Verify the secret doesn't survive in JSON serialization
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(
            !json.contains("hunter2"),
            "secret must not appear anywhere in snapshot JSON"
        );
    }

    #[test]
    fn test_fstab_credentials_path_preserved() {
        use inspectah_core::types::storage::{FstabEntry, StorageSection};
        let mut snapshot = InspectionSnapshot::new();
        snapshot.storage = Some(StorageSection {
            fstab_entries: vec![FstabEntry {
                device: "//server/share".into(),
                mount_point: "/mnt/cifs".into(),
                fstype: "cifs".into(),
                options: "credentials=/etc/cifs-creds,uid=1000".into(),
                ..Default::default()
            }],
            ..Default::default()
        });

        redact(&mut snapshot, &RedactOptions::default());

        // Credential file paths (starting with /) are not inline secrets
        // — they reference an external file, not an embedded password.
        let storage = snapshot.storage.as_ref().unwrap();
        assert!(
            storage.fstab_entries[0]
                .options
                .contains("credentials=/etc/cifs-creds"),
            "credential file path should be preserved, got: {}",
            storage.fstab_entries[0].options
        );
    }

    /// An instance timer's description shares its template's `path`. A
    /// high-confidence hint at that path is resolved only when its keyword
    /// survives in no description there, whichever was recorded last.
    #[test]
    fn a_hint_at_a_path_two_timer_descriptions_share_reads_both_commands() {
        use inspectah_core::types::scheduled::{ScheduledTaskSection, SystemdTimer};
        const PATH: &str = "etc/systemd/system/worker@.timer";
        for order in [["frobkey", "plain"], ["plain", "frobkey"]] {
            let mut snapshot = InspectionSnapshot::new();
            snapshot.scheduled_tasks = Some(ScheduledTaskSection {
                systemd_timers: order
                    .iter()
                    .enumerate()
                    .map(|(i, arg)| SystemdTimer {
                        name: ["worker@", "worker@a"][i].into(),
                        exec_start: format!("/usr/local/bin/worker {arg}"),
                        source: "local".into(),
                        path: PATH.into(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            });
            snapshot.redaction_hints = vec![RedactionHint {
                path: PATH.into(),
                reason: "ExecStart contains frobkey".into(),
                confidence: Some(Confidence::High),
            }];
            redact(&mut snapshot, &RedactOptions::default());
            assert!(
                matches!(
                    snapshot.redaction_state,
                    Some(RedactionState::PartiallyRedacted { .. })
                ),
                "{order:?}: {:?}",
                snapshot.redaction_state
            );
        }
    }

    #[test]
    fn test_high_confidence_hint_without_regex_match_is_partially_redacted() {
        let mut snapshot = InspectionSnapshot::new();
        // High-confidence hint at a path where no regex pass ran —
        // the hint's secret was never actually removed from content.
        // Honest behavior: PartiallyRedacted, not FullyRedacted.
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/proc/cmdline".into(),
            reason: "kernel cmdline contains key=".into(),
            confidence: Some(Confidence::High),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count,
                unresolved_hints,
                ..
            }) => {
                assert!(
                    *unresolved_count > 0,
                    "hint without regex match must remain unresolved"
                );
                assert!(
                    unresolved_hints.iter().any(|h| h.path == "/proc/cmdline"),
                    "unresolved hints must include the cmdline hint"
                );
            }
            other => {
                panic!("expected PartiallyRedacted for hint without regex match, got {other:?}")
            }
        }
    }

    #[test]
    fn test_cmdline_key_hint_produces_partially_redacted() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snapshot = InspectionSnapshot::new();
        // Kernel cmdline with rd.luks.key= — the inspector emits a
        // high-confidence hint for key=, but the regex pattern set
        // intentionally excludes bare key= (too broad). So the secret
        // value is NOT regex-redacted → PartiallyRedacted.
        snapshot.kernel_boot = Some(KernelBootSection {
            cmdline: "quiet rd.luks.key=/path/to/key rd.lvm.lv=vg/root".into(),
            ..Default::default()
        });
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/proc/cmdline".into(),
            reason: "kernel cmdline contains sensitive parameter: rd.luks.key".into(),
            confidence: Some(Confidence::High),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count,
                unresolved_hints,
                ..
            }) => {
                assert!(
                    *unresolved_count > 0,
                    "key= hint without regex redaction must remain unresolved"
                );
                assert!(
                    unresolved_hints.iter().any(|h| h.path == "/proc/cmdline"),
                    "unresolved hints must include the key= cmdline hint"
                );
            }
            other => panic!("expected PartiallyRedacted for key= cmdline hint, got {other:?}"),
        }
    }

    #[test]
    fn test_cmdline_password_and_key_hint_partially_redacted() {
        // Cmdline has BOTH password=hunter2 (regex-handled) AND
        // rd.luks.key=/path (hint-only, NOT regex-handled).
        // Per-hint resolution: password hint resolved, key hint NOT resolved.
        // Result: PartiallyRedacted (the key= secret survives).
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snapshot = InspectionSnapshot::new();
        snapshot.kernel_boot = Some(KernelBootSection {
            cmdline: "quiet password=hunter2 rd.luks.key=/path/to/key rd.lvm.lv=vg/root".into(),
            ..Default::default()
        });
        snapshot.redaction_hints = vec![
            RedactionHint {
                path: "/proc/cmdline".into(),
                reason: "kernel cmdline contains sensitive parameter: password".into(),
                confidence: Some(Confidence::High),
            },
            RedactionHint {
                path: "/proc/cmdline".into(),
                reason: "kernel cmdline contains sensitive parameter: rd.luks.key".into(),
                confidence: Some(Confidence::High),
            },
        ];

        redact(&mut snapshot, &RedactOptions::default());

        // password=hunter2 should be regex-redacted
        assert!(
            !snapshot
                .kernel_boot
                .as_ref()
                .unwrap()
                .cmdline
                .contains("hunter2"),
            "password value must be redacted"
        );

        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count,
                unresolved_hints,
                ..
            }) => {
                assert!(
                    *unresolved_count > 0,
                    "rd.luks.key hint must remain unresolved"
                );
                assert!(
                    unresolved_hints
                        .iter()
                        .any(|h| h.reason.contains("rd.luks.key")),
                    "unresolved hints must include the key hint, got: {:?}",
                    unresolved_hints
                );
            }
            other => panic!(
                "expected PartiallyRedacted (password redacted but key survives), got {other:?}"
            ),
        }
    }

    #[test]
    fn test_cmdline_password_only_hint_fully_redacted() {
        // Cmdline with only password=hunter2, hint for password.
        // Regex handles password= → password hint resolved → FullyRedacted.
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snapshot = InspectionSnapshot::new();
        snapshot.kernel_boot = Some(KernelBootSection {
            cmdline: "quiet password=hunter2 rd.lvm.lv=vg/root".into(),
            ..Default::default()
        });
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/proc/cmdline".into(),
            reason: "kernel cmdline contains sensitive parameter: password".into(),
            confidence: Some(Confidence::High),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        assert!(
            !snapshot
                .kernel_boot
                .as_ref()
                .unwrap()
                .cmdline
                .contains("hunter2"),
            "password value must be redacted"
        );

        match &snapshot.redaction_state {
            Some(RedactionState::FullyRedacted { .. }) => {
                // password= hint resolved because regex handled it
            }
            other => {
                panic!("expected FullyRedacted when password= is regex-handled, got {other:?}")
            }
        }
    }

    /// A secret on the kernel command line loses its whole kernel-parsed
    /// value, however the argument is quoted, so no fragment of it
    /// survives; the hint the collector raises for it counts as handled.
    #[test]
    fn cmdline_redaction_replaces_the_whole_kernel_value() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let cases: &[(&str, &str)] = &[
            (
                "ro \"rd.iscsi.password=alpha bravo\"",
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\"",
            ),
            (
                "ro rd.iscsi.password=\"alpha bravo\"",
                "ro rd.iscsi.password=\"REDACTED_PASSWORD_1\"",
            ),
            (
                "ro \"rd.iscsi.password=alpha\" quiet",
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\" quiet",
            ),
            (
                "quiet password=hunter2 rd.lvm.lv=vg/root",
                "quiet password=REDACTED_PASSWORD_1 rd.lvm.lv=vg/root",
            ),
            // A value whose first word is an NSS or PAM keyword is still
            // a password here: those exemptions are for nsswitch and PAM
            // files, not kernel arguments.
            (
                "ro \"rd.iscsi.password=files bravo7k\" nosmt=force",
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\" nosmt=force",
            ),
            (
                "ro rd.iscsi.password=\"files bravo7k\"",
                "ro rd.iscsi.password=\"REDACTED_PASSWORD_1\"",
            ),
            (
                "ro rd.iscsi.password=files quiet",
                "ro rd.iscsi.password=REDACTED_PASSWORD_1 quiet",
            ),
            // The kernel has no comments: a leading `#` is an argument.
            (
                "# ro rd.iscsi.password=bravo7k",
                "# ro rd.iscsi.password=REDACTED_PASSWORD_1",
            ),
            // A value starting with a space character, or made only of
            // them, is still the whole value: the name finds it.
            (
                "ro \"rd.iscsi.password=\u{2003}bravo7k\" nosmt=force",
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\" nosmt=force",
            ),
            (
                "ro rd.iscsi.password=\"\u{2003}\u{2002}\u{3000}\" nosmt=force",
                "ro rd.iscsi.password=\"REDACTED_PASSWORD_1\" nosmt=force",
            ),
            (
                "ro rd.iscsi.password=\u{a0}bravo7k nosmt=force",
                "ro rd.iscsi.password=REDACTED_PASSWORD_1 nosmt=force",
            ),
            (
                "ro \"rd.iscsi.password=\tbravo7k\" nosmt=force",
                "ro \"rd.iscsi.password=REDACTED_PASSWORD_1\" nosmt=force",
            ),
        ];
        for (cmdline, want) in cases {
            let mut snapshot = InspectionSnapshot::new();
            snapshot.kernel_boot = Some(KernelBootSection {
                cmdline: (*cmdline).into(),
                ..Default::default()
            });
            let name = cmdline
                .split_whitespace()
                .find(|p| p.contains("password="))
                .and_then(|p| p.split('=').next())
                .expect("each case has a password parameter");
            snapshot.redaction_hints = vec![RedactionHint {
                path: KERNEL_CMDLINE_PATH.into(),
                reason: format!("kernel cmdline contains sensitive parameter: {name}"),
                confidence: Some(Confidence::High),
            }];
            redact(&mut snapshot, &RedactOptions::default());
            let got = &snapshot.kernel_boot.as_ref().unwrap().cmdline;
            assert_eq!(got, want, "{cmdline}");
            for fragment in ["alpha", "bravo", "hunter2", "files"] {
                assert!(!got.contains(fragment), "{cmdline}: {got}");
            }
            assert!(
                matches!(
                    snapshot.redaction_state,
                    Some(RedactionState::FullyRedacted { .. })
                ),
                "{cmdline}: {:?}",
                snapshot.redaction_state
            );
        }
    }

    /// On the kernel command line a credential name decides, whatever the
    /// value holds: a non-empty value is one finding and is replaced
    /// whole; an empty value or a name with no `=` is no finding and
    /// changes nothing, and no pattern runs on from an empty value into
    /// the next argument. The kernel reads `-` as `_` in
    /// a name. Any other argument keeps its value.
    #[test]
    fn cmdline_credentials_are_found_by_name() {
        use inspectah_core::types::kernelboot::KernelBootSection;
        let cases: &[(&str, &str, usize)] = &[
            (
                "ro rd.iscsi.password=bravo7k",
                "ro rd.iscsi.password=REDACTED_PASSWORD_1",
                1,
            ),
            (
                "ro rd.foo.api-key=\u{2003}bravo7k quiet",
                "ro rd.foo.api-key=REDACTED_PASSWORD_1 quiet",
                1,
            ),
            (
                "ro rd.iscsi.password= nosmt=force",
                "ro rd.iscsi.password= nosmt=force",
                0,
            ),
            (
                "ro rd.iscsi.password=\"\" nosmt=force",
                "ro rd.iscsi.password=\"\" nosmt=force",
                0,
            ),
            (
                "ro rd.iscsi.password nosmt=force",
                "ro rd.iscsi.password nosmt=force",
                0,
            ),
            (
                "ro foo.bar=\u{2003}keep7k nosmt=force",
                "ro foo.bar=\u{2003}keep7k nosmt=force",
                0,
            ),
            // Patterns search one argument at a time, so an empty
            // credential value cannot take the next argument as its value
            // and hide a secret inside it.
            (
                "ro rd.iscsi.password= systemd.setenv=DB_PASSWORD=bravo7k nosmt=force",
                "ro rd.iscsi.password= systemd.setenv=REDACTED_PASSWORD_1 nosmt=force",
                1,
            ),
            (
                "ro rd.iscsi.password= foo=bar api_token=zz9secret nosmt=force",
                "ro rd.iscsi.password= foo=bar api_token=REDACTED_PASSWORD_1 nosmt=force",
                1,
            ),
            (
                "ro \"rd.iscsi.password=\" \"systemd.setenv=DB_PASSWORD=bravo7k\"",
                "ro \"rd.iscsi.password=\" \"systemd.setenv=REDACTED_PASSWORD_1\"",
                1,
            ),
        ];
        for (cmdline, want, finding_count) in cases {
            let findings = scan_content(cmdline, KERNEL_CMDLINE_PATH);
            assert_eq!(findings.len(), *finding_count, "{cmdline:?}: {findings:?}");
            assert!(
                findings
                    .iter()
                    .all(|f| f.finding_kind == Some(FindingKind::Password)),
                "{cmdline:?}: {findings:?}"
            );
            let mut snapshot = InspectionSnapshot::new();
            snapshot.kernel_boot = Some(KernelBootSection {
                cmdline: (*cmdline).into(),
                ..Default::default()
            });
            redact(&mut snapshot, &RedactOptions::default());
            let got = &snapshot.kernel_boot.as_ref().unwrap().cmdline;
            assert_eq!(got, want, "{cmdline:?}");
        }
    }

    #[test]
    fn test_cmdline_key_only_hint_partially_redacted() {
        // Cmdline with only rd.luks.key=/path, hint for key.
        // No regex matches key= → hint unresolved → PartiallyRedacted.
        use inspectah_core::types::kernelboot::KernelBootSection;
        let mut snapshot = InspectionSnapshot::new();
        snapshot.kernel_boot = Some(KernelBootSection {
            cmdline: "quiet rd.luks.key=/path/to/key rd.lvm.lv=vg/root".into(),
            ..Default::default()
        });
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/proc/cmdline".into(),
            reason: "kernel cmdline contains sensitive parameter: rd.luks.key".into(),
            confidence: Some(Confidence::High),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        match &snapshot.redaction_state {
            Some(RedactionState::PartiallyRedacted {
                unresolved_count,
                unresolved_hints,
                ..
            }) => {
                assert!(
                    *unresolved_count > 0,
                    "key-only hint must remain unresolved"
                );
                assert!(
                    unresolved_hints
                        .iter()
                        .any(|h| h.reason.contains("rd.luks.key")),
                    "unresolved hints must include the key hint"
                );
            }
            other => panic!("expected PartiallyRedacted for key-only cmdline hint, got {other:?}"),
        }
    }

    #[test]
    fn test_high_confidence_hint_with_regex_match_is_fully_redacted() {
        use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
        let mut snapshot = InspectionSnapshot::new();
        // Put a real password in the config file so the regex pass fires
        snapshot.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/config".to_string(),
                content: "password = s3cretP@ss\n".to_string(),
                ..Default::default()
            }],
        });
        // High-confidence hint at the same path where the regex DID match
        snapshot.redaction_hints = vec![RedactionHint {
            path: "/etc/myapp/config".into(),
            reason: "password detected".into(),
            confidence: Some(Confidence::High),
        }];

        redact(&mut snapshot, &RedactOptions::default());

        match &snapshot.redaction_state {
            Some(RedactionState::FullyRedacted { .. }) => {
                // Hint path had regex-sourced findings → hint is honestly resolved
            }
            other => {
                panic!("expected FullyRedacted when hint path was regex-redacted, got {other:?}")
            }
        }
    }

    // -- SensitiveRetained tests --

    #[test]
    fn redact_preserves_shadow_hash_when_credentials_preserved() {
        let mut snap = InspectionSnapshot::new();
        snap.sensitive_snapshot = true;
        snap.preserved_credentials = true;
        snap.users_groups = Some(UserGroupSection {
            shadow_entries: vec![
                "alice:$6$rounds=5000$salt$hash123:19000:0:99999:7:::".to_string(),
            ],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());
        let entry = &snap.users_groups.as_ref().unwrap().shadow_entries[0];
        assert!(
            entry.contains("$6$rounds=5000$salt$hash123"),
            "shadow hash must survive redaction when preserved_credentials is true, got: {entry}"
        );
    }

    #[test]
    fn redact_still_strips_shadow_hash_without_preserve_flag() {
        // Sanity: without preserved_credentials, shadow hashes ARE redacted.
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            shadow_entries: vec![
                "alice:$6$rounds=5000$salt$hash123:19000:0:99999:7:::".to_string(),
            ],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());
        let entry = &snap.users_groups.as_ref().unwrap().shadow_entries[0];
        assert!(
            !entry.contains("$6$rounds=5000$salt$hash123"),
            "shadow hash must be redacted without preserve flag, got: {entry}"
        );
    }

    #[test]
    fn redact_sets_sensitive_retained_state() {
        let mut snap = InspectionSnapshot::new();
        snap.sensitive_snapshot = true;
        snap.preserved_credentials = true;
        snap.users_groups = Some(UserGroupSection {
            shadow_entries: vec!["root:!!:19000:0:99999:7:::".to_string()],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());
        match &snap.redaction_state {
            Some(RedactionState::SensitiveRetained { redacted_by, .. }) => {
                assert!(redacted_by.starts_with("inspectah "));
            }
            other => panic!("expected SensitiveRetained, got {other:?}"),
        }
    }

    #[test]
    fn redact_preserves_ssh_keys_when_flag_set() {
        let mut snap = InspectionSnapshot::new();
        snap.sensitive_snapshot = true;
        snap.preserved_ssh_keys = true;
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "alice",
                "uid": 1000,
                "ssh_keys": [
                    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC... alice@example.com",
                    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAbc... alice@laptop"
                ]
            })],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());

        let users = &snap.users_groups.as_ref().unwrap().users;
        let ssh_keys = users[0]["ssh_keys"].as_array().unwrap();

        assert_eq!(ssh_keys.len(), 2, "both SSH keys must be preserved");
        assert!(
            ssh_keys[0].as_str().unwrap().contains("ssh-rsa"),
            "first SSH key must survive redaction when preserved_ssh_keys is true"
        );
        assert!(
            ssh_keys[1].as_str().unwrap().contains("ssh-ed25519"),
            "second SSH key must survive redaction when preserved_ssh_keys is true"
        );
    }

    #[test]
    fn redact_strips_ssh_keys_without_preserve_flag() {
        // Sanity: without preserved_ssh_keys, SSH keys are removed entirely.
        let mut snap = InspectionSnapshot::new();
        snap.users_groups = Some(UserGroupSection {
            users: vec![serde_json::json!({
                "name": "bob",
                "uid": 1001,
                "ssh_keys": [
                    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC... bob@example.com"
                ]
            })],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());

        let users = &snap.users_groups.as_ref().unwrap().users;

        assert!(
            users[0].get("ssh_keys").is_none(),
            "SSH keys field must be removed without preserve flag"
        );
    }

    // --- Dedup overlap regression tests ---

    #[test]
    fn test_dedup_jdbc_url_password_overlap() {
        // JDBC URL with password param: Password pattern matches the
        // `password=s3cret` substring, JdbcPassword matches the entire URL.
        // Dedup should keep only the longer JdbcPassword match.
        let input = "jdbc:postgresql://host:5432/db?user=admin&password=s3cret";
        let findings = scan_content(input, "/etc/app.conf");
        let jdbc_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::JdbcPassword))
            .collect();
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert_eq!(
            jdbc_findings.len(),
            1,
            "JDBC URL must produce exactly one JdbcPassword finding"
        );
        assert_eq!(
            password_findings.len(),
            0,
            "Password finding must be suppressed by longer JdbcPassword match"
        );
    }

    #[test]
    fn test_dedup_jdbc_url_redaction() {
        // The JDBC URL should be redacted as a single token, not double-redacted.
        let input = "jdbc:postgresql://host:5432/db?user=admin&password=s3cret";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_JDBCPASSWORD_"),
            "JDBC URL must be redacted with JdbcPassword token, got: {result}"
        );
        // The Password pattern's shorter match should NOT produce a separate token
        assert!(
            !result.contains("REDACTED_PASSWORD_"),
            "Password token must not appear when JdbcPassword subsumes it, got: {result}"
        );
    }

    #[test]
    fn test_dedup_standalone_password_unchanged() {
        // Standalone password=s3cret (no JDBC context) should still produce
        // a Password finding — dedup only drops subsets, not standalone matches.
        let input = "password=s3cret";
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_kind, Some(FindingKind::Password));
    }

    #[test]
    fn test_dedup_jdbc_url_without_password() {
        // JDBC URL without password param — no Password or JdbcPassword findings.
        let input = "jdbc:postgresql://host:5432/db?user=admin";
        let findings = scan_content(input, "/etc/app.conf");
        let relevant: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| {
                f.finding_kind == Some(FindingKind::Password)
                    || f.finding_kind == Some(FindingKind::JdbcPassword)
            })
            .collect();
        assert!(
            relevant.is_empty(),
            "JDBC URL without password must not produce Password/JdbcPassword findings"
        );
    }

    #[test]
    fn test_dedup_preserves_non_overlapping_matches() {
        // Two independent secrets on different lines — both must survive dedup.
        let input = "password=secret1\napi_key=secret2";
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(
            findings.len(),
            2,
            "non-overlapping matches must both survive dedup"
        );
    }

    #[test]
    fn test_eligible_match_line_numbers() {
        // Verify line numbers are computed correctly by collect_eligible_matches.
        let input = "clean line\npassword=s3cret\nanother clean line";
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].line,
            Some(2),
            "password on second line must report line 2"
        );
    }

    /// A finding's line is the line its secret starts on, wherever the
    /// match starts in that line and whatever context before it the
    /// pattern consumed.
    #[test]
    fn test_finding_line_is_the_secret_line() {
        let cases: &[(&str, i32)] = &[
            ("password=x", 1),
            ("x password=y", 1),
            ("  password=x", 1),
            ("a\n  password=x", 2),
            ("a\nproxy_password = x", 2),
            ("a\r\nb password=x", 2),
            ("a\n\nkey AKIAABCDEFGHIJKLMNOP", 3),
            // The AWS pattern consumes the newline before the key.
            ("a\nAKIAABCDEFGHIJKLMNOP", 2),
            ("url=jdbc:mysql://u:pw@db", 1),
        ];
        for (input, line) in cases {
            let findings = scan_content(input, "/etc/app.conf");
            assert_eq!(findings.len(), 1, "{input:?}: {findings:?}");
            assert_eq!(findings[0].line, Some(*line), "{input:?}");
        }
    }

    /// An AWS key id is a comment only when its own line is: the newline
    /// the pattern consumes before the key does not make the line above it
    /// the key's line. `#AKIA...` on its own line stays redacted, as it was
    /// before keys kept their context: the `#` is the boundary the pattern
    /// consumes, so its match starts at the line's start.
    #[test]
    fn test_aws_key_after_a_comment_line_is_redacted() {
        let cases: &[(&str, &str, usize)] = &[
            (
                "# note\nAKIAABCDEFGHIJKLMNOP",
                "# note\nREDACTED_AWSKEY_1",
                1,
            ),
            (
                "; note\nAKIAABCDEFGHIJKLMNOP",
                "; note\nREDACTED_AWSKEY_1",
                1,
            ),
            ("#AKIAABCDEFGHIJKLMNOP", "#REDACTED_AWSKEY_1", 1),
            // A comment line holding a key is still exempt.
            ("# AKIAABCDEFGHIJKLMNOP", "# AKIAABCDEFGHIJKLMNOP", 0),
            (
                "# old AKIAABCDEFGHIJKLMNOP\nAKIAQRSTUVWXYZ234567",
                "# old AKIAABCDEFGHIJKLMNOP\nREDACTED_AWSKEY_1",
                1,
            ),
        ];
        for (input, want, n) in cases {
            let mut snap = config_snapshot("/etc/app/app.ini", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(
                &snap.config.as_ref().unwrap().files[0].content,
                want,
                "{input:?}"
            );
            assert_eq!(snap.redactions.len(), *n, "{input:?}");
            assert!(
                matches!(
                    snap.redaction_state,
                    Some(RedactionState::FullyRedacted { .. })
                ),
                "{input:?}"
            );
            assert_eq!(
                scan_content(input, "/etc/app/app.ini").len(),
                *n,
                "{input:?}"
            );
        }
    }

    /// Two AWS key ids one separator apart are both found: the boundary
    /// after the first key is checked, not consumed, so it is still there
    /// to start the second match. `redact_string` replaces from the end,
    /// so the later key gets the lower number.
    #[test]
    fn test_adjacent_aws_keys_are_each_redacted() {
        let cases: &[(&str, &str)] = &[
            (
                "AKIA1111111111111111\nAKIA2222222222222222",
                "REDACTED_AWSKEY_2\nREDACTED_AWSKEY_1",
            ),
            (
                "AKIA1111111111111111 AKIA2222222222222222",
                "REDACTED_AWSKEY_2 REDACTED_AWSKEY_1",
            ),
            (
                "keys=AKIA1111111111111111,AKIA2222222222222222,AKIA1111111111111111",
                "keys=REDACTED_AWSKEY_1,REDACTED_AWSKEY_2,REDACTED_AWSKEY_1",
            ),
            // Still a boundary on each side: a longer run is not a key id.
            (
                "AKIA11111111111111112 XAKIA2222222222222222",
                "AKIA11111111111111112 XAKIA2222222222222222",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact_string(input), *want, "{input:?}");
            let lines: Vec<Option<i32>> = scan_content(input, "/etc/app.conf")
                .iter()
                .map(|f| f.line)
                .collect();
            assert_eq!(
                lines.len(),
                want.matches("REDACTED_AWSKEY_").count(),
                "{input:?}"
            );
        }
        let findings = scan_content(
            "AKIA1111111111111111\nAKIA2222222222222222",
            "/etc/app.conf",
        );
        let lines: Vec<Option<i32>> = findings.iter().map(|f| f.line).collect();
        assert_eq!(lines, vec![Some(1), Some(2)]);
    }

    /// Every credential on a line is masked, each with its own finding,
    /// not only the first match of each masker.
    #[test]
    fn test_url_maskers_mask_every_credential_on_a_line() {
        const PATH: &str = "/etc/environment";
        let token = "deploytoken0123456789xyz";
        let proxy_cases = [
            // Two passwords.
            (
                "M=https://a:pw1@h1 https://b:pw2@h2".to_string(),
                "M=https://a:[REDACTED]@h1 https://b:[REDACTED]@h2".to_string(),
                2,
            ),
            // Three passwords.
            (
                "https://a:pw1@h1,https://b:pw2@h2 ftp://c:pw3@h3".into(),
                "https://a:[REDACTED]@h1,https://b:[REDACTED]@h2 ftp://c:[REDACTED]@h3".into(),
                3,
            ),
            // The same credential repeated.
            (
                "https://a:same@h1 https://a:same@h1".into(),
                "https://a:[REDACTED]@h1 https://a:[REDACTED]@h1".into(),
                2,
            ),
            // A URL password and a bare proxy_password value on one line.
            (
                "proxy=http://u:pw1@p proxy_password=pw2".into(),
                "proxy=http://u:[REDACTED]@p proxy_password=[REDACTED]".into(),
                2,
            ),
            // A proxy_password value that is a URL is masked once, as the
            // URL's password.
            (
                "proxy_password=http://u:pw1@p".into(),
                "proxy_password=http://u:[REDACTED]@p".into(),
                1,
            ),
            // A password holding an `@` is masked to the authority's last
            // `@`, not cut at its first.
            (
                "https://deploy:p@s3cret@host/r".into(),
                "https://deploy:[REDACTED]@host/r".into(),
                1,
            ),
            (
                "https://u:a@b@c@host".into(),
                "https://u:[REDACTED]@host".into(),
                1,
            ),
            // An `@` in the path or query is not the authority's.
            (
                "https://u:p@s@host/x@y?to=a@b#c@d".into(),
                "https://u:[REDACTED]@host/x@y?to=a@b#c@d".into(),
                1,
            ),
            // Two such URLs on one line.
            (
                "M=https://a:p@1@h1/z@w https://b:q@2@h2".into(),
                "M=https://a:[REDACTED]@h1/z@w https://b:[REDACTED]@h2".into(),
                2,
            ),
            // An `@` user name, percent-encoded, keeps its user name.
            (
                "https://me%40corp.example:p@ss@host".into(),
                "https://me%40corp.example:[REDACTED]@host".into(),
                1,
            ),
            // An authority with no `@`: a raw `/` in the password still
            // runs to the first `@`, as before.
            (
                "https://u:pa/ss@host".into(),
                "https://u:[REDACTED]@host".into(),
                1,
            ),
        ];
        for (input, want, count) in proxy_cases {
            let (out, findings) = mask_proxy_credentials(&input, PATH);
            assert_eq!(out, want, "{input}");
            assert_eq!(findings.len(), count, "{input}");
        }

        let token_cases = [
            (
                format!("https://{token}@h1 https://{token}@h2"),
                "https://[REDACTED]@h1 https://[REDACTED]@h2".to_string(),
                2,
            ),
            (
                format!(
                    "https://{token}@h1 https://ghp_{}@github.com",
                    "x".repeat(36)
                ),
                "https://[REDACTED]@h1 https://[REDACTED]@github.com".into(),
                2,
            ),
            // A token holding an `@`, with an `@` in the path after it.
            (
                format!("https://{token}@x@h1/p@q https://{token}@h2"),
                "https://[REDACTED]@h1/p@q https://[REDACTED]@h2".into(),
                2,
            ),
        ];
        for (input, want, count) in token_cases {
            let (out, findings) = mask_token_username(&input, PATH);
            assert_eq!(out, want, "{input}");
            assert_eq!(findings.len(), count, "{input}");
        }
    }

    /// Where a value ends follows the file's format. A quoted Wi-Fi PSK is
    /// taken whole. In an INI-style file a credential key that opens its
    /// line has the rest of the line as its value, as the INI readers
    /// read it; in a format whose values end at whitespace (a command, a
    /// shell file) it does not, so the fields after it stay.
    #[test]
    fn test_value_boundaries_follow_the_format() {
        let whole: &[(&str, &str, &[&str])] = &[
            (
                "/etc/wpa_supplicant/wpa_supplicant.conf",
                "network={\n  psk=\"correct horse battery staple\"\n}\n",
                &["horse", "battery", "staple"],
            ),
            (
                "/etc/NetworkManager/system-connections/home.nmconnection",
                "[wifi-security]\npsk=correct horse battery\n",
                &["horse", "battery"],
            ),
            (
                "/etc/app/app.ini",
                "[db]\npassword=alpha bravo\n",
                &["alpha", "bravo"],
            ),
            (
                "/etc/app/app.conf",
                "  db_password = alpha bravo charlie  \n",
                &["alpha", "bravo", "charlie"],
            ),
            (
                "/opt/app/.env",
                "API_TOKEN=alpha bravo\n",
                &["alpha", "bravo"],
            ),
        ];
        for (path, input, secrets) in whole {
            let mut snap = config_snapshot(path, input);
            redact(&mut snap, &RedactOptions::default());
            let out = &snap.config.as_ref().unwrap().files[0].content;
            for secret in *secrets {
                assert!(!out.contains(secret), "{path}: {secret} survives: {out}");
            }
            assert!(out.ends_with('\n'), "{path}: {out:?}");
        }
        // The kernel argument after a value, a command after an inline
        // assignment, and a value that does not open its line keep what
        // follows them.
        let kept: &[(&str, &str, &str, &str)] = &[
            (
                "/etc/cron.d/backup",
                "DB_PASSWORD=alpha /usr/bin/backup --full\n",
                "alpha",
                "/usr/bin/backup --full",
            ),
            (
                "/etc/sysconfig/app",
                "PASSWORD=alpha OTHER=bravo\n",
                "alpha",
                "OTHER=bravo",
            ),
            (
                "/etc/app/app.conf",
                "opts = password=alpha retries=3\n",
                "alpha",
                "retries=3",
            ),
        ];
        for (path, input, secret, rest) in kept {
            let mut snap = config_snapshot(path, input);
            redact(&mut snap, &RedactOptions::default());
            let out = &snap.config.as_ref().unwrap().files[0].content;
            assert!(!out.contains(secret), "{path}: {secret} survives: {out}");
            assert!(out.contains(rest), "{path}: {rest} lost: {out}");
        }
    }

    /// A `proxy_password=` value is read as dnf reads it: to the end of
    /// its line when the key opens the line, whole when quoted, and to
    /// the next whitespace elsewhere.
    #[test]
    fn test_proxy_password_value_boundaries() {
        let cases = [
            ("proxy_password=alpha bravo", "proxy_password=[REDACTED]"),
            (
                "  proxy_password = alpha bravo ",
                "  proxy_password = [REDACTED] ",
            ),
            (
                r#"proxy_password="alpha bravo""#,
                "proxy_password=[REDACTED]",
            ),
            (
                r#"opts proxy_password="alpha bravo" retries=3"#,
                "opts proxy_password=[REDACTED] retries=3",
            ),
            (
                "opts proxy_password=alpha retries=3",
                "opts proxy_password=[REDACTED] retries=3",
            ),
        ];
        for (input, want) in cases {
            let (out, findings) = mask_proxy_credentials(input, "/etc/dnf/dnf.conf");
            assert_eq!(out, want, "{input}");
            assert_eq!(findings.len(), 1, "{input}");
        }
    }

    /// A `proxy_password=` that opens a line runs to the line's end only
    /// in a file whose format reads it so. In a shell file or a command,
    /// whitespace ends the value and the command after it stays.
    #[test]
    fn proxy_password_in_shell_keeps_the_command() {
        let (out, _) = redact_payload(
            "proxy_password=alpha /usr/bin/backup --full\n",
            "/opt/jobs/backup.sh",
        )
        .unwrap();
        assert!(!out.contains("alpha"), "{out}");
        assert!(out.contains("/usr/bin/backup --full"), "{out}");

        for path in ["/opt/jobs/backup.sh", "etc/profile.d/proxy.sh"] {
            let (out, findings) =
                mask_proxy_credentials("proxy_password=alpha /usr/bin/backup --full", path);
            assert_eq!(
                out, "proxy_password=[REDACTED] /usr/bin/backup --full",
                "{path}"
            );
            assert_eq!(findings.len(), 1, "{path}");
        }
        // The INI form still takes the rest of the line.
        let (out, _) = mask_proxy_credentials("proxy_password=alpha bravo", "etc/dnf/dnf.conf");
        assert_eq!(out, "proxy_password=[REDACTED]");
    }

    /// The URL credential edits a payload takes: one per credential, mixed
    /// password and token forms alike, each with its own finding on the
    /// right line, and none of the secrets left in the output.
    #[test]
    fn test_url_credential_edits_are_one_per_credential() {
        let token = "deploytoken0123456789xyz";
        let content = format!(
            "clean line\n\
             M=https://a:pw1@h1 https://{token}@h2 https://b:pw2@h3 https://a:pw1@h1\n"
        );
        let (edits, unedited) = url_credential_edits(&content, "/opt/app/.env");
        assert!(unedited.is_empty());
        assert_eq!(edits.len(), 4);
        let sources: Vec<&str> = edits
            .iter()
            .map(|e| {
                assert_eq!(e.findings.len(), 1);
                assert_eq!(e.findings[0].line, Some(2));
                assert_eq!(e.text, "[REDACTED]");
                e.findings[0].source.as_str()
            })
            .collect();
        assert_eq!(
            sources.iter().filter(|s| **s == "token_credential").count(),
            1,
            "{sources:?}"
        );
        let (out, findings) = apply_payload_edits(&content, edits);
        assert_eq!(findings.len(), 4);
        for secret in ["pw1", "pw2", token] {
            assert!(!out.contains(secret), "{secret} survives: {out}");
        }
        assert_eq!(out.matches("[REDACTED]").count(), 4, "{out}");

        // Passwords and a token holding an `@`: still one edit each, and
        // no tail of any left behind.
        let content = format!(
            "M=https://a:p@tail1@h1/x@y https://{token}@tail2@h2 https://b:q@r@tail3@h3?u=v@w\n"
        );
        let (edits, unedited) = url_credential_edits(&content, "/opt/app/.env");
        assert!(unedited.is_empty());
        assert_eq!(edits.len(), 3);
        let (out, findings) = apply_payload_edits(&content, edits);
        assert_eq!(findings.len(), 3);
        assert_eq!(
            out,
            "M=https://a:[REDACTED]@h1/x@y https://[REDACTED]@h2 https://b:[REDACTED]@h3?u=v@w\n"
        );
    }

    /// The proxy masker's `proxy_password =` stays on its line: with an
    /// empty value it matches nothing, rather than taking the next line as
    /// the password, as the generic credential pattern already does.
    #[test]
    fn test_proxy_password_masking_stays_on_its_line() {
        for input in ["proxy_password =\nnext=1", "proxy_password\n= next"] {
            let (out, findings) = mask_proxy_credentials(input, "/etc/dnf/dnf.conf");
            assert_eq!(out, input, "{input:?}");
            assert!(findings.is_empty(), "{input:?}");
        }
        let (out, findings) =
            mask_proxy_credentials("proxy_password =\thunter2", "/etc/dnf/dnf.conf");
        assert_eq!(out, "proxy_password =\t[REDACTED]");
        assert_eq!(findings.len(), 1);
    }

    /// A user name with a raw `?` or `#` is malformed, but its password is
    /// still masked, as it was before the last-`@` rule; an empty user
    /// name is masked too. Each runs through the git remote's sequential
    /// maskers and through the payload pass.
    #[test]
    fn test_url_password_masked_under_any_user_name() {
        let cases = [
            (
                "https://u?name:qmarkpw8812@host/r",
                "https://u?name:[REDACTED]@host/r",
            ),
            (
                "https://u#name:hashpw8812@host/r",
                "https://u#name:[REDACTED]@host/r",
            ),
            (
                "https://:emptyuserpw8812@host/r",
                "https://:[REDACTED]@host/r",
            ),
        ];
        for (input, want) in cases {
            let (out, findings) = mask_proxy_credentials(input, "/etc/environment");
            assert_eq!(out, want, "{input}");
            assert_eq!(findings.len(), 1, "{input}");
            let (text, findings) =
                redact_payload(&format!("M={input}\n"), "/opt/app/.env").expect("redacted");
            assert_eq!(text, format!("M={want}\n"), "{input}");
            assert_eq!(findings.len(), 1, "{input}");
        }
    }

    /// A token standing as the user name is masked when a password
    /// follows it, empty or not: the password does not make the token any
    /// less of a credential.
    #[test]
    fn test_token_user_name_masked_beside_a_password() {
        let token = "abcdefghijklmnopqrstuvwx";
        let cases = [
            (
                format!("https://{token}:x-oauth-basic@host/r"),
                "https://[REDACTED]:[REDACTED]@host/r",
                2,
            ),
            (
                format!("https://{token}:@host/r"),
                "https://[REDACTED]:@host/r",
                1,
            ),
            (
                format!("https://ghp_{}:x-oauth-basic@github.com/r", "y".repeat(36)),
                "https://[REDACTED]:[REDACTED]@github.com/r",
                2,
            ),
        ];
        for (input, want, count) in cases {
            let (text, findings) =
                redact_payload(&format!("{input}\n"), "/opt/app/repo.cfg").expect("redacted");
            assert_eq!(text, format!("{want}\n"), "{input}");
            assert_eq!(findings.len(), count, "{input}");
            // The git remote pass masks the password, then the token.
            let (masked, _) = mask_proxy_credentials(&input, "/srv/app");
            let (masked, _) = mask_token_username(&masked, "/srv/app");
            assert_eq!(masked, want, "{input}");
        }
        // A long host name before a port is no token user name.
        let host = "https://artifactory-prod-server01:8081/r";
        assert_eq!(mask_token_username(host, "/srv/app").0, host);
    }

    /// Two credentials whose ranges partly overlap are masked as one span
    /// over both, never by dropping one of them.
    #[test]
    fn test_partially_overlapping_url_credentials_are_merged() {
        let token = "abcdefghijklmnopqrstuvwx";
        let line = format!("https://{token}@proxy_password=tail@host\n");
        let (text, findings) = redact_payload(&line, "/opt/app/.env").expect("redacted");
        for secret in [token, "tail"] {
            assert!(!text.contains(secret), "{secret} survives: {text}");
        }
        assert_eq!(findings.len(), 1, "{text}");

        // A proxy_password value with a password before a URL in it.
        let (out, findings) =
            mask_proxy_credentials("proxy_password=hunter2,https://u:pw@h", "/etc/dnf/dnf.conf");
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains(":pw@"), "{out}");
        assert_eq!(findings.len(), 1, "{out}");
    }

    /// The merge of the URL maskers' edits does a bounded amount of work
    /// per credential, however many share a line: each candidate is
    /// compared with a constant number of others, not with every one.
    #[test]
    fn test_url_credential_merge_is_linear_on_a_dense_line() {
        const PAIRS: usize = 5_000;
        let unit = "https://u:p@h https://deploytoken0123456789xyz@h proxy_password=x ";
        let line = unit.repeat(PAIRS);
        MERGE_STEPS.with(|n| n.set(0));
        let (text, findings) = redact_payload(&line, "/opt/app/.env").expect("redacted");
        let steps = MERGE_STEPS.with(std::cell::Cell::get);
        assert_eq!(findings.len(), 3 * PAIRS);
        assert!(!text.contains("deploytoken"));
        // Three credentials per unit, each touched a few times.
        assert!(steps <= 12 * PAIRS, "{steps} merge steps for {PAIRS} units");
    }

    // --- Comment-line filtering tests (Gap 1) ---

    #[test]
    fn test_comment_hash_not_redacted_via_redact() {
        // Hash-commented line must be preserved through the full redact() path.
        let mut snapshot = InspectionSnapshot::new();
        snapshot.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/config".to_string(),
                content: "# password=old_value\npassword=real".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snapshot, &RedactOptions::default());
        let content = &snapshot.config.as_ref().unwrap().files[0].content;
        assert!(
            content.starts_with("# password=old_value\n"),
            "comment line must be untouched, got: {content}"
        );
        assert!(
            content.contains("REDACTED_"),
            "non-comment line must be redacted, got: {content}"
        );
        // Only one finding — the non-comment line
        let findings: Vec<&RedactionFinding> = snapshot
            .redactions
            .iter()
            .filter(|f| f.source == "pattern")
            .collect();
        assert_eq!(
            findings.len(),
            1,
            "only the non-comment line should produce a finding"
        );
    }

    #[test]
    fn test_comment_semicolon_preserved() {
        let input = "; token=example\ntoken=secret";
        let findings = scan_content(input, "/etc/php.ini");
        assert_eq!(
            findings.len(),
            1,
            "semicolon-commented match must be filtered"
        );
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn test_comment_cstyle_preserved() {
        let input = "// api_key=docs_example\napi_key=live";
        let findings = scan_content(input, "/etc/named.conf");
        assert_eq!(
            findings.len(),
            1,
            "C-style commented match must be filtered"
        );
        assert_eq!(findings[0].line, Some(2));
    }

    #[test]
    fn test_non_hash_markers_exempt_only_where_they_are_comment_syntax() {
        // (path, content, expected finding count). `//` and `;` are not
        // comment syntax in a generic /etc file, so a line that merely
        // begins with them must still be scanned.
        let cases: &[(&str, &str, usize)] = &[
            ("/etc/fstab", "//srv/share/password=hunter2", 1),
            ("/etc/app.conf", "//srv/share/password=hunter2", 1),
            ("/etc/app.conf", ";password=hunter2", 1),
            ("/etc/ssh/sshd_config", "// api_key=live", 1),
            ("/etc/named.conf", "// api_key=docs_example", 0),
            ("/etc/php.ini", "; token=example", 0),
            ("/etc/samba/smb.conf", "; password=example", 0),
            ("/etc/yum.repos.d/x.repo", "; password=example", 0),
            // systemd.syntax(7): units, drop-ins and the daemons' own
            // configuration files take `#` and `;` comments.
            (
                "/etc/systemd/system/example.service",
                "; password=example",
                0,
            ),
            (
                "/usr/lib/systemd/system/example.timer",
                "; password=example",
                0,
            ),
            (
                "/etc/systemd/system/example.service.d/override.conf",
                "; password=example",
                0,
            ),
            ("/etc/systemd/journald.conf", "; password=example", 0),
            (
                "/etc/systemd/journald.conf.d/50-local.conf",
                "; password=example",
                0,
            ),
            (
                "/etc/containers/systemd/app.container",
                "; password=example",
                0,
            ),
            ("generated:cron-backup.service", "; password=example", 0),
            (
                "/etc/systemd/nspawn/example.nspawn",
                "; password=example",
                0,
            ),
            // Not systemd syntax: a `.conf` outside a systemd tree, a
            // unit suffix outside one, and a non-`.conf` file in a
            // drop-in directory.
            ("/etc/tmpfiles.d/x.conf", ";password=hunter2", 1),
            ("/etc/app.d/x.conf", ";password=hunter2", 1),
            ("/opt/app/example.service", ";password=hunter2", 1),
            (
                "/etc/systemd/system/x.service.d/notes.txt",
                ";password=hunter2",
                1,
            ),
            (
                "/etc/systemd/system/x.service",
                "//srv/share/password=hunter2",
                1,
            ),
        ];
        for (path, input, want) in cases {
            let findings = scan_content(input, path);
            assert_eq!(
                findings.len(),
                *want,
                "{path}: {input:?} produced {findings:?}"
            );
        }
        // No path: only `#` is a comment.
        assert!(redact_string("//srv/share/password=hunter2").contains("REDACTED_"));
    }

    #[test]
    fn test_inline_comment_still_redacted() {
        // Comment marker mid-line is NOT a comment line — must still redact.
        let input = "password=secret # old was foo";
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(
            findings.len(),
            1,
            "inline comment (mid-line #) must still produce a finding"
        );
    }

    #[test]
    fn test_indented_comment_filtered() {
        let input = "  # password=old";
        let findings = scan_content(input, "/etc/app.conf");
        assert!(
            findings.is_empty(),
            "indented comment must be filtered (trimmed prefix starts with #)"
        );
    }

    #[test]
    fn test_first_line_comment_filtered() {
        // First line with no preceding newline must be handled.
        let input = "# secret=abc";
        let findings = scan_content(input, "/etc/app.conf");
        assert!(findings.is_empty(), "first-line comment must be filtered");
    }

    #[test]
    fn test_mixed_comment_and_real_via_redact() {
        // Full redact() path: comment line untouched, real line mutated.
        let mut snapshot = InspectionSnapshot::new();
        snapshot.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/config".to_string(),
                content: "# password=old\npassword=real".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snapshot, &RedactOptions::default());
        let content = &snapshot.config.as_ref().unwrap().files[0].content;
        // Line 1 must be byte-identical to input
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "# password=old", "comment line must survive");
        assert!(
            lines[1].contains("REDACTED_"),
            "real line must be redacted, got: {}",
            lines[1]
        );
    }

    #[test]
    fn test_comment_filtering_in_redact_string() {
        // redact_string() uses collect_eligible_matches, so comments
        // should be filtered. The fast-path is_match still triggers a
        // clone (wasted but harmless), so the result is Owned but
        // content-identical to input.
        let input = "# password=old_value";
        let result = redact_string(input);
        assert_eq!(
            &*result, input,
            "commented-only content must not be modified by redact_string"
        );
        assert!(
            !result.contains("REDACTED_"),
            "no redaction token should appear in commented content"
        );
    }

    // --- False-positive value filtering tests (Gap 4) ---

    /// SSSD's `*_authtok` value is the whole rest of the line. Spaces,
    /// tabs and quotes stay inside it, a value spelled like an NSS word
    /// is still a password, and the key, `=` and `_type` setting stay.
    #[test]
    fn test_sssd_authtok_value_is_redacted_whole() {
        let input = "[domain/a]\nldap_default_authtok_type = password\nldap_default_authtok = Bind Secret Tail\n\
                     [domain/b]\nldap_default_authtok =\tTab\tSecret \n\
                     [domain/c]\nldap_default_authtok = \"Quoted Secret\"\n\
                     [domain/d]\nldap_default_authtok = files\n\
                     [domain/e]\n# ldap_default_authtok = commented\n";
        let result = redact_string(input);
        for secret in ["Bind", "Secret", "Tail", "Tab", "Quoted", "files"] {
            assert!(!result.contains(secret), "{secret} survived: {result}");
        }
        assert!(
            result.contains("ldap_default_authtok_type = password\n"),
            "{result}"
        );
        assert!(
            result.contains("# ldap_default_authtok = commented\n"),
            "{result}"
        );
        assert_eq!(
            result
                .matches("ldap_default_authtok = REDACTED_PASSWORD_")
                .count(),
            3,
            "{result}"
        );
        assert!(
            result.contains("ldap_default_authtok =\tREDACTED_PASSWORD_"),
            "{result}"
        );
    }

    #[test]
    fn test_nsswitch_false_positive_no_finding() {
        // `passwd: files sss` — the regex fires on `passwd:` with `:` separator,
        // but `files` is a known false positive.
        let input = "passwd: files sss";
        let findings = scan_content(input, "/etc/nsswitch.conf");
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert!(
            password_findings.is_empty(),
            "NSS token 'files' must be filtered as false positive"
        );
    }

    #[test]
    fn test_nss_token_via_equals() {
        let input = "password=files";
        let findings = scan_content(input, "/etc/nsswitch.conf");
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert!(
            password_findings.is_empty(),
            "password=files must be filtered (value 'files' is false positive)"
        );
    }

    #[test]
    fn test_pam_token_via_colon() {
        let input = "password: sufficient";
        let findings = scan_content(input, "/etc/pam.d/system-auth");
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert!(
            password_findings.is_empty(),
            "password: sufficient must be filtered (value 'sufficient' is false positive)"
        );
    }

    #[test]
    fn test_pam_module_via_equals() {
        let input = "password=pam_unix.so";
        let findings = scan_content(input, "/etc/security/custom.conf");
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert!(
            password_findings.is_empty(),
            "password=pam_unix.so must be filtered as false positive"
        );
    }

    #[test]
    fn test_real_password_not_filtered() {
        let input = "password=s3cret";
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(
            findings.len(),
            1,
            "real password value must NOT be filtered"
        );
        assert_eq!(findings[0].finding_kind, Some(FindingKind::Password));
    }

    #[test]
    fn test_mixed_false_positive_and_real_via_redact() {
        // First line has false-positive value, second has a real secret.
        let mut snapshot = InspectionSnapshot::new();
        snapshot.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/config".to_string(),
                content: "passwd: files\ndb_password=real".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snapshot, &RedactOptions::default());
        let content = &snapshot.config.as_ref().unwrap().files[0].content;
        assert!(
            content.starts_with("passwd: files\n"),
            "false-positive line must be untouched, got: {content}"
        );
        assert!(
            content.contains("REDACTED_"),
            "real password line must be redacted, got: {content}"
        );
    }

    #[test]
    fn test_false_positive_case_insensitive() {
        let input = "password: FILES";
        let findings = scan_content(input, "/etc/nsswitch.conf");
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        assert!(
            password_findings.is_empty(),
            "case-insensitive false positive check must filter 'FILES'"
        );
    }

    #[test]
    fn test_real_secret_in_pam_path() {
        // A real secret in a pam.d path should still be redacted —
        // the false-positive filter checks VALUE, not path.
        let input = "password=actualpass123";
        let findings = scan_content(input, "/etc/pam.d/custom");
        assert_eq!(
            findings.len(),
            1,
            "real secret must still be detected even in pam.d path"
        );
    }

    // --- PasswordHash pattern tests (Gap 2) ---

    #[test]
    fn test_password_hash_detected_in_htpasswd() {
        let input = "admin:$6$rounds=5000$salt$hashvalue";
        let findings = scan_content(input, "/etc/htpasswd");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "crypt hash must be detected in htpasswd"
        );
    }

    #[test]
    fn test_password_hash_sha512() {
        let input = "$6$salt$hashvalue";
        let findings = scan_content(input, "/etc/kickstart.cfg");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "SHA-512 crypt hash must be detected"
        );
    }

    #[test]
    fn test_password_hash_bcrypt() {
        let input = "$2b$12$saltsaltsaltsaltsalt.hashhashhashhashhashhas";
        let findings = scan_content(input, "/etc/app.conf");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(hash_findings.len(), 1, "bcrypt hash must be detected");
    }

    #[test]
    fn test_password_hash_yescrypt() {
        let input = "$y$j9T$saltsalt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/app.conf");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(hash_findings.len(), 1, "yescrypt hash must be detected");
    }

    #[test]
    fn test_password_hash_excluded_from_shadow() {
        // PasswordHash pattern must be excluded from shadow files —
        // scan_shadow handles those with richer classification.
        let input = "admin:$6$rounds=5000$salt$hashvalue:19000:0:99999:7:::";
        let findings = scan_content(input, "/etc/shadow");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert!(
            hash_findings.is_empty(),
            "PasswordHash must be excluded from /etc/shadow (scan_shadow owns it)"
        );
        // But ShadowHash should still be detected
        let shadow_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::ShadowHash))
            .collect();
        assert_eq!(
            shadow_findings.len(),
            1,
            "ShadowHash must still be detected in /etc/shadow"
        );
    }

    #[test]
    fn test_password_hash_excluded_from_shadow_backup() {
        let input = "admin:$6$salt$hash:19000:0:99999:7:::";
        let findings = scan_content(input, "/etc/shadow-");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert!(
            hash_findings.is_empty(),
            "PasswordHash must be excluded from /etc/shadow- (backup)"
        );
    }

    #[test]
    fn test_password_hash_overlap_with_password_pattern() {
        // password=$6$salt$hash should match both Password and PasswordHash,
        // but dedup keeps the longer Password match (it includes the key prefix).
        let input = "password=$6$rounds=5000$salt$hash";
        let findings = scan_content(input, "/etc/kickstart.cfg");
        // The Password pattern matches the whole `password=$6$rounds=5000$salt$hash`
        // The PasswordHash pattern matches `$6$rounds=5000$salt$hash` (subset)
        // Dedup should keep the longer one (Password)
        let password_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::Password))
            .collect();
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            password_findings.len(),
            1,
            "Password pattern (longer match) must survive dedup"
        );
        assert!(
            hash_findings.is_empty(),
            "PasswordHash (shorter subset) must be suppressed by dedup"
        );
    }

    // --- PEM full-block matching tests (Gap 3) ---

    #[test]
    fn test_pem_rsa_private_key_full_block() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\nbase64data\n-----END RSA PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "full RSA private key block must be redacted, got: {result}"
        );
        // The entire block should be replaced with a single token
        assert!(
            !result.contains("BEGIN RSA PRIVATE KEY"),
            "BEGIN marker must not survive redaction"
        );
    }

    #[test]
    fn test_pem_ec_private_key_full_block() {
        let input = "-----BEGIN EC PRIVATE KEY-----\nMHQCAQEE...\n-----END EC PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "EC private key block must be redacted"
        );
    }

    #[test]
    fn test_pem_openssh_private_key_full_block() {
        let input = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1...\n-----END OPENSSH PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "OPENSSH private key block must be redacted"
        );
    }

    /// Every private-key label is redacted, the PKCS#8 encrypted and
    /// plain ones included, and the key's base64 does not survive.
    #[test]
    fn test_pem_every_private_key_label_is_redacted() {
        for label in [
            "ENCRYPTED PRIVATE KEY",
            "PRIVATE KEY",
            "RSA PRIVATE KEY",
            "EC PRIVATE KEY",
            "PGP PRIVATE KEY BLOCK",
            "PGP SECRET KEY BLOCK",
        ] {
            let input = format!(
                "listen = 443\n-----BEGIN {label}-----\nMIIFsecretkeybody\n-----END {label}-----\nworkers = 4\n"
            );
            let result = redact_string(&input);
            assert!(result.contains("REDACTED_PRIVATEKEY_"), "{label}: {result}");
            assert!(!result.contains("MIIFsecretkeybody"), "{label}: {result}");
            assert!(result.contains("listen = 443\n"), "{label}: {result}");
            assert!(result.contains("workers = 4"), "{label}: {result}");
        }
        // A public key block is not a secret.
        let public = "-----BEGIN PGP PUBLIC KEY BLOCK-----\nmQENBFtest\n-----END PGP PUBLIC KEY BLOCK-----\n";
        assert_eq!(redact_string(public), public);
    }

    #[test]
    fn test_pem_certificate_full_block() {
        let input = "-----BEGIN CERTIFICATE-----\nMIIDdzCCAl+gAwIBAgI...\nbase64data\n-----END CERTIFICATE-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_CERTIFICATE_"),
            "certificate block must be redacted, got: {result}"
        );
    }

    #[test]
    fn test_pem_mixed_bundle() {
        // Certificate + private key: both redacted independently.
        let input = "-----BEGIN CERTIFICATE-----\ncertdata\n-----END CERTIFICATE-----\n-----BEGIN RSA PRIVATE KEY-----\nkeydata\n-----END RSA PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_CERTIFICATE_"),
            "certificate block must be redacted in mixed bundle"
        );
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "private key block must be redacted in mixed bundle"
        );
    }

    #[test]
    fn test_pem_header_only_no_match() {
        // Header without END marker — must not match (avoids greedy consumption).
        let input = "-----BEGIN RSA PRIVATE KEY-----\nsome data but no end marker";
        let findings = scan_content(input, "/etc/ssl/key.pem");
        let key_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PrivateKey))
            .collect();
        assert!(
            key_findings.is_empty(),
            "header-only (no END marker) must not match"
        );
    }

    #[test]
    fn test_pem_adjacent_blocks() {
        // Two private key blocks in sequence — each matched independently.
        let input = "-----BEGIN RSA PRIVATE KEY-----\nkey1\n-----END RSA PRIVATE KEY-----\n-----BEGIN RSA PRIVATE KEY-----\nkey2\n-----END RSA PRIVATE KEY-----";
        let findings = scan_content(input, "/etc/ssl/keys.pem");
        let key_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PrivateKey))
            .collect();
        assert_eq!(
            key_findings.len(),
            2,
            "two adjacent key blocks must produce two findings"
        );
    }

    // --- S2: PasswordHash coverage for remaining algorithm IDs ---

    #[test]
    fn test_password_hash_md5() {
        let input = "$1$salt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/htpasswd");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "MD5 crypt hash ($1$) must be detected"
        );
    }

    #[test]
    fn test_password_hash_sha256() {
        let input = "$5$rounds=5000$salt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/htpasswd");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "SHA-256 crypt hash ($5$) must be detected"
        );
    }

    #[test]
    fn test_password_hash_scrypt() {
        let input = "$7$C6..../....$salt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/app.conf");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(hash_findings.len(), 1, "scrypt hash ($7$) must be detected");
    }

    #[test]
    fn test_password_hash_sha1() {
        let input = "$sha1$40000$salt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/app.conf");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "SHA-1 crypt hash ($sha1$) must be detected"
        );
    }

    #[test]
    fn test_password_hash_gost_yescrypt() {
        let input = "$gy$j9T$saltsalt$hashhashhashhashhashha";
        let findings = scan_content(input, "/etc/app.conf");
        let hash_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::PasswordHash))
            .collect();
        assert_eq!(
            hash_findings.len(),
            1,
            "gost-yescrypt hash ($gy$) must be detected"
        );
    }

    // --- S3: PEM edge cases — CRLF line endings and no trailing newline ---

    #[test]
    fn test_pem_crlf_line_endings() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\r\nMIIEpAIBAAKCAQEA...\r\nbase64data\r\n-----END RSA PRIVATE KEY-----\r\n";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "PEM block with CRLF line endings must be redacted, got: {result}"
        );
        assert!(
            !result.contains("BEGIN RSA PRIVATE KEY"),
            "BEGIN marker must not survive CRLF PEM redaction"
        );
    }

    #[test]
    fn test_pem_no_trailing_newline() {
        let input = "-----BEGIN EC PRIVATE KEY-----\nMHQCAQEE...\n-----END EC PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "PEM block without trailing newline must be redacted, got: {result}"
        );
    }

    // --- S4: DSA private key type ---

    #[test]
    fn test_pem_dsa_private_key_full_block() {
        let input = "-----BEGIN DSA PRIVATE KEY-----\nMIIBugIBAAKBgQ...\nbase64data\n-----END DSA PRIVATE KEY-----";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_PRIVATEKEY_"),
            "DSA private key block must be redacted, got: {result}"
        );
        assert!(
            !result.contains("BEGIN DSA PRIVATE KEY"),
            "BEGIN marker must not survive DSA key redaction"
        );
    }

    // --- Connection-string URI redaction tests ---
    // These patterns must preserve URL structure: scheme://user:REDACTED@host

    #[test]
    fn test_mongodb_uri_preserves_structure() {
        let input = "MONGODB_URL=mongodb://admin:s3cret@mongo.internal:27017/admin";
        let result = redact_string(input);
        assert!(
            !result.contains("s3cret"),
            "password must be redacted, got: {result}"
        );
        assert!(
            result
                .contains("mongodb://admin:REDACTED_MONGODBPASSWORD_1@mongo.internal:27017/admin"),
            "URL structure (scheme, user, @, host) must be preserved, got: {result}"
        );
    }

    #[test]
    fn test_mongodb_srv_uri_preserves_structure() {
        let input = "mongodb+srv://appuser:hunter2@cluster0.example.net/mydb";
        let result = redact_string(input);
        assert!(
            !result.contains("hunter2"),
            "password must be redacted, got: {result}"
        );
        assert!(
            result
                .contains("mongodb+srv://appuser:REDACTED_MONGODBPASSWORD_1@cluster0.example.net"),
            "mongodb+srv URL structure must be preserved, got: {result}"
        );
    }

    #[test]
    fn test_postgres_uri_preserves_structure() {
        let input = "DATABASE_URL=postgresql://dbuser:s3cretPass@db.example.com:5432/mydb";
        let result = redact_string(input);
        assert!(
            !result.contains("s3cretPass"),
            "password must be redacted, got: {result}"
        );
        assert!(
            result.contains("postgresql://dbuser:REDACTED_POSTGRESPASSWORD_1@db.example.com:5432"),
            "PostgreSQL URL structure must be preserved, got: {result}"
        );
    }

    #[test]
    fn test_postgres_short_scheme_preserves_structure() {
        let input = "postgres://user:secret@localhost/db";
        let result = redact_string(input);
        assert!(
            !result.contains("secret"),
            "password must be redacted, got: {result}"
        );
        assert!(
            result.contains("postgres://user:REDACTED_POSTGRESPASSWORD_1@localhost"),
            "postgres:// short scheme must be preserved, got: {result}"
        );
    }

    #[test]
    fn test_redis_uri_preserves_structure() {
        let input = "REDIS_URL=redis://:authpass@redis.internal:6379/0";
        let result = redact_string(input);
        assert!(
            !result.contains("authpass"),
            "password must be redacted, got: {result}"
        );
        assert!(
            result.contains("redis://:REDACTED_REDISPASSWORD_1@redis.internal:6379"),
            "Redis URL structure must be preserved, got: {result}"
        );
    }

    #[test]
    fn test_connection_string_deterministic_tokens() {
        // Same password in two different URIs → same token number
        let input = "mongodb://u1:shared@host1\nmongodb://u2:shared@host2";
        let result = redact_string(input);
        // Both "shared" passwords map to the same token
        let count = result.matches("REDACTED_MONGODBPASSWORD_1").count();
        assert_eq!(
            count, 2,
            "identical password values must produce the same token, got: {result}"
        );
    }

    #[test]
    fn test_connection_string_different_passwords_different_tokens() {
        let input = "mongodb://u1:pass1@host1\nmongodb://u2:pass2@host2";
        let result = redact_string(input);
        assert!(
            result.contains("REDACTED_MONGODBPASSWORD_1"),
            "first password must get token 1, got: {result}"
        );
        assert!(
            result.contains("REDACTED_MONGODBPASSWORD_2"),
            "second password must get token 2, got: {result}"
        );
    }

    #[test]
    fn test_connection_string_in_snapshot_redact() {
        // Full redact() path: connection string in a config file
        let mut snapshot = InspectionSnapshot::new();
        snapshot.config = Some(ConfigSection { withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: "/etc/myapp/db.conf".to_string(),
                content: "MONGO=mongodb://admin:s3cret@mongo:27017/db\nPG=postgresql://u:pass@pg:5432/db\nREDIS=redis://:tok@redis:6379\n".to_string(),
                ..Default::default()
            }],
        });
        redact(&mut snapshot, &RedactOptions::default());
        let content = &snapshot.config.as_ref().unwrap().files[0].content;

        // Passwords must be gone
        assert!(
            !content.contains("s3cret"),
            "mongo password must be redacted"
        );
        assert!(
            !content.contains(":pass@"),
            "postgres password must be redacted"
        );
        assert!(
            !content.contains(":tok@"),
            "redis password must be redacted"
        );

        // URL structure must be preserved
        assert!(
            content.contains("@mongo:27017"),
            "mongo host must be preserved, got: {content}"
        );
        assert!(
            content.contains("@pg:5432"),
            "postgres host must be preserved, got: {content}"
        );
        assert!(
            content.contains("@redis:6379"),
            "redis host must be preserved, got: {content}"
        );

        // Scheme prefixes must be preserved
        assert!(
            content.contains("mongodb://admin:"),
            "mongo scheme+user must be preserved, got: {content}"
        );
        assert!(
            content.contains("postgresql://u:"),
            "postgres scheme+user must be preserved, got: {content}"
        );
        assert!(
            content.contains("redis://:"),
            "redis scheme must be preserved, got: {content}"
        );
    }

    #[test]
    fn test_connection_string_scan_content_findings() {
        let input = "mongodb://admin:s3cret@mongo:27017/db";
        let findings = scan_content(input, "/etc/app.conf");
        let mongo_findings: Vec<&RedactionFinding> = findings
            .iter()
            .filter(|f| f.finding_kind == Some(FindingKind::MongodbPassword))
            .collect();
        assert_eq!(
            mongo_findings.len(),
            1,
            "MongoDB URI must produce exactly one finding"
        );
    }

    // --- Language-package manifest scrubbing in redact() ---

    /// Build a snapshot carrying one non-RPM item per manifest format, each
    /// manifest holding a credentialed registry URL.
    fn snapshot_with_credentialed_manifests() -> InspectionSnapshot {
        let cases: &[(&str, &str, &str)] = &[
            (
                "/srv/py",
                "requirements.txt",
                "--index-url https://deploy:s3cret@pypi.corp.example.com/simple\nflask==3.0.0\n",
            ),
            (
                "/srv/web",
                "package.json",
                "{\n  \"name\": \"webapp\",\n  \"publishConfig\": {\n    \"registry\": \"https://deploy:s3cret@npm.corp.example.com/\"\n  }\n}\n",
            ),
            (
                "/srv/lock",
                "package-lock.json",
                "{\n  \"packages\": {\n    \"node_modules/x\": {\n      \"resolved\": \"https://deploy:s3cret@npm.corp.example.com/x.tgz\"\n    }\n  }\n}\n",
            ),
            (
                "/srv/ruby",
                "Gemfile",
                "source 'https://deploy:s3cret@gems.corp.example.com'\ngem 'rails'\n",
            ),
            (
                "/srv/ruby",
                "Gemfile.lock",
                "GEM\n  remote: https://deploy:s3cret@gems.corp.example.com/\n  specs:\n",
            ),
        ];

        let items = cases
            .iter()
            .map(|(path, filename, content)| {
                let mut manifest_files = HashMap::new();
                manifest_files.insert(filename.to_string(), content.to_string());
                NonRpmItem {
                    path: path.to_string(),
                    manifest_files,
                    ..Default::default()
                }
            })
            .collect();

        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items,
            ..Default::default()
        });
        snap
    }

    #[test]
    fn redact_scrubs_manifest_credentials_in_every_format() {
        let mut snap = snapshot_with_credentialed_manifests();
        redact(&mut snap, &RedactOptions::default());

        for item in &snap.non_rpm_software.as_ref().unwrap().items {
            for (filename, content) in &item.manifest_files {
                assert!(
                    !content.contains("s3cret"),
                    "{filename} still carries the credential after redact(): {content}"
                );
                assert!(
                    content.contains("REDACTED"),
                    "{filename} must carry the redaction marker: {content}"
                );
            }
        }
    }

    /// The scrub has to be visible in the artifacts the operator reads.
    /// A silent rewrite leaves `secrets-review.md` empty and the README
    /// reporting `Secrets redacted | 0` over a credential that was found.
    #[test]
    fn redact_emits_a_finding_for_each_scrubbed_manifest() {
        let mut snap = snapshot_with_credentialed_manifests();
        redact(&mut snap, &RedactOptions::default());

        let manifest_findings: Vec<&RedactionFinding> = snap
            .redactions
            .iter()
            .filter(|f| f.source == "manifest_auth")
            .collect();
        assert_eq!(
            manifest_findings.len(),
            5,
            "one finding per credentialed manifest, got {manifest_findings:?}"
        );
        for f in &manifest_findings {
            assert_eq!(f.kind, RedactionKind::Inline);
            assert_eq!(f.confidence, Some(Confidence::High));
            assert!(
                !f.path.is_empty(),
                "the finding must name the manifest it came from"
            );
        }
    }

    /// A regression guard that `FullyRedacted` is still **reachable**
    /// after the manifest pass: every scrub here succeeds, so every
    /// finding is High and therefore resolved. It does not prove the
    /// label is honest — that is
    /// `redact_downgrades_when_a_manifest_value_cannot_be_located`
    /// below, which covers the case where a scrub does not succeed.
    #[test]
    fn redact_keeps_fully_redacted_reachable_for_scrubbed_manifests() {
        let mut snap = snapshot_with_credentialed_manifests();
        redact(&mut snap, &RedactOptions::default());

        assert!(
            matches!(
                snap.redaction_state,
                Some(RedactionState::FullyRedacted { .. })
            ),
            "state was {:?}",
            snap.redaction_state
        );
    }

    /// A manifest with no embedded credential must not inflate the count,
    /// and must come out byte-identical. The scrubbers rewrite content that
    /// they did not scrub — they drop the trailing newline, and
    /// `scrub_requirements_txt` re-emits its line from the trimmed form —
    /// so "the string changed" is not evidence that a credential was found.
    #[test]
    fn redact_leaves_a_credential_free_manifest_untouched() {
        let clean: &[(&str, &str)] = &[
            (
                "requirements.txt",
                "--index-url https://pypi.org/simple\nflask==3.0.0\n",
            ),
            // Indented and trailing-whitespace forms: reformatted by the
            // scrubber, but nothing was removed.
            (
                "requirements.txt",
                "  --index-url https://pypi.org/simple  \nflask==3.0.0",
            ),
            (
                "package.json",
                "{\n  \"registry\": \"https://registry.npmjs.org/\"\n}\n",
            ),
            ("Gemfile", "source 'https://rubygems.org'\ngem 'rails'\n"),
        ];

        for (filename, content) in clean {
            let mut manifest_files = HashMap::new();
            manifest_files.insert(filename.to_string(), content.to_string());
            let mut snap = InspectionSnapshot::new();
            snap.non_rpm_software = Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    path: "/srv/py".into(),
                    manifest_files,
                    ..Default::default()
                }],
                ..Default::default()
            });

            redact(&mut snap, &RedactOptions::default());

            assert!(
                snap.redactions.iter().all(|f| f.source != "manifest_auth"),
                "clean {filename} must not produce a finding: {:?}",
                snap.redactions
            );
            assert_eq!(
                &snap.non_rpm_software.as_ref().unwrap().items[0].manifest_files[*filename],
                content,
                "clean {filename} must come out byte-identical"
            );
        }
    }

    /// Re-running `redact()` on an already-scrubbed snapshot must not
    /// invent a second finding for the same credential.
    #[test]
    fn redact_is_idempotent_over_manifest_findings() {
        let mut snap = snapshot_with_credentialed_manifests();
        redact(&mut snap, &RedactOptions::default());
        let first = snap
            .redactions
            .iter()
            .filter(|f| f.source == "manifest_auth")
            .count();

        redact(&mut snap, &RedactOptions::default());
        let second = snap
            .redactions
            .iter()
            .filter(|f| f.source == "manifest_auth")
            .count();

        assert_eq!(first, 5);
        assert_eq!(second, 0, "a second pass has nothing left to scrub");
    }

    /// `METHOD_NPM_MANIFEST` items never get a sidecar
    /// (`write_language_package_manifests` routes them to `continue`), so
    /// the snapshot JSON is their only copy in the export. Scrubbing in
    /// `redact()` is what covers them; a sidecar-time scrub never would.
    #[test]
    fn redact_covers_manifest_only_items_that_get_no_sidecar() {
        let mut manifest_files = HashMap::new();
        manifest_files.insert(
            "package.json".to_string(),
            "{\n  \"registry\": \"https://deploy:s3cret@npm.corp.example.com/\"\n}\n".to_string(),
        );
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/srv/web".into(),
                method: inspectah_core::util::METHOD_NPM_MANIFEST.to_string(),
                manifest_files,
                ..Default::default()
            }],
            ..Default::default()
        });

        redact(&mut snap, &RedactOptions::default());

        let content =
            &snap.non_rpm_software.as_ref().unwrap().items[0].manifest_files["package.json"];
        assert!(
            !content.contains("s3cret"),
            "npm-manifest item must be scrubbed too: {content}"
        );
    }

    /// The honesty property, which is the one that matters: a manifest
    /// line the scrubbers cannot handle must stop the snapshot reaching
    /// `FullyRedacted`.
    ///
    /// Measured on the pre-rewrite code, four separate scrubber misses
    /// each produced `FullyRedacted` over a live credential with zero
    /// findings and `Secrets redacted | 0`. Three of those four forms
    /// are now simply scrubbed; this covers what is left, the line no
    /// scrubber can parse at all, by downgrading instead of passing in
    /// silence.
    #[test]
    fn redact_downgrades_when_a_manifest_value_cannot_be_located() {
        let mut manifest_files = HashMap::new();
        // A `"registry"` key with no colon after it: routed here by the
        // key match, and not a form the JSON scrubber can split.
        manifest_files.insert(
            "package.json".to_string(),
            "{\n  \"registry\" \"https://deploy:s3cret@npm.corp.example.com/\"\n}\n".to_string(),
        );
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            items: vec![NonRpmItem {
                path: "/srv/web".into(),
                manifest_files,
                ..Default::default()
            }],
            ..Default::default()
        });

        redact(&mut snap, &RedactOptions::default());

        let misses: Vec<&RedactionFinding> = snap
            .redactions
            .iter()
            .filter(|f| f.pattern == "manifest_value_not_located")
            .collect();
        assert_eq!(
            misses.len(),
            1,
            "the unlocatable line must be reported: {:?}",
            snap.redactions
        );
        assert_eq!(misses[0].confidence, Some(Confidence::Low));
        assert_eq!(misses[0].kind, RedactionKind::Flagged);
        assert!(
            matches!(
                snap.redaction_state,
                Some(RedactionState::PartiallyRedacted { .. })
            ),
            "a line nothing examined must not be labelled FullyRedacted, got {:?}",
            snap.redaction_state
        );

        // And the report must not itself carry the credential: these
        // findings are serialized into the export.
        let serialized = serde_json::to_string(&snap.redactions).unwrap();
        assert!(
            !serialized.contains("s3cret"),
            "the finding leaked the credential it failed to strip"
        );
    }

    /// The four forms the reviewer measured as `FullyRedacted` over a
    /// live credential. All four are scrubbed now, so the label is
    /// earned rather than merely applied.
    #[test]
    fn the_forms_that_used_to_escape_now_scrub_and_stay_fully_redacted() {
        let cases: &[(&str, &str)] = &[
            (
                "package.json",
                "{\"publishConfig\": { \"registry\": \"https://deploy:s3cret@npm.corp.example.com/\" }}",
            ),
            (
                "requirements.txt",
                "--index-url=https://deploy:s3cret@pypi.corp.example.com/simple\n",
            ),
            (
                "requirements.txt",
                "-i https://deploy:s3cret@pypi.corp.example.com/simple\n",
            ),
            (
                "Gemfile",
                "source 'https://deploy:s3cret@gems.corp.example.com' # see \"runbook\"\n",
            ),
        ];

        for (filename, content) in cases {
            let mut manifest_files = HashMap::new();
            manifest_files.insert(filename.to_string(), content.to_string());
            let mut snap = InspectionSnapshot::new();
            snap.non_rpm_software = Some(NonRpmSoftwareSection {
                items: vec![NonRpmItem {
                    path: "/srv/app".into(),
                    manifest_files,
                    ..Default::default()
                }],
                ..Default::default()
            });

            redact(&mut snap, &RedactOptions::default());

            let stored =
                &snap.non_rpm_software.as_ref().unwrap().items[0].manifest_files[*filename];
            assert!(
                !stored.contains("s3cret"),
                "{filename}: {content:?} kept its credential: {stored}"
            );
            assert!(
                snap.redactions
                    .iter()
                    .any(|f| f.pattern == "manifest_registry_auth"),
                "{filename}: the scrub must be reported"
            );
            assert!(
                snap.redactions
                    .iter()
                    .all(|f| f.pattern != "manifest_value_not_located"),
                "{filename}: this form is now locatable, not a miss"
            );
            assert!(
                matches!(
                    snap.redaction_state,
                    Some(RedactionState::FullyRedacted { .. })
                ),
                "{filename}: state was {:?}",
                snap.redaction_state
            );
        }
    }
    /// A snapshot whose `/etc/sysctl.d/80-app.conf` config entry holds
    /// `config` and whose sysctl copy holds `sysctl`, both as read.
    fn sysctl_and_config_snapshot(config: &str, sysctl: &str) -> InspectionSnapshot {
        use inspectah_core::types::kernelboot::KernelBootSection;
        use inspectah_core::types::sysctl::{SysctlDir, SysctlFile};
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: SYSCTL_APP_CONF.to_string(),
                content: config.to_string(),
                ..Default::default()
            }],
        });
        snap.kernel_boot = Some(KernelBootSection {
            sysctl: Some(SysctlSection {
                files: vec![SysctlFile {
                    dir: SysctlDir::Etc,
                    name: "80-app.conf".into(),
                    body: SysctlFileBody::Read {
                        link_target: None,
                        content: sysctl.to_string(),
                        redacted: false,
                    },
                }],
                runtime: Default::default(),
                redacted_runtime: Vec::new(),
            }),
            ..Default::default()
        });
        snap
    }

    const SYSCTL_APP_CONF: &str = "/etc/sysctl.d/80-app.conf";

    fn sysctl_copy(snap: &InspectionSnapshot) -> (&str, bool) {
        let section = snap
            .kernel_boot
            .as_ref()
            .and_then(|kb| kb.sysctl.as_ref())
            .expect("the sysctl section");
        match &section.files[0].body {
            SysctlFileBody::Read {
                content, redacted, ..
            } => (content, *redacted),
            other => panic!("expected a read file, got {other:?}"),
        }
    }

    /// A sysctl file's `redacted` flag, once set, stays set. A second pass
    /// over redacted text can replace each token with itself and change
    /// nothing, and assigning the flag from that pass cleared it, which
    /// made the report state values from a file redaction had changed.
    #[test]
    fn test_sysctl_redacted_flag_survives_a_second_pass() {
        let mut snap = sysctl_and_config_snapshot("", "kernel.x = password=hunter2\n");
        redact(&mut snap, &RedactOptions::default());
        let (content, redacted) = sysctl_copy(&snap);
        assert_eq!(content, "kernel.x = password=REDACTED_PASSWORD_1\n");
        assert!(redacted);
        redact(&mut snap, &RedactOptions::default());
        let (content, redacted) = sysctl_copy(&snap);
        assert_eq!(content, "kernel.x = password=REDACTED_PASSWORD_1\n");
        assert!(redacted, "a second pass cleared the flag");
    }

    /// The config entry and the sysctl copy of one path read differently
    /// (the file changed between the two collectors' reads), but their
    /// findings are equal field for field, since a finding holds no
    /// matched text. Both are separate secrets and both findings stay.
    ///
    /// Not collector-driven: the scan harness's `MockExecutor` returns one
    /// content per path, so a scan cannot produce two different reads of
    /// the same file. The snapshot is planted here instead.
    #[test]
    fn test_sysctl_copy_read_differently_keeps_both_findings() {
        let mut snap = sysctl_and_config_snapshot(
            "db_password = canary-config\n",
            "db_password = canary-sysctl\n",
        );
        redact(&mut snap, &RedactOptions::default());

        let written = serde_json::to_string(&snap).unwrap();
        assert!(!written.contains("canary-config"), "{written}");
        assert!(!written.contains("canary-sysctl"), "{written}");
        let config = &snap.config.as_ref().unwrap().files[0].content;
        assert!(config.contains("REDACTED_"), "{config:?}");
        let (content, redacted) = sysctl_copy(&snap);
        assert!(content.contains("REDACTED_"), "{content:?}");
        assert!(redacted);
        let at_path: Vec<_> = snap
            .redactions
            .iter()
            .filter(|f| f.path == SYSCTL_APP_CONF)
            .collect();
        assert_eq!(at_path.len(), 2, "{at_path:?}");
        assert_eq!(at_path[0], at_path[1], "the two findings are equal");
    }

    /// The same text in both copies is one match recorded once, and the
    /// shared registry gives both copies the same token.
    #[test]
    fn test_sysctl_copy_read_identically_is_one_finding() {
        let text = "db_password = canary-same\n";
        let mut snap = sysctl_and_config_snapshot(text, text);
        redact(&mut snap, &RedactOptions::default());

        let config = snap.config.as_ref().unwrap().files[0].content.clone();
        let (content, redacted) = sysctl_copy(&snap);
        assert!(!content.contains("canary-same"), "{content:?}");
        assert_eq!(content, config, "one token for one secret");
        assert!(redacted);
        let at_path = snap
            .redactions
            .iter()
            .filter(|f| f.path == SYSCTL_APP_CONF)
            .count();
        assert_eq!(at_path, 1, "{:?}", snap.redactions);
    }

    /// One config file holding `content`, as a scan would collect it.
    fn config_snapshot(path: &str, content: &str) -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.config = Some(ConfigSection {
            withheld: Vec::new(),
            files: vec![ConfigFileEntry {
                path: path.to_string(),
                content: content.to_string(),
                ..Default::default()
            }],
        });
        snap
    }

    /// Every input here was fully redacted when the whole match was
    /// replaced. Keeping the key must not cost any of that coverage: each
    /// listed secret is gone after `redact_string()` and after snapshot
    /// `redact()`, the snapshot is not labelled `FullyRedacted` over a
    /// surviving one, and a second pass changes nothing.
    #[test]
    fn test_inputs_redacted_by_whole_match_replacement_stay_redacted() {
        let cases: &[(&str, &[&str])] = &[
            (
                "jdbc:mysql://db/app?password=first&password=second",
                &["first", "second"],
            ),
            (
                "jdbc:mysql://db/app?password=first-secret&password=second-secret",
                &["first-secret", "second-secret"],
            ),
            (
                "jdbc:mysql://db/app?token=api-secret&password=db-secret",
                &["api-secret", "db-secret"],
            ),
            (
                "jdbc:mysql://db/app?next=postgres://u:inner@pg&password=outer",
                &["inner", "outer"],
            ),
            ("password=REDACTED_COMPANY_2026", &["COMPANY_2026"]),
            (
                "password=REDACTED_THIS_IS_THE_REAL_SECRET_123",
                &["THIS_IS_THE_REAL_SECRET_123"],
            ),
            ("token: REDACTED_PASSWORD_9", &["REDACTED_PASSWORD_9"]),
            ("secret=REDACTED_NOSUCHKIND_4", &["NOSUCHKIND"]),
            ("postgres://u:two words@db", &["two words"]),
            ("mongodb://u:two words@db", &["two words"]),
            ("redis://:two words@db", &["two words"]),
            ("postgres://u:tab\tbed@db", &["tab\tbed"]),
            ("mongodb+srv://u:tab\tbed@db", &["tab\tbed"]),
        ];
        let mut failures = Vec::new();
        for (input, secrets) in cases {
            let once = redact_string(input).into_owned();
            for secret in *secrets {
                if once.contains(secret) {
                    failures.push(format!("redact_string: {secret:?} survives in {once:?}"));
                }
            }
            let twice = redact_string(&once);
            if twice != once.as_str() {
                failures.push(format!("second pass of {input:?}: {once:?} -> {twice:?}"));
            }

            let mut snap = config_snapshot("/etc/app/app.conf", input);
            redact(&mut snap, &RedactOptions::default());
            let content = &snap.config.as_ref().unwrap().files[0].content;
            let written = serde_json::to_string(&snap).unwrap();
            let survivors: Vec<&&str> = secrets
                .iter()
                .filter(|s| content.contains(**s) || written.contains(**s))
                .collect();
            if !survivors.is_empty() {
                failures.push(format!(
                    "redact(): {survivors:?} survive from {input:?} under {:?}",
                    snap.redaction_state
                ));
            }
            if snap.redactions.is_empty() {
                failures.push(format!("redact(): no finding for {input:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A JDBC URL that carries a credential anywhere has everything after
    /// `scheme://` replaced, up to the next whitespace: the authority, host,
    /// database and query string go together. Parsing the URL into parts
    /// shipped a password with no user before it, and the part of one
    /// holding a `?`. A second pass changes nothing, as the replaced URL
    /// has no `@` or `password=` left.
    #[test]
    fn test_a_jdbc_url_with_a_credential_is_replaced_after_its_scheme() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "jdbc:mysql://u:authority-secret@db/app?password=query-secret",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["authority-secret", "query-secret"],
            ),
            (
                "jdbc:mysql://:authority-secret@db/app?password=query-secret",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["authority-secret", "query-secret"],
            ),
            (
                "jdbc:mysql://u:pa?ss@db/app?password=outer",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["pa?", "ss@", "outer"],
            ),
            (
                "jdbc:mysql://:only-secret@db/app",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["only-secret"],
            ),
            (
                "jdbc:mysql://u:spaced secret@db/app",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["spaced", "secret@"],
            ),
            (
                "url=jdbc:postgresql://u:inner@pg/app?password=outer tail",
                "url=jdbc:postgresql://REDACTED_JDBCPASSWORD_1 tail",
                &["inner", "outer"],
            ),
            // No credential, no change.
            (
                "jdbc:mysql://db/app?ssl=1&password=",
                "jdbc:mysql://db/app?ssl=1&password=",
                &[],
            ),
        ];
        for (input, want, secrets) in cases {
            let once = redact_string(input).into_owned();
            assert_eq!(&once, want, "{input:?}");
            assert_eq!(
                redact_string(&once),
                once.as_str(),
                "second pass of {input:?}"
            );

            let mut snap = config_snapshot("/etc/app/app.conf", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(&snap.config.as_ref().unwrap().files[0].content, want);
            let written = serde_json::to_string(&snap).unwrap();
            for secret in *secrets {
                assert!(!written.contains(secret), "{secret:?} from {input:?}");
            }
            let findings = scan_content(input, "/etc/app.conf");
            assert_eq!(
                findings.len(),
                usize::from(!secrets.is_empty()),
                "{input:?}"
            );
        }
    }

    /// Every input a review of redaction showed keeping secret bytes, from
    /// the first round to the last, through `redact_string()` and the
    /// snapshot path: the output is exactly the one given, no listed secret
    /// survives in it or anywhere in the serialized snapshot, and the
    /// snapshot is `FullyRedacted` with a finding. Redaction may take the
    /// quote, tag or text around a value with it; it never leaves any of
    /// the value behind. A value's end is not read off its punctuation, so
    /// an unquoted value runs to the next whitespace whatever it holds.
    /// The snapshot path is a shell-sourced file, where whitespace ends a
    /// value as it does for `redact_string()`; in an INI-style file a key
    /// that opens its line takes the rest of the line
    /// (`test_value_boundaries_follow_the_format`).
    #[test]
    fn test_no_secret_byte_survives_a_reviewed_input() {
        let cases: &[(&str, &str, &[&str])] = &[
            // A quote, `<` or `>` and punctuation after it are value.
            (
                r#"password=ab"/tail"#,
                "password=REDACTED_PASSWORD_1",
                &[r#"ab"/tail"#],
            ),
            (
                "password=ab',tail",
                "password=REDACTED_PASSWORD_1",
                &["ab',tail"],
            ),
            (
                "password=ab<;tail",
                "password=REDACTED_PASSWORD_1",
                &["ab<;tail"],
            ),
            (
                "password=ab>)tail",
                "password=REDACTED_PASSWORD_1",
                &["ab>)tail"],
            ),
            (
                r#"password=ab"]tail"#,
                "password=REDACTED_PASSWORD_1",
                &[r#"ab"]tail"#],
            ),
            (
                "password=ab'}tail",
                "password=REDACTED_PASSWORD_1",
                &["ab'}tail"],
            ),
            (
                r#"password="ab"/tail"#,
                "password=REDACTED_PASSWORD_1",
                &[r#""ab"/tail"#],
            ),
            (
                "password='ab'/tail",
                "password=REDACTED_PASSWORD_1",
                &["'ab'/tail"],
            ),
            (
                r#"password="ab\\"/tail"#,
                "password=REDACTED_PASSWORD_1",
                &[r#"ab\\"/tail"#],
            ),
            (
                r#"password="ab\"/tail"#,
                "password=REDACTED_PASSWORD_1",
                &[r#"ab\"/tail"#],
            ),
            // A value ending in a quote or `>` loses that too.
            (
                r#"password=ab""#,
                "password=REDACTED_PASSWORD_1",
                &[r#"ab""#],
            ),
            ("password=<x>", "password=REDACTED_PASSWORD_1", &["<x>"]),
            (
                r#"password=zsecret",zrest"#,
                "password=REDACTED_PASSWORD_1",
                &["zsecret", "zrest"],
            ),
            // An empty quoted prefix does not hide what follows it.
            (
                r#"password=""hunter2"#,
                "password=REDACTED_PASSWORD_1",
                &["hunter2"],
            ),
            (
                "password=''hunter2",
                "password=REDACTED_PASSWORD_1",
                &["hunter2"],
            ),
            (
                r#"DB_PASSWORD=""hunter2"#,
                "DB_PASSWORD=REDACTED_PASSWORD_1",
                &["hunter2"],
            ),
            (
                "DB_PASSWORD=''hunter2",
                "DB_PASSWORD=REDACTED_PASSWORD_1",
                &["hunter2"],
            ),
            // A quoted NSS/PAM word is a value, not the exempt keyword.
            (
                r#"password="files""#,
                "password=REDACTED_PASSWORD_1",
                &[r#""files""#],
            ),
            ("secret='ldap'", "secret=REDACTED_PASSWORD_1", &["ldap"]),
            (
                r#"password=files",hunter2"#,
                "password=REDACTED_PASSWORD_1",
                &["hunter2"],
            ),
            // A JDBC URL with a credential loses everything after its
            // scheme, through the next whitespace after its last marker.
            (
                r#"<db url="jdbc:mysql://db/app?sig=signsecret&password=\\"/>"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1"#,
                &["signsecret"],
            ),
            (
                r#"<db url="jdbc:mysql://db/app?sig=signsecret&password=\\"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1"#,
                &["signsecret"],
            ),
            (
                r#"url: "<db url='>jdbc:mysql://u:pa'ss@db/app?password=aux""#,
                r#"url: "<db url='>jdbc:mysql://REDACTED_JDBCPASSWORD_1"#,
                &["pa'ss", "=aux"],
            ),
            (
                r#"<db url="jdbc:mysql://u:secret\@db/app"/>"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1"#,
                &[r"u:secret\@"],
            ),
            // Earlier rounds: several credentials in one JDBC URL.
            (
                "jdbc:mysql://db/app?password=zfirst&password=zsecond",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["zfirst", "zsecond"],
            ),
            (
                "jdbc:mysql://db/app?token=api-secret&password=db-secret",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["api-secret", "db-secret"],
            ),
            (
                "jdbc:mysql://db/app?next=postgres://u:zinner@pg&password=zouter",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["zinner", "zouter"],
            ),
            (
                "jdbc:mysql://:authority-secret@db/app?password=query-secret",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["authority-secret", "query-secret"],
            ),
            (
                "jdbc:mysql://u:zpa?ss@db/app?password=zouter",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["zpa?ss", "zouter"],
            ),
            (
                "jdbc:postgresql://u:zinner@pg/app?password=zouter",
                "jdbc:postgresql://REDACTED_JDBCPASSWORD_1",
                &["zinner", "zouter"],
            ),
            (
                "jdbc:sqlserver://host;databaseName=app;user=sa;password=zsecret;encrypt=true",
                "jdbc:sqlserver://REDACTED_JDBCPASSWORD_1",
                &["zsecret"],
            ),
            (
                "url=jdbc:mysql://u:zsecret@db/app pool=4 password=zx tail=5",
                "url=jdbc:mysql://REDACTED_JDBCPASSWORD_1 tail=5",
                &["zsecret", "=zx"],
            ),
            (
                "jdbc:mysql://app:ztwo zwords@db",
                "jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["ztwo", "zwords"],
            ),
            // Earlier rounds: a value spelled like a token, URI passwords
            // with a space, and bridges between patterns.
            (
                "password=REDACTED_COMPANY_2026",
                "password=REDACTED_PASSWORD_1",
                &["COMPANY_2026"],
            ),
            (
                "postgres://u:ztwo zwords@db",
                "postgres://u:REDACTED_POSTGRESPASSWORD_1@db",
                &["ztwo", "zwords"],
            ),
            (
                "mongodb://u:ztwo zwords@db",
                "mongodb://u:REDACTED_MONGODBPASSWORD_1@db",
                &["ztwo", "zwords"],
            ),
            (
                "redis://:ztwo zwords@db",
                "redis://:REDACTED_REDISPASSWORD_1@db",
                &["ztwo", "zwords"],
            ),
            (
                "db_password=postgres://u:a token=b@abcdefghijklmnopqrstuvwxyz",
                "db_password=REDACTED_PASSWORD_1",
                &["a token=b", "abcdefghijklmnopqrstuvwxyz"],
            ),
            (
                "postgres://longusername:token=za@redis://:zb longsecret@h",
                "postgres://longusername:REDACTED_POSTGRESPASSWORD_1@h",
                &["token=za", "zb longsecret"],
            ),
            (
                "password=postgres://u:zp zw@db",
                "password=REDACTED_PASSWORD_1@db",
                &["zp zw"],
            ),
            // Earlier rounds: AWS key ids.
            (
                "AKIAABCDEFGHIJKLMNOP AKIAQRSTUVWXYZ234567",
                "REDACTED_AWSKEY_2 REDACTED_AWSKEY_1",
                &["ABCDEFGHIJKLMNOP", "QRSTUVWXYZ234567"],
            ),
            (
                "# note\nAKIAABCDEFGHIJKLMNOP",
                "# note\nREDACTED_AWSKEY_1",
                &["ABCDEFGHIJKLMNOP"],
            ),
            // A JSON string under a credential key, quotes and all.
            (
                r#"{"password": "zsecret"}"#,
                r#"{"password": REDACTED_PASSWORD_1"#,
                &["zsecret"],
            ),
            (
                r#"{"token": "\/zsecret"}"#,
                r#"{"token": REDACTED_PASSWORD_1"#,
                &["zsecret"],
            ),
            (
                r#"{"db_password": "a\"ztwo zwords", "pool": 4}"#,
                r#"{"db_password": REDACTED_PASSWORD_1 "pool": 4}"#,
                &["ztwo", "zwords"],
            ),
            (
                r#"{"password": "files"}"#,
                r#"{"password": REDACTED_PASSWORD_1"#,
                &[r#""files""#],
            ),
            // A quoted value under an unquoted key: `\S+` stopped at the
            // first space inside the quotes.
            (
                r#"password: "two words""#,
                "password: REDACTED_PASSWORD_1",
                &["two w", "o words"],
            ),
            (
                "password='two words'",
                "password=REDACTED_PASSWORD_1",
                &["two w", "o words"],
            ),
            (
                r#"secret = "a b c""#,
                "secret = REDACTED_PASSWORD_1",
                &["a b", "b c"],
            ),
            (
                r#"DB_PASSWORD="x y" tail=1"#,
                "DB_PASSWORD=REDACTED_PASSWORD_1 tail=1",
                &["x y", "y\""],
            ),
            (
                r#"password: "files""#,
                "password: REDACTED_PASSWORD_1",
                &[r#""files""#],
            ),
            // Unterminated: to the end of the line.
            ("password: \"a b", "password: REDACTED_PASSWORD_1", &["a b"]),
            (
                "password: \"a\tb c\"",
                "password: REDACTED_PASSWORD_1",
                &["a\tb", "b c"],
            ),
            // A value that runs on into the next key does not hide that
            // key: each key is looked for from where the one before it
            // ends, not from where its value ends.
            (
                r#"{"password":"alpha","token":"beta gamma"}"#,
                r#"{"password":REDACTED_PASSWORD_1"#,
                // Bare `beta` is in the snapshot's own version string.
                &["alpha", "\"beta", "gamma"],
            ),
            (
                r#"{"password":"alpha","token": "bravo"}"#,
                r#"{"password":REDACTED_PASSWORD_2 REDACTED_PASSWORD_1"#,
                &["alpha", "bravo"],
            ),
            (
                r#"{"password":"za1","token":"zb2","secret":"zc3 zd4"}"#,
                r#"{"password":REDACTED_PASSWORD_1"#,
                &["za1", "zb2", "zc3", "zd4"],
            ),
            (
                r#"{"password":"za1","token": "zb2","secret": "zc3"}"#,
                r#"{"password":REDACTED_PASSWORD_3 REDACTED_PASSWORD_2 REDACTED_PASSWORD_1"#,
                &["za1", "zb2", "zc3"],
            ),
            // Unquoted values still stop at whitespace: `c` is not a value.
            (
                "password=za token=zb c",
                "password=REDACTED_PASSWORD_2 token=REDACTED_PASSWORD_1 c",
                &["za", "zb"],
            ),
            // A database URL password holding an `@` is read to the last
            // `@` of its authority, not cut at the first.
            (
                "url=postgres://u:p@pgtail9@db/app",
                "url=postgres://u:REDACTED_POSTGRESPASSWORD_1@db/app",
                &["pgtail9"],
            ),
            (
                "url=mongodb://u:p@mgtail9@db/app",
                "url=mongodb://u:REDACTED_MONGODBPASSWORD_1@db/app",
                &["mgtail9"],
            ),
            (
                "url=redis://:p@rdtail9@db/0",
                "url=redis://:REDACTED_REDISPASSWORD_1@db/0",
                &["rdtail9"],
            ),
        ];
        for (input, want, secrets) in cases {
            let once = redact_string(input).into_owned();
            assert_eq!(&once, want, "{input:?}");

            let mut snap = config_snapshot("/etc/sysconfig/app", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(
                &snap.config.as_ref().unwrap().files[0].content,
                want,
                "{input:?}"
            );
            assert!(!snap.redactions.is_empty(), "{input:?}");
            assert!(
                matches!(
                    snap.redaction_state,
                    Some(RedactionState::FullyRedacted { .. })
                ),
                "{input:?}"
            );
            let written = serde_json::to_string(&snap).unwrap();
            for secret in *secrets {
                let escaped = serde_json::to_string(secret).unwrap();
                let escaped = &escaped[1..escaped.len() - 1];
                assert!(
                    !once.contains(secret),
                    "{secret:?} from {input:?}: {once:?}"
                );
                assert!(!written.contains(escaped), "{secret:?} from {input:?}");
            }
        }
    }

    /// What redaction costs instead of a secret byte: a quoted or XML
    /// value loses its closing quote, and its closing tag when the value
    /// runs into it, because where a value ends is not read off its
    /// punctuation. Format-aware redaction is what keeps them; until then
    /// these outputs are the expected ones.
    #[test]
    fn test_a_quoted_or_xml_value_loses_its_closing_syntax() {
        let cases: &[(&str, &str)] = &[
            // Tomcat context.xml
            (
                r#"<Resource name="jdbc/app" username="app" password="zsecret" url="jdbc:mysql://db/app"/>"#,
                r#"<Resource name="jdbc/app" username="app" password=REDACTED_PASSWORD_1 url="jdbc:mysql://db/app"/>"#,
            ),
            (
                r#"<Resource password="zsecret"/>"#,
                "<Resource password=REDACTED_PASSWORD_1",
            ),
            // A quoted JDBC URL
            (
                r#"url="jdbc:mysql://u:zsecret@db/app" pool=4"#,
                r#"url="jdbc:mysql://REDACTED_JDBCPASSWORD_1 pool=4"#,
            ),
            (
                r#"<db url="jdbc:mysql://u:zsecret@db/app" pool="4"/>"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1 pool="4"/>"#,
            ),
            (
                "<url>jdbc:mysql://u:zsecret@db/app</url>",
                "<url>jdbc:mysql://REDACTED_JDBCPASSWORD_1",
            ),
            // A JSON string under a credential key
            (
                r#"{"password": "zsecret", "pool": 4}"#,
                r#"{"password": REDACTED_PASSWORD_1 "pool": 4}"#,
            ),
            // A quoted value under an unquoted key, YAML and shell
            (
                r#"password: "zsecret two""#,
                "password: REDACTED_PASSWORD_1",
            ),
            (
                "DB_PASSWORD='zsecret two' export=1",
                "DB_PASSWORD=REDACTED_PASSWORD_1 export=1",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact_string(input), *want, "{input:?}");
            let mut snap = config_snapshot("/etc/app/context.xml", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(&snap.config.as_ref().unwrap().files[0].content, want);
        }
    }

    /// A URI's user and password classes cross `/`. A password written
    /// with a literal `/` (base64 passwords hold them) reads the same as a
    /// password-less URI whose query has an `@`, so the patterns keep the
    /// first covered and over-redact the second: the port, path and query
    /// before the `@` go with it.
    #[test]
    fn test_uri_password_with_a_slash_is_covered_at_the_cost_of_a_query_at() {
        let cases: &[(&str, &str)] = &[
            (
                "postgres://u:pa/zsecret@host/db",
                "postgres://u:REDACTED_POSTGRESPASSWORD_1@host/db",
            ),
            (
                "mongodb://u:pa/zsecret@host/db",
                "mongodb://u:REDACTED_MONGODBPASSWORD_1@host/db",
            ),
            // The cost.
            (
                "postgres://host:5432/db?u=a@b",
                "postgres://host:REDACTED_POSTGRESPASSWORD_1@b",
            ),
            (
                "mongodb://host:27017/db?authSource=a@b",
                "mongodb://host:REDACTED_MONGODBPASSWORD_1@b",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact_string(input), *want, "{input:?}");
        }
    }

    /// The generic match wins on `password=postgres://u:p w@db` (its whole
    /// match is longer), and its `\S+` value stops at the space inside the
    /// URI password. The URI match's secret runs on past it, so the winner
    /// absorbs it: dropping the URI match, as the whole-match replacement
    /// also did, shipped ` w`.
    #[test]
    fn test_overlap_absorbs_a_uri_password_the_generic_value_stops_inside() {
        let input = "password=postgres://u:p w@db";
        assert_eq!(redact_string(input), "password=REDACTED_PASSWORD_1@db");
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    /// A high-confidence hint on a collected `.env` file is resolved only
    /// when every value of its variable, to the end of the line or a `#`
    /// comment, is a token this run issued. Each file also has
    /// `password=seed`, so `REDACTED_PASSWORD_1` is issued and the check
    /// cannot pass merely because the token is unknown. No pattern handles
    /// `DB_KEY`, so its line comes through as written and its hint stays
    /// unresolved, whatever follows the token: a `/`, a `:` or a space.
    #[test]
    fn test_hint_with_a_token_prefixed_value_stays_unresolved() {
        use inspectah_core::types::nonrpm::NonRpmSoftwareSection;
        let seeded = "password=seed\n";
        let redacted_seed = "password=REDACTED_PASSWORD_1\n";
        let cases = [
            ("DB_KEY", "DB_KEY=REDACTED_PASSWORD_1/hunter2\n", false),
            ("DB_KEY", "DB_KEY=REDACTED_PASSWORD_1:still-secret\n", false),
            ("DB_KEY", "DB_KEY=REDACTED_PASSWORD_1 still-secret\n", false),
            (
                "DB_KEY",
                "DB_KEY=REDACTED_PASSWORD_1\tstill-secret\n",
                false,
            ),
            // A `.env` value runs to the end of its line, so ` words` is
            // replaced with the rest of the value and the hint resolved.
            ("DB_PASSWORD", "DB_PASSWORD=two words\n", true),
            // A replaced value followed by a comment or nothing is handled.
            ("DB_PASSWORD", "DB_PASSWORD=hunter2 # rotated\n", true),
            ("DB_PASSWORD", "DB_PASSWORD=hunter2  \r\n", true),
        ];
        for (var, line, resolved) in cases {
            let content = format!("{line}{seeded}");
            let mut snap = InspectionSnapshot::new();
            snap.non_rpm_software = Some(NonRpmSoftwareSection {
                env_files: vec![ConfigFileEntry {
                    path: "opt/app/.env".to_string(),
                    content: content.clone(),
                    ..Default::default()
                }],
                ..Default::default()
            });
            snap.redaction_hints = vec![RedactionHint {
                path: "opt/app/.env".into(),
                reason: format!("environment variable '{var}' in .env file may contain a secret"),
                confidence: Some(Confidence::High),
            }];
            redact(&mut snap, &RedactOptions::default());
            let out = &snap.non_rpm_software.as_ref().unwrap().env_files[0].content;
            assert!(out.ends_with(redacted_seed), "{out:?}");
            if var == "DB_KEY" {
                assert!(out.starts_with(line), "no pattern handles DB_KEY: {out:?}");
            }
            match (&snap.redaction_state, resolved) {
                (Some(RedactionState::FullyRedacted { .. }), true) => {}
                (
                    Some(RedactionState::PartiallyRedacted {
                        unresolved_hints, ..
                    }),
                    false,
                ) => assert!(
                    unresolved_hints.iter().any(|h| h.path == "opt/app/.env"),
                    "{unresolved_hints:?}"
                ),
                (other, _) => panic!("{line:?}: resolved should be {resolved}, got {other:?}"),
            }
        }
    }

    /// A payload is redacted by its path's rules: a compose file by the
    /// patterns and the compose pass, any other text by the URL maskers
    /// and the patterns. Key delimiters and certificates are left alone,
    /// and a file with nothing to redact gives `None`.
    #[test]
    fn redact_payload_follows_the_file_rules() {
        let payload = redact_payload;
        let compose = "services:\n  db:\n    environment:\n      POSTGRES_PASSWORD: pgsecret99\n";
        let (text, findings) =
            payload(compose, "/opt/app/docker-compose.yml").expect("compose redacts");
        assert!(!text.contains("pgsecret99"), "{text}");
        assert!(!findings.is_empty());

        let git = "[remote \"origin\"]\n\turl = https://deploy:gitsecret77@git.example.com/r.git\n";
        let (text, findings) = payload(git, "/opt/app/repo/.git/config").expect("masks");
        assert!(!text.contains("gitsecret77"), "{text}");
        assert_eq!(findings[0].line, Some(2));

        let env = "API_TOKEN=supersecretvalue123\n";
        let (text, _) = payload(env, "/opt/app/.env").expect("env redacts");
        assert!(!text.contains("supersecretvalue123"), "{text}");

        for kept in [
            "const begin = \"-----BEGIN PRIVATE KEY-----\", end = \"-----END PRIVATE KEY-----\";\n",
            "{\"begin\":\"-----BEGIN PRIVATE KEY-----\",\"end\":\"-----END PRIVATE KEY-----\"}\n",
            "echo \"-----BEGIN PRIVATE KEY-----\"\n",
            "pem = \"-----BEGIN RSA PRIVATE KEY-----\\n\" + body\n",
            "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ\n-----END CERTIFICATE-----\n",
            "port = 8080\n",
        ] {
            assert!(payload(kept, "/opt/app/x").is_none(), "{kept}");
        }
    }

    /// One base64 line, as long as the smallest real key's body.
    const KEY_LINE: &str = "MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXEBHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp";

    /// `KEY_LINE` cut into groups of `width`, joined by `sep`.
    fn split_key(width: usize, sep: &str) -> String {
        KEY_LINE
            .as_bytes()
            .chunks(width)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// Whether `text` still holds `KEY_LINE` once every byte outside the
    /// base64 alphabet is dropped: recoverable key material.
    fn holds_key(text: &str) -> bool {
        let packed: String = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "+/=".contains(*c))
            .collect();
        packed.contains(&KEY_LINE[..32]) || packed.contains(&KEY_LINE[32..])
    }

    /// A private-key block is left alone only when nothing but delimiters
    /// sits between its markers. A body in any spacing, split by other
    /// text, or after a marker nothing closes is replaced; the markers
    /// stay, and so does the rest of the file.
    #[test]
    fn payload_key_blocks_are_exempt_only_when_delimiter_only() {
        let begin = "-----BEGIN PRIVATE KEY-----";
        let end = "-----END PRIVATE KEY-----";
        let cases = [
            (
                "space split, closed, beside a token",
                format!(
                    "TLS_KEY=\"{begin} {} {end}\"\nAPI_TOKEN=supersecretvalue123\n",
                    split_key(4, " ")
                ),
            ),
            (
                "space split over lines",
                format!("key: |\n  {begin}\n  {}\n  {end}\n", split_key(4, "  \n  ")),
            ),
            (
                "comma split",
                format!("k = [\"{begin}\",{},\"{end}\"]\n", split_key(4, ",")),
            ),
            (
                "space split, no END marker",
                format!("TLS_KEY={begin} {}\nMODE=prod\n", split_key(4, " ")),
            ),
            (
                "escaped newlines, no END marker",
                format!("{{\"key\": \"{begin}\\n{KEY_LINE}\\n\"}}\n"),
            ),
        ];
        for (label, content) in cases {
            let (text, findings) = redact_payload(&content, "/opt/app/app.env")
                .unwrap_or_else(|| panic!("{label}: redacted"));
            assert!(!holds_key(&text), "{label}: {text}");
            assert!(!text.contains("supersecretvalue123"), "{label}: {text}");
            assert!(
                findings
                    .iter()
                    .any(|f| f.finding_kind == Some(FindingKind::PrivateKey)),
                "{label}: {findings:?}"
            );
            assert!(text.contains(begin), "{label}: the marker stays: {text}");
        }
        // The line after a key that nothing closes keeps its text.
        let (text, _) = redact_payload(
            &format!(
                "TLS_KEY={begin} {}\nAPI_TOKEN=supersecretvalue123\n",
                split_key(4, " ")
            ),
            "/opt/app/app.env",
        )
        .unwrap();
        assert!(text.contains("\nAPI_TOKEN=REDACTED_"), "{text}");
    }

    /// Exempt patterns are left out before overlaps are resolved, so a
    /// credential between private-key or certificate markers is redacted,
    /// with a finding, and the markers stay; a real certificate stays
    /// whole, also when a token sits beside it.
    #[test]
    fn payload_exemptions_never_hide_a_credential() {
        for (begin, end) in [
            ("-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----"),
            ("-----BEGIN CERTIFICATE-----", "-----END CERTIFICATE-----"),
        ] {
            let json = format!(
                "{{\n  \"begin\": \"{begin}\",\n  \"api_token\": \"supersecretvalue123\",\n  \"end\": \"{end}\"\n}}\n"
            );
            let (text, findings) = redact_payload(&json, "/opt/app/tmpl.json")
                .unwrap_or_else(|| panic!("{begin}: redacted"));
            assert!(!text.contains("supersecretvalue123"), "{text}");
            assert!(text.contains(begin) && text.contains(end), "{text}");
            assert!(!findings.is_empty());
        }
        let cert =
            "-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUQ\n-----END CERTIFICATE-----\n";
        let (text, _) = redact_payload(
            &format!("{cert}password=hunter2hunter2\n{cert}"),
            "/opt/app/bundle.pem",
        )
        .expect("the password is redacted");
        assert_eq!(text.matches(cert).count(), 2, "{text}");
        assert!(!text.contains("hunter2hunter2"), "{text}");
    }

    /// Every match is found in the original text, so a masker never
    /// hides a key from the pattern pass. The finding's reproducer: a
    /// proxy password whose value opens a private key whose body is split
    /// by spaces; the masker replaces only `-----BEGIN`, which would leave
    /// the body unrecognized. The key is gone, in a payload and in a
    /// compose file, and a URL masked after a multi-line key keeps its
    /// own line number.
    #[test]
    fn payload_matches_come_from_the_original_text() {
        let body = split_key(4, " ");
        let proxy = format!(
            "proxy_password=-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n"
        );
        for path in ["/opt/app/dnf.conf", "/opt/app/.env"] {
            let (text, findings) = redact_payload(&proxy, path).expect("redacted");
            assert!(!holds_key(&text), "{path}: {text}");
            assert!(
                findings
                    .iter()
                    .any(|f| f.finding_kind == Some(FindingKind::PrivateKey)),
                "{path}: {findings:?}"
            );
        }
        let compose = format!(
            "services:\n  app:\n    environment:\n      DB_PASSWORD: -----BEGIN PRIVATE KEY-----\n        {body}\n        -----END PRIVATE KEY-----\n"
        );
        let (text, _) = redact_payload(&compose, "/opt/app/docker-compose.yml").expect("redacted");
        assert!(!holds_key(&text), "{text}");

        let git = format!(
            "-----BEGIN PRIVATE KEY-----\n{KEY_LINE}\n-----END PRIVATE KEY-----\n\
             url = https://deploy:gitsecret77@git.example.com/r.git\n"
        );
        let (text, findings) = redact_payload(&git, "/opt/app/repo.cfg").expect("redacted");
        assert!(!text.contains("gitsecret77") && !holds_key(&text), "{text}");
        assert!(
            findings
                .iter()
                .any(|f| f.source == "proxy_credential" && f.line == Some(4)),
            "{findings:?}"
        );
    }

    /// A compose file takes the URL credential maskers too: a password in
    /// a `command:` URL, which the compose pass does not read as an
    /// assignment, is masked. A compose file with no credential is left
    /// as it is.
    #[test]
    fn compose_payload_masks_url_credentials() {
        let compose = "services:\n  app:\n    image: docker.io/library/nginx:1.27\n    \
                       command: curl https://deploy:orchid92@api.example.test/v1\n";
        let (text, findings) =
            redact_payload(compose, "/usr/local/app/docker-compose.yml").expect("redacted");
        assert!(!text.contains("orchid92"), "{text}");
        assert!(text.contains("api.example.test/v1"), "{text}");
        assert!(
            findings
                .iter()
                .any(|f| f.source == "proxy_credential" && f.line == Some(4)),
            "{findings:?}"
        );

        let plain = "services:\n  web:\n    image: docker.io/library/nginx:1.27\n    \
                     command: curl https://api.example.test/v1\n";
        assert_eq!(redact_payload(plain, "/usr/local/app/compose.yaml"), None);
    }

    /// Two URL credentials on one line, each also a pattern match, keep
    /// the text between them and each take their own token: the URL
    /// maskers' rewrites are separate edits, and each pattern finding is
    /// reported because its token is written.
    #[test]
    fn mixed_url_credentials_keep_separate_tokens() {
        let line = "mongodb://u:pw@db https://AKIA0123456789ABCDEF@host/\n";
        let (text, findings) = redact_payload(line, "/opt/mixed/app.conf").expect("redacted");
        assert_eq!(
            text,
            "mongodb://u:REDACTED_MONGODBPASSWORD_1@db https://REDACTED_AWSKEY_1@host/\n"
        );
        let mut patterns: Vec<_> = findings.iter().map(|f| f.pattern.as_str()).collect();
        patterns.sort_unstable();
        assert_eq!(patterns, ["awskey", "mongodbpassword"], "{findings:#?}");
    }

    /// Edits that overlap are applied as one, over all their bytes, and a
    /// pattern token stands for the merged span: no byte either edit
    /// replaces survives, and the masker's kept text does not come back.
    #[test]
    fn overlapping_payload_edits_replace_every_byte_once() {
        let content = "a=0123456789;b";
        let edit = |range: Range<usize>, text: &str, token: bool| PayloadEdit {
            range,
            text: text.into(),
            token,
            findings: Vec::new(),
        };
        assert_eq!(
            apply_payload_edits(
                content,
                vec![edit(2..9, "0[X]", false), edit(5..12, "TOKEN", true)]
            )
            .0,
            "a=TOKEN;b"
        );
        assert_eq!(
            apply_payload_edits(
                content,
                vec![edit(12..13, "", false), edit(2..4, "T", true)]
            )
            .0,
            "a=T23456789b"
        );
        assert_eq!(
            line_edit("caf\u{e9} x\n", "caf\u{e8} x\n"),
            (3..5, "\u{e8}".into())
        );
        assert_eq!(line_edit("same\n", "same\n"), (5..5, String::new()));
    }

    /// The check that a payload is what the snapshot's copy was read from
    /// runs `redact`'s own pass for that copy: it accepts the copy `redact`
    /// made, whatever numbers the snapshot gave its placeholders, and
    /// refuses text that changed.
    #[test]
    fn redacted_copy_check_runs_the_snapshot_pass() {
        use inspectah_core::types::containers::{ComposeFile, ContainerSection};
        let env = "proxy_password=-----BEGIN PRIVATE KEY-----\nMC4C AQAw BQYD K2Vw BCIE IBQh \
            0WfR 6aXE BHdg fX4i x5qD YmYR BZGZ EmNF dzJ1 Owlp\n-----END PRIVATE KEY-----\n\
            API_TOKEN=supersecretvalue123\n";
        let compose = "services:\n  db:\n    environment:\n      POSTGRES_PASSWORD: pgsecret99\n";
        let mut snap = InspectionSnapshot::new();
        snap.non_rpm_software = Some(NonRpmSoftwareSection {
            env_files: vec![
                // Earlier files take the low numbers.
                ConfigFileEntry {
                    path: "opt/other/.env".to_string(),
                    content: "API_TOKEN=anothersecret456\nPASSWORD=hunter2hunter2\n".into(),
                    ..Default::default()
                },
                ConfigFileEntry {
                    path: "opt/app/.env".to_string(),
                    content: env.into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        snap.containers = Some(ContainerSection {
            compose_files: vec![ComposeFile {
                path: "opt/app/docker-compose.yml".into(),
                raw_content: Some(compose.into()),
                ..Default::default()
            }],
            ..Default::default()
        });
        redact(&mut snap, &RedactOptions::default());
        let env_copy = &snap.non_rpm_software.as_ref().unwrap().env_files[1].content;
        assert!(!env_copy.contains("BEGIN"), "{env_copy}");
        assert!(is_redacted_copy_of(
            SnapshotText::EnvFile,
            "opt/app/.env",
            env_copy,
            env
        ));
        assert!(!is_redacted_copy_of(
            SnapshotText::EnvFile,
            "opt/app/.env",
            env_copy,
            &env.replace("API_TOKEN", "API_KEY")
        ));
        let compose_copy = snap.containers.as_ref().unwrap().compose_files[0]
            .raw_content
            .as_deref()
            .unwrap();
        assert!(is_redacted_copy_of(
            SnapshotText::ComposeFile,
            "opt/app/docker-compose.yml",
            compose_copy,
            compose
        ));
        assert!(!is_redacted_copy_of(
            SnapshotText::ComposeFile,
            "opt/app/docker-compose.yml",
            compose_copy,
            &format!("{compose}    ports: [\"80:80\"]\n")
        ));
    }

    /// The snapshot pass finds every edit in the original text: an `.env`
    /// URL password the patterns have no rule for is masked, a compose
    /// private key whose `BEGIN` marker the compose pass rewrites loses
    /// its body too, the twin check accepts both copies, and files with
    /// no credential come out byte-identical.
    #[test]
    fn snapshot_pass_matches_on_the_original_text() {
        let run = |kind, path: &str, text: &str| {
            let mut out = text.to_owned();
            let findings =
                redact_snapshot_text(kind, &mut out, path, &mut CounterRegistry::default());
            assert!(is_redacted_copy_of(kind, path, &out, text), "{out}");
            (out, findings)
        };

        let env = "SERVICE_URL=https://deploy:orchid92@api.example.test/v1
PORT=8080
";
        let (out, findings) = run(SnapshotText::EnvFile, "opt/app/.env", env);
        assert!(!out.contains("orchid92"), "{out}");
        assert!(out.contains("https://deploy:"), "{out}");
        assert!(out.ends_with("@api.example.test/v1\nPORT=8080\n"), "{out}");
        assert!(!findings.is_empty());

        let compose = "services:\n  app:\n    environment:\n      \
            PRIVATE_KEY: -----BEGIN PRIVATE KEY-----\n        MC4C AQAw BQYD K2Vw BCIE IBQh \
            0WfR 6aXE BHdg fX4i x5qD YmYR BZGZ EmNF dzJ1 Owlp\n        \
            -----END PRIVATE KEY-----\n      LOG_LEVEL: info\n";
        let (out, findings) = run(
            SnapshotText::ComposeFile,
            "opt/app/docker-compose.yml",
            compose,
        );
        let packed: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        for run in [
            "MC4CAQAwBQYDK2VwBCIEIBQh0WfR6aXE",
            "BHdgfX4ix5qDYmYRBZGZEmNFdzJ1Owlp",
        ] {
            assert!(!packed.contains(run), "{out}");
        }
        assert!(out.contains("PRIVATE_KEY: "), "{out}");
        assert!(out.ends_with("      LOG_LEVEL: info\n"), "{out}");
        assert!(!findings.is_empty());

        // The compose pass alone still reports, once.
        let (out, findings) = run(
            SnapshotText::ComposeFile,
            "opt/app/docker-compose.yml",
            "services:\n  app:\n    environment:\n      AUTH_HEADER: Basic\r\n",
        );
        assert_eq!(
            out,
            "services:\n  app:\n    environment:\n      AUTH_HEADER: <REDACTED>\r\n"
        );
        assert_eq!(
            findings,
            vec![compose_secret_finding("opt/app/docker-compose.yml")]
        );

        for (kind, path, text) in [
            (
                SnapshotText::EnvFile,
                "opt/app/.env",
                "PORT=8080\r\nLOG_LEVEL=info  \n",
            ),
            (
                SnapshotText::ComposeFile,
                "opt/app/docker-compose.yml",
                "services:\r\n  web:\n    image: nginx  \n    environment:\n      \
                 TZ: UTC  \n      PORT: 80",
            ),
        ] {
            let (out, findings) = run(kind, path, text);
            assert_eq!(out, text);
            assert!(findings.is_empty(), "{findings:?}");
        }
    }
}
