# inspectah Nit List

Small output quality and polish items. Not worth individual specs — just fix when touching nearby code.

## Repo Tier Model

- [ ] **RPM upload feature (needs spec):** Let users upload a local RPM into the tarball for repo-less packages. Separate folder in the tarball, direct `COPY + rpm -i` in the Containerfile. Turns "this package has no migration path" into "here's the RPM, install it directly." Solves the case for vendor installers, one-off downloads, and manual builds.

## Fleet Aggregate Output

- [ ] Aggregate output should surface useful information about divergence and agreement across the fleet. Give the user a sense of the mess they're dealing with — how consistent are the hosts, where do they diverge, what's uniform vs. scattered.

## Git History Cleanup

- [ ] Scrub `.git-backup/` from git history using `git filter-repo`. The directory was accidentally committed and contains a 68MB packfile. It's removed from the working tree and `.gitignore`'d, but still inflates clone size. Do this before the repo goes more public — requires a force push.

## ~~Scan Progress Follow-ups~~ (DONE — 2026-05-25)

Moved to Completed section.

## ~~Baseline Version Comparison Accuracy~~ (DONE — 2026-05-25)

Moved to Completed section.

## ~~Refine UI Polish~~ (DONE — 2026-05-26)

Moved to Completed section.

## ~~Fleet Refine UI~~ (DONE — 2026-05-26)

Moved to Completed section.

## RepoBar Click-to-Filter (v2 backlog)

- [ ] Repo names in the REPOSITORIES bar should be clickable. Clicking a repo name filters the package list to show only that repo's packages (or scrolls + highlights, lighter option). Render names as `<button>`, `cursor: pointer`, hover color shift to brand color, `aria-label="Jump to baseos packages (61)"`. Fern recommends scroll+highlight using existing `.inspectah-highlight` animation; Ember recommends filter-on-click as more useful for triage. Either way, distro repos gain their first interactive purpose beyond the "always included" label.

## RepoBar Accessibility

- [ ] RepoBar `aria-live` badge should announce dismiss/restore changes via a dedicated live-region message tied to the event, not just the static badge text. Currently the badge updates its visible count correctly, but the announcement is passive (relies on text mutation). A dedicated `aria-live` message ("1 conflict dismissed", "All conflicts restored") would be more reliable for screen readers. Flagged by Fern in round-2 review as important but non-blocking.

---

## Completed

### Naming Consistency (DONE — 2026-05-26)

- [x] **DECISION_SECTIONS / CONTEXT_SECTIONS → Review / Reference:** Renamed `DECISION_SECTIONS` → `REVIEW_SECTIONS` and `CONTEXT_SECTIONS` → `REFERENCE_SECTIONS` in `Sidebar.tsx`. No Rust-side constants existed. *(DONE — 2026-05-26)*

### Containerfile Output

