# Phase 4: Refine Web UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the interactive web UI for `inspectah refine` — a React+PatternFly 6 app embedded in the Rust CLI binary via rust-embed. The operator loads a scan tarball, triages packages and config files through include/exclude toggles, reviews informational context sections, and exports a refined tarball. The UI replaces the current placeholder `index.html` with a full triage interface.

**Tech Stack:** React 19, Vite, TypeScript, PatternFly 6 (`@patternfly/react-core`), highlight.js (Dockerfile grammar), rust-embed (embedding)

**Spec:** `docs/specs/proposed/2026-05-16-refine-web-ui-design.md` (2,334 lines, approved after 4 review rounds)

**Rust-side baseline:** Phase 3 shipped 9 API endpoints (`/api/health`, `/api/view`, `/api/op`, `/api/undo`, `/api/redo`, `/api/ops`, `/api/changes`, `/api/tarball`) + `RefineSession` with operation stack, generation counter, and cached view projection. This plan adds 3 new endpoints and the full frontend.

**Structure rationale:** Previous plans (Phases 2-3) were organized by implementation layer because the work was Rust — trait impls, inspectors, service methods. This plan is organized by **feature slice** because the work is React. Each task delivers a vertically integrated feature (types + hook + component + wiring) that can be verified end-to-end before moving to the next.

