# Executive Summary Renderer

**Date:** 2026-03-31
**Status:** Proposed
**Author:** Coral (via brainstorm with Mark)

## Goal

Generate a plain-English, half-page migration summary alongside the technical artifacts. Written for the sysadmin who needs to justify the migration to stakeholders. Available as both a standalone markdown file and embedded in the HTML report.

## Context

inspectah produces technical artifacts (Containerfile, audit report, secrets review) but nothing that a non-technical stakeholder can read. The sysadmin has to manually translate findings into a summary for their manager. This renderer automates that translation.

## Design

### SummaryContext — Canonical Data Structure

Both the standalone markdown file and the HTML embed MUST render from a single `SummaryContext` instance. This structure is built once (inside `_build_context()`, after triage is computed) and passed to both templates. No template may compute values independently — all derived values live here.

```python
@dataclass
class SummaryContext:
    """Single source of truth consumed by executive-summary.md.j2
    and the HTML embed in _summary.html.j2."""

    # -- Section 1: System Overview --
    hostname: str                      # meta["hostname"]
    os_desc: str                       # os_release.pretty_name
    arch: str                          # meta.get("arch", "unknown")
    base_image: str                    # snapshot.rpm.base_image or "not set"
    is_fleet: bool                     # fleet_meta is not None
    fleet_host_count: int              # fleet_meta.total_hosts (0 if single)
    fleet_prevalence: int              # fleet_meta.min_prevalence (0 if single)

    # -- Section 2: Migration Scope --
    packages_added: int                # counts["packages_added"]
    config_files: int                  # counts["config_files"] (excludes quadlets)
    services_enabled: int              # counts["services_enabled"]
    services_disabled: int             # counts["services_disabled"]
    quadlet_units: int                 # NEW — sum(1 for q in containers.quadlet_units if q.include)
    compose_files: int                 # NEW — len(snapshot.containers.compose_files or [])
    scheduled_tasks: int               # counts["scheduled_tasks"]
    non_rpm_items: int                 # counts["non_rpm"]

    # -- Section 3: Attention Items --
    unresolved_ties: int               # from _build_context
    fixme_count: int                   # triage["fixme"] — depends on Containerfile existing (see Implementation Notes)
    secrets_redacted: int              # counts["redactions"]
    manual_count: int                  # triage["manual"]
    manual_warnings: int               # len(snapshot.warnings)
    manual_ssh_keys: int               # len(ssh_authorized_keys_refs)
    has_any_attention: bool            # computed: any of above > 0

    # -- Section 4: Remaining Work --
    complexity: str                    # "Low" | "Medium" | "High"
    complexity_label: str              # "Straightforward migration" | "Moderate complexity" | "Significant manual work expected"
    recommendation: str               # One-line summary (see Complexity Scoring below)
    automatic_ratio: float             # triage["automatic"] / max(total, 1), range 0.0-1.0
```

#### Builder function

```python
def build_summary_context(
    snapshot: InspectionSnapshot,
    counts: dict,
    triage: dict,
    unresolved_ties: int,
    fleet_meta: Optional[FleetMeta],
) -> SummaryContext:
    """Build the SummaryContext from existing _build_context data.

    Called inside html_report._build_context() AFTER triage is computed.
    The same SummaryContext is passed to both the standalone renderer
    and the HTML embed.
    """
```

This function takes already-computed values — it does NOT re-query the snapshot independently. The HTML renderer computes triage/counts once, builds a `SummaryContext`, and stashes it in the context dict as `"summary"`. Both templates consume `summary.*`.

#### New counts needed in `_summary_counts()`

Add alongside the existing `containers` key (do not remove `containers` — it is used elsewhere):

```python
"quadlet_units": (
    sum(1 for q in snapshot.containers.quadlet_units if q.include)
    if snapshot.containers and snapshot.containers.quadlet_units else 0
),
"compose_files": (
    len(snapshot.containers.compose_files or [])
    if snapshot.containers else 0
),
```

