# Fleet Refine UI Spec (Phase 2b)

## Overview

Fleet Phase 2b adds the web frontend for interactive fleet refinement,
built against the Phase 2a engine's API contract. This spec covers
dedicated fleet API endpoints, a separate frontend component tree, zone
headers, variant comparison, a variant summary banner, and per-item
variant acknowledgment.

### Phasing Context

- **Phase 1 (shipped):** Fleet aggregate — merge engine, CLI, tarball output
- **Phase 2a (shipped):** Fleet refine engine — Rust backend for zone
  classification, variant ops, diff, auto-save, session persistence
- **Phase 2b (this spec):** Fleet refine UI — web frontend consuming the
  engine's capabilities
- **Phase 3 (future):** Fleet architect — cross-role hierarchy, multi-artifact
  decomposition

### Spec Boundary

This spec owns:
- **HTTP endpoints:** fleet-specific endpoint paths, request/response JSON
  schemas, handler wiring in `inspectah-web`
- **UI components:** fleet component tree, zone group rendering, variant
  comparison view, diff drawer, summary banner
- **Interaction patterns:** variant selection, diff comparison, zone
  collapse, banner navigation, variant acknowledgment
- **Fleet mutation contract:** how the fleet frontend uses shared mutation
  endpoints and manages post-mutation state

This spec does NOT own:
- **Engine types and logic:** zone classification, variant ops execution,
  diff computation, auto-save, session persistence (Phase 2a)
- **Editor drawer:** inline variant editing via `EditVariant` and
  `DiscardVariant` ops (deferred to a follow-up spec)
- **Threshold controls:** interactive prevalence threshold adjustment
  (deferred; fixed zones in v1, add if users request)
- **Single-host refine UI:** existing single-host component tree, hooks,
  and endpoints are unchanged by this spec

### Scope Exclusions

**Editor drawer deferred.** The variant lifecycle in this spec is
Select + Compare only. Users pick a winner from existing host-sourced
variants; they cannot create or edit variants through the UI. The engine
supports `EditVariant` and `DiscardVariant` already — the UI for these
ships in a follow-up spec. Users who need to edit variant content can
manually edit tarball files after export.

**Threshold control deferred.** The three prevalence zones are fixed at
100% / 50% / <50%. No interactive threshold adjustment. If real users
request zone boundary tuning, the upgrade path is Option 1 from the
brainstorm: threshold adjusts the Consensus boundary with dynamic
reclassification.

**Variant UI for non-config sections deferred.** The engine supports
variant ops for Config, DropIn, Quadlet, and Compose items. This spec
ships variant selection UI for **config files only**. DropIn, Quadlet,
and Compose are currently context sections in the frontend (read-only
presentation via `/api/snapshot/sections`). Promoting them to decision
sections requires separate UI work. The engine support is ready — the
UI for those types ships in a follow-up.

### Fleet Section Inventory

The current single-host UI distinguishes decision sections (mutable,
with include/exclude toggles) from context sections (read-only
presentation). Fleet v1 preserves this distinction:

**Decision sections (full toggle + zone grouping + variant support):**
- `packages` — RPM packages, repos, module streams, version locks
- `configs` — config files (variant selection available here)
- `users_groups` — user/group strategy decisions

**Context sections (prevalence badges, zone grouping, but read-only):**
- `services` — service state changes, drop-ins (variant UI deferred)
- `containers` — quadlet units, compose files (variant UI deferred)
- `network`, `storage`, `scheduled`, `selinux`, `kernel_boot`, `nonrpm`

**`/api/fleet/view` is the sole content source** for the fleet main
pane. Fleet mode does NOT call `/api/snapshot/sections` — all section
data (decision + context) comes from the fleet view endpoint.

## Architecture

### Approach: Dedicated Fleet Surface

Fleet and single-host are separate API surfaces with separate frontend
component trees. The fleet aggregate snapshot is structurally different
from a single-host snapshot — it represents statistical relationships
between hosts, not a single machine's state. A dedicated surface keeps
each mode simple and avoids conditional branching that accumulates debt
as fleet features grow.

**Rationale:** Fleet is the enterprise path. Building it as a layer on
top of single-host inverts the actual usage hierarchy. Separate surfaces
mean fleet changes never risk regressing single-host mode.

### Navigation Model: Single Active Section

Fleet preserves the existing single-active-section interaction model.
`FleetApp` renders one section at a time, selected via `FleetSidebar`.
This is the same pattern as single-host mode in `App.tsx` /
`MainContent.tsx`.

**Banner navigation** uses a portal pattern (section switch + scroll),
not cross-section scrolling:

1. Banner item click calls `setActiveSection(targetSectionId)`
2. After React renders the new section, a `useEffect` keyed on
   `activeSection` calls `scrollIntoView({ behavior: 'smooth',
   block: 'center' })` on the target item's DOM element
3. A CSS highlight animation (outline pulse, ~1.5s) marks the target
4. If the target item is inside a collapsed zone group, the zone
   auto-expands before scroll

Banner items show section membership (e.g., "nginx.conf [Config]")
so users know where the click will take them.

### Post-Refetch UI State Preservation

After any mutation (`/api/op`, `/api/undo`, `/api/redo`), the fleet
frontend re-fetches `GET /api/fleet/view`. The following UI state
**persists across refetch** (held in React state, not in the server
response):

| State | Persists | Mechanism |
|-------|----------|-----------|
| Active section | yes | React state (`activeSection`) |
| Zone collapse/expand | yes | React state (per-zone-id map) |
| Expanded item rows | yes | React state (Set of item IDs) |
| Open variant view | yes | React state (Set of item IDs) |
| Open diff + compare pair | **no** — diff cache survives, but the drawer closes | Diff cache (Map), drawer state resets |
| Variant ack state | yes | localStorage (survives refetch and reload) |
| Banner dismiss | yes | React state |
| Scroll position | **no** — re-render may shift layout | Scroll resets to section top |
| Highlight animation | **no** — one-shot, clears after animation | CSS animation class removed on `animationend` |
| Keyboard focus | restored to previously focused item if still present | `useEffect` with ref to last-focused item ID |

**Diff drawer closes on refetch** because the underlying variant data
may have changed (user selected a different variant, or undo reverted
a selection). The diff cache is keyed by (itemId, baseHash, targetHash)
and survives — re-opening the same comparison is instant.

