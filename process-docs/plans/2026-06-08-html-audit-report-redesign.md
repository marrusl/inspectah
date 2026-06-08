# HTML Audit Report Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the minimal `report.html` with a self-contained, offline-capable HTML audit report at full parity with `audit-report.md`, using minijinja templates and PatternFly 6.

**Architecture:** Server-side rendering via minijinja templates with an embedded minimized DTO for interactive enhancement (filter, TOC). All CSS and JS inlined. Report works offline, supports single-host and fleet snapshots, and degrades gracefully with JS disabled.

**Tech Stack:** Rust, minijinja 2.x, PatternFly 6 CSS (vendored), vanilla JS (~100 lines)

**Spec:** `/Users/mrussell/Work/bootc-migration/inspectah/docs/specs/proposed/2026-06-07-html-audit-report-redesign.md`

**Owner:** Tang (Rust), with Kit assisting on CSS/JS polish. Thorn checkpoints at T5, T10, T14, T17.

---

## File Map

### New Files

| File | Responsibility |
|------|---------------|
| `inspectah-pipeline/src/render/report_data.rs` | Shared computation: section states, counts, badges, conflict tallies |
| `inspectah-pipeline/assets/patternfly.min.css` | Vendored PatternFly 6 CSS (~400KB) |
| `inspectah-pipeline/assets/report.css` | Custom report styles (~100 lines) |
| `inspectah-pipeline/assets/report.js` | Interactive enhancement: filter, TOC, print (~100 lines) |
| `inspectah-pipeline/templates/report/base.html` | Page shell: DOCTYPE, head, inlined assets, body wrapper |
| `inspectah-pipeline/templates/report/section.html` | Reusable section macro (details/summary/badge/state) |
| `inspectah-pipeline/templates/report/header.html` | Dark header bar + warning badge |
| `inspectah-pipeline/templates/report/completeness.html` | Completeness warning banner |
| `inspectah-pipeline/templates/report/source-info.html` | Hostname, OS, baseline info |
| `inspectah-pipeline/templates/report/summary-cards.html` | Summary card grid |
| `inspectah-pipeline/templates/report/toc.html` | TOC bar with anchor links |
| `inspectah-pipeline/templates/report/packages.html` | Packages table + version changes |
| `inspectah-pipeline/templates/report/config.html` | Configuration Files table |
| `inspectah-pipeline/templates/report/services.html` | Service State Changes table |
| `inspectah-pipeline/templates/report/storage.html` | Storage/fstab table |
| `inspectah-pipeline/templates/report/kernel.html` | Kernel & Boot section |
| `inspectah-pipeline/templates/report/scheduled.html` | Scheduled Tasks section |
| `inspectah-pipeline/templates/report/security.html` | Security & Access Control (SELinux) |
| `inspectah-pipeline/templates/report/nonrpm.html` | Non-RPM Software section |
| `inspectah-pipeline/templates/report/users.html` | Users & Groups section |
| `inspectah-pipeline/templates/report/redactions.html` | Redactions (count + pointer) |
| `inspectah-pipeline/templates/report/warnings.html` | Warnings list |
| `inspectah-pipeline/templates/report/fleet-summary.html` | Fleet aggregate summary |
| `inspectah-pipeline/templates/report/incomplete.html` | Incomplete sections |
| `inspectah-pipeline/templates/report/baseline.html` | Baseline comparison |

### Modified Files

| File | Change |
|------|--------|
| `inspectah-pipeline/Cargo.toml` | Add `minijinja` dependency |
| `inspectah-pipeline/src/render/report.rs` | Rewrite: minijinja Environment + DTO builder + script_safe_json |
| `inspectah-pipeline/src/render/mod.rs` | Add `report_data` module, rename output to `audit-report.html` |
| `inspectah-pipeline/src/render/audit.rs` | Migrate counts to `report_data`, add Users & Groups section |
| `inspectah-pipeline/src/render/readme.rs` | Update `report.html` references |
| `inspectah-pipeline/src/render/containerfile.rs` | Update comment reference |
| `inspectah-pipeline/src/render/tarball.rs` | Update test fixture reference |
| `inspectah-pipeline/tests/redaction_2c_surfaces_test.rs` | Update test 11 filename |
| `inspectah-pipeline/src/lib.rs` | Re-export `report_data` if needed |
| `README.md` | Update output tree and references |

---

## Task 1: Add minijinja dependency and create report_data.rs skeleton

**Files:**
- Modify: `inspectah-pipeline/Cargo.toml`
- Create: `inspectah-pipeline/src/render/report_data.rs`
- Modify: `inspectah-pipeline/src/render/mod.rs` (add module declaration)

- [ ] **Step 1: Write the test for SectionState and section_state()**

In `inspectah-pipeline/src/render/report_data.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use inspectah_core::types::completeness::{Completeness, InspectorId};

    #[test]
    fn section_state_normal_when_complete() {
        let c = Completeness::Complete;
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_degraded_when_in_degraded_list() {
        let c = Completeness::Partial {
            degraded_sections: vec![InspectorId::Config],
            reason: "test".into(),
        };
        assert_eq!(section_state(InspectorId::Config, &c), SectionState::Degraded);
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }

    #[test]
    fn section_state_failed_when_in_failed_list() {
        let c = Completeness::Incomplete {
            failed_sections: vec![InspectorId::Storage],
            degraded_sections: vec![],
            reason: "timeout".into(),
        };
        assert_eq!(section_state(InspectorId::Storage, &c), SectionState::Failed);
        assert_eq!(section_state(InspectorId::Rpm, &c), SectionState::Normal);
    }
}
```