**Testing approach:** TDD is folded directly into this plan. Every task that produces components or hooks specifies tests-first: write the test, watch it fail, then implement. Vitest + React Testing Library for unit/integration tests (configured in Task 1's scaffold). Playwright e2e smoke tests are set up and run in Task 9. No separate test suite spec is needed — the test harness is part of the scaffold and each task defines its own test coverage.

---

## Task 1: Project scaffold, build pipeline, and CI

**Files:**
- Create: `inspectah-web/ui/package.json`
- Create: `inspectah-web/ui/vite.config.ts`
- Create: `inspectah-web/ui/tsconfig.json`
- Create: `inspectah-web/ui/index.html`
- Create: `inspectah-web/ui/src/main.tsx`
- Create: `inspectah-web/ui/src/App.tsx`
- Create: `inspectah-web/build.rs`
- Modify: `inspectah-web/src/assets.rs` — change rust-embed folder from `static/` to `ui/dist/`, add CSP header
- Delete: `inspectah-web/static/` directory (replaced by `ui/dist/`)
- Modify: `.gitignore` — add `inspectah-web/ui/node_modules/` and `inspectah-web/ui/dist/`
- Modify: `inspectah-web/Cargo.toml` — add `build = "build.rs"` if not present
- Modify: `.github/workflows/rust-ci.yml`

This is all project plumbing. No application logic. One task because a React developer would do this in one sitting and it has one verification criterion: `cargo build -p inspectah-cli` produces a binary that serves a React page.

- [ ] **Step 1: Create the React project**

`inspectah-web/ui/` is the React project root. No monorepo tooling — this is a single Vite app.

Create `package.json` with:
- Core deps: `react`, `react-dom`, `@patternfly/react-core`, `@patternfly/react-icons`, `@patternfly/react-styles`, `highlight.js`
- Dev deps: `@vitejs/plugin-react`, `vite`, `typescript`, `@types/react`, `@types/react-dom`, `vitest`, `@testing-library/react`, `@testing-library/jest-dom`, `jsdom`, `playwright`, `@playwright/test`, `@axe-core/playwright`
- Scripts: `dev` (vite), `build` (tsc && vite build), `preview` (vite preview), `test` (vitest run), `test:watch` (vitest), `test:e2e` (playwright test), `test:e2e:headed` (playwright test --headed)

Write `vite.config.ts` — proxy `/api/*` to `http://localhost:8642` for dev workflow. Output to `dist/`.

Write `tsconfig.json` — strict mode, JSX react-jsx, ES2020 target.

Write `index.html` — standard Vite shell with `<div id="root">`.

Write `main.tsx` — renders `<App />` into `#root`, imports PatternFly CSS.

Write `App.tsx` — PatternFly `<Page>` with placeholder text: "inspectah refine — loading..."

Run `npm install` to generate `package-lock.json`. Commit the lockfile.

- [ ] **Step 2: Write build.rs and update rust-embed**

Per spec SS Embedded UI Build Contract > Build Pipeline. `build.rs` runs `npm ci` + `npm run build` when Node is present. Falls back to pre-built `ui/dist/` when Node is absent. `rerun-if-changed` on `ui/src`, `ui/package.json`, `ui/package-lock.json`, `ui/vite.config.ts`, `ui/tsconfig.json`, `ui/index.html`.

Respect `INSPECTAH_SKIP_UI` env var to skip UI build entirely (for Rust-only development).

Update `assets.rs`: change `#[folder = "static/"]` to `#[folder = "ui/dist/"]`. Add CSP header per spec SS Browser Trust Contract > Content Security Policy:
```
default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline';
img-src 'self' data:; font-src 'self'; connect-src 'self';
frame-ancestors 'none'; base-uri 'none'; form-action 'self'
```

Delete `inspectah-web/static/` directory (replaced by `ui/dist/`).

Update `.gitignore` with `inspectah-web/ui/node_modules/` and `inspectah-web/ui/dist/`.

- [ ] **Step 3: CI integration**

Per spec SS CI Integration. Add `actions/setup-node@v4` to the tier1 job in `.github/workflows/rust-ci.yml` with `node-version: '20'` and `cache: 'npm'` keyed on `inspectah-web/ui/package-lock.json`. Place before the `cargo fmt` step.

No explicit `npm ci` or `npm run build` CI step — `build.rs` handles it during `cargo build`.

Tier2 (Fedora container): set `INSPECTAH_SKIP_UI=1` in tier2's env to avoid pulling Node into a container that only tests Rust FFI.

**Verify:**
- `cd inspectah-web/ui && npm run dev` serves a page at `:5173` with PatternFly styling
- `npm run build` produces `dist/` with `index.html` + JS/CSS assets
- `cargo build -p inspectah-cli` produces a binary that serves the React app at `http://127.0.0.1:8642`
- CSP header present in served responses

---

## Task 2: Rust-side API additions

**Files:**
- Modify: `inspectah-web/src/handlers.rs` — add ContextSection/ContextItem DTOs, `/api/snapshot/sections` handler, `/api/viewed` handlers, extended health response
- Modify: `inspectah-web/src/lib.rs` — add routes for new endpoints
- Modify: `inspectah-refine/src/session.rs` — add `viewed: HashSet<String>`, `mark_viewed()`, `is_viewed()`, `viewed_ids()` methods
- Create or modify: `inspectah-web/tests/` — tests for new endpoints

This task is Rust. It can run in parallel with Task 1 — the two share no files. Together they form the foundation the React UI builds on.

- [ ] **Step 1: Add viewed tracking to RefineSession**

Per spec SS Mutation and Review-State Contract > Viewed tracking. Add `viewed: HashSet<String>` field (non-serialized, excluded from tarball export). Three methods: `mark_viewed(&mut self, id: &str)`, `is_viewed(&self, id: &str) -> bool`, `viewed_ids(&self) -> &HashSet<String>`.

The `id` format is `"section:item_id"` (e.g., `"packages:httpd.x86_64"`).

- [ ] **Step 2: Define ContextSection and ContextItem DTOs**

Per spec SS Context Sections Contract > Wire types. These are presentation-layer DTOs in `handlers.rs`, NOT domain types:

```rust
pub struct ContextSection {
    pub id: String,
    pub display_name: String,
    pub items: Vec<ContextItem>,
}

pub struct ContextItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub searchable_text: String,
}
```

- [ ] **Step 3: Implement normalize_for_context()**

The mapping from `InspectionSnapshot` sections to `Vec<ContextSection>`. Per spec SS Context Sections Contract > Section-to-ContextItem mapping — 9 sections: services, containers, users_groups, network, storage, scheduled_tasks, non_rpm_software, kernel_boot, selinux.

Each section maps per the spec's field-by-field table. Sections that are `None` in the snapshot produce a `ContextSection` with an empty `items` vec.

RPM and config sections are NOT included — they're actionable items in the decision flow, not read-only context.

- [ ] **Step 4: Add /api/snapshot/sections endpoint**

`GET /api/snapshot/sections` — returns `Vec<ContextSection>`. This data is immutable for the session lifetime (computed once, cached). Call `normalize_for_context()` on first request, cache the result on `AppState`.

- [ ] **Step 5: Add /api/viewed endpoints**

`POST /api/viewed` with body `{"id": "packages:httpd.x86_64"}` -> calls `session.mark_viewed()` -> returns `204 No Content`.

`GET /api/viewed` -> returns `{"ids": ["packages:httpd.x86_64", ...]}`.

- [ ] **Step 6: Extend /api/health response**

Add host info from the snapshot: `hostname` (from `meta["hostname"]`), `os_release` (name + version from `OsRelease`), `system_type`, `completeness`. This lets the UI render host info without fetching the full snapshot.

- [ ] **Step 7: Add routes to router**

Wire the three new endpoints into `lib.rs`:
- `.route("/api/snapshot/sections", get(handlers::get_sections))`
- `.route("/api/viewed", get(handlers::get_viewed))`
- `.route("/api/viewed", post(handlers::mark_viewed))`

- [ ] **Step 8: Tests**

Test the new endpoints: sections returns correct structure, viewed POST/GET roundtrip, extended health response includes host info. Test normalize_for_context with a fixture snapshot.

**Verify:** `cargo test --workspace` passes. New endpoints return correct JSON. Viewed tracking persists within a session.

---

## Task 3: TypeScript types and API client

**Files:**
- Create: `inspectah-web/ui/src/api/types.ts` — TypeScript types mirroring Rust DTOs
- Create: `inspectah-web/ui/src/api/client.ts` — typed fetch wrappers
- Create: `inspectah-web/ui/src/api/__tests__/client.test.ts` — client tests with mocked fetch

This task establishes the TypeScript contract and the data layer. The types are derived from the spec, not from Task 2's Rust code — both implement the same spec. The API client can be tested with mocked fetch responses before the backend endpoints are wired.

- [ ] **Step 1: Define TypeScript types**

Mirror the Rust response types. These are the TypeScript side of the API contract:

- `RefinedView` — from `/api/view`: packages (with attention levels/reasons), configs, containerfile string, stats (counts, can_undo, can_redo, generation)
- `ContextSection`, `ContextItem` — from `/api/snapshot/sections`
- `HealthResponse` — from `/api/health` (extended): status + host info (hostname, os_name, os_version, os_id, system_type, schema_version, completeness)
- `RefinementOp` — discriminated union for POST `/api/op` body (ExcludePackage, IncludePackage, ExcludeConfig, IncludeConfig)
- `ChangesSummary` — from `/api/changes`
- `ApiError` — typed error with status code and message

- [ ] **Step 2: Write API client**

Typed fetch wrappers for all 12 endpoints. Each function handles response parsing and error conversion. Binary responses (tarball export) handled with `response.blob()`.

Mutations (`/api/op`, `/api/undo`, `/api/redo`, `/api/tarball`, `/api/viewed`) return typed results or throw typed `ApiError`.

Reads (`/api/view`, `/api/health`, `/api/snapshot/sections`, `/api/ops`, `/api/changes`, `/api/viewed` GET) return typed data.

- [ ] **Step 3: Test API client (TDD — write tests first)**

Write tests BEFORE implementing the client functions in Step 2. Define the expected API contract in test form: correct URL construction, request bodies, response parsing, error handling (4xx, 5xx, network failures), and binary response handling for tarball export. Use mocked `fetch`. Tests should fail initially, then pass as Step 2's implementation is completed.

**Sequencing:** Steps 1 (types) -> 3 (tests) -> 2 (client implementation). Types define the contract, tests encode expectations, implementation makes tests pass.

**Verify:** All client functions have test coverage. Types compile with `tsc --noEmit`. Mocked tests pass with `npm run test`.

---

## Task 4: App shell and layout with live data

**Files:**
- Modify: `inspectah-web/ui/src/App.tsx` — full layout with state management
- Create: `inspectah-web/ui/src/hooks/useView.ts`
- Create: `inspectah-web/ui/src/hooks/useSections.ts`
- Create: `inspectah-web/ui/src/hooks/useHealth.ts`
- Create: `inspectah-web/ui/src/hooks/useMutation.ts`
- Create: `inspectah-web/ui/src/components/Sidebar.tsx`
- Create: `inspectah-web/ui/src/components/StatsBar.tsx`
- Create: `inspectah-web/ui/src/components/ContainerfilePanel.tsx`
- Create: `inspectah-web/ui/src/components/MainContent.tsx`

This is the first task that renders real UI. It delivers the full app shell with live data from hooks — but the main content area is still a placeholder ("Select a section" / section name display). The hooks are built here because the shell needs them, and subsequent tasks consume them.

- [ ] **Step 1: Write React hooks**

`useView()` — fetches `/api/view`, returns `{ data, loading, error, refetch }`. Re-fetches after mutations via a generation-aware invalidation: compare local generation to response generation.

`useSections()` — fetches `/api/snapshot/sections` once on mount, caches for session lifetime (this data is immutable).

`useHealth()` — fetches extended `/api/health` once on mount. Provides host info for sidebar.

`useMutation()` — wraps `POST /api/op` with the mutation queue pattern from spec SS Mutation and Review-State Contract > Client mutation serialization. One mutation in flight at a time. Queue drains sequentially. On error, queue clears and all pending optimistic flips revert. Exposes `{ mutate, undo, redo, isPending }`. After successful mutation, triggers `useView` refetch.

- [ ] **Step 2: Implement App.tsx layout**

Per spec SS Layout. Three-zone layout using PatternFly `Page` and `PageSection`:
- Left: Sidebar (navigation, ~240px)
- Center: Main content area (fills remaining space)
- Right: Containerfile preview panel (collapsible, ~280px open / 28px collapsed)

Top: Stats bar spanning all zones.

App-level state: `activeSection` (string), `containerfilePanelOpen` (boolean, persisted to `localStorage`).

- [ ] **Step 3: Implement Sidebar**

Per spec SS Layout > Sidebar + SS Interaction/Accessibility > Sidebar ARIA Model.

Two visual groups: **Decisions** (Packages, Config Files) and **Context** (Services, Containers, Users & Groups, Network, Storage, Scheduled Tasks, Non-RPM Software, Kernel & Boot, SELinux).

PatternFly `Nav` with `NavGroup` and `NavItem`. Each item shows a `Badge` with item count (from `useView` for decisions, from `useSections` for context). Active section uses `aria-current="page"`. Host info summary at the bottom (from `useHealth`).

- [ ] **Step 4: Implement StatsBar**

Per spec SS Layout > Stats Bar.

Left side: Package counts (included/excluded), config counts, triage progress ("N of M remaining").
Right side: Undo button, Redo button (disabled states from `stats.can_undo`/`stats.can_redo`), Export button (primary, visually separated).

Data from `useView().data.stats`. Undo/redo wired to `useMutation().undo`/`useMutation().redo`.

- [ ] **Step 5: Implement ContainerfilePanel**

Per spec SS Layout > Containerfile Preview Panel.

Open state (~280px): "Containerfile" header + collapse chevron, syntax-highlighted content via `CodeBlock` with highlight.js Dockerfile grammar, footer with line count.

Collapsed state (28px): Vertical tab with rotated "Containerfile" label.

`Ctrl+E` toggles from anywhere. State persists in `localStorage`. Smooth CSS transition (`width 200ms ease`). Auto-collapses below 1280px viewport.

Content from `useView().data.containerfile` — updates after each mutation.

- [ ] **Step 6: Implement MainContent routing**

Stub component that renders the active section name in a `PageSection`. Decision sections (packages, configs) will render `DecisionList` (Task 5). Context sections will render `ContextList` (Task 6). For now, render the section name and a "Not yet implemented" message.

- [ ] **Step 7: Loading states**

Per spec SS States > Loading. Initial page load: PatternFly `Skeleton` components in main content area and Containerfile panel. Stats bar shows placeholder dashes until first `/api/view` response. Sidebar renders immediately (section list is static, counts show "..." until data loads).

- [ ] **Step 8: Tests (TDD — write tests alongside each step)**

Write component tests as each component is built. For hooks: test `useView`, `useSections`, `useHealth` return correct loading/data/error states with mocked fetch. Test `useMutation` queues mutations, triggers refetch, reverts on error. For components: test Sidebar renders all sections with correct counts and active state. Test StatsBar renders stats and undo/redo disabled states. Test ContainerfilePanel renders syntax-highlighted content, toggles collapse state, persists to localStorage. Test loading skeletons render when data is pending.

Create test files: `hooks/__tests__/useView.test.ts`, `hooks/__tests__/useMutation.test.ts`, `components/__tests__/Sidebar.test.tsx`, `components/__tests__/StatsBar.test.tsx`, `components/__tests__/ContainerfilePanel.test.tsx`.

**Verify:** Shell renders with PatternFly styling. Sidebar shows all 11 sections with item counts from live API data. Stats bar shows live counts. Containerfile panel shows syntax-highlighted content that updates when mutations happen (test via curl against the running API). Collapse/expand works with `Ctrl+E` and persists across reload. Loading skeletons show during initial fetch. All component/hook tests pass.

---

## Task 5: Decision sections — packages and config files

**Files:**
- Create: `inspectah-web/ui/src/components/DecisionList.tsx` — shared grid container with attention grouping
- Create: `inspectah-web/ui/src/components/AttentionGroup.tsx` — collapsible group
- Create: `inspectah-web/ui/src/components/DecisionItem.tsx` — single item row with toggle
- Create: `inspectah-web/ui/src/components/PackageDetail.tsx` — expanded package detail
- Create: `inspectah-web/ui/src/components/ConfigDetail.tsx` — expanded config detail
- Create: `inspectah-web/ui/src/components/__tests__/DecisionItem.test.tsx`
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — wire up decision sections

This is the core of the triage interface — the feature that makes the tool useful. After this task, the operator can toggle packages/configs and see the Containerfile update.

- [ ] **Step 1: Implement AttentionGroup**

Per spec SS Layout > Main Content Area > Attention Groups.

PatternFly `ExpandableSection` with count in header. Color-coded left border: NeedsReview (`--pf-t--global--color--status--danger`), Informational (`--pf-t--global--color--status--warning`), Routine (`--pf-t--global--color--status--success`).

NeedsReview always starts expanded. Informational and Routine start collapsed. Group header shows item count.

- [ ] **Step 2: Implement DecisionItem**

Per spec SS Interaction/Accessibility > Item List ARIA Model.

`role="row"` with `role="gridcell"` containers. Contains:
- Toggle switch (`Switch`) for include/exclude — in its own gridcell, reachable via Tab
- Item name/path (title)
- Attention label (`Label`, color-coded by reason)
- Expand chevron for detail view

Keyboard: `Space` or `x` on the row toggles include/exclude. `Enter` expands/collapses detail. Arrow keys move between rows.

Toggle fires optimistic UI update -> `useMutation().mutate()` with the appropriate `RefinementOp` variant -> on success, update from response -> on error, revert and show toast.

NeedsReview items: full card layout with attention reason as `Label`, left border color-coded.
Informational/Routine items: compact row with name + `Switch`, no card chrome.

- [ ] **Step 3: Implement viewed tracking triggers**

Per spec SS Mutation/Review-State Contract > Viewed triggers.

Asymmetric trigger: toggling always marks viewed (intentional action). Expanding a non-toggled item marks viewed (operator inspected it). Expanding an already-toggled item does NOT re-mark (no new information).

On trigger: `POST /api/viewed` with the item's `section:item_id`. Visual indicator on unviewed NeedsReview items (subtle dot or font-weight change) — disappears once viewed.

- [ ] **Step 4: Implement PackageDetail and ConfigDetail**

Expanded detail view rendered below the row when `aria-expanded="true"`.

Package detail shows: NEVRA, state (Added/Modified/BaseImageOnly), repo info, attention reasons with explanations.

Config detail shows: full path, kind (RPM-owned modified, unowned, orphaned), content preview if available.

- [ ] **Step 5: Implement DecisionList container**

Per spec SS Interaction/Accessibility > Item List ARIA Model.

`role="grid"` with `aria-label` describing the section. Contains AttentionGroups as logical divisions. Manages roving tabindex across all rows (focus moves between rows with Arrow keys, wraps at boundaries).

Items grouped by attention level: NeedsReview first, then Informational, then Routine.

- [ ] **Step 6: Wire up packages and config files sections**

Update `MainContent` to render `DecisionList` for packages and config files sections.

Packages: data from `useView().data.packages`. Each package maps to a DecisionItem. Attention tags from the view response (computed server-side).

Config files: same pattern using `useView().data.configs`. Config paths as titles, config kinds as attention context.

- [ ] **Step 7: Error handling for mutations**

Mutation errors: revert optimistic state, show transient `Alert` toast (3-second auto-dismiss via PatternFly `AlertGroup`). Network errors: persistent alert with retry action.

Generation mismatch on any operation: log a warning, auto re-fetch `/api/view` to resync.

- [ ] **Step 8: Tests (TDD — write tests before each component)**

Write tests BEFORE implementing each component. For each step above, the sequence is: write the test for the component's expected behavior, watch it fail, then implement until it passes.

Test coverage per component:
- `AttentionGroup`: renders with correct border color per level, NeedsReview starts expanded, others collapsed, count in header.
- `DecisionItem`: renders toggle in correct state, toggle fires mutation, `Space`/`x` keyboard toggle, `Enter` expands detail, attention label renders with correct color.
- Viewed tracking: toggling marks viewed, expanding non-toggled marks viewed, expanding already-toggled does NOT re-mark.
- `PackageDetail`/`ConfigDetail`: renders correct fields when expanded.
- `DecisionList`: roving tabindex across rows, attention group ordering (NeedsReview → Informational → Routine).
- Error handling: optimistic revert on mutation failure, toast appears and auto-dismisses.

Create test files: `components/__tests__/AttentionGroup.test.tsx`, `components/__tests__/DecisionList.test.tsx`, `components/__tests__/PackageDetail.test.tsx`, `components/__tests__/ConfigDetail.test.tsx`. Extend existing `components/__tests__/DecisionItem.test.tsx`.

**Verify:** Packages and config files render with correct attention grouping. Toggles fire mutations and update Containerfile preview live. Undo/redo from stats bar affects toggle state. Viewed tracking works (POST fires, visual indicator updates). Collapsed groups expand when clicked. Error toasts appear on mutation failure and auto-dismiss. All component tests pass.

---

## Task 6: Context sections

**Files:**
- Create: `inspectah-web/ui/src/components/ContextList.tsx` — read-only section renderer
- Create: `inspectah-web/ui/src/components/ContextItem.tsx` — single informational item
- Modify: `inspectah-web/ui/src/components/MainContent.tsx` — wire up context sections

Simpler than decision sections — no mutations, no attention grouping, no toggles.

- [ ] **Step 1: Implement ContextList and ContextItem**

Renders a `ContextSection` from `/api/snapshot/sections` as a PatternFly `DataList` (without selection). Items are read-only — no toggles, no mutations. Muted left-border (gray).

Each item shows title and subtitle. Items with non-null `detail` are expandable (click to show detail content).

- [ ] **Step 2: Wire up all 9 context sections**

Update `MainContent` to render `ContextList` for context sections using data from `useSections()`.

Sections: Services, Containers, Users & Groups, Network, Storage, Scheduled Tasks, Non-RPM Software, Kernel & Boot, SELinux.

- [ ] **Step 3: Handle empty sections**

If a section has zero items (the snapshot section was `None` or empty), render PatternFly `EmptyState` with appropriate messaging: "No [section name] data in this snapshot."

- [ ] **Step 4: Tests (TDD — write tests before each component)**

Write tests BEFORE implementing ContextList and ContextItem. Test coverage:
- `ContextList`: renders items as read-only DataList (no selection), muted left border.
- `ContextItem`: renders title and subtitle, expandable items show detail on click, non-expandable items (null detail) have no expand control.
- Empty state: renders EmptyState when section has zero items.
- Data stability: `useSections` fetches once on mount and caches (verify no re-fetch on re-render).

Create test files: `components/__tests__/ContextList.test.tsx`, `components/__tests__/ContextItem.test.tsx`.

**Verify:** All 9 context sections render correctly. Items with detail content expand. Empty sections show appropriate state. Data loads once on mount and does not re-fetch. All component tests pass.

---

## Task 7: Search and keyboard navigation

**Files:**
- Create: `inspectah-web/ui/src/components/SectionSearch.tsx`
- Create: `inspectah-web/ui/src/components/GlobalSearch.tsx`
- Create: `inspectah-web/ui/src/components/ShortcutOverlay.tsx`
- Create: `inspectah-web/ui/src/hooks/useKeyboard.ts`

- [ ] **Step 1: Implement section-level search**

Per spec SS Interaction/Accessibility > Search Reveal and Focus Contract > 5a.

`/` opens an inline `SearchInput` above the active section's item list. Real-time filtering by item name, path, and attention reason. Items that don't match are removed from the grid (not hidden with CSS — removed from roving tabindex pool).

When filter matches items inside a collapsed AttentionGroup, the group auto-expands. Track `filterForceExpanded` per group. Clearing the filter restores collapse state.

`Escape` clears filter and blurs input. `ArrowDown` from search input moves focus to first matching item.

- [ ] **Step 2: Implement global search**

Per spec SS Interaction/Accessibility > Search Reveal and Focus Contract > 5b.

`Ctrl+K` opens a global search overlay (modal or popover). Searches across all sections (decisions + context). Results show section name + item title. Selecting a result navigates to that section, expands the containing attention group if needed, scrolls the item into view, and gives it a brief highlight.

- [ ] **Step 3: Implement keyboard navigation**

Per spec SS Interaction/Accessibility > Item List ARIA Model + SS Keyboard shortcuts table.

Vim-style navigation on decision lists: `j`/`ArrowDown` next item, `k`/`ArrowUp` previous item, `g` first item, `G` last item.

Action keys: `Space`/`x` toggle include/exclude, `Enter` expand/collapse detail.

Global shortcuts: `Ctrl+Z` undo, `Ctrl+Shift+Z` redo, `Ctrl+E` toggle Containerfile panel, `Ctrl+Shift+E` export, `?` shortcut overlay, `1`-`9` jump to section by index.

All shortcuts suppressed when focus is in a text input (search fields).

- [ ] **Step 4: Implement shortcut overlay**

`?` opens a PatternFly `Modal` listing all keyboard shortcuts. Organized by category (Navigation, Actions, Global). `Escape` or `?` again closes.

- [ ] **Step 5: Tests (TDD — write tests before each component)**

Write tests BEFORE implementing each search/keyboard component. Test coverage:
- `SectionSearch`: `/` opens search input, typing filters items, `Escape` clears and blurs, `ArrowDown` moves focus to first match, collapsed groups auto-expand when filter matches items inside.
- `GlobalSearch`: `Ctrl+K` opens overlay, search returns cross-section results, selecting a result navigates to correct section.
- `useKeyboard`: vim keys (`j`/`k`/`g`/`G`) move focus correctly, `Space`/`x` toggle, `Enter` expand/collapse, global shortcuts fire correct actions, all shortcuts suppressed when focus is in a text input.
- `ShortcutOverlay`: `?` opens modal, lists all shortcuts by category, `Escape` or `?` again closes.

Create test files: `components/__tests__/SectionSearch.test.tsx`, `components/__tests__/GlobalSearch.test.tsx`, `hooks/__tests__/useKeyboard.test.ts`, `components/__tests__/ShortcutOverlay.test.tsx`.

**Verify:** Section search filters items in real time, auto-expands collapsed groups. Global search navigates cross-section. Vim keys navigate decision items. All global shortcuts work. Shortcuts suppressed in text inputs. All component/hook tests pass.

---

## Task 8: Export workflow and responsive layout

**Files:**
- Create: `inspectah-web/ui/src/components/ExportDialog.tsx`
- Modify: various components for responsive behavior
- Modify: `inspectah-web/ui/src/App.tsx` — responsive breakpoints

Two features that are each small enough that splitting them into separate tasks would add overhead without adding clarity.

- [ ] **Step 1: Implement export workflow**

Per spec SS Interaction Model > Export Workflow.

"Export Tarball" button (in stats bar) or `Ctrl+Shift+E` opens a PatternFly `Modal`:
- Summary: "X packages excluded, Y configs excluded"
- Current generation number displayed
- "Export" primary button, "Cancel" secondary

`POST /api/tarball` with `{generation: currentGeneration}`. On success (binary response): trigger browser download of `inspectah-refine-output.tar.gz`. Show success toast.

On 409 (generation mismatch): show stale-state alert, auto re-fetch `/api/view`, close modal.

- [ ] **Step 2: Implement responsive layout**

Per spec SS Interaction/Accessibility > Responsive Contract.

| Viewport | Behavior |
|----------|----------|
| >= 1280px | Full three-zone layout |
| < 1280px | Containerfile panel auto-collapses to 28px vertical tab |
| < 1024px | Sidebar hidden, hamburger button in masthead, sidebar renders as overlay |

Sidebar overlay (< 1024px): fixed-position overlay with semi-transparent backdrop. Focus trap while open. `Escape` closes. Clicking a section closes overlay and navigates. Hamburger button has `aria-expanded` and `aria-controls`.

- [ ] **Step 3: Empty and completion states**

Per spec SS States. Zero items in a decision section: `EmptyState` component. Zero filter results: "No items match your search" with clear-filter action. All triaged: completion message (use "triaged" language per spec, not "reviewed").

- [ ] **Step 4: Tests (TDD — write tests before each component)**

Write tests BEFORE implementing each component. Test coverage:
- `ExportDialog`: modal opens on button click and `Ctrl+Shift+E`, shows correct exclusion summary, "Export" triggers `/api/tarball` POST with current generation, success triggers browser download, 409 response shows stale-state alert and re-fetches view.
- Responsive layout: Containerfile panel auto-collapses below 1280px, sidebar hides below 1024px, hamburger button shows with correct `aria-expanded`/`aria-controls`, sidebar overlay has focus trap and `Escape` dismissal.
- Empty/completion states: EmptyState renders for zero-item sections, "No items match" for empty filter results with clear action, completion message uses "triaged" language.

Create test files: `components/__tests__/ExportDialog.test.tsx`. Responsive and empty-state tests can live in existing component test files or a dedicated `components/__tests__/ResponsiveLayout.test.tsx`.

**Verify:** Export downloads a valid tarball. 409 handling works (make a change, export with stale generation). Responsive breakpoints work at 1280px and 1024px. Sidebar overlay works on narrow viewports with focus trap and Escape dismissal. All component tests pass.

---

## Task 9: Integration verification and e2e tests

- [ ] **Step 1: Playwright e2e setup**

Add Playwright as a dev dependency in `inspectah-web/ui/package.json`. Create `inspectah-web/ui/playwright.config.ts` — base URL `http://127.0.0.1:8642`, single Chromium project, screenshot on failure. Add scripts: `test:e2e` (playwright test), `test:e2e:headed` (playwright test --headed).

Create `inspectah-web/ui/e2e/` directory for e2e test files.

- [ ] **Step 2: Playwright e2e smoke tests**

Write smoke tests that exercise the critical triage workflow against a running `inspectah refine` server:

- `e2e/smoke.spec.ts`: Page loads, sidebar shows all sections, stats bar renders counts.
- `e2e/triage.spec.ts`: Toggle a package -> Containerfile preview updates -> undo -> reverts -> redo -> re-applies. Export tarball downloads successfully.
- `e2e/keyboard.spec.ts`: `j`/`k` navigate items, `Space` toggles, `/` opens section search, `Ctrl+K` opens global search, `?` opens shortcut overlay, `Escape` closes overlays.
- `e2e/responsive.spec.ts`: At 1024px viewport, sidebar collapses to hamburger overlay. At 1280px, Containerfile panel auto-collapses.
- `e2e/a11y.spec.ts`: Run axe-core accessibility audit on the main page (install `@axe-core/playwright` as dev dep). Fail on any critical or serious violations.

Tests assume the server is started externally (`inspectah refine <tarball>`). Document the test data tarball requirement in a `e2e/README.md`.

- [ ] **Step 3: CI integration for e2e**

Add a Playwright step to the tier1 CI job: install Playwright browsers (`npx playwright install --with-deps chromium`), build the binary, run `inspectah refine <test-tarball> &` in background, wait for health check, run `npm run test:e2e`, kill server. Runs after the existing Vitest unit test step.

- [ ] **Step 4: Full build verification**

`cargo build -p inspectah-cli` succeeds. Binary size is reasonable. Run `inspectah refine <tarball>` — browser opens, full UI loads.

- [ ] **Step 5: Functional walkthrough**

Manual verification checklist:
1. Sidebar shows all sections with correct counts
2. Package list shows attention groups (NeedsReview expanded, others collapsed)
3. Toggle a package -> Containerfile preview updates -> stats update
4. Undo -> toggle reverts -> Containerfile reverts
5. Redo -> toggle re-applies
6. Expand a package -> detail view shows NEVRA and attention reasons
7. Switch to Config Files -> same pattern works
8. Switch to a Context section -> read-only DataList renders
9. Section search (`/`) filters items, auto-expands collapsed groups
10. Global search (`Ctrl+K`) finds items across sections
11. Keyboard nav: `j`/`k` move through items, `Space` toggles, `Enter` expands
12. Export -> confirmation modal -> download tarball
13. Containerfile panel: `Ctrl+E` toggles, auto-collapses below 1280px
14. Responsive: sidebar overlay below 1024px, hamburger button works
15. Host info displays in sidebar

- [ ] **Step 6: CI verification**

Push to `rust` branch. Tier1 passes (Node setup + full build + Vitest unit tests + Playwright e2e). Tier2 passes (skips UI build).

- [ ] **Step 7: Accessibility spot-check**

Tab through the interface — focus order makes sense. Screen reader announces section names, item states, toggle results. `role="grid"` items navigable with arrow keys. Escape closes overlays. axe-core e2e test (Step 2) catches WCAG violations automatically.

---

## Summary

| Task | What it builds | Domain | Depends on |
|------|---------------|--------|------------|
| 1 | React+Vite+PF6 scaffold, build.rs, CI | TS + Rust + CI | -- |
| 2 | /api/snapshot/sections, /api/viewed, extended health, viewed tracking | Rust | -- |
| 3 | API client, TypeScript types | TypeScript | 1 (project exists) |
| 4 | App shell layout — sidebar, stats bar, Containerfile panel, hooks | TypeScript/React | 1, 2, 3 |
| 5 | Decision items — packages + config files (grid, attention groups, toggles) | TypeScript/React | 4 |
| 6 | Context sections — 9 read-only informational sections | TypeScript/React | 4 |
| 7 | Search, keyboard navigation, shortcut overlay | TypeScript/React | 5 |
| 8 | Export workflow, responsive layout, empty/completion states | TypeScript/React | 5, 6 |
| 9 | Integration verification — build, functional, CI, a11y | Manual + CI | all |

**Total:** 9 tasks. Tasks 1-2 are foundation (parallelizable). Task 3 is the data layer. Tasks 4-8 are the UI, built feature-slice by feature-slice. Task 9 is verification.

**Parallelism:** Tasks 1 and 2 share no files and can run in parallel. Task 3 can start as soon as Task 1 is done (it needs the project to exist but not the Rust endpoints — types come from the spec). Tasks 5 and 6 can run in parallel after Task 4 (decision sections and context sections are independent features). Tasks 7 and 8 can run in parallel once Task 5 is done (search needs the decision list, export needs both sections).

**Testing:** TDD throughout. Vitest + React Testing Library for component/hook tests (every feature task). Playwright e2e smoke tests in Task 9. Test harness configured in Task 1's scaffold.
