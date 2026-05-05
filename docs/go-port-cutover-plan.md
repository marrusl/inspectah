# Go Port Cutover Plan

Track the remaining work to merge `go-port` to `main` and retire the Python/container implementation.

**Branch:** `go-port`
**Last updated:** 2026-05-05

---

## P0 — Pre-cutover verification (must complete before merge)

Order matters — each item may surface issues that affect the next.

- [ ] **0a. Triage UX fixups** ← CURRENT (spec + plan approved, ready to execute)
  Default states, wording, visual hierarchy, and information architecture fixes from live testing.
  **Spec:** `docs/specs/proposed/2026-05-04-triage-ux-fixups-design.md` (approved, revision 5 — 5 design review rounds by Kit, Fern, Collins)
  **Plan:** `docs/plans/2026-05-04-triage-ux-fixups.md` (approved — 5 plan review rounds by Kit, Thorn, Fern)
  **Scope:** 13 tasks across 4 tiers. Default inclusion sweep, tier labels, version changes section, informational wrapper, three-state secret cards, accessible preview pane, kernel module filtering.
  **Execution:** Subagent-driven development (Kit implements per task).

- [ ] **0b. Non-RPM + containers triage restructure** ← NEXT (plan approved, ready to execute)
  Split overloaded "Containers" section into four subsections (quadlets, flatpaks, running containers, compose). Extract non-RPM software into its own planning-worksheet section with review-status controls. Fix pip false-positive detection. Add data-driven Containerfile stubs for non-RPM items, flatpak manifest + oneshot service output, and running-container-to-quadlet draft generation.
  **Spec:** `docs/specs/proposed/2026-05-04-non-rpm-containers-design.md` (approved, revision 4 — 5 design review rounds by Collins, Fern, Ember, Seal)
  **Plan:** `docs/plans/2026-05-04-non-rpm-containers.md` (approved, revision 6 — 6 plan review rounds by Thorn, Fern, Seal)
  **Scope:** 19 tasks across 5 phases. Schema evolution, inspector fixes (pip/flatpak/quadlet), triage classifier restructure, renderer output (non-RPM stubs, flatpak manifest, quadlet draft), SPA (containers hierarchy, non-RPM review-status cards, draft button, compose cards).
  **Execution:** Subagent-driven development (Kit implements per task).

- [x] **0. Single-machine triage redesign**
  Accordion grouping, notification cards, display-only surfaces, leaf-only packages (57 not 510), dep drill-down chevrons, version changes accordion (153 items), SELinux moved to System & Security. 33 commits landed on `go-port`, browser-verified against a live Fedora 43 scan. Minor fixups pending from live testing.

  **What shipped:** `TriageItem` extended with Group, CardType, DisplayOnly, Acknowledged, Deps fields. All 6 classifiers updated for single-machine grouping. `buildOutputAccordion` (3-state toggle), `buildDisplayAccordion`, `buildNotificationCard` JS components. `renderTriageSection` rewritten for grouped rendering. Renderer truth fix (TODO comments for unreachable packages). Static mode omits controls from DOM. Focus restoration after re-renders. Leaf normalization before sidecar. `isPassiveItem` excludes version changes from accounting. `semod-*` as ungrouped notification cards.

  **Review history:** 3 rounds on the original redesign spec/plan. 3 rounds on the UX fixes spec. 2 rounds on the UX fixes plan. All approved. Reviewers: Collins, Fern, Thorn, Kit.

  **Open backlog:** ~27 review follow-up items in `workflow/backlog/inspectah-*triage*` — most likely resolved by implementation, needs triage pass.

- [ ] **1. Verify fleet aggregation**
  Run `inspectah fleet` against multi-host scan data, verify merged snapshot, prevalence thresholds, and leaf/auto package union.

  **What exists:** Fleet merge engine complete (Phase 6 of Go port). Fleet loader discovers `.tar.gz` and `.json` snapshots, validates schema/OS/base-image compat, progressive display name disambiguation. Merge engine handles identity merge (packages, repos, services, firewall, cron, SELinux, non-RPM), content-variant merge (configs, drop-ins, quadlets, compose) with SHA-256 tie-breaking, module stream profile union, dep tree merge, prevalence filtering. 33 tests including golden-file test with 3 overlapping snapshots. Fleet command supports `--json-only`, `--output-dir`, `--output-file`, `--min-prevalence`.

  **What to verify:** Run against real multi-host scans (not just test fixtures). Verify merged output matches Python fleet behavior. Check that single-machine triage redesign did NOT leak into fleet mode (`isFleet` gate on all grouping/leaf-only/display-only code).

- [ ] **2. Test/fix/improve fleet refine mode**
  Fleet tarballs render correctly in refine. Fleet-specific decisions (prevalence-based include/exclude) work. Verify fleet snapshots do NOT get single-machine grouping, leaf-only filtering, or display-only surfaces.

  **What to verify:** Open a fleet tarball in refine. Check that packages show ALL packages (not leaf-only). Check that no accordion grouping appears (fleet items should have empty Group field). Check that prevalence data is visible. Test include/exclude decisions with rebuild.

