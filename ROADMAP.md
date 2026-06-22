# inspectah Roadmap

**Current version:** v0.8.6-beta.3 (pure Rust, CLI cutover complete)

```
Group Rendering: Refine UI (spec'd)
    |
    +-- Classification Logic Developer Docs
    |
Aggregate Spec 3: Architect
    |
Architect v2 (multi-artifact decomposition)
```

## High

### NIC Naming Risk Detection

Detect `eth*` kernel-assigned NIC names on multi-NIC systems. After `bootc switch`, predictable naming kicks in and NIC assignment order may change, silently breaking networking. Emit a HIGH severity warning with remediation guidance.

### PAM Module Parsing

Parse `pam.d` module load lists, diff against the base image's module set, flag missing non-base modules (`pam_radius`, `pam_duo`, `pam_ldap`, `pam_centrify`) as HIGH severity. The difference between "your PAM config changed" and "your authentication will break."

## Ready (Spec'd / Planned)

### Group Rendering: Refine UI

Refine UI shows package groups as collapsible rows with ungroup action. Containerfile rendering (`dnf group install`) already ships; this covers the interactive review experience.

- **Spec:** `process-docs/specs/proposed/2026-06-11-group-rendering-spec.md`

### HTML Audit Report Redesign

Modernizes the HTML audit report output.

- **Status:** Spec approved, plan written

### Classification Logic Developer Docs

Developer-facing explanation covering the full classification pipeline: baseline subtraction, anaconda gap tiers, leaf/auto classification, service/config classification, aggregate consensus. Location: `docs/explanation/classification-logic.md`.

## Needs Spec

### Sysctl Source File Preservation

Preserve original sysctl source filenames instead of collapsing into a single `99-inspectah-migrated.conf`. Group sysctls by source file in the UI with per-file toggle behavior.

### Config Content Viewer

Full-content modal or drawer for config files. Show full file with monospace formatting, RPM diff, and file metadata.

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

Add `[profile.release]` settings: `lto = "thin"`, `strip = true`, `codegen-units = 1`. Expected 30-50% size reduction.

## Milestones

### Aggregate Spec 3: Architect

Takes refined aggregate tarballs, discovers cross-role hierarchy, exports decomposed tarball set. May be multi-phase. Spec after current work stabilizes.

### Architect v2

Multi-artifact decomposition — decomposes a refined tarball into per-role artifacts.
