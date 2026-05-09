# E2E Test Suite Coverage Reconciliation

> Reconciled against the March 31 interaction test matrix
> (`docs/specs/proposed/2026-03-31-playwright-e2e-test-suite-design.md`).
>
> Last updated: 2026-05-09

## Suite Inventory

**14 spec files, 125 test blocks** (before runtime skips).

The March 31 design specified 15 spec files (10 refine, 5 architect).
The current suite has 14 files with a different decomposition --
some planned specs were consolidated, others were added as new
behavioral proof areas not in the original matrix.

| Spec File | test() | describe() | Mode |
|-----------|--------|-----------|------|
| smoke.spec.ts | 6 | 1 | Refine |
| section-navigation.spec.ts | 5 | 1 | Refine |
| theme-switching.spec.ts | 5 | 1 | Refine |
| include-exclude.spec.ts | 6 | 1 | Refine |
| triage-cards.spec.ts | 5 | 1 | Refine |
| containerfile-preview.spec.ts | 6 | 1 | Refine |
| rebuild-cycle.spec.ts | 4 | 1 | Refine |
| editor.spec.ts | 10 | 1 | Refine |
| api-endpoints.spec.ts | 9 | 2 | Refine |
| accessibility.spec.ts | 18 | 3 | Refine |
| artifact-truth.spec.ts | 4 | 1 | Refine |
| fleet-threshold-action-bar.spec.ts | 12 | 1 | Refine |
| fleet-threshold-no-dirty-state.spec.ts | 2 | 1 | Refine |
| architect-smoke.spec.ts | 33 | 6 | Architect |
| **Total** | **125** | **23** | |

---

## Planned vs. Actual: Refine Specs

The March 31 design defined 10 refine spec files. The table below maps
each planned spec to its actual coverage status.

### Covered (full or substantial)

| Planned Spec | Actual Spec | Assessment |
|-------------|-------------|------------|
| `section-navigation.spec.ts` | `section-navigation.spec.ts` | **Covered.** Sidebar nav clicks, active state via `aria-current`, section heading visibility, conditional sections. |
| `include-exclude.spec.ts` | `include-exclude.spec.ts` | **Covered.** Toggle include/exclude, dirty state activates rebuild, re-render reflects toggle in Containerfile. Also covers card-based include/exclude via triage-cards. |
| `theme-switching.spec.ts` | `theme-switching.spec.ts` | **Covered.** Theme toggle, class change, localStorage persistence, badge contrast in light mode. |
| `re-render-cycle.spec.ts` | `rebuild-cycle.spec.ts` | **Covered (renamed).** Toggle item, rebuild, verify Containerfile updates, re-render badge count reset. |

### Partially covered (key seams present, some gaps)

| Planned Spec | Actual Coverage | What is covered | What is missing |
|-------------|----------------|-----------------|-----------------|
| `summary-dashboard.spec.ts` | `smoke.spec.ts` | Page load, title, sidebar rendering, heading presence, theme default, basic structure. | No 4-card grid assertion, no card count/label checks, no Needs Attention card, no tie callout, no drift callout, no single-host 3-card variant. The summary dashboard interaction coverage is smoke-level, not behavioral. |
| `config-editor.spec.ts` | `editor.spec.ts` | Tab bar rendering, ARIA tablist, tab switching, file list with `role="option"`, CodeMirror presence, edit button and save. | No end-to-end edit-save-rerender proof (edit content, save, rebuild, verify Containerfile reflects edit). The editor tests prove the UI scaffold but not the data flow. |

### Not covered (planned but absent)

