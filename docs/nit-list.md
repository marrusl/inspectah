# inspectah Nit List

Small output quality and polish items. Not worth individual specs — just fix when touching nearby code.

## Containerfile Output

- [ ] `RUN dnf install -y` lines should use `\` line continuation right after `-y`, one package per line. More readable. Same treatment for the cleanup `dnf` and `rm` commands.
- [ ] Don't emit comment headers for empty sections. If `# === Scheduled Tasks ===` (or any section header) has no content beneath it, skip the header entirely.

## Repo Tier Model

- [ ] Empty/unknown `section_id` falls through to `RepoTier::ThirdParty` in `repo_index.rs::repo_tier()`, leaving `provenance=Unknown` + `tier=ThirdParty` representable in the API. "No repo identity" is not the same as "known third-party." Either add an explicit `Unknown` tier variant, make tier optional, or gate the fallthrough on non-empty section_id. Flagged by Tang in unified-package-repo round-1 review.
- [ ] Repo-less packages (empty `source_repo`) should be highly visible — sort near top, default to `include=false`. These are the highest-risk migration items: no repo means no clean path into a Containerfile `dnf install`. Current behavior buries them as generic third-party.
- [ ] **Future feature (needs spec):** Let users upload a local RPM into the tarball for repo-less packages. Separate folder in the tarball, direct `rpm -i` or `COPY + rpm -i` in the Containerfile. Turns "this package has no migration path" into "here's the RPM, install it directly." Would close the loop for packages installed from one-off downloads, vendor installers, or manual builds.

## Fleet Aggregate Output

- [ ] Aggregate output should surface useful information about divergence and agreement across the fleet. Give the user a sense of the mess they're dealing with — how consistent are the hosts, where do they diverge, what's uniform vs. scattered.

## Fleet API Test Hardening

- [ ] Make the `fleet_state_with_packages()` fixture minority-first so the end-to-end API test would independently fail if the row-level `source_repo` majority rewrite in `merge_rpm_sections()` were removed. Currently the fixture happens to produce correct results even without the rewrite. Suggested by Tang in round-2 review.

## Go Retirement

- [ ] Remove Go source tree (`cmd/`, `go.mod`, `go.sum`, etc.) once the Go CLI wrapper is fully retired and the Rust binary is the sole distribution path. Straightforward delete — wait until Go is no longer packaged or referenced.

## Git History Cleanup

- [ ] Scrub `.git-backup/` from git history using `git filter-repo`. The directory was accidentally committed and contains a 68MB packfile. It's removed from the working tree and `.gitignore`'d, but still inflates clone size. Do this before the repo goes more public — requires a force push.

## Scan Progress Follow-ups

Deferred from the scan progress feature (2026-05-24):

- [ ] **Early headlines:** Print a one-line teaser after the first substantive inspector completes (e.g., "847 packages — 23 need attention"). Confidence-building moment. Ember's idea, deferred from v1.
- [ ] **`--verbose` / `--quiet` flags:** Layer on top of the three rendering modes. `--verbose` could show all sub-steps even for fast inspectors; `--quiet` could suppress the checklist entirely and just print the completion line.
- [ ] **Fleet scan progress:** Scanning N hosts needs a different UX — per-host progress, aggregate completion. Defer to fleet work.
- [ ] **Export failure double-error:** When tarball or `--inspect-only` write fails, `scan.rs` prints the structured error and exits directly. But if the error propagates through `main.rs` instead (e.g., from a pre-export failure), it can produce a duplicate `error:` line. Minor transcript polish.

## RepoBar Accessibility

- [ ] RepoBar `aria-live` badge should announce dismiss/restore changes via a dedicated live-region message tied to the event, not just the static badge text. Currently the badge updates its visible count correctly, but the announcement is passive (relies on text mutation). A dedicated `aria-live` message ("1 conflict dismissed", "All conflicts restored") would be more reliable for screen readers. Flagged by Fern in round-2 review as important but non-blocking.
