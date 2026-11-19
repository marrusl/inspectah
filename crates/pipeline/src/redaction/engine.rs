use inspectah_core::redaction::scrub_compose_secrets;
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionHint, RedactionKind,
    RedactionState,
};
use inspectah_core::types::sysctl::{SysctlFileBody, SysctlSection};
use regex::Regex;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;

use crate::redaction::patterns::{PATTERNS, SECRET_GROUP, SecretPattern, scan_shadow};
use crate::render::language_packages::scrub_manifest_auth;

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
static PROXY_CRED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(://[^:/@\s]+:)([^@\s]+)(@)").expect("proxy regex"));

/// Compiled regex for bare `proxy_password=VALUE` lines in DNF/Yum configs.
/// Captures two groups:
///   1. `proxy_password=` (the key prefix)
///   2. the password value
static PROXY_PASSWORD_KV_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(proxy_password\s*=\s*)(\S+)").expect("proxy_password key-value regex")
});

/// Compiled regex for username-only token remotes: `://token@host`.
/// Matches URLs where the username looks like a token (known prefixes or
/// long alphanumeric strings) with no colon/password separator.
/// Captures three groups:
///   1. `://` (scheme separator)
///   2. the token value (username portion)
///   3. `@` (delimiter before host)
static TOKEN_USERNAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(://)(?:(ghp_[A-Za-z0-9_]{36,}|gho_[A-Za-z0-9_]{36,}|glpat-[A-Za-z0-9_\-]{20,}|github_pat_[A-Za-z0-9_]{22,}|[A-Za-z0-9_\-]{20,}))(@)",
    )
    .expect("token username regex")
});

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

