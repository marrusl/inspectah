# CLI Scan Progress UX — Pre-Spec

**Status:** Brainstorm input
**Date:** 2026-05-24
**ROADMAP ref:** "CLI UX: Scan Progress Reporting (MEDIUM)"

## Problem

`inspectah scan` goes completely silent while inspectors run. The CLI
hangs with no feedback until all 10+ inspectors complete. RPM package
collection dominates wall-clock time.

## Design Goal

Make the scan feel like a competent diagnostic that's *seeing* the system
— not a progress bar, not a spinner, not an animation. The user should
trust the report before they even open it.

**Frame:** Confidence-building, not entertainment. "htop energy, not brew
energy." Delight is a side effect of competence, not a goal.

## Competitive Context

Migration tools universally phone it in on progress UX. Leapp dumps
inhibitors after a silent wait. OSCAP shows a bare progress bar. Lynis
prints check names with color-coded results (closest to interesting, but
still a flat checklist). Security scanners (Trivy, Grype) are fast enough
that progress barely matters. Nobody in the migration/analysis space has
made scan output something you'd show someone. The bar is low — and
that's the opportunity.

## Inspector Inventory (10 phases)

| # | Inspector       | Expected Duration | Sub-progress? |
|---|-----------------|-------------------|---------------|
| 1 | RPM Packages    | Dominant (seconds) | Yes — see below |
| 2 | Config Files    | Moderate (rpm -Va) | Maybe |
| 3 | Services        | Fast               | No  |
| 4 | Containers      | Fast               | No  |
| 5 | Kernel/Boot     | Fast               | No  |
| 6 | Network         | Fast               | No  |
| 7 | Storage         | Fast               | No  |
| 8 | SELinux         | Fast               | No  |
| 9 | Users/Groups    | Fast               | No  |
|10 | Scheduled Tasks | Fast               | No  |
|11 | Non-RPM Pkgs    | Fast-Moderate      | No  |

## Ideas to Explore

### 1. Phase + Count Pattern (core)

Structured outer progress with live evidence:

```
Scanning [1/10] RPM packages
  > 427 packages found, classifying...
  > 312/427 classified (74%) — 8 repos mapped
Scanning [2/10] Config files
  Done (1.2s)
```

The outer counter gives predictability. The inner lines give evidence of
work. Fast inspectors get a simple done + timing line. RPM gets sub-progress
because it's the only phase slow enough to need it.

### 2. RPM Sub-Progress Beats

The RPM inspector runs 7 distinct steps internally (traced from
`inspectah-collect/src/inspectors/rpm/mod.rs`). Steps marked with shell
commands are the ones with real wall-clock time:

| Step | Function | What it does | Shell command(s) | Duration |
|------|----------|-------------|-------------------|----------|
| 1 | `query_packages()` | Query all installed packages | `rpm -qa --queryformat` | Moderate |
| 2 | `build_baseline()` | Build baseline map from target image | Pure computation | Fast |
| 3 | `classify_packages()` | Classify Added/Modified/BaseImageOnly | Pure computation | Fast |
| 3b | `populate_source_repos()` | Map packages to source repos | `dnf repoquery` per batch, fallback `rpm -qi` | Slow |
| 4 | baseline_suppressed | Find packages present in baseline | Pure computation | Fast |
| 5 | `classify_leaf_auto()` | Classify leaf vs transitive deps | `dnf repoquery --userinstalled` + `--requires --resolve --recursive` | Slow |
| 6 | `collect_supplementary()` | Repo files, GPG keys, module streams, version locks, rpm -Va | `rpm -Va` + filesystem reads | Moderate-Slow |
| 7 | `query_file_ownership()` | Map files to owning packages | `rpm -qa --queryformat` (file format) | Moderate |

**Reportable sub-phases** (the ones worth showing the user):

1. **"Querying installed packages..."** — step 1, the opening beat
2. **"Classifying N packages..."** — step 3, fast but gives a count
3. **"Resolving source repositories..."** — step 3b, genuinely slow (dnf)
4. **"Resolving dependency tree..."** — step 5, genuinely slow (recursive dnf)
5. **"Verifying package integrity..."** — step 6, rpm -Va can be slow
6. **"Mapping file ownership..."** — step 7, moderate

Steps 2 and 4 are pure computation and too fast to report.

### 3. Early Headlines (the screenshot moment)

After the first substantive inspector completes, print a one-line teaser:

```
Early finding: 3 packages have no image-mode equivalent
```

or:

```
847 packages analyzed — 23 need attention
```

This is the "first migration tool where the scan builds confidence in the
result" moment. The user sees the tool *thinking*, not just counting.

### 4. Completion Summary

When the scan finishes, print a concise summary before the tarball/report
path:

```
Scan complete (14.2s) — 847 packages, 12 configs, 4 services, 2 containers
Report: /tmp/inspectah-host01-20260524.tar.gz
```

## Terminal Capability & Accessibility

- Detect `$TERM`, `NO_COLOR` env var, and whether stderr is a TTY.
- **Capable terminal:** `\r` overwrite for live counters. Each completed
  phase prints a permanent line (scrollback + screen reader friendly).
- **Piped / dumb terminal:** Flat lines, no ANSI, no cursor rewriting.
  `[1/10] RPM packages... done (3.4s)`.
- **Semantic prefixes:** Checkmark/arrow carry state independently of
  color. Don't rely solely on color for meaning.
- **Reduced motion:** No animation beyond counter updates. No flickering.
  The "motion" is information arriving, not pixels moving.

## Effort Calibration

| Tier | What | Effort |
|------|------|--------|
| A | Phase counter + per-phase done/timing | Half day |
| B | + RPM sub-progress (live counts) | ~1 day |
| C | + Early headlines after first inspector | +half day |
| D | + Completion summary line | Trivial |

## Open Questions for Brainstorm

1. Should RPM sub-lines use in-place rewrite or append-only streaming?
   In-place is cleaner but less accessible. Append is noisier but durable.
2. Elapsed time per phase or cumulative? Per-phase tells you what's slow.
   Cumulative tells you how long you've been waiting.
3. Where do early headlines come from? The inspector needs to surface a
   "headline-worthy" finding. Not all inspectors have one. Which ones do?
4. What about fleet scan? Progress gets more complex when scanning N hosts.
   Defer or design now?
5. Config file inspector runs `rpm -Va` which can be slow — does it need
   sub-progress too?
6. Should the output adapt based on inspector count? (e.g., if baseline
   pull is added in Phase 6, that's another slow phase to report on)
