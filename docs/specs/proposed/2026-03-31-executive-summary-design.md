# Executive Summary Renderer

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Coral (via brainstorm with Mark)

## Goal

Generate a plain-English, half-page migration summary alongside the technical artifacts. Written for the sysadmin who needs to justify the migration to stakeholders. Available as both a standalone markdown file and embedded in the HTML report.

## Context

yoinkc produces technical artifacts (Containerfile, audit report, secrets review) but nothing that a non-technical stakeholder can read. The sysadmin has to manually translate findings into a summary for their manager. This renderer automates that translation.

## Design

### Output Format

A structured half-page summary with four sections:

**1. System Overview** (2-3 sentences)
- Host identity (hostname, OS, architecture)
- Fleet context if applicable (N hosts, role)
- Base image target

**2. Migration Scope** (3-4 bullets)
- Total packages to install (count of included RPMs)
- Config files to carry (count, with variant callout if fleet mode)
- Services to enable/disable (count)
- Other artifacts (quadlets, scheduled tasks, non-RPM software — one line each if present, omitted if empty)

**3. Attention Items** (2-3 bullets, omitted if none)
- Unresolved variant ties (count, which files)
- FIXME items requiring manual review (count)
- Secrets flagged for remediation (count)
- Any items the tool couldn't handle automatically

**4. Assessment** (1-2 sentences)
- Migration complexity: Low / Medium / High (based on attention item count and triage ratios)
- One-line recommendation: "Ready to build" / "N items need resolution first" / "Significant manual work required"

### Complexity Scoring

Simple heuristic, not a sophisticated model:
- **Low:** 0 attention items, >80% automatic triage
- **Medium:** 1-5 attention items, or <80% automatic triage
- **High:** >5 attention items, or any redacted secrets, or >20% manual triage

### Dual Render Targets

**1. Standalone file:** `executive-summary.md` in the output tarball. Rendered by a new renderer function in `src/yoinkc/renderers/`. Uses the same Jinja2 environment as other renderers.

**2. HTML report embed:** A collapsible section at the top of the summary tab in the HTML report, above the dashboard cards. Collapsed by default — click to expand. Uses the same data, rendered as HTML rather than markdown.

### Data Sources

All data comes from the existing `_build_context()` output — no new inspection or computation needed:
- `os_desc`, `meta` — system overview
- `counts` — migration scope numbers
- `triage` — automatic/fixme/manual ratios
- `unresolved_ties` — variant tie count
- `fleet_meta` — fleet context

## Scope

**In scope:**
- New renderer: `src/yoinkc/renderers/executive_summary.py`
- New Jinja2 template: `src/yoinkc/templates/executive-summary.md.j2`
- HTML embed in `_summary.html.j2` (collapsible section)
- Register in `run_all()`
- E2E test: verify summary appears in HTML report
- Python test: verify standalone file generated with expected sections

**Out of scope:**
- Customizable summary templates
- PDF export
- Multi-language support

## Files to Modify

- Create: `src/yoinkc/renderers/executive_summary.py`
- Create: `src/yoinkc/templates/executive-summary.md.j2`
- Modify: `src/yoinkc/renderers/__init__.py` — add to `run_all()`
- Modify: `src/yoinkc/templates/report/_summary.html.j2` — add collapsible embed
- Modify: `src/yoinkc/templates/report/_css.html.j2` — embed styling
- Modify: `src/yoinkc/templates/report/_js.html.j2` — collapse/expand toggle

## Testing

| Test | Assertion |
|------|-----------|
| Standalone file generated | `executive-summary.md` exists in output, contains all 4 sections |
| Low complexity scored correctly | Snapshot with 0 attention items → "Low" / "Ready to build" |
| High complexity scored correctly | Snapshot with >5 attention items → "High" / "Significant manual work" |
| Fleet context included | Fleet snapshot summary mentions host count and prevalence |
| Single-host omits fleet context | Non-fleet snapshot has no fleet language |
| HTML embed exists | `#executive-summary` collapsible section in report.html |
| HTML embed collapses/expands | Click toggle, verify content visibility |
