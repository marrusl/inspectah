//! HTML report renderer — produces a self-contained PatternFly HTML report
//! using minijinja templates.
//!
//! The base template (`templates/report/base.html`) provides the structural
//! shell. Section templates are added incrementally in T5-T12.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use inspectah_core::types::completeness::{Completeness, InspectorId};
use minijinja::{context, Environment, Value};

use super::report_data::{build_filter_data, script_safe_json};

const PF_CSS: &str = include_str!("../../assets/patternfly.min.css");
const REPORT_CSS: &str = include_str!("../../assets/report.css");
const REPORT_JS: &str = include_str!("../../assets/report.js");

/// Render a self-contained PatternFly HTML report from the snapshot.
pub fn render_report(snap: &InspectionSnapshot, _context: &RenderContext) -> String {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/templates"
    )));

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

    let filter_data = build_filter_data(snap);
    let filter_json = serde_json::to_string(&filter_data).unwrap_or_default();
    let safe_json = script_safe_json(&filter_json);

    // Warning count for header badge
    let warning_count = snap.warnings.len();

    // Completeness data for banner
    let (failed_sections, degraded_sections, completeness_reason) = {
        fn inspector_display(id: &InspectorId) -> (&'static str, &'static str) {
            match id {
                InspectorId::Rpm => ("Packages", "packages"),
                InspectorId::Config => ("Configuration Files", "config-files"),
                InspectorId::Services => ("Service State Changes", "services"),
                InspectorId::Storage => ("Storage", "storage"),
                InspectorId::ScheduledTasks => ("Scheduled Tasks", "scheduled-tasks"),
                InspectorId::KernelBoot => ("Kernel & Boot", "kernel-boot"),
                InspectorId::Selinux => ("Security & Access Control", "security"),
                InspectorId::NonRpmSoftware => ("Non-RPM Software", "nonrpm"),
                InspectorId::UsersGroups => ("Users & Groups", "users-groups"),
                InspectorId::Containers => ("Containers", "containers"),
                InspectorId::Network => ("Network", "network"),
                InspectorId::Subscription => ("Subscription", "subscription"),
                InspectorId::Hardware => ("Hardware", "hardware"),
                InspectorId::Ostree => ("OSTree", "ostree"),
                InspectorId::OsRelease => ("OS Release", "os-release"),
            }
        }

        fn to_section_values(ids: &[InspectorId]) -> Vec<Value> {
            ids.iter()
                .map(|id| {
                    let (name, html_id) = inspector_display(id);
                    Value::from_serialize(serde_json::json!({
                        "name": name, "id": html_id
                    }))
                })
                .collect()
        }

        match &snap.completeness {
            Completeness::Complete => (vec![], vec![], String::new()),
            Completeness::Partial {
                degraded_sections: ds,
                reason,
            } => (vec![], to_section_values(ds), reason.clone()),
            Completeness::Incomplete {
                failed_sections: fs,
                degraded_sections: ds,
                reason,
            } => (to_section_values(fs), to_section_values(ds), reason.clone()),
        }
    };

    // System type — use serde name for human-readable display
    let system_type = serde_json::to_string(&snap.system_type)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();

    // Baseline ref from target_image identity (not BaselineData)
    let baseline_ref = snap
        .target_image
        .as_ref()
        .map(|t| t.image_ref.clone())
        .unwrap_or_default();

    // Host count for fleet snapshots
    let host_count = snap
        .fleet_meta
        .as_ref()
        .map(|fm| fm.host_count as i64)
        .unwrap_or(0);

    let tmpl = env
        .get_template("report/base.html")
        .expect("base template must exist at inspectah-pipeline/templates/report/base.html");

    let failed_val = Value::from(failed_sections);
    let degraded_val = Value::from(degraded_sections);

    tmpl.render(context! {
        os_name,
        hostname,
        warning_count,
        failed_sections => failed_val,
        degraded_sections => degraded_val,
        completeness_reason,
        system_type,
        baseline_ref,
        host_count,
        patternfly_css => PF_CSS,
        report_css => REPORT_CSS,
        report_js => REPORT_JS,
        filter_data_json => safe_json,
    })
    .unwrap_or_else(|e| format!("<!-- Template error: {e} -->"))
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
    fn test_report_html_renders_with_doctype() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn test_report_html_contains_csp() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("default-src 'none'"));
    }

    #[test]
    fn test_report_html_no_external_urls() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("http://"),
            "report must not contain http:// URLs"
        );
        assert!(
            !html.contains("https://"),
            "report must not contain https:// URLs"
        );
    }

    #[test]
    fn test_report_html_contains_patternfly() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("--pf-t--global"),
            "must contain PF design tokens"
        );
    }

    #[test]
    fn test_report_html_escapes_values() {
        let mut snap = test_snapshot();
        snap.rpm.as_mut().unwrap().packages_added[0].name = "<script>alert(1)</script>".into();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains("<script>alert"),
            "must escape snapshot values"
        );
    }

    #[test]
    fn test_report_failed_section_shows_in_completeness_banner() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![],
            reason: "permission denied".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element for incomplete snapshot"
        );
        assert!(
            html.contains("Failed:"),
            "banner must label failed sections"
        );
        assert!(
            html.contains("Configuration Files"),
            "banner must name the failed section"
        );
    }

    #[test]
    fn test_report_degraded_section_shows_in_completeness_banner() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Partial {
            degraded_sections: vec![InspectorId::Services],
            reason: "partial timeout".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element for partial snapshot"
        );
        assert!(
            html.contains("Degraded:"),
            "banner must label degraded sections"
        );
        assert!(
            html.contains("Service State Changes"),
            "banner must name the degraded section"
        );
    }

    #[test]
    fn test_report_completeness_banner_shows_reason() {
        use inspectah_core::types::completeness::{Completeness, InspectorId};
        let mut snap = test_snapshot();
        snap.completeness = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Config],
            degraded_sections: vec![InspectorId::Services],
            reason: "permission denied reading shadow file".into(),
        };
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="completeness-banner">"#),
            "must render completeness banner element"
        );
        assert!(
            html.contains("permission denied reading shadow file"),
            "must show reason text"
        );
        assert!(
            html.contains("Configuration Files"),
            "must show failed section name"
        );
        assert!(
            html.contains("Service State Changes"),
            "must show degraded section name"
        );
    }

    #[test]
    fn test_report_complete_has_no_banner() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            !html.contains(r#"<div class="completeness-banner">"#),
            "complete report must not render completeness banner element"
        );
    }

    #[test]
    fn test_report_source_info_bar() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains(r#"<div class="source-info">"#),
            "must render source info bar element"
        );
    }

    #[test]
    fn test_report_header_contains_warning_count() {
        let mut snap = test_snapshot();
        snap.warnings.push(inspectah_core::types::warnings::Warning {
            inspector: "test".into(),
            message: "test warning".into(),
            severity: None,
            extra: Default::default(),
        });
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(
            html.contains("1 warning"),
            "header must show warning count"
        );
    }
}
