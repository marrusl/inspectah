# Render Endpoint Validation & E2E Test Isolation

**Date:** 2026-05-05
**Status:** Proposed (revision 3 — addresses round 1 + round 2 review feedback)
**Scope:** Go refine server (`POST /api/render`, `PUT /api/snapshot`) + Playwright e2e test suite
**Reviewers (round 1):** Kit, Thorn, Collins

## Problem

The refine server's `POST /api/render` endpoint accepts arbitrary JSON payloads — including obviously invalid ones like `{"snapshot": {"not_valid": true}}` — and produces a successful render from the empty result. This replaces the server's live rendered state with empty output, breaking the SPA for all subsequent page loads.

In the Playwright e2e test suite, `api-endpoints.spec.ts` sends this malformed payload, causing 21 tests across 3 other spec files to skip because the SPA renders zero sections.

### Root Cause Chain

1. `api-endpoints.spec.ts:75` sends `POST /api/render` with `{snapshot: {not_valid: true}}`
2. All `InspectionSnapshot` section fields are optional pointers → junk unmarshals into a valid but empty struct
3. The empty struct renders successfully (nothing to fail on) → server returns 200 with empty HTML
4. The server's live rendered output is now empty → SPA renders zero sections
5. `editor.spec.ts`, `include-exclude.spec.ts`, `triage-cards.spec.ts` search for triage/toggle cards → find none → skip gracefully

Additional issue: the valid fixture snapshot fails render with `parse snapshot: unknown SystemType ""` because `SystemType.UnmarshalJSON` rejects empty strings. So the "valid" render returns 500 while the junk render returns 200.

### Existing Invariants Preserved

The current refine server has a deliberate state model with two separate seams:

- **`PUT /api/snapshot`** — durability seam. Persists the mutable working snapshot to disk and bumps `revision`. Does not render. This is the autosave path.
- **`POST /api/render`** — artifact seam. Re-renders the snapshot through the full pipeline (temp-render + sync-on-success via `nativeReRender`), updates `revision` and `render_id`. Existing `TestRenderAPI_FailedRender_EntireWorkingDirUnchanged` proves that failed renders leave the working directory byte-identical.
- **`original-inspection-snapshot.json`** — immutable sidecar created at startup. The canonical baseline for reset/revert.

This spec preserves all three invariants. Validation is added at the input boundary; the save/render/sidecar truth model is unchanged.

## Design

### Part 1: Minimum Snapshot Validation Gate

**What this is:** A narrow malformed-shape floor that rejects payloads that are not recognizable as inspectah snapshots. It is not a complete validity contract — the render pipeline itself provides deeper validation. This gate exists to prevent obviously junk input from entering the pipeline at all.

**Gate rule:** After JSON unmarshal into `schema.InspectionSnapshot`, reject if `Meta == nil`. Every inspectah-produced snapshot has a `Meta` map (even if empty). A payload that unmarshals with `Meta == nil` was never an inspectah snapshot.

This catches `{"not_valid": true}` (no `meta` key at all) while accepting `{"meta": {}}` (valid minimal snapshot per existing `schema/testdata/minimal-snapshot.json` and existing server tests).

#### 1a. Gate in handleRender (`server.go:479`)

After parsing the snapshot JSON wrapper (lines 505-512) and before calling `safeReRender`:

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

Returns HTTP 400 for rejected payloads. The existing atomicity guarantee (`TestRenderAPI_FailedRender_EntireWorkingDirUnchanged`) means a rejected render leaves all state unchanged — no revision bump, no render_id change, no disk mutation.

#### 1b. Gate in handleSnapshot PUT (`server.go:393`)

The PUT path already unmarshals into `InspectionSnapshot` at line 398. Add the same `Meta == nil` check after the existing unmarshal. Returns HTTP 400. The PUT path remains save-only — no render dependency is added. This prevents junk from being persisted as working state via autosave.

### Part 2: Accept Empty SystemType as Defensive Fallback

**File:** `cmd/inspectah/internal/schema/types.go`

In `UnmarshalJSON` (line 35), add a case for `""` that maps to a new `SystemTypeUnknown` constant. This is a defensive unmarshal fallback for snapshots where the inspector did not detect the system type, not a new first-class semantic state.

**Context (round 1 open question):** Collins raised that the current three-valued contract (`package-mode`, `rpm-ostree`, `bootc`) is intentional, and empty `system_type` likely indicates a fixture/schema-boundary mismatch rather than a real system state. Kit argued that hard-failing on empty string is wrong behavior regardless of cause. Both are right.

