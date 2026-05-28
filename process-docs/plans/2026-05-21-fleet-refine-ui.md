# Fleet Refine UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add dedicated fleet API endpoints and a React component tree for interactive fleet refinement with zone headers, variant comparison, per-item acknowledgment, and a variant summary banner.

**Architecture:** Dedicated fleet surface (Approach B). New `fleet_handlers.rs` module in `inspectah-web` serves fleet-specific DTOs via `/api/fleet/*` endpoints. New `FleetApp` component tree in `inspectah-web/ui` consumes these endpoints, mounted conditionally based on `/api/health` fleet detection. Shared shell surfaces (GlobalSearch, ExportDialog, ShortcutOverlay, toolbar) are extracted into composable pieces before fleet work begins. Single-host mode is completely untouched.

**Tech Stack:** Rust (axum handlers, serde DTOs), TypeScript (React 18, PatternFly 6), Vitest + Testing Library, Playwright E2E.

**Spec:** `docs/specs/proposed/2026-05-21-fleet-refine-ui-spec.md` (approved round 4)

---

## File Structure

### Backend (Rust)

| File | Action | Responsibility |
|------|--------|---------------|
| `inspectah-web/src/fleet_handlers.rs` | Create | Fleet-specific handlers: `fleet_view`, `fleet_diff`. Fleet response DTOs. |
| `inspectah-web/src/handlers.rs` | Modify | Enrich `health()` with fleet context from `snapshot.fleet_meta` and `session.fleet_context()`. |
| `inspectah-web/src/lib.rs` | Modify | Add `/api/fleet/view` and `/api/fleet/diff` routes, `pub mod fleet_handlers`. |
| `inspectah-web/tests/fleet_api_test.rs` | Create | HTTP contract tests for fleet endpoints. |

### Frontend (TypeScript/React)

| File | Action | Responsibility |
|------|--------|---------------|
| `ui/src/api/types.ts` | Modify | Add fleet types. |
| `ui/src/api/fleet-client.ts` | Create | `fetchFleetView()`, `fetchFleetDiff()`. |
| `ui/src/hooks/useFleetMutation.ts` | Create | Mutation hook: mutate → ignore ViewResponse → re-fetch fleet view. |
| `ui/src/hooks/useFleetDiff.ts` | Create | Lazy diff loading with client-side cache. |
| `ui/src/hooks/useVariantAck.ts` | Create | Per-item ack state backed by localStorage. |
| `ui/src/components/AppShell.tsx` | Create | Extracted shared shell: toolbar, GlobalSearch, ExportDialog, ShortcutOverlay, keyboard handler. |
| `ui/src/App.tsx` | Modify | Both SingleApp and FleetApp compose `AppShell`. Fleet fork based on `health.fleet`. |
| `ui/src/components/fleet/FleetApp.tsx` | Create | Fleet shell composing AppShell + fleet-specific content. |
| `ui/src/components/fleet/FleetSidebar.tsx` | Create | Section nav with zone counts and ack progress. |
| `ui/src/components/fleet/FleetSection.tsx` | Create | Active section container with zone groups. |
| `ui/src/components/fleet/ZoneGroup.tsx` | Create | Collapsible zone container with header and counts. |
| `ui/src/components/fleet/FleetItemRow.tsx` | Create | Item row with prevalence chip, variant indicator, toggle. |
| `ui/src/components/fleet/VariantView.tsx` | Create | Variant radio list, Compare button, Confirm button. |
| `ui/src/components/fleet/DiffDrawer.tsx` | Create | Unified diff display with syntax highlighting. |
| `ui/src/components/fleet/FleetBanner.tsx` | Create | Variant summary banner with severity scaling and navigation. |
| `ui/src/components/fleet/__tests__/` | Create | Unit tests for each fleet component. |
| `ui/src/hooks/__tests__/useFleetMutation.test.ts` | Create | Mutation hook tests. |
| `ui/src/hooks/__tests__/useFleetDiff.test.ts` | Create | Diff hook tests. |
| `ui/src/hooks/__tests__/useVariantAck.test.ts` | Create | Ack hook tests. |
| `ui/e2e/fleet.spec.ts` | Create | E2E tests for fleet refine flow. |

---

## Fleet-Mode Data Contract

All fleet frontend code consumes one authoritative contract. This is
the `FleetViewResponse` shape from `GET /api/fleet/view`, enriched
with shell fields that carry into shared surfaces:

```typescript
interface FleetViewResponse {
  generation: number;
  can_undo: boolean;
  can_redo: boolean;
  containerfile_preview: string;
  session_is_sensitive: boolean;
  summary: FleetSummary;
  sections: FleetSection[];
}
```

**Shell surface data sources:**

| Surface | Data source | Notes |
|---------|-------------|-------|
| ContainerfilePanel | `fleetView.containerfile_preview` | Same as single-host, from projected snapshot |
| ExportDialog | `fleetView.generation`, `health.session_is_sensitive` | Generation from fleet view; sensitive from health (session-lifetime) |
| GlobalSearch (`Ctrl+K`) | `searchableItems` prop on AppShell | See GlobalSearch contract below |
| Section search (`/`) | `filterText` render prop | See Section search contract below |
| Toolbar ack progress | `useVariantAck.unackedCount` | Frontend-only state |
| Undo/Redo buttons | `fleetView.can_undo`, `fleetView.can_redo` | From fleet view response |

**GlobalSearch contract:**

AppShell receives a `searchableItems` prop — an array of searchable
entries that GlobalSearch indexes for `Ctrl+K` results:

```typescript
interface SearchableEntry {
  id: string;           // unique key for the item
  sectionId: string;    // which section this item belongs to
  sectionLabel: string; // display name for the section
  title: string;        // primary display text (e.g., "httpd.x86_64")
  subtitle?: string;    // secondary text (e.g., "8/12 hosts")
  searchText: string;   // full searchable text (title + subtitle + detail)
}
```

**SingleApp** builds `searchableItems` from `packageItems`,
`configItems`, and `contextSections` (existing behavior, just
restructured into the common shape).

