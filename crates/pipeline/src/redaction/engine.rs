use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::kernelboot::ConfigSnippet;
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionHint, RedactionKind,
    RedactionState,
};
use regex::Regex;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::redaction::patterns::{PATTERNS, SecretPattern, scan_shadow};

/// Paths that are known false-positive sources for credential scanning.
/// Files under these prefixes use keywords like "password", "auth", and
/// "credential" as PAM module type tokens — not actual secrets.
/// Listed WITHOUT a leading slash — the check handles both absolute
/// (`/etc/pam.d/...`) and relative (`etc/pam.d/...`) forms.
const REDACTION_ALLOWLIST: &[&str] = &["etc/pam.d/"];

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
            format!("REDACTED_{}_{}", kind_label.to_uppercase(), n)
        }
    }
}

/// A match that survived all eligibility filters.
/// Contains byte offsets and the matched text, ready for replacement or finding generation.
struct EligibleMatch {
    start: usize,
    end: usize,
    text: String,
    line_num: usize,
    /// Captured groups for patterns with `replacement_template`.
    /// Groups 1 and 3 are preserved, group 2 is the secret to redact.
    captures: Option<(String, String, String)>,
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

/// Returns true if the match at `pos` falls on a comment line.
/// A comment line is one whose trimmed content (before `pos`) starts
/// with `#`, `//`, or `;`.
fn is_comment_line(content: &str, pos: usize) -> bool {
    let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
    let prefix = content[line_start..pos].trim_start();
    prefix.starts_with('#') || prefix.starts_with("//") || prefix.starts_with(';')
}

/// Collect regex matches from `content` for a single `pat`, filtering out:
/// - Matches on comment lines (# // ;)
/// - (Gap 4, future) Password-pattern matches whose value is a known NSS/PAM token
/// - (Gap 2, future) PasswordHash matches when `path` is a shadow file
///
/// This is the ONLY function that should call `pat.regex.find_iter()`.
/// Callers must invoke this for EVERY pattern, merge the results into a
/// single `Vec<(usize, EligibleMatch)>` (tagged with the pattern index),
/// then run `dedup_overlapping_matches` on the merged list before
/// processing matches.
fn collect_eligible_matches(
    pat: &SecretPattern,
    content: &str,
    path: Option<&str>,
) -> Vec<EligibleMatch> {
    // When the pattern has a replacement template, use captures_iter to
    // extract the capture groups alongside the match offsets.
    if pat.replacement_template.is_some() {
        return pat
            .regex
            .captures_iter(content)
            .filter_map(|caps| {
                let mat = caps.get(0)?;
                let start = mat.start();
                let end = mat.end();
                let text = mat.as_str();

                if is_comment_line(content, start) {
                    return None;
                }

                let line_num = content[..start].lines().count() + 1;

                let captures = Some((
                    caps.get(1).map_or("", |m| m.as_str()).to_string(),
                    caps.get(2).map_or("", |m| m.as_str()).to_string(),
                    caps.get(3).map_or("", |m| m.as_str()).to_string(),
                ));

                Some(EligibleMatch {
                    start,
                    end,
                    text: text.to_string(),
                    line_num,
                    captures,
                })
            })
            .collect();
    }

    pat.regex
        .find_iter(content)
        .filter_map(|mat| {
            let start = mat.start();
            let end = mat.end();
            let text = mat.as_str();

            // Filter 1: skip matches on comment lines
            if is_comment_line(content, start) {
                return None;
            }

            // Filter 2: skip Password matches whose value is a known
            // NSS/PAM false-positive token
            if pat.finding_kind == FindingKind::Password
                && let Some(sep_pos) = text.find('=').or_else(|| text.find(':'))
            {
                let value = &text[sep_pos + 1..];
                if is_false_positive_value(value) {
                    return None;
                }
            }

            // Filter 3: skip PasswordHash matches when path is a shadow
            // file — scan_shadow owns those with richer classification
            if pat.finding_kind == FindingKind::PasswordHash
                && let Some(p) = path
                && (p.ends_with("/shadow") || p.ends_with("/shadow-"))
            {
                return None;
            }

            let line_num = content[..start].lines().count() + 1;

            Some(EligibleMatch {
                start,
                end,
                text: text.to_string(),
                line_num,
                captures: None,
            })
        })
        .collect()
}

/// Remove matches that are entirely contained within a longer match.
/// Input must be the combined matches from ALL patterns for a single content blob.
/// Preserves the longer match when spans overlap. Ties broken by input order
/// (earlier pattern in PATTERNS vec wins).
fn dedup_overlapping_matches(matches: &mut Vec<(usize, EligibleMatch)>) {
    // Sort by start ascending, then by span length descending (longer first)
    matches.sort_by(|a, b| {
        a.1.start
            .cmp(&b.1.start)
            .then_with(|| (b.1.end - b.1.start).cmp(&(a.1.end - a.1.start)))
    });
    let mut keep = vec![true; matches.len()];
    for i in 0..matches.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..matches.len() {
            if !keep[j] {
                continue;
            }
            // If j is entirely within i's span, drop j
            if matches[j].1.start >= matches[i].1.start && matches[j].1.end <= matches[i].1.end {
                keep[j] = false;
            }
            // Partial overlap: j starts inside i but extends past i's end.
            // Keep the longer match, drop the shorter one.
            else if matches[j].1.start < matches[i].1.end && matches[j].1.end > matches[i].1.end {
                let len_i = matches[i].1.end - matches[i].1.start;
                let len_j = matches[j].1.end - matches[j].1.start;
                if len_i >= len_j {
                    keep[j] = false;
                } else {
                    keep[i] = false;
                    break; // i is dropped, stop comparing against it
                }
            }
            // If j starts beyond i's end, no more overlaps possible
            if matches[j].1.start >= matches[i].1.end {
                break;
            }
        }
    }
    let mut idx = 0;
    matches.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });

    // Safety net: assert no partial overlaps survive.
    // Current patterns only produce subset overlaps, but a future pattern
    // addition could introduce a partial overlap that corrupts content
    // during descending-offset replacement. Catch it at test time.
    #[cfg(debug_assertions)]
    {
        for i in 0..matches.len() {
            for j in (i + 1)..matches.len() {
                debug_assert!(
                    matches[j].1.start >= matches[i].1.end
                        || matches[i].1.start >= matches[j].1.end,
                    "partial overlap detected between patterns {} and {} \
                     (spans {}..{} and {}..{})",
                    matches[i].0,
                    matches[j].0,
                    matches[i].1.start,
                    matches[i].1.end,
                    matches[j].1.start,
                    matches[j].1.end,
                );
            }
        }
    }
}