### Crate Changes

**inspectah-web** (bulk of the work):
- New `fleet_handlers.rs` module with fleet-specific endpoint handlers
- New fleet response DTOs (`FleetViewResponse`, `FleetSectionResponse`,
  `FleetItemResponse`, `VariantInfo`, `FleetDiffRequest`,
  `FleetDiffResponse`)
- Router additions for `/api/fleet/*` endpoints
- Health endpoint enrichment with fleet context

**inspectah-web/ui** (frontend):
- New `FleetApp.tsx` top-level component
- New component directory: `src/components/fleet/`
- New `src/api/fleet-client.ts` for fleet-specific API calls
- New `src/hooks/useFleetMutation.ts` for fleet mutation flow
- Shared primitives remain in `src/components/`

### Module Layout

```
inspectah-web/
  src/
    lib.rs            — router with fleet endpoint additions
    handlers.rs       — existing single-host handlers (unchanged)
    fleet_handlers.rs — NEW: fleet-specific endpoint handlers
    assets.rs         — unchanged
    error.rs          — unchanged
  ui/src/
    App.tsx           — fork point: FleetApp vs SingleApp
    api/
      client.ts       — existing single-host API client (unchanged)
      fleet-client.ts — NEW: fleet-specific API calls
    hooks/
      useMutation.ts  — existing (unchanged)
      useFleetMutation.ts — NEW: fleet mutation flow
    components/
      fleet/
        FleetApp.tsx
        FleetBanner.tsx
        FleetSidebar.tsx
        FleetSection.tsx
        ZoneGroup.tsx
        FleetItemRow.tsx
        VariantView.tsx
        DiffDrawer.tsx
      # existing components unchanged
```

### Data Flow

```
User loads tarball
  → CLI detects fleet snapshot (FleetSnapshotMeta present)
  → RefineSession created with RefineMode::Fleet(FleetContext)
  → axum server starts

Frontend init:
  → GET /api/health
  → response.fleet !== null → mount <FleetApp>
  → GET /api/fleet/view
  → render active section with zone groups

User selects variant:
  → POST /api/op (existing endpoint)
  → endpoint returns ViewResponse (existing contract, ignored by fleet)
  → useFleetMutation re-fetches GET /api/fleet/view
  → UI state preserved per table above

User compares variants:
  → POST /api/fleet/diff (JSON body with ItemId + hashes)
  → DiffDrawer renders unified diff
  → diff cached client-side by (itemId, base, target)

User acknowledges variant:
  → click Confirm on variant card (or select a different variant)
  → ack state written to localStorage
  → banner unreviewed count decrements

User exports:
  → POST /api/tarball (existing, shared)
  → tarball includes selected variant content in main artifacts;
    alternative (non-selected) raw-content variants for configs,
    drop-ins, and quadlets are materialized under fleet/variants/
    as a convenience (compose variants are not materialized —
    structured carrier)
```

## API Surface

### Fleet Detection at Init

The existing `GET /api/health` response gains a `fleet` field. The
fleet metadata is derived from `FleetSnapshotMeta` in the snapshot.

```json
{
  "status": "ok",
  "generation": 42,
  "fleet": {
    "host_count": 12,
    "hostnames": ["web-01", "web-02", "db-01"],
    "zones_active": true,
    "variant_count": 4,
    "label": "fleet-merged",
    "merged_at": "2026-05-21T18:30:00Z"
  }
}
```

- `fleet` is `null` for single-host snapshots (no `FleetSnapshotMeta`)
- `zones_active` is `false` for fleet-of-2 (zone presentation suppressed)
- `variant_count` is the total number of **actionable** variant items
  (config files in v1 — does not count read-only context-section
  variants)
- `hostnames` maps directly to `FleetSnapshotMeta.hostnames`
- `host_count` maps to `FleetSnapshotMeta.host_count`

Frontend reads `fleet` once at init to determine which component tree
to mount. This field does not change during a session.

### Fleet Endpoints