#### Single-context requirement

Both `executive-summary.md.j2` and the HTML embed in `_summary.html.j2` MUST reference only `summary.*` fields. No direct snapshot access from executive summary templates. This prevents dual-template drift — if a field changes, it changes in one place (`SummaryContext`), and both outputs update together.

### Disclaimer

Both the standalone markdown file and the HTML embed must include a short disclaimer, placed at the bottom of the summary content:

> This summary reflects automated analysis of packages, services, and configuration files. It does not assess security posture, compliance requirements, operational readiness, or production suitability. Review with your platform team before proceeding.

This text is rendered as a styled note/aside, not buried in fine print. Keep it visible but visually secondary to the summary content.

### Output Format

A structured half-page summary with four sections:

**1. System Overview** (2-3 sentences)
- Host identity: hostname (`meta["hostname"]`), OS (`os_desc`), architecture (`meta.get("arch", "unknown")`)
- Fleet context if applicable: N hosts (`fleet_meta.total_hosts`), prevalence threshold (`fleet_meta.min_prevalence`)
- Base image target (`snapshot.rpm.base_image` or "not set")

**2. Migration Scope** (3-4 bullets)
- Total packages to install (count of included RPMs)
- Config files to carry (count, with variant callout if fleet mode)
- Services to enable/disable (count)
- Other artifacts (quadlet units, compose files, scheduled tasks, non-RPM software — one line each if present, omitted if zero). Note: the existing `counts["containers"]` aggregates quadlets + compose + running containers. The summary uses the new split counts (`quadlet_units`, `compose_files`) instead.

**3. Attention Items** (2-3 bullets, omitted entirely if `has_any_attention` is false)
- Unresolved variant ties (count, which files)
- FIXME items requiring manual review (count) — note: depends on Containerfile being rendered first (see Implementation Notes)
- Secrets flagged for remediation (count)
- Manual items breakdown: "N items need manual handling (M secrets, P warnings, Q SSH key references)" — this is the explicit mapping for "items the tool couldn't handle automatically", sourced from the `manual` triage bucket (`secrets_redacted + manual_warnings + manual_ssh_keys`)

**4. Remaining Work** (1-2 sentences)
- Complexity estimate: "Straightforward migration" / "Moderate complexity" / "Significant manual work expected" (based on attention item count and triage ratios)
- One-line summary: "Analysis complete — N items resolved automatically, M need review" / "N items need resolution before building" / "Significant manual work expected — see attention items"

### Complexity Scoring

Simple heuristic based on package/config analysis, not a risk assessment. Evaluated as a first-match-wins cascade (High checked first, then Medium, then Low).

#### Inputs

| Input | Source | Type |
|---|---|---|
| `attention_items` | `fixme_count + manual_count + unresolved_ties` | `int` |
| `automatic_ratio` | `triage["automatic"] / max(total, 1)` where `total = automatic + fixme + manual` | `float` |
| `has_redacted_secrets` | `secrets_redacted > 0` | `bool` |

#### Decision cascade

```python
def _score_complexity(
    attention_items: int,
    automatic_ratio: float,
    has_redacted_secrets: bool,
) -> tuple[str, str, str]:
    """Return (complexity, complexity_label, recommendation)."""

    # High — first match wins
    if attention_items > 5:
        return ("High", "Significant manual work expected",
                "Significant manual work expected — see attention items")
    if has_redacted_secrets:
        return ("High", "Significant manual work expected",
                "Significant manual work expected — see attention items")
    if automatic_ratio < 0.80:
        return ("High", "Significant manual work expected",
                "Significant manual work expected — see attention items")

    # Medium
    if 1 <= attention_items <= 5:
        return ("Medium", "Moderate complexity",
                f"{attention_items} item{'s' if attention_items != 1 else ''} need resolution before building")

    # Low — everything else
    return ("Low", "Straightforward migration",
            "Analysis complete — ready to build")
```