| Planned Spec | Status | Impact |
|-------------|--------|--------|
| `prevalence-slider.spec.ts` | **Not implemented.** | The original plan tested: slider drag updates card counts, preview-state dashed border appears on deviation, prevalence badges sync in section headers, returning to original clears dirty state. None of these flows exist in the current suite. Note: the `fleet-threshold-*` specs test a related but different feature (the threshold dropdown and action bar), not the prevalence slider itself. |
| `variant-selection.spec.ts` | **Not implemented.** | Planned tests: 2-way tie Compare buttons, 3-way tie Display buttons, radio selection persistence through re-render, minority selection sticks, tie count resolution. No variant selection coverage exists. |
| `fleet-popovers.spec.ts` | **Not implemented.** | Planned tests: fleet bar click opens PF6 popover with host breakdown, click outside closes, fleet bar active outline state. No fleet popover coverage exists in refine specs. |
| `keyboard-nav.spec.ts` | **Partially absorbed.** | The planned dedicated keyboard-nav spec (prevalence badge focus+Enter, priority row focus+Enter) does not exist as a standalone file. However, `accessibility.spec.ts` now includes behavioral keyboard tests: Enter/Space activation of `role="switch"` toggles, editor tab bar keyboard model, and skip link focus. This is partial coverage -- the planned prevalence badge and priority row keyboard flows are still missing. |

---

## Planned vs. Actual: Architect Specs

The March 31 design defined 5 architect spec files. The current suite
consolidates all architect coverage into a single `architect-smoke.spec.ts`
with 33 tests across 6 describe blocks.

| Planned Spec | Actual Coverage | Assessment |
|-------------|----------------|------------|
| `layer-decomposition.spec.ts` | `architect-smoke.spec.ts` > "Architect server smoke tests" + "Architect layer tree" | **Substantially covered.** Base/derived layer rendering, three-column layout, layer cards with package count badges, layer selection. Missing: explicit shared-package-in-base assertion, package count matching against fixture expectations. |
| `package-move.spec.ts` | `architect-smoke.spec.ts` > "Architect behavioral workflows" + "Architect API endpoints" | **Substantially covered.** Move-up button with toast notification, copy endpoint via API, move endpoint via API with topology validation, source layer package count preservation after copy. |
| `containerfile-preview.spec.ts` | `architect-smoke.spec.ts` > "Architect UI interactions" | **Partially covered.** Preview button opens modal with Containerfile `FROM` content, Escape closes modal. Missing: per-layer `dnf install` line validation, content update after package moves. |
| `export.spec.ts` | `architect-smoke.spec.ts` > "Architect API endpoints" + "Architect UI interactions" | **Partially covered.** Export API endpoint returns gzip with correct headers. Toolbar has export button. Missing: UI-driven download trigger test, exported content validation (Containerfiles per layer). |
| `impact-tooltips.spec.ts` | `architect-smoke.spec.ts` > "Architect fleet sidebar" | **Partially covered.** Fleet cards render, fleet card badges have image count and turbulence arrow text. Missing: explicit `.impact-badge` title attribute with fan-out count, hover-triggered tooltip display, layer-level badge summary. |

---

## New Specs Not in Original Matrix

The following spec files were added beyond the March 31 plan. These
represent genuine behavioral proof areas that strengthen the suite,
even though they were not in the original matrix.

| Spec File | Purpose | Value |
|-----------|---------|-------|
| `api-endpoints.spec.ts` | REST API contract tests (health, snapshot, render, tarball, 404, malformed payload rejection) | High -- proves server API contracts independently of UI. |
| `accessibility.spec.ts` | ARIA landmarks, role/attribute validation, keyboard activation (Enter/Space on switches), editor tab keyboard model, skip link, live regions | High -- fills the a11y gap the original matrix did not address. |
| `artifact-truth.spec.ts` | Three-way equality proof: UI preview === API response === tarball Containerfile | High -- proves render pipeline integrity end-to-end. |
| `triage-cards.spec.ts` | Triage card rendering, include/exclude via card buttons, section-specific card scoping | Medium -- extends include-exclude coverage to the card UI. |
| `fleet-threshold-action-bar.spec.ts` | `makeDecision()` priorValues preservation, bulk flip all/none/defaults, undo interaction with bulk flips, "apply suggestion" flow | High -- proves threshold action bar behavioral contracts. |
| `fleet-threshold-no-dirty-state.spec.ts` | Threshold dropdown does not mutate include state, does not increment change counter, does not lose focus | High -- proves presentation-only invariant. |

