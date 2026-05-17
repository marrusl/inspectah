//! HTML report renderer — produces a minimal PatternFly HTML report.
//!
//! Phase 1: static HTML with embedded snapshot data. The full interactive
//! dashboard is Phase 5.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;

use super::safety::html_escape;

/// Render a minimal PatternFly HTML report from the snapshot.
pub fn render_report(snap: &InspectionSnapshot, _context: &RenderContext) -> String {
    use inspectah_core::types::completeness::Completeness;

    let os_name = snap
        .os_release
        .as_ref()
        .map(|o| {
            if o.pretty_name.is_empty() {
                o.name.clone()
            } else {
                o.pretty_name.clone()
            }
        })
        .unwrap_or_else(|| "Unknown System".into());

    let hostname = snap
        .meta
        .get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let pkg_count = snap
        .rpm
        .as_ref()
        .map(|r| r.packages_added.iter().filter(|p| p.include).count())
        .unwrap_or(0);

    let config_count = snap
        .config
        .as_ref()
        .map(|c| c.files.iter().filter(|f| f.include).count())
        .unwrap_or(0);

    let svc_count = snap
        .services
        .as_ref()
        .map(|s| s.enabled_units.len() + s.disabled_units.len())
        .unwrap_or(0);

    let storage_count = snap
        .storage
        .as_ref()
        .map(|s| s.fstab_entries.len())
        .unwrap_or(0);

    let kernelboot_count = snap
        .kernel_boot
        .as_ref()
        .map(|k| {
            let sysctl = k.sysctl_overrides.iter().filter(|o| o.include).count();
            let modules = k.modules_load_d.len() + k.modprobe_d.len();
            sysctl + modules
        })
        .unwrap_or(0);

    let warning_count = snap.warnings.len();

    let scheduled_count = snap
        .scheduled_tasks
        .as_ref()
        .map(|st| {
            st.cron_jobs.len()
                + st.systemd_timers.len()
                + st.generated_timer_units.len()
                + st.at_jobs.len()
        })
        .unwrap_or(0);

    let selinux_mode = snap
        .selinux
        .as_ref()
        .map(|s| {
            if s.mode.is_empty() {
                "unknown".to_string()
            } else {
                s.mode.clone()
            }
        })
        .unwrap_or_else(|| "n/a".to_string());

    let nonrpm_count = snap
        .non_rpm_software
        .as_ref()
        .map(|n| n.items.len())
        .unwrap_or(0);

    // Build package table rows
    let mut pkg_rows = String::new();
    if let Some(rpm) = &snap.rpm {
        for p in &rpm.packages_added {
            if !p.include {
                continue;
            }
            pkg_rows.push_str(&format!(
                "        <tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                html_escape(&p.name),
                html_escape(&p.version),
                html_escape(&p.release),
                html_escape(&p.arch),
                html_escape(&p.source_repo),
            ));
        }
    }

    // Build storage table rows
    let mut storage_rows = String::new();
    if let Some(storage) = &snap.storage {
        for entry in &storage.fstab_entries {
            storage_rows.push_str(&format!(
                "        <tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                html_escape(&entry.device),
                html_escape(&entry.mount_point),
                html_escape(&entry.fstype),
                html_escape(&entry.options),
            ));
        }
    }

    // Build kernelboot table rows
    let mut sysctl_rows = String::new();
    let mut module_items = String::new();
    let mut kernelboot_cmdline = String::new();
    if let Some(kb) = &snap.kernel_boot {
        if !kb.cmdline.is_empty() {
            kernelboot_cmdline = format!(
                "  <p><strong>Command line:</strong> <code>{}</code></p>\n",
                html_escape(&kb.cmdline)
            );
        }
        for o in &kb.sysctl_overrides {
            if !o.include {
                continue;
            }
            sysctl_rows.push_str(&format!(
                "        <tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                html_escape(&o.key),
                html_escape(&o.runtime),
                html_escape(&o.default),
                html_escape(&o.source),
            ));
        }
        for m in &kb.modules_load_d {
            module_items.push_str(&format!(
                "        <li>{} (modules-load.d)</li>\n",
                html_escape(&m.path),
            ));
        }
        for m in &kb.modprobe_d {
            module_items.push_str(&format!(
                "        <li>{} (modprobe.d)</li>\n",
                html_escape(&m.path),
            ));
        }
    }

    // Build completeness banner (if not complete)
    let completeness_banner = match &snap.completeness {
        Completeness::Partial {
            degraded_sections,
            reason,
        } => {
            let sections: Vec<String> = degraded_sections
                .iter()
                .map(|id| format!("{id:?}"))
                .collect();
            format!(
                r#"  <div style="background: #faecd5; border: 1px solid #f0ab00; padding: 1rem; margin: 1rem 0; border-radius: 4px;">
    <strong>Warning:</strong> This report was generated from an incomplete inspection.
    Sections with missing or degraded data: {}.
    Reason: {}
  </div>"#,
                html_escape(&sections.join(", ")),
                html_escape(reason),
            )
        }
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            reason,
        } => {
            let mut all: Vec<String> = failed_sections.iter().map(|id| format!("{id:?}")).collect();
            all.extend(degraded_sections.iter().map(|id| format!("{id:?}")));
            format!(
                r#"  <div style="background: #faecd5; border: 1px solid #f0ab00; padding: 1rem; margin: 1rem 0; border-radius: 4px;">
    <strong>Warning:</strong> This report was generated from an incomplete inspection.
    Sections with missing or degraded data: {}.
    Reason: {}
  </div>"#,
                html_escape(&all.join(", ")),
                html_escape(reason),
            )
        }
        Completeness::Complete => String::new(),
    };

    // Build warning list
    let mut warning_items = String::new();
    for w in &snap.warnings {
        warning_items.push_str(&format!("        <li>{}</li>\n", html_escape(&w.message)));
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>inspectah Report — {os_escaped}</title>
  <link rel="stylesheet" href="https://unpkg.com/@patternfly/patternfly@6/patternfly.min.css">
  <style>
    body {{ margin: 2rem; font-family: var(--pf-t--global--font--family--body); }}
    .summary-grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 1rem; margin: 1rem 0; }}
    .summary-card {{ padding: 1rem; border: 1px solid var(--pf-t--global--border--color--default); border-radius: 8px; }}
    .summary-card h3 {{ margin: 0 0 0.5rem; font-size: 0.875rem; color: var(--pf-t--global--text--color--subtle); }}
    .summary-card .value {{ font-size: 1.5rem; font-weight: 600; }}
    table {{ border-collapse: collapse; width: 100%; margin: 1rem 0; }}
    th, td {{ text-align: left; padding: 0.5rem; border-bottom: 1px solid var(--pf-t--global--border--color--default); }}
    th {{ font-weight: 600; background: var(--pf-t--global--background--color--secondary--default); }}
  </style>
