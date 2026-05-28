# inspectah v0.8.2-alpha.1

**Date:** 2026-05-26
**Commits since v0.8.0-alpha.4:** 282

## Highlights

- **Section promotion** — Services, containers (quadlets/flatpaks), sysctls, and tuned profiles are now actionable Review sections with per-item include/exclude toggles, replacing their former read-only Reference status.
- **Unified package and repo management** — Packages are organized by source repository with tier-based classification (distro, official-optional, third-party). Repo-level toggles, conflict detection across fleet hosts, and an excluded-packages zone give direct control over what enters the migration output.
- **Triage classification engine** — The attention-level system is replaced by explicit triage buckets (Baseline/Site/Investigate for single-host; Universal/Partial/Divergent/Investigate for fleet) with structured reasons and validation signals.
- **CLI scan progress** — Three rendering backends (rich checklist with spinners, plain append-only, flat sequential) with `--verbose` and `--quiet` flags for controlling output detail.
- **Go fully retired** — Go source tree, CI workflow, schema compat code, and parity tests removed. Schema floor raised to current version. The Rust codebase is now the sole implementation.

## Fleet Mode

- Add strict intersection default for package include — packages below full fleet prevalence start excluded.
- Add repo groups, source repo, and repo conflict fields to fleet API response.
- Add aggregate prevalence for config file variants (file-level count alongside per-variant counts).
- Add fleet-wide divergent review tracking in session state.
- Add tuned profile prevalence and include state to fleet view.
- Add triage bucket badges to package rows with fleet-specific bucket rendering.
- Filter same-tier repo conflicts in fleet merge — same-tier differences (e.g., anaconda vs baseos) no longer report as conflicts.
- Fix tuned profile include reading from projected snapshot instead of hardcoding true.
- Fix intersection default applied consistently to all section types, not just packages.

## CLI Improvements

- Add `--verbose` (`-v`) and `--quiet` (`-q`) flags to `inspectah scan`. Quiet suppresses the progress checklist entirely; verbose plumbs a flag through for future sub-step detail. The two conflict via clap.
- Add three scan progress rendering backends: rich (block-redraw checklist with spinners and elapsed timers), plain (append-only with Unicode symbols), flat (numbered sequential lines for CI/piped output).
- Add `--progress` flag to select rendering backend; auto-detects TTY when omitted.
- Add `ProgressSink` trait threaded through all 11 inspector implementations and `collect()`.
- Surface not-in-base-image package count in version comparison summary output.

## Section Promotion

- Promote **services** to Review with parent-child drop-in toggles. Drop-ins render as indented children with cascade disable when the parent service is excluded.
- Promote **containers** to Review. Quadlets and flatpaks are decision items with lifecycle badges; compose items remain in Reference.
- Promote **sysctls** and **tuned profiles** to Review (merged as "System Tuning" section in the sidebar).
- Add SetInclude session ops for services, drop-ins, quadlets, flatpaks, sysctls, and tuned profiles.
- Add triage classification for tuned profiles and containers.
- Add ownership pruning for sysctl, tuned, and container config paths in the export pipeline.
- Update export contract to include promoted-section-owned file roots.
- Suppress stock default tuned profiles from Containerfile output.

## Triage Redesign

- Replace `AttentionLevel` enum with `TriageBucket` (single-host) and `FleetBucket` (fleet) types that do not map 1:1 — Baseline has no fleet equivalent, Divergent has no single-host equivalent.
- Add `TriageTag` struct carrying primary triage reason, validation signals, and prevalence data.
- Add `TriageBucketGroup` (collapsible) and `TriageStatusBar` (passive chip display) components.
- Collapse per-section toggle ops (ExcludePackage, IncludePackage, etc.) into unified `SetInclude` with `ItemId`.
- Implement autosave migration from v1 session format to v2 on load; remove all legacy `RefinementOp` variants.
- Add generic `ChangesSummary` and `RefineStats` supporting all promoted section types.

## Unified Package/Repo Management