- [ ] **Step 2: Implement SectionState enum and section_state()**

```rust
use inspectah_core::types::completeness::{Completeness, InspectorId};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState {
    Normal,
    Degraded,
    Failed,
}

pub fn section_state(id: InspectorId, completeness: &Completeness) -> SectionState {
    match completeness {
        Completeness::Complete => SectionState::Normal,
        Completeness::Partial { degraded_sections, .. } => {
            if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
        Completeness::Incomplete {
            failed_sections,
            degraded_sections,
            ..
        } => {
            if failed_sections.contains(&id) {
                SectionState::Failed
            } else if degraded_sections.contains(&id) {
                SectionState::Degraded
            } else {
                SectionState::Normal
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline report_data -- --nocapture`
Expected: 3 tests pass.

- [ ] **Step 4: Add minijinja to Cargo.toml**

Add to `[dependencies]` in `inspectah-pipeline/Cargo.toml`:

```toml
minijinja = { version = "2", features = ["builtins"] }
```

- [ ] **Step 5: Wire up module in mod.rs**

Add `pub mod report_data;` to `inspectah-pipeline/src/render/mod.rs` alongside the existing module declarations.

- [ ] **Step 6: Verify compilation**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo build -p inspectah-pipeline`
Expected: Compiles with no errors. Existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add inspectah-pipeline/Cargo.toml inspectah-pipeline/src/render/report_data.rs inspectah-pipeline/src/render/mod.rs
git commit -m "feat(report): add minijinja dependency and report_data module

Introduces SectionState enum and section_state() for shared
completeness state computation across HTML and markdown renderers."
```

---

## Task 2: Implement script_safe_json() and ReportFilterData DTO

**Files:**
- Modify: `inspectah-pipeline/src/render/report_data.rs`

- [ ] **Step 1: Write tests for script_safe_json()**

Add to the tests module in `report_data.rs`:

```rust
#[test]
fn script_safe_escapes_less_than() {
    let json = r#"{"name":"</script>"}"#;
    let safe = script_safe_json(json);
    assert!(!safe.contains("</script>"));
    assert!(safe.contains(r"<"));
    // Verify it's still valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&safe).unwrap();
    assert_eq!(parsed["name"].as_str().unwrap(), "</script>");
}

#[test]
fn script_safe_escapes_greater_than() {
    let json = r#"{"v":"a>b"}"#;
    let safe = script_safe_json(json);
    assert!(!safe.contains('>'));
    assert!(safe.contains(r">"));
    let parsed: serde_json::Value = serde_json::from_str(&safe).unwrap();
    assert_eq!(parsed["v"].as_str().unwrap(), "a>b");
}

#[test]
fn script_safe_escapes_html_comment() {
    let json = r#"{"v":"<!--comment-->"}"#;
    let safe = script_safe_json(json);
    assert!(!safe.contains("<!--"));
    let parsed: serde_json::Value = serde_json::from_str(&safe).unwrap();
    assert_eq!(parsed["v"].as_str().unwrap(), "<!--comment-->");
}

#[test]
fn script_safe_preserves_non_special_content() {
    let json = r#"{"name":"httpd","version":"2.4.57"}"#;
    let safe = script_safe_json(json);
    let parsed: serde_json::Value = serde_json::from_str(&safe).unwrap();
    assert_eq!(parsed["name"].as_str().unwrap(), "httpd");
    assert_eq!(parsed["version"].as_str().unwrap(), "2.4.57");
}
```

- [ ] **Step 2: Implement script_safe_json()**

```rust
pub fn script_safe_json(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for ch in json.chars() {
        match ch {
            '<' => out.push_str(r"<"),
            '>' => out.push_str(r">"),
            '\u{2028}' => out.push_str(r" "),
            '\u{2029}' => out.push_str(r" "),
            _ => out.push(ch),
        }
    }
    out
}
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline report_data -- --nocapture`
Expected: 7 tests pass (3 from T1 + 4 new).

- [ ] **Step 4: Define ReportFilterData structs**

Add to `report_data.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ReportFilterData {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<FilterablePackage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_files: Vec<FilterableConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<FilterableService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scheduled: Vec<FilterableScheduled>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<FilterableUser>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterablePackage {
    pub name: String,
    pub version: String,
    pub release: String,
    pub arch: String,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableConfig {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableService {
    pub unit: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableScheduled {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterableUser {
    pub name: String,
    pub uid: u64,
}
```

- [ ] **Step 5: Implement build_filter_data()**