**Resolution:** Accept empty string as `SystemTypeUnknown` at the unmarshal layer so the render pipeline doesn't hard-fail on legacy or incomplete data. The renderer already treats non-`rpm-ostree`/non-`bootc` values as the generic fallback path, so `unknown` falls through naturally. Separately, the e2e fixtures should be refreshed to emit current-contract `system_type` values (tracked as a follow-up, not in this spec).

```go
const (
    SystemTypeUnknown     SystemType = "unknown"
    SystemTypePackageMode SystemType = "package-mode"
    SystemTypeRpmOstree   SystemType = "rpm-ostree"
    SystemTypeBootc       SystemType = "bootc"
)

func (s *SystemType) UnmarshalJSON(data []byte) error {
    var raw string
    if err := json.Unmarshal(data, &raw); err != nil {
        return err
    }
    switch SystemType(raw) {
    case SystemTypePackageMode, SystemTypeRpmOstree, SystemTypeBootc:
        *s = SystemType(raw)
    case "":
        *s = SystemTypeUnknown
    default:
        return fmt.Errorf("unknown SystemType %q", raw)
    }
    return nil
}
```

**Test updates:**
- `types_test.go`: empty string unmarshals to `SystemTypeUnknown`
- One render-level test: `SystemTypeUnknown` yields generic output without hard error

### Part 3: Server-Owned Reset Endpoint

**File:** `cmd/inspectah/internal/refine/server.go`

Add `POST /api/reset` to `refineHandler`. This restores the server to its startup state by re-rendering from the existing immutable sidecar.

#### Implementation

1. Read `original-inspection-snapshot.json` from `h.outputDir` (the existing sidecar, created at startup by `RunRefine`)
2. Call the existing re-render path (`h.reRenderFn`) with the sidecar bytes — this uses the same temp-render + sync-on-success flow as `handleRender`, so a failed reset leaves the working directory unchanged
3. On success: bump `revision` and regenerate `render_id`
4. Return `{"status": "reset", "revision": N, "render_id": "..."}`
5. On failure: return 500 with error, state unchanged

No new state fields on `refineHandler`. The sidecar file is the sole source of truth for "original." No pre-writing the working snapshot before render success.

### Part 4: E2E Test Isolation

**Directory:** `tests/e2e-go/`

#### 4a. Reset helper in `helpers.ts`

```typescript
export async function resetServer(baseURL?: string): Promise<void> {
  const url = baseURL || process.env.REFINE_FLEET_URL || 'http://localhost:9200';
  const resp = await fetch(`${url}/api/reset`, { method: 'POST' });
  if (!resp.ok) throw new Error(`Server reset failed: ${resp.status}`);
}
```

#### 4b. Reset participation by behavior

Any spec file that performs one or more of these actions resets before and after:
- toggles output-affecting state (include/exclude switches)
- saves via `PUT /api/snapshot`
- triggers rebuilds via `POST /api/render` or the rebuild button
- generates drafts or other side-effecting POST requests

```typescript
test.beforeAll(async () => { await resetServer(); });
test.afterAll(async () => { await resetServer(); });
```

**Mutating specs** (need reset hooks):
- `accessibility.spec.ts` — keyboard toggle activation (Enter/Space on switches) and rebuild-triggered live region announcements
- `api-endpoints.spec.ts` — POST /api/render with valid and malformed payloads
- `artifact-truth.spec.ts` — toggles switches, triggers rebuilds
- `include-exclude.spec.ts` — toggles include/exclude switches
- `rebuild-cycle.spec.ts` — triggers rebuilds

**Read-only specs** (no reset needed):
- `smoke.spec.ts` — page load and structure checks
- `containerfile-preview.spec.ts` — reads preview panel content
- `section-navigation.spec.ts` — clicks sidebar nav links (no state mutation)
- `theme-switching.spec.ts` — localStorage only
- `triage-cards.spec.ts` — reads card structure
- `architect-smoke.spec.ts` — different server (port 9202)
- `editor.spec.ts` — currently read-only; add reset hooks if/when it starts persisting edits

**Isolation claim scope:** This pattern proves clean server state at spec-file boundaries in the current serial Playwright configuration (`workers: 1`, `fullyParallel: false`). It does not prove per-test isolation within a spec file or safety under parallel workers. If tighter isolation is needed later, `beforeEach` reset on the mutating spec set is the next step.