---

## Gaps Summary

### Must-fill before cutover

These are interaction seams that expert users depend on and that have
zero test coverage. They should be implemented before the suite can
claim broad UX coverage.

1. **Prevalence slider interactions** -- slider drag updates counts,
   preview-state border, badge sync, dirty state management.
   The threshold dropdown tests (fleet-threshold-*) are related but
   test a different control.

2. **Variant selection workflow** -- tie detection UI (Compare/Display
   buttons), radio selection, persistence through re-render, tie count
   resolution. This is a core fleet workflow with no coverage.

3. **Fleet popovers** -- fleet bar click opens popover with host
   breakdown, click outside closes, active state management. No
   coverage exists.

### Should-fill (quality gaps in covered areas)

4. **Summary dashboard behavioral depth** -- the smoke spec proves the
   page loads but does not validate card counts, labels, tie callouts,
   or the single-host variant. Upgrade smoke-level checks to
   behavioral assertions.

5. **Editor data flow proof** -- the editor spec proves the UI
   scaffold but does not complete the edit-save-rebuild-verify loop.
   Add an end-to-end edit flow test.

6. **Architect Containerfile preview depth** -- modal opens and shows
   `FROM`, but per-layer `dnf install` validation and post-move content
   updates are missing.

7. **Architect export download trigger** -- API-level export is tested
   but the UI download flow (click button, verify download event) is
   not.

8. **Impact tooltip behavior** -- fleet card badges are checked for
   text content but explicit `.impact-badge` title attributes and
   hover-triggered tooltip display are not tested.

### Intentionally deferred

9. **Keyboard navigation for prevalence badges and priority rows** --
   the planned keyboard-nav.spec.ts is partially absorbed into
   accessibility.spec.ts (which tests Enter/Space on switches and
   editor tab keyboard). The prevalence badge and priority row keyboard
   flows depend on the prevalence slider feature (gap 1) being
   implemented first. Defer until gap 1 is filled.

10. **Single-host refine server** -- the March 31 plan included a
    separate single-host server on port 9201 with dedicated test paths
    in summary-dashboard.spec.ts. The current suite runs
    api-endpoints.spec.ts against a single-host URL but does not
    exercise single-host-specific UI behavior. This is acceptable for
    now because the single-host path is a strict subset of the fleet
    path (fewer cards, no prevalence UI).

---

## Correction to Fern Round 1 Review

Fern's review (2026-05-05) stated "11 spec files and 70 test blocks"
and noted "no page.keyboard, no .press(), no Tab walking." The suite
has since been extended:

- **14 spec files, 125 test blocks** (was 11/70 at review time)
- `accessibility.spec.ts` now includes `page.keyboard.press('Enter')`,
  `page.keyboard.press('Space')`, and `.focus()` calls
- `architect-smoke.spec.ts` includes `page.keyboard.press('Escape')`
- `fleet-threshold-action-bar.spec.ts` and
  `fleet-threshold-no-dirty-state.spec.ts` are new since the review

These additions address Fern's must-fix #2 (accessibility checks markup
only) partially -- behavioral keyboard tests exist but are not
comprehensive across all keyboard contracts.

---

## Design Spec Status

The March 31 design spec at
`docs/specs/proposed/2026-03-31-playwright-e2e-test-suite-design.md`
remains in the `proposed/` directory. Its test coverage matrix no longer
accurately describes the implemented suite. Options:

1. **Move to `implemented/` with amendments** noting the actual file
   structure, consolidated architect spec, and deferred/gap items.
2. **Leave in `proposed/` and treat this COVERAGE.md as the
   authoritative coverage reference** for the Go-port e2e suite.

Recommendation: option 2. The design spec captures the original intent
and is useful as a reference for what was planned. This COVERAGE.md is
the live document that tracks what actually exists.
