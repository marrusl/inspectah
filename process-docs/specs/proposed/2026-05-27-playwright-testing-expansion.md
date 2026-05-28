# Playwright Testing Expansion for inspectah Refine

**Status:** Proposed (round 3)
**Date:** 2026-05-27
**Scope:** Full surface area — single-host refine, fleet, recent features
**Reviewed by:** Thorn (code quality), Tang (Rust contract alignment), Kit (implementation practicality)
**Round 1 verdict:** Request changes — fixture taxonomy / mock state model / phase scoping
**Round 2 verdict:** Request changes — Thorn approves, Kit approves, Tang: schema scope + users/groups `Value` typing

## Summary

Expand the existing Playwright e2e test suite from 6 spec files (many tests skipped) to comprehensive coverage of the refine UI. Uses a hybrid fixture strategy: 80% mock API via `page.route()` with canned JSON, 20% real-server smoke tests with checked-in tarballs. Schema validation via `schemars` + `insta` prevents mock-rot.

Single design spec, four implementation sub-phases. CI automation, visual regression, and multi-browser testing are deferred to future phases (roadmap items).

**Terminology note:** "Single spec" refers to this design document — not a single `.spec.ts` file. The implementation produces 11 spec files organized by functional area.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Fixture strategy | Hybrid (mock + real-server) | Mock for speed and determinism, real server for integration confidence. Mock-only misses serialization drift; real-only is too slow. |
| Mock approach | Shared fixture modules | DRY without being clever. One place to update when API changes. `page.route()` built-in, no MSW dependency. |
| Scope | Full surface area | Recent features (containerfile highlights, section promotion, user/group) shipped without e2e coverage — they need it most. Mock approach makes each test cheap. |
| Structure | Single design spec, four sub-phases | Mock fixture infrastructure is shared foundation. Uniform test pattern (mount, inject, interact, assert) keeps one design spec coherent. Four sub-phases keep it shippable with honest checkpoints. |
| Schema validation | `schemars` + `insta` snapshots (primary) + CI fixture validation (backstop) | `insta` catches Rust-side drift in `cargo test`. CI validation catches stale frontend fixtures. |
| Generic PostOutcome | Rejected | Error payloads are mostly generic `{ "error": string }` envelopes. Fixture files carry `_status` as transport metadata alongside the response body. |
| Mock state model | Narrow for mock tier, widen via real-server | Mock tier tests UI rendering given static/sequenced state. Cross-endpoint state coherence (ops/changes/viewed persistence) tested in real-server tier. |
| DTO drift scope | Playwright fixtures only | Existing TypeScript DTO drift in `api/types.ts` (ChangesSummary, FleetTriageDto, RepoTier) is a separate concern. This spec solves Playwright fixture drift. |

---

## 1. Architecture & Fixture Infrastructure

### Test pyramid position

Playwright e2e sits on top of ~30+ vitest unit tests (component rendering) and ~300 Rust unit/integration tests (backend logic). The e2e layer verifies that the full user experience works — interactions, state transitions, visual feedback — not component internals or API correctness.

### Dual-tier fixture strategy

**Mock tier (80%):** `page.route()` intercepts API calls with canned JSON from `e2e/fixtures/`. A helper module (`e2e/helpers/mock-api.ts`) provides two concepts:

- **GET presets** — govern render state. `applyMockApi(page, 'single-host')` wires all GET routes to return a consistent view. Presets: `single-host`, `fleet-3`, `empty`.
- **POST handlers** — govern interaction outcomes. Each mutation route gets explicit response variants per test. `mockPostResponse(page, '/api/op', 'post-responses/op/success.json')`. POST responses are always set per-test, never baked into presets.

**Mock tier scope and limits:** The mock tier tests UI rendering and interaction given static or sequenced state. It does NOT test cross-endpoint state coherence — e.g., whether `/api/ops` reflects the operation that `/api/op` just applied, or whether `/api/viewed` persists across page reload. Those behaviors require real server state and are covered by the real-server tier.

**Stateful mock progression** — for sequential workflows (exclude → undo → redo), the helper supports response sequences with explicit trigger binding. The sequence model accounts for two distinct UI mutation patterns:

