# Playwright Testing Expansion for inspectah Refine

**Status:** Proposed
**Date:** 2026-05-27
**Scope:** Full surface area — single-host refine, fleet, recent features
**Reviewed by:** Thorn (code quality), Tang (Rust contract alignment), Kit (implementation practicality)

## Summary

Expand the existing Playwright e2e test suite from 6 spec files (many tests skipped) to comprehensive coverage of the refine UI. Uses a hybrid fixture strategy: 80% mock API via `page.route()` with canned JSON, 20% real-server smoke tests with checked-in tarballs. Schema validation via `schemars` + `insta` prevents mock-rot.

Single spec, three implementation phases. CI automation, visual regression, and multi-browser testing are deferred to future phases (roadmap items).

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Fixture strategy | Hybrid (mock + real-server) | Mock for speed and determinism, real server for integration confidence. Mock-only misses serialization drift; real-only is too slow. |
| Mock approach | Shared fixture modules | DRY without being clever. One place to update when API changes. `page.route()` built-in, no MSW dependency. |
| Scope | Full surface area | Recent features (containerfile highlights, section promotion, user/group) shipped without e2e coverage — they need it most. Mock approach makes each test cheap. |
| Structure | Single spec, phased plan | Mock fixture infrastructure is shared foundation. Uniform test pattern (mount, inject, interact, assert) keeps one spec coherent. Three phases keep it shippable. |
| Schema validation | `schemars` + `insta` snapshots (primary) + CI fixture validation (backstop) | `insta` catches Rust-side drift in `cargo test`. CI validation catches stale frontend fixtures. |
| Generic PostOutcome | Rejected | Route-specific error shapes (409 StaleGeneration vs 409 NothingToUndo vs 400 ArchiveSafety) don't map cleanly. Fixture files carry `_status` as source of truth. |

---

## 1. Architecture & Fixture Infrastructure

### Test pyramid position

Playwright e2e sits on top of ~30+ vitest unit tests (component rendering) and ~300 Rust unit/integration tests (backend logic). The e2e layer verifies that the full user experience works — interactions, state transitions, visual feedback — not component internals or API correctness.

### Dual-tier fixture strategy

**Mock tier (80%):** `page.route()` intercepts API calls with canned JSON from `e2e/fixtures/`. A helper module (`e2e/helpers/mock-api.ts`) provides two concepts:

- **GET presets** — govern render state. `applyMockApi(page, 'single-host')` wires all GET routes to return a consistent view. Presets: `single-host`, `fleet-3`, `empty`.
- **POST handlers** — govern interaction outcomes. Each mutation route gets explicit response variants per test. `mockPostResponse(page, '/api/op', 'post-responses/op/success.json')`. POST responses are always set per-test, never baked into presets.

**Stateful mock progression** — for sequential workflows (exclude → undo → redo), the helper supports response sequences with explicit trigger binding: `mockSequence(page, '/api/view', [...], { triggerOn: ['/api/op', '/api/undo'] })`. The POST response returns the next state inline (matching real server behavior where `/api/op`, `/api/undo`, `/api/redo` all return the full updated `ViewResponse`), and subsequent GETs serve that same state.

**Real-server tier (20%):** Integration tests in `smoke.spec.ts` (existing) and `smoke-integration.spec.ts` (new) hit the actual Rust refine server with 2 checked-in tarballs (one single-host ~200-500KB, one fleet ~300-500KB). Catch serialization drift, tarball parsing, and full-stack wiring. A CI step diffs real server API responses against the corresponding mock fixtures to catch tarball staleness.

**Error handling coverage:** Three distinct error kinds: `500` (server error), `timeout` (network timeout, default 1000ms to keep suites fast), `malformed` (unparseable response). The UI handles these differently and each needs its own assertions.

### API route inventory

