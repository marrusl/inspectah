# Render Validation & E2E Test Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Revision 3** — addresses round 1 + round 2 plan review feedback.

**Goal:** Reject malformed render payloads at the server boundary, accept empty SystemType as a defensive fallback, add a server-owned reset endpoint, and add per-spec-file reset hooks to the Playwright e2e suite so the 21 currently-skipping tests pass.

**Architecture:** Validation gate (`Meta != nil`) added to `handleRender` and `handleSnapshot` PUT before processing. SystemType `""` maps to `SystemTypeUnknown` (bounded compatibility shim for legacy fixture data, not a new first-class producer semantic). New `POST /api/reset` endpoint restores from existing `original-inspection-snapshot.json` sidecar via the existing re-render path. E2E specs that mutate server state reset before/after via `resetServer()` helper.

**Tech Stack:** Go 1.23+, Playwright (TypeScript), `testify/assert` + `testify/require`

**Spec:** `docs/specs/proposed/2026-05-05-render-validation-test-isolation-design.md`

**Go module root:** `cmd/inspectah/` — all `go test` commands run from this directory, not the repo root.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `cmd/inspectah/internal/schema/types.go` | Modify | Add `SystemTypeUnknown`, accept `""` and `"unknown"` in unmarshal |
| `cmd/inspectah/internal/schema/types_test.go` | Modify | Add empty-string and `"unknown"` unmarshal test cases |
| `cmd/inspectah/internal/refine/server.go` | Modify | Add `Meta != nil` gate in `handleRender` and `handleSnapshot` PUT; add `handleReset` with route in `ServeHTTP` switch; register route in `newRefineHandler` |
| `cmd/inspectah/internal/refine/server_test.go` | Modify | Add tests for malformed rejection (render + PUT with disk proof), reset with dirty-then-restore proof, render-level SystemTypeUnknown proof |
| `tests/e2e-go/tests/helpers.ts` | Modify | Add `resetServer()` export |
| `tests/e2e-go/tests/accessibility.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/api-endpoints.spec.ts` | Modify | Add reset hooks, rewrite malformed + valid render tests |
| `tests/e2e-go/tests/artifact-truth.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/include-exclude.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/rebuild-cycle.spec.ts` | Modify | Add reset hooks |

---

### Task 1: Accept empty SystemType in schema + render-level proof

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:22-47`
- Modify: `cmd/inspectah/internal/schema/types_test.go:14-43`
- Modify: `cmd/inspectah/internal/refine/server_test.go`

**TDD note:** The render-level proof is written first because it compiles and runs on the current branch (it doesn't reference `SystemTypeUnknown`). It goes red now, proving the real compatibility seam is broken. Then the schema tests and fix are applied, turning everything green.

- [ ] **Step 1: Write the render-level proof (goes red on current branch)**

Add to `server_test.go`. The test's `reRenderFn` mirrors the real `nativeReRender` unmarshal step (`cli/refine.go:57-59`): it calls `json.Unmarshal(snapData, &snap)` into `schema.InspectionSnapshot` before proceeding. This is the exact code path that currently fails with `parse snapshot: unknown SystemType ""`.

```go
func TestRenderAPI_SystemTypeUnknown_GenericPath(t *testing.T) {
	dir := setupTestOutputDir(t)

	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		// Mirror the real nativeReRender unmarshal step (cli/refine.go:57-59).
		// This is the exact seam where SystemType "" currently fails.
		var snap schema.InspectionSnapshot
		if err := json.Unmarshal(snapData, &snap); err != nil {
			return ReRenderResult{}, fmt.Errorf("parse snapshot: %w", err)
		}
		return ReRenderResult{
			HTML: "<html>generic</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\n", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"meta":{"hostname":"test"},"system_type":""}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code,
		"empty system_type must not cause render failure after SystemTypeUnknown fix")
}
```

- [ ] **Step 2: Run render-level proof — must fail on current branch**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run TestRenderAPI_SystemTypeUnknown -v`
Expected: FAIL — gets `500` instead of `200`. The `reRenderFn` returns `parse snapshot: unknown SystemType ""` because the schema fix hasn't been applied yet. **If this passes, the test is not coupled to the real compatibility seam.**