Two fleet-specific endpoints plus shared mutation endpoints:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/fleet/view` | GET | Zone-grouped view with variant metadata, includes summary |
| `/api/fleet/diff` | POST | On-demand pairwise unified diff |
| `/api/op` | POST | Shared — all ops including `SelectVariant` |
| `/api/undo` | POST | Shared — undo last op |
| `/api/redo` | POST | Shared — redo |
| `/api/tarball` | POST | Shared — export |

Fleet mode does NOT use `/api/view`, `/api/snapshot/sections`, or
`/api/viewed`. Those endpoints serve single-host mode only.

### Fleet Mutation Contract

The existing shared mutation endpoints (`/api/op`, `/api/undo`,
`/api/redo`) continue to return a full `ViewResponse` — this is the
existing contract and changing it would affect single-host mode.

Fleet mode uses a new `useFleetMutation` hook that:

1. Calls the existing mutation endpoint (e.g., `applyOp()` from
   `client.ts`)
2. Ignores the returned `ViewResponse` (it is single-host-shaped)
3. Re-fetches `GET /api/fleet/view` to get fleet-specific state
4. Compares the new `generation` to detect staleness

This requires no backend changes to mutation endpoints. The re-fetch
is fast (local server, ~10ms). The existing `useMutation` hook in
`src/hooks/useMutation.ts` is NOT reused — `useFleetMutation` is a
new hook with the fetch-after-mutate pattern instead of the current
mutate-returns-view pattern.

```typescript
// src/hooks/useFleetMutation.ts
function useFleetMutation(onViewUpdate: (view: FleetViewResponse) => void) {
  // Wraps applyOp/undo/redo from client.ts
  // After each mutation succeeds:
  //   1. Calls fetchFleetView()
  //   2. Compares generation
  //   3. Calls onViewUpdate with new fleet view
  // Queues mutations sequentially (same pattern as useMutation)
}
```

### Async Failure Recovery

**Mutation succeeds but fleet refetch fails:**
- The mutation endpoint returned success, so the server state changed.
- The fleet frontend shows an inline error banner: "View update failed.
  Your change was saved." with a "Retry" button.
- The mutation queue clears (same behavior as current `useMutation`
  error handling). No further queued ops are attempted.
- Retry calls `fetchFleetView()` again. On success, normal flow resumes.
- The UI does NOT revert to a stale view — it holds the last confirmed
  view and shows the error overlay. Focus stays on the error banner.

**Diff load fails after Compare click:**
- `DiffDrawer` renders an inline error: "Could not load diff" with a
  "Retry" button.
- The variant view stays open — only the diff region shows the error.
- Focus moves to the Retry button.
- The diff cache does NOT store failed results — retry re-fetches.

**Queued mutations encounter refetch failure:**
- The queue clears on any error (mutation or refetch). This matches
  the current `useMutation` contract where `onError` clears the queue.
- Pending queued ops are lost — the user must re-initiate them after
  recovery. This is acceptable because mutation queues in practice
  contain 0-1 pending items (variant selection is a discrete action,
  not a rapid-fire sequence).

### GET /api/fleet/view

Returns the fleet view with items grouped by prevalence zone per
section, plus a top-level summary for the variant banner. This is a
fleet-specific DTO, not the engine's `RefinedView` serialized directly.

```json
{
  "generation": 42,
  "can_undo": true,
  "can_redo": false,
  "summary": {
    "host_count": 12,
    "actionable_variant_items": [
      {
        "item_id": { "kind": "Config", "key": { "path": "/etc/nginx/nginx.conf" } },
        "section_id": "configs",
        "variant_count": 3,
        "max_host_spread": 8
      }
    ],
    "informational_variant_count": 3
  },
  "sections": [
    {
      "id": "packages",
      "display_name": "RPM Packages",
      "is_decision_section": true,
      "zones": {
        "consensus": { "items": [], "count": 84 },
        "near_consensus": { "items": [], "count": 12 },
        "divergent": { "items": [], "count": 3 }
      }
    }
  ]
}
```

When `zones_active` is `false` (fleet-of-2), sections use a flat shape:

```json
{
  "id": "configs",
  "display_name": "Config Files",
  "is_decision_section": true,
  "zones": null,
  "items": []
}
```

Frontend checks: if `section.zones` is non-null, render zone groups.
If `section.zones` is null but `section.items` exists, render flat list.

### Fleet Item Shape

Each item in a zone group (or flat list). This is a fleet-specific DTO
constructed by `fleet_handlers.rs`, not a direct serialization of engine
types.

```json
{
  "item_id": { "kind": "Config", "key": { "path": "/etc/nginx/nginx.conf" } },
  "include": true,
  "attention": {
    "level": "needs_review",
    "reason": "config_default",
    "zone": "divergent",
    "prevalence": 8
  },
  "prevalence": { "count": 8, "total": 12 },
  "variants": {
    "count": 3,
    "selected": "a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8",
    "options": [
      {
        "hash": "a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8",
        "hosts": ["web-01", "web-02", "web-03", "app-01", "app-02",
                  "app-03", "app-04", "app-05"],
        "host_count": 8,
        "selected": true
      },
      {
        "hash": "b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1",
        "hosts": ["db-01", "db-02", "db-03"],
        "host_count": 3,
        "selected": false
      }
    ]
  }
}
```

**Field semantics:**

- `item_id` uses the engine's `ItemId` serde encoding: adjacently
  tagged with `#[serde(tag = "kind", content = "key")]`. Each variant
  has a single struct field under `key`. Examples:
  - Config: `{"kind": "Config", "key": {"path": "/etc/nginx/nginx.conf"}}`
  - Package: `{"kind": "Package", "key": {"name_arch": "httpd.x86_64"}}`
  - DropIn: `{"kind": "DropIn", "key": {"path": "/etc/systemd/system/httpd.service.d/override.conf"}}`
  - Service: `{"kind": "Service", "key": {"unit": "httpd.service"}}`
  - Compose: `{"kind": "Compose", "key": {"path": "/opt/app/docker-compose.yml"}}`
- `attention.level` is `AttentionLevel` serialized as snake_case
  (`"needs_review"`, `"informational"`, `"routine"`)
- `attention.reason` is the fleet DTO's normalized reason string.
  The handler maps `AttentionReason` variants to snake_case strings
  (e.g., `"config_default"`, `"package_user_added"`). The engine's
  `Custom(String)` variant, if present, is passed through as-is.
  The frontend treats `reason` as an opaque display string — it does
  not branch on specific reason values.
- `attention.zone` is `PrevalenceZone` serialized as snake_case
  (`"consensus"`, `"near_consensus"`, `"divergent"`), or `null` when
  `zones_active` is false
- `attention.prevalence` is the raw host count from `FleetAttention`
- `variants` is `null` for items without multiple variants (the vast
  majority). When present, `selected` is always non-null — the engine
  deterministically selects a winner for every multi-variant item,
  including tie-breaks by lexicographic content hash. There is no
  "unresolved" state at the engine level.
- `variants.selected` is the full 64-character hex `ContentHash` of
  the currently selected variant

**Items without toggles:** Context section items (services, containers,
etc.) have `include` but it is read-only in the UI — no toggle rendered.
The `is_decision_section` flag on the parent section controls whether
toggles render.

### POST /api/fleet/diff

On-demand pairwise unified diff between two variant contents. Uses POST
with a JSON body because `ItemId`'s adjacently-tagged struct payloads
cannot be losslessly encoded in query parameters.

**Request:**
```json
{
  "item_id": { "kind": "Config", "key": { "path": "/etc/nginx/nginx.conf" } },
  "base": "a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8",
  "target": "b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1"
}
```

**Response:**

The response is a fleet-specific DTO, not a direct serialization of the
engine's `DiffResult`. The handler calls `compute_diff()` and maps the
result into this wire format:

```json
{
  "base_hash": "a1b2c3d4...",
  "target_hash": "b2c3d4e5...",
  "base_hosts": ["web-01", "web-02", "app-01"],
  "target_hosts": ["db-01", "db-02", "db-03"],
  "hunks": [
    {
      "base_range": { "start": 10, "count": 8 },
      "target_range": { "start": 10, "count": 8 },
      "changes": [
        { "kind": "equal", "content": "http {" },
        { "kind": "equal", "content": "    include       /etc/nginx/mime.types;" },
        { "kind": "delete", "content": "    worker_connections 1024;" },
        { "kind": "insert", "content": "    worker_connections 2048;" },
        { "kind": "equal", "content": "    keepalive_timeout 65;" }
      ]
    }
  ],
  "stats": {
    "total_changes": 5,
    "insertions": 2,
    "deletions": 2
  }
}
```