| Route | Method | Response type | Notes |
|---|---|---|---|
| `/api/health` | GET | JSON | HealthResponse |
| `/api/view` | GET | JSON | ViewResponse (includes sections data) |
| `/api/ops` | GET | JSON | AnnotatedOp[] |
| `/api/changes` | GET | JSON | ChangesSummary |
| `/api/user-preview` | GET | JSON | UserPreviewResponse; has `reveal` query param for sensitive redaction |
| `/api/viewed` | GET | JSON | `{ ids: string[] }` |
| `/api/viewed` | POST | JSON (204) | Mark item viewed — dual-method route, same path |
| `/api/fleet/view` | GET | JSON | FleetViewResponse |
| `/api/snapshot/sections` | GET | JSON | ContextSection[] — exists in router, tested in smoke |
| `/api/op` | POST | JSON | Returns updated ViewResponse |
| `/api/undo` | POST | JSON | Returns updated ViewResponse |
| `/api/redo` | POST | JSON | Returns updated ViewResponse |
| `/api/user-strategy` | POST | JSON | Returns updated ViewResponse |
| `/api/user-password` | POST | JSON | Returns updated ViewResponse |
| `/api/tarball` | POST | **Binary** (gzip) | Not JSON — 200+gzip, 409 stale generation, 428 sensitive gating |
| `/api/fleet/diff` | POST | JSON | FleetDiffResponse |

### Schema validation (mock-rot prevention)

Two layers:

1. **`insta` snapshots (primary, fast):** A Rust integration test derives `schemars::JsonSchema` on each API response type, generates JSON Schema, and snapshots it with `insta`. Any Rust struct change fails `cargo test` immediately. Phase 1 covers the ~20 handler/fleet DTO types. Internal refine types (~30 additional) deferred to Phase 2 when fixtures need their schemas.

2. **CI fixture validation (backstop):** The same test writes schema files to `e2e/schemas/`. A CI step validates all `e2e/fixtures/**/*.json` against these schemas. Catches stale frontend fixtures when Rust developers update `insta` snapshots but fixture files aren't updated.

**`schemars` notes (from Tang):**
- `#[serde(skip_serializing_if)]` — handled correctly, fields marked optional in schema.
- `#[serde(flatten)]` on `ViewResponse` — `schemars` inlines flattened properties. Verify output manually once; `insta` catches future drift.
- `serde_json::Value` fields — produce unconstrained `any` schema, correct but not useful for validation. Acceptable.

### Fixture file structure

```
e2e/
  fixtures/
    single-host/
      health.json
      view.json
      ops-empty.json
      changes-empty.json
      viewed-empty.json
      sections.json             # for /api/snapshot/sections
      user-preview.json
      user-preview-redacted.json # reveal=false, sensitive session
    fleet/
      health.json
      fleet-view.json
      sections.json
    post-responses/
      op/
        success.json            # { "_status": 200, "generation": 2, ... }
        stale-generation.json   # { "_status": 409, "error": "stale generation: expected 5, got 3" }
      undo/
        success.json
        nothing-to-undo.json    # { "_status": 409, "error": "nothing to undo" }
      redo/
        success.json
      tarball/
        stub.tar.gz             # tiny real gzip (~100 bytes), hardcoded 200
        sensitive-required.json # { "_status": 428, ... } with sensitivity summary
        stale.json              # { "_status": 409, ... }
      user-strategy/
        success.json
      user-password/
        success.json
        invalid.json            # { "_status": 400, ... }
      viewed/
        success.json            # { "_status": 204 }
      fleet-diff/
        success.json
    sequences/
      exclude-undo-redo/
        01-after-exclude.json
        02-after-undo.json
        03-after-redo.json
    errors/
      server-500.json           # { "_status": 500, "error": "internal server error" }
      malformed.txt             # deliberately invalid JSON
  helpers/
    mock-api.ts
    assertions.ts
  schemas/                      # generated by Rust, validated in CI
    ViewResponse.schema.json
    FleetViewResponse.schema.json
    ...
  *.spec.ts
```