/// A match that survived all eligibility filters.
///
/// `start..end` and `text` are the secret only: the pattern's
/// [`SECRET_GROUP`] when it has one, otherwise the whole match. Every caller
/// replaces exactly that span, so a key, delimiter or URI around the secret
/// stays as written. `claim` is the whole match; it decides which of two
/// matches with overlapping secrets wins (see
/// [`dedup_overlapping_matches`]).
struct EligibleMatch {
    claim: Range<usize>,
    start: usize,
    end: usize,
    text: String,
    line_num: usize,
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

/// Returns true if `value` is a known NSS/PAM token, not a real secret.
fn is_false_positive_value(value: &str) -> bool {
    FALSE_POSITIVE_VALUES.contains(value.trim().to_lowercase().as_str())
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
/// the file at `path`. `#` always applies; `;` and `//` apply only to
/// file types that use them, because elsewhere a line starting with
/// them (`//srv/share/...`) is data and must still be scanned.
fn comment_markers(path: Option<&str>) -> Vec<&'static str> {
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

/// Byte offset of the start of the line holding `pos`.
fn line_start(content: &str, pos: usize) -> usize {
    content[..pos].rfind('\n').map_or(0, |i| i + 1)
}

/// The 1-based number of the line holding byte `pos`: one more than the
/// newlines before it. `str::lines` on the text before `pos` counts the
/// partial line too, which put every match not at column 0 one line low.
fn line_number(content: &str, pos: usize) -> usize {
    content[..pos].matches('\n').count() + 1
}

/// Returns true if a match whose secret starts at `secret_start` and whose
/// whole match starts at `claim_start` falls on a comment line: the text
/// of the secret's line before the match, trimmed, starts with one of
/// `markers` (see [`comment_markers`]).
///
/// The line is the secret's, never the line the match starts on. A
/// pattern can consume a boundary byte before its secret, and when that
/// byte is the newline ending a comment, reading the match's line took the
/// comment above for the secret's own line and exempted it. Context the
/// match consumed on the secret's own line is not part of the prefix, so a
/// marker the pattern took as its boundary (`#AKIA...`, `#jdbc:`) does not
/// make the line a comment.
fn is_comment_line(
    content: &str,
    claim_start: usize,
    secret_start: usize,
    markers: &[&str],
) -> bool {
    let start = line_start(content, secret_start);
    let prefix = content[start..claim_start.max(start)].trim_start();
    markers.iter().any(|m| prefix.starts_with(m))
}

/// Collect regex matches from `content` for a single `pat`, filtering out:
/// - Matches on comment lines (`#`, plus `;` / `//` where `path` uses them)
/// - Password-pattern matches whose value is a known NSS/PAM token
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
fn collect_eligible_matches(
    pat: &SecretPattern,
    content: &str,
    path: Option<&str>,
) -> Vec<EligibleMatch> {
    let markers = comment_markers(path);
    pat.regex
        .captures_iter(content)
        .filter_map(|caps| {
            let whole = caps.get(0)?;
            let secret = caps.name(SECRET_GROUP).unwrap_or(whole);
            let text = secret.as_str();

            // Filter 0: the pattern's own condition on the match
            if let Some(accept) = pat.accept
                && !accept(content, &caps)
            {
                return None;
            }

            // Filter 1: skip matches on comment lines
            if is_comment_line(content, whole.start(), secret.start(), &markers) {
                return None;
            }

            // No filter skips a value spelled like a redaction token. The
            // spelling is not provenance: `password=REDACTED_COMPANY_2026`
            // can be the host's real password. `redact()` runs once per
            // snapshot; a second pass over its output reports most tokens
            // again and replaces each with a token, usually itself, and can
            // change the text around one (see
            // `test_a_second_pass_is_not_guaranteed_to_return_the_same_text`).

            // Filter 2: skip Password matches whose value is a known
            // NSS/PAM false-positive token
            if pat.finding_kind == FindingKind::Password && is_false_positive_value(text) {
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

            let line_num = line_number(content, secret.start());

            Some(EligibleMatch {
                claim: whole.range(),
                start: secret.start(),
                end: secret.end(),
                text: text.to_string(),
                line_num,
            })
        })
        .collect()
}

/// Resolve overlapping matches from different patterns so no two replaced
/// spans overlap, without losing any secret byte. Input must be the combined
/// matches from ALL patterns for a single content blob, and `content` must be
/// that blob: a merged span's text is read from it.
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
/// start to the largest end, and its text is taken from `content` rather
/// than stitched from the pieces. Stitching assumed each pair overlapped,
/// and a bridge merged after the spans it joined broke that and panicked.
fn dedup_overlapping_matches(content: &str, matches: &mut Vec<(usize, EligibleMatch)>) {
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
        kept.text = content[start..end].to_string();
    }
    matches.sort_by_key(|(_, em)| em.start);
}

/// Build the replacement for a matched secret: the token for its text.
/// It replaces `em.start..em.end`, which is the secret alone (see
/// [`EligibleMatch`]), so the key or URI around it stays as written.
fn build_replacement(
    em: &EligibleMatch,
    registry: &mut CounterRegistry,
    kind_label: &str,
) -> String {
    registry.token_for(kind_label, &em.text)
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
    dedup_overlapping_matches(&result, &mut all_matches);
    if all_matches.is_empty() {
        // Regex matched but all hits were filtered (comment lines, false
        // positives, etc.) — no actual redaction needed.
        return Cow::Borrowed(content);
    }
    // Sort descending by offset so replacements don't invalidate earlier offsets
    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
    for (pat_idx, em) in all_matches {
        let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
        let replacement = build_replacement(&em, &mut registry, &kind_label);
        result.replace_range(em.start..em.end, &replacement);
    }

    Cow::Owned(result)
}

/// The path of the kernel command line, where a high-confidence hint is
/// raised for each sensitive parameter.
const KERNEL_CMDLINE_PATH: &str = "/proc/cmdline";

/// Where a hinted value ends, which is what must have been replaced for the
/// hint to count as handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintValueEnd {
    /// At the next whitespace: the kernel command line, whose parameters
    /// are separated by whitespace.
    Whitespace,
    /// At the end of the line, or at a `#` comment after whitespace: a
    /// `.env` file, where the value runs to the end of the line.
    EndOfLine,
}

impl HintValueEnd {
    /// The end that applies to content collected at `path`. Only the
    /// kernel command line is known to end a value at whitespace; anywhere
    /// else the conservative end is the end of the line.
    fn for_path(path: &str) -> Self {
        if path == KERNEL_CMDLINE_PATH {
            Self::Whitespace
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
/// handed out) up to the next whitespace, and, under
/// [`HintValueEnd::EndOfLine`], nothing but whitespace or a `#` comment
/// follows it on the line. A value that only starts with a token
/// (`REDACTED_PASSWORD_1/hunter2`, `REDACTED_PASSWORD_1 still-secret` in a
/// `.env` file) or is only spelled like one is the host's, and so is
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
        let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if !issued.contains(&rest[..token_end]) {
            return true;
        }
        match value_end {
            HintValueEnd::Whitespace => false,
            HintValueEnd::EndOfLine => {
                let after = &rest[token_end..];
                let line_rest = after[..after.find('\n').unwrap_or(after.len())].trim_start();
                !(line_rest.is_empty() || line_rest.starts_with('#'))
            }
        }
    })
}

/// Mask embedded credentials in a proxy URL line.
///
/// Replaces only the password segment in `://user:password@` patterns,
/// preserving the rest of the URL structure. This is a dedicated function
/// because `PATTERNS` has no rule for a password in an arbitrary URL's
/// authority, only for PostgreSQL, MongoDB, Redis and JDBC URLs, so
/// `redact_string()`, which also replaces only the secret, would not find
/// this one.
///
/// Returns `(Cow<str>, Option<RedactionFinding>)` — borrowed if no match,
/// owned with a finding if credentials were masked.
pub fn mask_proxy_credentials<'a>(
    line: &'a str,
    source_path: &str,
) -> (Cow<'a, str>, Option<RedactionFinding>) {
    // First: check for URL-shaped credentials (://user:password@host)
    if PROXY_CRED_RE.is_match(line) {
        let masked = PROXY_CRED_RE
            .replace(line, "${1}[REDACTED]${3}")
            .into_owned();
        let finding = RedactionFinding {
            path: source_path.to_string(),
            source: "proxy_credential".into(),
            kind: RedactionKind::Inline,
            pattern: "proxy_password".into(),
            remediation: "Remove embedded credentials from proxy URL; use environment-specific auth configuration".to_string(),
            line: None,
            replacement: None,
            detection_method: DetectionMethod::Pattern,
            confidence: Some(Confidence::High),
            finding_kind: Some(FindingKind::Password),
        };
        return (Cow::Owned(masked), Some(finding));
    }

    // Second: check for bare proxy_password=VALUE lines (DNF/Yum config)
    if PROXY_PASSWORD_KV_RE.is_match(line) {
        let masked = PROXY_PASSWORD_KV_RE
            .replace(line, "${1}[REDACTED]")
            .into_owned();
        let finding = RedactionFinding {
            path: source_path.to_string(),
            source: "proxy_credential".into(),
            kind: RedactionKind::Inline,
            pattern: "proxy_password".into(),
            remediation: "Remove plaintext proxy_password from DNF/Yum configuration; use repository-level auth instead".to_string(),
            line: None,
            replacement: None,
            detection_method: DetectionMethod::Pattern,
            confidence: Some(Confidence::High),
            finding_kind: Some(FindingKind::Password),
        };
        return (Cow::Owned(masked), Some(finding));
    }

    (Cow::Borrowed(line), None)
}