The engine's `DiffHunk` uses `LineRange { start, count }` (zero-based
`start`) and `DiffChange { kind: ChangeKind, content: String }`. The
DTO maps `ChangeKind` variants to the wire strings: `Equal` → `"equal"`,
`Delete` → `"delete"`, `Insert` → `"insert"`. The `DiffStats` fields
map 1:1: `total_changes`, `insertions`, `deletions`.

Host lists for each variant are resolved by the handler from the
fleet view data (the diff engine works on content strings only).

**Error cases** (all use 422 to match current web endpoint semantics
where invalid-but-parseable input returns Unprocessable Entity):
- Unknown `ItemId`: 422 with `{"error": "item not found"}`
- Unknown `ContentHash`: 422 with `{"error": "variant not found"}`
- Binary content: 422 with `{"error": "binary content, diff not available"}`
- Content too large (>100KB): 422 with `{"error": "content exceeds diff limit"}`

### POST /api/op (SelectVariant)

Variant selection goes through the existing `/api/op` endpoint. The
`SelectVariant` op is already a variant of the `RefinementOp` enum.

The `RefinementOp` enum uses `#[serde(tag = "op", content = "target")]`.
`SelectVariant` has two fields: `item_id: ItemId` and `target:
ContentHash`. The wire format is:

**Request:**
```json
{
  "op": "SelectVariant",
  "target": {
    "item_id": { "kind": "Config", "key": { "path": "/etc/nginx/nginx.conf" } },
    "target": "a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8a1b2c3d4e5f6g7h8"
  }
}
```

Note: the outer `"target"` is the serde adjacent-tag content wrapper.
The inner `"target"` is the `ContentHash` field of `SelectVariant`.
This is confusing but matches the actual serde contract.

**Response:** Returns a full `ViewResponse` (existing single-host
contract). The fleet frontend ignores this response body and re-fetches
`GET /api/fleet/view` to get fleet-specific state. See Fleet Mutation
Contract above.

## Frontend Architecture

### Top-Level Fork

```tsx
function App() {
  const health = useHealth();

  if (health.fleet) {
    return <FleetApp fleet={health.fleet} />;
  }
  return <SingleApp />;  // existing, unchanged
}
```

The fork happens once at init based on `/api/health`. No conditional
branches leak into child components.

### Component Tree

```
<FleetApp>
  ├─ <FleetBanner />              variant summary, severity-styled
  ├─ <FleetSidebar />             section nav with zone counts
  │    props: sections, activeSection, onSelect, ackCounts
  ├─ <ContainerfilePanel />       shared, reused as-is
  └─ <FleetSection>               active section only (single-active-section model)
       ├─ <ZoneGroup zone="consensus" defaultCollapsed={true}>
       │    └─ <FleetItemRow /> ...
       ├─ <ZoneGroup zone="near_consensus" defaultCollapsed={false}>
       │    └─ <FleetItemRow /> ...
       └─ <ZoneGroup zone="divergent" defaultCollapsed={false}>
            └─ <FleetItemRow>
                 └─ <VariantView />   inline when item has variants
                      └─ <DiffDrawer />   expanded on Compare click
```

### Shared Shell Surfaces

The following current UI surfaces carry into fleet mode:

| Surface | Fleet behavior | Notes |
|---------|---------------|-------|
| Sidebar shell (layout) | Reused — `FleetSidebar` fills the slot | Structure shared, content fleet-specific |
| ContainerfilePanel | Reused as-is | Reads from projected snapshot, mode-agnostic |
| AttentionBadge | Reused as-is | Renders attention level regardless of mode |
| Theme tokens, CSS vars | Reused as-is | All PF6 tokens carry over |
| Global search (`Ctrl+K`) | **Carried** — searches within active fleet section | Same shortcut, fleet-scoped results |
| Section search (`/`) | **Carried** — filters items in active section | Same behavior |
| Containerfile toggle (`Ctrl+E`) | **Carried** — toggles ContainerfilePanel | Same behavior |
| Export (`Ctrl+Shift+E`) | **Carried** — opens ExportDialog | Same shortcut |
| Shortcut overlay (`?`) | **Carried** — extended with fleet-specific keys | Fleet keys appended to overlay |
| Toolbar/action area | **Adapted** — fleet adds ack progress indicator | Export button reused, ack progress new |
| `ExportDialog` | **Adapted** — see export contract below | Not reused unchanged |

**Surfaces NOT carried into fleet v1:**
- Mobile/responsive overlay: fleet v1 is desktop-only (PatternFly
  responsive breakpoints apply, but no fleet-specific mobile layout)

### Export Contract in Fleet Mode

`ExportDialog` is reused but requires adaptation:

- **Trigger:** same export button in toolbar, same `Ctrl+E` shortcut
- **Generation check:** uses `generation` from fleet view response
  (same semantic, different source)
- **Sensitive content:** `session_is_sensitive` is read from
  `/api/health` response (shared, mode-agnostic) — no change needed
- **Post-export refresh:** fleet frontend re-fetches
  `GET /api/fleet/view` after export (not `/api/view`). The
  `ExportDialog` calls an `onExportComplete` callback; fleet wires
  this to `useFleetMutation.refetch()`
- **Dialog content:** unchanged (file name, generation guard, sensitive
  acknowledgment). Fleet does not add fleet-specific export options
  in v1.

### New Fleet Components

| Component | Purpose | Key Props |
|-----------|---------|-----------|
| `FleetBanner` | Variant summary strip at top | `variantItems`, `ackState`, `onNavigate`, `onDismiss` |
| `FleetSidebar` | Section nav with per-zone item counts | `sections`, `activeSection`, `onSelect`, `ackCounts` |
| `ZoneGroup` | Collapsible zone container with header | `zone`, `items`, `count`, `defaultCollapsed`, `isExpanded`, `onToggle` |
| `FleetItemRow` | Item row with prevalence chip + variant indicator | `item`, `isDecision`, `onToggle`, `onSelectVariant`, `ackState`, `onAck` |
| `VariantView` | Variant list with radio select + Compare + Confirm | `variants`, `itemId`, `onSelect`, `onCompare`, `ackState`, `onConfirm` |
| `DiffDrawer` | Unified diff display | `diff`, `loading`, `baseHosts`, `targetHosts` |