#### Truth table

| Condition | Complexity | Label | Recommendation |
|---|---|---|---|
| 0 attention items AND auto ratio >= 0.80 AND no secrets | **Low** | Straightforward migration | "Analysis complete — ready to build" |
| 1-5 attention items AND auto ratio >= 0.80 AND no secrets | **Medium** | Moderate complexity | "N items need resolution before building" |
| >5 attention items OR any redacted secrets OR auto ratio < 0.80 | **High** | Significant manual work expected | "Significant manual work expected — see attention items" |

Note: The original spec listed `<80% automatic` as a Medium trigger, but `<80% automatic` mathematically implies `>20% non-automatic`, which is a High trigger. The cascade resolves this: `<80%` always hits High first. Medium only fires on 1-5 attention items with >=80% auto ratio and no secrets.

### Dual Render Targets

**1. Standalone file:** `executive-summary.md` in the output tarball. Rendered by a new renderer function in `src/inspectah/renderers/`. Uses the same Jinja2 environment as other renderers.

**2. HTML report embed:** A `<details open>` section at the top of the summary tab in the HTML report, above the dashboard cards. Expanded by default — the summary IS the stakeholder artifact, so it should be visible immediately. Evidence and detail sections use plain `<details>` (collapsed). Uses the same data, rendered as HTML rather than markdown.

### Data Sources

All data comes from the existing `_build_context()` output, plus two fields that require extraction from the snapshot directly:

- `os_desc` — system overview (`os_release.pretty_name`)
- `meta` — free-form dict containing `hostname` and `arch`. Note: `meta` is untyped (`dict = Field(default_factory=dict)`); the builder uses `meta.get("arch", "unknown")` defensively.
- `base_image` — from `snapshot.rpm.base_image` (not directly in `_build_context()` return; extracted from the snapshot object). Falls back to `"not set"` if absent.
- `counts` — migration scope numbers. Extended with `quadlet_units` and `compose_files` (see SummaryContext above).
- `triage` — automatic/fixme/manual ratios. `triage["fixme"]` is computed by `_count_containerfile_fixmes(output_dir)` which reads the generated Containerfile from disk — requires Containerfile to exist first.
- `unresolved_ties` — variant tie count
- `fleet_meta` — fleet context (`total_hosts`, `min_prevalence`). Note: the spec previously mentioned "role" but `FleetMeta` has no `role` field; use the profile name from `meta` as a proxy if needed, or omit.

## Scope

**In scope:**
- New renderer: `src/inspectah/renderers/executive_summary.py`
- New Jinja2 template: `src/inspectah/templates/executive-summary.md.j2`
- HTML embed in `_summary.html.j2` (collapsible section)
- Register in `run_all()`
- E2E test: verify summary appears in HTML report
- Python test: verify standalone file generated with expected sections

**Out of scope:**
- Customizable summary templates
- PDF export
- Multi-language support

## Files to Modify

- Create: `src/inspectah/renderers/_summary_context.py` — `SummaryContext` dataclass, `build_summary_context()`, `_score_complexity()`
- Create: `src/inspectah/renderers/executive_summary.py` — standalone markdown renderer
- Create: `src/inspectah/templates/executive-summary.md.j2` — standalone markdown template
- Modify: `src/inspectah/renderers/__init__.py` — add to `run_all()`, ensure ordering (after Containerfile, before HTML)
- Modify: `src/inspectah/renderers/html_report.py` — build `SummaryContext` in `_build_context()`, add `quadlet_units`/`compose_files` to `_summary_counts()`
- Modify: `src/inspectah/templates/report/_summary.html.j2` — add `<details open>` collapsible embed consuming `summary.*`
- Modify: `src/inspectah/templates/report/_css.html.j2` — embed styling
- ~~Modify: `src/inspectah/templates/report/_js.html.j2` — collapse/expand toggle~~ (not needed — use native `<details>`/`<details open>` instead of custom JS)