- [ ] **Step 3: Write the schema-level tests**

Add a test case to the existing table in `types_test.go`. Insert this entry into the `tests` slice at line 21:

```go
{SystemTypeUnknown, `"unknown"`},
```

Below the existing table-driven loop (after line 36), add a new subtest for empty-string input:

```go
t.Run("empty-string-to-unknown", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`""`), &st)
    require.NoError(t, err)
    assert.Equal(t, SystemTypeUnknown, st)
})
```

Replace the existing "unknown must fail" test at lines 38-42 (after our change, `"unknown"` is a valid value that must round-trip):

```go
t.Run("unknown-string-accepted", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`"unknown"`), &st)
    require.NoError(t, err)
    assert.Equal(t, SystemTypeUnknown, st)
})

t.Run("bogus-value-rejected", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`"bogus-type"`), &st)
    assert.Error(t, err)
    assert.Contains(t, err.Error(), "unknown SystemType")
})
```

These won't compile yet (`SystemTypeUnknown` undefined).

- [ ] **Step 4: Apply the fix — add SystemTypeUnknown constant and update UnmarshalJSON**

In `types.go`, replace the const block at lines 22-26 with:

```go
const (
	SystemTypeUnknown     SystemType = "unknown"
	SystemTypePackageMode SystemType = "package-mode"
	SystemTypeRpmOstree   SystemType = "rpm-ostree"
	SystemTypeBootc       SystemType = "bootc"
)
```

Replace the `UnmarshalJSON` function at lines 35-47 with:

```go
func (s *SystemType) UnmarshalJSON(data []byte) error {
	var raw string
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	switch SystemType(raw) {
	case SystemTypePackageMode, SystemTypeRpmOstree, SystemTypeBootc, SystemTypeUnknown:
		*s = SystemType(raw)
	case "":
		*s = SystemTypeUnknown
	default:
		return fmt.Errorf("unknown SystemType %q", raw)
	}
	return nil
}
```

- [ ] **Step 5: Run all tests — schema + render-level proof must now pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/ -run TestSystemTypeJSON -v`
Expected: all subtests PASS.

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run TestRenderAPI_SystemTypeUnknown -v`
Expected: PASS — the same test that failed in Step 2 now succeeds because the unmarshal accepts `""`.

- [ ] **Step 6: Run full schema + refine test suites**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/ ./internal/refine/ -v`
Expected: all PASS, no regressions.

- [ ] **Step 7: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go cmd/inspectah/internal/refine/server_test.go
git commit -m "fix(schema): accept empty SystemType as defensive fallback

Empty system_type now unmarshals to SystemTypeUnknown instead of
hard-failing. Bounded compatibility shim for legacy fixture data that
omits the field. Includes schema-level and render-level proof.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 2: Add Meta validation gate to handleRender

**Files:**
- Modify: `cmd/inspectah/internal/refine/server.go:479-536`
- Modify: `cmd/inspectah/internal/refine/server_test.go`

- [ ] **Step 1: Write the failing test for malformed render rejection**

Add to `server_test.go` after the existing `TestRenderAPI_FailedRender_EntireWorkingDirUnchanged` function:

```go
func TestRenderAPI_MalformedSnapshot_Rejected(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		t.Fatal("reRenderFn must not be called for malformed snapshots")
		return ReRenderResult{}, nil
	})

	// Record state before
	beforeFiles := snapshotDirContents(t, dir)

	// POST a payload with no meta field
	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"not_valid": true}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 400, w.Code)
	var body map[string]interface{}
	require.NoError(t, json.Unmarshal(w.Body.Bytes(), &body))
	assert.Contains(t, body["error"], "missing meta")

	// Working directory must be unchanged
	afterFiles := snapshotDirContents(t, dir)
	assert.Equal(t, beforeFiles, afterFiles,
		"working directory must be unchanged after malformed render rejection")
}