### State Management

```typescript
// src/api/fleet-client.ts — NEW
export function fetchFleetView(): Promise<FleetViewResponse> {
  return getJson("/api/fleet/view");
}

export function fetchFleetDiff(req: FleetDiffRequest): Promise<FleetDiffResponse> {
  return postJson("/api/fleet/diff", req);
}

// src/hooks/useFleetMutation.ts — NEW
// Wraps applyOp/undo/redo from existing client.ts
// After mutation: ignores ViewResponse, re-fetches fleet view
// Queues mutations sequentially (same pattern as useMutation)

// src/hooks/useFleetDiff.ts — NEW
// Lazy-loads POST /api/fleet/diff on demand
// Caches results by (itemId JSON, base, target) key
// Returns: { diff, loading, error, fetch }

// src/hooks/useVariantAck.ts — NEW
// Per-item variant acknowledgment state
// Backed by localStorage keyed by fleet label + merged_at
// Returns: { isAcked, confirm, reset, unackedCount }
```

## Zone Headers

### Zone Header Component

Each section's items are grouped under collapsible zone dividers.

**Zone header anatomy:**
```
▾ Consensus (84 items)                              [all included]
▸ Near Consensus (12 items)                          [3 need review]
▾ Divergent (3 items — 2 with variants)              [1 need review]
```

**Default collapse state:**
- Consensus: **collapsed** — these are decided, expand to verify
- NearConsensus: **expanded** — judgment calls, needs review
- Divergent: **expanded** — that's where the work is

### Behavioral Rules

- Collapse state is **local React state** — not persisted to server or
  session file. Survives refetch (see Post-Refetch UI State table).
- Empty zones are **hidden entirely**. A section with all items at 100%
  shows no zone headers — just the items directly.
- When a section has items in **only one zone**, zone headers are
  suppressed for that section. No value in a single-group grouping.
- Zone membership is fixed by prevalence — it does not change during a
  session. Include/exclude toggles change attention level but not zone.

### Accessibility

- Zone headers are `<button>` elements with `aria-expanded="true|false"`
- `aria-label` includes context: "Divergent zone, 3 items, 2 with
  variants, 1 needs review"
- Keyboard: Enter/Space toggles collapse
- Focus management: collapsing a zone moves focus to the zone header
  button; expanding does not move focus

## Variant Summary Banner

### Placement and Anatomy

Lives at the top of `FleetApp`, above the active section. Functions as
a **severity signal and navigation aid** — not a data container. Items
live in one canonical place (their section + zone); the banner indexes
them.

The banner tracks **unreviewed actionable variant items** — config
files (in v1) that the user has not yet acknowledged. Non-config
variants (drop-ins, quadlets, compose) are informational only and do
NOT participate in the banner count, ack model, or completion state.
As users confirm config variant decisions, the banner count decrements.

```
⚠ 2 config variant decisions to review
  /etc/nginx/nginx.conf — 3 variants, widest spread: 8/12 hosts
  /etc/sysconfig/network — 2 variants
```

Non-config variants are surfaced separately as a static informational
line below the banner (if any exist), without a review obligation:

```
ℹ 3 additional variant items in read-only sections (services, containers)
```

### Behavioral Rules

- **Vanishes when unreviewed actionable count reaches zero** — "All
  config variant decisions confirmed" brief success message, then hidden.
  The informational line for non-config variants persists independently.
- **Visual weight scales with unreviewed actionable count:**
  - 1-2 unreviewed: subtle info style (PatternFly `Alert` with
    `variant="info"`)
  - 3-5 unreviewed: warning amber (`variant="warning"`)
  - 6+: strong attention (`variant="danger"`)
- **Highest-impact item shown first** — sorted by `max_host_spread`
  descending (biggest host disagreement = most important)
- **Banner items show section membership** — e.g., "[Config]" tag so
  users know which section the click will navigate to
- **Banner click navigation** (portal pattern, not scroll):
  1. `setActiveSection(targetSectionId)` — switches to target section
  2. `useEffect` fires after render → `scrollIntoView({ behavior:
     'smooth', block: 'center' })` on target item
  3. If target item is in a collapsed zone, auto-expand the zone first
  4. Auto-expand the variant view on the target item
  5. CSS highlight animation (outline pulse, ~1.5s) on the target card
- **Dismissible per session** — close button hides the banner. UI
  state only, not persisted across reload.

### Accessibility

- `role="status"` with `aria-live="polite"` — screen readers announce
  unreviewed count on initial load and when count changes
- Item links are `<button>` elements (not `<a>`, since navigation is
  programmatic section-switching, not URL anchoring)
- Each button has `aria-label`: "Navigate to nginx.conf in Config Files,
  3 variants"
- Dismiss button has `aria-label="Dismiss variant summary"`

## Variant View

### When Variants Are Present

When a config item has `variants !== null`, expanding the toggle card
shows the variant view directly. Metadata (attention reason, prevalence)
is shown as a compact header within the variant view — not as a separate
collapsible layer. Three-level nesting: Zone → Card → Variant View.

For items **without variants** (the majority), expanding the card shows
the existing detail view with attention reason and prevalence chip.

Variant selection UI is available only in decision sections (`configs`
in fleet v1). Context section items with variants show the variant
count as a read-only indicator but do not render the variant view.

### Variant List

```
┌───────────────────────────────────────────────────────┐
│  ◉ Variant A                                selected  │
│    web-01, web-02, web-03, +5 more         (8 hosts)  │
├───────────────────────────────────────────────────────┤
│  ○ Variant B                                          │
│    db-01, db-02, db-03                     (3 hosts)  │
├───────────────────────────────────────────────────────┤
│  ○ Variant C                                          │
│    edge-01                                 (1 host)   │
└───────────────────────────────────────────────────────┘

Compare: [ A  ↔  B ▾ ]           [ ✓ Confirm ]
```

**Radio buttons** for variant selection:
- Selecting a different radio fires `POST /api/op` with `SelectVariant`
- The fleet frontend uses `useFleetMutation`: mutation succeeds → re-fetch
  fleet view → UI state preserved per the table above