- [x] `RUN dnf install -y` lines should use `\` line continuation right after `-y`, one package per line. More readable. Same treatment for the cleanup `dnf` and `rm` commands. *(DONE — 2026-05-25)*
- [x] Don't emit comment headers for empty sections. Structural fix: all 22 section headers route through a `section()` helper that only emits the header if body is non-empty. New sections get the guard automatically. *(DONE — 2026-05-25)*

### Repo Tier Model

- [x] Empty/unknown `section_id` falls through to `RepoTier::ThirdParty` in `repo_index.rs::repo_tier()`. Added explicit `RepoTier::None` variant (renamed from `Unknown` — "none" = no repo identity, not merely unidentified). *(DONE — 2026-05-25)*
- [x] **Config-only `@commandline` packages (auto-exclude):** `@commandline` packages whose ALL owned files are under `/etc/` and in the config tree get auto-excluded with `PackageConfigCaptured` reason. `epel-release` correctly rejected (has `/usr/bin/crb`). `repo_tier("@commandline")` returns `None`. *(DONE — 2026-05-25)*
- [x] **Repo-less packages (visibility):** `@commandline` source_repo now treated like empty — `NeedsReview` / `PackageNoRepoSource`. These surface as highest-risk migration items. *(DONE — 2026-05-25)*

### Fleet Prevalence Bug (configs)

- [x] Config prevalence undercount fixed. `FleetPrevalence` gains `aggregate_count`/`aggregate_hosts` (populated for multi-variant items). `merge_with_variants()` computes union of all variant hosts. Per-variant prevalence still tracks per-variant; aggregate tracks file-level. *(DONE — 2026-05-25)*

### Fleet Conflict Count Bug

- [x] `repo_conflict_count` overcount fixed. Root cause: `anaconda` vs `baseos` counted as conflict despite both being distro repos. Conflict detection now normalizes through `repo_tier()` — same-tier differences are skipped. Repo tier constants moved to `inspectah-core::types::repo` to avoid circular dep. *(DONE — 2026-05-25)*

### Fleet API Test Hardening

- [x] Make the `fleet_state_with_packages()` fixture minority-first so the end-to-end API test would independently fail if the row-level `source_repo` majority rewrite in `merge_rpm_sections()` were removed. *(DONE — 2026-05-25)*

### Go Retirement (DONE — 2026-05-24)

- [x] Remove Go source tree, CI workflow, packaging spec rewritten for Rust, schema floor raised, all Go compat code and parity tests removed.

### Scan Progress Follow-ups

- [x] **Export failure double-error:** Prevented duplicate error output on export failure. *(DONE — 2026-05-25)*

### Fleet vs Single-Machine Reference Section Density

- [x] Reference/context sections converged to compact rows matching fleet density. Replaced PF DataList with simple divs, subsection headers in uppercase. *(DONE — 2026-05-25)*

### Host Info Display

- [x] `os_name`/`os_version` duplication ("CentOS Stream 9 9"). Fixed in `handlers.rs` with `deduplicate_version()` — word-boundary matching handles RHEL 9/10, CentOS Stream, Fedora, Bazzite, Aurora, Bluefin. 14 tests. Minor version shown in parens when pretty_name has only major (e.g., "RHEL 10 (10.2)"). *(DONE — 2026-05-25)*

### Baseline Version Comparison Accuracy

- [x] Reframed: the "host newer" count was correct (only shared packages), but the summary didn't surface packages absent from the base image. `version_comparison_summary()` now takes a `not_in_base` count and appends ", N not in base image" when > 0. Both call sites (CLI scan + Containerfile report table) updated. *(DONE — 2026-05-25)*

### Scan Progress Follow-ups

- [x] **`--verbose` / `--quiet` flags:** Added `-v` and `-q` flags to `inspectah scan`. `--quiet` suppresses progress checklist (null renderer), still prints completion + output path. `--verbose` plumbed through to all three renderers (sub-step collapsing not yet implemented — plumbing ready). `conflicts_with` enforced via clap. *(DONE — 2026-05-25)*

### Refine UI Polish (2026-05-26 testing session)

- [x] **User toggle missing:** Added checkbox to UserCard header mapping to skip (unchecked) / useradd (checked), matching DecisionItem pattern. *(DONE — 2026-05-26)*
- [x] **Row click to expand:** Added `onClick={handleExpand}` to DecisionItem and ContextItem row divs with `stopPropagation` on interactive children. `cursor: pointer` styling. *(DONE — 2026-05-26)*
- [x] **@commandline repo toggle:** Made non-toggleable by filtering on `provenance === "unknown"`. Display name changed to "Local / Manual installs". *(DONE — 2026-05-26)*
- [x] **Missing section titles:** Added consistent `<h2>` headings to all content panes via `SECTION_LABELS` map in MainContent. *(DONE — 2026-05-26)*
- [x] **Non-functional chevrons in Storage:** Added `item.detail.trim().length > 0` check to ContextItem. *(DONE — 2026-05-26)*
- [x] **Storage mounts leaking into unrelated sections:** Frontend bug — missing React keys on ContextList caused cross-section state reuse, and GlobalSearch `searchTextMap` had bare-ID key collisions across sections. Fixed with section-scoped keys. *(DONE — 2026-05-26)*

### Fleet Refine UI (2026-05-26 testing session)

- [x] **Warning banners too dark:** Replaced hardcoded backgrounds with `color-mix()` tints that work in both themes. *(DONE — 2026-05-26)*
- [x] **Fleet defaults to intersection:** Applied strict intersection default to all 14 section types — items below full prevalence start excluded but visible. *(DONE — 2026-05-26)*
- [x] **Fleet toggles broken:** Configs, services, drop-ins, and quadlets were recalculating include from raw prevalence instead of reading projected snapshot state. Fixed to use `entry.include`. *(DONE — 2026-05-26)*
- [x] **Tuned fleet data wrong:** Three fixes — merge hardcoded `true`, view hardcoded `true`, prevalence passed as `None`. Added `is_stock_tuned_profile()` recognizing 14 stock profiles; both single-host and fleet paths suppress stock defaults. *(DONE — 2026-05-26)*
- [x] **Prevalence badge contrast:** Replaced yellow-on-white text with tinted pill badges (green/amber/red) all exceeding WCAG AA 4.5:1. *(DONE — 2026-05-26)*