- Add `RepoBar` component with vertical layout, "always included" labels for distro repos, and toggle switches for non-distro repos.
- Add `SortHeader` with uppercase labels, letter-spacing, and blue/gray active state.
- Add `ExcludedZone` for packages removed by repo-level or individual exclusion.
- Add `RepoConflictPopover` showing cross-tier repo conflicts in fleet mode.
- Add `PackageList` with prevalence color coding and tier-then-repo-then-name sort order.
- Add repo tier classification (Distro, OfficialOptional, ThirdParty, None) in core types.
- Rename `RepoTier::Unknown` to `RepoTier::None` for clarity.
- Classify `@commandline` packages correctly — tier Unknown, provenance Unknown, auto-exclude when config-only.
- Make `@commandline` repo non-toggleable with a friendly label in the UI.

## UI Polish

- Converge single-machine refine UI to match fleet visual patterns (RepoBar, SortHeader, PackageList, DecisionItem, ContextItem layouts).
- Rename sidebar groups from "Decisions/Context" to "Review/Reference."
- Make entire DecisionItem and ContextItem rows clickable for expand/collapse (not just the chevron).
- Add top-level checkbox to UserCard header.
- Add consistent section headings to all content panes.
- Hide expand chevron for items with empty or whitespace-only detail.
- Improve prevalence badge contrast — replace colored text on transparent with tinted pill badges exceeding WCAG AA 4.5:1 in both themes.
- Improve fleet banner contrast for dark theme.
- Fix `deduplicate_version()` display bug showing "CentOS Stream 9 9."
- Extract `AppShell` from `App.tsx` for fleet/single-host composition reuse.

## Bug Fixes

- Fix cross-section state bleed and search collisions when switching Reference tabs with overlapping item IDs.
- Fix CORS guard to accept both `localhost` and `127.0.0.1` as valid origins.
- Fix `RefineStats` TypeScript type alignment with Rust sections-based struct.
- Fix sidebar badge counts and StatsBar counters for new stats shape.
- Guard against missing annotations array in SysctlSection.
- Fix `@commandline` package classification falling through to ThirdParty.
- Remove unsafe type casts in triage migration code.

## Internal

- **Go retirement:** Remove Go source tree (`cmd/`, `go.mod`, `go.sum`), Go CI workflow, RPM spec `Conflicts: python3-inspectah` line. Rewrite RPM spec for Rust binary. Update release workflow for Rust toolchain.
- **Schema cleanup:** Raise `MIN_SCHEMA` to current version. Remove `patch_legacy_tie_fields()`, `deserialize_null_default` helper, `base_image_from_snapshot()` fallback, `normalize` module, and all legacy migration/parity tests. Simplify `migrate()` to version-stamp no-op.
- **Flatpak merge identity:** Use `(app_id, remote, branch)` tuple instead of name-only matching.
- **Refactoring:** Extract reusable repo tier constants (`DISTRO_REPOS`, `OFFICIAL_OPTIONAL_REPOS`, `repo_tier()`) to `inspectah-core::types::repo`. Collapse nested ifs per clippy suggestions.
- **Test improvements:** End-to-end validation for section promotion. Fleet integration tests updated for new type fields. 14 tests covering `deduplicate_version()` across RHEL, Fedora, and UBlue variants. Frontend test suite at 516+ passing tests.
- **Build:** Remove `.git-backup` from repo and add to `.gitignore`. Bump intermediate versions (0.8.1-alpha.1, 0.8.1-alpha.2).

## Known Issues

- Verbose sub-step collapsing in scan progress is plumbed but not yet implemented — `--verbose` flag accepted, rendering unchanged from normal mode.
- Variant selection UI ships for config files only; drop-in, quadlet, and compose variant UI deferred.
- Editor drawer for inline variant editing deferred (engine supports it, UI does not).
- Sysctl source file preservation (tracking which `.conf` file set a value) not yet implemented.
- Config noise filtering (deny-list for default/system-generated configs) not yet implemented.