**FleetApp** builds `searchableItems` from `fleetView.sections` — all
items across all sections (both decision and context), with `title`
derived from `ItemId` display text and `subtitle` from prevalence.

GlobalSearch result click calls `onNavigateSection(sectionId)` then
(in fleet mode) sets `pendingNavTarget` to scroll to the item.
AppShell receives an optional `onSearchNavigate(sectionId, itemId)`
callback that FleetApp uses for the portal flow.

**Section search contract:**

AppShell owns `sectionSearchOpen: boolean` and `filterText: string`
state internally. The `/` key toggles `sectionSearchOpen`. Typing in
the search input updates `filterText`. The content area receives
`filterText` via the render prop:

```tsx
children: (filterText: string) => React.ReactNode
```

**Filter reset rules:**
- `activeSection` changes (any cause: sidebar click, banner portal,
  GlobalSearch navigation, keyboard 1-9) → `filterText` resets to `""`
  and `sectionSearchOpen` closes. Implemented via `useEffect` keyed on
  `activeSection`.
- Escape in search input → `filterText` resets, search closes, focus
  returns to the first item in the section.
- Refetch after mutation → `filterText` preserved (filter state is
  independent of server data). The same filter continues to apply to
  the updated item list.

**Refetch-failure contract:**

| Failure mode | Behavior |
|-------------|----------|
| Mutation succeeds, refetch fails | Show inline error "View update failed." with Retry. Hold `lastConfirmedView` (last successful fleet view). Queue clears. |
| Diff load fails | Show inline error in DiffDrawer with Retry. Variant view stays open. |
| Initial fleet view load fails | Show error page with Retry. No stale view to hold. |

The `useFleetMutation` hook maintains a `lastConfirmedView` ref that
always points to the most recent successful `FleetViewResponse`. On
refetch failure, the UI keeps rendering `lastConfirmedView` with the
error overlay — it does not go blank.

---

## Section-Source Contract

### Fleet v1 decision sections

| Section | Section ID | Variant UI? | Identity via `ItemId` |
|---------|-----------|-------------|----------------------|
| RPM Packages | `packages` | no | `Package { name_arch }` |
| Config Files | `configs` | **yes** (radio + compare) | `Config { path }` |

**`users_groups` deferred from fleet v1.** The current backend does not
produce per-user `FleetPrevalence` or attention scores for fleet
snapshots — user entries lack fleet metadata fields, and `RefinedView`
does not include users (they come from a separate
`snapshot.users_groups.users` path with no fleet enrichment). Adding
fleet-aware user merging would require upstream core work
(`FleetMergeable` impl for users, `FleetPrevalence` on `UserEntry`,
attention scoring for users) that is out of scope for this plan.

Fleet v1 omits the `users_groups` section from `/api/fleet/view`.
User strategy decisions are not lost — they persist in the snapshot
and export. They are simply not surfaced in the fleet refine UI until
the backend produces truthful fleet metadata for users.

### Fleet v1 context sections

All other sections (services, containers, network, storage, scheduled,
selinux, kernel_boot, nonrpm, version_changes) are read-only context
sections. They get prevalence badges and zone grouping but no toggles
and no variant UI. Items with engine-level variants show a count
indicator ("2 variants") as read-only informational text.

### Fleet-of-2 behavior

When `fleet_context.zones_active == false`:
- `FleetSection.zones` is `null`, `FleetSection.items` is a flat array
- `FleetItem.attention.zone` is `null`
- All zone-group rendering is suppressed — items render flat
- Variant ops, ack, and banner still function normally

### Single-host fallback

`GET /api/fleet/view` on a single-host session returns 200 with an
informative error body: `{"error": "not a fleet session"}`. The
frontend never calls this endpoint for single-host (it checks
`health.fleet` first), but the handler degrades gracefully.

---

## Task 1: Fleet Health Endpoint Enrichment

**Files:**
- Modify: `inspectah-web/src/handlers.rs`
- Modify: `ui/src/api/types.ts`
- Test: `inspectah-web/tests/fleet_api_test.rs` (create)

- [ ] **Step 1: Write failing Rust test for fleet health response**

In `inspectah-web/tests/fleet_api_test.rs`, test that `/api/health`
returns a `fleet` field for fleet snapshots. The fleet metadata lives
on `InspectionSnapshot.fleet_meta` (typed `Option<FleetSnapshotMeta>`
in `inspectah-core/src/snapshot.rs`), and the fleet context is
available via `RefineSession::fleet_context()` in
`inspectah-refine/src/session.rs`.

```rust
#[tokio::test]
async fn health_returns_fleet_context_for_fleet_snapshot() {
    // Load a fleet snapshot fixture (must have fleet_meta populated)
    // Create RefineSession — session auto-detects fleet mode from fleet_meta
    // Build AppState and router
    // GET /api/health
    // Assert: response["fleet"]["host_count"] matches fleet_meta.host_count
    // Assert: response["fleet"]["hostnames"] matches fleet_meta.hostnames
    // Assert: response["fleet"]["zones_active"] is true (3+ hosts) or false (2 hosts)
    // Assert: response["fleet"]["variant_count"] is a number
}

#[tokio::test]
async fn health_returns_null_fleet_for_single_host_snapshot() {
    // Load single-host snapshot (fleet_meta is None)
    // GET /api/health
    // Assert: response["fleet"] is null
}
```

