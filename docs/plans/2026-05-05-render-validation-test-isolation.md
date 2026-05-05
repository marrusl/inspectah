# Render Validation & E2E Test Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reject malformed render payloads at the server boundary, accept empty SystemType as a defensive fallback, add a server-owned reset endpoint, and add per-spec-file reset hooks to the Playwright e2e suite so the 21 currently-skipping tests pass.

**Architecture:** Validation gate (`Meta != nil`) added to `handleRender` and `handleSnapshot` PUT before processing. SystemType `""` maps to `SystemTypeUnknown`. New `POST /api/reset` endpoint restores from existing `original-inspection-snapshot.json` sidecar via the existing re-render path. E2E specs that mutate server state reset before/after via `resetServer()` helper.

**Tech Stack:** Go 1.23+, Playwright (TypeScript), `testify/assert` + `testify/require`

**Spec:** `docs/specs/proposed/2026-05-05-render-validation-test-isolation-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `cmd/inspectah/internal/schema/types.go` | Modify | Add `SystemTypeUnknown`, accept `""` and `"unknown"` in unmarshal |
| `cmd/inspectah/internal/schema/types_test.go` | Modify | Add empty-string and `"unknown"` unmarshal test cases |
| `cmd/inspectah/internal/refine/server.go` | Modify | Add `Meta != nil` gate in `handleRender` and `handleSnapshot` PUT; add `handleReset` |
| `cmd/inspectah/internal/refine/server_test.go` | Modify | Add tests for malformed rejection, PUT validation, and reset endpoint |
| `tests/e2e-go/tests/helpers.ts` | Modify | Add `resetServer()` export |
| `tests/e2e-go/tests/accessibility.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/api-endpoints.spec.ts` | Modify | Add reset hooks, rewrite malformed + valid render tests |
| `tests/e2e-go/tests/artifact-truth.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/include-exclude.spec.ts` | Modify | Add reset hooks |
| `tests/e2e-go/tests/rebuild-cycle.spec.ts` | Modify | Add reset hooks |

---

### Task 1: Accept empty SystemType in schema

**Files:**
- Modify: `cmd/inspectah/internal/schema/types.go:22-47`
- Modify: `cmd/inspectah/internal/schema/types_test.go:14-43`

- [ ] **Step 1: Write the failing test for empty string**

Add a test case to the existing table in `types_test.go`. Insert this entry into the `tests` slice at line 21:

```go
{SystemTypeUnknown, `"unknown"`},
```

This will fail with a compile error because `SystemTypeUnknown` doesn't exist yet.

- [ ] **Step 2: Write the failing test for empty-string unmarshal**

Below the existing table-driven loop (after line 36), add a new subtest for empty-string input:

```go
t.Run("empty-string-to-unknown", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`""`), &st)
    require.NoError(t, err)
    assert.Equal(t, SystemTypeUnknown, st)
})
```

- [ ] **Step 3: Update the existing "unknown must fail" test**

The existing test at line 38-42 asserts that `"unknown"` fails. After our change, `"unknown"` is a valid value (it's what `SystemTypeUnknown` marshals to). Replace lines 38-42:

```go
// "unknown" is now a valid value (SystemTypeUnknown marshals to it).
t.Run("unknown-string-accepted", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`"unknown"`), &st)
    require.NoError(t, err)
    assert.Equal(t, SystemTypeUnknown, st)
})

// Truly unknown values must still fail.
t.Run("bogus-value-rejected", func(t *testing.T) {
    var st SystemType
    err := json.Unmarshal([]byte(`"bogus-type"`), &st)
    assert.Error(t, err)
    assert.Contains(t, err.Error(), "unknown SystemType")
})
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -run TestSystemTypeJSON -v`
Expected: compile error — `SystemTypeUnknown` undefined.

- [ ] **Step 5: Add SystemTypeUnknown constant and update UnmarshalJSON**

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

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -run TestSystemTypeJSON -v`
Expected: all subtests PASS.

- [ ] **Step 7: Run the full schema test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/schema/ -v`
Expected: all PASS, no regressions.

- [ ] **Step 8: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/schema/types.go cmd/inspectah/internal/schema/types_test.go
git commit -m "fix(schema): accept empty SystemType as defensive fallback

Empty system_type now unmarshals to SystemTypeUnknown instead of
hard-failing. Defensive compatibility for legacy fixture data that
omits the field.

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

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run "TestRenderAPI_Malformed|TestRenderAPI_EmptyMeta" -v`
Expected: `TestRenderAPI_MalformedSnapshot_Rejected` FAILS (gets 200 instead of 400). `TestRenderAPI_EmptyMeta_Accepted` may pass or fail depending on the render function.