---

## 2. Mock API Helper Design

### Core API

```typescript
// e2e/helpers/mock-api.ts

type Preset = 'single-host' | 'fleet-3' | 'empty';
type RouteOverrides = Record<string, string>; // route path → fixture file path

// GET presets — wire all read routes to consistent render state
// Calls clearMocks() internally before wiring
function applyMockApi(page: Page, preset: Preset, overrides?: RouteOverrides): Promise<void>;

// POST handlers — per-test, never baked into presets
// Reads { "_status": N, ...body } from fixture, strips _status, returns body with that status
// Runtime assertion if fixture is missing _status field
// Binary fixtures (.tar.gz) hardcode status 200, no _status stripping
// Accepts x-acknowledge-sensitive header for tarball sensitive flow
function mockPostResponse(page: Page, route: string, fixturePath: string): Promise<void>;

// Stateful sequences — for multi-step workflows
// triggerOn: which POST route(s) trigger advancement
// POST response returns the NEXT state inline (matching real server behavior)
// Subsequent GETs also return that state
// Counter resets on clearMocks()
function mockSequence(
  page: Page,
  route: string,
  responses: string[],
  opts: { triggerOn: string | string[] }
): Promise<void>;

// Error simulation
type ErrorKind = '500' | 'timeout' | 'malformed';
// timeout defaults to 1000ms to keep test suites fast
function mockError(
  page: Page,
  route: string,
  kind: ErrorKind,
  opts?: { timeoutMs?: number }
): Promise<void>;

// Explicit cleanup — removes all route handlers AND resets sequence counters
// Also called internally by applyMockApi
function clearMocks(page: Page): Promise<void>;
```

### Preset mechanics

`applyMockApi(page, 'single-host')` reads all JSON files from `fixtures/single-host/` and wires `page.route()` for each API path. Mapping is defined in a lookup table inside `mock-api.ts` (not filename convention), e.g. `{ 'view.json': '/api/view', 'fleet-view.json': '/api/fleet/view', 'sections.json': '/api/snapshot/sections' }`. This avoids ambiguity for routes with path segments. Dual-method `/api/viewed`: preset wires the GET, `mockPostResponse` wires the POST.

Override individual GET responses:
```typescript
await applyMockApi(page, 'single-host', {
  '/api/view': 'fixtures/overrides/view-empty-packages.json',
});
```

### Sequence mechanics

```typescript
await applyMockApi(page, 'single-host');
await mockSequence(page, '/api/view', [
  'fixtures/sequences/exclude-undo-redo/01-after-exclude.json',
  'fixtures/sequences/exclude-undo-redo/02-after-undo.json',
  'fixtures/sequences/exclude-undo-redo/03-after-redo.json',
], { triggerOn: ['/api/op', '/api/undo', '/api/redo'] });

// Initial state: preset's view.json
// POST to /api/op → POST response body = 01-after-exclude, next GET /api/view = same
// POST to /api/undo → POST response body = 02-after-undo, next GET /api/view = same
// POST to /api/redo → POST response body = 03-after-redo, next GET /api/view = same
```

### Shared assertions

```typescript
// e2e/helpers/assertions.ts

function expectStatsBar(page: Page, expected: { packages?: string; configs?: string }): Promise<void>;
function expectSidebarSection(page: Page, name: string, visible?: boolean): Promise<void>;
function expectDecisionItem(page: Page, testId: string, included: boolean): Promise<void>;
function expectContainerfileContains(page: Page, text: string): Promise<void>;
function expectNoAxeViolations(page: Page, tags?: string[]): Promise<void>;
```

---

## 3. Test Coverage Map

### Spec file organization