- **Single-host:** UI re-fetches `GET /api/view` after a POST mutation (the POST response body is not used as the primary state carrier). `mockSequence` advances the GET response when a trigger POST is intercepted.
- **Fleet:** UI always re-fetches `GET /api/fleet/view` after POST mutations (POST response ignored entirely). Same mechanism — sequence on the GET route, triggered by POST.

**Real-server tier (20%):** Integration tests in `smoke.spec.ts` (existing) and `smoke-integration.spec.ts` (new) hit the actual Rust refine server with 2 checked-in tarballs. These tests are **manual-only** until the CI automation phase lands — they are NOT part of the default `npx playwright test` run path. See "Real-server test run path" below.

**Error handling coverage:** Three distinct error kinds: `500` (server error), `timeout` (network timeout, default 1000ms to keep suites fast), `malformed` (unparseable response). The UI handles these differently and each needs its own assertions.

### API route inventory

| Route | Method | Response type | Success | Error shapes | Notes |
|---|---|---|---|---|---|
| `/api/health` | GET | JSON | HealthResponse | — | hostname, completeness, fleet flag |
| `/api/view` | GET | JSON | ViewResponse | — | Includes sections data, `session_is_sensitive` |
| `/api/ops` | GET | JSON | AnnotatedOp[] | — | Operation history |
| `/api/changes` | GET | JSON | ChangesSummary | — | Pending changes summary |
| `/api/user-preview` | GET | JSON | UserPreviewResponse | — | `reveal` query param for sensitive redaction |
| `/api/viewed` | GET | JSON | `{ ids: string[] }` | — | Dual-method route |
| `/api/viewed` | POST | 204 | — | — | Mark item viewed |
| `/api/fleet/view` | GET | JSON | FleetViewResponse | — | Fleet-mode only |
| `/api/snapshot/sections` | GET | JSON | ContextSection[] | — | Exists in router, separate from `/api/view` |
| `/api/op` | POST | JSON | ViewResponse | `{ "error": string }` generic envelope | Returns updated view |
| `/api/undo` | POST | JSON | ViewResponse | 409 `{ "error": "nothing to undo" }` | Returns updated view |
| `/api/redo` | POST | JSON | ViewResponse | 409 `{ "error": "nothing to redo" }` | Returns updated view |
| `/api/user-strategy` | POST | JSON | ViewResponse | — | Returns updated view |
| `/api/user-password` | POST | JSON | ViewResponse | 400 `{ "error": string }` | Preserve validation |
| `/api/tarball` | POST | **Binary** (gzip) | 200 + gzip | 409 stale gen `{ "error": string }`, 428 sensitive `{ summary }` | Only route with structured (non-envelope) error DTO at 428 |
| `/api/fleet/diff` | POST | JSON | FleetDiffResponse | — | — |

**Error contract note:** All POST error responses use a generic `{ "error": string }` envelope, except `/api/tarball` at 428 which returns a structured sensitivity summary DTO. Schema validation applies to success DTOs; error envelopes are validated against the envelope shape only.

### Fixture taxonomy

Fixtures are classified into four categories with different validation rules:

| Category | Location | Schema validation rule | Examples |
|---|---|---|---|
| **Body fixtures** | `fixtures/single-host/`, `fixtures/fleet/` | Validate against Rust DTO schema for the corresponding endpoint | `view.json` → ViewResponse schema |
| **Harness wrappers** | `fixtures/post-responses/**/*.json` | Strip `_status` field, then validate body against DTO schema | `op/success.json` → ViewResponse schema |
| **Error envelopes** | `fixtures/post-responses/**/error-*.json`, `fixtures/errors/` | Validate against generic `{ "error": string }` envelope schema only. Exception: `tarball/sensitive-required.json` validates against tarball sensitivity DTO schema. | `undo/nothing-to-undo.json`, `server-500.json` |
| **Excluded from validation** | Binary stubs, malformed fixtures | Not validated — explicitly excluded by file extension or directory | `tarball/stub.tar.gz`, `errors/malformed.txt` |

**Sequence fixtures** (`fixtures/sequences/`) are body fixtures — they represent ViewResponse snapshots at different points in a workflow and validate against the ViewResponse schema.

