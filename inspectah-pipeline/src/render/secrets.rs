//! Secrets review renderer — produces secrets-review.md listing all
//! redaction findings and recommended actions.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::redaction::{DetectionMethod, RedactionFinding, RedactionKind};

/// Render the secrets review markdown from a snapshot.
pub fn render_secrets_review(snap: &InspectionSnapshot) -> String {
    // Check if there's ANY sensitive content (redactions OR subscription)
    let has_redactions = !snap.redactions.is_empty();
    let has_subscription = snap.preserved_subscription;

    if !has_redactions && !has_subscription {
        return "# Secrets Review\n\nNo redactions recorded.\n".to_string();
    }

    let mut lines = Vec::new();
    lines.push("# Secrets Review".into());
    lines.push(String::new());

    // Classify findings by kind
    let mut excluded = Vec::new();
    let mut inline_redacted = Vec::new();
    let mut flagged = Vec::new();
    let overridden: Vec<&RedactionFinding> = Vec::new(); // Phase 1: no "overridden" variant in enum

    for finding in &snap.redactions {
        if finding.source.is_empty() {
            continue;
        }
        match finding.kind {
            RedactionKind::Excluded => excluded.push(finding),
            RedactionKind::Inline => inline_redacted.push(finding),
            RedactionKind::Flagged => flagged.push(finding),
        }
    }

    let _ = &overridden; // suppress unused warning until overridden variant added
    let n_redacted = excluded.len() + inline_redacted.len();
    let mut parts = Vec::new();
    if !excluded.is_empty() {
        parts.push(format!("{} excluded", excluded.len()));
    }
    if !inline_redacted.is_empty() {
        parts.push(format!("{} inline", inline_redacted.len()));
    }
    let breakdown = if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    };
    let flagged_part = if flagged.is_empty() {
        String::new()
    } else {
        format!(", {} flagged for review", flagged.len())
    };
    let overridden_part = if overridden.is_empty() {
        String::new()
    } else {
        format!(", {} overridden", overridden.len())
    };

    lines.push(format!(
        "> Detected secrets: {n_redacted} redacted{breakdown}{flagged_part}{overridden_part}"
    ));
    lines.push(String::new());

    lines.push("The following items were redacted or excluded. Handle them according to".into());
    lines.push("the action specified for each item.".into());
    lines.push(String::new());

    // Excluded files
    if !excluded.is_empty() {
        lines.push("## Excluded Files".into());
        lines.push(String::new());
        lines.push("These files were removed from the output entirely.".into());
        lines.push(String::new());
        lines.push("| Path | Pattern | Remediation |".into());
        lines.push("|------|---------|-------------|".into());
        for f in &excluded {
            let rem = remediation_label(&f.remediation);
            lines.push(format!("| {} | {} | {} |", f.path, f.pattern, rem));
        }
        lines.push(String::new());
    }

    // Inline redactions
    if !inline_redacted.is_empty() {
        lines.push("## Inline Redactions".into());
        lines.push(String::new());
        lines.push(
            "Secret values in these files/entries were replaced with `[REDACTED-*]` tokens.".into(),
        );
        lines.push(String::new());
        lines.push("| Path | Line | Pattern | Detection |".into());
        lines.push("|------|------|---------|-----------|".into());
        for f in &inline_redacted {
            let line_str = f.line.map(|l| l.to_string()).unwrap_or_else(|| "--".into());
            let detection = detection_label(f);
            lines.push(format!(
                "| {} | {} | {} | {} |",
                f.path, line_str, f.pattern, detection
            ));
        }
        lines.push(String::new());
    }

    // Flagged for review
    if !flagged.is_empty() {
        lines.push("## Flagged for Review".into());
        lines.push(String::new());
        lines.push("| Path | Line | Confidence | Why Flagged |".into());
        lines.push("|------|------|------------|-------------|".into());
        for f in &flagged {
            let line_str = f.line.map(|l| l.to_string()).unwrap_or_else(|| "--".into());
            let conf = f
                .confidence
                .as_ref()
                .map(|c| format!("{:?}", c).to_lowercase())
                .unwrap_or_else(|| "--".into());
            let why = if f.pattern.is_empty() {
                "--"
            } else {
                &f.pattern
            };
            lines.push(format!(
                "| {} | {} | {} | {} |",
                f.path, line_str, conf, why
            ));
        }
        lines.push(String::new());
    }

    // Overridden exclusions
    if !overridden.is_empty() {
        lines.push("## Overridden Exclusions".into());
        lines.push(String::new());
        lines.push("These files were originally excluded by the scanner but deliberately".into());
        lines.push("re-included by the operator during triage.".into());
        lines.push(String::new());
        for f in &overridden {
            let method = format!("{:?}", f.detection_method).to_lowercase();
            lines.push(format!(
                "- **{}** — {} (originally excluded for: {})",
                f.path, f.pattern, method
            ));
        }
        lines.push(String::new());
    }

    // Subscription material
    if snap.preserved_subscription {
        lines.push("## Subscription Material".into());
        lines.push(String::new());
        lines.push(
            "RHEL subscription material was preserved in `subscription/` for build-time mounting."
                .into(),
        );
        lines.push(String::new());
        lines.push(
            "**Action:** Mount these directories at build time (do NOT copy into the image):"
                .into(),
        );
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("podman build \\".into());
        lines.push("  -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into());
        lines.push("  -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("  -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("  -f Containerfile .".into());
        lines.push("```".into());
        lines.push(String::new());
        lines.push("Or use the build helper:".into());
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("inspectah build <tarball> -t <name:tag>".into());
        lines.push("```".into());
        lines.push(String::new());
        if let Some(ref sub) = snap.subscription
            && !sub.entitlement_certs.is_empty()
        {
            lines.push(format!(
                "**Certificates:** {} entitlement certificate(s)",
                sub.entitlement_certs.len()
            ));
            if let Some(expiry) = sub.earliest_expiry {
                lines.push(format!(
                    "**Earliest expiry:** {}",
                    expiry
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| "unknown".into())
                ));
            }
            lines.push(String::new());
        }
    }

    lines.join("\n")
}