#### 4c. Strict E2E assertions for the malformed render test

Replace the current tolerant malformed-render test (`api-endpoints.spec.ts:75`) with strict contract assertions:

```typescript
test('POST /api/render rejects malformed payload with 400', async ({ request }) => {
  // Capture state before the malformed request
  const snapBefore = await request.get('/api/snapshot');
  const revisionBefore = (await snapBefore.json()).revision;

  const renderResp = await request.post('/api/render', {
    data: { snapshot: { not_valid: true } },
  });

  // Must be rejected with 400, not any other non-OK status
  expect(renderResp.status()).toBe(400);
  const body = await renderResp.json();
  expect(body.error).toBeDefined();
  expect(typeof body.error).toBe('string');

  // Canonical state must be unchanged
  const snapAfter = await request.get('/api/snapshot');
  const revisionAfter = (await snapAfter.json()).revision;
  expect(revisionAfter).toBe(revisionBefore);
});
```

The key assertion is `revisionAfter === revisionBefore` — rejected malformed renders must leave canonical state unchanged, not just return an error.

#### 4d. Strict valid render test

With the SystemType fix, the valid fixture render should succeed. Update `api-endpoints.spec.ts:45`:

```typescript
test('POST /api/render accepts valid snapshot with 200', async ({ request }) => {
  const snapResp = await request.get('/api/snapshot');
  const snapBody = await snapResp.json();

  const renderResp = await request.post('/api/render', {
    data: { snapshot: snapBody.snapshot },
  });

  // Must succeed — if this fails, the fixture or schema contract has a bug
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

No error branch. If this test fails, it means the fixture or schema contract has a real bug that needs fixing, not tolerating.

## Testing

### Go unit tests

- `handleRender` with `{not_valid: true}` → HTTP 400, working directory unchanged
- `handleRender` with `{meta: {}}` → accepted (renders or fails at pipeline level, not at gate)
- `handleSnapshot` PUT with `{not_valid: true}` → HTTP 400, revision unchanged
- `SystemType` empty string → `SystemTypeUnknown`
- `SystemTypeUnknown` renders through generic path without error
- `POST /api/reset` → restores sidecar state, bumps revision and render_id

### E2E tests

- Full suite: 0 skips, 0 failures. Run 3 times to verify determinism. (This is serial-runner hygiene, not a general isolation proof.)
- Malformed render: HTTP 400, revision unchanged after rejection
- Valid render: HTTP 200, full response shape validated
- Isolation probe: `api-endpoints.spec.ts` followed by `include-exclude.spec.ts` — verify include-exclude finds cards

## Not In Scope

- Architect server validation (port 9202, different handler)
- Refreshing fleet-3host fixture to emit current-contract `system_type` (follow-up)
- Per-test isolation within spec files (current serial runner makes this unnecessary)
- Concurrent/parallel test runner support
- Rate limiting or auth on render/reset endpoints (refine is a local dev tool)

## Round 1 Review Resolution Log

| Blocker | Resolution |
|---------|-----------|
| Meta non-empty overclaims minimal contract | Changed to `Meta != nil` (present). Existing `{meta: {}}` fixtures accepted. Described as minimum malformed-shape floor, not complete validity model. |
| Part 1c blurs PUT/render seam | Removed entirely. PUT stays save-only. No render dependency added to PUT path. |
| Reset duplicates sidecar truth | Reset reads from existing `original-inspection-snapshot.json` sidecar, calls existing re-render path. No `originalSnapshot []byte` field. |
| Reset pre-writes snapshot before render | Removed. Reset uses temp-render + sync-on-success like handleRender. |
| Reset response missing render_id | Added `render_id` to reset response. |
| E2E malformed render too tolerant | Assert HTTP 400 specifically. Assert revision unchanged after rejection. |
| E2E valid render tolerates error branch | Strict HTTP 200 assertion. No error branch. |
| Reset participation is filename-guessed | Defined by mutating behavior criteria. Corrected accessibility.spec.ts classification (read-only). |
| Repeatability claim too strong | Narrowed to "serial spec-file boundary hygiene." |
| SystemType: fixture mismatch vs new semantic (open) | Accept empty as defensive fallback (`SystemTypeUnknown`). Not a new first-class state. Fixture refresh tracked separately. |
| `accessibility.spec.ts` misclassified as read-only (round 2) | Moved to mutating set. Three tests toggle switches via keyboard (Enter/Space) and one triggers a rebuild with live region assertion. |
