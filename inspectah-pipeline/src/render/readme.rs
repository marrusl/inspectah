//! README renderer — produces README.md with build commands and findings summary.

use inspectah_core::snapshot::InspectionSnapshot;

use super::containerfile::base_image_from_snapshot;

/// Render the README markdown from a snapshot.
pub fn render_readme(snap: &InspectionSnapshot) -> String {
    let mut lines = Vec::new();

    lines.push("# inspectah output".into());
    lines.push(String::new());

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

    let no_baseline = snap
        .rpm
        .as_ref()
        .map(|r| r.no_baseline)
        .unwrap_or(false);
    if no_baseline {
        lines.push(format!(
            "| Packages (all -- no baseline) | {pkg_added} |"
        ));
    } else {
        lines.push(format!(
            "| Packages added (beyond base image) | {pkg_added} |"
        ));
    }

    lines.push(format!(
        "| Services ({svc_enabled} enabled, {svc_disabled} disabled) | {} |",
        svc_enabled + svc_disabled
    ));

    if let Some(nrs) = &snap.non_rpm_software {
        if !nrs.items.is_empty() {
            lines.push(format!(
                "| Non-RPM software items | {} |",
                nrs.items.len()
            ));
        }
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
    lines.push("```bash".into());
    lines.push("podman build -t my-bootc-image -f Containerfile .".into());
    lines.push("```".into());
    lines.push(String::new());
    lines.push("```bash".into());

    let has_kargs = snap
        .kernel_boot
        .as_ref()
        .map(|kb| !kb.cmdline.is_empty())
        .unwrap_or(false);
    if has_kargs {
        lines.push(
            "# Custom kernel args detected -- verify they are baked into the image".into(),
        );
        lines.push(
            "# or pass them via the bootloader configuration at deploy time.".into(),
        );
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
    if let Some(sel) = &snap.selinux {
        if sel.mode == "enforcing" {
            install_flags.push("--enforce-container-sigpolicy");
        }
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
    lines.push("| `report.html` | Interactive report (open in browser) |".into());
    lines.push("| `secrets-review.md` | Redacted items requiring manual handling |".into());
    lines.push("| `kickstart-suggestion.ks` | Suggested deploy-time settings |".into());
    lines.push("| `inspection-snapshot.json` | Raw data for re-rendering (`--from-snapshot`) |".into());
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
        "See [`audit-report.md`](audit-report.md) or [`report.html`](report.html) for full details."
            .into(),
    );
    lines.push(String::new());

    let _ = base_image_from_snapshot(snap); // retained for future FROM reference
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readme_renders() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(md.contains("podman build"), "must contain podman build command");
    }

    #[test]
    fn test_readme_contains_artifacts() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(md.contains("Containerfile"));
        assert!(md.contains("audit-report.md"));
        assert!(md.contains("report.html"));
        assert!(md.contains("secrets-review.md"));
        assert!(md.contains("kickstart-suggestion.ks"));
    }

    #[test]
    fn test_readme_findings_summary() {
        let snap = InspectionSnapshot::new();
        let md = render_readme(&snap);
        assert!(md.contains("## Findings summary"));
    }
}