The CI validation script uses these categories to route each fixture to the right validation rule. The routing logic lives in a manifest file (`e2e/fixtures/manifest.json`) that maps each fixture path to its category and target schema, rather than relying on directory convention alone.

### Fixture-structure validation (mock-rot prevention)

**Approach:** A Rust integration test (`inspectah-web/tests/fixture_structure_test.rs`) parses each e2e fixture JSON file as `serde_json::Value` and snapshots it with `insta`. Any fixture change (field added, removed, renamed, retyped) breaks the snapshot and requires explicit `cargo insta review` acceptance. This catches unreviewed fixture drift.

**What this validates and what it doesn't:**
- **Validates:** Fixture JSON is structurally intentional — every change is explicitly reviewed via insta snapshot acceptance. Catches accidental edits, stale fixtures from copy-paste, and forgotten updates.
- **Does not validate:** Fixtures against Rust response types. The response DTOs (`ViewResponse`, `FleetViewResponse`, etc.) derive `Serialize` only — adding `Deserialize` would require transitive derives across ~50 types in 3 crates. Rust-type compatibility is proven by the real-server smoke tests (Phase 1d), where the actual server serializes responses and Playwright asserts against them.

**No `schemars` dependency.** The earlier plan to derive `JsonSchema` across the type graph was dropped during plan review (rounds 3-5). The fixture-structure snapshot approach provides equivalent drift detection with zero cross-crate changes. Only `insta` (already a workspace dev-dependency) is needed.

**`manifest.json`** documents fixture categories (body, harness wrapper, error envelope, excluded) for future CI validation tooling. Phase 1c does not implement manifest-driven validation — it provides the insta snapshot safety net only.

**Users/groups limitation:** The `users_groups_decisions` field on `ViewResponse` is typed as `Vec<serde_json::Value>`. The `users.spec.ts` tests in Phase 3 are **not schema-backed** — they rely on structural fixture correctness only. Typing this field as a proper DTO is tracked as a separate backlog item.

### Fixture file structure

```
e2e/
  fixtures/
    manifest.json               # maps fixture paths → { category, schema }
    single-host/                 # body fixtures — validate against DTO schemas
      health.json
      view.json
      ops-empty.json
      changes-empty.json
      viewed-empty.json
      sections.json
      user-preview.json
      user-preview-redacted.json
    fleet/                       # body fixtures
      health.json
      fleet-view.json
      sections.json
    post-responses/              # harness wrappers — strip _status, then validate
      op/
        success.json             # { "_status": 200, "generation": 2, ... }
      undo/
        success.json
        nothing-to-undo.json     # { "_status": 409, "error": "nothing to undo" } — error envelope
      redo/
        success.json
      tarball/
        stub.tar.gz              # excluded from validation — binary
        sensitive-required.json  # { "_status": 428, ... } — structured error DTO (exception)
        stale.json               # { "_status": 409, "error": "..." } — error envelope
      user-strategy/
        success.json
      user-password/
        success.json
        invalid.json             # { "_status": 400, "error": "..." } — error envelope
      viewed/
        success.json             # { "_status": 204 }
      fleet-diff/
        success.json
    sequences/                   # body fixtures — validate against ViewResponse schema
      exclude-undo-redo/
        01-after-exclude.json
        02-after-undo.json
        03-after-redo.json
    errors/                      # error simulation — envelope or excluded
      server-500.json            # { "_status": 500, "error": "internal server error" }
      malformed.txt              # excluded from validation
  helpers/
    mock-api.ts
    assertions.ts
  schemas/                       # generated by Rust, validated in CI
    ViewResponse.schema.json
    FleetViewResponse.schema.json
    ErrorEnvelope.schema.json    # { "error": string } — generic envelope
    TarballSensitivity.schema.json
    ...
  *.spec.ts
```

### Real-server test run path

Real-server tests (`smoke.spec.ts`, `smoke-integration.spec.ts`) require a running `inspectah refine` server. Until the CI automation phase lands, these are **manual developer tests**:

```bash
# Start server with single-host fixture
cargo run -p inspectah-cli -- refine testdata/single-host-e2e.tar.gz --no-browser --port 8642 &

# Run real-server tests only
cd inspectah-web/ui && npx playwright test e2e/smoke.spec.ts e2e/smoke-integration.spec.ts

# Kill server
kill %1
```

