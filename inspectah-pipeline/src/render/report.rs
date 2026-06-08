//! HTML report renderer — produces a self-contained PatternFly HTML report
//! using minijinja templates.
//!
//! The base template (`templates/report/base.html`) provides the structural
//! shell. Section templates are added incrementally in T5-T12.

use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::traits::renderer::RenderContext;
use minijinja::{context, Environment};

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

    let tmpl = env
        .get_template("report/base.html")
        .expect("base template must exist at inspectah-pipeline/templates/report/base.html");

    tmpl.render(context! {
        os_name,
        hostname,
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
}