- Every multi-variant item always has one selected variant (engine
  guarantee — deterministic tie-break). The radio group always has
  exactly one option checked.
- Selecting a different variant auto-confirms the item (transitions
  ack state to "changed")

**Host list:**
- Shows individual hostnames up to 5, then "+N more" with expand
  on click
- Hostnames from `FleetSnapshotMeta.hostnames`

**Compare control:**
- For 2 variants: simple "Compare" button (no dropdown)
- For 3+ variants: dropdown defaulting to selected-vs-next-most-prevalent
  with all pairwise options available

### Diff Display

Clicking Compare lazy-loads the diff via `POST /api/fleet/diff` and
renders a unified diff below the variant list.

```
┌─ Variant A vs Variant B ─────────────────────────────┐
│                                                       │
│  --- Variant A (web-01, web-02, +6)                   │
│  +++ Variant B (db-01, db-02, db-03)                  │
│                                                       │
│  @@ -10,8 +10,8 @@                                   │
│   http {                                              │
│       include       /etc/nginx/mime.types;            │
│  -    worker_connections 1024;                         │
│  +    worker_connections 2048;                         │
│       keepalive_timeout 65;                           │
│                                                       │
└───────────────────────────────────────────────────────┘
```

**Diff rendering:**
- Syntax highlighting via existing `highlight.js` dependency for known
  file types
- Line numbers shown for context
- Removed lines: red background (dark theme: `--pf-t--color--red--30`)
- Added lines: green background (dark theme: `--pf-t--color--green--30`)
- Context lines: default background
- Diff stats shown below: "+2 insertions, -2 deletions"
- Diff results are cached client-side by
  `JSON.stringify(itemId) + base + target` key — re-comparing the same
  pair is instant

### Variant-Capable Item Types

Not all item types support variants. The variant view renders only for
items whose `variants` field is non-null AND whose parent section is a
decision section.

**Fleet v1 variant UI:**

| Item type | Section | Decision section? | Variant UI in v1? |
|-----------|---------|-------------------|-------------------|
| Config files | configs | yes | yes — full radio + compare |
| Systemd drop-ins | services | no (context) | no — count indicator only |
| Quadlet units | containers | no (context) | no — count indicator only |
| Compose files | containers | no (context) | no — count indicator only |

Compose files use a structured carrier — diff is not available for
Compose items even when variant UI ships for containers.

### Accessibility

- Radio group: `role="radiogroup"` with `aria-label="Select variant
  for /etc/nginx/nginx.conf"`
- Each radio includes host count in the accessible label: "Variant A,
  8 hosts, selected"
- Diff view: `<pre>` with `aria-label="Unified diff between Variant A
  and Variant B"` and `role="region"`
- Compare button: `aria-label="Compare selected variant with next most
  prevalent"`
- Confirm button: `aria-label="Confirm variant selection for
  /etc/nginx/nginx.conf"`

## Variant Acknowledgment

### Purpose

The engine always selects a winner for multi-variant items (deterministic
tie-break). The UI needs to distinguish "engine picked this default" from
"the user reviewed this decision" so that the banner can track review
progress and the operator can trust the output.

This is a **review action**, not a **decision action**. The
include/exclude toggle changes what goes in the Containerfile. The
Confirm button says "I've looked at this and I'm satisfied with the
selection."

### Three States

| State | Meaning | Visual treatment |
|-------|---------|------------------|
| Unreviewed | Engine auto-selected, user hasn't looked | Subtle "Review" badge on the item row |
| Confirmed | User clicked Confirm (accepting the default) | Checkmark on item, card settles (muted border) |
| Changed | User selected a different variant | Auto-confirmed, checkmark on item |

### Interaction

- **Confirm button** in the variant view — lightweight, single click.
  Visible only on unreviewed items. Once confirmed, the button is
  replaced by a checkmark indicator.
- **Selecting a different variant** auto-confirms — the act of choosing
  is itself an acknowledgment.
- **Undo** after a `SelectVariant` reverts the selection but does NOT
  revert the ack state (the user has already reviewed the item).
- **Banner count** = actionable config variant items minus
  confirmed/changed items. Non-config variants (drop-ins, quadlets,
  compose) do not participate in ack math. When the actionable
  unreviewed count reaches zero, banner shows brief success and hides.

### Storage

Per-item ack state is **frontend-only**, stored in `localStorage` keyed
by `fleet:{label}:{merged_at}`. This means:

- Ack state survives page reload and browser restart
- Ack state does NOT survive re-scanning/re-aggregating the fleet (new
  `merged_at` = new key = fresh ack state)
- No backend changes needed
- If cross-session durability proves valuable, a `/api/fleet/confirmed`
  endpoint can be added later without changing the frontend contract

### Sidebar Integration

`FleetSidebar` shows ack progress only for decision sections with
actionable variant items (config files in v1):

```
Config Files          2/4 confirmed
```

Context sections (services, containers, etc.) do not show ack progress
— their variants are informational-only and have no review obligation.
Sections without variant items show no ack indicator at all.

## Fleet-of-2 Behavior

When `fleet_context.zones_active === false` (fleet of 2 hosts):

- **No zone headers** — items render in a flat list per section,
  sorted by attention level (same ordering as single-host)
- **Prevalence chips still visible** — "1/2 hosts" or "2/2 hosts" is
  useful context even without zone grouping
- **Variant view fully functional for config items** — config items
  with variants get the same radio select + Compare flow + ack model.
  Non-config variant items show a count indicator only (same as
  fleet 3+ hosts).
- **Summary banner still shows** if actionable config variants exist
- **`/api/fleet/view` response** uses the flat shape: `zones: null`,
  `items: [...]` per section
- **Variant acknowledgment tracks config items only** — fleet-of-2
  commonly produces config variants (two hosts = two potential config
  variants). Ack math follows the same config-only rule as larger
  fleets.

## Keyboard Contract

Fleet mode extends the existing keyboard model from
`src/hooks/useKeyboard.ts`. The full keyboard path across the fleet
component tree:

### Navigation

| Key | Context | Action |
|-----|---------|--------|
| `1`-`9` | anywhere | Switch to section N in sidebar |
| `j` / `k` | section active | Move focus between items in active zone |
| `Tab` | item focused | Move into item's interactive elements |
| `Shift+Tab` | inside item | Return focus to item row |
| `Enter` / `Space` | zone header focused | Toggle zone collapse |
| `Enter` / `Space` | item row focused | Toggle item expand (variant view or detail) |

