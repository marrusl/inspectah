# Design Doc Refresh + Backlog

**Date:** 2026-03-15
**Status:** Approved

## Problem

Both `inspectah/design.md` and `driftify/design.md` were written before most
features existed. Major capabilities — fleet analysis, tarball-first output,
prevalence UI, entitlement bundling, fleet testing — are implemented but not
reflected in the design docs. The inspectah design doc also buries its
architecture overview under inspector-level details, making it hard to get
the big picture without reading the whole thing.

## Scope

1. Restructure and update `inspectah/design.md` as the living system doc
2. Targeted update of `driftify/design.md`
3. Refresh Future Work sections with a prioritized backlog and tech debt items

## Approach

**Restructure and update in-place.** The existing docs have strong writing
and reasoning worth preserving. Reorganize with architecture-first structure,
update all sections to match current implementation, add sections for missing
features. The implemented spec files in `docs/specs/implemented/` remain as
historical artifacts.

---

## inspectah design.md — New Structure

### 1. Overview & Principles

**New section.** Currently the doc jumps straight into Runtime Model.

Contents:
- One-paragraph purpose statement (inspect package-based RHEL/CentOS/Fedora
  hosts, produce bootc migration artifacts)
- Core design principle: **baseline subtraction** (currently buried in
  README's Architecture section)
- The inspect → schema → render pipeline as the organizing concept
- Three companion tools in one sentence each: `inspectah-refine`,
  `inspectah-build`, `inspectah-fleet`

### 2. Architecture

**Expanded from brief existing subsection.**

Contents:
- Module map: `inspectors/` → `schema.py` → `renderers/`, orchestrated by
  `pipeline.py`
- Runtime model (existing content, lightly edited — container, `nsenter`,
  `--pid=host`, progress output)
- Baseline generation (existing content relocated from deep in the doc —
  architecturally central, belongs here)
- Executor abstraction (how inspectors talk to the host)

### 3. CLI Reference

**Refreshed from current `cli.py`.**

Contents:
- Verify against actual argument parser and update
- All current entry points: `inspectah`, `inspectah-fleet`, `inspectah-refine`,
  `inspectah-build` (future entry points like `inspectah-render` are documented
  in section 10, not here)
- Environment variables (`INSPECTAH_HOSTNAME`, `INSPECTAH_IMAGE`, `INSPECTAH_DEBUG`)
- Wrapper scripts (`run-inspectah.sh`, `run-inspectah-fleet.sh`)

### 4. Schema

**Entirely new section.** Currently not documented at all.

Contents:
- Schema versioning (`SCHEMA_VERSION`) and version check on load
- Key model hierarchy: `InspectionSnapshot` as root, section models, item
  models
- Fleet metadata: `FleetPrevalence` and `FleetMeta` models, the `fleet`
  field on item models
- Serialization: `model_dump_json` / `model_validate` round-trip
- Brief note on Pydantic v2 usage

### 5. Inspectors

**Existing content, refreshed.**

The inspector catalog is the strongest part of the current doc. Changes:
- Light pass to verify each inspector's description matches current code
- Add any detection capabilities added since the doc was written (tuned
  profiles, drop-in overrides, etc.)
- Remove the "Implementation Language" section (pre-implementation decision
  record, not ongoing design)

### 6. Renderers

**New section.** Currently renderers get brief mentions in "Output Artifacts"
but no architectural treatment.

Contents:
- The 6 renderers: containerfile, audit_report, html_report, readme,
  kickstart, secrets_review
- Jinja2 environment setup, template loading
- `_triage.py` shared computation
- HTML report architecture: self-contained single file, PF6 CSS (unminified),
  sidebar navigation, section toggling, fleet-aware conditionals
- Containerfile layer ordering (existing content relocated from Output
  Artifacts)

### 7. Fleet Analysis

**Entirely new section.**

Contents:
- Purpose: aggregate multi-host inspections into one image spec
- Architecture: `fleet/loader.py` (discovery + validation) →
  `fleet/merge.py` (identity/content merge + prevalence) → rendering
- Merge engine: identity keys per item type, content variant grouping,
  prevalence computation
- Prevalence threshold (`-p`) and its effect on Containerfile inclusion
- Fleet UI: summary banner, color bars, toggle, host popovers, content
  variant grouping
- `run-inspectah-fleet.sh` container wrapper

### 8. Pipeline & Packaging

**Reorganized from existing content.**

Contents:
- `pipeline.py` orchestrator: inspect-or-load → redact → render → package
- Tarball-first output (new default, replaces old directory-based default)
- Entitlement cert bundling (moved from wrapper into pipeline)
- `run-inspectah.sh` wrapper script and its env vars
- `--validate` build validation
- `--push-to-github` flow
- Containerfile/image build (`Containerfile` at repo root)

### 9. Secret Handling

**Existing content, mostly unchanged.**

Contents:
- Redaction patterns and layers (verify patterns in `redact.py` still match)
- GitHub push guardrails (verify `git_github.py` pre-push checks)
- Both subsystems need a read-through against current code

### 10. Future Work & Backlog

**Refreshed.** Remove completed items. Source material for this section is
the Prioritized Backlog and Tech Debt sections of this spec — they become
the new Future Work content in `design.md`.

---

## driftify design.md — Update Plan

The existing structure stays the same. Targeted updates:

### Coverage Map Updates
- **Service Inspector:** Add drop-in override rows for httpd (standard) and
  nginx (kitchen-sink)
- **User/Group Inspector:** Add shared `developers` group with supplementary
  membership
- **RPM Inspector:** Verify RPM Fusion package distribution matches current
  code (reshuffled in recent commits)

### Non-Goals Update
- Currently says "driftify does not handle multi-host scenarios"
- Update: driftify itself is single-host, but `run-fleet-test.sh` orchestrates
  multi-host fleet testing

### New Section: Fleet Testing
- Document `run-fleet-test.sh`: what it does, how it works
- Curls driftify + run-inspectah.sh, runs 3 profiles with unique hostnames,
  aggregates results
- Place after CLI Interface or as subsection of Coverage Verification

### File Inventory
- Add drop-in override files and any new files created since doc was written
- Verify full list against current code

### CLI Interface
- Verify flags match current `argparse` block
- Check `--run-inspectah` behavior

### Future Work
- Fleet drift variations: partially addressed by `run-fleet-test.sh`, true
  within-profile variation still future work
- CI integration: still unimplemented, keep
- Parameterized scenarios: still unimplemented, keep

---

## Prioritized Backlog

### High Priority

**1. Cross-stream targeting (resume paused brainstorm)**

The spec at `docs/specs/proposed/2026-03-13-cross-stream-targeting-design.md`
is paused at "propose approaches" with 6 open questions. Most impactful
unbuilt feature — unlocks RHEL 9→10 upgrades, CentOS→RHEL lateral moves,
Fedora→CentOS targeting. Infrastructure partially ready: `baseline.py` has
OS→image mapping tables, schema has `os_release` metadata. Open questions
need resolution: how target context flows through the pipeline, package name
mapping granularity, repo file handling, config file version differences,
validation against target image, report presentation.

**2. CI integration (both projects)**

Neither project has CI. Both design docs identify this as future work. GitHub
Actions workflow: driftify → inspectah → validate output on CentOS Stream 9, 10,
and Fedora. Catches regressions unit tests can't (real package installs, real
service state, real SELinux). Biggest quality-of-life improvement for ongoing
development.

