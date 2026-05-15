use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::kernelboot::ConfigSnippet;
use inspectah_core::types::redaction::{
    Confidence, DetectionMethod, FindingKind, RedactionFinding, RedactionHint, RedactionKind,
    RedactionState,
};
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::redaction::patterns::{scan_shadow, PATTERNS};

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

    for pat in PATTERNS.iter() {
        let kind_label = format!("{:?}", pat.finding_kind).to_lowercase();
        // Collect matches first to avoid borrow issues.
        let matches: Vec<(usize, usize, String)> = pat
            .regex
            .find_iter(&result)
            .map(|m| (m.start(), m.end(), m.as_str().to_string()))
            .collect();

        if !matches.is_empty() {
            // Replace from end to start to preserve offsets.
            let mut buf = result.clone();
            for (start, end, matched) in matches.into_iter().rev() {
                let token = registry.token_for(&kind_label, &matched);
                buf.replace_range(start..end, &token);
            }
            result = buf;
        }
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

/// Scan content for secrets. Returns findings for all detected patterns.
pub fn scan_content(content: &str, path: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();

    // Shadow-specific logic for /etc/shadow paths
    if path.ends_with("/shadow") || path == "/etc/shadow" {
        findings.extend(scan_shadow(content, path));
    }

    // Generic pattern matching
    for pat in PATTERNS.iter() {
        for mat in pat.regex.find_iter(content) {
            // Find line number
            let line_num = content[..mat.start()].lines().count() + 1;

            findings.push(RedactionFinding {
                path: path.to_string(),
                source: "pattern".into(),
                kind: RedactionKind::Inline,
                pattern: format!("{:?}", pat.finding_kind).to_lowercase(),
                remediation: pat.remediation.to_string(),
                line: Some(line_num as i32),
                replacement: None,
                detection_method: pat.detection_method.clone(),
                confidence: Some(pat.confidence),
                finding_kind: Some(pat.finding_kind.clone()),
            });
        }
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
    if let Some(rest) = reason.strip_suffix('"').or(Some(reason)) {
        if let Some(idx) = rest.find("parameter: ") {
            let kw = rest[idx + "parameter: ".len()..].trim().trim_matches('"');
            if !kw.is_empty() {
                return Some(kw.to_string());
            }
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
                // Redact high-confidence findings inline
                let mut content = file.content.clone();
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        // Find and replace pattern matches in content
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
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
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
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
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
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
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
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
            for finding in &cmdline_findings {
                if finding.confidence == Some(Confidence::High) {
                    let kind_label = finding
                        .finding_kind
                        .as_ref()
                        .map(|k| format!("{:?}", k).to_lowercase())
                        .unwrap_or_else(|| "secret".to_string());
                    if let Some(pat) = PATTERNS
                        .iter()
                        .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                    {
                        let matches: Vec<(usize, usize, String)> = pat
                            .regex
                            .find_iter(&content)
                            .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                            .collect();
                        for (start, end, matched) in matches.into_iter().rev() {
                            let token = registry.token_for(&kind_label, &matched);
                            content.replace_range(start..end, &token);
                        }
                    }
                }
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
                    for finding in &findings {
                        if finding.confidence == Some(Confidence::High) {
                            let kind_label = finding
                                .finding_kind
                                .as_ref()
                                .map(|k| format!("{:?}", k).to_lowercase())
                                .unwrap_or_else(|| "secret".to_string());
                            if let Some(pat) = PATTERNS.iter().find(|p| {
                                format!("{:?}", p.finding_kind).to_lowercase() == kind_label
                            }) {
                                let matches: Vec<(usize, usize, String)> = pat
                                    .regex
                                    .find_iter(&content)
                                    .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                    .collect();
                                for (start, end, matched) in matches.into_iter().rev() {
                                    let token = registry.token_for(&kind_label, &matched);
                                    content.replace_range(start..end, &token);
                                }
                            }
                        }
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

    // Scan scheduled task commands for secrets.
    if let Some(ref mut sched) = snapshot.scheduled_tasks {
        // GeneratedTimerUnit.command fields (cron commands converted to timers)
        for unit in &mut sched.generated_timer_units {
            let findings = scan_content(&unit.command, &unit.source_path);
            if !findings.is_empty() {
                let mut content = unit.command.clone();
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
                }
                unit.command = content;
                all_findings.extend(findings);
            }
        }

        // AtJob.command fields
        for at_job in &mut sched.at_jobs {
            let findings = scan_content(&at_job.command, &at_job.file);
            if !findings.is_empty() {
                let mut content = at_job.command.clone();
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
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
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
                }
                timer.exec_start = content;
                all_findings.extend(findings);
            }
        }
    }

    // Scan SELinux audit rules and PAM configs for secrets.
    if let Some(ref mut selinux) = snapshot.selinux {
        for rule in &mut selinux.audit_rules {
            let redacted = redact_string(&rule.content);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(&rule.content, "audit_rules");
                all_findings.extend(findings);
                rule.content = new_val.clone();
            }
        }
        for pam in &mut selinux.pam_configs {
            let redacted = redact_string(&pam.content);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(&pam.content, "pam_configs");
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
                for finding in &findings {
                    if finding.confidence == Some(Confidence::High) {
                        let kind_label = finding
                            .finding_kind
                            .as_ref()
                            .map(|k| format!("{:?}", k).to_lowercase())
                            .unwrap_or_else(|| "secret".to_string());
                        if let Some(pat) = PATTERNS
                            .iter()
                            .find(|p| format!("{:?}", p.finding_kind).to_lowercase() == kind_label)
                        {
                            let matches: Vec<(usize, usize, String)> = pat
                                .regex
                                .find_iter(&content)
                                .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                .collect();
                            for (start, end, matched) in matches.into_iter().rev() {
                                let token = registry.token_for(&kind_label, &matched);
                                content.replace_range(start..end, &token);
                            }
                        }
                    }
                }
                env_file.content = content;
                all_findings.extend(findings);
            }
        }

        // Git remote URLs — scan for embedded credentials (user:pass@host)
        for item in &mut nrs.items {
            if !item.git_remote.is_empty() {
                let (masked, finding) = mask_proxy_credentials(&item.git_remote, &item.path);
                if let Some(f) = finding {
                    item.git_remote = masked.into_owned();
                    all_findings.push(f);
                }
                // Also run generic pattern scan for other secret patterns
                let findings = scan_content(&item.git_remote, &item.path);
                if !findings.is_empty() {
                    let mut content = item.git_remote.clone();
                    for finding in &findings {
                        if finding.confidence == Some(Confidence::High) {
                            let kind_label = finding
                                .finding_kind
                                .as_ref()
                                .map(|k| format!("{:?}", k).to_lowercase())
                                .unwrap_or_else(|| "secret".to_string());
                            if let Some(pat) = PATTERNS.iter().find(|p| {
                                format!("{:?}", p.finding_kind).to_lowercase() == kind_label
                            }) {
                                let matches: Vec<(usize, usize, String)> = pat
                                    .regex
                                    .find_iter(&content)
                                    .map(|m| (m.start(), m.end(), m.as_str().to_string()))
                                    .collect();
                                for (start, end, matched) in matches.into_iter().rev() {
                                    let token = registry.token_for(&kind_label, &matched);
                                    content.replace_range(start..end, &token);
                                }
                            }
                        }
                    }
                    item.git_remote = content;
                    all_findings.extend(findings);
                }
            }
        }
    }

    // Scan sudoers rules for secrets via generic pattern pass.
    if let Some(ref mut users) = snapshot.users_groups {
        for rule in &mut users.sudoers_rules {
            let redacted = redact_string(rule);
            if let Cow::Owned(ref new_val) = redacted {
                let findings = scan_content(rule, "/etc/sudoers");
                all_findings.extend(findings);
                *rule = new_val.clone();
            }
        }
    }

    // Scan shadow entries in users_groups section
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
            all_findings.extend(findings);
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

    if all_findings.is_empty() {
        // Nothing to redact — mark as fully clean
        snapshot.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by,
            config_hash,
        });
    } else if unresolved.is_empty() {
        // All findings were high-confidence and auto-resolved
        snapshot.redaction_state = Some(RedactionState::FullyRedacted {
            redacted_by,
            config_hash,
        });
    } else {
        // Some findings need operator triage
        let hints: Vec<RedactionHint> = unresolved
            .iter()
            .map(|f| RedactionHint {
                path: f.path.clone(),
                reason: f.remediation.clone(),
                confidence: f.confidence,
            })
            .collect();

        snapshot.redaction_state = Some(RedactionState::PartiallyRedacted {
            redacted_by,
            config_hash,
            unresolved_count: unresolved.len() as u32,
            unresolved_hints: hints,
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
}