### Variant Interactions

| Key | Context | Action |
|-----|---------|--------|
| `↑` / `↓` | radio group focused | Move between variant options |
| `Enter` | radio option focused | Select variant (fires SelectVariant op) |
| `c` | variant view open | Trigger Compare (default pair) |
| `Escape` | diff drawer open | Close diff drawer, return focus to Compare button |
| `Escape` | variant view open, no diff | Collapse item, return focus to item row |

### Focus Recovery

After mutations (op/undo/redo → refetch):
- If the previously-focused item still exists in the new view, restore
  focus to it
- If it was removed (e.g., by undo), move focus to the nearest sibling
  item or the zone header
- Track last-focused item ID in a ref; `useEffect` after refetch
  attempts `getElementById(lastFocusedId)?.focus()`

After banner navigation (section switch):
- Focus moves to the target item after scroll completes
- `useEffect` chain: `setActiveSection` → render → scroll → focus

## Testing Strategy

### Unit Tests (vitest)

**Zone rendering:**
- `ZoneGroup` collapse/expand toggle
- `ZoneGroup` hidden when item count is zero
- Zone headers suppressed when section has only one zone
- Default collapse state per zone type

**Fleet item rendering:**
- `FleetItemRow` prevalence chip shows correct count/total
- `FleetItemRow` variant indicator shows count
- `FleetItemRow` without variants renders attention detail on expand
- `FleetItemRow` read-only mode for context sections (no toggle)

**Variant view:**
- `VariantView` renders radio buttons for each variant option
- Radio always has one option checked (engine guarantee)
- Radio selection fires `SelectVariant` op with correct item_id and hash
- Host list truncation at 5 with expand behavior
- Compare dropdown shows correct pairwise options
- Compare button (not dropdown) for 2-variant items
- Confirm button visible on unreviewed items, hidden after ack

**Variant acknowledgment:**
- `useVariantAck` initializes only actionable config variant items as
  unreviewed — non-config variants are excluded from ack tracking
- Clicking Confirm transitions to confirmed state
- Selecting different variant auto-confirms
- Undo does not revert ack state
- localStorage persistence across component unmount/remount
- New fleet (different merged_at) resets ack state
- Non-config variant items (drop-ins, quadlets, compose) never appear
  in ack state or banner math

**Banner:**
- `FleetBanner` hidden when unreviewed actionable count is zero
- Severity scaling: info at 1-2, warning at 3-5, danger at 6+
- Actionable items sorted by max_host_spread descending
- Non-config variants shown as informational line, not in actionable list
- Dismiss hides banner
- Ack decrement updates banner count (actionable only)

**Async recovery:**
- Mutation success + refetch failure shows inline error with Retry
- Diff load failure shows inline error in DiffDrawer with Retry
- Queue clears on any error (no stale ops retained)

**Diff rendering:**
- `DiffDrawer` renders hunks with correct change kinds (equal/delete/insert)
- Syntax highlighting applied for known file types
- Diff stats displayed correctly
- Diff cache: same pair returns cached result without re-fetch

**Navigation:**
- Banner click switches active section
- Post-section-switch: scroll to target, highlight animation
- Auto-expand zone if target is in collapsed zone
- Auto-expand variant view on target item

**Fleet-of-2:**
- Flat rendering (no zone headers) when zones is null
- Prevalence chips still render
- Variant view still functional

### Integration Tests (vitest + testing-library)

- Full fleet view render from mock `/api/fleet/view` response
- SelectVariant flow: radio click → mutation → refetch → verify
  selection updated, UI state preserved (zone collapse, expanded items)
- Banner click → active section switches → target item highlighted
- Undo after SelectVariant → variant reverts, ack state preserved
- Zone collapse persists across refetch
- Fleet-of-2 full render with flat item lists
- Keyboard: j/k navigation between items, Enter to expand, radio
  arrow keys to move between variants

### E2E Tests (Playwright)

- Load fleet tarball → verify zone headers render with correct counts
- Load fleet-of-2 tarball → verify no zone headers, items render flat
- Variant selection flow: expand item → compare → select → verify
  radio state persists after refetch
- Variant ack flow: confirm default → verify banner count decrements,
  checkmark appears
- Banner navigation: click banner item → verify section switches,
  target item highlighted, variant view expanded
- Export: verify exported tarball contains selected variant content
- Undo/redo: select variant → undo → verify original selection restored,
  ack state not reverted
- Accessibility: axe-core audit on fleet view (no critical violations)
- Keyboard: full navigation path (section switch → zone → item →
  variant → compare → diff → escape back out)

### Backend Tests (Rust)

**Handler tests:**
- `/api/fleet/view` returns zone-grouped response for fleet snapshot
- `/api/fleet/view` returns flat response for fleet-of-2 snapshot
- `/api/fleet/view` returns 200 with empty sections for single-host
  (graceful degradation, not 404)
- `/api/fleet/diff` returns diff with correct hunk structure matching
  engine `DiffResult` → DTO mapping
- `/api/fleet/diff` returns 422 for unknown ItemId
- `/api/fleet/diff` returns 422 for unknown ContentHash
- `/api/fleet/diff` returns 422 for binary content
- `/api/fleet/diff` returns 422 for oversized content (>100KB)
- `/api/health` includes fleet context for fleet snapshots
- `/api/health` has `fleet: null` for single-host snapshots
- Fleet health fields match `FleetSnapshotMeta` field names

**Serialization tests:**
- `ItemId` adjacently-tagged encoding round-trip: verify each variant
  produces the expected `{"kind": "...", "key": {"field": "value"}}`
  shape
- `FleetItemResponse` with and without variants
- `FleetDiffResponse` hunk structure matches engine `DiffResult` mapping
- `AttentionLevel` serializes as snake_case (`"needs_review"`)
- `AttentionReason` fleet DTO mapping: verify handler maps engine
  `AttentionReason` variants to snake_case strings, and `Custom(s)`
  passes through as-is
- `PrevalenceZone` serializes as snake_case (`"near_consensus"`)

