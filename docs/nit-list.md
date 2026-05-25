# inspectah Nit List

Small output quality and polish items. Not worth individual specs — just fix when touching nearby code.

## Containerfile Output

- [x] `RUN dnf install -y` lines should use `\` line continuation right after `-y`, one package per line. More readable. Same treatment for the cleanup `dnf` and `rm` commands. *(DONE — 2026-05-25)*
- [x] Don't emit comment headers for empty sections. Structural fix: all 22 section headers route through a `section()` helper that only emits the header if body is non-empty. New sections get the guard automatically. *(DONE — 2026-05-25)*

## Repo Tier Model

- [x] Empty/unknown `section_id` falls through to `RepoTier::ThirdParty` in `repo_index.rs::repo_tier()`. Added explicit `RepoTier::Unknown` variant. *(DONE — 2026-05-25)*
- [ ] **Config-only `@commandline` packages (auto-exclude):** Packages like `epel-release` installed via `rpm -i` show as `@commandline` source. When ALL files owned by the package are already captured in the config tree (repo files, GPG keys), auto-exclude the package with a note: "contents captured via config files." The Containerfile doesn't need `dnf install epel-release` because the repo config is already `COPY`'d in. Classification fix in the refine pipeline.
- [ ] **Repo-less packages (visibility):** Non-config `@commandline` packages (actual software installed from local RPM) should be highly visible — sort near top, default to `include=false`. These are the highest-risk migration items: no repo means no clean path into a Containerfile `dnf install`.
- [ ] **RPM upload feature (needs spec):** Let users upload a local RPM into the tarball for repo-less packages. Separate folder in the tarball, direct `COPY + rpm -i` in the Containerfile. Turns "this package has no migration path" into "here's the RPM, install it directly." Solves the case for vendor installers, one-off downloads, and manual builds.

## Fleet Prevalence Bug (configs)

- [ ] Config prevalence is undercounted when a file has multiple variants. `/etc/chrony.conf` exists on 2 hosts (web-02, web-03) with 2 different content variants, but prevalence reports 1/3 instead of 2/3. The merge logic likely counts unique content hashes rather than summing `host_count` across all variants. Prevalence = "how many hosts have this file," not "how many hosts have the same version." Fix in the fleet merge prevalence calculation for config items.

## Fleet Conflict Count Bug

- [ ] `repo_conflict_count` in the fleet API response reports 8 but only 1 package (`sssd-kcm.aarch64`) actually has a `repo_conflict` entry. The count may be conflating a repo's package count with actual cross-host repo conflicts. Check the conflict counting logic in the fleet merge/view code.

## Fleet Aggregate Output

- [ ] Aggregate output should surface useful information about divergence and agreement across the fleet. Give the user a sense of the mess they're dealing with — how consistent are the hosts, where do they diverge, what's uniform vs. scattered.

## Fleet API Test Hardening

- [x] Make the `fleet_state_with_packages()` fixture minority-first so the end-to-end API test would independently fail if the row-level `source_repo` majority rewrite in `merge_rpm_sections()` were removed. *(DONE — 2026-05-25)*

## ~~Go Retirement~~ (DONE — 2026-05-24)

- [x] Remove Go source tree, CI workflow, packaging spec rewritten for Rust, schema floor raised, all Go compat code and parity tests removed.

## Git History Cleanup

- [ ] Scrub `.git-backup/` from git history using `git filter-repo`. The directory was accidentally committed and contains a 68MB packfile. It's removed from the working tree and `.gitignore`'d, but still inflates clone size. Do this before the repo goes more public — requires a force push.

## Scan Progress Follow-ups

Deferred from the scan progress feature (2026-05-24):

- [ ] **Early headlines:** Print a one-line teaser after the first substantive inspector completes (e.g., "847 packages — 23 need attention"). Confidence-building moment. Ember's idea, deferred from v1.
- [ ] **`--verbose` / `--quiet` flags:** Layer on top of the three rendering modes. `--verbose` could show all sub-steps even for fast inspectors; `--quiet` could suppress the checklist entirely and just print the completion line.
- [ ] **Fleet scan progress:** Scanning N hosts needs a different UX — per-host progress, aggregate completion. Defer to fleet work.
- [x] **Export failure double-error:** Prevented duplicate error output on export failure. *(DONE — 2026-05-25)*

## Baseline Version Comparison Accuracy

- [ ] CLI summary says "15 packages host newer" but conflates two different cases: (1) packages shared between host and target image where the host genuinely has a newer version, and (2) packages that aren't in the target image at all. "Host newer" should only apply when both sides have the package and the host version is actually newer. Packages absent from the target image should be counted and labeled separately (e.g., "not in base image") rather than lumped into the "host newer" bucket.

## Fleet vs Single-Machine Reference Section Density

- [x] Reference/context sections converged to compact rows matching fleet density. Replaced PF DataList with simple divs, subsection headers in uppercase. *(DONE — 2026-05-25)*

## Host Info Display

- [x] `os_name`/`os_version` duplication ("CentOS Stream 9 9"). Fixed in `handlers.rs` with `deduplicate_version()` — word-boundary matching handles RHEL 9/10, CentOS Stream, Fedora, Bazzite, Aurora, Bluefin. 14 tests. Minor version shown in parens when pretty_name has only major (e.g., "RHEL 10 (10.2)"). *(DONE — 2026-05-25)*

## RepoBar Click-to-Filter (v2 backlog)

- [ ] Repo names in the REPOSITORIES bar should be clickable. Clicking a repo name filters the package list to show only that repo's packages (or scrolls + highlights, lighter option). Render names as `<button>`, `cursor: pointer`, hover color shift to brand color, `aria-label="Jump to baseos packages (61)"`. Fern recommends scroll+highlight using existing `.inspectah-highlight` animation; Ember recommends filter-on-click as more useful for triage. Either way, distro repos gain their first interactive purpose beyond the "always included" label.

## RepoBar Accessibility

- [ ] RepoBar `aria-live` badge should announce dismiss/restore changes via a dedicated live-region message tied to the event, not just the static badge text. Currently the badge updates its visible count correctly, but the announcement is passive (relies on text mutation). A dedicated `aria-live` message ("1 conflict dismissed", "All conflicts restored") would be more reliable for screen readers. Flagged by Fern in round-2 review as important but non-blocking.