```rust
use inspectah_core::snapshot::InspectionSnapshot;
use inspectah_core::types::config::ConfigFileKind;
use inspectah_core::types::users::UserGroupDecision;

pub fn build_filter_data(snap: &InspectionSnapshot) -> ReportFilterData {
    let packages = snap.rpm.as_ref().map(|rpm| {
        rpm.packages_added.iter()
            .filter(|p| p.include)
            .map(|p| FilterablePackage {
                name: p.name.clone(),
                version: p.version.clone(),
                release: p.release.clone(),
                arch: p.arch.clone(),
                repo: p.source_repo.clone(),
            })
            .collect()
    }).unwrap_or_default();

    let config_files = snap.config.as_ref().map(|c| {
        c.files.iter()
            .filter(|f| f.include)
            .map(|f| FilterableConfig {
                path: f.path.clone(),
                kind: serde_json::to_string(&f.kind)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string(),
            })
            .collect()
    }).unwrap_or_default();

    let services = snap.services.as_ref().map(|s| {
        s.state_changes.iter()
            .map(|sc| FilterableService {
                unit: sc.unit.clone(),
                state: serde_json::to_string(&sc.current_state)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string(),
            })
            .collect()
    }).unwrap_or_default();

    let scheduled = snap.scheduled_tasks.as_ref().map(|st| {
        let mut items = Vec::new();
        for cj in &st.cron_jobs {
            items.push(FilterableScheduled {
                name: cj.command.clone(),
                kind: "cron".into(),
            });
        }
        for t in &st.systemd_timers {
            items.push(FilterableScheduled {
                name: t.unit.clone(),
                kind: "timer".into(),
            });
        }
        for t in &st.generated_timer_units {
            items.push(FilterableScheduled {
                name: t.unit.clone(),
                kind: "generated_timer".into(),
            });
        }
        for a in &st.at_jobs {
            items.push(FilterableScheduled {
                name: format!("at job #{}", a.id),
                kind: "at".into(),
            });
        }
        items
    }).unwrap_or_default();

    let users = snap.users_groups.as_ref().map(|ug| {
        ug.users.iter()
            .filter_map(|v| serde_json::from_value::<UserGroupDecision>(v.clone()).ok())
            .filter(|u| u.include)
            .map(|u| FilterableUser {
                name: u.name.clone(),
                uid: u.uid,
            })
            .collect()
    }).unwrap_or_default();

    ReportFilterData { packages, config_files, services, scheduled, users }
}
```

- [ ] **Step 6: Write test for DTO minimization (no secrets in DTO)**

```rust
#[test]
fn filter_data_excludes_password_hash() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "testuser",
            "uid": 1000,
            "gid": 1000,
            "shell": "/bin/bash",
            "home": "/home/testuser",
            "include": true,
            "classification": "interactive",
            "containerfile_strategy": "useradd",
            "password_choice": "preserve",
            "password_hash": "$6$rounds=5000$secret_hash",
            "ssh_keys": ["ssh-ed25519 AAAA_secret_key"]
        })],
        ..Default::default()
    });
    let dto = build_filter_data(&snap);
    let json = serde_json::to_string(&dto).unwrap();
    assert!(!json.contains("secret_hash"), "password_hash must not appear in DTO");
    assert!(!json.contains("secret_key"), "ssh_keys must not appear in DTO");
    assert!(json.contains("testuser"), "name should be in DTO");
}
```

- [ ] **Step 7: Run all tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline report_data -- --nocapture`
Expected: 8 tests pass.

- [ ] **Step 8: Commit**

```bash
git add inspectah-pipeline/src/render/report_data.rs
git commit -m "feat(report): add script_safe_json and ReportFilterData DTO

Script-safe JSON serialization uses </> unicode escapes
for safe embedding in <script> blocks. ReportFilterData carries
only filterable display-safe fields — no secrets, no redaction data."
```

---

## Task 3: Vendor PatternFly CSS and create custom assets

**Files:**
- Create: `inspectah-pipeline/assets/patternfly.min.css`
- Create: `inspectah-pipeline/assets/report.css`
- Create: `inspectah-pipeline/assets/report.js`

- [ ] **Step 1: Download and vendor PatternFly 6 minified CSS**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
mkdir -p inspectah-pipeline/assets
curl -sL 'https://unpkg.com/@patternfly/patternfly@6/patternfly.min.css' \
  -o inspectah-pipeline/assets/patternfly.min.css
wc -c inspectah-pipeline/assets/patternfly.min.css
```

Expected: File exists, ~400KB.

- [ ] **Step 2: Create report.css with custom styles**

Write `inspectah-pipeline/assets/report.css` with PF-token-based custom styles for report-section, report-header, report-toc, summary cards, badges (normal, degraded, failed), filter controls, print media query, and responsive breakpoints. All colors/spacing reference PF custom properties (`--pf-t--global--*`). See spec sections "PatternFly Usage" and "Responsive Behavior" for the token and breakpoint contracts.

- [ ] **Step 3: Create report.js with interactive enhancement**

Write `inspectah-pipeline/assets/report.js` with three features:
1. **Table filtering** (~30 lines): On `input` events on `.report-filter input`, read the corresponding section from `#report-filter-data` JSON, filter rows, update "Showing X of Y" count, show "No matching items" with `aria-live="polite"` on zero matches.
2. **TOC navigation** (~10 lines): On `hashchange` and on initial `DOMContentLoaded` (if URL has hash), find target `<details>`, open it, scroll to it, move focus to `<summary>`.
3. **Print support** (~10 lines): On `beforeprint`, save all `<details>` open states, open all. On `afterprint`, restore.

- [ ] **Step 4: Verify assets exist and are non-empty**

```bash
ls -lh inspectah-pipeline/assets/
```

Expected: `patternfly.min.css` (~400KB), `report.css` (~3-5KB), `report.js` (~3-5KB).

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/assets/
git commit -m "feat(report): vendor PatternFly 6 CSS and create report assets