Use existing `api_test.rs` patterns for app state setup. If no fleet
test fixture exists, generate one: `cargo run -- fleet aggregate
testdata/some-tarballs/ -o testdata/fleet-test.tar.gz`, then extract
the `inspection-snapshot.json`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package inspectah-web -- fleet_api`
Expected: FAIL — `health` handler doesn't return `fleet` field yet.

- [ ] **Step 3: Implement fleet context in health handler**

In `handlers.rs`, modify `health()` to read from the real seam:

```rust
pub async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();
    let snap = session.snapshot();
    let generation = session.generation();

    // Fleet context from the typed field, not snap.meta["fleet"]
    let fleet = snap.fleet_meta.as_ref().map(|meta| {
        let variant_count = inspectah_refine::fleet::variant_summary(
            snap, session.fleet_context()
        ).map(|s| s.paths_with_variants).unwrap_or(0);

        serde_json::json!({
            "host_count": meta.host_count,
            "hostnames": meta.hostnames,
            "zones_active": session.fleet_context()
                .map(|fc| fc.zones_active).unwrap_or(false),
            "variant_count": variant_count,
            "label": meta.label,
            "merged_at": meta.merged_at,
        })
    });

    Json(json!({
        "status": "ok",
        "generation": generation,
        "fleet": fleet,
        "session_is_sensitive": session.is_sensitive(),
        // ... other existing fields
    }))
}
```

Key difference from round 1: uses `snap.fleet_meta` (typed
`Option<FleetSnapshotMeta>` on `InspectionSnapshot`) and
`session.fleet_context()` (derived at session creation from
`fleet_meta`), NOT `snap.meta["fleet"]` (untyped JSON map).

- [ ] **Step 4: Add `FleetHealthInfo` to frontend types**

In `ui/src/api/types.ts`:

```typescript
export interface FleetHealthInfo {
  host_count: number;
  hostnames: string[];
  zones_active: boolean;
  variant_count: number;
  label: string;
  merged_at: string;
}

