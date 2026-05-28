# Code Review: Live Containerfile Preview

**Reviewer:** Code Review Agent
**Date:** 2026-04-01
**Branch:** `feat/live-containerfile-preview` (7 commits, `dcf83e6..c608e9b`)
**Spec:** `docs/specs/proposed/2026-03-31-live-containerfile-preview-design.md`

---

## Spec Compliance Status: PASS (with minor gaps)

The implementation faithfully delivers the spec's core requirements. All major features are present and correctly wired:

- Client-side Containerfile preview generator with all 11 sections
- Toolbar restructuring (Rebuild & Download, Discard with confirmation dialog)
- Copy button removal from Containerfile tab
- Preview cues on both Containerfile and Audit tabs (exact copy matches spec)
- Hooks into all 6 event paths (include toggle, variant selection, config editor save, prevalence slider, discard, page load)
- Dirty state lifecycle with deep clone of originalSnapshot
- Baseline refresh on successful Rebuild & Download
- DOMParser-based in-place update (no `document.write()`)
- `textContent` used for all preview output (injection-safe)

---

## Issues

### Important (should fix)

**1. Services section does not react to include/exclude toggles**

The spec says services should be generated from `snapshot.services.state_changes` where `include === true`, grouped by action. The implementation instead uses the pre-computed `enabled_units` / `disabled_units` flat string arrays (lines 912-913 of `_js.html.j2`), which are static lists set at inspection time and do not carry `include` flags. When a user toggles a service off, the preview does not reflect that change.

This is consistent with how the Python renderer works (it also uses `enabled_units`/`disabled_units` directly), so the preview is not misleading relative to what Rebuild & Download would produce. However, it means the services section of the preview is not reactive -- toggling services in the UI has no visible effect on the Containerfile preview.

**Recommendation:** Either (a) filter `state_changes` by `include === true` and group by `action` to generate `RUN systemctl enable/disable` lines (matching the spec), or (b) document this as an intentional simplification in the spec. Option (b) is reasonable given that the Python renderer has the same behavior.

**2. DOMParser snapshot extraction uses `doc.querySelector('script')` (fragile)**

Line 742: `var scriptTag = doc.querySelector('script')` gets only the first `<script>` tag. Currently this works because the snapshot declaration is in the first script tag. If the HTML template structure changes (e.g., an inline script is added before the main block), this extraction would silently fail to update the snapshot, leading to stale state after Rebuild & Download.

**Recommendation:** Use a more targeted selector. For example, add a `data-snapshot-script` attribute to the main script tag and select with `doc.querySelector('script[data-snapshot-script]')`, or search all script tags for the one containing `var snapshot =`.

### Minor (nice to have)

**3. Stale `.btn-copy-cf` reference in toolbar template comment**

`_toolbar.html.j2` line 41 still lists `.btn-copy-cf` in the JS-targeted class inventory comment. The class no longer exists. Cosmetic only.

**4. Test gap: "Audit counts update on prevalence slider" not covered**

The spec's test table includes "Audit counts update on prevalence slider" but no E2E test covers this. The existing prevalence slider test (line 111 of `live-preview.spec.ts`) only asserts the Containerfile updates, not audit summary counts.

**5. Test gap: "After Rebuild & Download, preview matches export" is indirect**

The spec test table calls for asserting that `#containerfile-pre` matches the server-rendered content after Rebuild & Download. The test at line 150 tests the Discard-after-rebuild baseline behavior (which implicitly requires the Containerfile to have been updated), but does not directly assert that the preview text matches the Python-rendered output.

**6. Prevalence slider and variant selection test assertions are weak**

The prevalence slider test (line 125) and variant selection test (line 108) both assert `expect(updatedText).toBeTruthy()` rather than asserting the text actually changed. If `initialText` was already truthy and the toggle had no effect, these tests would pass vacuously.

---

## Risk Assessment

**Low risk:**
- The DOMParser approach for Rebuild & Download is sound. The in-place update correctly preserves event listeners, dirty state, and client-side state that `document.write()` would destroy.
- The deep clone via `JSON.parse(JSON.stringify())` is the correct approach for preventing reference sharing.
- The `textContent` assignment prevents XSS.
- The Discard confirmation dialog correctly gates on dirty state.

**Medium risk:**
- The DOMParser snapshot extraction (issue #2) could silently fail if template structure changes. The regex-based JSON extraction from script content is inherently fragile. A future template refactor could break this without obvious test failures.
- The services section non-reactivity (issue #1) could confuse users who toggle services off and see no change in the Containerfile preview.

**No regressions identified:**
- Existing Python tests updated correctly for the new `JSON.parse(JSON.stringify(snapshot))` pattern (exactly 1 occurrence in rebuild handler, not on initial load).
- Existing E2E re-render tests updated for new button labels.
- No server-side pipeline changes.

---

## What Was Done Well

- **Clean commit progression:** Each commit is atomic and reviewable. The fix commit (`ea6e53e`) for the redundant preview call shows good attention to behavioral correctness.
- **Defensive coding:** Null checks throughout (`if (!s) return`, `|| []` fallbacks, `try/catch` around JSON.parse). The generator handles minimal snapshots gracefully.
- **Spec fidelity:** Preview cue copy matches the spec character-for-character. Dialog copy matches. Trust model is correctly implemented (preview vs. export separation).
- **Existing test updates:** Both `test_editor.py` and `test_html_report_output.py` are updated with clear comments explaining why the `JSON.parse(JSON.stringify(snapshot))` pattern now appears once (the rebuild handler baseline refresh).
- **CSS:** Dark theme support for the discard dialog. Preview cue styling is appropriately low-weight.

---

## Overall Assessment

**Ready to merge** with the understanding that issues #1 and #2 should be tracked for follow-up. The implementation is well-structured, defensively coded, and delivers the spec's core value proposition (eliminating the re-render cycle for preview). The test coverage is good though not exhaustive against the spec's test table.