fn remediation_label(rem: &str) -> &str {
    match rem {
        "regenerate" => "Regenerate on target",
        "provision" => "Provision from secret store",
        "value-removed" => "Value removed inline",
        other => other,
    }
}

fn detection_label(f: &RedactionFinding) -> String {
    match f.detection_method {
        DetectionMethod::Pattern => "pattern".into(),
        DetectionMethod::Heuristic => {
            let conf = f
                .confidence
                .as_ref()
                .map(|c| format!("{:?}", c).to_lowercase())
                .unwrap_or_else(|| "unknown".into());
            format!("heuristic ({conf})")
        }
        DetectionMethod::PathBased => "path-based".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secrets_review_renders() {
        let snap = InspectionSnapshot::new();
        let md = render_secrets_review(&snap);
        assert!(md.contains("# Secrets Review"));
    }

    #[test]
    fn test_secrets_review_no_redactions() {
        let snap = InspectionSnapshot::new();
        let md = render_secrets_review(&snap);
        assert!(md.contains("No redactions recorded"));
    }

    #[test]
    fn test_secrets_review_with_findings() {
        let mut snap = InspectionSnapshot::new();
        snap.redactions = vec![RedactionFinding {
            path: "/etc/shadow".into(),
            source: "file".into(),
            kind: RedactionKind::Excluded,
            pattern: "shadow_hash".into(),
            remediation: "regenerate".into(),
            detection_method: DetectionMethod::Pattern,
            line: None,
            replacement: None,
            confidence: None,
            finding_kind: None,
        }];
        let md = render_secrets_review(&snap);
        assert!(md.contains("# Secrets Review"));
        assert!(md.contains("Excluded Files"));
        assert!(md.contains("/etc/shadow"));
    }

    #[test]
    fn test_secrets_review_subscription_only() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(SubscriptionSection::default());
        snap.redactions = vec![]; // No redactions, only subscription
        let md = render_secrets_review(&snap);
        assert!(
            md.contains("# Secrets Review"),
            "must have secrets review header"
        );
        assert!(
            !md.contains("No redactions recorded"),
            "must not say 'no redactions' when subscription is present"
        );
        assert!(
            md.contains("## Subscription Material"),
            "must have subscription section"
        );
        assert!(
            md.contains("inspectah build"),
            "must reference inspectah build helper"
        );
    }
}