- [ ] **Step 3: Add the validation gate to handleRender**

In `server.go`, insert the following block between line 512 (end of wrapper parsing) and line 514 (`result, err := safeReRender(...)`):

```go
	// Validate snapshot shape — reject payloads that are not recognizable
	// as inspectah snapshots. This is a minimum malformed-shape floor, not
	// a complete validity contract.
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

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run "TestRenderAPI_Malformed|TestRenderAPI_EmptyMeta" -v`
Expected: both PASS.

- [ ] **Step 5: Run full refine test suite to check for regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -v`
Expected: all PASS. The existing `TestRenderAPI_FailedRender_EntireWorkingDirUnchanged` test uses `{"meta":{}}` which passes the gate (Meta is non-nil).

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

- [ ] **Step 1: Write the failing test for malformed PUT rejection**

Add to `server_test.go`:

```go
func TestSnapshotPUT_MalformedSnapshot_Rejected(t *testing.T) {
	dir := setupTestOutputDir(t)
	handler := newRefineHandler(dir, nil)

	// Get current revision
	reqGet := httptest.NewRequest("GET", "/api/snapshot", nil)
	wGet := httptest.NewRecorder()
	handler.ServeHTTP(wGet, reqGet)
	var snapResp map[string]interface{}
	require.NoError(t, json.Unmarshal(wGet.Body.Bytes(), &snapResp))
	revision := int(snapResp["revision"].(float64))

	// PUT a malformed snapshot
	putBody := fmt.Sprintf(`{"snapshot": {"not_valid": true}, "revision": %d}`, revision)
	reqPut := httptest.NewRequest("PUT", "/api/snapshot", strings.NewReader(putBody))
	reqPut.Header.Set("Content-Type", "application/json")
	wPut := httptest.NewRecorder()
	handler.ServeHTTP(wPut, reqPut)

	assert.Equal(t, 400, wPut.Code)

	// Revision must be unchanged
	reqGet2 := httptest.NewRequest("GET", "/api/snapshot", nil)
	wGet2 := httptest.NewRecorder()
	handler.ServeHTTP(wGet2, reqGet2)
	var snapResp2 map[string]interface{}
	require.NoError(t, json.Unmarshal(wGet2.Body.Bytes(), &snapResp2))
	assert.Equal(t, float64(revision), snapResp2["revision"],
		"revision must be unchanged after malformed PUT rejection")
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run TestSnapshotPUT_MalformedSnapshot -v`
Expected: FAIL — gets 200 instead of 400.

- [ ] **Step 3: Add the Meta gate to handleSnapshot PUT**

In `server.go`, after the existing unmarshal at line 398-402 (the `var validSnap` block), add the Meta check. The existing code already has:

```go
var validSnap schema.InspectionSnapshot
if err := json.Unmarshal(req.Snapshot, &validSnap); err != nil {
    h.sendError(w, 400, "invalid snapshot: "+err.Error())
    return
}
```

Add immediately after that block (after line 402):

```go
if validSnap.Meta == nil {
    h.sendError(w, 400, "invalid snapshot: missing meta field")
    return
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run TestSnapshotPUT_MalformedSnapshot -v`
Expected: PASS.

- [ ] **Step 5: Run full refine test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -v`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "fix(refine): reject malformed snapshots in PUT /api/snapshot

Same Meta != nil gate as the render endpoint. PUT remains save-only
with no render dependency — this just prevents junk from being
persisted as working state.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 4: Add POST /api/reset endpoint

**Files:**
- Modify: `cmd/inspectah/internal/refine/server.go` (route registration + handler)
- Modify: `cmd/inspectah/internal/refine/server_test.go`

- [ ] **Step 1: Write the failing test for reset**

Add to `server_test.go`:

```go
func TestResetAPI_RestoresSidecarState(t *testing.T) {
	dir := setupTestOutputDir(t)

	renderCalls := 0
	handler := newRefineHandler(dir, func(snapData []byte, origData []byte, outputDir string) (ReRenderResult, error) {
		renderCalls++
		os.WriteFile(filepath.Join(outputDir, "inspection-snapshot.json"), snapData, 0644)
		return ReRenderResult{
			HTML: "<html>rendered</html>", Snapshot: json.RawMessage(snapData),
			Containerfile: "FROM ubi9\nRUN echo rendered", TriageManifest: json.RawMessage("[]"),
		}, nil
	})

	// Get initial revision
	reqSnap := httptest.NewRequest("GET", "/api/snapshot", nil)
	wSnap := httptest.NewRecorder()
	handler.ServeHTTP(wSnap, reqSnap)
	var snap0 map[string]interface{}
	require.NoError(t, json.Unmarshal(wSnap.Body.Bytes(), &snap0))
	rev0 := snap0["revision"].(float64)

	// POST /api/reset
	reqReset := httptest.NewRequest("POST", "/api/reset", nil)
	wReset := httptest.NewRecorder()
	handler.ServeHTTP(wReset, reqReset)

	assert.Equal(t, 200, wReset.Code)

	var resetResp map[string]interface{}
	require.NoError(t, json.Unmarshal(wReset.Body.Bytes(), &resetResp))
	assert.Equal(t, "reset", resetResp["status"])
	assert.Greater(t, resetResp["revision"].(float64), rev0)
	assert.NotEmpty(t, resetResp["render_id"])

	// reRenderFn must have been called
	assert.Equal(t, 1, renderCalls)
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

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run "TestResetAPI" -v`
Expected: FAIL — 404 (route not registered).

- [ ] **Step 3: Register the route and implement handleReset**

In `server.go`, add the route registration in `newRefineHandler` after line 258:

```go
h.mux.HandleFunc("/api/reset", h.handleReset)
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

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -run "TestResetAPI" -v`
Expected: both PASS.

- [ ] **Step 5: Run full refine test suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/refine/ -v`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
cd /Users/mrussell/Work/bootc-migration/inspectah
git add cmd/inspectah/internal/refine/server.go cmd/inspectah/internal/refine/server_test.go
git commit -m "feat(refine): add POST /api/reset endpoint

Restores the server to startup state by re-rendering from the
existing original-inspection-snapshot.json sidecar. Uses the same
temp-render + sync-on-success path as handleRender. Bumps both
revision and render_id on success.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 5: Add resetServer helper to e2e test helpers

**Files:**
- Modify: `tests/e2e-go/tests/helpers.ts`

- [ ] **Step 1: Add the resetServer export**

Add at the end of `helpers.ts` (after the `architectURL` function):

```typescript
/** Reset the refine server to its startup state via POST /api/reset. */
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

Add hooks to each `test.describe` block. In `'ARIA landmarks and attributes'` (line 9), add before the `beforeEach`:

```typescript
  test.beforeAll(async () => { await resetServer(); });
  test.afterAll(async () => { await resetServer(); });
```

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

beforeAll/afterAll resetServer() on accessibility, artifact-truth,
include-exclude, and rebuild-cycle specs. These specs toggle switches,
trigger rebuilds, or otherwise mutate shared refine server state.

Assisted-by: Claude Code (Opus 4.6)"
```

---

### Task 7: Rewrite api-endpoints malformed and valid render tests

**Files:**
- Modify: `tests/e2e-go/tests/api-endpoints.spec.ts`

- [ ] **Step 1: Add resetServer import and hooks**

Update the import at line 6. Currently there is no helpers import — add one:

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

### Task 8: Run full E2E suite and verify 0 skips

**Files:** None (verification only)

- [ ] **Step 1: Kill any stale servers**

```bash
for port in 9200 9201 9202; do lsof -ti :$port | xargs kill 2>/dev/null; done
```

- [ ] **Step 2: Run the full suite**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test --reporter=list`
Expected: 107 passed, 0 skipped, 0 failed.

If any tests still skip, investigate which spec caused state contamination and add reset hooks to it.

- [ ] **Step 3: Run the isolation probe**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test tests/api-endpoints.spec.ts tests/include-exclude.spec.ts --reporter=list`
Expected: all tests pass (include-exclude finds cards after api-endpoints runs).

- [ ] **Step 4: Run the full suite two more times for determinism**

Run the full suite two more times:
```bash
cd /Users/mrussell/Work/bootc-migration/inspectah/tests/e2e-go && npx playwright test --reporter=list
```
Expected: identical results each time — 0 skips, 0 failures.

- [ ] **Step 5: Run Go test suite to confirm no regressions**

Run: `cd /Users/mrussell/Work/bootc-migration/inspectah && go test ./cmd/inspectah/internal/... -v`
Expected: all PASS.