Vendored patternfly.min.css for offline-capable HTML reports.
Custom report.css uses PF design tokens. report.js provides
progressive-enhancement filter, TOC nav, and print support."
```

---

## Task 4: Create base template and minijinja Environment

**Files:**
- Create: `inspectah-pipeline/templates/report/base.html`
- Modify: `inspectah-pipeline/src/render/report.rs`

- [ ] **Step 1: Write test for minijinja environment initialization**

In `report.rs`, replace the existing test module with:

```rust
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
        assert!(!html.contains("http://"), "report must not contain http:// URLs");
        assert!(!html.contains("https://"), "report must not contain https:// URLs");
    }

    #[test]
    fn test_report_html_contains_patternfly() {
        let snap = test_snapshot();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(html.contains("--pf-t--global"), "must contain PF design tokens");
    }

    #[test]
    fn test_report_html_escapes_values() {
        let mut snap = test_snapshot();
        snap.rpm.as_mut().unwrap().packages_added[0].name = "<script>alert(1)</script>".into();
        let html = render_report(&snap, &RenderContext { target: None });
        assert!(!html.contains("<script>alert"), "must escape snapshot values");
    }
}
```

- [ ] **Step 2: Create base.html template**

Write `inspectah-pipeline/templates/report/base.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta http-equiv="Content-Security-Policy"
        content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
  <title>inspectah Audit Report — {{ os_name }}</title>
  <style>{{ patternfly_css }}</style>
  <style>{{ report_css }}</style>
</head>
<body>
  {% include "report/header.html" %}
  {% if completeness_banner %}
    {% include "report/completeness.html" %}
  {% endif %}
  {% include "report/source-info.html" %}
  {% include "report/summary-cards.html" %}
  {% include "report/toc.html" %}

  {% for section in sections %}
    {% include "report/" ~ section.template %}
  {% endfor %}

  <footer class="report-footer">
    <p>Generated by inspectah v{{ version }}.
    See <a href="audit-report.md">audit-report.md</a> for the full report in Markdown format.</p>
  </footer>

  <script type="application/json" id="report-filter-data">{{ filter_data_json|safe }}</script>
  <script>{{ report_js }}</script>
</body>
</html>
```

Note: The exact template structure will evolve as section templates are added. The key contract is: PF CSS inlined in `<style>`, CSP meta tag present, no external URLs, DTO in `<script type="application/json">` with `|safe`, report JS at end.

- [ ] **Step 3: Rewrite render_report() to use minijinja**

Replace the `render_report()` function body in `report.rs` with minijinja Environment setup:

```rust
use minijinja::{Environment, context};
use super::report_data::{self, SectionState, build_filter_data, script_safe_json};

const PF_CSS: &str = include_str!("../../assets/patternfly.min.css");
const REPORT_CSS: &str = include_str!("../../assets/report.css");
const REPORT_JS: &str = include_str!("../../assets/report.js");

pub fn render_report(snap: &InspectionSnapshot, _context: &RenderContext) -> String {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("inspectah-pipeline/templates"));
    // For release builds, use embedded templates instead:
    // Templates will be embedded via include_str!() in a later step.

    let os_name = snap.os_release.as_ref()
        .map(|o| if o.pretty_name.is_empty() { o.name.clone() } else { o.pretty_name.clone() })
        .unwrap_or_else(|| "Unknown System".into());

    let hostname = snap.meta.get("hostname")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let filter_data = build_filter_data(snap);
    let filter_json = serde_json::to_string(&filter_data).unwrap_or_default();
    let safe_json = script_safe_json(&filter_json);

    let tmpl = env.get_template("report/base.html").expect("base template");
    tmpl.render(context! {
        os_name,
        hostname,
        patternfly_css => PF_CSS,
        report_css => REPORT_CSS,
        report_js => REPORT_JS,
        filter_data_json => safe_json,
        // Section-specific context will be added in subsequent tasks
    }).unwrap_or_else(|e| format!("<!-- Template error: {e} -->"))
}
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline report::tests -- --nocapture`
Expected: All 5 tests pass. The template renders a valid HTML document with inlined PF CSS, CSP header, no external URLs.

- [ ] **Step 5: Commit**

```bash
git add inspectah-pipeline/templates/ inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): wire up minijinja environment with base template

Replaces format!() HTML generation with minijinja templates.
Base template inlines PF CSS, custom CSS, report JS, and CSP header.
All external URL references removed — report is fully self-contained."
```

---

## **THORN CHECKPOINT T5**

Verify: minijinja compiles, report_data tests pass, script_safe_json is correct, DTO excludes secrets, base template renders valid offline HTML. Run `cargo clippy -p inspectah-pipeline -- -D warnings`.

---

## Task 5: Create section macro and header/completeness/source-info templates

**Files:**
- Create: `inspectah-pipeline/templates/report/section.html`
- Create: `inspectah-pipeline/templates/report/header.html`
- Create: `inspectah-pipeline/templates/report/completeness.html`
- Create: `inspectah-pipeline/templates/report/source-info.html`

- [ ] **Step 1: Create section.html macro**

The reusable section macro handles all five states from the spec matrix. See spec "Section Template Macro" for the exact template. The macro takes: `id`, `title`, `count`, `state` ("normal"/"degraded"/"failed"), `conflict_count`, `extra_badge`. Failed sections show "data unavailable" with failed body text. Degraded sections show "partial data" pill. Empty sections show "(0)" in grayed text.

- [ ] **Step 2: Create header.html**

Dark header bar with "inspectah Migration Audit Report", generation timestamp, and a persistent warning badge showing warning count (if > 0). Uses `.report-header` class.

- [ ] **Step 3: Create completeness.html**

Conditional yellow banner listing failed and degraded sections by name (as anchor links to their `<details id="...">` sections) with the global reason string. Follows the spec's Completeness Banner contract — no per-section reasons.

- [ ] **Step 4: Create source-info.html**

Hostname (large), OS pretty name + arch + SELinux mode, baseline image ref + digest (conditional). For fleet snapshots, includes fleet aggregate summary grouped visually with source info.

- [ ] **Step 5: Write test for failed section rendering**

```rust
#[test]
fn test_report_failed_section_shows_data_unavailable() {
    use inspectah_core::types::completeness::{Completeness, InspectorId};
    let mut snap = test_snapshot();
    snap.completeness = Completeness::Incomplete {
        failed_sections: vec![InspectorId::Config],
        degraded_sections: vec![],
        reason: "permission denied".into(),
    };
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("data unavailable"), "failed section must show data unavailable");
    assert!(html.contains("Data collection failed"), "failed section body text");
}
```

- [ ] **Step 6: Write test for degraded section rendering**

```rust
#[test]
fn test_report_degraded_section_shows_partial_data() {
    use inspectah_core::types::completeness::{Completeness, InspectorId};
    let mut snap = test_snapshot();
    snap.completeness = Completeness::Partial {
        degraded_sections: vec![InspectorId::Services],
        reason: "partial timeout".into(),
    };
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("partial data"), "degraded section must show partial data pill");
}
```

- [ ] **Step 7: Run tests, commit**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline report -- --nocapture`