func TestRenderAPI_EmptyMeta_Accepted(t *testing.T) {
	dir := setupTestOutputDir(t)
	renderCalled := false
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		renderCalled = true
		return ReRenderResult{
			HTML: "<html>ok</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\n", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	// POST with empty meta (valid minimal snapshot)
	req := httptest.NewRequest("POST", "/api/render",
		strings.NewReader(`{"snapshot": {"meta": {}}}`))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.True(t, renderCalled, "reRenderFn must be called for valid snapshots")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run "TestRenderAPI_Malformed|TestRenderAPI_EmptyMeta" -v`
Expected: `TestRenderAPI_MalformedSnapshot_Rejected` FAILS (gets 200 instead of 400, and `t.Fatal` fires because the render function is called). `TestRenderAPI_EmptyMeta_Accepted` PASSES (mock render always succeeds).

- [ ] **Step 3: Add the validation gate to handleRender**

In `server.go`, insert the following block between line 512 (end of wrapper parsing) and line 514 (`result, err := safeReRender(...)`):

```go
	var probe schema.InspectionSnapshot
	if err := json.Unmarshal(snapData, &probe); err != nil {
		h.sendError(w, 400, "invalid snapshot: "+err.Error())
		return
	}
	if probe.Meta == nil {
		h.sendError(w, 400, "invalid snapshot: missing meta field")
		return
	}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run "TestRenderAPI_Malformed|TestRenderAPI_EmptyMeta" -v`
Expected: both PASS.

- [ ] **Step 5: Run full refine test suite to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -v`
Expected: all PASS. The existing `TestRenderAPI_FailedRender_EntireWorkingDirUnchanged` test uses `{"meta":{}}` which passes the gate.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "fix(refine): reject malformed snapshots in POST /api/render

Add Meta != nil validation gate after wrapper parsing and before
calling safeReRender. Junk payloads like {not_valid: true} now return
400 instead of producing empty rendered output. Existing minimal
snapshots with {meta: {}} are still accepted.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 3: Add Meta validation gate to handleSnapshot PUT

**Files:**
- Modify: `cmd/inspectah/internal/refine/server.go:393-402`
- Modify: `cmd/inspectah/internal/refine/server_test.go`

- [ ] **Step 1: Add "missing meta" case to the existing malformed PUT table test**

The existing `TestAPISnapshot_PutMalformedSnapshot` at line 435 uses a table-driven pattern and already asserts disk non-advance. Extend the table with a "missing meta" case. Add this entry to the `tests` slice inside that function:

```go
{"valid JSON but missing meta", `{"snapshot": {"not_valid": true}, "revision": 1}`},
```

- [ ] **Step 2: Run test to verify the new case fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run TestAPISnapshot_PutMalformedSnapshot -v`
Expected: the new "valid JSON but missing meta" subtest FAILS (gets 200 instead of 400).

- [ ] **Step 3: Add the Meta gate to handleSnapshot PUT**

In `server.go`, after the existing unmarshal block at line 398-402:

```go
var validSnap schema.InspectionSnapshot
if err := json.Unmarshal(req.Snapshot, &validSnap); err != nil {
    h.sendError(w, 400, "invalid snapshot: "+err.Error())
    return
}
```

Add immediately after (after line 402):

```go
if validSnap.Meta == nil {
    h.sendError(w, 400, "invalid snapshot: missing meta field")
    return
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run TestAPISnapshot_PutMalformedSnapshot -v`
Expected: all subtests PASS including the new "valid JSON but missing meta" case. The existing disk-state assertion at line 460-464 verifies `inspection-snapshot.json` was not modified.

- [ ] **Step 5: Run full refine test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -v`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "fix(refine): reject malformed snapshots in PUT /api/snapshot

Same Meta != nil gate as the render endpoint. PUT remains save-only
with no render dependency. Extends existing malformed PUT table test
with disk non-advance proof.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Add POST /api/reset endpoint

**Files:**
- Modify: `cmd/inspectah/internal/refine/server.go` (route in `ServeHTTP` switch + handler)
- Modify: `cmd/inspectah/internal/refine/server_test.go`

- [ ] **Step 1: Write the failing test for reset with dirty-then-restore proof**

Add to `server_test.go`. This test first mutates working state away from the sidecar, then resets and asserts sidecar restoration:

```go
func TestResetAPI_RestoresSidecarState(t *testing.T) {
	dir := setupTestOutputDir(t)

	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		// Write the snapshot to the working file (simulating a real render)
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		return ReRenderResult{
			HTML: "<html>rendered</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\nRUN echo rendered", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	// Capture the sidecar content (created by newRefineHandler)
	sidecarPath := filepath.Join(dir, "original-inspection-snapshot.json")
	sidecarData, err := os.ReadFile(sidecarPath)
	require.NoError(t, err)

	// Step 1: Dirty the working snapshot so it diverges from the sidecar
	dirtySnap := []byte(`{"meta":{"hostname":"DIRTY-STATE"}}`)
	require.NoError(t, os.WriteFile(filepath.Join(dir, "inspection-snapshot.json"), dirtySnap, 0644))

	// Verify working state is now different from sidecar
	workingData, _ := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NotEqual(t, string(sidecarData), string(workingData), "working state must differ from sidecar before reset")

	// Step 2: POST /api/reset
	reqReset := httptest.NewRequest("POST", "/api/reset", nil)
	wReset := httptest.NewRecorder()
	handler.ServeHTTP(wReset, reqReset)

	assert.Equal(t, 200, wReset.Code)

	var resetResp map[string]interface{}
	require.NoError(t, json.Unmarshal(wReset.Body.Bytes(), &resetResp))
	assert.Equal(t, "reset", resetResp["status"])
	assert.NotEmpty(t, resetResp["render_id"])

	// Step 3: Assert working snapshot was restored to sidecar content
	restoredData, err := os.ReadFile(filepath.Join(dir, "inspection-snapshot.json"))
	require.NoError(t, err)
	assert.JSONEq(t, string(sidecarData), string(restoredData),
		"working snapshot must match sidecar after reset")

	// Step 4: Assert sidecar itself was not modified
	sidecarAfter, err := os.ReadFile(sidecarPath)
	require.NoError(t, err)
	assert.Equal(t, string(sidecarData), string(sidecarAfter),
		"sidecar must remain unchanged after reset")
}

func TestResetAPI_FailedRender_WorkingDirUnchanged(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		return ReRenderResult{}, fmt.Errorf("reset render exploded")
	})

	beforeFiles := snapshotDirContents(t, dir)

	req := httptest.NewRequest("POST", "/api/reset", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 500, w.Code)

	afterFiles := snapshotDirContents(t, dir)
	assert.Equal(t, beforeFiles, afterFiles,
		"working directory must be unchanged after failed reset")
}

func TestResetAPI_MethodNotAllowed(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	req := httptest.NewRequest("GET", "/api/reset", nil)
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, req)

	assert.Equal(t, 405, w.Code)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run "TestResetAPI" -v`
Expected: all three FAIL — requests hit the `default` case in `ServeHTTP` and serve a static file (likely 200 with HTML or 404), not the expected handler responses.

- [ ] **Step 3: Add the route to ServeHTTP switch AND implement handleReset**

In `server.go`, add the route case in the `ServeHTTP` switch (around line 297, after the `api/quadlet-draft` case):

```go
	case path == "/api/reset":
		h.handleReset(w, r)
```

Add the handler function after `handleRender` (after line 536):

```go
func (h *refineHandler) handleReset(w http.ResponseWriter, r *http.Request) {
	if r.Method != "POST" {
		h.sendError(w, 405, "method not allowed")
		return
	}

	if h.reRenderFn == nil {
		h.sendError(w, 503, "re-rendering not available")
		return
	}

	sidecarPath := filepath.Join(h.outputDir, "original-inspection-snapshot.json")
	snapData, err := os.ReadFile(sidecarPath)
	if err != nil {
		h.sendError(w, 500, "failed to read original snapshot: "+err.Error())
		return
	}

	result, err := safeReRender(h.reRenderFn, snapData, nil, h.outputDir)
	if err != nil {
		h.sendError(w, 500, "reset re-render failed: "+err.Error())
		return
	}
	_ = result

	h.mu.Lock()
	h.revision++
	h.renderID = generateRenderID()
	rev := h.revision
	rid := h.renderID
	h.mu.Unlock()

	h.sendJSON(w, 200, map[string]interface{}{
		"status":    "reset",
		"revision":  rev,
		"render_id": rid,
	})
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -run "TestResetAPI" -v`
Expected: all three PASS.

- [ ] **Step 5: Run full refine test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/refine/ -v`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "feat(refine): add POST /api/reset endpoint

Restores the server to startup state by re-rendering from the
existing original-inspection-snapshot.json sidecar. Uses the same
temp-render + sync-on-success path as handleRender. Bumps both
revision and render_id on success. Route wired through both
ServeHTTP switch and mux registration.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Add resetServer helper to e2e test helpers

**Files:**
- Modify: `tests/e2e-go/tests/helpers.ts`

- [ ] **Step 1: Add the resetServer export**

Add at the end of `helpers.ts` (after the `architectURL` function):

```typescript
export async function resetServer(baseURL?: string): Promise<void> {
  const url = baseURL || process.env.REFINE_FLEET_URL || 'http://localhost:9200';
  const resp = await fetch(`${url}/api/reset`, { method: 'POST' });
  if (!resp.ok) throw new Error(`Server reset failed: ${resp.status}`);
}
```

- [ ] **Step 2: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/helpers.ts
git commit -m "test(e2e): add resetServer helper for per-spec state isolation

Calls POST /api/reset to restore the refine server to its startup
state from the immutable sidecar snapshot.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 6: Add reset hooks to mutating spec files

**Files:**
- Modify: `tests/e2e-go/tests/accessibility.spec.ts`
- Modify: `tests/e2e-go/tests/artifact-truth.spec.ts`
- Modify: `tests/e2e-go/tests/include-exclude.spec.ts`
- Modify: `tests/e2e-go/tests/rebuild-cycle.spec.ts`

- [ ] **Step 1: Add reset hooks to accessibility.spec.ts**

Add `resetServer` to the import at line 7:

```typescript
import { waitForBoot, navigateToSection, resetServer } from './helpers';
```

Add file-level hooks. `accessibility.spec.ts` has three `test.describe` blocks. The cleanest approach is to add hooks to each block that contains mutating tests. The mutating tests are in the `'Keyboard navigation'` (Enter/Space toggle) and `'Live region announcements'` (toggle + rebuild) blocks. The `'ARIA landmarks and attributes'` block is read-only.

In `'Keyboard navigation'` (line 89), add before the `beforeEach`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

In `'Live region announcements'` (line 180), add before the `beforeEach`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

- [ ] **Step 2: Add reset hooks to artifact-truth.spec.ts**

Add `resetServer` to the import at line 13:

```typescript
import { waitForRefineBoot, navigateToSection, findToggleInSection, resetServer } from './helpers';
```

Add hooks inside the `test.describe` block at line 15, before `test.use`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

- [ ] **Step 3: Add reset hooks to include-exclude.spec.ts**

Add `resetServer` to the import at line 13:

```typescript
import { waitForRefineBoot, navigateToSection, resetServer } from './helpers';
```

Add hooks inside the `test.describe` block at line 53, before `beforeEach`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

- [ ] **Step 4: Add reset hooks to rebuild-cycle.spec.ts**

Add `resetServer` to the import at line 11:

```typescript
import { waitForBoot, navigateToSection, isRefineMode, findToggleInSection, resetServer } from './helpers';
```

Add hooks inside the `test.describe` block at line 13, before `beforeEach`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

- [ ] **Step 5: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/accessibility.spec.ts tests/e2e-go/tests/artifact-truth.spec.ts tests/e2e-go/tests/include-exclude.spec.ts tests/e2e-go/tests/rebuild-cycle.spec.ts
git commit -m "test(e2e): add reset hooks to mutating spec files

beforeAll/afterAll resetServer() on accessibility (keyboard nav +
live region blocks only), artifact-truth, include-exclude, and
rebuild-cycle specs. Read-only describe blocks are not hooked.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Rewrite api-endpoints malformed and valid render tests

**Files:**
- Modify: `tests/e2e-go/tests/api-endpoints.spec.ts`

- [ ] **Step 1: Add resetServer import and hooks**

Add a helpers import after line 5:

```typescript
import { test, expect } from '@playwright/test';
import { resetServer } from './helpers';
```

Add reset hooks inside the first `test.describe` block (line 7), before the first test:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

- [ ] **Step 2: Replace the valid render test (line 45)**

Replace the test at line 45-73 with the strict version:

```typescript
  test('POST /api/render accepts valid snapshot with 200', async ({ request }) => {
    const snapResp = await request.get('/api/snapshot');
    const snapBody = await snapResp.json();

    const renderResp = await request.post('/api/render', {
      data: { snapshot: snapBody.snapshot },
    });

    expect(renderResp.status()).toBe(200);
    const body = await renderResp.json();
    expect(body.html).toBeDefined();
    expect(body.containerfile).toBeDefined();
    expect(body.containerfile).toContain('FROM');
    expect(body.render_id).toBeDefined();
    expect(body.revision).toBeGreaterThan(0);
    expect(body.triage_manifest).toBeDefined();
  });
```

- [ ] **Step 3: Replace the malformed render test (line 75)**

Replace the test at line 75-94 with the strict version:

```typescript
  test('POST /api/render rejects malformed payload with 400', async ({ request }) => {
    const snapBefore = await request.get('/api/snapshot');
    const revisionBefore = (await snapBefore.json()).revision;

    const renderResp = await request.post('/api/render', {
      data: { snapshot: { not_valid: true } },
    });

    expect(renderResp.status()).toBe(400);
    const body = await renderResp.json();
    expect(body.error).toBeDefined();
    expect(typeof body.error).toBe('string');

    const snapAfter = await request.get('/api/snapshot');
    const revisionAfter = (await snapAfter.json()).revision;
    expect(revisionAfter).toBe(revisionBefore);
  });
```

- [ ] **Step 4: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add tests/e2e-go/tests/api-endpoints.spec.ts
git commit -m "test(e2e): strict render contract assertions + reset hooks

Malformed render asserts 400 and revision-unchanged. Valid render
asserts 200 with full response shape (no error branch). Reset hooks
prevent cross-spec state contamination.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 8: Verification

**Files:** None (verification only)

- [ ] **Step 1: Run Go unit tests**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/cmd/inspectah && go test ./internal/schema/ ./internal/refine/ -v`
Expected: all PASS.

- [ ] **Step 2: Run the targeted isolation probe**

This is the primary e2e proof for this plan's stated regression:

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test tests/api-endpoints.spec.ts tests/include-exclude.spec.ts --reporter=list`
Expected: all tests pass — include-exclude finds cards after api-endpoints runs.

- [ ] **Step 3: Run the secondary isolation probe**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test tests/api-endpoints.spec.ts tests/editor.spec.ts --reporter=list`
Expected: all editor tests pass (no skips) after api-endpoints runs.

- [ ] **Step 4: Run the full Playwright suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test --reporter=list`

Note: the full suite may have pre-existing failures unrelated to this plan (server-liveness / connection-refused issues identified during Kit's review). The acceptance criteria for THIS plan are:

- The 21 previously-skipping tests (editor, include-exclude, triage-cards) now pass
- The malformed render test asserts 400 (not 200)
- The valid render test asserts 200 (not 500)
- No new test failures introduced

If the suite has broader failures beyond the 21 targeted skips, those are pre-existing and tracked separately.

- [ ] **Step 5: Run the full suite two more times for determinism**

Run the full suite two more times. The targeted tests should produce identical results each time.