**Integration:**
- Fleet view → SelectVariant op via `/api/op` → re-fetch fleet view →
  verify variant selection updated in fleet response
- Fleet view → undo → verify selection reverted
- Generation tracking: op increments generation, stale re-fetch
  detects change

## Known Limitations

### No three-way merge
Variant comparison is pairwise only. Users compare A vs. B, A vs. C,
etc. Three-way merge is complex and not justified by the "pick a
winner" use case.

### No syntax highlighting in diffs
Diff lines use `highlight.js` for file-type detection but line-level
highlighting is best-effort. Complex formats (YAML with anchors,
templated configs) may not highlight correctly.

### No diff significance classification
All diff hunks are presented equally. The engine does not classify
whether a diff is "cosmetic" (whitespace, comments) vs. "semantic"
(config values, directives). Future work.

### Compose diff not available
Compose files use a structured carrier that doesn't produce meaningful
line-level diffs. Users can select between Compose variants but cannot
compare them visually. (Compose variant UI is also deferred — context
section in v1.)

### Fixed zone boundaries
Zone thresholds are compiled into the engine (100% / 50% / <50%).
Per-session zone boundary adjustment is deferred. The upgrade path is
well-defined: threshold adjusts the Consensus boundary with dynamic
reclassification.

### Variant ack not persisted server-side
Per-item variant acknowledgment state lives in `localStorage` only. It
survives page reload but not browser data clearing. Cross-session
backend persistence can be added later via a new endpoint if needed.

### Variant UI limited to config files in v1
Drop-ins, quadlets, and compose files can have variants at the engine
level, but the UI only renders variant selection for config files.
Those other types live in context sections (services, containers)
which are read-only in the current frontend. Promoting them to decision
sections is future work.

## Review History

### Round 1

Panel: Fern, Kit, Tang, Thorn. Verdict: request-changes (4/4).

Four MUST-FIX themes addressed in revision 2:

1. **Navigation model ambiguity** — spec mixed single-active-section
   and unified-scroll models. Revision: explicitly chose
   single-active-section, defined banner as portal pattern (section
   switch + scroll-to), added post-refetch UI state preservation table.
2. **Variant state mismatch with engine** — spec assumed
   `selected: null` and `resolved` distinction the engine doesn't
   expose. Revision: aligned with engine truth (always one selected
   variant), replaced `resolved` with frontend-only per-item ack model.
3. **Wire contracts not truthful** — `ItemId` examples wrong, diff
   shapes wrong, hook names wrong, host metadata fields wrong. Revision:
   all examples now show actual serde shapes, diff endpoint changed to
   POST with JSON body, fleet mutation contract defined with new
   `useFleetMutation` hook, `FleetSnapshotMeta` fields used directly.
4. **Section inventory undefined** — spec didn't define which sections
   are decision vs context, or how drop-ins/quadlets/compose work.
   Revision: added Fleet Section Inventory, scoped variant UI to
   config files only in v1, stated `/api/fleet/view` replaces
   `/api/snapshot/sections` for fleet.

Two SHOULD-FIX themes addressed:
- Keyboard/focus contract expanded to full navigation path
- Variant ack model replaces ambiguous `resolved` semantics

### Round 2

Panel: Fern, Kit, Tang, Thorn. Verdict: request-changes (1 approve,
3 request-changes). Tang approved from the Rust/API lane.

One MUST-FIX addressed in revision 3:

1. **Banner/ack/progress scope mismatch** — spec narrowed variant UI
   to configs-only but banner/ack/sidebar still counted non-config
   variants as reviewable work. Revision: `summary.actionable_variant_items`
   lists only config variants. Non-config variants split to
   `informational_variant_count`. Banner counts actionable only.
   Sidebar progress shows only decision sections. Informational
   non-config variants shown as static line, no review obligation.

Four SHOULD-FIX themes addressed:
- Shared shell surfaces and shortcut inheritance explicitly listed
- Export contract adapted for fleet (not claimed as unchanged reuse)
- `attention.reason` clarified as normalized DTO with `Custom()`
  pass-through
- `/api/fleet/diff` error codes aligned to 422 (current web semantics)
- Async failure recovery pinned: mutation-then-refetch failure,
  diff load failure, queue clear behavior

### Round 3

Panel: Fern, Kit, Tang, Thorn. Verdict: request-changes (2 approve,
2 request-changes). Kit and Tang approved.

One MUST-FIX addressed in revision 4:

1. **Config-only scoping inconsistency in downstream sections** —
   Variant Acknowledgment → Interaction, Fleet-of-2 Behavior, and
   test plan lines still used "all variant items" wording for
   ack/progress math. Revision: all three sections now explicitly
   scope ack/banner/progress to actionable config variant items only.
   Non-config variants excluded from ack tracking, banner math, and
   useVariantAck initialization.

Three non-blocking nits addressed:
- Keyboard shortcuts corrected to match current app (`Ctrl+K` for
  global search, `Ctrl+E` for Containerfile, `Ctrl+Shift+E` for
  export)
- Export `fleet/variants/` wording tightened: alternative raw-content
  variants only for configs/drop-ins/quadlets, not selected content,
  not compose
- Backend `AttentionReason` test rewritten to assert fleet DTO
  mapping (handler conversion), not raw enum serialization

## Brainstorm Team

| Name | Role | Contribution |
|------|------|-------------|
| Mark | PM / Product Owner | All design decisions, scope, phasing, ack model direction |
| Fern | UX Specialist | Zone presentation (Option A), three-level nesting, banner as navigation aid, scroll-to guardrails, ack interaction model, keyboard contract gaps |
| Ember | Product Strategist | Prevalence-first axis, variant promotion, banner severity scaling, dedicated fleet surface, ack as product positioning |
| Collins | Image Mode Specialist | Attention-first counterpoint, data shape analysis (fleet ≠ single-host), performance considerations |
| Tang | Rust Systems Engineer | API/engine type alignment, ItemId serialization, SelectVariant routing, attention reasons, variant distribution, wire contract accuracy |
| Kit | Full-Stack Developer | Component reuse plan, summary endpoint consolidation, lazy diff loading, fleet-of-2 flat rendering, mutation contract, section inventory gaps |
| Thorn | Code Quality Engineer | Variant state / engine truth alignment, `resolved` semantics gap, testability review |