## Implementation Notes

### Renderer ordering dependency

`triage["fixme"]` is computed by `_count_containerfile_fixmes(output_dir)`, which reads `# FIXME` markers from the generated Containerfile on disk. The executive summary renderer MUST run after the Containerfile renderer. In `run_all()`, the ordering must be: `render_containerfile()` -> `render_audit_report()` -> `render_executive_summary()` -> `render_html_report()`.

Preferred approach: have `render_executive_summary()` accept a pre-computed `SummaryContext` rather than recomputing triage. Either (a) `run_all()` computes triage once and passes it to both renderers, or (b) the executive summary renderer is called from within `render_html_report()` after `_build_context()` returns. Option (a) is cleaner; option (b) is simpler and guarantees ordering but couples the renderers.

### Dual-template drift mitigation

Both `executive-summary.md.j2` and the HTML embed in `_summary.html.j2` consume `summary.*` fields exclusively. No direct snapshot access from executive summary templates. The single `SummaryContext` instance is the contract — if a field changes, it changes in one place and both outputs update together. A drift test (see Testing) verifies that key numbers match across both outputs.

### E2E collapse/expand testing

The spec requires that the `<details open>` collapsible section works correctly. Browser-based assertions (Playwright/Selenium) would be needed to test interactive collapse/expand behavior. For the initial implementation, test that `#executive-summary` exists in the HTML output and contains the expected text. Defer interactive testing unless the E2E framework already supports it.

### Division-by-zero guard

If all triage counts are zero (empty snapshot), `automatic_ratio` must use `max(total, 1)` as the denominator to avoid division by zero.

## Testing

| Test | Type | Assertion |
|------|------|-----------|
| Standalone file generated | Unit | `executive-summary.md` exists in output, contains all 4 sections |
| Straightforward complexity scored correctly | Unit | Snapshot with 0 attention items, >=80% auto, no secrets → "Straightforward migration" / "Analysis complete — ready to build" |
| Moderate complexity scored correctly | Unit | Snapshot with 3 attention items, >=80% auto, no secrets → "Moderate complexity" / "3 items need resolution before building" |
| Significant complexity scored correctly | Unit | Snapshot with >5 attention items → "Significant manual work expected" |
| Secrets trigger High | Unit | Snapshot with 0 attention items but 1+ redacted secret → "Significant manual work expected" |
| Low auto ratio triggers High | Unit | Snapshot with 0 attention items, <80% auto ratio → "Significant manual work expected" |
| Edge case: exactly 5 items | Unit | 5 attention items with >=80% auto, no secrets → "Moderate complexity" |
| Edge case: exactly 6 items | Unit | 6 attention items → "Significant manual work expected" |
| `build_summary_context()` fields | Unit | Known snapshot produces correct `SummaryContext` field values |
| `_score_complexity()` cascade | Unit | First-match-wins: High checked before Medium |
| Fleet context included | Unit | Fleet snapshot summary mentions host count and prevalence |
| Single-host omits fleet context | Unit | Non-fleet snapshot has no fleet language |
| HTML embed exists | E2E | `#executive-summary` collapsible section in report.html |
| HTML embed expanded by default | E2E | `<details open>` present on executive summary section |
| `executive-summary.md` in tarball | E2E | File exists in output tarball |
| Disclaimer present in standalone | Unit | `executive-summary.md` contains disclaimer text |
| Disclaimer present in HTML | E2E | `#executive-summary` section contains disclaimer text |
| Dual-output drift check | Unit | Parse both MD and HTML outputs, verify key numbers (packages, configs, services, complexity) match |
| Division by zero guard | Unit | Empty snapshot (all zeros) produces valid `SummaryContext` with `automatic_ratio = 0.0` and complexity "Low" |