**3. Containerless re-rendering**

`inspectah-render` entry point: takes snapshot JSON, produces output artifacts
using only Python (no container, no `nsenter`). Enables tarball-only
workflows on machines without podman. Rendering pipeline is already pure
Python — mostly a CLI/packaging exercise.

### Medium Priority

**4. Fleet drift variations in driftify**

`run-fleet-test.sh` produces 3 hosts with different profiles. Real fleet
drift is subtler — same role, slightly different configs. A mode applying
small per-run variations (extra package, different config value) would produce
more realistic fleet test data.

**5. Enhanced cron-to-timer conversion**

Current cron inspector captures jobs but conversion is basic. Deeper semantic
analysis: `MAILTO` → journal notifications, `@reboot` handling, environment
variable preservation.

**6. `/var` size estimation improvement**

Storage inspector estimates directory sizes via Python file iteration (slow
for large trees). Using `du` via executor would be faster. Low risk, small
scope.

### Lower Priority

**7. In-place migration mode**

Run inspectah on one representative host, refine, apply across fleet. Large
feature depending on cross-stream targeting.

**8. Snapshot diffing / drift detection**

Compare snapshots across hosts or time for compliance auditing. Fleet
analysis is the foundation — merge engine already computes prevalence.

**9. Cross-family migration (Ubuntu/Debian → RPM bootc)**

New inspectors, generalized package model, config path translation. Separate
project with its own spec cycle.

---

## Tech Debt

### Address Soon

**1. `containerfile.py` (1,293 lines)**

Largest source file. Renders every Containerfile section in one module.
Cross-stream targeting will add conditional logic per target distro, making
this harder to maintain. Split by section (packages, services, config, etc.)
similar to inspector structure.

**2. `report.html.j2` (2,556 lines)**

Single Jinja2 template with embedded CSS, JS, and all section markup. Fleet
prevalence UI added significant complexity. Use Jinja2 `{% include %}` to
split into section partials while keeping single-file HTML output.

**3. Large test files**

`test_renderer_outputs.py` (1,764 lines), `test_plan_items.py` (1,463
lines), `test_inspectors.py` (1,208 lines). Not broken, but harder to
navigate. Split by renderer or inspector to match source structure.

### Monitor

**4. `schema.py` (523 lines, ~50 models)**

Growing but coherent. All models in one file keeps imports simple. Consider
splitting into `schema/` package if fleet metadata or cross-stream targeting
adds significantly more models.

**5. Renderer triage computation**

`_triage.py` shared between audit report and HTML report. Fine for current
renderer count. May need more explicit structure if renderers multiply.

**6. PF6 CSS size**

Full unminified PF6 (36,408 lines) embedded in every report. Pragmatic and
debuggable. Size difference vs. minified is ~6%. Only matters if report file
size becomes a concern (emailing, archiving thousands).