```bash
git add inspectah-pipeline/templates/report/
git commit -m "feat(report): add section macro, header, completeness, and source-info templates

Section macro handles all five states: normal, empty, degraded, failed, absent.
Header bar has persistent warning badge. Completeness banner links to sections."
```

---

## Task 6: Create summary cards and TOC templates

**Files:**
- Create: `inspectah-pipeline/templates/report/summary-cards.html`
- Create: `inspectah-pipeline/templates/report/toc.html`

- [ ] **Step 1: Create summary-cards.html**

Responsive grid using `<dl>`/`<dt>`/`<dd>` markup. Shows cards for all always-rendered sections (including empty ones with "0") and present conditional sections. Warnings excluded (header badge instead). Failed sections show "n/a". Uses `.report-cards` class with PF spacing tokens.

- [ ] **Step 2: Create toc.html**

Gray bar with anchor links. Each link shows section name and count. Warning count in red. Failed sections show "failed" indicator. Empty sections are anchor links (they scroll to rendered sections). Absent sections omitted. Uses `flex-wrap: wrap` for overflow, hidden at <600px.

- [ ] **Step 3: Write test for warnings excluded from summary cards**

```rust
#[test]
fn test_report_warnings_not_in_summary_cards() {
    let mut snap = test_snapshot();
    snap.warnings = vec![inspectah_core::types::warning::Warning {
        message: "test warning".into(),
        ..Default::default()
    }];
    let html = render_report(&snap, &RenderContext { target: None });
    // Warnings should be in header badge, not in summary card grid
    assert!(html.contains("report-header") && html.contains("1")); // header badge
    // The summary cards section should not have a Warnings card
    // (exact assertion depends on template structure)
}
```

- [ ] **Step 4: Run tests, commit**

```bash
git add inspectah-pipeline/templates/report/summary-cards.html inspectah-pipeline/templates/report/toc.html
git commit -m "feat(report): add summary cards and TOC bar templates

Summary cards use semantic dl/dt/dd markup. Warnings excluded from cards,
shown in header badge. TOC bar with anchor links, flex-wrap for overflow."
```

---

## Task 7: Packages and Baseline Comparison section templates

**Files:**
- Create: `inspectah-pipeline/templates/report/packages.html`
- Create: `inspectah-pipeline/templates/report/baseline.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add section context)

- [ ] **Step 1: Create packages.html template**

Uses the section macro. Shows added packages in a table (Name, Version, Release, Arch, Repo). Includes filter `<input>` with visible `<label>` when 10+ rows. Includes version changes sub-table when baseline data exists. Badge format: "(N)" + version change count if applicable.

- [ ] **Step 2: Create baseline.html template**

Conditional section showing target image ref, digest, and resolution strategy. Not collapsible (it's context, not a findings table).

- [ ] **Step 3: Wire packages context into render_report()**

Add packages data to the minijinja context: package list, count, section state, version changes.

- [ ] **Step 4: Write test for packages section rendering**

```rust
#[test]
fn test_report_contains_packages_section() {
    let snap = test_snapshot();
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("Packages"));
    assert!(html.contains("httpd"));
    assert!(html.contains("2.4.57"));
}

#[test]
fn test_report_empty_packages_shows_zero() {
    let mut snap = InspectionSnapshot::new();
    snap.rpm = Some(inspectah_core::types::rpm::RpmSection::default());
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("(0)"));
    assert!(html.contains("No items detected"));
}
```

- [ ] **Step 5: Run tests, commit**

```bash
git add inspectah-pipeline/templates/report/packages.html inspectah-pipeline/templates/report/baseline.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add packages and baseline comparison templates

Packages section with filterable table and version changes sub-table.
Empty packages section renders with (0) badge and 'No items detected'."
```

---

## Task 8: Configuration Files and Service State Changes templates

**Files:**
- Create: `inspectah-pipeline/templates/report/config.html`
- Create: `inspectah-pipeline/templates/report/services.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add section context)

- [ ] **Step 1: Create config.html template**