/// Mask username-only token credentials in URLs.
///
/// Catches `://ghp_xxx@github.com`, `://glpat-xxx@gitlab.com`, and generic
/// long alphanumeric usernames (>= 20 chars) used as tokens without a
/// colon/password separator. These slip past `mask_proxy_credentials()`
/// which requires the `://user:password@` shape.
///
/// Returns `(Cow<str>, Option<RedactionFinding>)` — borrowed if no match.
pub fn mask_token_username<'a>(
    line: &'a str,
    source_path: &str,
) -> (Cow<'a, str>, Option<RedactionFinding>) {
    if TOKEN_USERNAME_RE.is_match(line) {
        let masked = TOKEN_USERNAME_RE
            .replace(line, "${1}[REDACTED]${3}")
            .into_owned();
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
        return (Cow::Owned(masked), Some(finding));
    }

    (Cow::Borrowed(line), None)
}

/// Scan content for secrets. Returns findings for all detected patterns.
pub fn scan_content(content: &str, path: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();

    // Shadow-specific logic for /etc/shadow paths
    if path.ends_with("/shadow") || path == "/etc/shadow" {
        findings.extend(scan_shadow(content, path));
    }

    // Generic pattern matching — collect from all patterns, then dedup overlaps
    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
    for (idx, pat) in PATTERNS.iter().enumerate() {
        for em in collect_eligible_matches(pat, content, Some(path)) {
            all_matches.push((idx, em));
        }
    }
    dedup_overlapping_matches(content, &mut all_matches);
    for (pat_idx, em) in all_matches {
        let pat = &PATTERNS[pat_idx];
        findings.push(RedactionFinding {
            path: path.to_string(),
            source: "pattern".into(),
            kind: RedactionKind::Inline,
            pattern: format!("{:?}", pat.finding_kind).to_lowercase(),
            remediation: pat.remediation.to_string(),
            line: Some(em.line_num as i32),
            replacement: None,
            detection_method: pat.detection_method.clone(),
            confidence: Some(pat.confidence),
            finding_kind: Some(pat.finding_kind.clone()),
        });
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

/// Run the generic pattern pass over one content-bearing field, in place.
///
/// This is the block `redact()` repeats for every content field it visits:
/// scan for findings, and when there are any, collect every pattern's
/// matches, deduplicate overlapping spans, then replace in descending
/// offset order so an earlier replacement cannot invalidate a later
/// offset. Returns the findings, and leaves `content` untouched when
/// there are none.
///
/// The older call sites above still inline this block; three of them
/// call `registry.token_for` directly instead of `build_replacement`,
/// which is equivalent now that a match's span is the secret alone.
fn scan_and_redact_field(
    content: &mut String,
    source_path: &str,
    registry: &mut CounterRegistry,
) -> Vec<RedactionFinding> {
    let findings = scan_content(content, source_path);
    if findings.is_empty() {
        return findings;
    }
    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
    for (idx, pat) in PATTERNS.iter().enumerate() {
        for em in collect_eligible_matches(pat, content, Some(source_path)) {
            all_matches.push((idx, em));
        }
    }
    dedup_overlapping_matches(content, &mut all_matches);
    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
    for (pat_idx, em) in all_matches {
        let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
        let replacement = build_replacement(&em, registry, &kind_label);
        content.replace_range(em.start..em.end, &replacement);
    }
    findings
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
            SysctlFileBody::Mask => {}
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
                *redacted = *content != before;
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

    // Scan config file contents
    if let Some(ref mut config) = snapshot.config {
        for file in &mut config.files {
            let findings = scan_content(&file.content, &file.path);
            if !findings.is_empty() {
                let mut content = file.content.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&file.path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                file.content = content;
                all_findings.extend(findings);
            }
        }
    }

    // Scan repo file contents for embedded credentials
    if let Some(ref mut rpm) = snapshot.rpm {
        for repo_file in &mut rpm.repo_files {
            let findings = scan_content(&repo_file.content, &repo_file.path);
            if !findings.is_empty() {
                let mut content = repo_file.content.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&repo_file.path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                repo_file.content = content;
                all_findings.extend(findings);
            }
        }

        // Scan GPG key contents
        for gpg_key in &mut rpm.gpg_keys {
            let findings = scan_content(&gpg_key.content, &gpg_key.path);
            if !findings.is_empty() {
                let mut content = gpg_key.content.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&gpg_key.path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                gpg_key.content = content;
                all_findings.extend(findings);
            }
        }
    }

    // Scan systemd drop-in contents in services section. The units
    // section's other rows hold no file content: a mount finding has no
    // copy of `What=`, which can hold a credential (the unit file's config
    // entry carries it, scrubbed by the config pass), and its
    // `mount_point` is a path, classified as an identifier.
    if let Some(ref mut services) = snapshot.units {
        for drop_in in &mut services.drop_ins {
            let findings = scan_content(&drop_in.content, &drop_in.path);
            if !findings.is_empty() {
                let mut content = drop_in.content.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&drop_in.path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                drop_in.content = content;
                all_findings.extend(findings);
            }
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
        let cmdline_findings = scan_content(&kernelboot.cmdline, "/proc/cmdline");
        if !cmdline_findings.is_empty() {
            let mut content = kernelboot.cmdline.clone();
            let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
            for (idx, pat) in PATTERNS.iter().enumerate() {
                for em in collect_eligible_matches(pat, &content, Some("/proc/cmdline")) {
                    all_matches.push((idx, em));
                }
            }
            dedup_overlapping_matches(&content, &mut all_matches);
            all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
            for (pat_idx, em) in all_matches {
                let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                let token = registry.token_for(&kind_label, &em.text);
                content.replace_range(em.start..em.end, &token);
            }
            kernelboot.cmdline = content;
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
            let (masked, finding) = mask_proxy_credentials(&entry.line, &entry.source);
            if let Some(f) = finding {
                entry.line = masked.into_owned();
                all_findings.push(f);
            }
        }
    }

    // Scan container env vars for secrets via generic pattern pass.
    if let Some(ref mut containers) = snapshot.containers {
        for container in &mut containers.running_containers {
            for env_entry in &mut container.env {
                let redacted = redact_string(env_entry);
                if let Cow::Owned(ref new_val) = redacted {
                    // Collect findings for each pattern match.
                    let findings =
                        scan_content(env_entry, &format!("container:{}", container.name));
                    all_findings.extend(findings);
                    *env_entry = new_val.clone();
                }
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

        // Compose file content. Two passes, because neither alone covers
        // the other's cases:
        //
        // 1. `scrub_compose_secrets` keys on its own name list and
        //    rewrites credentials in a URL of any scheme, which the
        //    generic patterns do not (they cover postgres, mongodb,
        //    redis and jdbc only). It emits no finding of its own.
        // 2. The generic pass detects and reports, and catches the case
        //    that defeats pass 1 entirely: a connection string whose key
        //    name carries no secret-like word, which also means the
        //    collector's hint never fires for it either.
        //
        // Pass 1 runs first so the generic pass sees its output and can
        // still report on a line it rewrote. Assigning only when the
        // marker count rises keeps a secret-free compose file
        // byte-identical, because `scrub_compose_secrets` normalizes
        // trailing whitespace and appends a final newline regardless.
        for compose in &mut containers.compose_files {
            let Some(raw) = compose.raw_content.as_mut() else {
                continue;
            };
            let scrubbed = scrub_compose_secrets(raw);
            let compose_pass_fired = scrubbed.matches(COMPOSE_REDACTED_MARKER).count()
                > raw.matches(COMPOSE_REDACTED_MARKER).count();
            if compose_pass_fired {
                *raw = scrubbed;
            }

            let generic_findings = scan_and_redact_field(raw, &compose.path, &mut registry);
            let generic_pass_fired = !generic_findings.is_empty();
            all_findings.extend(generic_findings);

            // Only when pass 1 found something pass 2 did not report, so
            // one secret does not produce two findings. A file with both
            // kinds is reported once rather than twice; it is never
            // reported as clean.
            if compose_pass_fired && !generic_pass_fired {
                all_findings.push(RedactionFinding {
                    path: compose.path.clone(),
                    source: "compose_env".into(),
                    kind: RedactionKind::Inline,
                    pattern: "compose_secret_value".into(),
                    remediation:
                        "Move the secret out of the compose file: use a compose secret, an \
                         env_file outside the image, or a systemd credential"
                            .to_string(),
                    line: None,
                    replacement: None,
                    detection_method: DetectionMethod::Pattern,
                    confidence: Some(Confidence::High),
                    finding_kind: Some(FindingKind::Password),
                });
            }
        }
    }

    // Scan scheduled task commands and service-unit content blobs for secrets.
    if let Some(ref mut sched) = snapshot.scheduled_tasks {
        // GeneratedTimerUnit.command fields (cron commands converted to timers)
        for unit in &mut sched.generated_timer_units {
            let findings = scan_content(&unit.command, &unit.source_path);
            if !findings.is_empty() {
                let mut content = unit.command.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&unit.source_path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                unit.command = content;
                all_findings.extend(findings);
            }

            // GeneratedTimerUnit.service_content — raw unit file text that
            // may contain secrets in ExecStart= or Environment= lines.
            if !unit.service_content.is_empty() {
                let svc_path = format!("generated:{}.service", unit.name);
                let findings = scan_content(&unit.service_content, &svc_path);
                if !findings.is_empty() {
                    let mut content = unit.service_content.clone();
                    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                    for (idx, pat) in PATTERNS.iter().enumerate() {
                        for em in collect_eligible_matches(pat, &content, Some(&svc_path)) {
                            all_matches.push((idx, em));
                        }
                    }
                    dedup_overlapping_matches(&content, &mut all_matches);
                    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                    for (pat_idx, em) in all_matches {
                        let kind_label =
                            format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                        let token = registry.token_for(&kind_label, &em.text);
                        content.replace_range(em.start..em.end, &token);
                    }
                    unit.service_content = content;
                    all_findings.extend(findings);
                }
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
            let findings = scan_content(&at_job.command, &at_job.file);
            if !findings.is_empty() {
                let mut content = at_job.command.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&at_job.file)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                at_job.command = content;
                all_findings.extend(findings);
            }
        }

        // SystemdTimer.exec_start fields
        for timer in &mut sched.systemd_timers {
            let source_path = if timer.path.is_empty() {
                format!("timer:{}", timer.name)
            } else {
                timer.path.clone()
            };
            let findings = scan_content(&timer.exec_start, &source_path);
            if !findings.is_empty() {
                let mut content = timer.exec_start.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&source_path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                timer.exec_start = content;
                all_findings.extend(findings);
            }

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
            let findings = scan_content(&env_file.content, &env_file.path);
            if !findings.is_empty() {
                let mut content = env_file.content.clone();
                let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                for (idx, pat) in PATTERNS.iter().enumerate() {
                    for em in collect_eligible_matches(pat, &content, Some(&env_file.path)) {
                        all_matches.push((idx, em));
                    }
                }
                dedup_overlapping_matches(&content, &mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement = build_replacement(&em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                env_file.content = content;
                all_findings.extend(findings);
            }
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
                let (masked, finding) = mask_proxy_credentials(&item.git_remote, &item.path);
                if let Some(f) = finding {
                    item.git_remote = masked.into_owned();
                    all_findings.push(f);
                }
                // Username-only token remotes: ://ghp_xxx@host, ://glpat-xxx@host
                let (masked, finding) = mask_token_username(&item.git_remote, &item.path);
                if let Some(f) = finding {
                    item.git_remote = masked.into_owned();
                    all_findings.push(f);
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
                    dedup_overlapping_matches(&content, &mut all_matches);
                    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                    for (pat_idx, em) in all_matches {
                        let kind_label =
                            format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                        let token = registry.token_for(&kind_label, &em.text);
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

    // Determine redaction state
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
        // the next whitespace)
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
            // Spelled like a token, but this run never issued it.
            ("DB_KEY=REDACTED_PASSWORD_3", "DB_KEY", true, true),
            ("DB_KEY=REDACTED_COMPANY_2026", "DB_KEY", true, true),
        ];
        for (content, kw, to_line_end, to_whitespace) in cases {
            for (end, survives) in [
                (HintValueEnd::EndOfLine, to_line_end),
                (HintValueEnd::Whitespace, to_whitespace),
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
            HintValueEnd::Whitespace
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
            text: content[secret].to_string(),
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
        dedup_overlapping_matches(content, &mut matches);
        let got: Vec<(usize, Range<usize>, &str)> = matches
            .iter()
            .map(|(p, em)| (*p, em.start..em.end, em.text.as_str()))
            .collect();
        assert_eq!(got, vec![(0, 7..18, "789abcdefgh"), (3, 19..20, "j")]);
    }

    /// A candidate can overlap two kept secrets that do not overlap each
    /// other. All of them become one span, whichever side the winner is on
    /// and however long the chain of bridges is, and the span's text is
    /// the content under it.
    #[test]
    fn test_a_bridge_between_kept_secrets_merges_all_of_them() {
        let content = "0123456789abcdefghijklmnopqrstuvwxyz";
        let em = |claim: Range<usize>, secret: Range<usize>| EligibleMatch {
            claim,
            start: secret.start,
            end: secret.end,
            text: content[secret].to_string(),
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
            dedup_overlapping_matches(content, &mut matches);
            let got: Vec<(usize, Range<usize>)> = matches
                .iter()
                .map(|(p, em)| (*p, em.start..em.end))
                .collect();
            assert_eq!(got, want, "{name}");
            for (_, em) in &matches {
                assert_eq!(em.text, &content[em.start..em.end], "{name}");
            }
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
            dedup_overlapping_matches(&input, &mut matches);
            for pair in matches.windows(2) {
                proptest::prop_assert!(pair[0].1.end <= pair[1].1.start, "{input:?}");
            }
            for (_, em) in &matches {
                proptest::prop_assert_eq!(&em.text, &input[em.start..em.end]);
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
    /// consumes, so its match starts at the line's start, like `#jdbc:`.
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

    // --- Comment-line filtering tests (Gap 1) ---

    #[test]
    fn test_comment_hash_not_redacted_via_redact() {
        // Hash-commented line must be preserved through the full redact() path.
        let mut snapshot = InspectionSnapshot::new();
        snapshot.config = Some(ConfigSection {
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
        snapshot.config = Some(ConfigSection {
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

    /// A JDBC URL inside quotes or XML element text is replaced up to its
    /// closing quote or tag, which stays, so the file around it still
    /// parses. An unquoted URL is replaced up to the next whitespace, so a
    /// following assignment stays too.
    #[test]
    fn test_a_jdbc_url_keeps_the_quotes_and_tags_around_it() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "<url>jdbc:mysql://u:zsecret@db/app</url>",
                "<url>jdbc:mysql://REDACTED_JDBCPASSWORD_1</url>",
                &["zsecret"],
            ),
            (
                r#"<db url="jdbc:mysql://u:zsecret@db/app" pool="4"/>"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool="4"/>"#,
                &["zsecret"],
            ),
            (
                r#"url="jdbc:mysql://u:zsecret@db/app" pool=4"#,
                r#"url="jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool=4"#,
                &["zsecret"],
            ),
            (
                "url='jdbc:mysql://u:zsecret@db/app' pool=4",
                "url='jdbc:mysql://REDACTED_JDBCPASSWORD_1' pool=4",
                &["zsecret"],
            ),
            (
                "url=jdbc:mysql://u:zsecret@db/app pool=4",
                "url=jdbc:mysql://REDACTED_JDBCPASSWORD_1 pool=4",
                &["zsecret"],
            ),
            // Inside quotes or element text a space does not end the URL.
            (
                r#"url="jdbc:mysql://app:ztwo zsecret@db" pool=4"#,
                r#"url="jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool=4"#,
                &["ztwo", "zsecret"],
            ),
            (
                "<url>jdbc:mysql://db/app?password=ztwo zsecret</url>",
                "<url>jdbc:mysql://REDACTED_JDBCPASSWORD_1</url>",
                &["ztwo", "zsecret"],
            ),
            // A quoted URL with no credential is left alone, and the `@` in
            // a later attribute does not pull it in.
            (
                r#"<db url="jdbc:mysql://db/app" owner="ops@example.com"/>"#,
                r#"<db url="jdbc:mysql://db/app" owner="ops@example.com"/>"#,
                &[],
            ),
            // The cost of keeping a whitespace-holding authority password
            // covered: an unquoted URL reaches the line's last marker, so a
            // later `password=` takes the assignment between with it.
            (
                "url=jdbc:mysql://u:zsecret@db/app pool=4 password=x tail=5",
                "url=jdbc:mysql://REDACTED_JDBCPASSWORD_1 tail=5",
                &["zsecret"],
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

    /// A credential marker inside a quoted JDBC URL counts wherever it
    /// sits, escaped or not. A backslash is no escape in an XML attribute
    /// or a single-quoted scalar, and `\@` there is a backslash and an
    /// `@`; treating `\@` as one escaped unit left no bare `@` to find,
    /// and the whole credential shipped.
    #[test]
    fn test_an_escaped_marker_in_a_quoted_jdbc_url_is_still_a_credential() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                r#"<db url="jdbc:mysql://u:zsecret\@db/app"/>"#,
                r#"<db url="jdbc:mysql://REDACTED_JDBCPASSWORD_1"/>"#,
                &["zsecret"],
            ),
            (
                r#"url="jdbc:mysql://db/app?pass\word=zsecret" pool=4"#,
                r#"url="jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool=4"#,
                &["zsecret"],
            ),
            (
                r"url='jdbc:mysql://u:zsecret\@db/app' pool=4",
                r"url='jdbc:mysql://REDACTED_JDBCPASSWORD_1' pool=4",
                &["zsecret"],
            ),
            // Unterminated: the value runs to the end of the line.
            (
                r#"url="jdbc:mysql://u:zsecret\@db/app"#,
                r#"url="jdbc:mysql://REDACTED_JDBCPASSWORD_1"#,
                &["zsecret"],
            ),
            (
                r"url='jdbc:mysql://u:zsecret\@db/app",
                r"url='jdbc:mysql://REDACTED_JDBCPASSWORD_1",
                &["zsecret"],
            ),
            // An escaped quote is still not the end of a JSON string.
            (
                r#"{"url": "jdbc:mysql://u:p\"zsecret\@db/app", "pool": 4}"#,
                r#"{"url": "jdbc:mysql://REDACTED_JDBCPASSWORD_1", "pool": 4}"#,
                &["zsecret"],
            ),
            // No marker inside the quotes, escaped or not: left alone.
            (
                r#"<db url="jdbc:mysql://db/a\pp" owner="ops@example.com"/>"#,
                r#"<db url="jdbc:mysql://db/a\pp" owner="ops@example.com"/>"#,
                &[],
            ),
        ];
        for (input, want, secrets) in cases {
            let once = redact_string(input).into_owned();
            assert_eq!(&once, want, "{input:?}");

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
                "{input:?}: {findings:?}"
            );
        }
    }

    /// A `>` inside a quoted attribute does not open element text, so a
    /// JDBC URL right after one ends at the attribute's closing quote, and
    /// the attributes and markup after it stay. A `>` that closes a tag
    /// still opens element text.
    #[test]
    fn test_a_jdbc_url_after_a_gt_inside_an_attribute_keeps_the_tag() {
        let cases: &[(&str, &str)] = &[
            (
                r#"<db url=">jdbc:mysql://u:zsecret@db/app" pool="4"/>"#,
                r#"<db url=">jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool="4"/>"#,
            ),
            (
                "<db url='>jdbc:mysql://u:zsecret@db/app' pool='4'/>",
                "<db url='>jdbc:mysql://REDACTED_JDBCPASSWORD_1' pool='4'/>",
            ),
            (
                r#"<db name="a>b" url=">jdbc:mysql://u:zsecret@db/app" pool="4"/>"#,
                r#"<db name="a>b" url=">jdbc:mysql://REDACTED_JDBCPASSWORD_1" pool="4"/>"#,
            ),
            // A `>` that closes a tag, even after a quoted `>`, is text.
            (
                r#"<url kind="a>b">jdbc:mysql://u:zsecret@db/app</url>"#,
                r#"<url kind="a>b">jdbc:mysql://REDACTED_JDBCPASSWORD_1</url>"#,
            ),
            (
                "<url>jdbc:mysql://u:zsecret@db/app</url>",
                "<url>jdbc:mysql://REDACTED_JDBCPASSWORD_1</url>",
            ),
        ];
        for (input, want) in cases {
            let once = redact_string(input).into_owned();
            assert_eq!(&once, want, "{input:?}");

            let mut snap = config_snapshot("/etc/app/context.xml", input);
            redact(&mut snap, &RedactOptions::default());
            assert_eq!(&snap.config.as_ref().unwrap().files[0].content, want);
            let written = serde_json::to_string(&snap).unwrap();
            assert!(!written.contains("zsecret"), "{input:?}");
            let findings = scan_content(input, "/etc/app/context.xml");
            assert_eq!(findings.len(), 1, "{input:?}: {findings:?}");
            // Every quote that opened an attribute still closes it.
            assert_eq!(once.matches('"').count() % 2, 0, "{once:?}");
            assert_eq!(once.matches('\'').count() % 2, 0, "{once:?}");
            assert!(once.ends_with("/>") || once.ends_with("</url>"), "{once:?}");
        }
    }

    /// The redacted text of a quoted JDBC URL still parses as the format
    /// it came in, with the token as the value and its neighbours intact.
    #[test]
    fn test_a_redacted_quoted_jdbc_url_still_parses() {
        const TOKEN_URL: &str = "jdbc:mysql://REDACTED_JDBCPASSWORD_1";

        let toml_in = "[db]\nurl = \"jdbc:mysql://u:zsecret@db/app\"\n\
                       literal = 'jdbc:mysql://u:zsecret@db/app'\npool = 4\n";
        let toml_out: toml::Value = toml::from_str(&redact_string(toml_in)).unwrap();
        assert_eq!(toml_out["db"]["url"].as_str(), Some(TOKEN_URL));
        assert_eq!(toml_out["db"]["literal"].as_str(), Some(TOKEN_URL));
        assert_eq!(toml_out["db"]["pool"].as_integer(), Some(4));

        let json_in = r#"{"url": "jdbc:mysql://u:p\"zsecret@db/app", "pool": 4}"#;
        let json_out: serde_json::Value = serde_json::from_str(&redact_string(json_in)).unwrap();
        assert_eq!(json_out["url"].as_str(), Some(TOKEN_URL));
        assert_eq!(json_out["pool"].as_i64(), Some(4));
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
            // The generic `\S+` value stops at the space, so ` words` is
            // left; the hint says so.
            ("DB_PASSWORD", "DB_PASSWORD=two words\n", false),
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
}