/// Build the replacement string for a matched secret.
///
/// For patterns with `replacement_template` (connection-string URIs), preserves
/// URL structure by replacing only the password portion (capture group 2) with
/// the redaction token while keeping the scheme prefix and `@` delimiter.
///
/// For patterns without a template, replaces the entire match with the token.
fn build_replacement(
    pat: &SecretPattern,
    em: &EligibleMatch,
    registry: &mut CounterRegistry,
    kind_label: &str,
) -> String {
    if pat.replacement_template.is_some()
        && let Some((ref prefix, ref secret, ref suffix)) = em.captures
    {
        let token = registry.token_for(kind_label, secret);
        return format!("{prefix}{token}{suffix}");
    }
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
        let replacement = build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
        result.replace_range(em.start..em.end, &replacement);
    }

    Cow::Owned(result)
}

/// Mask embedded credentials in a proxy URL line.
///
/// Replaces only the password segment in `://user:password@` patterns,
/// preserving the rest of the URL structure. This is a dedicated function
/// because the generic `redact_string()` does whole-match replacement and
/// would destroy the URL.
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
    dedup_overlapping_matches(&mut all_matches);
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

/// Redact a snapshot in place, setting its `redaction_state` and populating `redactions`.
///
/// - Scans config file contents and shadow entries for secrets.
/// - High-confidence findings are auto-resolved (content redacted inline).
/// - Low-confidence findings remain unresolved → PartiallyRedacted state.
/// - If all findings are high-confidence and resolved → FullyRedacted state.
pub fn redact(snapshot: &mut InspectionSnapshot, _opts: &RedactOptions) {
    let mut all_findings: Vec<RedactionFinding> = Vec::new();
    let mut registry = CounterRegistry::default();

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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                gpg_key.content = content;
                all_findings.extend(findings);
            }
        }
    }

    // Scan systemd drop-in contents in services section
    if let Some(ref mut services) = snapshot.services {
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
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

    // Scan kernel cmdline and config snippets in kernelboot section
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
            dedup_overlapping_matches(&mut all_matches);
            all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
            for (pat_idx, em) in all_matches {
                let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                let token = registry.token_for(&kind_label, &em.text);
                content.replace_range(em.start..em.end, &token);
            }
            kernelboot.cmdline = content;
            all_findings.extend(cmdline_findings);
        }

        // Scan dracut, modprobe, modules-load, and tuned config snippets
        let snippet_chains: Vec<&mut Vec<ConfigSnippet>> = vec![
            &mut kernelboot.dracut_conf,
            &mut kernelboot.modprobe_d,
            &mut kernelboot.modules_load_d,
            &mut kernelboot.tuned_custom_profiles,
        ];
        for snippets in snippet_chains {
            for snippet in snippets.iter_mut() {
                let findings = scan_content(&snippet.content, &snippet.path);
                if !findings.is_empty() {
                    let mut content = snippet.content.clone();
                    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                    for (idx, pat) in PATTERNS.iter().enumerate() {
                        for em in collect_eligible_matches(pat, &content, Some(&snippet.path)) {
                            all_matches.push((idx, em));
                        }
                    }
                    dedup_overlapping_matches(&mut all_matches);
                    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                    for (pat_idx, em) in all_matches {
                        let kind_label =
                            format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                        let token = registry.token_for(&kind_label, &em.text);
                        content.replace_range(em.start..em.end, &token);
                    }
                    snippet.content = content;
                    all_findings.extend(findings);
                }
            }
        }
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
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
                    dedup_overlapping_matches(&mut all_matches);
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                timer.exec_start = content;
                all_findings.extend(findings);
            }

            // SystemdTimer.service_content — raw service unit text that
            // may contain secrets the exec_start parse didn't capture.
            if !timer.service_content.is_empty() {
                let svc_path = format!(
                    "{}:{}.service",
                    if timer.path.is_empty() {
                        "timer"
                    } else {
                        &timer.path
                    },
                    timer.name
                );
                let findings = scan_content(&timer.service_content, &svc_path);
                if !findings.is_empty() {
                    let mut content = timer.service_content.clone();
                    let mut all_matches: Vec<(usize, EligibleMatch)> = Vec::new();
                    for (idx, pat) in PATTERNS.iter().enumerate() {
                        for em in collect_eligible_matches(pat, &content, Some(&svc_path)) {
                            all_matches.push((idx, em));
                        }
                    }
                    dedup_overlapping_matches(&mut all_matches);
                    all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                    for (pat_idx, em) in all_matches {
                        let kind_label =
                            format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                        let token = registry.token_for(&kind_label, &em.text);
                        content.replace_range(em.start..em.end, &token);
                    }
                    timer.service_content = content;
                    all_findings.extend(findings);
                }
            }
        }
    }

    // Scan SELinux audit rules and PAM configs for secrets.
    // Commented lines still need redaction — secrets in comments are
    // still secrets in the file.
    if let Some(ref mut selinux) = snapshot.selinux {
        for rule in &mut selinux.audit_rules {
            let redacted = redact_string(&rule.content);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(&rule.content, "audit_rules");
                all_findings.extend(findings);
                rule.content = new_val.clone();
            } else {
                // Multi-line content may have comment lines with secrets.
                let mut lines: Vec<String> = rule.content.lines().map(String::from).collect();
                let mut changed = false;
                for line in &mut lines {
                    if let Some(body) = line.strip_prefix('#') {
                        let body_trimmed = body.trim_start();
                        let prefix_len = line.len() - body_trimmed.len();
                        let body_redacted = redact_string(body_trimmed);
                        if let Cow::Owned(ref new_body) = body_redacted {
                            let findings = scan_content(body_trimmed, "audit_rules");
                            all_findings.extend(findings);
                            *line = format!("{}{}", &line[..prefix_len], new_body);
                            changed = true;
                        }
                    }
                }
                if changed {
                    rule.content = lines.join("\n");
                }
            }
        }
        for pam in &mut selinux.pam_configs {
            let redacted = redact_string(&pam.content);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(&pam.content, &pam.path);
                all_findings.extend(findings);
                pam.content = new_val.clone();
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
                dedup_overlapping_matches(&mut all_matches);
                all_matches.sort_by_key(|item| std::cmp::Reverse(item.1.start));
                for (pat_idx, em) in all_matches {
                    let kind_label = format!("{:?}", PATTERNS[pat_idx].finding_kind).to_lowercase();
                    let replacement =
                        build_replacement(&PATTERNS[pat_idx], &em, &mut registry, &kind_label);
                    content.replace_range(em.start..em.end, &replacement);
                }
                env_file.content = content;
                all_findings.extend(findings);
            }
        }

        // Git remote URLs — scan for embedded credentials (user:pass@host
        // and token-as-username ://token@host patterns)
        for item in &mut nrs.items {
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
                    dedup_overlapping_matches(&mut all_matches);
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
    if let Some(ref services) = snapshot.services {
        for drop_in in &services.drop_ins {
            post_redaction_content.insert(drop_in.path.clone(), drop_in.content.clone());
        }
    }
    if let Some(ref kernelboot) = snapshot.kernel_boot {
        post_redaction_content.insert("/proc/cmdline".to_string(), kernelboot.cmdline.clone());
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
        for timer in &sched.systemd_timers {
            let source_path = if timer.path.is_empty() {
                format!("timer:{}", timer.name)
            } else {
                timer.path.clone()
            };
            post_redaction_content.insert(source_path, timer.exec_start.clone());
            if !timer.service_content.is_empty() {
                let svc_path = format!(
                    "{}:{}.service",
                    if timer.path.is_empty() {
                        "timer"
                    } else {
                        &timer.path
                    },
                    timer.name
                );
                post_redaction_content.insert(svc_path, timer.service_content.clone());
            }
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
                    // The hint's keyword is gone from post-redaction content
                    // → the regex pass handled it.
                    !content.contains(kw)
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
    use inspectah_core::types::config::{ConfigFileEntry, ConfigSection};
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

    #[test]
    fn test_redact_scans_repo_file_content() {
        use inspectah_core::types::rpm::{RepoFile, RpmSection};
        let mut snapshot = InspectionSnapshot::new();
        snapshot.rpm = Some(RpmSection {
            repo_files: vec![RepoFile {
                path: "/etc/yum.repos.d/custom.repo".into(),
                content: "password = s3cretP@ss\nbaseurl=https://repo.example.com/\n".into(),
                include: true,
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
                include: true,
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
        let findings = scan_content(input, "/etc/app.conf");
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
        let findings = scan_content(input, "/etc/app.conf");
        assert_eq!(
            findings.len(),
            1,
            "C-style commented match must be filtered"
        );
        assert_eq!(findings[0].line, Some(2));
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
}