// Update HealthResponse to include fleet and session_is_sensitive:
export interface HealthResponse {
  status: string;
  generation: number;
  fleet: FleetHealthInfo | null;
  session_is_sensitive: boolean;
  // ... existing fields
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --package inspectah-web -- fleet_api`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/src/handlers.rs inspectah-web/tests/fleet_api_test.rs inspectah-web/ui/src/api/types.ts
git commit -m "feat(web): enrich /api/health with fleet context from snapshot.fleet_meta"
```

---

## Task 2: Fleet View Handler and DTOs

**Files:**
- Create: `inspectah-web/src/fleet_handlers.rs`
- Modify: `inspectah-web/src/lib.rs`
- Test: `inspectah-web/tests/fleet_api_test.rs`

- [ ] **Step 1: Define fleet response DTOs**

Create `fleet_handlers.rs`. These are fleet-specific DTOs — NOT direct
serializations of engine types. The handler maps engine types into
these shapes.

```rust
use serde::{Deserialize, Serialize};
use inspectah_core::types::fleet::PrevalenceZone;
use inspectah_refine::types::{AttentionLevel, AttentionReason, ContentHash, ItemId};

#[derive(Serialize)]
pub struct FleetViewResponse {
    pub generation: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub containerfile_preview: String,
    pub session_is_sensitive: bool,
    pub summary: FleetSummary,
    pub sections: Vec<FleetSection>,
}

#[derive(Serialize)]
pub struct FleetSummary {
    pub host_count: usize,
    pub actionable_variant_items: Vec<ActionableVariantItem>,
    pub informational_variant_count: usize,
}

#[derive(Serialize)]
pub struct ActionableVariantItem {
    pub item_id: ItemId,
    pub section_id: String,
    pub variant_count: usize,
    pub max_host_spread: usize,
}

#[derive(Serialize)]
pub struct FleetSection {
    pub id: String,
    pub display_name: String,
    pub is_decision_section: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones: Option<FleetZones>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<FleetItem>>,
}

#[derive(Serialize)]
pub struct FleetZones {
    pub consensus: FleetZoneGroup,
    pub near_consensus: FleetZoneGroup,
    pub divergent: FleetZoneGroup,
}

#[derive(Serialize)]
pub struct FleetZoneGroup {
    pub items: Vec<FleetItem>,
    pub count: usize,
}

#[derive(Serialize)]
pub struct FleetItem {
    pub item_id: ItemId,
    pub include: bool,
    pub attention: FleetAttentionDto,
    pub prevalence: FleetPrevalenceDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variants: Option<FleetVariants>,
}

#[derive(Serialize)]
pub struct FleetAttentionDto {
    pub level: AttentionLevel,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<PrevalenceZone>,
    pub prevalence: u32,
}

#[derive(Serialize)]
pub struct FleetPrevalenceDto {
    pub count: u32,
    pub total: u32,
}

#[derive(Serialize)]
pub struct FleetVariants {
    pub count: usize,
    pub selected: String,
    pub options: Vec<FleetVariantOption>,
}

#[derive(Serialize)]
pub struct FleetVariantOption {
    pub hash: String,
    pub hosts: Vec<String>,
    pub host_count: usize,
    pub selected: bool,
}
```

- [ ] **Step 2: Write failing tests**

```rust
#[tokio::test]
async fn fleet_view_returns_zone_grouped_sections() {
    // Load fleet-3host fixture
    // GET /api/fleet/view
    // Assert: response has containerfile_preview (non-empty string)
    // Assert: response has session_is_sensitive (boolean)
    // Assert: sections have zones with consensus/near_consensus/divergent
    // Assert: items have item_id with {kind, key} shape
    // Assert: summary.actionable_variant_items lists config variants only
    // Assert: summary.informational_variant_count counts non-config variants
}

#[tokio::test]
async fn fleet_view_returns_flat_for_fleet_of_2() {
    // Load fleet-2host fixture (zones_active: false)
    // GET /api/fleet/view
    // Assert: sections have zones: null, items: [...]
    // Assert: items have attention.zone: null
}

#[tokio::test]
async fn fleet_view_returns_error_for_single_host() {
    // Load single-host snapshot
    // GET /api/fleet/view
    // Assert: 200 with {"error": "not a fleet session"}
}
```

- [ ] **Step 3: Implement fleet view handler**

```rust
pub async fn fleet_view(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let session = state.session.lock().unwrap();

    match session.fleet_context() {
        Some(ctx) => {
            let response = build_fleet_view_response(&session, ctx);
            Json(serde_json::to_value(&response).unwrap()).into_response()
        }
        None => {
            Json(json!({"error": "not a fleet session"})).into_response()
        }
    }
}

fn build_fleet_view_response(session: &RefineSession, ctx: &FleetContext) -> FleetViewResponse {
    let view = session.view();
    let snap = session.snapshot_projected();

    FleetViewResponse {
        generation: session.generation(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        containerfile_preview: view.containerfile_preview.clone(),
        session_is_sensitive: session.is_sensitive(),
        summary: build_fleet_summary(&snap, ctx),
        sections: build_fleet_sections(&snap, &view, ctx),
    }
}
```

The `build_fleet_sections` function maps each snapshot section into
`FleetSection` with zone grouping. Decision sections: `packages`,
`configs`. All others: context (read-only). `users_groups` is
deferred from fleet v1 and is NOT included in the fleet view response
(see Section-Source Contract).

`AttentionReason` mapping: match on each variant to produce a
snake_case string. `Custom(s)` passes through as-is.

- [ ] **Step 4: Wire route**

In `lib.rs`:
```rust
pub mod fleet_handlers;
// In router():
.route("/api/fleet/view", get(fleet_handlers::fleet_view))
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test --package inspectah-web -- fleet`

```bash
git add inspectah-web/src/fleet_handlers.rs inspectah-web/src/lib.rs inspectah-web/tests/fleet_api_test.rs
git commit -m "feat(web): add /api/fleet/view with zone-grouped DTOs and shell fields"
```

---

## Task 3: Fleet Diff Handler

**Files:**
- Modify: `inspectah-web/src/fleet_handlers.rs`
- Modify: `inspectah-web/src/lib.rs`
- Test: `inspectah-web/tests/fleet_api_test.rs`

- [ ] **Step 1: Add diff DTOs and handler**

```rust
#[derive(Deserialize)]
pub struct FleetDiffRequest {
    pub item_id: ItemId,
    pub base: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct FleetDiffResponse {
    pub base_hash: String,
    pub target_hash: String,
    pub base_hosts: Vec<String>,
    pub target_hosts: Vec<String>,
    pub hunks: Vec<FleetDiffHunk>,
    pub stats: FleetDiffStats,
}

#[derive(Serialize)]
pub struct FleetDiffHunk {
    pub base_range: FleetLineRange,
    pub target_range: FleetLineRange,
    pub changes: Vec<FleetDiffChange>,
}

#[derive(Serialize)]
pub struct FleetLineRange { pub start: usize, pub count: usize }

#[derive(Serialize)]
pub struct FleetDiffChange { pub kind: String, pub content: String }

#[derive(Serialize)]
pub struct FleetDiffStats {
    pub total_changes: usize,
    pub insertions: usize,
    pub deletions: usize,
}
```

Handler: resolve base/target content from the projected snapshot's
config entries, call `compute_diff()`, map `ChangeKind` variants to
wire strings (`Equal` → `"equal"`, `Delete` → `"delete"`, `Insert` →
`"insert"`). Host lists resolved from `FleetPrevalence` on the config
entries. All errors return 422.

- [ ] **Step 2: Write tests, wire route, commit**

```rust
#[tokio::test]
async fn fleet_diff_returns_unified_diff() { /* valid pair → hunks */ }

#[tokio::test]
async fn fleet_diff_422_unknown_item() { /* bad item_id → 422 */ }

#[tokio::test]
async fn fleet_diff_422_unknown_hash() { /* bad hash → 422 */ }

#[tokio::test]
async fn fleet_diff_422_binary() { /* binary content → 422 */ }
```

Route: `.route("/api/fleet/diff", post(fleet_handlers::fleet_diff))`

```bash
git commit -m "feat(web): add POST /api/fleet/diff endpoint"
```

---

## Task 4: Fleet API Types and Client (Frontend)

**Files:**
- Modify: `ui/src/api/types.ts`
- Create: `ui/src/api/fleet-client.ts`
- Test: `ui/src/api/__tests__/fleet-client.test.ts`

- [ ] **Step 1: Add complete fleet types to types.ts**

All types from the Fleet-Mode Data Contract section above. Key types:
`FleetViewResponse` (includes `containerfile_preview`,
`session_is_sensitive`), `FleetSection`, `FleetItem`, `FleetVariants`,
`FleetDiffRequest`, `FleetDiffResponse`, `DiffHunk`, `DiffChange`.

- [ ] **Step 2: Create fleet-client.ts**

```typescript
export function fetchFleetView(): Promise<FleetViewResponse> {
  return getJson("/api/fleet/view");
}

export function fetchFleetDiff(req: FleetDiffRequest): Promise<FleetDiffResponse> {
  return postJson("/api/fleet/diff", req);
}
```

- [ ] **Step 3: Write tests, commit**

Test: mock `fetch`, verify request shapes, assert response parsing.

```bash
git commit -m "feat(ui): add fleet API types and client"
```

---

## Task 5: Fleet Mutation Hook

**Files:**
- Create: `ui/src/hooks/useFleetMutation.ts`
- Test: `ui/src/hooks/__tests__/useFleetMutation.test.ts`

- [ ] **Step 1: Write tests covering mutation + refetch + failure**

```typescript
describe("useFleetMutation", () => {
  it("calls applyOp then re-fetches fleet view", async () => {
    // Mock applyOp success, fetchFleetView success
    // Assert: onViewUpdate called with fleet view
  });

  it("clears queue on mutation failure", async () => {
    // Mock applyOp reject
    // Assert: onError called, queue empty
  });

  it("holds lastConfirmedView on refetch failure", async () => {
    // Mock applyOp success, fetchFleetView reject
    // Assert: refetchError set, lastConfirmedView still available
    // Assert: retry re-fetches successfully
  });

  it("queues mutations sequentially", async () => {
    // Enqueue two ops
    // Assert: second waits for first to complete + refetch
  });
});
```

- [ ] **Step 2: Implement with lastConfirmedView**

```typescript
export function useFleetMutation(
  onViewUpdate: (view: FleetViewResponse) => void,
  onError: (err: Error) => void,
): UseFleetMutationResult {
  const [isPending, setIsPending] = useState(false);
  const [refetchError, setRefetchError] = useState<string | null>(null);
  const lastConfirmedView = useRef<FleetViewResponse | null>(null);
  const queueRef = useRef<QueueEntry[]>([]);
  const processingRef = useRef(false);

  const processQueue = useCallback(async () => {
    if (processingRef.current) return;
    processingRef.current = true;
    setIsPending(true);
    setRefetchError(null);

    while (queueRef.current.length > 0) {
      const entry = queueRef.current[0];
      try {
        if (entry.kind === "op") await applyOp(entry.op);
        else if (entry.kind === "undo") await apiUndo();
        else await apiRedo();
        queueRef.current.shift();

        try {
          const fleetView = await fetchFleetView();
          lastConfirmedView.current = fleetView;
          onViewUpdate(fleetView);
        } catch (refetchErr: unknown) {
          setRefetchError(
            refetchErr instanceof Error ? refetchErr.message : "View update failed"
          );
          queueRef.current = [];
          break;
        }
      } catch (err: unknown) {
        queueRef.current = [];
        onError(err instanceof Error ? err : new Error(String(err)));
        break;
      }
    }

    processingRef.current = false;
    setIsPending(false);
  }, [onViewUpdate, onError]);

  const retry = useCallback(async () => {
    setRefetchError(null);
    try {
      const fleetView = await fetchFleetView();
      lastConfirmedView.current = fleetView;
      onViewUpdate(fleetView);
    } catch (err: unknown) {
      setRefetchError(err instanceof Error ? err.message : "Retry failed");
    }
  }, [onViewUpdate]);

  // ... enqueue helpers same as before

  return { mutate, undo, redo, isPending, refetchError, retry, lastConfirmedView };
}
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat(ui): add useFleetMutation with lastConfirmedView and retry"
```

---

## Task 6: Variant Ack Hook

Same as plan round 1, Task 6. No changes needed — the hook is
frontend-only and was correctly scoped to actionable config items.

```bash
git commit -m "feat(ui): add useVariantAck hook with localStorage persistence"
```

---

## Task 7: Fleet Diff Hook

Same as plan round 1, Task 7. No changes needed.

```bash
git commit -m "feat(ui): add useFleetDiff hook with client-side cache"
```

---

## Task 8: Shell Extraction

**Files:**
- Create: `ui/src/components/AppShell.tsx`
- Modify: `ui/src/App.tsx`
- Test: `ui/src/components/__tests__/AppShell.test.tsx`

This task extracts shared shell surfaces from `App.tsx` into a
composable `AppShell` component. Both `SingleApp` (existing) and
`FleetApp` (new) will compose this shell. This is a refactor — no
behavioral changes, all existing tests must still pass.

- [ ] **Step 1: Identify shell surfaces in current App.tsx**

Current `App.tsx` directly owns these shared concerns (lines from
current source):
- `GlobalSearch` (lines ~23-24, ~62, ~255-270, ~388-394) — `Ctrl+K`
- Section search (lines ~55, ~58, ~445) — `/` key, `sectionSearchOpen` + `filterClearCounter` state
- `ExportDialog` (lines ~25, ~56, ~248, ~477-479) — `Ctrl+Shift+E`
- `ShortcutOverlay` (lines ~22, ~473-477) — `?` key
- `ContainerfilePanel` (lines ~20, ~452-453) — `Ctrl+E`
- `useKeyboard` (lines ~17, ~334-341) — all global shortcuts
- Toolbar/action area (export button, undo/redo buttons)

- [ ] **Step 2: Write test proving AppShell renders shared surfaces**

```typescript
describe("AppShell", () => {
  it("renders GlobalSearch", () => {
    // Render AppShell with minimal props
    // Assert: Ctrl+K opens search
  });

  it("renders ShortcutOverlay on ?", () => {
    // Press ?
    // Assert: overlay appears
  });

  it("renders ExportDialog on export trigger", () => {
    // Trigger export
    // Assert: dialog opens
  });

  it("renders children in the content area", () => {
    // Render with <div data-testid="content">
    // Assert: content visible
  });

  it("opens section search on / and passes filterText to onFilterChange", () => {
    // Press /
    // Assert: search input appears
    // Type "nginx"
    // Assert: onFilterChange called with "nginx"
  });

  it("clears section search on Escape", () => {
    // Open search, type text, press Escape
    // Assert: search closes, onFilterChange called with ""
  });
});
```

- [ ] **Step 3: Extract AppShell**

```tsx
// ui/src/components/AppShell.tsx
interface AppShellProps {
  // Section navigation
  sidebar: React.ReactNode;
  // Main content area — receives filterText for section search
  children: (filterText: string) => React.ReactNode;
  // Containerfile panel
  containerfilePreview?: string;
  // Data for shell surfaces
  generation: number;
  canUndo: boolean;
  canRedo: boolean;
  sessionIsSensitive: boolean;
  // Callbacks
  onUndo: () => void;
  onRedo: () => void;
  onExportComplete: () => void;
  // Section navigation for GlobalSearch
  sections: Array<{ id: string; label: string }>;
  activeSection: string;
  onNavigateSection: (sectionId: string) => void;
  // GlobalSearch data — mode-specific searchable index
  searchableItems: SearchableEntry[];
  // GlobalSearch result navigation — fleet uses this for portal flow
  onSearchNavigate?: (sectionId: string, itemId?: string) => void;
  // Section search — AppShell owns open/close state and filterText,
  // passes filterText to children via render prop.
  // filterText resets to "" when activeSection changes (useEffect).
  sectionSearchEnabled?: boolean;  // default true
  // Shortcut overlay — fleet appends extra bindings
  extraShortcuts?: Array<{ key: string; description: string }>;
  // Optional: fleet-specific toolbar additions
  toolbarExtra?: React.ReactNode;
}

// AppShell internally owns:
// - sectionSearchOpen: boolean (toggled by / key)
// - filterText: string (from search input)
// - exportDialogOpen: boolean
// - shortcutOverlayOpen: boolean
// - useKeyboard bindings (Ctrl+K, Ctrl+E, Ctrl+Shift+E, ?, /, 1-9, j/k)
//
// Children receive filterText via render prop:
//   {(filterText) => <FleetSection filterText={filterText} ... />}
//
// This means both SingleApp and FleetApp get section search
// without reimplementing the search UI or keyboard binding.

export function AppShell({ ... }: AppShellProps) {
  // Owns: useKeyboard, GlobalSearch, ExportDialog, ShortcutOverlay,
  // toolbar (undo/redo/export buttons), ContainerfilePanel
  // Renders sidebar + children in the PF6 Page layout
}
```

- [ ] **Step 4: Rewire App.tsx to use AppShell**

Existing App.tsx rendering moves inside AppShell. The current inline
layout becomes:

```tsx
function App() {
  const health = useHealth();
  // ... existing state

  if (health.data?.fleet) {
    return <FleetApp fleet={health.data.fleet} health={health.data} />;
  }

  // Single-host path uses AppShell with existing content
  return (
    <AppShell
      sidebar={<Sidebar ... />}
      containerfilePreview={viewData?.containerfile_preview}
      generation={viewData?.generation ?? 0}
      canUndo={viewData?.can_undo ?? false}
      canRedo={viewData?.can_redo ?? false}
      sessionIsSensitive={health.data?.session_is_sensitive ?? false}
      onUndo={mutation.undo}
      onRedo={mutation.redo}
      onExportComplete={refetchView}
      sections={sectionList}
      activeSection={activeSection}
      onNavigateSection={setActiveSection}
    >
      <MainContent ... />
    </AppShell>
  );
}
```

- [ ] **Step 5: Verify all existing tests pass**

Run: `cd inspectah-web/ui && npx vitest run`
Expected: ALL PASS — this is a refactor, no behavioral changes.

- [ ] **Step 6: Commit**

```bash
git add inspectah-web/ui/src/components/AppShell.tsx inspectah-web/ui/src/App.tsx inspectah-web/ui/src/components/__tests__/AppShell.test.tsx
git commit -m "refactor(ui): extract AppShell from App.tsx for fleet/single-host composition"
```

---

## Task 9: FleetApp Shell

**Files:**
- Create: `ui/src/components/fleet/FleetApp.tsx`
- Create: `ui/src/components/fleet/FleetSidebar.tsx`
- Test: `ui/src/components/fleet/__tests__/FleetApp.test.tsx`

- [ ] **Step 1: Implement FleetApp composing AppShell**

```tsx
// Banner portal navigation state machine
interface NavTarget {
  sectionId: string;
  itemId: ItemId;
  // Lifecycle: set by banner click → consumed by FleetSection after render
}

export function FleetApp({ fleet, health }: FleetAppProps) {
  const [view, setView] = useState<FleetViewResponse | null>(null);
  const [activeSection, setActiveSection] = useState("packages");
  const [error, setError] = useState<string | null>(null);

  // Banner portal state — set by banner click, consumed by FleetSection
  const [pendingNavTarget, setPendingNavTarget] = useState<NavTarget | null>(null);
  // Focus recovery — tracks last focused item for post-refetch restoration
  const lastFocusedItemRef = useRef<string | null>(null);

  useEffect(() => {
    fetchFleetView().then(setView).catch((e) => setError(e.message));
  }, []);

  const { mutate, undo, redo, isPending, refetchError, retry } = useFleetMutation(
    setView,
    (err) => setError(err.message),
  );

  const actionableIds = view?.summary.actionable_variant_items.map((v) => v.item_id) ?? [];
  const ack = useVariantAck(fleet.label, fleet.merged_at, actionableIds);

  // Banner navigation handler (portal pattern)
  const handleBannerNavigate = useCallback((sectionId: string, itemId: ItemId) => {
    setActiveSection(sectionId);
    setPendingNavTarget({ sectionId, itemId });
    // FleetSection consumes pendingNavTarget after render — see Task 11
  }, []);

  if (!view) return <LoadingState />;

  const sectionList = view.sections.map((s) => ({ id: s.id, label: s.display_name }));

  return (
    <AppShell
      sidebar={
        <FleetSidebar
          sections={view.sections}
          activeSection={activeSection}
          onSelect={setActiveSection}
          ackState={ack}
        />
      }
      containerfilePreview={view.containerfile_preview}
      generation={view.generation}
      canUndo={view.can_undo}
      canRedo={view.can_redo}
      sessionIsSensitive={view.session_is_sensitive}
      onUndo={undo}
      onRedo={redo}
      onExportComplete={() => fetchFleetView().then(setView)}
      sections={sectionList}
      activeSection={activeSection}
      onNavigateSection={setActiveSection}
      toolbarExtra={<AckProgress count={ack.unackedCount} />}
      extraShortcuts={[
        { key: "c", description: "Compare variants (when variant view open)" },
      ]}
    >
      {(filterText) => (
        <>
          <FleetBanner
            summary={view.summary}
            ackState={ack}
            onNavigate={handleBannerNavigate}
          />
          <FleetSection
            section={view.sections.find((s) => s.id === activeSection)}
            filterText={filterText}
            mutate={mutate}
            ack={ack}
            pendingNavTarget={pendingNavTarget}
            onNavTargetConsumed={() => setPendingNavTarget(null)}
            lastFocusedItemRef={lastFocusedItemRef}
          />
        </>
      )}
    </AppShell>
  );
}
```

FleetApp gets GlobalSearch, section search, ExportDialog,
ShortcutOverlay, ContainerfilePanel, undo/redo, keyboard shortcuts —
all via AppShell. Section search filters fleet items via the
`filterText` render prop. No duplication.

- [ ] **Step 2: Implement FleetSidebar**

Section nav with PF6 `Nav` component. Shows zone counts per section.
Decision sections with config variants show ack progress ("2/4
confirmed"). Context sections do not show ack progress.

- [ ] **Step 3: Write tests, commit**

```bash
git commit -m "feat(ui): add FleetApp shell composing AppShell with fleet content"
```

---

## Task 10: ZoneGroup Component

Same as plan round 1, Task 9. No changes needed — the component is
self-contained and has no upstream dependencies.

```bash
git commit -m "feat(ui): add ZoneGroup collapsible zone container"
```

---

## Task 11: FleetSection + FleetItemRow

**Files:**
- Create: `ui/src/components/fleet/FleetSection.tsx`
- Create: `ui/src/components/fleet/FleetItemRow.tsx`
- Tests for both

FleetSection renders the active section. Zone-grouped when `zones`
is non-null, flat when null. Manages zone collapse state in a
`Record<string, boolean>` (persists across refetch).

**FleetSection owns the banner portal flow.** When `pendingNavTarget`
is non-null:

1. If the target item is inside a collapsed zone, auto-expand that zone
2. Auto-expand the target item row (show VariantView if it has variants)
3. `scrollIntoView({ behavior: 'smooth', block: 'center' })` on the
   target element (found via `[data-item-id="..."]` attribute)
4. Apply CSS highlight class (outline pulse, ~1.5s, removed on
   `animationend`)
5. Move focus to the target item row
6. Call `onNavTargetConsumed()` to clear the pending state

This runs in a `useEffect` keyed on `pendingNavTarget`. The effect
fires after React renders the section (which may have just switched
via `setActiveSection`).

**Focus fallback after refetch:** When `lastFocusedItemRef.current` is
set, `useEffect` after view update attempts:
```typescript
const el = document.querySelector(`[data-item-id="${lastFocusedItemRef.current}"]`);
if (el) (el as HTMLElement).focus();
else {
  // Item no longer exists (removed by undo) — focus nearest sibling
  // or zone header
  const firstItem = document.querySelector('[data-item-id]');
  if (firstItem) (firstItem as HTMLElement).focus();
}
```

**Section search filtering:** FleetSection receives `filterText` from
AppShell's render prop. When non-empty, items are filtered by matching
`filterText` against the display text derived from `item_id`. Filtered
items are hidden, not removed — zone counts stay stable.

FleetItemRow renders: toggle (if decision section), item name,
prevalence chip ("8/12 hosts"), variant indicator ("3 variants"),
attention badge. Expanding shows VariantView for config items with
variants, or attention detail for all others. Each row carries
`data-item-id={JSON.stringify(item.item_id)}` for scroll-to targeting.

FleetItemRow tracks focus: `onFocus` sets `lastFocusedItemRef.current`
to the item's serialized `item_id`.

Context section items render without the toggle (read-only).
`is_decision_section` from the parent section controls this.

Zone headers suppressed when section has items in only one zone.

```bash
git commit -m "feat(ui): add FleetSection and FleetItemRow with zone rendering"
```

---

## Task 12: VariantView + DiffDrawer

Same as plan round 1, Task 11. Components are self-contained and
consume the types defined in Task 4.

Key test additions for this revision:
- DiffDrawer shows inline error with Retry on load failure
- VariantView auto-confirms via ack hook when different variant selected

```bash
git commit -m "feat(ui): add VariantView with radio select and DiffDrawer"
```

---

## Task 13: FleetBanner

Same as plan round 1, Task 12, with the portal pattern from the spec:
banner click calls `onNavigate(sectionId, itemId)` which sets
`activeSection` then scrolls to the target.

Banner items show section tag (e.g., "[Config]"). Informational line
for non-config variants rendered separately.

```bash
git commit -m "feat(ui): add FleetBanner with severity scaling and portal navigation"
```

---

## Task 14: Fleet Keyboard Extensions

Extend keyboard handling for fleet-specific interactions. Since
`useKeyboard` is now owned by `AppShell` (Task 8), fleet-specific
bindings are either:
- Additional bindings passed via an `extraBindings` prop to AppShell, or
- A `useFleetKeyboard` hook in FleetApp that handles fleet-only keys
  (c for Compare, Escape for close diff, arrow keys in radio groups)

Global shortcuts (`Ctrl+K`, `Ctrl+E`, `Ctrl+Shift+E`, `?`, `1-9`,
`j/k`) are already carried via AppShell.

Focus recovery: track `lastFocusedItemId` in a ref. After refetch,
`useEffect` attempts `getElementById(lastFocusedItemId)?.focus()`.

```bash
git commit -m "feat(ui): add fleet keyboard shortcuts and focus recovery"
```

---

## Task 15: Integration Tests

Tests using Vitest + Testing Library that render FleetApp with mocked
API responses and verify end-to-end flows:

- Full fleet view render with zones
- SelectVariant → mutation → refetch → selection updated, zone collapse preserved
- **Full banner portal flow:**
  - Banner click → `pendingNavTarget` set → active section switches
  - Target zone auto-expands if collapsed
  - Target variant view auto-opens if item has variants
  - Target item receives highlight class
  - Focus lands on target item
  - `onNavTargetConsumed` called (pendingNavTarget cleared)
- **Focus fallback after refetch:**
  - Item still exists → focus restored to same item
  - Item removed (by undo) → focus moves to nearest sibling or zone header
- Ack flow → banner count decrements (actionable config items only)
- Undo → selection reverts, ack preserved
- Fleet-of-2 → flat rendering, no zone headers
- Refetch failure → error overlay with Retry, lastConfirmedView held
- Section search → `/` opens search, typing filters items, Escape clears
- Keyboard: j/k navigation, Enter expand, radio arrows, c Compare, Escape close

```bash
git commit -m "test(ui): add fleet integration tests"
```

---

## Task 16: E2E Tests

**Harness strategy:** Each test uses a **fresh refine server session**.
The E2E setup script starts `inspectah refine` with the fleet test
fixture, and `afterEach` kills and restarts the server. This prevents
stateful test pollution (variant selections, ack state in localStorage).

```typescript
// ui/e2e/fleet.spec.ts
import { test, expect } from "@playwright/test";

let serverProcess: ChildProcess;

test.beforeEach(async () => {
  // Start fresh refine server with fleet fixture
  serverProcess = spawn("cargo", ["run", "--", "refine",
    "testdata/fleet-e2e.tar.gz", "--no-browser", "--port", "8642"]);
  await waitForServer("http://127.0.0.1:8642/api/health");
});

test.afterEach(async () => {
  serverProcess.kill();
  // Clear localStorage between tests
});
```

**Playwright parallelism:** `fullyParallel: false` for fleet tests.
Each test gets a fresh server, and server startup/teardown is not
parallelism-safe (port conflicts). Fleet E2E runs serial within the
fleet suite; other suites can run parallel.

Tests covering the full portal path and focus:
- Zone headers render with correct counts
- Variant selection persists after re-render
- Ack flow: confirm → banner count decrements → checkmark appears
- **Banner portal (full path):** click banner item → section switches →
  zone auto-expands → variant view auto-opens → item highlighted →
  focus on target item (verify via `document.activeElement`)
- **Focus fallback:** select variant → undo → verify focus lands on
  nearest item (not lost to body)
- Diff comparison: expand variant → Compare → diff renders with
  add/delete lines
- Section search: `/` → type → items filter → Escape → filter clears
- Export: verify download triggered
- Undo/redo: select variant → undo → verify original selection
- a11y audit: axe-core, no critical violations

```bash
git commit -m "test(e2e): add fleet refine E2E tests with per-test server reset"
```

---

## Task Dependencies

```
Backend (parallel lane):
  Task 1 (Health) → Task 2 (View) → Task 3 (Diff)

Frontend hooks (parallel lane, can use mocks):
  Task 4 (Types+Client) → Task 5 (Mutation) ─┐
                           Task 6 (Ack)  ─────┤
                           Task 7 (Diff) ─────┘

Shell extraction (blocks fleet components):
  Task 8 (AppShell) — depends on nothing fleet-specific

Fleet components (sequential after Task 8 + hooks):
  Task 9 (FleetApp) → Task 10 (ZoneGroup) → Task 11 (Section+ItemRow)
    → Task 12 (VariantView+Diff) → Task 13 (Banner)

Polish (sequential after components):
  Task 14 (Keyboard) → Task 15 (Integration) → Task 16 (E2E)
```

**Checkpoints:**
- After Task 3: backend complete, fleet endpoints ready
- After Task 8: shell extracted, single-host verified unchanged
- After Task 13: all fleet components built
- After Task 16: feature complete with full test coverage

---

## Verification Checklist

Before marking the feature complete:

- [ ] `cargo test --package inspectah-web` passes (all fleet handler tests)
- [ ] `cd inspectah-web/ui && npx vitest run` passes (all unit + integration tests)
- [ ] `cd inspectah-web/ui && npx playwright test e2e/fleet.spec.ts` passes
- [ ] `cargo clippy --workspace` clean
- [ ] `cd inspectah-web/ui && npx tsc --noEmit` clean
- [ ] Manual: load a fleet tarball, verify zone headers, variant selection, diff, banner, ack flow
- [ ] Manual: load a single-host tarball, verify no fleet UI (regression check)
- [ ] Manual: load a fleet-of-2 tarball, verify no zone headers, variant ops work
- [ ] Manual: export fleet tarball, verify selected variant in output

## Review History

### Round 1
Panel: Tang, Kit, Thorn, Fern. Verdict: request-changes (4/4).

Three MUST-FIX themes addressed in revision 2:
1. Fleet metadata seam corrected — uses `snapshot.fleet_meta` and
   `session.fleet_context()`, not `snap.meta["fleet"]`
2. Shell extraction task added (Task 8) — shared surfaces composable
   via `AppShell` before fleet components build
3. Fleet-mode data contract pinned — `FleetViewResponse` includes
   `containerfile_preview`, `session_is_sensitive`. Refetch-failure
   contract explicit with `lastConfirmedView`. `useFleetMutation`
   holds last-good state.

Three SHOULD-FIX themes addressed:
- `users_groups` handling defined (decision section, no variants, maps
  existing user decision ops into FleetItem rows)
- Fleet-of-2 and single-host fallback pinned in Section-Source Contract
- E2E harness strategy defined (fresh server per test, localStorage
  cleared between tests)
- Task ordering made execution-safe (Task 8 before all fleet components)

### Round 2
Panel: Tang, Kit, Thorn, Fern. Verdict: request-changes (1 approve,
3 request-changes). Kit approved.

Three MUST-FIX themes addressed in revision 3:
1. `users_groups` contract — dedicated `FleetUserRow` DTO instead of
   forcing into generic `FleetItem`. No `ItemId`, no variants. Own
   response field (`users` instead of `items`). Ops via existing
   `UserStrategy`/`UserPassword` endpoints.
2. Section search carried through AppShell — `sectionSearchOpen` and
   `filterText` state owned by AppShell, passed to content area via
   render prop `children: (filterText: string) => ReactNode`. Fleet
   content filters items by display text. Tests added.
3. Full banner portal flow made explicit — `pendingNavTarget` state in
   FleetApp, consumed by FleetSection via `useEffect`. Six-step
   lifecycle documented: zone auto-expand → item auto-expand → scroll →
   highlight → focus → clear pending. Focus fallback for missing items
   after refetch. Integration + E2E tests cover full path.

Non-blocking addressed:
- Playwright parallelism pinned: `fullyParallel: false` for fleet suite
- AppShell `extraShortcuts` prop for fleet-specific shortcut overlay

### Round 3
Panel: Tang, Kit, Thorn, Fern. Verdict: request-changes (1 approve,
3 request-changes). Fern approved (portal/focus blocker closed).

Two MUST-FIX themes addressed in revision 4:
1. `users_groups` deferred from fleet v1 — current backend does not
   produce per-user `FleetPrevalence` or attention scores. User entries
   lack fleet metadata fields and are not part of `RefinedView`.
   Adding fleet-aware user merging would require upstream core work
   out of scope. `FleetUserRow` DTO removed. `users_groups` omitted
   from `/api/fleet/view`.
2. AppShell search contract fully pinned — `SearchableEntry` type
   defined for GlobalSearch data. Mode-specific searchable index passed
   via `searchableItems` prop. `onSearchNavigate` callback for fleet
   portal flow from search results. Section search filter reset rules
   explicit: resets on `activeSection` change (any cause), preserved
   across refetch.