| Spec file | Area | Tier | Phase |
|---|---|---|---|
| `smoke.spec.ts` (existing) | Page load, health, sidebar/statsbar, `/api/snapshot/sections` 200 check | Real server | 1 |
| `smoke-integration.spec.ts` (new) | Real-server golden path: health → view → toggle → containerfile preview → undo → tarball export | Real server | 1 |
| `triage.spec.ts` (existing, unskip) | Package include/exclude, config toggle, undo/redo, containerfile preview, export/download, viewed mark/persist, sensitive tarball gating (428) | Mock | 2 |
| `keyboard.spec.ts` (existing, unskip) | j/k navigation, ? overlay, Escape, section switching, search focus | Mock | 2 |
| `a11y.spec.ts` (existing, expand) | axe-core scans on single-host + fleet views, ARIA roles, focus management | Mock | 2 |
| `responsive.spec.ts` (existing, expand) | Hamburger menu, sidebar overlay, resize transitions | Mock | 2 |
| `sections.spec.ts` (new) | Context section rendering (services, containers, users/groups, network, storage, scheduled tasks, non-RPM, kernel/boot, SELinux), section navigation, section search | Mock | 2 (late) |
| `containerfile.spec.ts` (new) | Containerfile panel open/close, change highlights, auto-scroll, diff indicators, reduced-motion | Mock | 3 |
| `fleet.spec.ts` (existing, unskip) | Zone rendering, variant ack, diff drawer, fleet undo/redo, fleet banner | Mock | 3 |
| `repos.spec.ts` (new) | Repo groups, repo bar, repo exclude/include, repo conflict popover, attention summary | Mock | 3 |
| `users.spec.ts` (new) | User/group materialization: strategy picker, password entry, user preview (redacted + revealed), preview panel | Mock | 3 |

### Explicitly tested contract edges

These were identified by Thorn and Tang as easy-to-miss contract behaviors:

- **Sensitive tarball gating:** `/api/tarball` returns 428 when `session_is_sensitive` and `x-acknowledge-sensitive` header is missing. Tested in `triage.spec.ts`.
- **User preview redaction:** `/api/user-preview` redacts kickstart/blueprint content when sensitive and `reveal` query param is absent. Tested in `users.spec.ts` with fixture pair.
- **Viewed persistence:** `/api/viewed` dual-method (GET reads, POST marks). Mark-as-viewed → reload → verify persistence flow in `triage.spec.ts`.
- **Stale generation conflict:** `/api/op` returns 409 with `expected`/`actual` fields. Tested in `triage.spec.ts`.
- **`/api/ops` and `/api/changes`:** GET-only routes — covered in GET preset fixtures, asserted in `triage.spec.ts` (operation history display, changes summary panel).

### Phase breakdown

**Phase 1 — Foundation (~2 days):**
- `e2e/helpers/mock-api.ts` with `applyMockApi`, `mockPostResponse`, `mockSequence`, `mockError`, `clearMocks`
- `e2e/helpers/assertions.ts` with shared assertion helpers
- Canned JSON fixtures: `single-host/` preset, `fleet/` preset, `post-responses/` with per-route variants
- Binary tarball stub (`post-responses/tarball/stub.tar.gz`)
- Sequence fixtures for exclude-undo-redo flow
- Error fixtures (500, malformed)
- Schema validation: add `schemars` to workspace deps, derive `JsonSchema` on ~20 handler/fleet DTO types, `insta` snapshot test, CI validation script
- `smoke-integration.spec.ts`: health → view (verify generation, sections, `session_is_sensitive`) → toggle package (verify `can_undo: true`) → containerfile preview updated → undo (verify `can_undo: false`, `can_redo: true`) → tarball export (verify gzip response)
- Check in 2 curated tarballs to `testdata/` (one single-host, one fleet)