Section macro with fleet conflict badge support. Shows modified RPM-owned files and unowned files. Filter input on 10+ rows. Badge: "(N, K conflicts)" for fleet.

- [ ] **Step 2: Create services.html template**

Section macro with fleet variant support. Table: Unit, Current, Default, Action. Filter input on 10+ rows. Badge: "(N enabled, M masked)".

- [ ] **Step 3: Wire context into render_report()**

Add config and services data to context with counts, states, fleet conflict tallies.

- [ ] **Step 4: Write tests**

Test that config section renders, services section shows state breakdown, fleet conflict badge appears for fleet snapshots.

- [ ] **Step 5: Run tests, commit**

```bash
git add inspectah-pipeline/templates/report/config.html inspectah-pipeline/templates/report/services.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add configuration files and service state changes templates

Config section with fleet conflict badges. Services with enabled/masked
breakdown. Both filterable at 10+ rows."
```

---

## Task 9: Storage, Kernel & Boot, Security & Access Control templates

**Files:**
- Create: `inspectah-pipeline/templates/report/storage.html`
- Create: `inspectah-pipeline/templates/report/kernel.html`
- Create: `inspectah-pipeline/templates/report/security.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add section context)

- [ ] **Step 1: Create storage.html**

Conditional section. Fstab entries table: Device, Mount Point, Type, Options. Not filterable. Badge: "(N entries)".

- [ ] **Step 2: Create kernel.html**

Conditional section. Shows kernel command line, sysctl overrides table, module configurations list. Badge: "(N items)".

- [ ] **Step 3: Create security.html**

Conditional section (SELinux). Shows mode, custom modules, boolean overrides, fcontext rules, FIPS mode. Badge: "(mode)".

- [ ] **Step 4: Wire context, write tests, commit**

Verify conditional sections omit when data source absent. Verify failed-conditional section renders as "data unavailable" (proof #16 from spec).

```bash
git add inspectah-pipeline/templates/report/storage.html inspectah-pipeline/templates/report/kernel.html inspectah-pipeline/templates/report/security.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add storage, kernel/boot, and security templates

All three are conditional sections. Failed-conditional renders as
'data unavailable' per precedence rule."
```

---

## **THORN CHECKPOINT T10**

Verify: 7 of 14 sections rendering correctly. Section states (normal, empty, degraded, failed, absent) all tested. Fleet conflict badges working. Filter inputs appearing on 10+ row tables. Run full test suite.

---

## Task 10: Scheduled Tasks, Non-RPM Software, Warnings, Redactions templates

**Files:**
- Create: `inspectah-pipeline/templates/report/scheduled.html`
- Create: `inspectah-pipeline/templates/report/nonrpm.html`
- Create: `inspectah-pipeline/templates/report/warnings.html`
- Create: `inspectah-pipeline/templates/report/redactions.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add section context)

- [ ] **Step 1: Create scheduled.html**

Shows cron jobs, systemd timers, generated timer units, at jobs. Filterable at 10+ rows. Badge: "(N cron, M timers)". Includes @reboot warning if applicable.

- [ ] **Step 2: Create nonrpm.html**

Items grouped by method with counts. Env file warning if applicable. Not filterable. Badge: "(N)".

- [ ] **Step 3: Create warnings.html**

Warning list with red left border accent. Not filterable. Badge: "(N)". Excluded from summary cards (enforced in summary-cards.html).

- [ ] **Step 4: Create redactions.html**

Count + pointer to secrets-review.md. Monospace, muted color. No structured findings. Body: "N item(s) redacted. See secrets-review.md for details."

- [ ] **Step 5: Wire context, write tests, commit**

```bash
git add inspectah-pipeline/templates/report/scheduled.html inspectah-pipeline/templates/report/nonrpm.html inspectah-pipeline/templates/report/warnings.html inspectah-pipeline/templates/report/redactions.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add scheduled tasks, non-RPM, warnings, redactions templates

Redactions section shows count + pointer only — no structured findings.
Warnings section has red accent, excluded from summary cards."
```

---

## Task 11: Users & Groups template and safe-field whitelist

**Files:**
- Create: `inspectah-pipeline/templates/report/users.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add section context)

- [ ] **Step 1: Create users.html template**

Table showing only whitelisted fields: name, uid, gid, shell, home, classification, has_sudo, ssh_key_count, supplementary_groups, password_status. Filterable at 10+ rows. Badge: "(N)".

- [ ] **Step 2: Wire context with safe-field projection**

In `report.rs`, deserialize each `serde_json::Value` in `snap.users_groups.users` into `UserGroupDecision` and pass only the whitelisted fields to the template context. Never pass `password_hash`, `ssh_keys`, or raw section fields.

- [ ] **Step 3: Write whitelist enforcement test**

```rust
#[test]
fn test_report_users_excludes_password_hash() {
    let mut snap = InspectionSnapshot::new();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice",
            "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "include": true,
            "classification": "interactive",
            "containerfile_strategy": "useradd",
            "password_choice": "preserve",
            "password_hash": "$6$secret_hash_value",
            "ssh_keys": ["ssh-ed25519 AAAA_secret_key_content"]
        })],
        ..Default::default()
    });
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("alice"), "user name should appear");
    assert!(!html.contains("secret_hash_value"), "password_hash must not appear");
    assert!(!html.contains("secret_key_content"), "ssh_keys must not appear");
}
```

- [ ] **Step 4: Run tests, commit**

```bash
git add inspectah-pipeline/templates/report/users.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add users and groups template with safe-field whitelist

