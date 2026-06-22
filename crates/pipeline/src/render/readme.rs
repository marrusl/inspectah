//! README renderer — produces README.md with build commands and findings summary.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::completeness::Completeness;

use super::baseline_fmt;
use super::containerfile::base_image_from_snapshot;

/// Render the README markdown from a snapshot.
pub fn render_readme(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();

    lines.push("# inspectah output".into());
    lines.push(String::new());

    // Completeness warning
    let (affected_ids, reason) = match &snap.completeness {
        Completeness::Partial {
            degraded_sections,
            reason,
        } => (degraded_sections.clone(), reason.clone()),
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            reason,
        } => {
            let mut ids = failed_sections.clone();
            ids.extend(degraded_sections.iter().copied());
            (ids, reason.clone())
        }
        Completeness::Complete => (vec![], String::new()),
    };
    if !affected_ids.is_empty() {
        let section_names: Vec<String> = affected_ids
            .iter()
            .map(|id| format!("{:?}", id).to_lowercase())
            .collect();
        lines.push("> **WARNING: Incomplete inspection**".into());
        lines.push(">".into());
        lines.push(format!(
            "> The following inspector sections may be missing or degraded: {}",
            section_names.join(", ")
        ));
        if !reason.is_empty() {
            lines.push(format!("> Reason: {reason}"));
        }
        lines.push(">".into());
        lines.push("> Review the audit report for details before building.".into());
        lines.push(String::new());
    }

    // Summary of findings
    if let Some(os) = &snap.os_release {
        let name = if os.pretty_name.is_empty() {
            &os.name
        } else {
            &os.pretty_name
        };
        lines.push(format!("Generated from **{name}**."));
        lines.push(String::new());
    }

    let hostname = snap
        .meta
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !hostname.is_empty() {
        lines.push(format!("Hostname: `{hostname}`"));
        lines.push(String::new());
    }

    // Findings summary table
    lines.push("## Findings summary".into());
    lines.push(String::new());

    let pkg_added = snap
        .rpm
        .as_ref()
        .map(|r| r.packages_added.iter().filter(|p| p.include).count())
        .unwrap_or(0);

    let svc_enabled = snap
        .services
        .as_ref()
        .map(|s| s.enabled_units.len())
        .unwrap_or(0);
    let svc_disabled = snap
        .services
        .as_ref()
        .map(|s| s.disabled_units.len())
        .unwrap_or(0);

    let warning_count = snap.warnings.len();
    let redaction_count = snap.redactions.len();

    lines.push("| Category | Count |".into());
    lines.push("|---|---|".into());

    lines.push(format!(
        "| Packages added (beyond base image) | {pkg_added} |"
    ));

    lines.push(format!(
        "| Services ({svc_enabled} enabled, {svc_disabled} disabled) | {} |",
        svc_enabled + svc_disabled
    ));

    if let Some(nrs) = &snap.non_rpm_software
        && !nrs.items.is_empty()
    {
        lines.push(format!("| Non-RPM software items | {} |", nrs.items.len()));
    }

    if let Some(containers) = &snap.containers {
        let q = containers.quadlet_units.len();
        let c = containers.compose_files.len();
        if q > 0 || c > 0 {
            lines.push(format!(
                "| Container workloads | {q} quadlet, {c} compose |"
            ));
        }
    }

    if redaction_count > 0 {
        lines.push(format!("| Secrets redacted | {redaction_count} |"));
    }
    lines.push(format!("| Warnings | {warning_count} |"));
    lines.push(String::new());

    // Build and deploy
    lines.push("## Build and deploy".into());
    lines.push(String::new());

    // Subscription-specific build instructions
    if snap.preserved_subscription {
        lines.push("## Building with Subscription".into());
        lines.push(String::new());
        lines.push("This image requires RHEL subscription material at build time.".into());
        lines.push(String::new());

        // Include cert expiry warning in README
        if let Some(sub) = &snap.subscription
            && let Some(expiry) = sub.earliest_expiry
        {
            let format = time::format_description::parse("[year]-[month]-[day]")
                .expect("static format description");
            let date_str =
                expiry.format(&format).unwrap_or_else(|_| "unknown".into());
            let now = time::OffsetDateTime::now_utc();
            let days = (expiry - now).whole_days();

            if days < 0 {
                let abs_days = days.unsigned_abs();
                let day_word = if abs_days == 1 { "day" } else { "days" };
                lines.push(format!(
                    "> **WARNING:** Subscription certs EXPIRED on {date_str} \
                     ({abs_days} {day_word} ago). Builds will fail on unregistered systems. \
                     Re-scan with fresh certs."
                ));
            } else if days < 7 {
                let day_word = if days == 1 { "day" } else { "days" };
                lines.push(format!(
                    "> **WARNING:** Subscription certs expire {date_str} \
                     ({days} {day_word} remaining). Rebuild soon."
                ));
            } else {
                lines.push(format!(
                    "> Subscription certs expire: {date_str} ({days} days remaining)."
                ));
            }
            lines.push(String::new());
        }
        lines.push(
            "**Recommended:** Use the build helper (handles subscription mounts automatically):"
                .into(),
        );
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("inspectah build <tarball> -t my-bootc-image:latest".into());
        lines.push("```".into());
        lines.push(String::new());
        lines.push("**Manual build:** Mount subscription directories:".into());
        lines.push(String::new());
        lines.push("```bash".into());
        lines.push("podman build \\".into());
        lines.push("  -v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z \\".into());
        lines.push("  -v ./subscription/rhsm:/run/secrets/rhsm:z \\".into());
        lines.push("  -v ./subscription/redhat.repo:/run/secrets/redhat.repo:z \\".into());
        lines.push("  -t my-bootc-image -f Containerfile .".into());
        lines.push("```".into());
        lines.push(String::new());
    } else {
        lines.push("```bash".into());
        lines.push("podman build -t my-bootc-image -f Containerfile .".into());
        lines.push("```".into());
        lines.push(String::new());
    }
    lines.push("```bash".into());

    let has_kargs = snap
        .kernel_boot
        .as_ref()
        .map(|kb| !kb.cmdline.is_empty())
        .unwrap_or(false);
    if has_kargs {
        lines.push("# Custom kernel args detected -- verify they are baked into the image".into());
        lines.push("# or pass them via the bootloader configuration at deploy time.".into());
    }
    lines.push("# Switch an existing system to the new image:".into());
    lines.push("bootc switch my-bootc-image:latest".into());
    lines.push(String::new());
    lines.push("# Or install to a new disk:".into());

    let mut install_flags = Vec::new();
    let is_centos = snap
        .os_release
        .as_ref()
        .map(|o| o.id == "centos")
        .unwrap_or(false);
    if is_centos {
        install_flags.push("--target-no-signature-verification");
    }
    if let Some(sel) = &snap.selinux
        && sel.mode == "enforcing"
    {
        install_flags.push("--enforce-container-sigpolicy");
    }
    if install_flags.is_empty() {
        lines.push("bootc install to-disk /dev/sdX".into());
    } else {
        lines.push(format!(
            "bootc install to-disk {} /dev/sdX",
            install_flags.join(" ")
        ));
    }
    lines.push("```".into());
    lines.push(String::new());
    lines.push(
        "Review `kickstart-suggestion.ks` for deployment-time settings (hostname, DHCP, DNS)."
            .into(),
    );
    lines.push(String::new());

    // Artifacts
    lines.push("## Artifacts".into());
    lines.push(String::new());
    lines.push("| File | Description |".into());
    lines.push("|---|---|".into());
    lines.push("| `Containerfile` | Image definition |".into());
    lines.push("| `config/` | Files to COPY into the image |".into());
    lines.push("| `audit-report.md` | Full findings (markdown) |".into());
    lines.push("| `audit-report.html` | Interactive report (open in browser) |".into());
    lines.push("| `secrets-review.md` | Redacted items requiring manual handling |".into());
    lines.push("| `kickstart-suggestion.ks` | Suggested deploy-time settings |".into());
    lines.push(
        "| `inspection-snapshot.json` | Raw data for re-rendering (`--from-snapshot`) |".into(),
    );
    lines.push(String::new());

    // Warnings
    if !snap.warnings.is_empty() {
        lines.push("## Warnings".into());
        lines.push(String::new());
        for w in &snap.warnings {
            if !w.message.is_empty() {
                let prefix = if w.inspector.is_empty() {
                    String::new()
                } else {
                    format!("**{}:** ", w.inspector)
                };
                lines.push(format!("- {prefix}{}", w.message));
            }
        }
        lines.push(String::new());
    }

    lines.push(
        "See [`audit-report.md`](audit-report.md) or [`audit-report.html`](audit-report.html) for full details."
            .into(),
    );
    lines.push(String::new());

    // Baseline comparison section
    let baseline_lines = baseline_fmt::baseline_section_lines(snap);
    if !baseline_lines.is_empty() {
        lines.extend(baseline_lines);
    }

    let _ = base_image_from_snapshot(snap); // retained for future FROM reference
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::baseline::{BaselineData, ResolutionStrategy, TargetImageIdentity};
    use inspectah_core::types::completeness::InspectorId;
    use inspectah_core::types::rpm::{RpmSection, VersionChange, VersionChangeDirection};
    use std::collections::HashMap;

    fn test_target_image() -> TargetImageIdentity {
        TargetImageIdentity {
            image_ref: "quay.io/centos-bootc/centos-bootc:stream9".into(),
            strategy: ResolutionStrategy::OsRelease,
        }
    }

    fn test_baseline() -> BaselineData {
        BaselineData {
            image_digest: "sha256:abc123def456".into(),
            packages: HashMap::new(),
            extracted_at: "2026-05-18T14:32:00Z".into(),
        }
    }

    #[test]
    fn readme_includes_baseline_section_full() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = Some(test_baseline());
        snap.rpm = Some(RpmSection {
            version_changes: vec![VersionChange {
                name: "glibc".into(),
                direction: VersionChangeDirection::Upgrade,
                ..Default::default()
            }],
            ..Default::default()
        });
        let md = render_readme(&snap);
        assert!(
            md.contains("## Baseline comparison"),
            "must have baseline section"
        );
        assert!(md.contains("centos-bootc:stream9"));
        assert!(md.contains("os-release (auto-detected)"));
        assert!(md.contains("sha256:abc123def456"));
    }

    #[test]
    fn readme_baseline_section_degraded() {
        let mut snap = InspectionSnapshot::new();
        snap.target_image = Some(test_target_image());
        snap.baseline = None;
        let md = render_readme(&snap);
        assert!(md.contains("## Baseline comparison"));
        assert!(md.contains("unavailable"));
    }

    #[test]
    fn readme_baseline_section_absent_when_no_target() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(!md.contains("Baseline comparison"));
    }

    #[test]
    fn test_readme_renders() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(
            md.contains("podman build"),
            "must contain podman build command"
        );
    }

    #[test]
    fn test_readme_contains_artifacts() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(md.contains("Containerfile"));
        assert!(md.contains("audit-report.md"));
        assert!(md.contains("audit-report.html"));
        assert!(md.contains("secrets-review.md"));
        assert!(md.contains("kickstart-suggestion.ks"));
    }

    #[test]
    fn test_readme_findings_summary() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(md.contains("## Findings summary"));
    }

    #[test]
    fn test_readme_partial_completeness_warning() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config, InspectorId::Rpm],
            degraded_sections: vec![],
            reason: "inspectors timed out".into(),
        };
        let md = render_readme(&snap);
        assert!(
            md.contains("WARNING: Incomplete inspection"),
            "must contain incompleteness warning"
        );
        assert!(md.contains("config"), "must list config section");
        assert!(md.contains("rpm"), "must list rpm section");
        assert!(
            md.contains("inspectors timed out"),
            "must include the reason"
        );
    }

    #[test]
    fn test_readme_full_completeness_no_warning() {
        let mut snap = InspectionSnapshot::new();
        snap.completeness = Completeness::Complete;
        let md = render_readme(&snap);
        assert!(
            !md.contains("WARNING: Incomplete inspection"),
            "full completeness must not produce warning"
        );
    }

    #[test]
    fn test_readme_subscription_build_instructions() {
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        let md = render_readme(&snap);
        assert!(
            md.contains("## Building with Subscription"),
            "must have subscription build section heading"
        );
        assert!(
            md.contains("inspectah build"),
            "must reference inspectah build helper"
        );
        assert!(
            md.contains("-v ./subscription/entitlement:/run/secrets/etc-pki-entitlement:z"),
            "must include subscription mount instructions"
        );
    }

    #[test]
    fn test_readme_subscription_expiry_far_future() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        let expiry = time::OffsetDateTime::now_utc() + time::Duration::days(30);
        snap.subscription = Some(SubscriptionSection {
            earliest_expiry: Some(expiry),
            ..Default::default()
        });
        let md = render_readme(&snap);
        assert!(
            md.contains("Subscription certs expire:"),
            "must show expiry date"
        );
        assert!(
            md.contains("days remaining"),
            "must show days remaining"
        );
    }

    #[test]
    fn test_readme_subscription_expiry_imminent() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        let expiry = time::OffsetDateTime::now_utc() + time::Duration::days(3);
        snap.subscription = Some(SubscriptionSection {
            earliest_expiry: Some(expiry),
            ..Default::default()
        });
        let md = render_readme(&snap);
        assert!(
            md.contains("WARNING"),
            "must show warning for imminent expiry"
        );
        assert!(
            md.contains("Rebuild soon"),
            "must advise rebuild"
        );
    }

    #[test]
    fn test_readme_subscription_expired() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        let expiry = time::OffsetDateTime::now_utc() - time::Duration::days(5);
        snap.subscription = Some(SubscriptionSection {
            earliest_expiry: Some(expiry),
            ..Default::default()
        });
        let md = render_readme(&snap);
        assert!(
            md.contains("EXPIRED"),
            "must show EXPIRED for past certs"
        );
        assert!(
            md.contains("Re-scan"),
            "must advise re-scanning"
        );
    }

    #[test]
    fn test_readme_subscription_no_expiry_no_warning() {
        use inspectah_core::types::subscription::SubscriptionSection;
        let mut snap = InspectionSnapshot::new();
        snap.preserved_subscription = true;
        snap.subscription = Some(SubscriptionSection::default());
        let md = render_readme(&snap);
        assert!(
            md.contains("## Building with Subscription"),
            "must still show build section"
        );
        assert!(
            !md.contains("Subscription certs"),
            "must not show expiry when none available"
        );
    }
}
