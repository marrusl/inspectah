# inspectah Roadmap

**Current version:** v0.8.6-beta.3 (Rust rewrite, Go retired)

```
Anaconda Gap Classifier (in progress)
    |
    +-- Group-Aware Rendering (blocked on classifier)
    |
    +-- Classification Logic Developer Docs
    |
Aggregate Spec 3: Architect
    |
CLI Cutover: Rust binary becomes primary `inspectah` command
    |
Post-cutover: Architect v2, `inspectah build`
```

## In Progress

### Anaconda Gap Classifier

Four-tier classification for packages Anaconda installs beyond the base container image: platform plumbing (locked exclude), promoted (user-intent via service+config signals), installer noise (soft exclude), ambiguous (Investigate, included by default). Group data collected via `dnf group list --installed`.

- **Priority:** In Progress
- **Branch:** `feature/anaconda-gap-classifier`
- **Spec:** `process-docs/specs/proposed/2026-06-11-anaconda-gap-classifier.md`
- **Plan:** `process-docs/plans/2026-06-11-anaconda-gap-classifier.md`

## High

### NIC Naming Risk Detection

Detect `eth*` kernel-assigned NIC names on multi-NIC systems. After `bootc switch`, predictable naming kicks in and NIC assignment order may change, silently breaking networking. Emit a HIGH severity warning with remediation guidance.

### PAM Module Parsing

Parse `pam.d` module load lists, diff against the base image's module set, flag missing non-base modules (`pam_radius`, `pam_duo`, `pam_ldap`, `pam_centrify`) as HIGH severity. The difference between "your PAM config changed" and "your authentication will break."

## Ready (Spec'd / Planned)

### Group-Aware Rendering

Render group-installed packages as `dnf group install` in the Containerfile instead of individual `dnf install` lines. Refine UI shows groups as collapsible rows with ungroup action. Depends on Anaconda Gap Classifier.

- **Pre-spec:** `process-docs/specs/proposed/2026-06-11-group-rendering-pre-spec.md`

### Unified Include-Default Model

Unifies include/default behavior across single-host and fleet modes. Consistent toggle semantics for all section types.

- **Status:** Spec + plan approved, ready for implementation

### Mandatory Baseline Requirement

Removes `--no-baseline`, adds exit code 3 for classified errors, schema v19. Baseline comparison becomes mandatory for accurate classification.

- **Status:** Spec + plan approved, ready for implementation

### Context Section Overhaul

Redesigns the refine UX context section. 8 implementation tasks.

- **Status:** Spec + plan approved, ready for implementation

### HTML Audit Report Redesign

Modernizes the HTML audit report output.

- **Status:** Spec approved, plan written

### Scan Output Rethink

Rethink `inspectah scan` CLI progress for the inspector section. Current per-inspector spinners were designed for 12-minute scans; the Rust rewrite runs in ~10 seconds. Direction: streaming append-only receipt, sub-steps behind `--verbose`.

- **Pre-spec:** `process-docs/specs/proposed/2026-06-10-scan-output-rethink.md`

### Classification Logic Developer Docs

Developer-facing explanation covering the full classification pipeline: baseline subtraction, anaconda gap tiers, leaf/auto classification, service/config classification, aggregate consensus. Location: `docs/explanation/classification-logic.md`.

## Needs Spec

### Sysctl Source File Preservation

Preserve original sysctl source filenames instead of collapsing into a single `99-inspectah-migrated.conf`. Group sysctls by source file in the UI with per-file toggle behavior.

### Config Content Viewer

Full-content modal or drawer for config files (currently truncated at 500 chars). Show full file with monospace formatting, RPM diff, and file metadata.

### Fleet Divergence Review UX

Clarify the variant acknowledgment workflow — the "0/11 confirmed" counter is opaque. Spec the confirmation model and its integration with include/exclude toggles.

### Clean Export Mode

Export option that strips working-state files (`snapshot.json`, `session.json`, `secrets-review.md`) from the tarball, producing build-pipeline-ready output.

### Autosave UX Improvements

Rethink the resume experience — show session info, possibly add in-UI "reset to original" option.

### sshd_config Structured Parse

Parse individual `sshd_config` directives instead of raw file diff. Flag deprecated/removed directives against the target RHEL version.

### Tier 2 Section Promotion

Promote scheduled tasks, SELinux booleans, and boot parameters from Reference to Review. Follows Tier 1 patterns with additional complexity (JSON dedup, cmdline decomposition, RPM-owned filtering).

## Testing

### Driftify E2E Fixture Coverage Audit

Verify driftify's kitchen-sink mode covers all inspectah sections. Expand mutations to fill gaps so the E2E fixture exercises every triage path.

### Playwright E2E: CI Automation, Visual Regression, Multi-Browser

Three incremental improvements to the Playwright suite: (1) auto-start refine server via `webServer` config + GitHub Actions integration, (2) screenshot comparison for key views to catch CSS regressions, (3) Firefox project for cross-engine coverage.

## Low / Pre-1.0

### Internationalization (i18n)

Locale-aware output for HTML audit reports and CLI. Translate user-facing strings at the render boundary. Initial language support driven by demand.

### Release Binary Size Optimization

Add `[profile.release]` settings: `lto = "thin"`, `strip = true`, `codegen-units = 1`. Expected 30-50% size reduction (current: 15-18 MB).

### Pre-1.0 Compat Sweep

Remove defensive backward-compatibility code from the Rust rewrite era. Before 1.0, old tarballs are not sacred — users re-scan.

## Milestones

### Aggregate Spec 3: Architect

Takes refined aggregate tarballs, discovers cross-role hierarchy, exports decomposed tarball set. May be multi-phase. Spec after current work stabilizes.

### CLI Cutover

Rust binary becomes the primary `inspectah` command.

### Post-Cutover

- Architect v2 (multi-artifact decomposition)
- `inspectah build` command