Only whitelisted fields rendered: name, uid, gid, shell, home,
classification, has_sudo, ssh_key_count, groups, password_status.
password_hash and ssh_keys content are excluded."
```

---

## Task 12: Fleet Aggregate Summary and Incomplete Sections templates

**Files:**
- Create: `inspectah-pipeline/templates/report/fleet-summary.html`
- Create: `inspectah-pipeline/templates/report/incomplete.html`
- Modify: `inspectah-pipeline/src/render/report.rs` (add fleet/completeness context)

- [ ] **Step 1: Create fleet-summary.html**

Conditional on `fleet_meta`. Shows label, host count, hostnames list, section coverage table, baseline status (unanimous/provisional), variant conflict count. Grouped visually with source info.

- [ ] **Step 2: Create incomplete.html**

Conditional on failed/degraded sections existing. Shows "Failed (no data collected)" and "Degraded (partial data collected)" sub-lists with section names as anchor links. Global reason string.

- [ ] **Step 3: Write fleet rendering test**

```rust
#[test]
fn test_report_fleet_summary_rendered() {
    use inspectah_core::types::fleet::FleetSnapshotMeta;
    use std::collections::BTreeMap;
    let mut snap = test_snapshot();
    snap.fleet_meta = Some(FleetSnapshotMeta {
        label: "web-servers".into(),
        host_count: 3,
        hostnames: vec!["host1".into(), "host2".into(), "host3".into()],
        merged_at: "2026-06-08T12:00:00Z".into(),
        baseline_provisional: false,
        section_host_counts: BTreeMap::from([("rpm".into(), 3)]),
    });
    let html = render_report(&snap, &RenderContext { target: None });
    assert!(html.contains("Fleet Aggregate Summary"));
    assert!(html.contains("web-servers"));
    assert!(html.contains("3")); // host count
}
```

- [ ] **Step 4: Run tests, commit**

```bash
git add inspectah-pipeline/templates/report/fleet-summary.html inspectah-pipeline/templates/report/incomplete.html inspectah-pipeline/src/render/report.rs
git commit -m "feat(report): add fleet aggregate summary and incomplete sections templates

Fleet summary conditional on fleet_meta. Incomplete sections list
failed/degraded with anchor links to affected sections."
```

---

## Task 13: Section parity test and structure snapshot

**Files:**
- Modify: `inspectah-pipeline/src/render/report.rs` (add parity test)

- [ ] **Step 1: Write the parity mapping test (proof #1)**

```rust
#[test]
fn test_section_parity_with_audit_report() {
    // Build a fully-populated snapshot with all section data sources present
    let snap = fully_populated_snapshot(); // helper function with all fields set

    let md = super::audit::render_audit(&snap);
    let html = render_report(&snap, &RenderContext { target: None });

    // Extract markdown headings
    let md_headings: Vec<&str> = md.lines()
        .filter(|l| l.starts_with("## "))
        .map(|l| l.trim_start_matches("## "))
        .collect();

    // Extract HTML section IDs
    let html_ids: Vec<String> = {
        let re = regex::Regex::new(r#"<details id="([^"]+)">"#).unwrap();
        re.captures_iter(&html)
            .map(|c| c[1].to_string())
            .collect()
    };

    // Verify each markdown heading maps to an HTML section per parity table
    let expected_mappings = vec![
        ("Fleet Aggregate Summary", "fleet-summary"),
        ("Incomplete Sections", "incomplete-sections"),
        ("Packages", "packages"),
        ("Configuration Files", "config-files"),
        ("Service State Changes", "service-changes"),
        ("Storage", "storage"),
        ("Kernel & Boot", "kernel-boot"),
        ("Scheduled Tasks", "scheduled-tasks"),
        ("Security & Access Control", "security"),
        ("Non-RPM Software", "nonrpm"),
        ("Redactions", "redactions"),
        ("Warnings", "warnings"),
        ("Users & Groups", "users-groups"),
    ];

    for (md_heading, html_id) in &expected_mappings {
        assert!(md_headings.contains(md_heading),
            "markdown missing section: {md_heading}");
        assert!(html_ids.contains(&html_id.to_string()),
            "HTML missing section: {html_id}");
    }
}
```

- [ ] **Step 2: Write the structure snapshot test (proof #11)**

Use `insta` to snapshot the HTML body with PF CSS and DTO replaced by placeholders.

- [ ] **Step 3: Run tests, commit**

```bash
git add inspectah-pipeline/src/render/report.rs
git commit -m "test(report): add section parity test and structure snapshot

Parity test verifies 1:1 mapping between markdown headings and HTML
section IDs. Structure snapshot excludes vendored CSS and DTO JSON."
```

---

## **THORN CHECKPOINT T14**

All 14 sections rendering. Parity test passes. Structure snapshot captured. Run full `cargo test` + `cargo clippy -p inspectah-pipeline -- -D warnings`.

---

## Task 14: Add Users & Groups section to markdown audit renderer

**Files:**
- Modify: `inspectah-pipeline/src/render/audit.rs`
- Modify: `inspectah-pipeline/src/render/report_data.rs` (add shared user count helper)

- [ ] **Step 1: Write test for Users & Groups in markdown**

```rust
#[test]
fn test_audit_users_groups_section() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "include": true, "classification": "interactive",
            "containerfile_strategy": "useradd",
            "password_choice": "none"
        })],
        ..Default::default()
    });
    let md = render_audit(&snap);
    assert!(md.contains("## Users & Groups"));
    assert!(md.contains("alice"));
}