**Phase 2 — Single-host core (~2-3 days):**
- Unskip and rewrite `triage.spec.ts` to use mock fixtures: package toggle, config toggle, undo/redo sequences, containerfile preview, export/download, viewed mark/persist, sensitive tarball gating, `/api/ops` history, `/api/changes` summary
- Unskip and rewrite `keyboard.spec.ts` to use mock fixtures
- Expand `a11y.spec.ts` with mock-backed comprehensive axe scans (single-host + fleet presets)
- Expand `responsive.spec.ts` with mock-backed resize tests
- New `sections.spec.ts` (late Phase 2): all 9 context sections rendering, section navigation, section search — prerequisite for `repos.spec.ts`

**Phase 3 — Recent features + fleet gaps (~2-3 days):**
- Unskip and rewrite `fleet.spec.ts` to use mock fixtures
- New `containerfile.spec.ts`: change highlights, auto-scroll, diff indicators, reduced-motion
- New `repos.spec.ts`: repo groups, repo bar, repo exclude/include, repo conflict popover, attention summary (depends on `sections.spec.ts` from Phase 2)
- New `users.spec.ts`: strategy picker, password entry, user preview with redaction pair

### What's out of scope (future roadmap items)

These will be added to `docs/ROADMAP.md` under Upcoming Work:

- **CI-runnable test suite** — `webServer` config in `playwright.config.ts` to auto-start the refine server, tarball fixture management in CI, GitHub Actions integration. Makes the full Playwright suite runnable without manual server startup.
- **Visual regression** — Playwright screenshot comparison for key views (single-host refine, fleet zones, containerfile panel, responsive breakpoints). Catches CSS regressions and theme rendering bugs that functional tests miss.
- **Multi-browser** — Add Firefox project to `playwright.config.ts`. Firefox's Gecko engine handles CSS grid/flexbox and keyboard events differently from Chromium, especially relevant for PatternFly 6.
- **Not planned:** Performance/load testing (not meaningful for a local single-user tool). Mobile Safari viewport testing (no mobile target).

---

## 4. Rust-Side Requirements

### `schemars` integration

Add `schemars = { version = "0.8", features = ["derive"] }` to workspace dependencies. Derive `JsonSchema` alongside `Serialize` on all handler and fleet DTO types (~20 types in Phase 1):

```rust
#[derive(Serialize, Clone, Debug, schemars::JsonSchema)]
pub struct ViewResponse { ... }
```

**Phase 1 scope:** Handler DTOs in `inspectah-web/src/handlers.rs` and `inspectah-web/src/fleet_handlers.rs`. Internal refine types in `inspectah-refine/src/types.rs` (~30 types) deferred to Phase 2.

### Schema export test

```rust
// inspectah-web/tests/schema_export_test.rs

use schemars::schema_for;

#[test]
fn export_api_schemas() {
    let schemas: Vec<(&str, schemars::schema::RootSchema)> = vec![
        ("ViewResponse", schema_for!(ViewResponse)),
        ("FleetViewResponse", schema_for!(FleetViewResponse)),
        ("FleetDiffResponse", schema_for!(FleetDiffResponse)),
        // ... other response types
    ];

    for (name, schema) in &schemas {
        let json = serde_json::to_string_pretty(schema).unwrap();
        // insta snapshot — catches drift in cargo test
        insta::assert_snapshot!(format!("schema_{name}"), json);
        // Write to disk for CI fixture validation
        let path = format!("ui/e2e/schemas/{name}.schema.json");
        std::fs::write(&path, &json).unwrap();
    }
}
```

### Curated test tarballs

Two tarballs checked into `testdata/`:

- `testdata/single-host-e2e.tar.gz` (~200-500KB): One RHEL host scan with packages across multiple repos, config files, services, users/groups, containers. Must include at least one sensitive field to exercise tarball gating.
- `testdata/fleet-e2e.tar.gz` (~300-500KB): 3-host fleet merge with items across consensus/near-consensus/divergent zones, actionable variant items, config file variants.

A CI step runs the real server against these tarballs and diffs API responses against mock fixtures to catch tarball staleness.
