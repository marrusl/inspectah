# Go Port Cutover Plan

Track the remaining work to merge `go-port` to `main` and retire the Python/container implementation.

**Branch:** `go-port`
**Last updated:** 2026-05-04

---

## P0 — Pre-cutover verification (must complete before merge)

- [x] **0. Single-machine triage redesign**
  Accordion grouping, notification cards, display-only surfaces, leaf-only packages, dep drill-down, version changes, SELinux in System & Security. 33 commits landed, browser-verified. Minor fixups pending from live testing.

- [ ] **1. Verify fleet aggregation**
  Fleet command produces correct merged output. Run `inspectah fleet` against multi-host scan data, verify merged snapshot, prevalence thresholds, and leaf/auto package union.

- [ ] **2. Test/fix/improve fleet refine mode**
  Aggregate data renders correctly in refine. Fleet-specific decisions (prevalence-based include/exclude) work. Verify fleet snapshots do NOT get single-machine grouping or leaf-only filtering.

- [ ] **3. Test/fix architect command UI**
  Web SPA for the architect command. Verify it loads, renders layers, and interactive editing works.

- [ ] **4. Functional parity check — Go matches Python**
  Compare Go and Python output on the same scan data. All sections should match (excluding "+ New File" which is a post-cutover feature). Check: Containerfile, kickstart, config files, services, users/groups, SELinux, secrets, non-RPM software.

## P0 — Cutover (Mark's steps, after verification)

- [ ] **5. Tag v0.7.0-rc1 on go-port**
- [ ] **6. Smoke test on real RHEL system**
- [ ] **7. Tag final container image v0.6.x-final on main**
- [ ] **8. Merge go-port to main, delete Python/container artifacts**
- [ ] **9. Tag v0.7.0**

## P1 — Post-cutover features

- [ ] **10. Secrets v2 implementation**
  Plan approved (revision 5). Kit implements. Spec in PKA.

- [ ] **11. Architect v2 — multi-artifact decomposition**
  Design spec exists: `docs/specs/proposed/2026-04-07-architect-v2-multi-artifact-design.md`. Plan approved. Ready to implement.

- [ ] **12. "+ New File" button in editor**
  Not started. Deferred from parity check — new feature, not parity.

- [ ] **13. COPR RPM packaging for Go binary**
  Not started. Go binary ships, COPR packages it for `dnf install inspectah`.

## P2 — Future

- [ ] **14. Secrets safety net**
  Needs design spec. Deferred from secrets v2.

- [ ] **15. Fleet prevalence-driven grouping**
  Follow-up to single-machine triage redesign. Apply grouping concepts to fleet mode with prevalence-aware accordion behavior.