- [ ] **3. Test/fix architect command UI**
  The architect web SPA for layer decomposition.

  **What exists:** Architect command implemented in Phase 7 of Go port. Web SPA serves an interactive layer editor. Package decomposition works (move packages between layers).

  **What to verify:** `inspectah architect <tarball>` launches, SPA loads, layers render, drag-and-drop or move UI works, export produces valid multi-layer Containerfiles.

  **Note:** Architect v2 (multi-artifact decomposition) is a P1 post-cutover feature. This verification is for the current single-artifact (packages-only) architect, not v2.

- [ ] **4. Functional parity check — Go matches Python**
  Compare Go and Python output on the same scan data. The Go port should produce equivalent output for all renderer sections.

  **What to compare:** Containerfile (package install lines, config COPY, service enablement, SELinux, users/groups, scheduled tasks, non-RPM, kernel/boot), kickstart, audit report, README, secrets review, merge notes. All sections should match semantically (formatting differences are OK).

  **Excluded from parity:** "+ New File" button (new feature, deferred to P1). Single-machine triage UI enhancements (intentionally different — accordion grouping, leaf-only, etc.).

  **How to test:** Run Python inspectah (container) and Go inspectah against the same scan tarball. Diff the Containerfile output. Check each renderer output file.

## P0 — Cutover (Mark's steps, after verification)

- [ ] **5. Tag v0.7.0-rc1 on go-port**
- [ ] **6. Smoke test on real RHEL system**
  Run `inspectah scan` on a real RHEL 9.x system (not just Fedora). Verify scan completes, tarball is valid, refine works. This is the final gate before merge.
- [ ] **7. Tag final container image v0.6.x-final on main**
  Last Python/container release. Preserves the ability to roll back.
- [ ] **8. Merge go-port to main, delete Python/container artifacts**
  Remove `src/inspectah/` (Python source), `Containerfile`, `Makefile` container targets, `run-inspectah.sh` (or keep as legacy wrapper temporarily). Keep Go source under `cmd/inspectah/`.
- [ ] **9. Tag v0.7.0**
  First pure-Go release.

## P1 — Post-cutover features

- [ ] **10. Secrets v2 implementation**
  Closes detection gaps (keystores, cockpit certs, WireGuard, WiFi), replaces hash-based tokens with deterministic sequential counters, adds remediation states, creates `redacted/` output directory with operator-facing guidance.

  **Spec:** `docs/specs/proposed/2026-04-08-secrets-handling-v2-design.md`
  **Plan:** `docs/plans/2026-04-08-secrets-handling-v2.md` (revision 5, approved)
  **Review history:** 4 rounds (1 full + 3 spot-checks) by Kit, Thorn, Sage. All blockers resolved. Two non-blocking nits noted for implementation (sudoers/passwd source field clarification, shuffled-input test scope).
  **Structure:** 5 milestones, 13 tasks with TDD steps. Kit implements via `superpowers:subagent-driven-development`.

- [ ] **11. Architect v2 — multi-artifact decomposition**
  Expands architect from package-only decomposition to 6 additional artifact types: configs, services, firewall rules, quadlets, users/groups, sysctls. The "move httpd and its configs follow" tied-change story is the single strongest differentiator.

  **Spec:** `docs/specs/proposed/2026-04-07-architect-v2-multi-artifact-design.md` (approved after 6 review rounds)
  **Plan:** `docs/plans/2026-04-07-architect-v2-implementation.md` (approved with minor wording nits)
  **UX spec:** `marks-inbox/reviews/2026-04-07-architect-v2-ux-spec.md` (Fern)
  **Key decisions:** 7 tabs (packages + 6 new). Artifact maturity tiers: full-support (packages, configs, services, quadlets), visible+decompose (firewall, sysctls), display-only (users/groups). Tied changes in scope. Config filtering at refine, not architect. Export reuses existing renderer modules. 4-phase implementation.

- [ ] **12. "+ New File" button in editor**
  Not started. Deferred from parity check — new feature for the editor section of refine mode.

- [ ] **13. COPR RPM packaging for Go binary**
  Go CLI wrapper is implemented and tested on a VM. First-run worked. UX improvements (Fern's phase markers) shipped. Package name is `inspectah`. Python CLI flags registered on Go commands for tab completion parity.

  **COPR state:** Repo exists at `copr.fedorainfracloud.org/coprs/mrussell/inspectah`. Currently builds the Python/container version. Needs update to build the Go binary instead. Target: Fedora 43/44/rawhide, EPEL 9/10, both x86_64 and aarch64. Cross-compilation from the Go module (no container build needed).

  **Key decisions:** Go wrapper is THE distribution method everywhere (not just EL8/9). Both wrapper + container versions shown in output until compat fears resolved. `run-inspectah.sh` stays functional until Go wrapper proves full parity.

## P2 — Future

- [ ] **14. Secrets safety net**
  Deferred from secrets v2 spec. Needs its own design spec. The safety net is a defense-in-depth mechanism that catches secrets the primary detection missed — potentially a post-render scan of output artifacts.

- [ ] **15. Fleet prevalence-driven grouping**
  Follow-up to single-machine triage redesign. Apply accordion grouping concepts to fleet mode with prevalence-aware behavior — group packages by prevalence tier, display low-prevalence items as candidates for exclusion.