</head>
<body>
  <h1>inspectah Migration Report</h1>
{completeness_banner}
  <p>Source: <strong>{os_escaped}</strong> ({hostname_escaped})</p>

  <div class="summary-grid">
    <div class="summary-card"><h3>Packages Added</h3><div class="value">{pkg_count}</div></div>
    <div class="summary-card"><h3>Config Files</h3><div class="value">{config_count}</div></div>
    <div class="summary-card"><h3>Service Changes</h3><div class="value">{svc_count}</div></div>
    <div class="summary-card"><h3>Storage Entries</h3><div class="value">{storage_count}</div></div>
    <div class="summary-card"><h3>Kernel/Boot Items</h3><div class="value">{kernelboot_count}</div></div>
    <div class="summary-card"><h3>Warnings</h3><div class="value">{warning_count}</div></div>
    <div class="summary-card"><h3>Scheduled Tasks</h3><div class="value">{scheduled_count}</div></div>
    <div class="summary-card"><h3>Security</h3><div class="value">{selinux_mode_escaped}</div></div>
    <div class="summary-card"><h3>Non-RPM Items</h3><div class="value">{nonrpm_count}</div></div>
  </div>

  <h2>Packages</h2>
  <table>
    <thead><tr><th>Name</th><th>Version</th><th>Release</th><th>Arch</th><th>Repo</th></tr></thead>
    <tbody>
{pkg_rows}    </tbody>
  </table>

  <h2>Storage</h2>
  <table>
    <thead><tr><th>Device</th><th>Mount Point</th><th>Type</th><th>Options</th></tr></thead>
    <tbody>
{storage_rows}    </tbody>
  </table>

  <h2>Kernel &amp; Boot</h2>
{kernelboot_cmdline}  <h3>Sysctl Overrides</h3>
  <table>
    <thead><tr><th>Key</th><th>Runtime</th><th>Default</th><th>Source</th></tr></thead>
    <tbody>
{sysctl_rows}    </tbody>
  </table>
  <h3>Module Configurations</h3>
  <ul>
{module_items}  </ul>

  <h2>Warnings</h2>
  <ul>
{warning_items}  </ul>

  <hr>
  <p><em>Generated by inspectah. See <a href="audit-report.md">audit-report.md</a> for full details.</em></p>
</body>
</html>
"#,
        os_escaped = html_escape(&os_name),
        hostname_escaped = html_escape(hostname),
        pkg_count = pkg_count,
        config_count = config_count,
        svc_count = svc_count,
        storage_count = storage_count,
        kernelboot_count = kernelboot_count,
        warning_count = warning_count,
        pkg_rows = pkg_rows,
        storage_rows = storage_rows,
        kernelboot_cmdline = kernelboot_cmdline,
        sysctl_rows = sysctl_rows,
        module_items = module_items,
        warning_items = warning_items,
        scheduled_count = scheduled_count,
        selinux_mode_escaped = html_escape(&selinux_mode),
        nonrpm_count = nonrpm_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::rpm::{PackageEntry, PackageState, RpmSection};

    fn test_snapshot() -> InspectionSnapshot {
        let mut snap = InspectionSnapshot::new();
        snap.rpm = Some(RpmSection {
            packages_added: vec![PackageEntry {
                name: "httpd".into(),
                version: "2.4.57".into(),
                release: "5.el9".into(),
                arch: "x86_64".into(),
                state: PackageState::Added,
                include: true,
                source_repo: "appstream".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        snap
    }

    #[test]
    fn test_report_html_renders() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(
            html.to_lowercase().contains("patternfly"),
            "must reference PatternFly CSS"
        );
    }

    #[test]
    fn test_report_html_escapes_values() {
        let mut snap = test_snapshot();
        snap.rpm.as_mut().unwrap().packages_added[0].name = "<script>alert(1)</script>".into();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("<script>alert"),
            "HTML must escape snapshot values"
        );
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }

    #[test]
    fn test_report_contains_summary_cards() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("Packages Added"));
        assert!(html.contains("Config Files"));
        assert!(html.contains("Service Changes"));
        assert!(html.contains("Warnings"));
    }

    #[test]
    fn test_report_partial_completeness_warning() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "config inspector failed".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("incomplete inspection"),
            "must warn about incomplete inspection"
        );
        assert!(html.contains("Config"), "must name the incomplete section");
    }

    #[test]
    fn test_report_full_completeness_no_warning() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("incomplete inspection"),
            "full snapshot must not show warning"
        );
    }
}