#[test]
fn test_audit_users_groups_excludes_password_hash() {
    let mut snap = test_snapshot();
    snap.users_groups = Some(inspectah_core::types::users::UserGroupSection {
        users: vec![serde_json::json!({
            "name": "alice", "uid": 1000, "gid": 1000,
            "shell": "/bin/bash", "home": "/home/alice",
            "include": true, "classification": "interactive",
            "containerfile_strategy": "useradd",
            "password_choice": "preserve",
            "password_hash": "$6$secret_hash"
        })],
        ..Default::default()
    });
    let md = render_audit(&snap);
    assert!(!md.contains("secret_hash"), "password_hash must not appear in audit");
}
```

- [ ] **Step 2: Add Users & Groups section to render_audit()**

Add after the Non-RPM Software section and before Redactions, using the same safe-field whitelist as the HTML renderer. Show a table: Name, UID, Shell, Classification, Sudo, SSH Keys (count only).

- [ ] **Step 3: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline`
Expected: All tests pass, including the parity test from T13 (which now expects Users & Groups in both).

- [ ] **Step 4: Commit**

```bash
git add inspectah-pipeline/src/render/audit.rs inspectah-pipeline/src/render/report_data.rs
git commit -m "feat(audit): add Users & Groups section to markdown renderer

Same safe-field whitelist as HTML: name, uid, shell, classification,
has_sudo, ssh_key_count. password_hash and ssh_keys excluded."
```

---

## Task 15: Execute rename sweep

**Files:** See spec Rename Sweep table for full list.

- [ ] **Step 1: Update render/mod.rs**

Change `output_dir.join("report.html")` to `output_dir.join("audit-report.html")`. Update doc comments and test assertions.

- [ ] **Step 2: Update readme.rs**

Change artifact table entry and footer link from `report.html` to `audit-report.html`. Update test assertion.

- [ ] **Step 3: Update containerfile.rs**

Change comment reference from `report.html` to `audit-report.html`.

- [ ] **Step 4: Update tarball.rs**

Change test fixture reference from `report.html` to `audit-report.html`.

- [ ] **Step 5: Update redaction_2c_surfaces_test.rs**

Change test 11 to reference `audit-report.html`.

- [ ] **Step 6: Verify no remaining references**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
grep -rn 'report\.html' inspectah-pipeline/ inspectah-cli/
```

Expected: Zero hits.

- [ ] **Step 7: Run full test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && cargo test -p inspectah-pipeline`
Expected: All tests pass. `render_all()` produces `audit-report.html`, not `report.html`.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(report): rename report.html to audit-report.html

Output contract change. All code references updated:
mod.rs, readme.rs, containerfile.rs, tarball.rs, redaction tests.
grep confirms zero remaining report.html references."
```

---

## Task 16: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/getting-started.md`
- Modify: `docs/explanation/architecture.md`
- Modify: `docs/reference/output-artifacts.md` (if exists)
- Modify: `docs/tutorials/first-migration.md`
- Modify: `docs/how-to/review-and-refine.md` (if references report.html)

- [ ] **Step 1: Update README.md**

Change "interactive HTML dashboard" to "HTML audit report". Update the output tree to show `audit-report.html` instead of `report.html`. Update the description comment.

- [ ] **Step 2: Update docs/**

Search all docs files for `report.html`, "HTML dashboard", "interactive dashboard" and replace with correct terminology and filename.

```bash
grep -rn 'report\.html\|HTML.*dashboard\|interactive.*dashboard' docs/
```

- [ ] **Step 3: Verify no stale references**

```bash
grep -rn 'report\.html' README.md docs/
```

Expected: Zero hits.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/
git commit -m "docs: update report.html references to audit-report.html

Renamed from 'interactive HTML dashboard' to 'HTML audit report'
across README and all documentation files."
```

---

## **THORN CHECKPOINT T17**

Final review. All 14 sections at parity. Rename complete. Documentation updated. Run full test suite + clippy. Verify proof matrix items:
- Proof #1: Section parity test passes
- Proof #5: Failed vs empty distinction
- Proof #6: Script-safe JSON
- Proof #7: Redaction surface (test 11 with new filename)
- Proof #8: DTO minimization
- Proof #9: Offline / no-CDN
- Proof #10: Filename rename
- Proof #15: Users & Groups safe-field whitelist
- Proof #16: Failed conditional section

---

## Task 17: Final verification pass

**Files:** No new files. Test-only.

- [ ] **Step 1: Run the complete proof matrix**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
cargo test -p inspectah-pipeline -- --nocapture 2>&1 | tail -20
cargo clippy -p inspectah-pipeline -- -D warnings
```

- [ ] **Step 2: Verify offline contract manually**

```bash
# Render a test report and check for external URLs
cargo test -p inspectah-pipeline test_report_html_no_external_urls -- --nocapture
```

- [ ] **Step 3: Verify redaction surface**

```bash
cargo test -p inspectah-pipeline redaction_2c -- --nocapture
```

- [ ] **Step 4: Verify rename is clean**

```bash
grep -rn 'report\.html' inspectah-pipeline/ inspectah-cli/ README.md docs/
```

Expected: Zero hits.

- [ ] **Step 5: Final commit if any test fixes were needed**

```bash
git add -A
git commit -m "test(report): final verification pass — all proof matrix items green"
```
