use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::{
    Confidence, FindingKind, RedactionFinding, RedactionHint, RedactionKind, RedactionState,
};
use std::borrow::Cow;
use std::collections::HashMap;

use crate::redaction::patterns::{scan_shadow, PATTERNS};

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
}