**Default run path** (no server needed — mock tier only):
```bash
cd inspectah-web/ui && npx playwright test --grep-invert smoke-integration
```

The `smoke-integration.spec.ts` file uses a `test.beforeEach` guard that skips all tests if the server isn't running, so `npx playwright test` (all specs) is safe to run without a server — integration tests skip gracefully.

**Acceptance gate for CI automation phase:** `webServer` config in `playwright.config.ts` auto-starts the refine server with a checked-in tarball. At that point, `npx playwright test` runs everything including real-server tests.

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
// Runtime assertion if JSON fixture is missing _status field
// Binary fixtures (.tar.gz) hardcode status 200, no _status stripping
// Accepts x-acknowledge-sensitive header for tarball sensitive flow
function mockPostResponse(page: Page, route: string, fixturePath: string): Promise<void>;

// Stateful sequences — for multi-step workflows
// triggerOn: which POST route(s) trigger advancement
// On trigger: advances the GET response for the sequenced route
// For single-host: UI re-fetches GET /api/view after POST (POST body not primary)
// For fleet: UI re-fetches GET /api/fleet/view after POST (POST body ignored)
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
// POST to /api/op triggers → next GET /api/view returns 01-after-exclude
// POST to /api/undo triggers → next GET /api/view returns 02-after-undo
// POST to /api/redo triggers → next GET /api/view returns 03-after-redo
```

Fleet equivalent:
```typescript
await applyMockApi(page, 'fleet-3');
await mockSequence(page, '/api/fleet/view', [
  'fixtures/sequences/fleet-toggle/01-after-toggle.json',
], { triggerOn: ['/api/op'] });
```

### Mock tier coverage boundary

The mock tier proves:
- UI renders correctly given a specific API state (GET preset)
- UI sends the right mutation when the user interacts (POST intercept)
- UI updates correctly when the API state changes (sequence progression)
- UI handles errors gracefully (error simulation)

The mock tier does NOT prove:
- Cross-endpoint state coherence (ops reflects last op, changes reflects pending mutations)
- Viewed persistence across page reload
- Tarball content correctness
- Real serialization/deserialization round-trip

Those are covered by the real-server tier.

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
| `smoke.spec.ts` (existing) | Page load, health, sidebar/statsbar, `/api/snapshot/sections` 200 check | Real server | 1d |
| `smoke-integration.spec.ts` (new) | Real-server golden path (see below) | Real server | 1d |
| `triage.spec.ts` (existing, unskip) | Package include/exclude, config toggle, undo/redo, containerfile preview, export/download, sensitive tarball gating (428) | Mock | 2 |
| `keyboard.spec.ts` (existing, unskip) | j/k navigation, ? overlay, Escape, section switching, search focus | Mock | 2 |
| `a11y.spec.ts` (existing, expand) | axe-core scans on single-host + fleet views, ARIA roles, focus management | Mock | 2 |
| `responsive.spec.ts` (existing, expand) | Hamburger menu, sidebar overlay, resize transitions | Mock | 2 |
| `sections.spec.ts` (new) | Context section rendering (all 9 sections), section navigation, section search | Mock | 2 (late) |
| `containerfile.spec.ts` (new) | Containerfile panel open/close, change highlights, auto-scroll, diff indicators, reduced-motion | Mock | 3 |
| `fleet.spec.ts` (existing, unskip) | Zone rendering, variant ack, diff drawer, fleet undo/redo, fleet banner | Mock | 3 |
| `repos.spec.ts` (new) | Repo groups, repo bar, repo exclude/include, repo conflict popover, attention summary | Mock | 3 |
| `users.spec.ts` (new) | User/group materialization: strategy picker, password entry, user preview (redacted + revealed) | Mock | 3 |

### Real-server golden path (`smoke-integration.spec.ts`)

Minimum viable integration sequence (5 requests):

1. `GET /api/health` — server alive, hostname and completeness populated
2. `GET /api/view` — generation 1, sections non-empty, `session_is_sensitive` present
3. `POST /api/op` (package exclude) — returns generation 2, `can_undo: true`
4. `POST /api/undo` — generation 3, `can_undo: false`, `can_redo: true`
5. `POST /api/tarball` with correct generation — 200 with `content-type: application/gzip`, body length > 0

Containerfile preview verification between steps 3 and 4: after the package exclude, verify the containerfile panel content changed. This catches the entire render pipeline (projection → Containerfile generation → tar assembly).

Redo is skipped — it's the mirror of undo and the mock tests cover it.

### Coverage by tier

| Behavior | Mock tier | Real-server tier |
|---|---|---|
| UI renders given API state | Yes | — |
| User interaction sends correct mutation | Yes | — |
| UI updates after state change | Yes (sequence) | Yes (live) |
| Error handling (500, timeout, malformed) | Yes | — |
| Sensitive tarball gating (428) | Yes (fixture) | Yes (if tarball has sensitive data) |
| Cross-endpoint state coherence | — | Yes |
| Viewed persistence across reload | — | Yes |
| Serialization round-trip | — | Yes |
| Tarball content correctness | — | Yes |
| Containerfile preview after mutation | Yes (sequence) | Yes |

### Phase breakdown

**Phase 1a — Mock infrastructure proof-of-concept (~1-2 days):**
- `e2e/helpers/mock-api.ts` with `applyMockApi`, `mockPostResponse`, `clearMocks`
- `e2e/helpers/assertions.ts` with shared assertion helpers
- Canned JSON fixtures: `single-host/` preset (body fixtures only)
- Rewrite one existing spec (`keyboard.spec.ts`) to use mock fixtures as proof-of-concept
- Verify the pattern: mount with preset → interact → assert

**Phase 1b — Remaining fixture infrastructure (~1-2 days):**
- `mockSequence` and `mockError` support in `mock-api.ts`
- `fleet/` preset fixtures
- `post-responses/` with per-route success and error variants
- Binary tarball stub
- Sequence fixtures for exclude-undo-redo flow
- Error fixtures (500, malformed)
- `e2e/fixtures/manifest.json` mapping fixtures to categories and schemas

**Phase 1c — Fixture-structure validation (~1 day):**
- Verify `insta` is a dev-dependency of `inspectah-web`
- `inspectah-web/tests/fixture_structure_test.rs` — parses each fixture as `serde_json::Value`, snapshots with `insta`
- Run with `INSPECTAH_SKIP_UI=1 cargo test -p inspectah-web --test fixture_structure_test`
- No `schemars`, no cross-crate derives, no CI validation script — insta snapshots are the safety net
- **Note:** `users_groups_decisions: Vec<serde_json::Value>` means users/groups fixtures are structural-only until typed

**Phase 1d — Real-server smoke tests (~1 day):**
- Curate and check in 2 tarballs to `testdata/`
- `smoke-integration.spec.ts` with 5-request golden path
- `beforeEach` guard that skips when server isn't running
- Document the manual run path in spec file comments

**Phase 2 — Single-host core (~2-3 days):**
- Unskip and rewrite `triage.spec.ts` to use mock fixtures: package toggle, config toggle, undo/redo sequences, containerfile preview, export/download, sensitive tarball gating
- Unskip and rewrite `keyboard.spec.ts` with full coverage (Phase 1a rewrites the basics)
- Expand `a11y.spec.ts` with mock-backed comprehensive axe scans (single-host + fleet presets)
- Expand `responsive.spec.ts` with mock-backed resize tests
- New `sections.spec.ts` (late Phase 2): all 9 context sections rendering, section navigation, section search — prerequisite for `repos.spec.ts`

**Phase 3 — Recent features + fleet gaps (~2-3 days):**
- Unskip and rewrite `fleet.spec.ts` to use mock fixtures (sequence on `/api/fleet/view`)
- New `containerfile.spec.ts`: change highlights, auto-scroll, diff indicators, reduced-motion
- New `repos.spec.ts`: repo groups, repo bar, repo exclude/include, repo conflict popover, attention summary
- New `users.spec.ts`: strategy picker, password entry, user preview with redaction pair

### Negative-path matrix

User-visible error branches and the spec file that owns each:

| Error condition | Status | Spec file | Phase |
|---|---|---|---|
| Server unreachable (initial load) | — | `smoke.spec.ts` (existing — EmptyState "Failed to load") | 1d |
| Server error on mutation | 500 | `triage.spec.ts` | 2 |
| Network timeout on mutation | — | `triage.spec.ts` | 2 |
| Malformed response | — | `triage.spec.ts` | 2 |
| Nothing to undo | 409 | `triage.spec.ts` | 2 |
| Nothing to redo | 409 | `triage.spec.ts` | 2 |
| Stale generation on tarball export | 409 | `triage.spec.ts` | 2 |
| Sensitive tarball gating | 428 | `triage.spec.ts` | 2 |
| Invalid user password | 400 | `users.spec.ts` | 3 |

### Route-to-surface source-of-truth table

Maps each API route to the UI surface(s) that consume it:

| Route | UI consumer | Data displayed |
|---|---|---|
| `/api/health` | StatsBar hostname, fleet detection | Hostname label, fleet/single-host mode switch |
| `/api/view` | DecisionList, StatsBar counts, ContainerfilePanel, ExportDialog, sidebar badges | Package/config items, include/exclude state, generation, containerfile preview, undo/redo availability |
| `/api/ops` | (not directly rendered in current UI) | Operation history — consumed by real-server coherence tests only |
| `/api/changes` | (not directly rendered in current UI) | Changes summary — consumed by real-server coherence tests only |
| `/api/user-preview` | UserPreviewPanel | Kickstart/blueprint preview, redaction when sensitive |
| `/api/viewed` (GET) | Sidebar badge counts, triage progress | Which items the user has already reviewed |
| `/api/viewed` (POST) | (side effect) | Marks an item as viewed |
| `/api/fleet/view` | FleetSection, ZoneGroup, FleetBanner, FleetItemRow, FleetSidebar | Fleet items by zone, variant counts, ack progress, consensus data |
| `/api/snapshot/sections` | Sidebar context section list, MainContent section rendering | Section names, item counts, context items |
| `/api/op` | (triggers view refresh) | Mutation — include/exclude/config/user decisions |
| `/api/undo` / `/api/redo` | (triggers view refresh) | State rollback/replay |
| `/api/user-strategy` | (triggers view refresh) | Set user skip/useradd strategy |
| `/api/user-password` | (triggers view refresh) | Set user password |
| `/api/tarball` | ExportDialog download | Binary tarball download |
| `/api/fleet/diff` | DiffDrawer | Per-host config diff for fleet variant comparison |

### Fixture refresh workflow

When a Rust API response type changes:

1. `cargo test` fails — `insta` snapshot for the affected schema no longer matches
2. Developer runs `cargo insta review` to accept the new schema snapshot
3. `insta` acceptance writes updated schema to `e2e/schemas/`
4. CI validation step fails — existing fixtures don't match the new schema
5. Developer updates affected fixtures in `e2e/fixtures/` to match the new response shape
6. Developer updates `e2e/fixtures/manifest.json` if new fixtures were added or categories changed
7. `npx playwright test` confirms the mock tests still pass with updated fixtures

When adding a new API route:
1. Add fixture files to the appropriate preset directory and `post-responses/`
2. Add entries to `manifest.json` with category and target schema
3. Add the route to the lookup table in `mock-api.ts`
4. Write tests in the appropriate spec file

### What's out of scope (future roadmap items)

These will be added to `docs/ROADMAP.md` under Upcoming Work:

- **CI-runnable test suite** — `webServer` config in `playwright.config.ts` to auto-start the refine server, tarball fixture management in CI, GitHub Actions integration. Makes `npx playwright test` run everything including real-server tests.
- **Visual regression** — Playwright screenshot comparison for key views (single-host refine, fleet zones, containerfile panel, responsive breakpoints). Catches CSS regressions and theme rendering bugs that functional tests miss.
- **Multi-browser** — Add Firefox project to `playwright.config.ts`. Firefox's Gecko engine handles CSS grid/flexbox and keyboard events differently from Chromium, especially relevant for PatternFly 6.
- **Not planned:** Performance/load testing (not meaningful for a local single-user tool). Mobile Safari viewport testing (no mobile target).

---

## 4. Rust-Side Requirements

### Fixture-structure test

Verify `insta` is a dev-dependency of `inspectah-web` (it is already a workspace dependency). No other Cargo.toml changes needed. No `schemars` dependency.

The test at `inspectah-web/tests/fixture_structure_test.rs` parses each fixture as `serde_json::Value` and snapshots it. Uses `CARGO_MANIFEST_DIR` for reliable path resolution. Run with `INSPECTAH_SKIP_UI=1` to skip the UI build in `build.rs`.

Snapshots land at `inspectah-web/tests/snapshots/fixture_structure_test__*.snap`. See the implementation plan for the full test code.

### Curated test tarballs

Two tarballs checked into `testdata/`:

- `testdata/single-host-e2e.tar.gz` (~200-500KB): One RHEL host scan with packages across multiple repos, config files, services, users/groups, containers. Must include at least one sensitive field to exercise tarball gating.
- `testdata/fleet-e2e.tar.gz` (~300-500KB): 3-host fleet merge with items across consensus/near-consensus/divergent zones, actionable variant items, config file variants.

---

## Revision history

### Round 2 → Round 3

### Round 3 → Round 4 (post-plan-approval alignment)

Aligned spec with the approved implementation plan's final Task 7 approach:

1. **Replaced schemars/JsonSchema approach with fixture-structure snapshots.** Response DTOs are `Serialize`-only; adding `Deserialize` would cascade through ~50 types across 3 crates. Fixture-structure validation via insta snapshots of `serde_json::Value` provides equivalent drift detection with zero cross-crate changes.
2. **Removed `schemars` dependency references.** No `schemars` in workspace deps, no `JsonSchema` derives, no schema export to `e2e/schemas/`. Only `insta` (already a workspace dev-dep).
3. **Removed CI fixture validation layer.** The manifest documents fixture categories for future tooling but Phase 1c does not implement manifest-driven validation. Insta snapshots are the safety net; real-server smoke tests prove Rust-type compatibility.
4. **Phase 1c re-estimated to ~1 day** (down from 2-3 days) since no cross-crate derive work is needed.

### Round 2 → Round 3

Revisions addressing Tang's two remaining blockers plus non-blocking follow-ups from Thorn and Kit:

1. **Schema scope honesty (Tang blocker 1):** Phase 1c now states the full transitive type closure (~50 types across handlers, fleet_handlers, refine types, and core types). Estimate raised from 1-2 days to 2-3 days. The "~20 handler types, defer ~30 refine types" framing is removed — the compiler forces the transitive work when exporting top-level response schemas.
2. **Users/groups schema gap (Tang blocker 2):** `users_groups_decisions: Vec<serde_json::Value>` explicitly carved out of schema-backed validation claims. `users.spec.ts` tests are structural-only. Typing this field is tracked as a separate backlog item.
3. **Negative-path matrix (Thorn follow-up):** Added table mapping each user-visible error branch to the spec file that owns it.
4. **Route-to-surface table (Kit follow-up):** Added table mapping each API route to the UI surface(s) that consume it and what data they display.
5. **Fixture refresh workflow (Kit follow-up):** Added step-by-step workflow for when Rust types change and when new routes are added.

### Round 1 → Round 2

Revisions based on Thorn, Tang, and Kit round-1 review:

1. **Fixture taxonomy:** Added explicit classification (body, harness wrapper, error envelope, excluded) with per-category validation rules and `manifest.json` routing.
2. **Mock state model:** Narrowed mock-tier claims — cross-endpoint state coherence, viewed persistence, and ops/changes consistency moved to real-server tier. Added explicit "Coverage by tier" table.
3. **Route contract accuracy:** Fixed stale generation (tarball, not op). Error payloads described as generic `{ "error": string }` envelopes except tarball 428. Fleet mutation model updated (UI re-fetches GET, ignores POST body).
4. **Real-server run path:** Defined manual run command, default mock-only path, `beforeEach` skip guard, and CI automation acceptance gate.
5. **Phase 1 split:** Decomposed into 4 sub-phases (1a mock infra + POC, 1b remaining fixtures, 1c schema validation, 1d real-server smoke) with honest checkpoints.
