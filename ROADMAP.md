# inspectah Roadmap

**Current version:** v0.9.0-beta.3 (pure Rust, schema 22)

## High

### NIC Naming Risk Detection

Detect `eth*` kernel-assigned NIC names on multi-NIC systems. After `bootc switch`, predictable naming kicks in and NIC assignment order may change, silently breaking networking. Emit a HIGH severity warning with remediation guidance.

### PAM Module Parsing

Parse `pam.d` module load lists, diff against the base image's module set, flag missing non-base modules (`pam_radius`, `pam_duo`, `pam_ldap`, `pam_centrify`) as HIGH severity. The difference between "your PAM config changed" and "your authentication will break."

### Secrets v2

Structured secrets detection and lifecycle improvements. Separate spec needed.

### /usr Walk Presentation

The remaining follow-up from the extended findings pass. `usr_entries` are collected today but still have no user-facing surface. Beta.3 should add a dedicated "Unmanaged /usr" section and carry per-entry decisions (`include-in-export`, `package`, `remove`, `exception`) through aggregate, refine, report, and export.

## Ready (Spec'd / Planned)

### Internals Documentation

Plain-English documentation of inspectah's internal decision logic for
contributors and maintainers. 5 documents in `docs/internals/`:

- Inspector logic reference (all 12 inspectors) — ~3,000-4,500 lines
- Classification engine (classify.rs, anaconda gap, triage buckets) — ~2,000-3,000 lines
- Containerfile renderer (section ordering, per-artifact rules) — ~500-800 lines
- Redaction engine (pattern catalog, confidence, false-positive filtering) — ~400-600 lines
- Baseline extraction (image pull, RPM diff, suppression) — ~300-500 lines

Estimated total: 6,200-9,400 lines.

### Docs Overhaul

User-facing documentation refresh. See `process-docs/backlog/documentation-backlog.md`.

## Needs Spec

### Sysctl Source File Preservation

Preserve original sysctl source filenames instead of collapsing into a single `99-inspectah-migrated.conf`. Group sysctls by source file in the UI with per-file toggle behavior.

### Config Content Viewer

Full-content modal or drawer for config files. Show full file with monospace formatting, RPM diff, and file metadata.

### Fleet Divergence Review UX

Clarify the divergent-item acknowledgment workflow. The session still tracks confirmed and changed divergent items, but the review model and its integration with include/exclude toggles need a clearer spec and UI.

### Clean Export Mode

Export option that strips working-state files (`snapshot.json`, `session.json`, `secrets-review.md`) from the tarball, producing build-pipeline-ready output.

### sshd_config Structured Parse

Parse individual `sshd_config` directives instead of raw file diff. Flag deprecated/removed directives against the target RHEL version.

### Secrets Safety Net

Guardrails to prevent accidental credential exposure in inspectah output. Separate spec needed.

## Ecosystem Expansion (Demand-Driven)

### Additional Language Package Ecosystems

- **yarn.lock** parsing + `yarn install --frozen-lockfile --production` rendering
- **pnpm-lock.yaml** parsing + `pnpm install --frozen-lockfile --prod` rendering
- **poetry.lock** detection + rendering
- **Conda/mamba** environment detection
- **PHP/Composer** detection (`composer.lock` / `composer.json`)
- **Maven/Gradle** advisory-only detection

Adoption context (as of 2026-07): yarn 21.5% Node.js usage (declining), pnpm 19.9% (rising), poetry ~85M monthly PyPI downloads. None ship as RPMs in RHEL.

### Locale/Timezone Containerfile Rendering

Inspector detects locale (`LANG`, `LC_*`) and timezone settings but the renderer does not emit instructions. Migrated image gets base image defaults. Likely advisory or commented-out since deploy-time config is the expected pattern for image mode.

### Version Pinning for System pip/gems

Same per-package pin toggle as npm globals, for consistency across ecosystems.

## Testing

### Driftify E2E Fixture Coverage Audit

Verify driftify's kitchen-sink mode covers all inspectah sections. Expand mutations to fill gaps so the E2E fixture exercises every triage path.

### Playwright E2E: CI Automation, Visual Regression, Multi-Browser

GitHub Actions coverage is in place, but the suite still needs a shared refine-server startup path for local and CI runs, screenshot assertions for key views, and a Firefox project for cross-engine coverage.

## Low / Pre-1.0

### Internationalization (i18n)

Locale-aware output for HTML audit reports and CLI. Translate user-facing strings at the render boundary. Initial language support driven by demand.

### Release Binary Size Optimization

Add `[profile.release]` settings: `lto = "thin"`, `strip = true`, `codegen-units = 1`. Expected 30-50% size reduction.

## Milestones

### Aggregate Spec 3: Factor

A proposed factor spec now exists in `process-docs/specs/proposed/2026-08-15-factor-spec.md`. Remaining work is the refined-aggregate export contract it depends on, then the factor implementation that consumes refined aggregate tarballs.

### Factor v2

Multi-artifact decomposition — decomposes a refined tarball into per-role artifacts.

## Done

- **Roadmap scope** — this file tracks remaining work. Completed release inventories live in `CHANGELOG.md`.
- **Recent shipped context** — see `process-docs/release-notes-0.9.0-beta.1.md` and `process-docs/release-notes-0.9.0-beta.2.md`.
