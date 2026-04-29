# Phase 2: Refine Server API -- Code Review

**Reviewer:** Thorn (Code Quality Engineer)
**Date:** 2026-04-28
**Scope:** d90f547..37dcd23 (single commit)
**Verdict:** Solid implementation. One Important issue, rest are Suggestions.

---

## What Was Done Well

- **Temp-copy-and-swap in nativeReRender is correct.** The pattern (copy to temp, clean, render, sync back, defer cleanup) provides genuine failure safety. If `renderer.RunAll()` fails, `defer os.RemoveAll(renderDir)` cleans up and outputDir is untouched. No race conditions in this path -- it runs synchronously within the HTTP handler.
- **syncRenderedOutput's 4-phase approach is sound.** Inventory, copy-with-atomic-rename-per-file, stale file removal, empty directory cleanup. The bottom-up directory sweep is correctly ordered (deepest first via reverse iteration).
- **Test quality is high.** Tests exercise real code paths -- `TestNativeReRender_ProducesCanonicalOutput` uses the actual renderer pipeline, not mocks. The E2E equality proof (comparing against a ground-truth render) and the failed-render working-dir-unchanged test are particularly strong.
- **render_id binding and revision guard are correctly mutex-protected.** Lock scopes are tight -- acquire, read/write, release -- no I/O under lock.
- **Sidecar exclusion is consistent** across cleanRendererOutputs, syncRenderedOutput, and RepackTarballFiltered.

---

## Important (should fix)

### 1. PUT /api/snapshot: revision incremented before disk write, no rollback on failure

**File:** `cmd/inspectah/internal/refine/server.go`, lines 362-384

The handler increments `h.revision` and releases the mutex (line 374), then attempts `os.WriteFile` (line 377). If the write fails, the revision counter is already advanced but the disk still holds the old snapshot. The client receives a 500 error but the next PUT will need revision N+1 (matching the phantom increment), while GET returns revision N+1 with old data.

**Fix:** Either (a) write to disk under the lock and roll back `h.revision` on failure, or (b) use write-then-increment ordering:

```go
// Write first, then increment on success
snapPath := filepath.Join(h.outputDir, "inspection-snapshot.json")
if err := os.WriteFile(snapPath, req.Snapshot, 0644); err != nil {
    h.mu.Unlock()
    h.sendError(w, 500, "failed to write snapshot")
    return
}
h.revision++
newRev := h.revision
h.mu.Unlock()
```

This holds the lock slightly longer but prevents phantom revision increments. Since this is a single-user local tool, the lock contention is negligible.

---

## Suggestions (nice to have)

### 2. Duplicate sidecar creation logic

Sidecar creation appears in both `RunRefine()` (line 108-113) and `newRefineHandler()` (line 228-234). The RunRefine path creates it before the initial re-render; newRefineHandler creates it again. Both are guarded by `os.IsNotExist` so this is safe, but the duplication means future changes to sidecar logic need updating in two places. Consider extracting to a shared `ensureSidecar(dir)` helper.

### 3. handleOptions does not include PUT in Allow header

**File:** `cmd/inspectah/internal/refine/server.go`, line 497

The OPTIONS handler advertises `GET, POST, OPTIONS` but the server now accepts PUT on `/api/snapshot`. Should be `GET, POST, PUT, OPTIONS` for correct CORS preflight behavior.

### 4. syncRenderedOutput partial-failure leaves dst in mixed state

If Phase 2 (copy) fails partway through, some files in dst will be updated while others retain old content. Phase 3/4 (stale removal) never runs. This is a narrow edge case (disk-full during sync) and the existing behavior is reasonable for a local CLI tool, but worth noting. A future enhancement could checkpoint Phase 2 and roll back on failure.

### 5. cleanRendererOutputs silences os.ReadDir errors

**File:** `cmd/inspectah/internal/cli/refine.go`, line 139

`entries, _ := os.ReadDir(dir)` discards the error. If the directory doesn't exist or is unreadable, the function silently does nothing. This is benign in the current call site (the dir was just created by copyDir), but returning an error would make the function safer for reuse.

### 6. Phase 4 sidecar-name check on directories is unnecessary

**File:** `cmd/inspectah/internal/cli/refine.go`, line 231

`filepath.Base(rel) == "original-inspection-snapshot.json"` is checked in the directory-cleanup phase, but a directory will never have that filename. Harmless but slightly confusing to read.

---

## Plan Alignment

The implementation matches the Phase 2 plan closely. All specified tasks are present:
- Revision tracking with optimistic locking (409 on stale)
- PUT /api/snapshot autosave
- render_id binding on render and tarball endpoints
- Sidecar management (create-once, exclude from tarball/sync)
- RepackTarballFiltered with exclusion set
- nativeReRender rewritten with temp-copy-and-swap
- cleanRendererOutputs + syncRenderedOutput helpers
- E2E equality proof, stale artifact disappearance, failed-render safety tests

One deviation: the plan mentions `renderer.ClassifySnapshot()` in the nativeReRender return, but the implementation returns `json.RawMessage("[]")` with a comment noting Phase 3 populates it. This is correct -- ClassifySnapshot is a Phase 3 deliverable.

---

## Summary

| Category | Count |
|----------|-------|
| Critical | 0 |
| Important | 1 |
| Suggestions | 5 |

The implementation is well-structured and the failure-safety model is sound. The one Important issue (phantom revision increment on write failure) is straightforward to fix. All tests pass, `go vet` is clean.
